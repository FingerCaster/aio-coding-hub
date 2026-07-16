use crate::infra::codex_retry_gateway::bridge::create_bridge_details_session;
use crate::infra::codex_retry_gateway::managed_state::{
    read_manager_state, write_manager_state, CodexRetryGatewayManagedProcessRecord,
    CodexRetryGatewayManagerPaths, CodexRetryGatewayManagerState,
};
use crate::infra::codex_retry_gateway::node::{public_node_error, resolve_node_status};
use crate::infra::codex_retry_gateway::process::{
    reconcile_pending_runtime_launch as reconcile_pending_process_launch,
    reconcile_runtime_process, start_runtime_process, stop_runtime_process,
    CodexRetryGatewayManagedProcess, CodexRetryGatewayProcessReconcileResult,
};
use crate::infra::codex_retry_gateway::source::{
    install_source_commit, official_commit_distance, public_source_error,
    resolve_official_main_candidate, revalidate_cached_source, validate_commit_request,
    CodexRetryGatewaySourceHttpConfig,
};
use crate::infra::codex_retry_gateway::util::{normalize_full_sha, now_unix_ms, strip_trailing_v1};
use crate::infra::codex_retry_gateway::{
    normalize_preferred_port, AioGatewayOrigin, CodexProviderSyncPlan,
    CodexRetryGatewayApplyCommitRequest, CodexRetryGatewayCommitValidation,
    CodexRetryGatewayDetailsSession, CodexRetryGatewayEnableConfirmation,
    CodexRetryGatewayEnablePlan, CodexRetryGatewayError, CodexRetryGatewayErrorCategory,
    CodexRetryGatewayGenerationRequest, CodexRetryGatewayLifecycleCallback,
    CodexRetryGatewayLifecycleFuture, CodexRetryGatewayNodeStatus, CodexRetryGatewayProcessPhase,
    CodexRetryGatewayRouteCallbackRequest, CodexRetryGatewayRuntimePhase,
    CodexRetryGatewaySetEnabledRequest, CodexRetryGatewaySetNodeOverrideRequest,
    CodexRetryGatewayStatus, CodexRetryGatewayTrustState, CodexRetryGatewayUninstallRequest,
    CodexRetryGatewayUpdateCandidate, CodexRetryGatewayValidateCommitRequest, CodexRouteMode,
    CODEX_RETRY_GATEWAY_DEFAULT_PORT, CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT,
};
use crate::shared::error::{AppError, AppResult};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

const RECOVERY_FAILURE_PAUSE_THRESHOLD: u32 = 5;
const RECOVERY_BACKOFF_BASE_MS: u64 = 5_000;
const RECOVERY_BACKOFF_MAX_MS: u64 = 5 * 60 * 1_000;

struct RuntimeFailClosedCallback;

impl CodexRetryGatewayLifecycleCallback for RuntimeFailClosedCallback {
    fn request_gateway_disable(
        &self,
        _request: CodexRetryGatewayRouteCallbackRequest,
    ) -> CodexRetryGatewayLifecycleFuture {
        Box::pin(async {
            Err(AppError::new(
                "CODEX_RETRY_GATEWAY_ROUTE_CALLBACK_UNAVAILABLE",
                "gateway lifecycle callback is not installed",
            ))
        })
    }
}

static LIFECYCLE_CALLBACK: OnceLock<RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>>> =
    OnceLock::new();

#[derive(Clone)]
struct RuntimeRollbackState {
    settings: crate::settings::AppSettings,
    manager: CodexRetryGatewayManagerState,
    restart_process: bool,
}

fn lifecycle_callback_slot() -> &'static RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>> {
    LIFECYCLE_CALLBACK.get_or_init(|| RwLock::new(Arc::new(RuntimeFailClosedCallback)))
}

pub(crate) async fn install_lifecycle_callback(
    callback: Arc<dyn CodexRetryGatewayLifecycleCallback>,
) {
    *lifecycle_callback_slot().write().await = callback;
}

pub(crate) async fn current_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexRetryGatewayStatus> {
    build_runtime_status(app, None).await
}

pub(crate) async fn reconcile_pending_runtime_launch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<()> {
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    reconcile_pending_process_launch(&paths).await?;
    Ok(())
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
        preferred_port: normalize_preferred_port(
            settings.codex_retry_gateway_preferred_port,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT,
        ),
        wsl_codex_unprotected: settings.wsl_auto_config && settings.wsl_target_cli.codex,
    })
}

