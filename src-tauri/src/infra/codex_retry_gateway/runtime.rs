use crate::infra::codex_retry_gateway::bridge::create_bridge_details_session;
use crate::infra::codex_retry_gateway::config::DEFAULT_LISTEN_HOST;
use crate::infra::codex_retry_gateway::managed_state::{
    read_manager_state, write_manager_state, CodexRetryGatewayManagerPaths,
    CodexRetryGatewayManagerState,
};
use crate::infra::codex_retry_gateway::node::{public_node_error, resolve_node_status};
use crate::infra::codex_retry_gateway::process::{
    reconcile_runtime_process, start_runtime_process, stop_runtime_process,
    CodexRetryGatewayManagedProcess, CodexRetryGatewayProcessReconcileResult,
};
use crate::infra::codex_retry_gateway::source::{
    install_source_commit, public_source_error, revalidate_cached_source, validate_commit_request,
    CodexRetryGatewaySourceHttpConfig,
};
use crate::infra::codex_retry_gateway::util::{normalize_full_sha, now_unix_ms};
use crate::infra::codex_retry_gateway::{
    AioGatewayOrigin, CodexProviderSyncPlan, CodexRetryGatewayApplyCommitRequest,
    CodexRetryGatewayCommitValidation, CodexRetryGatewayDetailsSession,
    CodexRetryGatewayEnableConfirmation, CodexRetryGatewayEnablePlan, CodexRetryGatewayError,
    CodexRetryGatewayErrorCategory, CodexRetryGatewayGenerationRequest,
    CodexRetryGatewayLifecycleCallback, CodexRetryGatewayNodeStatus, CodexRetryGatewayProcessPhase,
    CodexRetryGatewayRouteCallbackRequest, CodexRetryGatewayRuntimePhase,
    CodexRetryGatewaySetEnabledRequest, CodexRetryGatewaySetNodeOverrideRequest,
    CodexRetryGatewayStatus, CodexRetryGatewayTrustState, CodexRetryGatewayUninstallRequest,
    CodexRetryGatewayUpdateCandidate, CodexRetryGatewayValidateCommitRequest, CodexRouteMode,
    CODEX_RETRY_GATEWAY_DEFAULT_PORT, CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT,
};
use crate::shared::error::{AppError, AppResult};
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};

const RECOVERY_FAILURE_PAUSE_THRESHOLD: u32 = 5;
const RECOVERY_BACKOFF_BASE_MS: u64 = 5_000;
const RECOVERY_BACKOFF_MAX_MS: u64 = 5 * 60 * 1_000;

struct RuntimeFailClosedCallback;

impl CodexRetryGatewayLifecycleCallback for RuntimeFailClosedCallback {
    fn request_direct_aio(&self, _request: CodexRetryGatewayRouteCallbackRequest) -> AppResult<()> {
        Err(AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_CALLBACK_UNAVAILABLE",
            "gateway lifecycle callback is not installed",
        ))
    }

    fn request_gateway_disable(
        &self,
        _request: CodexRetryGatewayRouteCallbackRequest,
    ) -> AppResult<()> {
        Err(AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_CALLBACK_UNAVAILABLE",
            "gateway lifecycle callback is not installed",
        ))
    }
}

static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static LIFECYCLE_CALLBACK: OnceLock<RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>>> =
    OnceLock::new();

fn runtime_lock() -> &'static Mutex<()> {
    RUNTIME_LOCK.get_or_init(|| Mutex::new(()))
}

fn lifecycle_callback_slot() -> &'static RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>> {
    LIFECYCLE_CALLBACK.get_or_init(|| RwLock::new(Arc::new(RuntimeFailClosedCallback)))
}

#[cfg(test)]
pub(crate) async fn install_lifecycle_callback_for_tests(
    callback: Arc<dyn CodexRetryGatewayLifecycleCallback>,
) {
    *lifecycle_callback_slot().write().await = callback;
}

