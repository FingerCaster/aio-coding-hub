//! Application-layer coordinator for the managed external Codex retry gateway.

use super::app_state::{ensure_db_ready, DbInitState};
use crate::infra::codex_retry_gateway::{
    self, normalize_preferred_port, CodexRetryGatewayApplyCommitRequest,
    CodexRetryGatewayCommitValidation, CodexRetryGatewayDetailsSession,
    CodexRetryGatewayEnablePlan, CodexRetryGatewayGenerationRequest,
    CodexRetryGatewayLifecycleCallback, CodexRetryGatewayLifecycleFuture,
    CodexRetryGatewayNodeStatus, CodexRetryGatewayOperationKind,
    CodexRetryGatewayRevokeDetailsSessionRequest, CodexRetryGatewayRouteCallbackRequest,
    CodexRetryGatewaySetEnabledRequest, CodexRetryGatewaySetEnabledResult,
    CodexRetryGatewaySetNodeOverrideRequest, CodexRetryGatewayStatus,
    CodexRetryGatewayStatusFuture, CodexRetryGatewayUninstallRequest,
    CodexRetryGatewayUpdateCandidate, CodexRetryGatewayValidateCommitRequest, CodexRouteMode,
    CODEX_RETRY_GATEWAY_DEFAULT_PORT, CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME,
};
use crate::shared::error::{AppError, AppResult};
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::{oneshot, Mutex};

const SUPERVISOR_INTERVAL: Duration = Duration::from_secs(10);
const SUPERVISOR_STOP_TIMEOUT: Duration = Duration::from_secs(2);

struct PreparedEnablePlan {
    public: CodexRetryGatewayEnablePlan,
    runtime_generation: u64,
}

struct HealthSupervisor {
    shutdown: Option<oneshot::Sender<()>>,
    task: tauri::async_runtime::JoinHandle<()>,
}

static HEALTH_SUPERVISOR: OnceLock<Mutex<Option<HealthSupervisor>>> = OnceLock::new();

fn health_supervisor_slot() -> &'static Mutex<Option<HealthSupervisor>> {
    HEALTH_SUPERVISOR.get_or_init(|| Mutex::new(None))
}

struct AppLifecycleCallback {
    app: tauri::AppHandle,
}

impl CodexRetryGatewayLifecycleCallback for AppLifecycleCallback {
    fn request_gateway_disable(
        &self,
        request: CodexRetryGatewayRouteCallbackRequest,
    ) -> CodexRetryGatewayLifecycleFuture {
        let app = self.app.clone();
        Box::pin(async move {
            let _lifecycle = super::gateway_lifecycle_lock::lock().await;
            disable_gateway_unlocked(
                &app,
                Some(request.generation),
                CodexRetryGatewayOperationKind::ExternalRestore,
            )
            .await?;
            Ok(())
        })
    }

    fn current_gateway_status(&self) -> CodexRetryGatewayStatusFuture {
        let app = self.app.clone();
        Box::pin(async move {
            let _lifecycle = super::gateway_lifecycle_lock::lock().await;
            codex_retry_gateway::current_status(&app).await
        })
    }
}

pub(crate) async fn install_lifecycle_callback(app: &tauri::AppHandle) {
    codex_retry_gateway::install_lifecycle_callback(Arc::new(AppLifecycleCallback {
        app: app.clone(),
    }))
    .await;
}

pub(crate) async fn status(app: &tauri::AppHandle) -> AppResult<CodexRetryGatewayStatus> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    codex_retry_gateway::current_status(app).await
}

pub(crate) async fn enable_plan(app: &tauri::AppHandle) -> AppResult<CodexRetryGatewayEnablePlan> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    Ok(prepare_enable_plan_unlocked(app).await?.public)
}

pub(crate) async fn set_enabled(
    app: &tauri::AppHandle,
    request: CodexRetryGatewaySetEnabledRequest,
) -> AppResult<CodexRetryGatewaySetEnabledResult> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    if request.enabled {
        enable_gateway_unlocked(app, request).await
    } else {
        let status = disable_gateway_unlocked(
            app,
            Some(request.plan_generation),
            CodexRetryGatewayOperationKind::DisableGateway,
        )
        .await?;
        Ok(CodexRetryGatewaySetEnabledResult {
            status,
            provider_sync: None,
        })
    }
}

pub(crate) async fn check_update(
    app: &tauri::AppHandle,
) -> AppResult<Option<CodexRetryGatewayUpdateCandidate>> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    codex_retry_gateway::runtime_update_candidate(app).await
}

pub(crate) async fn validate_commit(
    request: CodexRetryGatewayValidateCommitRequest,
) -> AppResult<CodexRetryGatewayCommitValidation> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    codex_retry_gateway::validate_selected_commit(request).await
}

