use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::domain::provider_account_usage::{
    build_account_usage_url, config_from_extension_values, http_status_result,
    parse_account_usage_response, redact_secret, ProviderAccountUsageAdapterKind,
    ProviderAccountUsageConfigState, ProviderAccountUsageResult, ProviderAccountUsageStatus,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_account_usage_fetch(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<ProviderAccountUsageResult, String> {
    if provider_id <= 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid provider_id={provider_id}"
        ));
    }

    let db = ensure_db_ready(app, db_state.inner()).await?;
    let provider = blocking::run("provider_account_usage_fetch_load_provider", {
        let db = db.clone();
        move || {
            let conn = db.open_connection()?;
            crate::providers::get_by_id(&conn, provider_id)
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!(
            "aio-coding-hub-provider-account-usage/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|err| format!("SYSTEM_ERROR: failed to build HTTP client: {err}"))?;

    let fetched_at = crate::shared::time::now_unix_seconds();
    let mut request = client.get(&url).bearer_auth(&api_key);
    if let Some(new_api_user_id) = config.new_api_user_id.as_deref() {
        request = request.header("New-Api-User", new_api_user_id);
    }

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

    let body_text = match response.text().await {
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