pub(crate) async fn current_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexRetryGatewayStatus> {
    build_runtime_status(app, None).await
}

pub(crate) async fn build_enable_plan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexRetryGatewayEnablePlan> {
    let status = current_status(app).await?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let first_download_required =
        revalidate_cached_source(&paths, &status.selected_commit)?.is_none();
    let settings = crate::settings::read(app)?;
    Ok(CodexRetryGatewayEnablePlan {
        generation: status.generation,
        selected_commit: status.selected_commit.clone(),
        trust_state: status.trust_state,
        first_download_required,
        unreviewed_commit: status.trust_state
            == CodexRetryGatewayTrustState::OfficialMainUnreviewed,
        cli_proxy_enable_required: !status.cli_proxy_enabled,
        provider_sync: CodexProviderSyncPlan {
            current_provider: Some("aio".to_string()),
            target_provider: "aio".to_string(),
            change_required: false,
            codex_must_be_closed: false,
        },
        node_status: status.node_status.clone(),
        preferred_port: settings
            .codex_retry_gateway_preferred_port
            .max(CODEX_RETRY_GATEWAY_DEFAULT_PORT),
        wsl_codex_unprotected: settings.wsl_auto_config && settings.wsl_target_cli.codex,
    })
}

pub(crate) async fn runtime_update_candidate<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Option<CodexRetryGatewayUpdateCandidate>> {
    let status = current_status(app).await?;
    let candidate =
        validate_commit_request("main", &CodexRetryGatewaySourceHttpConfig::default()).await;
    let Some(selection) = candidate.selection else {
        return Ok(None);
    };
    let current_commit = status
        .active_commit
        .clone()
        .or_else(|| Some(status.selected_commit.clone()));
    if current_commit.as_deref() == Some(selection.canonical_commit.as_str()) {
        return Ok(None);
    }
    Ok(Some(CodexRetryGatewayUpdateCandidate {
        commit: selection.canonical_commit.clone(),
        current_commit,
        previous_commit: status.previous_commit.clone(),
        official_main_commit: selection.official_main_commit,
        commits_ahead: None,
        summary: selection.summary,
        trust_state: selection.trust_state,
    }))
}

pub(crate) async fn validate_selected_commit(
    request: CodexRetryGatewayValidateCommitRequest,
) -> AppResult<CodexRetryGatewayCommitValidation> {
    Ok(validate_commit_request(
        &request.commit,
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await
    .validation)
}

pub(crate) async fn set_runtime_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewaySetEnabledRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _guard = runtime_lock().lock().await;
    let plan = build_enable_plan(app).await?;
    if request.plan_generation != plan.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "enable request generation no longer matches the latest runtime plan",
        ));
    }
    let mut settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;

    if !request.enabled {
        settings.codex_retry_gateway_enabled = false;
        crate::settings::write(app, &settings)?;
        if let Some(record) = manager.process_record.clone() {
            let _ = stop_runtime_process(&paths, &record).await;
        }
        manager.generation = manager.generation.saturating_add(1);
        manager.last_error = None;
        manager.process_record = None;
        manager.recovery_failure_count = 0;
        manager.recovery_next_retry_at_ms = None;
        manager.recovery_paused = false;
        write_manager_state(&paths, &manager)?;
        return build_runtime_status(app, Some(manager)).await;
    }

    require_enable_confirmations(&plan, &request.confirmation)?;
    settings.codex_retry_gateway_enabled = true;
    crate::settings::write(app, &settings)?;

    match ensure_runtime_process(app, &settings, &paths, &mut manager).await {
        Ok(process) => {
            manager.generation = manager.generation.saturating_add(1);
            manager.active_commit = Some(process.record.source_commit.clone());
            manager.effective_port = parse_listener_port(&process.record.listener);
            manager.process_record = Some(process.record);
            manager.verified_main_commit = Some(
                manager
                    .verified_main_commit
                    .clone()
                    .unwrap_or_else(|| CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string()),
            );
            manager.recovery_failure_count = 0;
            manager.recovery_next_retry_at_ms = None;
            manager.recovery_paused = false;
            manager.last_error = None;
            write_manager_state(&paths, &manager)?;
            build_runtime_status(app, Some(manager)).await
        }
        Err(error) => {
            record_runtime_failure(&mut manager, runtime_public_error(&error));
            write_manager_state(&paths, &manager)?;
            Err(error)
        }
    }
}

