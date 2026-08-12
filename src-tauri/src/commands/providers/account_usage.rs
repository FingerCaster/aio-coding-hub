use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::domain::provider_account_usage::{
    apply_account_usage_cache_busting, build_account_usage_url, config_from_extension_values,
    custom_config_from_draft, fetch_newapi_account_usage, fetch_newapi_user_account_usage,
    http_status_result, parse_account_usage_response, redact_secret, NewapiQueryMode,
    ProviderAccountUsageAdapterKind, ProviderAccountUsageConfigState,
    ProviderAccountUsageCustomScriptDraft, ProviderAccountUsageResult, ProviderAccountUsageStatus,
    SUB2API_RESPONSE_BODY_LIMIT,
};
use crate::domain::provider_account_usage_script::execute_custom_account_usage;
use tauri::Manager;

fn account_usage_provider_snapshot_matches(
    provider: &crate::providers::ProviderAccountUsageFetchContext,
    credential_context: &crate::providers::ProviderAccountUsageCredentialContext,
) -> bool {
    provider.provider_uuid == credential_context.provider_uuid
        && provider.base_urls == credential_context.base_urls
        && provider.auth_mode == credential_context.auth_mode
        && provider.source_provider_id == credential_context.source_provider_id
        && provider.source_provider_uuid == credential_context.source_provider_uuid
}

async fn load_account_usage_runtime_target(
    db: &crate::db::Db,
    provider_id: i64,
) -> Result<Option<crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget>, String>
{
    let db = db.clone();
    let context = blocking::run("provider_account_usage_load_runtime_target", move || {
        let conn = db.open_connection()?;
        crate::providers::get_account_usage_fetch_context(&conn, provider_id)
    })
    .await
    .map_err(Into::<String>::into)?;
    Ok(
        crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget::from_fetch_context(
            provider_id,
            &context,
        ),
    )
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_snapshot(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    runtime_state: tauri::State<
        '_,
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >,
    provider_id: i64,
) -> Result<ProviderAccountUsageResult, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }

    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let Some(target) = load_account_usage_runtime_target(&db, provider_id).await? else {
        runtime_state.invalidate(provider_id).await?;
        return Ok(ProviderAccountUsageResult::local_status(
            None,
            ProviderAccountUsageStatus::Unsupported,
            "未配置账户用量适配器",
        ));
    };
    runtime_state.acquire_desktop_lease(&app, target).await?;
    Ok(runtime_state
        .snapshot(provider_id)
        .await
        .map(|snapshot| snapshot.result)
        .unwrap_or_else(|| {
            ProviderAccountUsageResult::local_status(
                None,
                ProviderAccountUsageStatus::QueryFailed,
                "账户用量尚未获取",
            )
        }))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_refresh(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    runtime_state: tauri::State<
        '_,
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >,
    provider_id: i64,
) -> Result<ProviderAccountUsageResult, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let Some(target) = load_account_usage_runtime_target(&db, provider_id).await? else {
        runtime_state.invalidate(provider_id).await?;
        return fetch_account_usage_uncached(app, provider_id).await;
    };
    runtime_state.refresh(app, target).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_desktop_lease_acquire(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    runtime_state: tauri::State<
        '_,
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >,
    provider_id: i64,
) -> Result<bool, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let Some(target) = load_account_usage_runtime_target(&db, provider_id).await? else {
        runtime_state.invalidate(provider_id).await?;
        return Ok(false);
    };
    runtime_state.acquire_desktop_lease(&app, target).await?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_desktop_lease_heartbeat(
    runtime_state: tauri::State<
        '_,
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >,
    provider_id: i64,
) -> Result<bool, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    Ok(runtime_state.heartbeat_desktop_lease(provider_id).await)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_desktop_lease_release(
    runtime_state: tauri::State<
        '_,
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >,
    provider_id: i64,
) -> Result<(), String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    runtime_state.release_desktop_lease(provider_id).await;
    Ok(())
}