pub(crate) async fn runtime_update_candidate<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Option<CodexRetryGatewayUpdateCandidate>> {
    let status = current_status(app).await?;
    let http = CodexRetryGatewaySourceHttpConfig::default();
    let selection = resolve_official_main_candidate(&http).await?;
    let current_commit = status
        .active_commit
        .clone()
        .or_else(|| Some(status.selected_commit.clone()));
    if current_commit.as_deref() == Some(selection.canonical_commit.as_str()) {
        return Ok(None);
    }
    let commits_ahead = match current_commit.as_deref() {
        Some(current) if normalize_full_sha(current).is_ok() => {
            official_commit_distance(current, &selection.canonical_commit, &http).await?
        }
        _ => None,
    };
    Ok(Some(CodexRetryGatewayUpdateCandidate {
        commit: selection.canonical_commit.clone(),
        current_commit: current_commit.clone(),
        previous_commit: status.previous_commit.clone(),
        rollback_commit: current_commit,
        official_main_commit: selection.official_main_commit,
        commits_ahead,
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
    let status = current_status(app).await?;
    if request.plan_generation != status.generation {
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
        let mut stop_error = None;
        if let Some(record) = manager.process_record.clone() {
            match stop_runtime_process(&paths, &record).await {
                Ok(true) => manager.process_record = None,
                Ok(false) => {
                    stop_error = Some(runtime_public_error_message(
                        "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                        "managed gateway stop could not be verified; cleanup is still required",
                    ));
                }
                Err(error) => stop_error = Some(runtime_public_error(&error)),
            }
        }
        manager.generation = manager.generation.saturating_add(1);
        manager.last_error = stop_error;
        manager.recovery_failure_count = 0;
        manager.recovery_next_retry_at_ms = None;
        manager.recovery_paused = false;
        write_manager_state(&paths, &manager)?;
        return build_runtime_status(app, Some(manager)).await;
    }

    let plan = build_enable_plan(app).await?;
    if request.plan_generation != plan.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "enable request generation no longer matches the latest runtime plan",
        ));
    }
    require_enable_confirmations(&plan, &request.confirmation)?;
    let rollback = RuntimeRollbackState {
        settings: settings.clone(),
        manager: manager.clone(),
        restart_process: false,
    };
    settings.codex_retry_gateway_enabled = true;

    match ensure_runtime_process(app, &settings, &paths, &mut manager).await {
        Ok(process) => {
            let cleanup_process = (rollback.manager.process_record.as_ref()
                != Some(&process.record))
            .then_some(&process);
            manager.generation = manager.generation.saturating_add(1);
            manager.active_commit = Some(process.record.source_commit.clone());
            manager.effective_port = parse_listener_port(&process.record.listener);
            manager.process_record = Some(process.record.clone());
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
            persist_runtime_transition(app, &paths, &settings, &manager, cleanup_process, rollback)
                .await?;
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

    let previous_settings = crate::settings::read(app)?;
    let mut settings = previous_settings.clone();
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    let previous_manager = manager.clone();
    let node = resolve_required_node(app, &settings.codex_retry_gateway_node_override)?;
    let installed = install_source_commit(
        &paths,
        &selection,
        &node,
        &CodexRetryGatewaySourceHttpConfig::default(),
    )
    .await?;
    let mut rollback = RuntimeRollbackState {
        settings: previous_settings,
        manager: previous_manager,
        restart_process: false,
    };

    manager.verified_main_commit = Some(installed.manifest.verified_main_commit.clone());
    let mut started_process = None;

    if settings.codex_retry_gateway_enabled {
        rollback.restart_process =
            stop_verified_record_for_change(&paths, &manager, "commit switch").await?;
        let aio_origin = aio_origin(app, &settings);
        let provider_name = current_managed_provider_name(app)?;
        let process = match start_runtime_process(
            &paths,
            &installed,
            &node,
            &aio_origin,
            &provider_name,
            normalize_preferred_port(
                settings.codex_retry_gateway_preferred_port,
                CODEX_RETRY_GATEWAY_DEFAULT_PORT,
            ),
            manager.effective_port,
        )
        .await
        {
            Ok(process) => process,
            Err(error) => {
                if rollback.restart_process {
                    let rollback_error = rollback_runtime_state(app, &paths, &rollback).await.err();
                    return Err(match rollback_error {
                        Some(rollback_error) => AppError::new(
                            error.code(),
                            format!("{error}; rollback failed: {rollback_error}"),
                        ),
                        None => error,
                    });
                }
                return Err(error);
            }
        };
        if manager.active_commit.as_deref() != Some(selection.canonical_commit.as_str()) {
            manager.previous_commit = manager.active_commit.clone();
        }
        manager.active_commit = Some(selection.canonical_commit.clone());
        manager.process_record = Some(process.record.clone());
        manager.effective_port = parse_listener_port(
            manager
                .process_record
                .as_ref()
                .map(|record| record.listener.as_str())
                .unwrap_or_default(),
        );
        started_process = Some(process);
    }

    settings.codex_retry_gateway_selected_commit = selection.canonical_commit.clone();
    manager.generation = manager.generation.saturating_add(1);
    manager.last_error = None;
    persist_runtime_transition(
        app,
        &paths,
        &settings,
        &manager,
        started_process.as_ref(),
        rollback,
    )
    .await?;
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
    let rollback = RuntimeRollbackState {
        settings: settings.clone(),
        manager: manager.clone(),
        restart_process: false,
    };
    manager.recovery_paused = false;
    manager.recovery_next_retry_at_ms = None;
    let mut cleanup_process = None;
    if settings.codex_retry_gateway_enabled {
        match ensure_runtime_process(app, &settings, &paths, &mut manager).await {
            Ok(process) => {
                cleanup_process = (rollback.manager.process_record.as_ref()
                    != Some(&process.record))
                .then_some(process.clone());
                manager.process_record = Some(process.record.clone());
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
    persist_runtime_transition(
        app,
        &paths,
        &settings,
        &manager,
        cleanup_process.as_ref(),
        rollback,
    )
    .await?;
    build_runtime_status(app, Some(manager)).await
}

pub(crate) async fn uninstall_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: CodexRetryGatewayUninstallRequest,
) -> AppResult<CodexRetryGatewayStatus> {
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
    ensure_runtime_uninstall_ready(&status)?;
    let mut settings = crate::settings::read(app)?;
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    if paths.root.exists() {
        std::fs::remove_dir_all(&paths.root).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_UNINSTALL_FAILED",
                format!(
                    "failed to remove runtime data root {}: {err}",
                    paths.root.display()
                ),
            )
        })?;
    }
    settings.codex_retry_gateway_enabled = false;
    settings.codex_retry_gateway_selected_commit =
        CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string();
    settings.codex_retry_gateway_preferred_port = CODEX_RETRY_GATEWAY_DEFAULT_PORT;
    settings.codex_retry_gateway_node_override.clear();
    crate::settings::write(app, &settings)?;
    current_status(app).await
}

pub(crate) fn ensure_runtime_uninstall_ready(status: &CodexRetryGatewayStatus) -> AppResult<()> {
    if status.desired_enabled {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_UNINSTALL_REQUIRES_DISABLED",
            "disable the managed gateway before uninstalling its data",
        ));
    }
    if status.route_mode == CodexRouteMode::Guarded {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_UNINSTALL_ROUTE_UNSAFE",
            "managed source cannot be removed while Codex targets the external gateway",
        ));
    }
    if status.process_status.phase != CodexRetryGatewayProcessPhase::Stopped {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_UNINSTALL_PROCESS_ACTIVE",
            "managed gateway process must be fully stopped before uninstalling its data",
        ));
    }
    Ok(())
}