pub(crate) async fn apply_selected_commit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewayApplyCommitRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _guard = runtime_lock().lock().await;
    let status = current_status(app).await?;
    if request.plan_generation != status.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "commit apply generation no longer matches the runtime state",
        ));
    }
    let candidate = validate_commit_request(
        &request.commit,
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await;
    let Some(selection) = candidate.selection else {
        return Err(AppError::new(
            candidate
                .validation
                .error
                .as_ref()
                .map(|error| error.code.clone())
                .unwrap_or_else(|| "CODEX_RETRY_GATEWAY_SOURCE_RESOLUTION_FAILED".to_string()),
            candidate
                .validation
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "failed to validate commit selection".to_string()),
        ));
    };
    if !request.accepted_update && status.selected_commit != selection.canonical_commit {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "commit update requires explicit confirmation",
        ));
    }
    if selection.trust_state == CodexRetryGatewayTrustState::OfficialMainUnreviewed
        && !request.accepted_unreviewed_commit
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "unreviewed official-main commits require explicit confirmation",
        ));
    }

    let mut settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    settings.codex_retry_gateway_selected_commit = selection.canonical_commit.clone();
    crate::settings::write(app, &settings)?;

    let node = resolve_required_node(app, &settings.codex_retry_gateway_node_override)?;
    let installed = install_source_commit(
        &paths,
        &selection,
        &node,
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await?;
    manager.verified_main_commit = Some(installed.manifest.verified_main_commit.clone());

    if settings.codex_retry_gateway_enabled {
        if let Some(record) = manager.process_record.clone() {
            let _ = stop_runtime_process(&paths, &record).await;
        }
        let aio_origin = aio_origin(app, &settings);
        let process = start_runtime_process(
            &paths,
            &installed,
            &node,
            &aio_origin,
            settings
                .codex_retry_gateway_preferred_port
                .max(CODEX_RETRY_GATEWAY_DEFAULT_PORT),
            manager.effective_port,
        )
        .await?;
        if manager.active_commit.as_deref() != Some(selection.canonical_commit.as_str()) {
            manager.previous_commit = manager.active_commit.clone();
        }
        manager.active_commit = Some(selection.canonical_commit.clone());
        manager.process_record = Some(process.record);
        manager.effective_port = parse_listener_port(
            manager
                .process_record
                .as_ref()
                .map(|record| record.listener.as_str())
                .unwrap_or_default(),
        );
    }

    manager.generation = manager.generation.saturating_add(1);
    manager.last_error = None;
    write_manager_state(&paths, &manager)?;
    build_runtime_status(app, Some(manager)).await
}

pub(crate) async fn set_runtime_node_override<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewaySetNodeOverrideRequest,
) -> AppResult<CodexRetryGatewayNodeStatus> {
    let status = current_status(app).await?;
    if request.generation != status.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "node override generation no longer matches the runtime state",
        ));
    }
    crate::infra::codex_retry_gateway::node::set_node_override(app, request.executable.as_deref())
}

