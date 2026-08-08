use crate::app_state::{ensure_db_ready, DbInitState};
use crate::gateway_control::{
    app_gateway_clear_cli_route_runtime_state, app_gateway_reconcile_account_usage_targets,
};
use crate::{blocking, providers};
use tauri::Manager;

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderUpsertInput {
    pub provider_id: Option<i64>,
    pub cli_key: String,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: providers::ProviderBaseUrlMode,
    pub auth_mode: Option<providers::ProviderAuthMode>,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub cost_multiplier: f64,
    pub priority: Option<i64>,
    pub claude_models: Option<providers::ClaudeModels>,
    pub availability_test_model: Option<String>,
    #[serde(rename = "limit5hUsd", alias = "limit5HUsd")]
    #[specta(rename = "limit5hUsd")]
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub daily_reset_mode: Option<providers::DailyResetMode>,
    pub daily_reset_time: Option<String>,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub tags: Option<Vec<String>>,
    pub note: Option<String>,
    pub source_provider_id: Option<i64>,
    pub bridge_type: Option<String>,
    pub stream_idle_timeout_seconds: Option<u32>,
    pub extension_values: Option<Vec<providers::ProviderExtensionValuesInput>>,
    #[serde(default)]
    pub account_usage_credentials:
        Option<crate::domain::provider_account_usage::ProviderAccountUsageCredentialsPatch>,
    pub upstream_retry_policy_override: Option<crate::settings::UpstreamRetryPolicy>,
    #[serde(default)]
    pub upstream_retry_policy_override_specified: bool,
    pub model_routing_policy_override: Option<crate::settings::ModelRoutingPolicy>,
    #[serde(default)]
    pub model_routing_policy_override_specified: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ProviderRuntimeResetDecision {
    clear_route_runtime_state: bool,
    clear_account_usage_runtime_state: bool,
}

fn normalize_provider_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn build_duplicated_provider_name(
    source_name: &str,
    existing_providers: &[providers::ProviderSummary],
) -> String {
    let base_name = format!("{} 副本", source_name.trim());
    let used_names: std::collections::HashSet<String> = existing_providers
        .iter()
        .map(|provider| normalize_provider_name(&provider.name))
        .collect();

    if !used_names.contains(&normalize_provider_name(&base_name)) {
        return base_name;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base_name} {index}");
        if !used_names.contains(&normalize_provider_name(&candidate)) {
            return candidate;
        }
        index += 1;
    }
}

fn submitted_api_key_changed(
    previous_api_key: Option<&str>,
    submitted_api_key: Option<&str>,
) -> bool {
    let Some(submitted) = submitted_api_key
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    else {
        return false;
    };

    previous_api_key
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        != Some(submitted)
}

fn provider_runtime_reset_decision(
    previous: Option<&providers::ProviderSummary>,
    previous_api_key: Option<&str>,
    next: &providers::ProviderSummary,
    submitted_api_key: Option<&str>,
    account_usage_secret_changed: bool,
) -> ProviderRuntimeResetDecision {
    let Some(previous) = previous else {
        return ProviderRuntimeResetDecision {
            clear_route_runtime_state: next.enabled,
            clear_account_usage_runtime_state: false,
        };
    };

    let sensitive_config_changed = previous.base_urls != next.base_urls
        || previous.base_url_mode != next.base_url_mode
        || previous.enabled != next.enabled
        || previous.auth_mode != next.auth_mode
        || submitted_api_key_changed(previous_api_key, submitted_api_key)
        || previous.source_provider_id != next.source_provider_id
        || previous.bridge_type != next.bridge_type
        || previous.upstream_retry_policy_override != next.upstream_retry_policy_override
        || previous.model_routing_policy_override != next.model_routing_policy_override;

    ProviderRuntimeResetDecision {
        clear_route_runtime_state: sensitive_config_changed
            || account_usage_route_semantics(previous) != account_usage_route_semantics(next),
        clear_account_usage_runtime_state: previous.enabled != next.enabled
            || previous.base_urls != next.base_urls
            || previous.auth_mode != next.auth_mode
            || previous.source_provider_id != next.source_provider_id
            || submitted_api_key_changed(previous_api_key, submitted_api_key)
            || previous.newapi_account_user_id != next.newapi_account_user_id
            || previous.newapi_account_access_token_configured
                != next.newapi_account_access_token_configured
            || account_usage_secret_changed
            || account_usage_query_semantics(previous) != account_usage_query_semantics(next),
    }
}