pub(crate) async fn apply_commit(
    app: &tauri::AppHandle,
    request: CodexRetryGatewayApplyCommitRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let before = codex_retry_gateway::current_status(app).await?;
    if request.plan_generation != before.generation {
        return Err(stale_generation_error(
            request.plan_generation,
            before.generation,
        ));
    }
    let prior_selected_commit = before
        .active_commit
        .clone()
        .unwrap_or_else(|| before.selected_commit.clone());

    if before.desired_enabled {
        let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
        route_direct_aio_unlocked(
            app,
            &aio_origin,
            true,
            CodexRetryGatewayOperationKind::Update,
        )
        .await?;
    }

    let switched = match codex_retry_gateway::apply_selected_commit(app, request).await {
        Ok(status) => status,
        Err(error) => {
            if before.desired_enabled {
                let _ =
                    route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Update)
                        .await;
                emit_current_status(app).await;
            }
            return Err(error);
        }
    };

    if before.desired_enabled {
        if let Err(route_error) =
            route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Update).await
        {
            let rollback =
                codex_retry_gateway::rollback_selected_commit(app, &prior_selected_commit).await;
            let rollback_route = if rollback.is_ok() {
                route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Update)
                    .await
                    .err()
            } else {
                None
            };
            emit_current_status(app).await;
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_UPDATE_ROLLBACK_FAILED",
                match (rollback.err(), rollback_route) {
                    (None, None) => format!(
                        "candidate route verification failed and the prior commit was restored: {route_error}"
                    ),
                    (Some(rollback_error), _) => format!(
                        "candidate route verification failed: {route_error}; process rollback failed: {rollback_error}"
                    ),
                    (None, Some(rollback_route_error)) => format!(
                        "candidate route verification failed: {route_error}; prior process was restored but guarded route restoration failed: {rollback_route_error}"
                    ),
                },
            ));
        }
    }

    let result = if before.desired_enabled {
        codex_retry_gateway::current_status(app).await?
    } else {
        switched
    };
    emit_status(app, result.clone());
    Ok(result)
}

pub(crate) async fn set_node_override(
    app: &tauri::AppHandle,
    request: CodexRetryGatewaySetNodeOverrideRequest,
) -> AppResult<CodexRetryGatewayNodeStatus> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let status = codex_retry_gateway::set_runtime_node_override(app, request).await?;
    emit_current_status(app).await;
    Ok(status)
}

pub(crate) async fn retry(
    app: &tauri::AppHandle,
    request: CodexRetryGatewayGenerationRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let before = codex_retry_gateway::current_status(app).await?;
    if request.generation != before.generation {
        return Err(stale_generation_error(
            request.generation,
            before.generation,
        ));
    }
    if !before.desired_enabled {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_RECOVERY_NOT_DESIRED",
            "gateway recovery requires desired state to be enabled",
        ));
    }
    let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
    route_direct_aio_unlocked(
        app,
        &aio_origin,
        true,
        CodexRetryGatewayOperationKind::Recover,
    )
    .await?;
    let recovered = codex_retry_gateway::retry_runtime_recovery(app, request).await?;
    let result = if recovered.process_status.healthy {
        route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Recover).await?;
        codex_retry_gateway::current_status(app).await?
    } else {
        recovered
    };
    emit_status(app, result.clone());
    Ok(result)
}

pub(crate) async fn uninstall(
    app: &tauri::AppHandle,
    request: CodexRetryGatewayUninstallRequest,
) -> AppResult<CodexRetryGatewayStatus> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let before = codex_retry_gateway::current_status(app).await?;
    if request.generation != before.generation {
        return Err(stale_generation_error(
            request.generation,
            before.generation,
        ));
    }
    if !request.confirmed_data_removal {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_CONFIRMATION_REQUIRED",
            "runtime uninstall requires explicit data removal confirmation",
        ));
    }
    codex_retry_gateway::ensure_runtime_uninstall_ready(&before)?;

    let route = crate::app::cli_proxy_service::cli_proxy_codex_verify_route(app.clone())
        .await
        .map_err(AppError::from)?;
    ensure_uninstall_route_safe(&route)?;
    let result = codex_retry_gateway::uninstall_runtime(
        app,
        CodexRetryGatewayUninstallRequest {
            generation: before.generation,
            confirmed_data_removal: true,
        },
    )
    .await?;
    emit_status(app, result.clone());
    Ok(result)
}

fn ensure_uninstall_route_safe(route: &crate::cli_proxy::CodexRouteVerifyResult) -> AppResult<()> {
    if route.desired_enabled
        || route.route_mode == CodexRouteMode::Guarded
        || !route.live_matches_projection
        || !route.auth_matches_projection
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_UNINSTALL_ROUTE_UNSAFE",
            "managed source cannot be removed until Codex has a verified non-guarded route",
        ));
    }
    Ok(())
}