pub(crate) async fn retry_runtime_recovery<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewayGenerationRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _guard = runtime_lock().lock().await;
    let status = current_status(app).await?;
    if request.generation != status.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "retry generation no longer matches the runtime state",
        ));
    }
    let settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    manager.recovery_paused = false;
    manager.recovery_next_retry_at_ms = None;
    if settings.codex_retry_gateway_enabled {
        match ensure_runtime_process(app, &settings, &paths, &mut manager).await {
            Ok(process) => {
                manager.process_record = Some(process.record);
                manager.active_commit = manager
                    .process_record
                    .as_ref()
                    .map(|record| record.source_commit.clone());
                manager.effective_port = manager
                    .process_record
                    .as_ref()
                    .and_then(|record| parse_listener_port(&record.listener));
                manager.recovery_failure_count = 0;
                manager.last_error = None;
            }
            Err(error) => {
                record_runtime_failure(&mut manager, runtime_public_error(&error));
            }
        }
    }
    manager.generation = manager.generation.saturating_add(1);
    write_manager_state(&paths, &manager)?;
    build_runtime_status(app, Some(manager)).await
}

pub(crate) async fn uninstall_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewayUninstallRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _guard = runtime_lock().lock().await;
    let status = current_status(app).await?;
    if request.generation != status.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "uninstall generation no longer matches the runtime state",
        ));
    }
    if !request.confirmed_data_removal {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "runtime uninstall requires explicit data removal confirmation",
        ));
    }
    let mut settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let manager = read_manager_state(&paths)?;
    if let Some(record) = manager.process_record.as_ref() {
        let _ = stop_runtime_process(&paths, record).await;
    }
    let _ = std::fs::remove_dir_all(&paths.root);
    settings.codex_retry_gateway_enabled = false;
    settings.codex_retry_gateway_selected_commit =
        CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string();
    settings.codex_retry_gateway_preferred_port = CODEX_RETRY_GATEWAY_DEFAULT_PORT;
    settings.codex_retry_gateway_node_override.clear();
    crate::settings::write(app, &settings)?;
    current_status(app).await
}

pub(crate) async fn create_details_session<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexRetryGatewayDetailsSession> {
    let status = current_status(app).await?;
    if !status.desired_enabled || !status.process_status.healthy {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
            "managed gateway must be healthy before opening the details bridge",
        ));
    }
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let manager = read_manager_state(&paths)?;
    let record = manager.process_record.as_ref().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
            "managed gateway process record is missing",
        )
    })?;
    let reconciled =
        reconcile_runtime_process(&paths, Some(record), manager.effective_port).await?;
    let Some(process) = reconciled.managed else {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
            "managed gateway process is no longer owned and healthy",
        ));
    };
    let callback = lifecycle_callback_slot().read().await.clone();
    Ok(
        create_bridge_details_session(&paths, status.generation, &process, callback)
            .await?
            .session,
    )
}

async fn build_runtime_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manager_override: Option<CodexRetryGatewayManagerState>,
) -> AppResult<CodexRetryGatewayStatus> {
    let settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let manager = match manager_override {
        Some(manager) => manager,
        None => read_manager_state(&paths)?,
    };
    let selected_commit = normalize_selected_commit(&settings.codex_retry_gateway_selected_commit);
    let node_status = resolve_node_status(app, Some(&settings.codex_retry_gateway_node_override));
    let reconcile = reconcile_runtime_process(
        &paths,
        manager.process_record.as_ref(),
        manager.effective_port,
    )
    .await?;
    let aio_origin = aio_origin(app, &settings);
    let cli_proxy = codex_cli_proxy_projection(app, manager.effective_port, &aio_origin.url);
    let route_mode = cli_proxy.route_mode;

    let trust_state = if selected_commit == CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT {
        CodexRetryGatewayTrustState::AioReviewedRecommendation
    } else {
        CodexRetryGatewayTrustState::OfficialMainUnreviewed
    };
    let last_error = manager
        .last_error
        .clone()
        .or_else(|| reconcile.error.clone())
        .or_else(|| node_status.error.clone());
    let runtime_phase = derive_runtime_phase(
        settings.codex_retry_gateway_enabled,
        route_mode,
        &reconcile,
        manager.recovery_paused,
    );
    Ok(CodexRetryGatewayStatus {
        generation: manager.generation,
        desired_enabled: settings.codex_retry_gateway_enabled,
        runtime_phase,
        route_mode,
        cli_proxy_enabled: cli_proxy.enabled,
        cli_proxy_applied: cli_proxy.applied,
        effective_port: manager.effective_port.or_else(|| {
            manager
                .process_record
                .as_ref()
                .and_then(|record| parse_listener_port(&record.listener))
        }),
        repository: crate::infra::codex_retry_gateway::CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
        license: None,
        selected_commit,
        active_commit: manager.active_commit.clone(),
        previous_commit: manager.previous_commit.clone(),
        recommended_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string(),
        trust_state,
        node_status,
        process_status: reconcile.status,
        update_candidate: None,
        wsl_codex_unprotected: settings.wsl_auto_config && settings.wsl_target_cli.codex,
        last_error,
        details_available: reconcile.managed.is_some(),
        operation_pending: false,
    })
}