pub(crate) async fn stop_runtime_for_shutdown<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexRetryGatewayStatus> {
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    let mut stop_error = None;
    if let Some(record) = manager.process_record.clone() {
        match stop_runtime_process(&paths, &record).await {
            Ok(true) => manager.process_record = None,
            Ok(false) => {
                stop_error = Some(runtime_public_error_message(
                    "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                    "managed gateway shutdown could not be verified",
                ));
            }
            Err(error) => stop_error = Some(runtime_public_error(&error)),
        }
    }
    manager.generation = manager.generation.saturating_add(1);
    manager.last_error = stop_error;
    write_manager_state(&paths, &manager)?;
    build_runtime_status(app, Some(manager)).await
}

pub(crate) fn runtime_recovery_due<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<bool> {
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let manager = read_manager_state(&paths)?;
    Ok(!manager.recovery_paused
        && manager
            .recovery_next_retry_at_ms
            .is_none_or(|retry_at| retry_at <= now_unix_ms()))
}

pub(crate) async fn record_runtime_recovery_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    error: &AppError,
) -> AppResult<CodexRetryGatewayStatus> {
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    record_runtime_failure(&mut manager, runtime_public_error(error));
    manager.generation = manager.generation.saturating_add(1);
    write_manager_state(&paths, &manager)?;
    build_runtime_status(app, Some(manager)).await
}