pub(crate) async fn details_session(
    app: &tauri::AppHandle,
) -> AppResult<CodexRetryGatewayDetailsSession> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    codex_retry_gateway::create_details_session(app).await
}

pub(crate) async fn revoke_details_session(
    request: CodexRetryGatewayRevokeDetailsSessionRequest,
) -> AppResult<()> {
    codex_retry_gateway::revoke_details_session(request).await
}

pub(crate) async fn reconcile_startup(app: &tauri::AppHandle) {
    install_lifecycle_callback(app).await;
    {
        let _lifecycle = super::gateway_lifecycle_lock::lock().await;
        if let Err(error) = reconcile_startup_unlocked(app).await {
            tracing::warn!(error = %error, "Codex retry gateway startup reconciliation failed");
            match codex_retry_gateway::record_runtime_recovery_failure(app, &error).await {
                Ok(status) => emit_status(app, status),
                Err(record_error) => {
                    tracing::warn!(error = %record_error, "failed to persist startup recovery failure");
                    emit_current_status(app).await;
                }
            }
        }
    }
    start_health_supervisor(app.clone()).await;
}

pub(crate) async fn reconcile_after_aio_start_unlocked(app: &tauri::AppHandle) -> AppResult<()> {
    reconcile_desired_runtime_unlocked(app, CodexRetryGatewayOperationKind::Startup).await
}

pub(crate) async fn shutdown_for_aio_stop_unlocked(app: &tauri::AppHandle) -> AppResult<()> {
    let status = codex_retry_gateway::current_status(app).await?;
    let route = crate::app::cli_proxy_service::cli_proxy_codex_verify_route(app.clone())
        .await
        .map_err(AppError::from)?;
    if route.cli_proxy_enabled || route.route_mode != CodexRouteMode::Unproxied {
        crate::app::cli_proxy_service::cli_proxy_codex_restore_unproxied_route_for_operation(
            app.clone(),
            None,
            crate::cli_proxy::CodexRestoreUnproxiedRouteRequest {
                expected_generation: route.generation,
                expected_canonical_sha256: route.canonical_config_sha256,
                aio_origin: route.aio_origin,
                desired_enabled: status.desired_enabled,
                keep_cli_proxy_enabled: true,
                source_commit: status.active_commit.clone(),
                process_should_run: false,
            },
            CodexRetryGatewayOperationKind::Shutdown,
        )
        .await
        .map_err(AppError::from)?;
    }
    let stopped = codex_retry_gateway::stop_runtime_for_shutdown(app).await?;
    emit_status(app, stopped);
    Ok(())
}

pub(crate) async fn stop_health_supervisor() {
    let mut guard = health_supervisor_slot().lock().await;
    if let Some(supervisor) = guard.take() {
        stop_health_supervisor_instance(supervisor).await;
    }
}

async fn stop_health_supervisor_instance(mut supervisor: HealthSupervisor) {
    if let Some(shutdown) = supervisor.shutdown.take() {
        let _ = shutdown.send(());
    }
    if tokio::time::timeout(SUPERVISOR_STOP_TIMEOUT, &mut supervisor.task)
        .await
        .is_err()
    {
        supervisor.task.abort();
        let _ = supervisor.task.await;
    }
}

pub(crate) async fn disable_codex_cli_proxy<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
) -> Result<crate::cli_proxy::CliProxyResult, String> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let previous_settings = crate::settings::read(&app).map_err(String::from)?;
    let status = codex_retry_gateway::current_status(&app)
        .await
        .map_err(String::from)?;
    let route = crate::app::cli_proxy_service::cli_proxy_codex_verify_route(app.clone()).await?;

    let mut desired_off = previous_settings.clone();
    desired_off.codex_retry_gateway_enabled = false;
    crate::settings::write(&app, &desired_off).map_err(String::from)?;
    let restored =
        crate::app::cli_proxy_service::cli_proxy_codex_restore_unproxied_route_for_operation(
            app.clone(),
            db_state,
            crate::cli_proxy::CodexRestoreUnproxiedRouteRequest {
                expected_generation: route.generation,
                expected_canonical_sha256: route.canonical_config_sha256,
                aio_origin: route.aio_origin,
                desired_enabled: false,
                keep_cli_proxy_enabled: false,
                source_commit: status.active_commit,
                process_should_run: true,
            },
            CodexRetryGatewayOperationKind::DisableCliProxy,
        )
        .await;
    let restored = match restored {
        Ok(restored) => restored,
        Err(error) => {
            let _ = crate::settings::write(&app, &previous_settings);
            return Err(error);
        }
    };
    let stopped = codex_retry_gateway::set_runtime_enabled(
        &app,
        CodexRetryGatewaySetEnabledRequest {
            enabled: false,
            plan_generation: status.generation,
            confirmation: Default::default(),
        },
    )
    .await
    .map_err(String::from)?;
    emit_status(&app, stopped);
    Ok(crate::cli_proxy::CliProxyResult {
        trace_id: restored.transition_operation_id,
        cli_key: "codex".to_string(),
        enabled: false,
        ok: true,
        error_code: None,
        message: "Codex CLI proxy disabled after restoring the unproxied route".to_string(),
        base_origin: restored.route.effective_origin,
    })
}