async fn ensure_runtime_process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &crate::settings::AppSettings,
    paths: &CodexRetryGatewayManagerPaths,
    manager: &mut CodexRetryGatewayManagerState,
) -> AppResult<CodexRetryGatewayManagedProcess> {
    let node = resolve_required_node(app, &settings.codex_retry_gateway_node_override)?;
    let selection = validate_commit_request(
        &normalize_selected_commit(&settings.codex_retry_gateway_selected_commit),
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await;
    let selection = selection.selection.ok_or_else(|| {
        let error = selection.validation.error.unwrap_or_else(|| {
            runtime_public_error_message(
                "CODEX_RETRY_GATEWAY_SOURCE_RESOLUTION_FAILED",
                "failed to validate selected commit",
            )
        });
        AppError::new(error.code, error.message)
    })?;

    if let Some(record) = manager.process_record.as_ref() {
        let reconciled =
            reconcile_runtime_process(paths, Some(record), manager.effective_port).await?;
        if let Some(process) = reconciled.managed.as_ref() {
            if process.record.source_commit == selection.canonical_commit
                && process.record.node_executable == node.executable.display().to_string()
            {
                return Ok(process.clone());
            }
        }
        if reconciled.managed.is_some() {
            let _ = stop_runtime_process(paths, record).await;
        }
    }

    let installed = install_source_commit(
        paths,
        &selection,
        &node,
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await?;
    manager.verified_main_commit = Some(installed.manifest.verified_main_commit.clone());
    let aio_origin = aio_origin(app, settings);
    start_runtime_process(
        paths,
        &installed,
        &node,
        &aio_origin,
        settings
            .codex_retry_gateway_preferred_port
            .max(CODEX_RETRY_GATEWAY_DEFAULT_PORT),
        manager.effective_port,
    )
    .await
}

fn resolve_required_node<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manual_override: &str,
) -> AppResult<crate::infra::codex_retry_gateway::CodexRetryGatewayResolvedNode> {
    crate::infra::codex_retry_gateway::resolve_node_runtime(app, Some(manual_override))
}

fn require_enable_confirmations(
    plan: &CodexRetryGatewayEnablePlan,
    confirmation: &CodexRetryGatewayEnableConfirmation,
) -> AppResult<()> {
    if plan.first_download_required && !confirmation.accepted_first_download {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "first managed download requires explicit confirmation",
        ));
    }
    if plan.unreviewed_commit && !confirmation.accepted_unreviewed_commit {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "unreviewed commit requires explicit confirmation",
        ));
    }
    if plan.cli_proxy_enable_required && !confirmation.accepted_cli_proxy_enable {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "CLI proxy enablement requires explicit confirmation",
        ));
    }
    if plan.provider_sync.change_required && !confirmation.accepted_provider_sync {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "provider sync requires explicit confirmation",
        ));
    }
    if plan.wsl_codex_unprotected && !confirmation.accepted_wsl_unprotected {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "WSL Codex traffic remains unprotected and requires explicit confirmation",
        ));
    }
    Ok(())
}