pub(crate) async fn fetch_account_usage_uncached<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    provider_id: i64,
) -> Result<ProviderAccountUsageResult, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    let db_state = app
        .try_state::<DbInitState>()
        .ok_or_else(|| "SYSTEM_ERROR: database state is unavailable".to_string())?;
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let provider = blocking::run("provider_account_usage_fetch_load_provider", {
        let db = db.clone();
        move || {
            let conn = db.open_connection()?;
            crate::providers::get_account_usage_fetch_context(&conn, provider_id)
        }
    })
    .await
    .map_err(Into::<String>::into)?;

    let config = match config_from_extension_values(&provider.extension_values) {
        ProviderAccountUsageConfigState::Configured(config) => config,
        ProviderAccountUsageConfigState::Missing | ProviderAccountUsageConfigState::Disabled => {
            return Ok(ProviderAccountUsageResult::local_status(
                None,
                ProviderAccountUsageStatus::Unsupported,
                "未配置账户用量适配器",
            ));
        }
        ProviderAccountUsageConfigState::Invalid(message) => {
            return Ok(ProviderAccountUsageResult::local_status(
                None,
                ProviderAccountUsageStatus::ConfigurationRequired,
                message,
            ));
        }
    };

    if provider.auth_mode != "api_key" || provider.source_provider_id.is_some() {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(config.adapter_kind),
            ProviderAccountUsageStatus::Unsupported,
            "账户用量查询仅支持直接 API Key 供应商",
        ));
    }

    let Some(base_url) = provider
        .base_urls
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
    else {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(config.adapter_kind),
            ProviderAccountUsageStatus::ConfigurationRequired,
            "供应商 Base URL 为空",
        ));
    };

    let custom_config =
        if config.adapter_kind == ProviderAccountUsageAdapterKind::Custom {
            let Some(custom) = config.custom.as_ref() else {
                return Ok(ProviderAccountUsageResult::local_status(
                    Some(ProviderAccountUsageAdapterKind::Custom),
                    ProviderAccountUsageStatus::ConfigurationRequired,
                    "自定义账户用量脚本配置无效",
                ));
            };
            if !custom.enabled {
                return Ok(ProviderAccountUsageResult::local_status(
                    Some(ProviderAccountUsageAdapterKind::Custom),
                    ProviderAccountUsageStatus::ConfigurationRequired,
                    "需确认启用自定义账户用量脚本",
                ));
            }
            let permission_scope =
                match crate::domain::provider_account_usage::custom_account_usage_permission_scope(
                    &provider.provider_uuid,
                    &provider.auth_mode,
                    provider.source_provider_uuid.as_deref(),
                    base_url,
                ) {
                    Ok(scope) => scope,
                    Err(message) => {
                        return Ok(ProviderAccountUsageResult::local_status(
                            Some(ProviderAccountUsageAdapterKind::Custom),
                            ProviderAccountUsageStatus::ConfigurationRequired,
                            message,
                        ));
                    }
                };
            let expected_fingerprint = crate::domain::provider_account_usage::
            custom_account_usage_authorization_fingerprint(custom, &permission_scope);
            if custom.permission_fingerprint.as_deref() != Some(expected_fingerprint.as_str()) {
                return Ok(ProviderAccountUsageResult::local_status(
                    Some(ProviderAccountUsageAdapterKind::Custom),
                    ProviderAccountUsageStatus::ConfigurationRequired,
                    "供应商身份或自定义账户用量权限已变更，需重新确认脚本",
                ));
            }
            Some(custom)
        } else {
            None
        };

    let fetched_at = crate::shared::time::now_unix_seconds();
    if config.adapter_kind == ProviderAccountUsageAdapterKind::Newapi
        && config.new_api_query_mode == NewapiQueryMode::Account
    {
        let credentials = blocking::run("provider_account_usage_fetch_load_account_credentials", {
            let db = db.clone();
            move || {
                let conn = db.open_connection()?;
                crate::domain::provider_account_usage::load_account_usage_credentials(
                    &conn,
                    provider_id,
                )
            }
        })
        .await
        .map_err(Into::<String>::into)?;
        let (Some(user_id), Some(access_token)) = (
            credentials.new_api_user_id.as_deref(),
            credentials.new_api_access_token.as_deref(),
        ) else {
            return Ok(ProviderAccountUsageResult::local_status(
                Some(config.adapter_kind),
                ProviderAccountUsageStatus::ConfigurationRequired,
                "需配置账户凭据",
            ));
        };
        return Ok(fetch_newapi_user_account_usage(
            base_url,
            access_token,
            user_id,
            fetched_at,
            fetched_at,
        )
        .await);
    }

    if let Some(custom) = custom_config {
        let credential_context = blocking::run(
            "provider_account_usage_fetch_load_custom_credential_context",
            {
                let db = db.clone();
                move || {
                    let conn = db.open_connection()?;
                    crate::providers::get_account_usage_credential_context(&conn, provider_id)
                }
            },
        )
        .await
        .map_err(Into::<String>::into)?;
        if !account_usage_provider_snapshot_matches(&provider, &credential_context)
            || provider.extension_values != credential_context.extension_values
        {
            return Ok(ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Custom),
                ProviderAccountUsageStatus::QueryFailed,
                "供应商配置在账户用量查询期间发生变化，请重试",
            ));
        }
        let Some(current_base_url) = credential_context
            .base_urls
            .iter()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
        else {
            return Ok(ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Custom),
                ProviderAccountUsageStatus::ConfigurationRequired,
                "供应商 Base URL 为空",
            ));
        };
        let api_key = credential_context.api_key_plaintext.trim();
        if api_key.is_empty() {
            return Ok(ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Custom),
                ProviderAccountUsageStatus::ConfigurationRequired,
                "供应商 API Key 为空",
            ));
        }
        return Ok(
            execute_custom_account_usage(custom, current_base_url, api_key, fetched_at).await,
        );
    }

    let api_key = blocking::run("provider_account_usage_fetch_load_api_key", {
        let db = db.clone();
        move || crate::providers::get_api_key_plaintext(&db, provider_id)
    })
    .await
    .map_err(Into::<String>::into)?
    .trim()
    .to_string();
    if api_key.is_empty() {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(config.adapter_kind),
            ProviderAccountUsageStatus::ConfigurationRequired,
            "供应商 API Key 为空",
        ));
    }

    if config.adapter_kind == ProviderAccountUsageAdapterKind::Newapi {
        return Ok(fetch_newapi_account_usage(base_url, &api_key, fetched_at, fetched_at).await);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!(
            "aio-coding-hub-provider-account-usage/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|err| format!("SYSTEM_ERROR: failed to build HTTP client: {err}"))?;

    let url = match build_account_usage_url(base_url, config.adapter_kind) {
        Ok(url) => url,
        Err(message) => {
            return Ok(ProviderAccountUsageResult::local_status(
                Some(config.adapter_kind),
                ProviderAccountUsageStatus::ConfigurationRequired,
                message,
            ));
        }
    };
    let request = build_sub2api_account_usage_request(&client, &url, &api_key);

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            let mut result = ProviderAccountUsageResult::fetched(
                config.adapter_kind,
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
            );
            result.message = Some(redact_secret(&format!("账户用量查询失败: {err}"), &api_key));
            if result
                .message
                .as_deref()
                .is_some_and(|message| message.len() > 160)
            {
                result.message = Some("账户用量查询失败".to_string());
            }
            return Ok(result);
        }
    };

    let status = response.status();
    if !status.is_success() {
        return Ok(http_status_result(config.adapter_kind, status, fetched_at));
    }

    let body_text = match crate::shared::http_body::read_text_with_limit(
        response,
        SUB2API_RESPONSE_BODY_LIMIT,
        "sub2api account usage",
    )
    .await
    {
        Ok(body) => body,
        Err(err) => {
            let message = redact_secret(&format!("账户用量响应读取失败: {err}"), &api_key);
            return Ok(query_failed_result(
                config.adapter_kind,
                fetched_at,
                message,
            ));
        }
    };

    let body: serde_json::Value = match serde_json::from_str(&body_text) {
        Ok(body) => body,
        Err(_) => {
            return Ok(query_failed_result(
                config.adapter_kind,
                fetched_at,
                "账户用量接口返回了无效 JSON".to_string(),
            ));
        }
    };

    Ok(parse_account_usage_response(
        config.adapter_kind,
        &body,
        fetched_at,
        fetched_at,
    ))
}