pub(crate) async fn rollback_selected_commit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    commit: &str,
) -> AppResult<CodexRetryGatewayStatus> {
    let commit = normalize_full_sha(commit)?;
    let previous_settings = crate::settings::read(app)?;
    let mut settings = previous_settings.clone();
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    let mut manager = read_manager_state(&paths)?;
    let previous_manager = manager.clone();
    let node = resolve_required_node(app, &settings.codex_retry_gateway_node_override)?;
    let installed = revalidate_cached_source(&paths, &commit)?.ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_RESOLUTION_FAILED",
            format!("cached rollback source {commit} is unavailable or invalid"),
        )
    })?;
    let mut rollback = RuntimeRollbackState {
        settings: previous_settings,
        manager: previous_manager,
        restart_process: false,
    };
    let mut started_process = None;

    if settings.codex_retry_gateway_enabled {
        rollback.restart_process =
            stop_verified_record_for_change(&paths, &manager, "route rollback").await?;
        let aio_origin = aio_origin(app, &settings);
        let provider_name = current_managed_provider_name(app)?;
        let process = match start_runtime_process(
            &paths,
            &installed,
            &node,
            &aio_origin,
            &provider_name,
            normalize_preferred_port(
                settings.codex_retry_gateway_preferred_port,
                CODEX_RETRY_GATEWAY_DEFAULT_PORT,
            ),
            manager.effective_port,
        )
        .await
        {
            Ok(process) => process,
            Err(error) => {
                let rollback_error = rollback_runtime_state(app, &paths, &rollback).await.err();
                return Err(match rollback_error {
                    Some(rollback_error) => AppError::new(
                        error.code(),
                        format!("{error}; failed to restore update candidate: {rollback_error}"),
                    ),
                    None => error,
                });
            }
        };
        manager.previous_commit = manager.active_commit.clone();
        manager.active_commit = Some(commit.clone());
        manager.process_record = Some(process.record.clone());
        manager.effective_port = parse_listener_port(&process.record.listener);
        started_process = Some(process);
    }

    settings.codex_retry_gateway_selected_commit = commit;
    manager.verified_main_commit = Some(installed.manifest.verified_main_commit.clone());
    manager.generation = manager.generation.saturating_add(1);
    manager.last_error = None;
    persist_runtime_transition(
        app,
        &paths,
        &settings,
        &manager,
        started_process.as_ref(),
        rollback,
    )
    .await?;
    build_runtime_status(app, Some(manager)).await
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
    let selected_commit_result =
        normalize_selected_commit(&settings.codex_retry_gateway_selected_commit);
    let selected_commit = selected_commit_result
        .as_ref()
        .cloned()
        .unwrap_or_else(|_| {
            settings
                .codex_retry_gateway_selected_commit
                .trim()
                .to_string()
        });
    let selected_commit_error = selected_commit_result
        .err()
        .map(|error| runtime_public_error(&error));
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

    let trust_state = if selected_commit_error.is_some() {
        CodexRetryGatewayTrustState::Unavailable
    } else if selected_commit == CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT {
        CodexRetryGatewayTrustState::AioReviewedRecommendation
    } else {
        CodexRetryGatewayTrustState::OfficialMainUnreviewed
    };
    let last_error = selected_commit_error.clone().or_else(|| {
        manager
            .last_error
            .clone()
            .or_else(|| reconcile.error.clone())
            .or_else(|| node_status.error.clone())
    });
    let runtime_phase = if selected_commit_error.is_some() {
        CodexRetryGatewayRuntimePhase::Error
    } else {
        derive_runtime_phase(
            settings.codex_retry_gateway_enabled,
            route_mode,
            &reconcile,
            manager.recovery_paused,
        )
    };
    let route_transition_pending = match std::fs::symlink_metadata(&paths.transition_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    let provider_sync_pending =
        crate::infra::codex_provider_sync::has_pending_provider_sync_recovery(app).unwrap_or(true);
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
        operation_pending: route_transition_pending
            || provider_sync_pending
            || manager.pending_launch.is_some(),
    })
}

async fn ensure_runtime_process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &crate::settings::AppSettings,
    paths: &CodexRetryGatewayManagerPaths,
    manager: &mut CodexRetryGatewayManagerState,
) -> AppResult<CodexRetryGatewayManagedProcess> {
    let node = resolve_required_node(app, &settings.codex_retry_gateway_node_override)?;
    let selected_commit = normalize_selected_commit(&settings.codex_retry_gateway_selected_commit)?;
    let provider_name = current_managed_provider_name(app)?;
    let aio_origin = aio_origin(app, settings);

    if let Some(record) = manager.process_record.as_ref() {
        let reconciled =
            reconcile_runtime_process(paths, Some(record), manager.effective_port).await?;
        if let Some(process) = reconciled.managed.as_ref() {
            let canonical_node = std::fs::canonicalize(&node.executable)
                .ok()
                .map(|path| path.display().to_string());
            let source_valid = revalidate_cached_source(paths, &selected_commit)?.is_some();
            if healthy_process_can_be_reused(
                &process.record,
                &selected_commit,
                canonical_node.as_deref(),
                &provider_name,
                &aio_origin.url,
                source_valid,
            ) {
                return Ok(process.clone());
            }
        }
        if reconciled.managed.is_some() {
            stop_runtime_process(paths, record).await?;
        }
    }

    let installed = if let Some(installed) = revalidate_cached_source(paths, &selected_commit)? {
        installed
    } else {
        let selection = validate_commit_request(
            &selected_commit,
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
        install_source_commit(
            paths,
            &selection,
            &node,
            &CodexRetryGatewaySourceHttpConfig::default(),
        )
        .await?
    };
    manager.verified_main_commit = Some(installed.manifest.verified_main_commit.clone());
    start_runtime_process(
        paths,
        &installed,
        &node,
        &aio_origin,
        &provider_name,
        normalize_preferred_port(
            settings.codex_retry_gateway_preferred_port,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT,
        ),
        manager.effective_port,
    )
    .await
}

fn healthy_process_can_be_reused(
    process: &CodexRetryGatewayManagedProcessRecord,
    selected_commit: &str,
    canonical_node: Option<&str>,
    provider_name: &str,
    aio_upstream_base_url: &str,
    source_valid: bool,
) -> bool {
    source_valid
        && process.source_commit == selected_commit
        && canonical_node.is_some_and(|path| process.node_executable == path)
        && process.provider_name == provider_name
        && process.upstream_base_url == aio_upstream_base_url
}

async fn stop_verified_record_for_change(
    paths: &CodexRetryGatewayManagerPaths,
    manager: &CodexRetryGatewayManagerState,
    context: &str,
) -> AppResult<bool> {
    let Some(record) = manager.process_record.as_ref() else {
        return Ok(false);
    };
    let reconciled = reconcile_runtime_process(paths, Some(record), manager.effective_port).await?;
    if reconciled.managed.is_some() {
        stop_runtime_process(paths, record).await?;
        return Ok(true);
    }
    if reconciled.status.phase == CodexRetryGatewayProcessPhase::Stopped {
        return Ok(false);
    }
    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
        format!("{context} requires a verified stop of the currently managed gateway process"),
    ))
}