async fn enable_gateway_unlocked(
    app: &tauri::AppHandle,
    request: CodexRetryGatewaySetEnabledRequest,
) -> AppResult<CodexRetryGatewaySetEnabledResult> {
    let prepared = prepare_enable_plan_unlocked(app).await?;
    if request.plan_generation != prepared.public.generation {
        return Err(stale_generation_error(
            request.plan_generation,
            prepared.public.generation,
        ));
    }
    codex_retry_gateway::require_enable_confirmations(&prepared.public, &request.confirmation)?;

    let _ = ensure_aio_gateway_running_unlocked(app).await?;
    let prepared = prepare_enable_plan_unlocked(app).await?;
    if request.plan_generation != prepared.public.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_STALE_GENERATION",
            "enable plan changed after the AIO gateway became ready; request a new plan",
        ));
    }
    codex_retry_gateway::require_enable_confirmations(&prepared.public, &request.confirmation)?;
    if prepared.public.provider_sync.change_required {
        crate::blocking::run("codex_retry_gateway_provider_sync_preflight", || {
            crate::infra::codex_provider_sync::codex_provider_sync_preflight()
        })
        .await?;
    }

    let runtime_rollback = codex_retry_gateway::capture_runtime_enable_rollback(app).await?;
    let enable_result = start_then_verified_route(
        || {
            codex_retry_gateway::set_runtime_enabled(
                app,
                CodexRetryGatewaySetEnabledRequest {
                    enabled: true,
                    plan_generation: prepared.runtime_generation,
                    confirmation: request.confirmation,
                },
            )
        },
        || route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Enable),
        || codex_retry_gateway::rollback_runtime_enable(app, runtime_rollback),
    )
    .await;
    let (_, provider_sync) = match enable_result {
        Ok(result) => result,
        Err(error) => {
            emit_current_status(app).await;
            return Err(error);
        }
    };
    let result = codex_retry_gateway::current_status(app).await?;
    if result.route_mode != CodexRouteMode::Guarded || !result.process_status.healthy {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_VERIFY_FAILED",
            "enable completed without a verified guarded route",
        ));
    }
    emit_status(app, result.clone());
    Ok(CodexRetryGatewaySetEnabledResult {
        status: result,
        provider_sync,
    })
}

async fn disable_gateway_unlocked(
    app: &tauri::AppHandle,
    expected_generation: Option<u64>,
    operation_kind: CodexRetryGatewayOperationKind,
) -> AppResult<CodexRetryGatewayStatus> {
    let before = codex_retry_gateway::current_status(app).await?;
    if let Some(expected) = expected_generation {
        if expected != before.generation {
            return Err(stale_generation_error(expected, before.generation));
        }
    }
    let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
    let result = safe_route_then_runtime(
        || route_direct_aio_unlocked(app, &aio_origin, false, operation_kind),
        || {
            codex_retry_gateway::set_runtime_enabled(
                app,
                CodexRetryGatewaySetEnabledRequest {
                    enabled: false,
                    plan_generation: before.generation,
                    confirmation: Default::default(),
                },
            )
        },
    )
    .await?;
    emit_status(app, result.clone());
    Ok(result)
}

async fn prepare_enable_plan_unlocked(app: &tauri::AppHandle) -> AppResult<PreparedEnablePlan> {
    let mut public = codex_retry_gateway::build_enable_plan(app).await?;
    let runtime_generation = public.generation;
    let settings = crate::settings::read(app)?;
    let runtime_status = codex_retry_gateway::current_status(app).await?;
    let aio_origin = projected_aio_origin(app, &settings);
    let port = normalize_preferred_port(
        runtime_status
            .effective_port
            .unwrap_or(settings.codex_retry_gateway_preferred_port),
        CODEX_RETRY_GATEWAY_DEFAULT_PORT,
    );
    let guarded_origin = format!("http://127.0.0.1:{port}");
    let route = crate::app::cli_proxy_service::cli_proxy_codex_plan_external_enable(
        app.clone(),
        aio_origin,
        guarded_origin,
    )
    .await
    .map_err(AppError::from)?;
    public.cli_proxy_enable_required = route.cli_proxy_enable_required;
    public.provider_sync = route.provider_sync.clone();
    public.generation = enable_plan_fingerprint(runtime_generation, &route)?;
    Ok(PreparedEnablePlan {
        public,
        runtime_generation,
    })
}