fn build_sub2api_account_usage_request(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    apply_account_usage_cache_busting(client.get(url).bearer_auth(api_key))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_test_custom_script(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    draft: ProviderAccountUsageCustomScriptDraft,
) -> Result<ProviderAccountUsageResult, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }
    let custom = custom_config_from_draft(draft)
        .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))?;
    let confirmation_app = app.clone();
    let db = ensure_db_ready(app, db_state.inner()).await?;
    let provider = blocking::run("provider_account_usage_test_custom_script_load_provider", {
        let db = db.clone();
        move || {
            let conn = db.open_connection()?;
            crate::providers::get_account_usage_fetch_context(&conn, provider_id)
        }
    })
    .await
    .map_err(Into::<String>::into)?;

    if provider.auth_mode != "api_key" || provider.source_provider_id.is_some() {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(ProviderAccountUsageAdapterKind::Custom),
            ProviderAccountUsageStatus::Unsupported,
            "账户用量查询仅支持直接 API Key 供应商",
        ));
    }
    let Some(base_url) = provider
        .base_urls
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
    else {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(ProviderAccountUsageAdapterKind::Custom),
            ProviderAccountUsageStatus::ConfigurationRequired,
            "供应商 Base URL 为空",
        ));
    };
    let permission_scope =
        crate::domain::provider_account_usage::custom_account_usage_permission_scope(
            &provider.provider_uuid,
            &provider.auth_mode,
            provider.source_provider_uuid.as_deref(),
            base_url,
        )
        .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))?;
    let origins = crate::domain::provider_account_usage::custom_account_usage_network_origins(
        base_url,
        &custom.allowed_origins,
    )
    .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))?;
    let permission_fingerprint =
        crate::domain::provider_account_usage::custom_account_usage_authorization_fingerprint(
            &custom,
            &permission_scope,
        );
    let confirmed = crate::app::provider_account_usage_confirmation::
        confirm_custom_account_usage_network_access(
            &confirmation_app,
            crate::app::provider_account_usage_confirmation::
                CustomAccountUsageConfirmationKind::Test,
            &origins,
            &permission_fingerprint,
        )
        .await?;
    if !confirmed {
        return Err(
            "SEC_CONFIRM_REQUIRED: custom account usage test permission was not confirmed"
                .to_string(),
        );
    }
    let credential_context = blocking::run(
        "provider_account_usage_test_custom_script_load_credential_context",
        move || {
            let conn = db.open_connection()?;
            crate::providers::get_account_usage_credential_context(&conn, provider_id)
        },
    )
    .await
    .map_err(Into::<String>::into)?;
    if !account_usage_provider_snapshot_matches(&provider, &credential_context) {
        return Err(
            "SEC_CONFIRM_STALE: provider configuration changed during custom account usage confirmation"
                .to_string(),
        );
    }
    let Some(current_base_url) = credential_context
        .base_urls
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
    else {
        return Err("SEC_CONFIRM_STALE: provider Base URL is no longer available".to_string());
    };
    let api_key = credential_context.api_key_plaintext.trim();
    if api_key.is_empty() {
        return Ok(ProviderAccountUsageResult::local_status(
            Some(ProviderAccountUsageAdapterKind::Custom),
            ProviderAccountUsageStatus::ConfigurationRequired,
            "供应商 API Key 为空",
        ));
    }
    let fetched_at = crate::shared::time::now_unix_seconds();
    Ok(execute_custom_account_usage(&custom, current_base_url, api_key, fetched_at).await)
}