fn normalize_selected_commit(value: &str) -> String {
    normalize_full_sha(value).unwrap_or_else(|_| CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string())
}

fn aio_origin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &crate::settings::AppSettings,
) -> AioGatewayOrigin {
    let status = crate::gateway_runtime_access::app_gateway_status(app);
    let base_origin = if status.running {
        status.base_url.unwrap_or_else(|| {
            format!(
                "http://127.0.0.1:{}",
                status.port.unwrap_or(crate::settings::DEFAULT_GATEWAY_PORT)
            )
        })
    } else {
        format!(
            "http://127.0.0.1:{}",
            settings
                .preferred_port
                .max(crate::settings::DEFAULT_GATEWAY_PORT)
        )
    };
    AioGatewayOrigin {
        url: format!("{}/v1", base_origin.trim_end_matches('/')),
    }
}

struct CliProxyProjection {
    enabled: bool,
    applied: bool,
    route_mode: CodexRouteMode,
}

fn codex_cli_proxy_projection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    effective_port: Option<u16>,
    aio_origin: &str,
) -> CliProxyProjection {
    let aio_base_origin = aio_origin
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string();
    let external_origin = effective_port.map(|port| format!("http://{DEFAULT_LISTEN_HOST}:{port}"));
    let row = crate::cli_proxy::status_all(app, Some(&aio_base_origin))
        .ok()
        .and_then(|rows| rows.into_iter().find(|row| row.cli_key == "codex"));

    let enabled = row.as_ref().map(|row| row.enabled).unwrap_or(false);
    let applied = row
        .as_ref()
        .and_then(|row| row.applied_to_current_gateway)
        .unwrap_or(false);
    let route_mode = if enabled
        && external_origin.as_deref() == row.as_ref().and_then(|row| row.base_origin.as_deref())
    {
        CodexRouteMode::Guarded
    } else if enabled && applied {
        CodexRouteMode::DirectAio
    } else {
        CodexRouteMode::Unproxied
    };
    CliProxyProjection {
        enabled,
        applied,
        route_mode,
    }
}

fn derive_runtime_phase(
    desired_enabled: bool,
    route_mode: CodexRouteMode,
    reconcile: &CodexRetryGatewayProcessReconcileResult,
    recovery_paused: bool,
) -> CodexRetryGatewayRuntimePhase {
    if recovery_paused {
        return CodexRetryGatewayRuntimePhase::RecoveryPaused;
    }
    if !desired_enabled {
        return match reconcile.status.phase {
            CodexRetryGatewayProcessPhase::Healthy
            | CodexRetryGatewayProcessPhase::Starting
            | CodexRetryGatewayProcessPhase::Unhealthy
            | CodexRetryGatewayProcessPhase::OwnershipMismatch => {
                CodexRetryGatewayRuntimePhase::CleanupNeeded
            }
            CodexRetryGatewayProcessPhase::Stopped => CodexRetryGatewayRuntimePhase::Disabled,
        };
    }
    match reconcile.status.phase {
        CodexRetryGatewayProcessPhase::Healthy if route_mode == CodexRouteMode::Guarded => {
            CodexRetryGatewayRuntimePhase::Guarded
        }
        CodexRetryGatewayProcessPhase::Healthy => CodexRetryGatewayRuntimePhase::BypassedRecovering,
        CodexRetryGatewayProcessPhase::Starting => CodexRetryGatewayRuntimePhase::Starting,
        CodexRetryGatewayProcessPhase::Stopped => CodexRetryGatewayRuntimePhase::Preparing,
        CodexRetryGatewayProcessPhase::Unhealthy
        | CodexRetryGatewayProcessPhase::OwnershipMismatch => {
            CodexRetryGatewayRuntimePhase::BypassedRecovering
        }
    }
}