async fn rollback_runtime_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: &CodexRetryGatewayManagerPaths,
    rollback: &RuntimeRollbackState,
) -> AppResult<()> {
    let mut manager = rollback.manager.clone();
    let mut restarted = None;
    if rollback.restart_process {
        if let Some(record) = rollback.manager.process_record.as_ref() {
            let node =
                resolve_required_node(app, &rollback.settings.codex_retry_gateway_node_override)?;
            let installed =
                revalidate_cached_source(paths, &record.source_commit)?.ok_or_else(|| {
                    AppError::new(
                        "CODEX_RETRY_GATEWAY_SOURCE_RESOLUTION_FAILED",
                        format!(
                            "failed to revalidate cached source for rollback commit {}",
                            record.source_commit
                        ),
                    )
                })?;
            let aio_origin = aio_origin(app, &rollback.settings);
            let provider_name = current_managed_provider_name(app)?;
            let process = start_runtime_process(
                paths,
                &installed,
                &node,
                &aio_origin,
                &provider_name,
                normalize_preferred_port(
                    rollback.settings.codex_retry_gateway_preferred_port,
                    CODEX_RETRY_GATEWAY_DEFAULT_PORT,
                ),
                rollback.manager.effective_port,
            )
            .await?;
            manager.process_record = Some(process.record.clone());
            manager.active_commit = Some(process.record.source_commit.clone());
            manager.effective_port = parse_listener_port(&process.record.listener);
            manager.last_error = None;
            restarted = Some(process);
        }
    }
    if let Err(error) = crate::settings::write(app, &rollback.settings) {
        if let Some(process) = restarted.as_ref() {
            let _ = stop_runtime_process(paths, &process.record).await;
        }
        return Err(error);
    }
    if let Err(error) = write_manager_state(paths, &manager) {
        if let Some(process) = restarted.as_ref() {
            let _ = stop_runtime_process(paths, &process.record).await;
        }
        return Err(error);
    }
    Ok(())
}

async fn persist_runtime_transition<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: &CodexRetryGatewayManagerPaths,
    settings: &crate::settings::AppSettings,
    manager: &CodexRetryGatewayManagerState,
    started_process: Option<&CodexRetryGatewayManagedProcess>,
    rollback: RuntimeRollbackState,
) -> AppResult<()> {
    if let Err(error) = crate::settings::write(app, settings) {
        if let Some(process) = started_process {
            let _ = stop_runtime_process(paths, &process.record).await;
        }
        let rollback_error = rollback_runtime_state(app, paths, &rollback).await.err();
        return Err(match rollback_error {
            Some(rollback_error) => AppError::new(
                error.code(),
                format!("{error}; rollback failed: {rollback_error}"),
            ),
            None => error,
        });
    }
    if let Err(error) = write_manager_state(paths, manager) {
        if let Some(process) = started_process {
            let _ = stop_runtime_process(paths, &process.record).await;
        }
        let rollback_error = rollback_runtime_state(app, paths, &rollback).await.err();
        return Err(match rollback_error {
            Some(rollback_error) => AppError::new(
                error.code(),
                format!("{error}; rollback failed: {rollback_error}"),
            ),
            None => error,
        });
    }
    Ok(())
}