fn query_failed_result(
    adapter_kind: ProviderAccountUsageAdapterKind,
    fetched_at: i64,
    message: String,
) -> ProviderAccountUsageResult {
    let mut result = ProviderAccountUsageResult::fetched(
        adapter_kind,
        ProviderAccountUsageStatus::QueryFailed,
        fetched_at,
    );
    result.message = Some(message);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        ProviderAccountUsageCredentialContext, ProviderAccountUsageFetchContext,
    };

    fn fetch_context(provider_uuid: &str) -> ProviderAccountUsageFetchContext {
        ProviderAccountUsageFetchContext {
            provider_uuid: provider_uuid.to_string(),
            base_urls: vec!["https://api.example.test/v1".to_string()],
            auth_mode: "api_key".to_string(),
            source_provider_id: None,
            source_provider_uuid: None,
            extension_values: Vec::new(),
        }
    }

    fn credential_context(provider_uuid: &str) -> ProviderAccountUsageCredentialContext {
        ProviderAccountUsageCredentialContext {
            provider_uuid: provider_uuid.to_string(),
            base_urls: vec!["https://api.example.test/v1".to_string()],
            auth_mode: "api_key".to_string(),
            source_provider_id: None,
            source_provider_uuid: None,
            extension_values: Vec::new(),
            api_key_plaintext: "synthetic-key".to_string(),
        }
    }

    #[test]
    fn provider_snapshot_rejects_reused_id_with_different_uuid() {
        let before = fetch_context("11111111-1111-4111-8111-111111111111");
        let after = credential_context("22222222-2222-4222-8222-222222222222");

        assert!(!account_usage_provider_snapshot_matches(&before, &after));
    }

    #[test]
    fn provider_snapshot_accepts_unchanged_identity_and_transport() {
        let provider_uuid = "11111111-1111-4111-8111-111111111111";
        let before = fetch_context(provider_uuid);
        let after = credential_context(provider_uuid);

        assert!(account_usage_provider_snapshot_matches(&before, &after));
    }

    #[test]
    fn sub2api_account_usage_request_bypasses_http_caches() {
        let request = build_sub2api_account_usage_request(
            &reqwest::Client::new(),
            "https://usage.example.test/v1/usage",
            "synthetic-key",
        )
        .build()
        .expect("build sub2api account-usage request");

        assert_eq!(
            request.headers().get(reqwest::header::CACHE_CONTROL),
            Some(&reqwest::header::HeaderValue::from_static(
                "no-cache, no-store"
            ))
        );
        assert_eq!(
            request.headers().get(reqwest::header::PRAGMA),
            Some(&reqwest::header::HeaderValue::from_static("no-cache"))
        );
    }
}