fn parse_listener_port(listener: &str) -> Option<u16> {
    reqwest::Url::parse(listener)
        .ok()
        .and_then(|url| url.port_or_known_default())
}

fn record_runtime_failure(
    manager: &mut CodexRetryGatewayManagerState,
    error: CodexRetryGatewayError,
) {
    manager.last_error = Some(error);
    manager.recovery_failure_count = manager.recovery_failure_count.saturating_add(1);
    let exponent = manager.recovery_failure_count.saturating_sub(1).min(16);
    let backoff = RECOVERY_BACKOFF_BASE_MS
        .saturating_mul(1_u64 << exponent)
        .min(RECOVERY_BACKOFF_MAX_MS);
    manager.recovery_next_retry_at_ms = Some(now_unix_ms().saturating_add(backoff));
    manager.recovery_paused = manager.recovery_failure_count >= RECOVERY_FAILURE_PAUSE_THRESHOLD;
}

fn runtime_public_error(error: &AppError) -> CodexRetryGatewayError {
    match error.code() {
        value if value.contains("NODE") => public_node_error(error),
        value if value.contains("SOURCE") => public_source_error(error),
        value if value.contains("PORT") => {
            runtime_public_error_message(error.code(), &error.to_string())
                .with_category(CodexRetryGatewayErrorCategory::PortConflict)
        }
        value if value.contains("HEALTH") => {
            runtime_public_error_message(error.code(), &error.to_string())
                .with_category(CodexRetryGatewayErrorCategory::HealthTimeout)
        }
        value if value.contains("OWNERSHIP") => {
            runtime_public_error_message(error.code(), &error.to_string())
                .with_category(CodexRetryGatewayErrorCategory::OwnershipMismatch)
        }
        _ => runtime_public_error_message(error.code(), &error.to_string()),
    }
}

trait RuntimeErrorExt {
    fn with_category(self, category: CodexRetryGatewayErrorCategory) -> CodexRetryGatewayError;
}

impl RuntimeErrorExt for CodexRetryGatewayError {
    fn with_category(mut self, category: CodexRetryGatewayErrorCategory) -> CodexRetryGatewayError {
        self.category = category;
        self
    }
}

fn runtime_public_error_message(code: &str, rendered: &str) -> CodexRetryGatewayError {
    CodexRetryGatewayError {
        code: code.to_string(),
        category: CodexRetryGatewayErrorCategory::Internal,
        message: rendered
            .split_once(':')
            .map(|(_, message)| message.trim().to_string())
            .unwrap_or_else(|| rendered.to_string()),
        retryable: matches!(
            code,
            "CODEX_RETRY_GATEWAY_PORT_CONFLICT"
                | "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT"
                | "CODEX_RETRY_GATEWAY_SOURCE_TRANSIENT"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_runtime_phase_is_truthful_for_bypassed_enablement() {
        let reconcile = CodexRetryGatewayProcessReconcileResult {
            status: crate::infra::codex_retry_gateway::CodexRetryGatewayProcessStatus {
                phase: CodexRetryGatewayProcessPhase::Healthy,
                owned: true,
                healthy: true,
                process_id: Some(7),
                listener: Some("http://127.0.0.1:4610".to_string()),
            },
            managed: None,
            health: None,
            error: None,
        };
        assert_eq!(
            derive_runtime_phase(true, CodexRouteMode::DirectAio, &reconcile, false),
            CodexRetryGatewayRuntimePhase::BypassedRecovering
        );
    }

    #[test]
    fn record_runtime_failure_applies_backoff_and_pause_threshold() {
        let mut manager = CodexRetryGatewayManagerState::default();
        for _ in 0..RECOVERY_FAILURE_PAUSE_THRESHOLD {
            record_runtime_failure(
                &mut manager,
                runtime_public_error_message("CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT", "timeout"),
            );
        }
        assert!(manager.recovery_next_retry_at_ms.is_some());
        assert!(manager.recovery_paused);
    }
}