fn account_usage_route_semantics(
    provider: &providers::ProviderSummary,
) -> Option<serde_json::Value> {
    let mut values = provider
        .extension_values
        .iter()
        .find(|value| {
            value.plugin_id == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
                && value.namespace == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
        })?
        .values
        .clone();
    if let Some(object) = values.as_object_mut() {
        object.remove("timedRefreshEnabled");
    }
    Some(values)
}

fn account_usage_query_semantics(
    provider: &providers::ProviderSummary,
) -> Option<serde_json::Value> {
    let mut values = provider
        .extension_values
        .iter()
        .find(|value| {
            value.plugin_id == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
                && value.namespace == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
        })?
        .values
        .clone();
    if let Some(object) = values.as_object_mut() {
        object.remove("timedRefreshEnabled");
        object.remove("routeGateEnabled");
    }
    Some(values)
}

async fn invalidate_provider_account_usage_runtime(app: &tauri::AppHandle, provider_id: i64) {
    let Some(runtime) = app.try_state::<
        crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
    >() else {
        return;
    };
    if let Err(error) = runtime.invalidate(provider_id).await {
        tracing::warn!(
            provider_id,
            error = %error,
            "failed to invalidate provider account usage runtime"
        );
    }
}

async fn reconcile_provider_account_usage_gateway_targets(app: &tauri::AppHandle) {
    if let Err(error) = app_gateway_reconcile_account_usage_targets(app).await {
        tracing::warn!(
            error = %error,
            "failed to reconcile account usage targets after provider mutation"
        );
    }
}

fn custom_account_usage_permission_request(
    values: Option<&[providers::ProviderExtensionValuesInput]>,
    permission_scope: &crate::domain::provider_account_usage::ProviderAccountUsageCustomPermissionScope,
) -> Result<
    Option<crate::domain::provider_account_usage::ProviderAccountUsageCustomPermissionRequest>,
    String,
> {
    crate::domain::provider_account_usage::custom_account_usage_permission_request(
        values,
        permission_scope,
    )
    .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))
}

fn custom_account_usage_enable_requested(
    values: Option<&[providers::ProviderExtensionValuesInput]>,
) -> bool {
    values
        .and_then(|values| {
            values.iter().find(|value| {
                value.plugin_id.trim()
                    == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
                    && value.namespace.trim()
                        == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
            })
        })
        .is_some_and(|value| {
            value
                .values
                .get("adapterKind")
                .and_then(serde_json::Value::as_str)
                == Some("custom")
                && value
                    .values
                    .get("customEnabled")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        })
}