fn resolve_required_node<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manual_override: &str,
) -> AppResult<crate::infra::codex_retry_gateway::CodexRetryGatewayResolvedNode> {
    crate::infra::codex_retry_gateway::resolve_node_runtime(app, Some(manual_override))
}

fn current_managed_provider_name<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<String> {
    let config = crate::infra::codex_config::codex_config_get(app)?;
    Ok(if config.features_remote_compaction == Some(true) {
        crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_OPENAI.to_string()
    } else {
        crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO.to_string()
    })
}

pub(crate) fn require_enable_confirmations(
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

fn normalize_selected_commit(value: &str) -> AppResult<String> {
    normalize_full_sha(value).map_err(|_| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_SELECTION_INVALID",
            "persisted selected commit must be a full 40-hex SHA",
        )
    })
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
            normalize_preferred_port(
                settings.preferred_port,
                crate::settings::DEFAULT_GATEWAY_PORT,
            )
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
    _effective_port: Option<u16>,
    aio_origin: &str,
) -> CliProxyProjection {
    let aio_base_origin = strip_trailing_v1(aio_origin);
    let row = crate::cli_proxy::status_all(app, Some(&aio_base_origin))
        .ok()
        .and_then(|rows| rows.into_iter().find(|row| row.cli_key == "codex"));

    let enabled = row.as_ref().map(|row| row.enabled).unwrap_or(false);
    let applied = row
        .as_ref()
        .and_then(|row| row.applied_to_current_gateway)
        .unwrap_or(false);
    let route_mode = truthful_route_mode(
        enabled,
        applied,
        row.as_ref().and_then(|row| row.route_mode),
    );
    CliProxyProjection {
        enabled,
        applied,
        route_mode,
    }
}