async fn route_guarded_if_healthy_unlocked(
    app: &tauri::AppHandle,
    operation_kind: CodexRetryGatewayOperationKind,
) -> AppResult<Option<crate::infra::codex_provider_sync::CodexProviderSyncResult>> {
    let status = codex_retry_gateway::current_status(app).await?;
    if !status.process_status.healthy {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT",
            "managed gateway must be healthy before committing the guarded route",
        ));
    }
    let port = status.effective_port.ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_APPLY_FAILED",
            "managed gateway has no verified effective port",
        )
    })?;
    let settings = crate::settings::read(app)?;
    let aio_origin = projected_aio_origin(app, &settings);
    let guarded_origin = format!("http://127.0.0.1:{port}");
    let route_plan = crate::app::cli_proxy_service::cli_proxy_codex_plan_external_enable(
        app.clone(),
        aio_origin.clone(),
        guarded_origin.clone(),
    )
    .await
    .map_err(AppError::from)?;
    if !route_plan.route_change_required {
        return Ok(None);
    }
    let applied = crate::app::cli_proxy_service::cli_proxy_codex_apply_guarded_route_for_operation(
        app.clone(),
        None,
        crate::cli_proxy::CodexGuardedRouteApplyRequest {
            expected_generation: route_plan.generation,
            expected_canonical_sha256: route_plan.canonical_config_sha256,
            aio_origin,
            guarded_origin,
            desired_enabled: true,
            source_commit: status.active_commit,
            process_should_run: true,
        },
        operation_kind,
    )
    .await
    .map_err(AppError::from)?;
    if applied.route.route_mode != CodexRouteMode::Guarded
        || !applied.route.live_matches_projection
        || !applied.route.auth_matches_projection
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_VERIFY_FAILED",
            "Codex guarded route did not match the committed projection",
        ));
    }
    Ok(applied.provider_sync)
}

async fn route_direct_aio_unlocked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    aio_origin: &str,
    desired_enabled: bool,
    operation_kind: CodexRetryGatewayOperationKind,
) -> AppResult<()> {
    let route = crate::app::cli_proxy_service::cli_proxy_codex_verify_route(app.clone())
        .await
        .map_err(AppError::from)?;
    if route.route_mode == CodexRouteMode::DirectAio
        && route.desired_enabled == desired_enabled
        && route.aio_origin.as_deref() == Some(aio_origin)
        && route.live_matches_projection
        && route.auth_matches_projection
    {
        return Ok(());
    }
    // Fail-open routing must not depend on probing the process we are bypassing.
    let source_commit = codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(app)
        .and_then(|paths| codex_retry_gateway::read_manager_state(&paths))
        .ok()
        .and_then(|manager| manager.active_commit);
    let applied =
        crate::app::cli_proxy_service::cli_proxy_codex_apply_direct_aio_route_for_operation(
            app.clone(),
            None,
            crate::cli_proxy::CodexDirectAioRouteApplyRequest {
                expected_generation: route.generation,
                expected_canonical_sha256: route.canonical_config_sha256,
                aio_origin: aio_origin.to_string(),
                desired_enabled,
                source_commit,
                process_should_run: true,
            },
            operation_kind,
        )
        .await
        .map_err(AppError::from)?;
    if applied.route.route_mode != CodexRouteMode::DirectAio
        || !applied.route.live_matches_projection
        || !applied.route.auth_matches_projection
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_ROUTE_VERIFY_FAILED",
            "Codex direct-AIO fallback route did not match the committed projection",
        ));
    }
    Ok(())
}

async fn reconcile_startup_unlocked(app: &tauri::AppHandle) -> AppResult<()> {
    let route_reconcile = reconcile_startup_steps(
        || async {
            crate::app::cli_proxy_service::cli_proxy_codex_reconcile_pending_route(app.clone())
                .await
                .map_err(AppError::from)
        },
        || async { codex_retry_gateway::reconcile_pending_runtime_launch(app).await },
        || async {
            reconcile_desired_runtime_unlocked(app, CodexRetryGatewayOperationKind::Startup).await
        },
    )
    .await?;
    if let Some(warning) = route_reconcile.recovery_warning.as_deref() {
        let status = codex_retry_gateway::record_route_recovery_warning(app, warning).await?;
        emit_status(app, status);
    }
    Ok(())
}

async fn reconcile_startup_steps<
    T,
    Route,
    RouteFuture,
    PendingLaunch,
    PendingLaunchFuture,
    DesiredRuntime,
    DesiredRuntimeFuture,
>(
    route: Route,
    pending_launch: PendingLaunch,
    desired_runtime: DesiredRuntime,
) -> AppResult<T>
where
    Route: FnOnce() -> RouteFuture,
    RouteFuture: std::future::Future<Output = AppResult<T>>,
    PendingLaunch: FnOnce() -> PendingLaunchFuture,
    PendingLaunchFuture: std::future::Future<Output = AppResult<()>>,
    DesiredRuntime: FnOnce() -> DesiredRuntimeFuture,
    DesiredRuntimeFuture: std::future::Future<Output = AppResult<()>>,
{
    let route = route().await?;
    pending_launch().await?;
    desired_runtime().await?;
    Ok(route)
}