pub(crate) async fn providers_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
) -> Result<Vec<providers::ProviderSummary>, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("providers_list", move || {
        providers::list_by_cli(&db, &cli_key)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn provider_upsert(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    mut input: ProviderUpsertInput,
) -> Result<providers::ProviderSummary, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    crate::domain::provider_account_usage::strip_custom_account_usage_permission_proofs(
        &mut input.extension_values,
    );
    let provider_uuid_override = input
        .provider_id
        .is_none()
        .then(crate::shared::uuid::new_uuid_v4);
    if custom_account_usage_enable_requested(input.extension_values.as_deref()) {
        let provider_id = input.provider_id;
        let source_provider_id = input.source_provider_id;
        let db_for_identity = db.clone();
        let provider_uuid_for_identity = provider_uuid_override.clone();
        let (provider_uuid, existing_values, existing_auth_mode, source_provider_uuid) =
            blocking::run(
                "provider_upsert_load_custom_account_usage_identity",
                move || {
                    let conn = db_for_identity.open_connection()?;
                    let (provider_uuid, existing_values, existing_auth_mode) = match provider_id {
                        Some(provider_id) => {
                            let context =
                                providers::get_account_usage_fetch_context(&conn, provider_id)?;
                            (
                                context.provider_uuid,
                                context.extension_values,
                                Some(context.auth_mode),
                            )
                        }
                        None => (
                            provider_uuid_for_identity.expect(
                                "create provider UUID must be allocated before confirmation",
                            ),
                            Vec::new(),
                            None,
                        ),
                    };
                    let source_provider_uuid = source_provider_id
                        .map(|source_provider_id| {
                            providers::get_account_usage_fetch_context(&conn, source_provider_id)
                                .map(|context| context.provider_uuid)
                        })
                        .transpose()?;
                    Ok::<_, crate::shared::error::AppError>((
                        provider_uuid,
                        existing_values,
                        existing_auth_mode,
                        source_provider_uuid,
                    ))
                },
            )
            .await
            .map_err(Into::<String>::into)?;
        let resolved_auth_mode = input
            .auth_mode
            .map(providers::ProviderAuthMode::as_str)
            .or(existing_auth_mode.as_deref())
            .unwrap_or(providers::ProviderAuthMode::ApiKey.as_str());
        if resolved_auth_mode != providers::ProviderAuthMode::ApiKey.as_str()
            || input.source_provider_id.is_some()
        {
            return Err(
                "SEC_INVALID_INPUT: custom account usage requires a direct API Key provider"
                    .to_string(),
            );
        }
        let permission_base_url = input
            .base_urls
            .iter()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let permission_scope =
            crate::domain::provider_account_usage::custom_account_usage_permission_scope(
                &provider_uuid,
                resolved_auth_mode,
                source_provider_uuid.as_deref(),
                permission_base_url,
            )
            .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))?;
        let permission = custom_account_usage_permission_request(
            input.extension_values.as_deref(),
            &permission_scope,
        )?
        .ok_or_else(|| {
            "SEC_INVALID_INPUT: custom account usage permission request is missing".to_string()
        })?;
        let already_confirmed =
            crate::domain::provider_account_usage::custom_account_usage_saved_permission_matches(
                &existing_values,
                &permission.fingerprint,
                &permission_scope,
            );
        if !already_confirmed {
            let confirmed = crate::app::provider_account_usage_confirmation::
                confirm_custom_account_usage_network_access(
                    &app,
                    crate::app::provider_account_usage_confirmation::
                        CustomAccountUsageConfirmationKind::Enable,
                    &permission.network_origins,
                    &permission.fingerprint,
                )
                .await?;
            if !confirmed {
                return Err(
                    "SEC_CONFIRM_REQUIRED: custom account usage permission was not confirmed"
                        .to_string(),
                );
            }
            crate::domain::provider_account_usage::add_custom_account_usage_permission_proof(
                &mut input.extension_values,
                &permission.fingerprint,
                &permission.base_origin,
            )?;
        }
    }

    let account_usage_secret_changed =
        input
            .account_usage_credentials
            .as_ref()
            .is_some_and(|patch| {
                patch.clear_new_api_access_token
                    || patch
                        .new_api_access_token
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty())
            });
    let ProviderUpsertInput {
        provider_id,
        cli_key,
        name,
        base_urls,
        base_url_mode,
        auth_mode,
        api_key,
        enabled,
        cost_multiplier,
        priority,
        claude_models,
        availability_test_model,
        limit_5h_usd,
        limit_daily_usd,
        daily_reset_mode,
        daily_reset_time,
        limit_weekly_usd,
        limit_monthly_usd,
        limit_total_usd,
        tags,
        note,
        source_provider_id,
        bridge_type,
        stream_idle_timeout_seconds,
        extension_values,
        account_usage_credentials,
        upstream_retry_policy_override,
        upstream_retry_policy_override_specified,
        model_routing_policy_override,
        model_routing_policy_override_specified,
    } = input;

    let is_create = provider_id.is_none();
    let name_for_log = name.clone();
    let cli_key_for_log = cli_key.clone();
    let submitted_api_key = api_key.clone();
    let result = blocking::run("provider_upsert", move || {
        let previous = match provider_id {
            Some(id) => {
                let conn = db.open_connection()?;
                Some(providers::get_by_id(&conn, id)?)
            }
            None => None,
        };
        let previous_api_key = match provider_id {
            Some(id) => Some(providers::get_api_key_plaintext(&db, id)?),
            None => None,
        };

        let saved = providers::upsert_with_provider_uuid(
            &db,
            providers::ProviderUpsertParams {
                provider_id,
                cli_key,
                name,
                base_urls,
                base_url_mode,
                auth_mode,
                api_key,
                enabled,
                cost_multiplier,
                priority,
                claude_models,
                availability_test_model,
                limit_5h_usd,
                limit_daily_usd,
                daily_reset_mode,
                daily_reset_time,
                limit_weekly_usd,
                limit_monthly_usd,
                limit_total_usd,
                tags,
                note,
                source_provider_id,
                bridge_type,
                stream_idle_timeout_seconds,
                extension_values,
                account_usage_credentials_patch: account_usage_credentials,
                account_usage_credentials_copy_from_provider_id: None,
                upstream_retry_policy_override,
                upstream_retry_policy_override_specified,
                model_routing_policy_override,
                model_routing_policy_override_specified,
            },
            provider_uuid_override,
        )?;

        let decision = provider_runtime_reset_decision(
            previous.as_ref(),
            previous_api_key.as_deref(),
            &saved,
            submitted_api_key.as_deref(),
            account_usage_secret_changed,
        );

        Ok::<_, crate::shared::error::AppError>((saved, decision))
    })
    .await
    .map_err(Into::into);

    if let Ok((ref provider, decision)) = result {
        let reconcile_account_usage_targets =
            decision.clear_route_runtime_state || decision.clear_account_usage_runtime_state;
        if is_create {
            tracing::info!(
                provider_id = provider.id,
                provider_name = %name_for_log,
                cli_key = %cli_key_for_log,
                "provider created"
            );
        } else {
            tracing::info!(
                provider_id = provider.id,
                provider_name = %name_for_log,
                cli_key = %cli_key_for_log,
                "provider updated"
            );
        }

        if decision.clear_route_runtime_state {
            let cleared = app_gateway_clear_cli_route_runtime_state(&app, &provider.cli_key);
            tracing::info!(
                provider_id = provider.id,
                cli_key = %provider.cli_key,
                cleared_sessions = cleared.cleared_sessions,
                cleared_recent_errors = cleared.cleared_recent_errors,
                "provider route runtime state cleared after provider save"
            );
        }
        if decision.clear_account_usage_runtime_state {
            invalidate_provider_account_usage_runtime(&app, provider.id).await;
        }
        if reconcile_account_usage_targets {
            reconcile_provider_account_usage_gateway_targets(&app).await;
        }
    }

    result.map(|(provider, _)| provider)
}