fn truthful_route_mode(
    enabled: bool,
    applied: bool,
    advertised: Option<CodexRouteMode>,
) -> CodexRouteMode {
    if enabled && applied {
        advertised.unwrap_or(CodexRouteMode::DirectAio)
    } else {
        CodexRouteMode::Unproxied
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
    use crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord;

    #[test]
    fn invalid_persisted_commit_is_not_replaced_with_recommendation() {
        let error = normalize_selected_commit("not-a-full-sha").unwrap_err();
        assert_eq!(error.code(), "CODEX_RETRY_GATEWAY_SOURCE_SELECTION_INVALID");
        assert_ne!("not-a-full-sha", CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT);
    }
    use axum::routing::get;
    use axum::{Json, Router};
    use std::sync::{Mutex as StdMutex, OnceLock as StdOnceLock};
    use tauri::Manager;
    use tempfile::TempDir;

    fn runtime_test_lock() -> &'static StdMutex<()> {
        static LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();
        LOCK.get_or_init(|| StdMutex::new(()))
    }

    struct RuntimeTestContext {
        _guard: std::sync::MutexGuard<'static, ()>,
        _home: TempDir,
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        paths: CodexRetryGatewayManagerPaths,
    }

    impl RuntimeTestContext {
        fn new() -> Self {
            let guard = runtime_test_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let home = tempfile::tempdir().unwrap();
            std::env::set_var("AIO_CODING_HUB_TEST_HOME", home.path());
            let app = tauri::test::mock_app().handle().clone();
            app.manage(crate::app::gateway_state::GatewayState::default());
            let paths = CodexRetryGatewayManagerPaths::from_app(&app).unwrap();
            paths.ensure_dirs().unwrap();
            crate::settings::write(&app, &crate::settings::AppSettings::default()).unwrap();
            Self {
                _guard: guard,
                _home: home,
                app,
                paths,
            }
        }
    }

    impl Drop for RuntimeTestContext {
        fn drop(&mut self) {
            std::env::remove_var("AIO_CODING_HUB_TEST_HOME");
        }
    }

    fn managed_record(
        paths: &CodexRetryGatewayManagerPaths,
        listener: &str,
    ) -> CodexRetryGatewayManagedProcessRecord {
        CodexRetryGatewayManagedProcessRecord {
            pid: 4242,
            start_identity: Some(77),
            started_at_ms: 1,
            node_executable: "C:/node.exe".to_string(),
            source_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string(),
            source_dir_rel: format!("sources/{}", CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT),
            config_path_rel: relative_runtime_path(&paths.runtime_config_path, &paths.root),
            state_path_rel: relative_runtime_path(&paths.runtime_state_path, &paths.root),
            log_path_rel: relative_runtime_path(&paths.runtime_log_path, &paths.root),
            listener: listener.to_string(),
            upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
            instance_nonce: "nonce".to_string(),
            provider_name: crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO
                .to_string(),
        }
    }

    fn relative_runtime_path(path: &std::path::Path, root: &std::path::Path) -> String {
        path.strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    }

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
    fn guarded_status_requires_enabled_and_verified_live_projection() {
        assert_eq!(
            truthful_route_mode(true, true, Some(CodexRouteMode::Guarded)),
            CodexRouteMode::Guarded
        );
        assert_eq!(
            truthful_route_mode(true, false, Some(CodexRouteMode::Guarded)),
            CodexRouteMode::Unproxied
        );
        assert_eq!(
            truthful_route_mode(false, true, Some(CodexRouteMode::Guarded)),
            CodexRouteMode::Unproxied
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

    #[tokio::test]
    async fn status_exposes_invalid_persisted_commit_without_substitution() {
        let ctx = RuntimeTestContext::new();
        let settings = crate::settings::AppSettings {
            codex_retry_gateway_enabled: true,
            codex_retry_gateway_selected_commit: "invalid-selection".to_string(),
            ..Default::default()
        };
        crate::settings::write(&ctx.app, &settings).unwrap();

        let status = current_status(&ctx.app).await.unwrap();
        assert_eq!(status.selected_commit, "invalid-selection");
        assert_eq!(status.trust_state, CodexRetryGatewayTrustState::Unavailable);
        assert_eq!(status.runtime_phase, CodexRetryGatewayRuntimePhase::Error);
        assert_eq!(
            status.last_error.as_ref().map(|error| error.code.as_str()),
            Some("CODEX_RETRY_GATEWAY_SOURCE_SELECTION_INVALID")
        );
    }

    #[test]
    fn healthy_process_reuse_requires_revalidated_source() {
        let commit = CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT;
        let root = tempfile::tempdir().expect("runtime root");
        let paths = CodexRetryGatewayManagerPaths::from_root(root.path().join("gateway"));
        let mut record = managed_record(&paths, "http://127.0.0.1:4610");
        assert!(healthy_process_can_be_reused(
            &record,
            commit,
            Some("C:/node.exe"),
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO,
            "http://127.0.0.1:37123/v1",
            true,
        ));
        assert!(!healthy_process_can_be_reused(
            &record,
            commit,
            Some("C:/node.exe"),
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO,
            "http://127.0.0.1:37123/v1",
            false,
        ));
        record.provider_name =
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_OPENAI.to_string();
        assert!(!healthy_process_can_be_reused(
            &record,
            commit,
            Some("C:/node.exe"),
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO,
            "http://127.0.0.1:37123/v1",
            true,
        ));
        record.provider_name =
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO.to_string();
        record.upstream_base_url = "http://127.0.0.1:37124/v1".to_string();
        assert!(!healthy_process_can_be_reused(
            &record,
            commit,
            Some("C:/node.exe"),
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO,
            "http://127.0.0.1:37123/v1",
            true,
        ));
    }

    #[test]
    fn recovery_due_honors_backoff_and_pause() {
        let ctx = RuntimeTestContext::new();
        let mut manager = CodexRetryGatewayManagerState {
            recovery_next_retry_at_ms: Some(now_unix_ms().saturating_add(60_000)),
            ..Default::default()
        };
        write_manager_state(&ctx.paths, &manager).unwrap();
        assert!(!runtime_recovery_due(&ctx.app).unwrap());

        manager.recovery_next_retry_at_ms = Some(now_unix_ms().saturating_sub(1));
        write_manager_state(&ctx.paths, &manager).unwrap();
        assert!(runtime_recovery_due(&ctx.app).unwrap());

        manager.recovery_paused = true;
        write_manager_state(&ctx.paths, &manager).unwrap();
        assert!(!runtime_recovery_due(&ctx.app).unwrap());
    }

    #[tokio::test]
    async fn disable_ignores_corrupt_cache_and_preserves_unverified_process_record() {
        let ctx = RuntimeTestContext::new();
        let settings = crate::settings::AppSettings {
            codex_retry_gateway_enabled: true,
            ..Default::default()
        };
        crate::settings::write(&ctx.app, &settings).unwrap();

        let source_dir = ctx
            .paths
            .source_dir(CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT)
            .unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(source_dir.join("manifest.json"), b"{").unwrap();

        let manager = CodexRetryGatewayManagerState {
            generation: 7,
            process_record: Some(managed_record(&ctx.paths, "http://127.0.0.1:4610")),
            ..Default::default()
        };
        write_manager_state(&ctx.paths, &manager).unwrap();

        let status = set_runtime_enabled(
            &ctx.app,
            CodexRetryGatewaySetEnabledRequest {
                enabled: false,
                plan_generation: manager.generation,
                confirmation: CodexRetryGatewayEnableConfirmation::default(),
            },
        )
        .await
        .unwrap();
        let after = read_manager_state(&ctx.paths).unwrap();
        assert!(!status.desired_enabled);
        assert!(after.process_record.is_some());
    }

    #[tokio::test]
    async fn uninstall_rejects_enabled_state_without_deleting_data() {
        let ctx = RuntimeTestContext::new();
        let settings = crate::settings::AppSettings {
            codex_retry_gateway_enabled: true,
            ..Default::default()
        };
        crate::settings::write(&ctx.app, &settings).unwrap();
        std::fs::create_dir_all(&ctx.paths.root).unwrap();
        std::fs::write(ctx.paths.root.join("sentinel.txt"), "keep").unwrap();

        let err = uninstall_runtime(
            &ctx.app,
            CodexRetryGatewayUninstallRequest {
                generation: current_status(&ctx.app).await.unwrap().generation,
                confirmed_data_removal: true,
            },
        )
        .await
        .expect_err("enabled desired state must block uninstall");
        assert_eq!(
            err.code(),
            "CODEX_RETRY_GATEWAY_UNINSTALL_REQUIRES_DISABLED"
        );
        assert!(ctx.paths.root.join("sentinel.txt").exists());
    }

    #[tokio::test]
    async fn uninstall_fails_without_deleting_data_when_process_is_not_stopped() {
        let ctx = RuntimeTestContext::new();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let listener_url = format!("http://127.0.0.1:{port}");

        let health_body = serde_json::json!({
            "ok": true,
            "listen": format!("127.0.0.1:{port}"),
            "upstream_base_url": "http://127.0.0.1:37123/v1",
            "ui_path": "/__codex_retry_gateway/ui"
        });
        let status_body = serde_json::json!({
            "ok": true,
            "listen": format!("127.0.0.1:{port}"),
            "state": {
                "process_id": 9999,
                "original_base_url": "http://127.0.0.1:37123/v1",
                "gateway_base_url": listener_url.clone(),
                "aio_instance_nonce": "nonce",
                "provider_name": "aio"
            },
            "paths": {
                "config_path": "C:/fake/config.json",
                "state_path": "C:/fake/state.json",
                "state_root": "C:/fake/runtime",
                "log_path": "C:/fake/gateway.log"
            }
        });

        let server = tauri::async_runtime::spawn(async move {
            let router = Router::new()
                .route(
                    "/__codex_retry_gateway/health",
                    get(move || {
                        let body = health_body.clone();
                        async move { Json(body) }
                    }),
                )
                .route(
                    "/__codex_retry_gateway/api/status",
                    get(move || {
                        let body = status_body.clone();
                        async move { Json(body) }
                    }),
                );
            let _ = axum::serve(listener, router).await;
        });

        let settings = crate::settings::AppSettings::default();
        crate::settings::write(&ctx.app, &settings).unwrap();
        let manager = CodexRetryGatewayManagerState {
            generation: 3,
            process_record: Some(managed_record(&ctx.paths, &listener_url)),
            ..Default::default()
        };
        write_manager_state(&ctx.paths, &manager).unwrap();
        std::fs::write(ctx.paths.root.join("sentinel.txt"), "keep").unwrap();

        let err = uninstall_runtime(
            &ctx.app,
            CodexRetryGatewayUninstallRequest {
                generation: current_status(&ctx.app).await.unwrap().generation,
                confirmed_data_removal: true,
            },
        )
        .await
        .expect_err("non-stopped process state must block uninstall");
        assert_eq!(err.code(), "CODEX_RETRY_GATEWAY_UNINSTALL_PROCESS_ACTIVE");
        assert!(ctx.paths.root.exists());
        server.abort();
    }

    #[tokio::test]
    async fn uninstall_removes_only_disabled_stopped_runtime_data() {
        let ctx = RuntimeTestContext::new();
        let settings = crate::settings::AppSettings {
            codex_retry_gateway_enabled: false,
            codex_retry_gateway_selected_commit: "1".repeat(40),
            codex_retry_gateway_preferred_port: 4620,
            codex_retry_gateway_node_override: "C:/Tools/node.exe".to_string(),
            ..Default::default()
        };
        crate::settings::write(&ctx.app, &settings).unwrap();
        std::fs::create_dir_all(&ctx.paths.root).unwrap();
        std::fs::write(ctx.paths.root.join("sentinel.txt"), "remove").unwrap();

        let result = uninstall_runtime(
            &ctx.app,
            CodexRetryGatewayUninstallRequest {
                generation: current_status(&ctx.app).await.unwrap().generation,
                confirmed_data_removal: true,
            },
        )
        .await
        .expect("disabled and stopped runtime should uninstall");

        assert!(!ctx.paths.root.exists());
        assert!(!result.desired_enabled);
        let settings = crate::settings::read(&ctx.app).unwrap();
        assert_eq!(
            settings.codex_retry_gateway_selected_commit,
            CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT
        );
        assert_eq!(
            settings.codex_retry_gateway_preferred_port,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT
        );
        assert!(settings.codex_retry_gateway_node_override.is_empty());
    }
}