async fn reconcile_desired_runtime_unlocked(
    app: &tauri::AppHandle,
    operation_kind: CodexRetryGatewayOperationKind,
) -> AppResult<()> {
    let before = codex_retry_gateway::current_status(app).await?;
    if !before.desired_enabled {
        if before.route_mode == CodexRouteMode::Guarded {
            let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
            route_direct_aio_unlocked(app, &aio_origin, false, operation_kind).await?;
        }
        if before.process_status.phase
            != codex_retry_gateway::CodexRetryGatewayProcessPhase::Stopped
        {
            let _ = codex_retry_gateway::set_runtime_enabled(
                app,
                CodexRetryGatewaySetEnabledRequest {
                    enabled: false,
                    plan_generation: before.generation,
                    confirmation: Default::default(),
                },
            )
            .await?;
        }
        emit_current_status(app).await;
        return Ok(());
    }

    let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
    route_direct_aio_unlocked(app, &aio_origin, true, operation_kind).await?;
    let recovered = codex_retry_gateway::retry_runtime_recovery(
        app,
        CodexRetryGatewayGenerationRequest {
            generation: before.generation,
        },
    )
    .await?;
    if recovered.process_status.healthy {
        route_guarded_if_healthy_unlocked(app, operation_kind).await?;
    }
    emit_current_status(app).await;
    Ok(())
}

async fn health_supervisor_tick(app: &tauri::AppHandle) -> AppResult<()> {
    let _lifecycle = super::gateway_lifecycle_lock::lock().await;
    let settings = crate::settings::read(app)?;
    if !settings.codex_retry_gateway_enabled {
        return Ok(());
    }
    let status = match codex_retry_gateway::current_status(app).await {
        Ok(status) => status,
        Err(status_error) => {
            let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
            route_direct_aio_unlocked(
                app,
                &aio_origin,
                true,
                CodexRetryGatewayOperationKind::Recover,
            )
            .await
            .map_err(|route_error| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_FAIL_OPEN_FAILED",
                    format!(
                        "runtime status failed before fallback: {status_error}; direct-AIO fallback failed: {route_error}"
                    ),
                )
            })?;
            return Err(status_error);
        }
    };
    if status.runtime_phase == codex_retry_gateway::CodexRetryGatewayRuntimePhase::Guarded
        && status.process_status.healthy
        && status.route_mode == CodexRouteMode::Guarded
    {
        return Ok(());
    }
    let aio_origin = ensure_aio_gateway_running_unlocked(app).await?;
    if status.route_mode != CodexRouteMode::DirectAio {
        route_direct_aio_unlocked(
            app,
            &aio_origin,
            true,
            CodexRetryGatewayOperationKind::Recover,
        )
        .await?;
    }
    if !codex_retry_gateway::runtime_recovery_due(app)? {
        emit_current_status(app).await;
        return Ok(());
    }
    let current = codex_retry_gateway::current_status(app).await?;
    let recovered = codex_retry_gateway::retry_runtime_recovery(
        app,
        CodexRetryGatewayGenerationRequest {
            generation: current.generation,
        },
    )
    .await?;
    if recovered.process_status.healthy {
        route_guarded_if_healthy_unlocked(app, CodexRetryGatewayOperationKind::Recover).await?;
    }
    emit_current_status(app).await;
    Ok(())
}

pub(crate) async fn start_health_supervisor(app: tauri::AppHandle) {
    let mut guard = health_supervisor_slot().lock().await;
    if let Some(supervisor) = guard.take() {
        stop_health_supervisor_instance(supervisor).await;
    }
    let (shutdown, mut shutdown_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                _ = tokio::time::sleep(SUPERVISOR_INTERVAL) => {
                    if let Err(error) = health_supervisor_tick(&app).await {
                        tracing::warn!(error = %error, "Codex retry gateway health supervision failed");
                        let _lifecycle = super::gateway_lifecycle_lock::lock().await;
                        match codex_retry_gateway::record_runtime_recovery_failure(&app, &error).await {
                            Ok(status) => emit_status(&app, status),
                            Err(record_error) => {
                                tracing::warn!(error = %record_error, "failed to persist health supervisor failure");
                                emit_current_status(&app).await;
                            }
                        }
                    }
                }
            }
        }
    });
    *guard = Some(HealthSupervisor {
        shutdown: Some(shutdown),
        task,
    });
}