pub(crate) async fn provider_duplicate(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<providers::ProviderSummary, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let destination_provider_uuid = crate::shared::uuid::new_uuid_v4();
    let source_snapshot = blocking::run("provider_duplicate_load_permission_snapshot", {
        let db = db.clone();
        move || {
            let conn = db.open_connection()?;
            providers::get_account_usage_fetch_context(&conn, provider_id)
        }
    })
    .await
    .map_err(Into::<String>::into)?;
    let mut extension_values = Some(
        source_snapshot
            .extension_values
            .iter()
            .map(|value| providers::ProviderExtensionValuesInput {
                plugin_id: value.plugin_id.clone(),
                namespace: value.namespace.clone(),
                values: value.values.clone(),
            })
            .collect::<Vec<_>>(),
    );
    crate::domain::provider_account_usage::clear_custom_account_usage_permission(
        &mut extension_values,
    );

    let source_custom_authorized = source_snapshot.auth_mode == "api_key"
        && source_snapshot.source_provider_id.is_none()
        && match crate::domain::provider_account_usage::config_from_extension_values(
            &source_snapshot.extension_values,
        )
    {
        crate::domain::provider_account_usage::ProviderAccountUsageConfigState::Configured(
            config,
        ) if config.adapter_kind
            == crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind::Custom =>
        {
            let base_url = source_snapshot
                .base_urls
                .iter()
                .map(|value| value.trim())
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            config.custom.as_ref().is_some_and(|custom| {
                crate::domain::provider_account_usage::custom_account_usage_permission_scope(
                    &source_snapshot.provider_uuid,
                    &source_snapshot.auth_mode,
                    source_snapshot.source_provider_uuid.as_deref(),
                    base_url,
                )
                .is_ok_and(|scope| {
                    custom.enabled
                        && custom.permission_fingerprint.as_deref()
                            == Some(
                                crate::domain::provider_account_usage::
                                    custom_account_usage_authorization_fingerprint(custom, &scope)
                                    .as_str(),
                            )
                })
            })
        }
            _ => false,
        };

    if source_custom_authorized {
        let base_url = source_snapshot
            .base_urls
            .iter()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let destination_scope =
            crate::domain::provider_account_usage::custom_account_usage_permission_scope(
                &destination_provider_uuid,
                &source_snapshot.auth_mode,
                source_snapshot.source_provider_uuid.as_deref(),
                base_url,
            )
            .map_err(|message| format!("SEC_INVALID_INPUT: {message}"))?;
        let permission = custom_account_usage_permission_request(
            extension_values.as_deref(),
            &destination_scope,
        )?
        .ok_or_else(|| {
            "SEC_INVALID_INPUT: duplicated custom account usage permission is missing".to_string()
        })?;
        let confirmed = crate::app::provider_account_usage_confirmation::
            confirm_custom_account_usage_network_access(
                &app,
                crate::app::provider_account_usage_confirmation::
                    CustomAccountUsageConfirmationKind::Duplicate,
                &permission.network_origins,
                &permission.fingerprint,
            )
            .await?;
        if !confirmed {
            return Err(
                "SEC_CONFIRM_REQUIRED: custom account usage duplicate was not confirmed"
                    .to_string(),
            );
        }
        let current_snapshot = blocking::run("provider_duplicate_recheck_permission_snapshot", {
            let db = db.clone();
            move || {
                let conn = db.open_connection()?;
                providers::get_account_usage_fetch_context(&conn, provider_id)
            }
        })
        .await
        .map_err(Into::<String>::into)?;
        if source_snapshot.provider_uuid != current_snapshot.provider_uuid
            || source_snapshot.base_urls != current_snapshot.base_urls
            || source_snapshot.auth_mode != current_snapshot.auth_mode
            || source_snapshot.source_provider_id != current_snapshot.source_provider_id
            || source_snapshot.source_provider_uuid != current_snapshot.source_provider_uuid
            || source_snapshot.extension_values != current_snapshot.extension_values
        {
            return Err(
                "SEC_CONFIRM_STALE: source provider changed during custom account usage confirmation"
                    .to_string(),
            );
        }
        crate::domain::provider_account_usage::add_custom_account_usage_permission_proof(
            &mut extension_values,
            &permission.fingerprint,
            &permission.base_origin,
        )?;
    } else if let Some(values) = extension_values.as_mut() {
        for value in values {
            if value.plugin_id.trim()
                == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
                && value.namespace.trim()
                    == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
            {
                if let Some(object) = value.values.as_object_mut() {
                    if object
                        .get("adapterKind")
                        .and_then(serde_json::Value::as_str)
                        == Some("custom")
                    {
                        object.insert("customEnabled".to_string(), serde_json::Value::Bool(false));
                    }
                }
            }
        }
    }

    let result = blocking::run("provider_duplicate", move || {
        let conn = db.open_connection()?;
        let current_snapshot = providers::get_account_usage_fetch_context(&conn, provider_id)?;
        if source_snapshot.provider_uuid != current_snapshot.provider_uuid
            || source_snapshot.base_urls != current_snapshot.base_urls
            || source_snapshot.auth_mode != current_snapshot.auth_mode
            || source_snapshot.source_provider_id != current_snapshot.source_provider_id
            || source_snapshot.source_provider_uuid != current_snapshot.source_provider_uuid
            || source_snapshot.extension_values != current_snapshot.extension_values
        {
            return Err(crate::shared::error::AppError::new(
                "SEC_CONFIRM_STALE",
                "source provider changed before duplication",
            ));
        }
        let source = providers::get_by_id(&conn, provider_id)?;
        let siblings = providers::list_by_cli(&db, &source.cli_key)?;
        let api_key = if source.auth_mode == "api_key" && source.source_provider_id.is_none() {
            Some(providers::get_api_key_plaintext(&db, provider_id)?)
        } else {
            None
        };
        providers::upsert_with_provider_uuid(
            &db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: source.cli_key.clone(),
                name: build_duplicated_provider_name(&source.name, &siblings),
                base_urls: source.base_urls.clone(),
                base_url_mode: source.base_url_mode,
                auth_mode: match source.auth_mode.as_str() {
                    "oauth" => Some(providers::ProviderAuthMode::Oauth),
                    _ => Some(providers::ProviderAuthMode::ApiKey),
                },
                api_key,
                enabled: source.enabled,
                cost_multiplier: source.cost_multiplier,
                priority: None,
                claude_models: Some(source.claude_models.clone()),
                availability_test_model: source.availability_test_model.clone(),
                limit_5h_usd: source.limit_5h_usd,
                limit_daily_usd: source.limit_daily_usd,
                daily_reset_mode: Some(source.daily_reset_mode),
                daily_reset_time: Some(source.daily_reset_time.clone()),
                limit_weekly_usd: source.limit_weekly_usd,
                limit_monthly_usd: source.limit_monthly_usd,
                limit_total_usd: source.limit_total_usd,
                tags: Some(source.tags.clone()),
                note: Some(source.note.clone()),
                source_provider_id: source.source_provider_id,
                bridge_type: source.bridge_type.clone(),
                stream_idle_timeout_seconds: source.stream_idle_timeout_seconds,
                extension_values,
                account_usage_credentials_patch: None,
                account_usage_credentials_copy_from_provider_id: Some(provider_id),
                upstream_retry_policy_override: source.upstream_retry_policy_override.clone(),
                upstream_retry_policy_override_specified: true,
                model_routing_policy_override: source.model_routing_policy_override.clone(),
                model_routing_policy_override_specified: true,
            },
            Some(destination_provider_uuid),
        )
    })
    .await
    .map_err(Into::into);

    if let Ok(ref provider) = result {
        if provider.enabled {
            let cleared = app_gateway_clear_cli_route_runtime_state(&app, &provider.cli_key);
            tracing::info!(
                provider_id = provider.id,
                cli_key = %provider.cli_key,
                cleared_sessions = cleared.cleared_sessions,
                cleared_recent_errors = cleared.cleared_recent_errors,
                "provider route runtime state cleared after duplicate"
            );
        }

        tracing::info!(
            provider_id = provider.id,
            cli_key = %provider.cli_key,
            provider_name = %provider.name,
            "provider duplicated"
        );
        reconcile_provider_account_usage_gateway_targets(&app).await;
    }

    result
}