async fn ensure_aio_gateway_running_unlocked(app: &tauri::AppHandle) -> AppResult<String> {
    let current = crate::app::gateway_runtime_access::app_gateway_status(app);
    if current.running {
        return Ok(gateway_status_origin(&current));
    }
    let db_state = app.state::<DbInitState>();
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let settings = crate::settings::read(app)?;
    let status = crate::blocking::run("codex_retry_gateway_ensure_aio", {
        let app = app.clone();
        move || {
            crate::app::gateway_control::app_ensure_gateway_running(
                &app,
                db,
                Some(settings.preferred_port),
            )
        }
    })
    .await?;
    crate::app::heartbeat_watchdog::gated_emit(
        app,
        crate::gateway::events::GATEWAY_STATUS_EVENT_NAME,
        status.clone(),
    );
    Ok(gateway_status_origin(&status))
}

fn projected_aio_origin(app: &tauri::AppHandle, settings: &crate::settings::AppSettings) -> String {
    let status = crate::app::gateway_runtime_access::app_gateway_status(app);
    if status.running {
        gateway_status_origin(&status)
    } else {
        format!("http://127.0.0.1:{}", settings.preferred_port)
    }
}

fn gateway_status_origin(status: &crate::gateway::GatewayStatus) -> String {
    status.base_url.clone().unwrap_or_else(|| {
        format!(
            "http://127.0.0.1:{}",
            status.port.unwrap_or(crate::settings::DEFAULT_GATEWAY_PORT)
        )
    })
}

fn enable_plan_fingerprint(
    runtime_generation: u64,
    route: &crate::cli_proxy::CodexExternalEnablePlan,
) -> AppResult<u64> {
    let mut hasher = Sha256::new();
    hasher.update(runtime_generation.to_le_bytes());
    hasher
        .update(serde_json::to_vec(route).map_err(|error| {
            AppError::new("CODEX_RETRY_GATEWAY_PLAN_FAILED", error.to_string())
        })?);
    let digest = hasher.finalize();
    Ok(u64::from_le_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 prefix always contains eight bytes"),
    ))
}

fn stale_generation_error(expected: u64, actual: u64) -> AppError {
    AppError::new(
        "CODEX_RETRY_GATEWAY_STALE_GENERATION",
        format!("expected gateway generation {expected}, got {actual}"),
    )
}

async fn start_then_verified_route<
    T,
    U,
    Start,
    StartFuture,
    Route,
    RouteFuture,
    Rollback,
    RollbackFuture,
>(
    start: Start,
    route: Route,
    rollback: Rollback,
) -> AppResult<(T, U)>
where
    Start: FnOnce() -> StartFuture,
    StartFuture: std::future::Future<Output = AppResult<T>>,
    Route: FnOnce() -> RouteFuture,
    RouteFuture: std::future::Future<Output = AppResult<U>>,
    Rollback: FnOnce() -> RollbackFuture,
    RollbackFuture: std::future::Future<Output = AppResult<()>>,
{
    let started = start().await?;
    let routed = match route().await {
        Ok(routed) => routed,
        Err(route_error) => {
            return match rollback().await {
                Ok(()) => Err(route_error),
                Err(rollback_error) => Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_ENABLE_ROLLBACK_FAILED",
                    format!(
                        "guarded route activation failed: {route_error}; process rollback failed: {rollback_error}"
                    ),
                )),
            }
        }
    };
    Ok((started, routed))
}

async fn safe_route_then_runtime<T, Route, RouteFuture, Runtime, RuntimeFuture>(
    route: Route,
    runtime: Runtime,
) -> AppResult<T>
where
    Route: FnOnce() -> RouteFuture,
    RouteFuture: std::future::Future<Output = AppResult<()>>,
    Runtime: FnOnce() -> RuntimeFuture,
    RuntimeFuture: std::future::Future<Output = AppResult<T>>,
{
    route().await?;
    runtime().await
}

fn emit_status<R: tauri::Runtime>(app: &tauri::AppHandle<R>, status: CodexRetryGatewayStatus) {
    crate::app::heartbeat_watchdog::gated_emit(app, CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME, status);
}

pub(crate) async fn emit_current_status<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if app
        .try_state::<crate::app::gateway_state::GatewayState>()
        .is_none()
    {
        return;
    }
    match codex_retry_gateway::current_status(app).await {
        Ok(status) => emit_status(app, status),
        Err(error) => tracing::warn!(error = %error, "failed to emit Codex retry gateway status"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn uninstall_route(route_mode: CodexRouteMode) -> crate::cli_proxy::CodexRouteVerifyResult {
        crate::cli_proxy::CodexRouteVerifyResult {
            generation: 7,
            cli_proxy_enabled: route_mode != CodexRouteMode::Unproxied,
            route_mode,
            desired_enabled: false,
            aio_origin: Some("http://127.0.0.1:37123".to_string()),
            guarded_origin: Some("http://127.0.0.1:4610".to_string()),
            effective_origin: (route_mode == CodexRouteMode::DirectAio)
                .then(|| "http://127.0.0.1:37123/v1".to_string()),
            canonical_config_sha256: "a".repeat(64),
            live_config_sha256: "b".repeat(64),
            projected_live_config_sha256: "b".repeat(64),
            auth_projection_managed: route_mode != CodexRouteMode::Unproxied,
            live_matches_projection: true,
            auth_matches_projection: true,
        }
    }

    #[test]
    fn enable_plan_fingerprint_changes_with_route_generation() {
        let route = crate::cli_proxy::CodexExternalEnablePlan {
            generation: 1,
            canonical_config_sha256: "a".repeat(64),
            live_config_sha256: "b".repeat(64),
            projected_live_config_sha256: "c".repeat(64),
            cli_proxy_enable_required: true,
            route_change_required: true,
            current_route_mode: CodexRouteMode::Unproxied,
            desired_enabled: false,
            aio_origin: None,
            guarded_origin: None,
            effective_origin: None,
            target_guarded_origin: "http://127.0.0.1:4610".to_string(),
            provider_sync: Default::default(),
        };
        let first = enable_plan_fingerprint(7, &route).unwrap();
        let mut changed = route;
        changed.generation = 2;
        let second = enable_plan_fingerprint(7, &changed).unwrap();
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn enable_starts_process_before_route_and_rolls_back_route_failure() {
        let steps = Arc::new(StdMutex::new(Vec::new()));
        let start_steps = steps.clone();
        let route_steps = steps.clone();
        let rollback_steps = steps.clone();
        let error = start_then_verified_route(
            move || async move {
                start_steps.lock().unwrap().push("process");
                Ok(())
            },
            move || async move {
                route_steps.lock().unwrap().push("guarded_route");
                Err::<(), _>(AppError::new("TEST_ROUTE_FAILED", "route failed"))
            },
            move || async move {
                rollback_steps.lock().unwrap().push("rollback");
                Ok(())
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), "TEST_ROUTE_FAILED");
        assert_eq!(
            steps.lock().unwrap().as_slice(),
            ["process", "guarded_route", "rollback"]
        );
    }

    #[tokio::test]
    async fn enable_returns_the_verified_route_result() {
        let result = start_then_verified_route(
            || async { Ok("runtime") },
            || async { Ok("provider-sync") },
            || async { Ok(()) },
        )
        .await
        .expect("enable route result");

        assert_eq!(result, ("runtime", "provider-sync"));
    }

    #[tokio::test]
    async fn disable_routes_direct_before_stopping_runtime() {
        let steps = Arc::new(StdMutex::new(Vec::new()));
        let route_steps = steps.clone();
        let stop_steps = steps.clone();
        safe_route_then_runtime(
            move || async move {
                route_steps.lock().unwrap().push("direct_aio");
                Ok(())
            },
            move || async move {
                stop_steps.lock().unwrap().push("stop");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(steps.lock().unwrap().as_slice(), ["direct_aio", "stop"]);
    }

    #[tokio::test]
    async fn startup_reconciles_pending_launch_before_desired_runtime() {
        let steps = Arc::new(StdMutex::new(Vec::new()));
        let route_steps = steps.clone();
        let pending_steps = steps.clone();
        let desired_steps = steps.clone();

        let route_result = reconcile_startup_steps(
            move || async move {
                route_steps.lock().unwrap().push("route");
                Ok("route-result")
            },
            move || async move {
                pending_steps.lock().unwrap().push("pending_launch");
                Ok(())
            },
            move || async move {
                desired_steps.lock().unwrap().push("desired_runtime");
                Ok(())
            },
        )
        .await
        .expect("startup reconciliation");

        assert_eq!(route_result, "route-result");
        assert_eq!(
            steps.lock().unwrap().as_slice(),
            ["route", "pending_launch", "desired_runtime"]
        );
    }

    #[test]
    fn uninstall_requires_a_verified_non_guarded_route() {
        assert!(ensure_uninstall_route_safe(&uninstall_route(CodexRouteMode::DirectAio)).is_ok());
        assert!(ensure_uninstall_route_safe(&uninstall_route(CodexRouteMode::Unproxied)).is_ok());

        let guarded = ensure_uninstall_route_safe(&uninstall_route(CodexRouteMode::Guarded))
            .expect_err("guarded route must block uninstall");
        assert_eq!(guarded.code(), "CODEX_RETRY_GATEWAY_UNINSTALL_ROUTE_UNSAFE");

        let mut drifted = uninstall_route(CodexRouteMode::DirectAio);
        drifted.live_matches_projection = false;
        assert!(ensure_uninstall_route_safe(&drifted).is_err());

        let mut still_desired = uninstall_route(CodexRouteMode::DirectAio);
        still_desired.desired_enabled = true;
        assert!(ensure_uninstall_route_safe(&still_desired).is_err());
    }
}