pub(crate) async fn provider_set_enabled(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    enabled: bool,
) -> Result<providers::ProviderSummary, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run("provider_set_enabled", move || {
        providers::set_enabled(&db, provider_id, enabled)
    })
    .await
    .map_err(Into::into);

    if let Ok(ref provider) = result {
        invalidate_provider_account_usage_runtime(&app, provider.id).await;
        let cleared = app_gateway_clear_cli_route_runtime_state(&app, &provider.cli_key);
        tracing::info!(
            provider_id = provider.id,
            enabled = provider.enabled,
            cleared_sessions = cleared.cleared_sessions,
            cleared_recent_errors = cleared.cleared_recent_errors,
            "provider enabled state changed"
        );
        reconcile_provider_account_usage_gateway_targets(&app).await;
    }

    result
}

pub(crate) async fn provider_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    clear_usage_stats: bool,
) -> Result<bool, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run(
        "provider_delete",
        move || -> crate::shared::error::AppResult<(bool, String)> {
            let cli_key = providers::cli_key_by_id(&db, provider_id)?.ok_or_else(|| {
                crate::shared::error::AppError::from("DB_NOT_FOUND: provider not found")
            })?;
            providers::delete(&db, provider_id, clear_usage_stats)?;
            Ok((true, cli_key))
        },
    )
    .await
    .map_err(Into::into);

    if let Ok((true, ref cli_key)) = result {
        invalidate_provider_account_usage_runtime(&app, provider_id).await;
        let cleared = app_gateway_clear_cli_route_runtime_state(&app, cli_key);
        tracing::info!(
            provider_id = provider_id,
            cli_key = %cli_key,
            clear_usage_stats = clear_usage_stats,
            cleared_sessions = cleared.cleared_sessions,
            cleared_recent_errors = cleared.cleared_recent_errors,
            "provider deleted"
        );
        reconcile_provider_account_usage_gateway_targets(&app).await;
    }

    result.map(|(deleted, _)| deleted)
}

pub(crate) async fn providers_reorder(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    ordered_provider_ids: Vec<i64>,
) -> Result<Vec<providers::ProviderSummary>, String> {
    let cli_key_for_log = cli_key.clone();
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run("providers_reorder", move || {
        providers::reorder(&db, &cli_key, ordered_provider_ids)
    })
    .await
    .map_err(Into::into);

    if let Ok(ref providers) = result {
        tracing::info!(
            cli_key = %cli_key_for_log,
            count = providers.len(),
            "provider pool display order updated"
        );
    }

    result
}

pub(crate) async fn default_route_providers_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
) -> Result<Vec<providers::ProviderRouteRow>, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("default_route_providers_list", move || {
        providers::default_route_list(&db, &cli_key)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn default_route_providers_set_order(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    ordered_provider_ids: Vec<i64>,
) -> Result<Vec<providers::ProviderRouteRow>, String> {
    let cli_key_for_log = cli_key.clone();
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run("default_route_providers_set_order", move || {
        providers::default_route_set_order(&db, &cli_key, ordered_provider_ids)
    })
    .await
    .map_err(Into::into);

    if let Ok(ref rows) = result {
        let cleared = app_gateway_clear_cli_route_runtime_state(&app, &cli_key_for_log);
        tracing::info!(
            cli_key = %cli_key_for_log,
            count = rows.len(),
            cleared_sessions = cleared.cleared_sessions,
            cleared_recent_errors = cleared.cleared_recent_errors,
            "default route provider order updated"
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_account_usage_config(
        provider: &mut providers::ProviderSummary,
        adapter_kind: &str,
        timed_refresh_enabled: bool,
        route_gate_enabled: bool,
    ) {
        provider.extension_values = vec![providers::ProviderExtensionValues {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": adapter_kind,
                "newApiQueryMode": "billing",
                "refreshIntervalSeconds": 300,
                "timedRefreshEnabled": timed_refresh_enabled,
                "routeGateEnabled": route_gate_enabled,
            }),
            updated_at: 1,
        }];
    }

    #[test]
    fn custom_account_usage_permission_precheck_classifies_invalid_input() {
        let values = vec![providers::ProviderExtensionValuesInput {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": "custom",
                "customAllowedOrigins": [],
                "customTimeoutSeconds": 5,
                "customEnabled": true
            }),
        }];

        let scope = crate::domain::provider_account_usage::custom_account_usage_permission_scope(
            "11111111-1111-4111-8111-111111111111",
            "api_key",
            None,
            "https://api.example.test/v1",
        )
        .expect("valid synthetic scope");
        let error = custom_account_usage_permission_request(Some(&values), &scope)
            .expect_err("invalid custom config must fail precheck");

        assert_eq!(error, "SEC_INVALID_INPUT: 自定义账户用量脚本不能为空");
    }

    #[test]
    fn provider_upsert_input_deserializes_runtime_camel_case_shape() {
        let input: ProviderUpsertInput = serde_json::from_value(serde_json::json!({
            "providerId": 1,
            "cliKey": "claude",
            "name": "P1",
            "baseUrls": ["https://example.com"],
            "baseUrlMode": "order",
            "authMode": "api_key",
            "apiKey": "k1",
            "enabled": true,
            "costMultiplier": 1.0,
            "priority": 10,
            "claudeModels": null,
            "limit5hUsd": 5.0,
            "limitDailyUsd": 10.0,
            "dailyResetMode": "fixed",
            "dailyResetTime": "00:00:00",
            "limitWeeklyUsd": null,
            "limitMonthlyUsd": null,
            "limitTotalUsd": null,
            "tags": ["x"],
            "note": "n",
            "streamIdleTimeoutSeconds": 90
        }))
        .expect("deserialize provider input");

        assert_eq!(input.base_url_mode, providers::ProviderBaseUrlMode::Order);
        assert_eq!(input.auth_mode, Some(providers::ProviderAuthMode::ApiKey));
        assert_eq!(input.limit_5h_usd, Some(5.0));
        assert_eq!(
            input.daily_reset_mode,
            Some(providers::DailyResetMode::Fixed)
        );
        assert_eq!(input.stream_idle_timeout_seconds, Some(90));
    }

    #[test]
    fn provider_upsert_input_accepts_legacy_generated_limit_alias() {
        let input: ProviderUpsertInput = serde_json::from_value(serde_json::json!({
            "providerId": 1,
            "cliKey": "claude",
            "name": "P1",
            "baseUrls": ["https://example.com"],
            "baseUrlMode": "ping",
            "enabled": true,
            "costMultiplier": 1.0,
            "limit5HUsd": 7.0,
            "limitDailyUsd": null,
            "dailyResetMode": "rolling",
            "dailyResetTime": "00:00:00",
            "limitWeeklyUsd": null,
            "limitMonthlyUsd": null,
            "limitTotalUsd": null
        }))
        .expect("deserialize provider input legacy alias");

        assert_eq!(input.base_url_mode, providers::ProviderBaseUrlMode::Ping);
        assert_eq!(input.limit_5h_usd, Some(7.0));
        assert_eq!(
            input.daily_reset_mode,
            Some(providers::DailyResetMode::Rolling)
        );
    }

    #[test]
    fn provider_runtime_reset_decision_handles_create_and_non_sensitive_edits() {
        let next = providers::ProviderSummary {
            id: 1,
            provider_uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            cli_key: "claude".to_string(),
            name: "Provider A".to_string(),
            base_urls: vec!["https://api.example.com".to_string()],
            base_url_mode: providers::ProviderBaseUrlMode::Order,
            claude_models: Default::default(),
            availability_test_model: None,
            enabled: true,
            priority: 1,
            cost_multiplier: 1.0,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: providers::DailyResetMode::Fixed,
            daily_reset_time: "00:00:00".to_string(),
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
            tags: vec![],
            note: String::new(),
            created_at: 1,
            updated_at: 1,
            auth_mode: "api_key".to_string(),
            oauth_provider_type: None,
            oauth_email: None,
            oauth_expires_at: None,
            oauth_last_error: None,
            source_provider_id: None,
            bridge_type: None,
            stream_idle_timeout_seconds: None,
            extension_values: vec![],
            upstream_retry_policy_override: None,
            model_routing_policy_override: None,
            api_key_configured: true,
            newapi_account_user_id: None,
            newapi_account_access_token_configured: false,
        };

        assert_eq!(
            provider_runtime_reset_decision(None, None, &next, None, false),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: false,
            }
        );

        let mut disabled_create = next.clone();
        disabled_create.enabled = false;
        assert_eq!(
            provider_runtime_reset_decision(None, None, &disabled_create, None, false),
            ProviderRuntimeResetDecision::default()
        );

        let mut previous = next.clone();
        previous.name = "Old Name".to_string();
        previous.note = "old".to_string();
        previous.updated_at = 0;

        assert_eq!(
            provider_runtime_reset_decision(
                Some(&previous),
                Some("sk-existing"),
                &next,
                Some("   "),
                false
            ),
            ProviderRuntimeResetDecision::default()
        );

        assert_eq!(
            provider_runtime_reset_decision(
                Some(&previous),
                Some("sk-existing"),
                &next,
                Some("sk-existing"),
                false
            ),
            ProviderRuntimeResetDecision::default()
        );

        let mut timed_previous = next.clone();
        set_account_usage_config(&mut timed_previous, "sub2api", false, false);
        let mut timed_next = timed_previous.clone();
        set_account_usage_config(&mut timed_next, "sub2api", true, false);
        assert_eq!(
            provider_runtime_reset_decision(
                Some(&timed_previous),
                Some("sk-existing"),
                &timed_next,
                None,
                false,
            ),
            ProviderRuntimeResetDecision::default(),
            "display-only timed refresh must not invalidate route recovery state",
        );

        let mut gate_next = timed_previous.clone();
        set_account_usage_config(&mut gate_next, "sub2api", false, true);
        assert_eq!(
            provider_runtime_reset_decision(
                Some(&timed_previous),
                Some("sk-existing"),
                &gate_next,
                None,
                false,
            ),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: false,
            },
            "route gate changes must clear sessions without evicting the display cache",
        );

        let mut adapter_next = gate_next.clone();
        set_account_usage_config(&mut adapter_next, "newapi", false, true);
        assert_eq!(
            provider_runtime_reset_decision(
                Some(&gate_next),
                Some("sk-existing"),
                &adapter_next,
                None,
                false,
            ),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: true,
            },
            "adapter changes must invalidate both route and query runtime state",
        );

        let mut disabled = next.clone();
        disabled.enabled = false;

        assert_eq!(
            provider_runtime_reset_decision(
                Some(&next),
                Some("sk-existing"),
                &disabled,
                None,
                false,
            ),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: true,
            }
        );
    }

    #[test]
    fn provider_runtime_reset_decision_detects_sensitive_claude_changes() {
        let previous = providers::ProviderSummary {
            id: 1,
            provider_uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            cli_key: "claude".to_string(),
            name: "Provider A".to_string(),
            base_urls: vec!["https://api.old.example.com".to_string()],
            base_url_mode: providers::ProviderBaseUrlMode::Order,
            claude_models: Default::default(),
            availability_test_model: None,
            enabled: true,
            priority: 1,
            cost_multiplier: 1.0,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: providers::DailyResetMode::Fixed,
            daily_reset_time: "00:00:00".to_string(),
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
            tags: vec![],
            note: String::new(),
            created_at: 1,
            updated_at: 1,
            auth_mode: "api_key".to_string(),
            oauth_provider_type: None,
            oauth_email: None,
            oauth_expires_at: None,
            oauth_last_error: None,
            source_provider_id: None,
            bridge_type: None,
            stream_idle_timeout_seconds: None,
            extension_values: vec![],
            upstream_retry_policy_override: None,
            model_routing_policy_override: None,
            api_key_configured: true,
            newapi_account_user_id: None,
            newapi_account_access_token_configured: false,
        };

        let mut next = previous.clone();
        next.base_urls = vec!["https://api.new.example.com".to_string()];

        assert_eq!(
            provider_runtime_reset_decision(Some(&previous), Some("sk-old"), &next, None, false,),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: true,
            }
        );

        let mut next_non_claude = previous.clone();
        next_non_claude.cli_key = "codex".to_string();

        assert_eq!(
            provider_runtime_reset_decision(
                Some(&next_non_claude),
                Some("sk-old"),
                &next_non_claude,
                Some("sk-new"),
                false
            ),
            ProviderRuntimeResetDecision {
                clear_route_runtime_state: true,
                clear_account_usage_runtime_state: true,
            }
        );
    }
}
