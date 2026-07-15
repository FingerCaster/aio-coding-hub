//! Usage: CLI proxy configuration related Tauri commands.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::gateway::events::GATEWAY_STATUS_EVENT_NAME;
use crate::gateway_control::app_ensure_gateway_running;
use crate::gateway_runtime_access::app_gateway_status;
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayManagerPaths, CodexRetryGatewayOperationKind,
    FileCodexRetryGatewayTransitionStore,
};
use crate::{blocking, cli_proxy, mcp, settings};

fn codex_route_transition_store<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<FileCodexRetryGatewayTransitionStore> {
    let paths = CodexRetryGatewayManagerPaths::from_app(app)?;
    paths.ensure_dirs()?;
    Ok(FileCodexRetryGatewayTransitionStore::new(
        paths.transition_path,
    ))
}

async fn resync_codex_mcp_after_route_change<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
) {
    let db_result = match db_state {
        Some(db_state) => ensure_db_ready(app.clone(), db_state).await,
        None => crate::infra::db::init(&app),
    };
    match db_result {
        Ok(db) => {
            if let Err(err) = blocking::run("cli_proxy_codex_route_mcp_resync", move || {
                let conn = db.open_connection()?;
                mcp::sync_one_cli(&app, &conn, "codex")
            })
            .await
            {
                tracing::warn!("codex route MCP re-sync failed: {err}");
            }
        }
        Err(err) => {
            tracing::warn!("codex route MCP re-sync skipped, db unavailable: {err}");
        }
    }
}

pub(crate) async fn cli_proxy_status_all(
    app: tauri::AppHandle,
) -> Result<Vec<cli_proxy::CliProxyStatus>, String> {
    let status = app_gateway_status(&app);
    let current_base_origin = if status.running {
        Some(status.base_url.unwrap_or_else(|| {
            format!(
                "http://127.0.0.1:{}",
                status.port.unwrap_or(settings::DEFAULT_GATEWAY_PORT)
            )
        }))
    } else {
        None
    };

    blocking::run("cli_proxy_status_all", move || {
        cli_proxy::status_all(&app, current_base_origin.as_deref())
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn cli_proxy_set_enabled_impl(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    cli_key: String,
    enabled: bool,
) -> Result<cli_proxy::CliProxyResult, String> {
    tracing::info!(cli_key = %cli_key, enabled = enabled, "cli proxy enabled state changing");

    let gateway_lifecycle: Option<crate::app::gateway_lifecycle_lock::GatewayLifecycleGuard>;
    let base_origin = if enabled {
        let db = ensure_db_ready(app.clone(), db_state).await?;
        gateway_lifecycle = Some(crate::app::gateway_lifecycle_lock::lock().await);

        blocking::run("cli_proxy_set_enabled_ensure_gateway", {
            let app = app.clone();
            let db = db.clone();
            move || -> crate::shared::error::AppResult<String> {
                let settings = settings::read(&app)?;
                let was_running = app_gateway_status(&app).running;
                let status = app_ensure_gateway_running(&app, db, Some(settings.preferred_port))?;
                if !was_running {
                    crate::app::heartbeat_watchdog::gated_emit(
                        &app,
                        GATEWAY_STATUS_EVENT_NAME,
                        status.clone(),
                    );
                }

                Ok(status.base_url.unwrap_or_else(|| {
                    format!(
                        "http://127.0.0.1:{}",
                        status.port.unwrap_or(settings::DEFAULT_GATEWAY_PORT)
                    )
                }))
            }
        })
        .await?
    } else {
        gateway_lifecycle = None;
        format!("http://127.0.0.1:{}", settings::DEFAULT_GATEWAY_PORT)
    };

    let result = blocking::run("cli_proxy_set_enabled_apply", {
        let app = app.clone();
        let cli_key = cli_key.clone();
        move || cli_proxy::set_enabled(&app, &cli_key, enabled, &base_origin)
    })
    .await
    .map_err(Into::into);

    // After successful proxy toggle, re-sync MCP servers to CLI config file.
    // cli_proxy and mcp_sync both write to the same config file (e.g. ~/.codex/config.toml).
    // Without this re-sync, MCP entries get lost during the backup/restore cycle.
    if let Ok(ref r) = result {
        if r.ok {
            match ensure_db_ready(app.clone(), db_state).await {
                Ok(db) => {
                    let sync_app = app.clone();
                    let sync_cli_key = cli_key.clone();
                    if let Err(err) = blocking::run("cli_proxy_mcp_resync", move || {
                        let conn = db.open_connection()?;
                        mcp::sync_one_cli(&sync_app, &conn, &sync_cli_key)
                    })
                    .await
                    {
                        tracing::warn!(cli_key = %cli_key, "mcp re-sync after proxy toggle failed: {err}");
                    }
                }
                Err(err) => {
                    tracing::warn!(cli_key = %cli_key, "mcp re-sync skipped, db unavailable: {err}");
                }
            }
        }
    }

    match &result {
        Ok(r) if !r.ok => {
            tracing::warn!(
                cli_key = %r.cli_key,
                error_code = %r.error_code.as_deref().unwrap_or(""),
                "cli proxy set_enabled failed: {}",
                r.message
            );
        }
        Err(err) => {
            tracing::warn!("cli proxy set_enabled error: {}", err);
        }
        _ => {}
    }

    drop(gateway_lifecycle);
    result
}

pub(crate) async fn cli_proxy_set_disabled_impl<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    cli_key: String,
) -> Result<cli_proxy::CliProxyResult, String> {
    tracing::info!(cli_key = %cli_key, enabled = false, "cli proxy enabled state changing");

    if cli_key == "codex" {
        return crate::app::codex_retry_gateway_service::disable_codex_cli_proxy(app, db_state)
            .await;
    }

    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;

    let base_origin = format!("http://127.0.0.1:{}", settings::DEFAULT_GATEWAY_PORT);
    let result = blocking::run("cli_proxy_set_enabled_apply", {
        let app = app.clone();
        let cli_key = cli_key.clone();
        move || cli_proxy::set_enabled(&app, &cli_key, false, &base_origin)
    })
    .await
    .map_err(Into::into);

    if let Ok(ref r) = result {
        if r.ok {
            let db_result = match db_state {
                Some(db_state) => ensure_db_ready(app.clone(), db_state).await,
                None => crate::infra::db::init(&app),
            };
            match db_result {
                Ok(db) => {
                    let sync_app = app.clone();
                    let sync_cli_key = cli_key.clone();
                    if let Err(err) = blocking::run("cli_proxy_mcp_resync", move || {
                        let conn = db.open_connection()?;
                        mcp::sync_one_cli(&sync_app, &conn, &sync_cli_key)
                    })
                    .await
                    {
                        tracing::warn!(cli_key = %cli_key, "mcp re-sync after proxy toggle failed: {err}");
                    }
                }
                Err(err) => {
                    tracing::warn!(cli_key = %cli_key, "mcp re-sync skipped, db unavailable: {err}");
                }
            }
        }
    }

    match &result {
        Ok(r) if !r.ok => {
            tracing::warn!(
                cli_key = %r.cli_key,
                error_code = %r.error_code.as_deref().unwrap_or(""),
                "cli proxy set_enabled failed: {}",
                r.message
            );
        }
        Err(err) => {
            tracing::warn!("cli proxy set_enabled error: {}", err);
        }
        _ => {}
    }

    result
}

pub(crate) async fn cli_proxy_sync_enabled(
    app: tauri::AppHandle,
    base_origin: String,
    apply_live: Option<bool>,
) -> Result<Vec<cli_proxy::CliProxyResult>, String> {
    blocking::run("cli_proxy_sync_enabled", move || {
        cli_proxy::sync_enabled(&app, &base_origin, apply_live.unwrap_or(true))
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn cli_proxy_rebind_codex_home(
    app: tauri::AppHandle,
) -> Result<cli_proxy::CliProxyResult, String> {
    let status = app_gateway_status(&app);
    let (gateway_running, base_origin) = if status.running {
        (
            true,
            status.base_url.unwrap_or_else(|| {
                format!(
                    "http://127.0.0.1:{}",
                    status.port.unwrap_or(settings::DEFAULT_GATEWAY_PORT)
                )
            }),
        )
    } else {
        let settings = settings::read(&app)?;
        (
            false,
            format!("http://127.0.0.1:{}", settings.preferred_port),
        )
    };

    blocking::run("cli_proxy_rebind_codex_home", move || {
        cli_proxy::rebind_codex_home_after_change(&app, &base_origin, gateway_running)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn cli_proxy_codex_plan_external_enable<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aio_origin: String,
    guarded_origin: String,
) -> Result<cli_proxy::CodexExternalEnablePlan, String> {
    blocking::run("cli_proxy_codex_plan_external_enable", move || {
        cli_proxy::plan_external_enable(&app, &aio_origin, &guarded_origin)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn cli_proxy_codex_apply_guarded_route<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexGuardedRouteApplyRequest,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    cli_proxy_codex_apply_guarded_route_for_operation(
        app,
        db_state,
        request,
        CodexRetryGatewayOperationKind::Enable,
    )
    .await
}

pub(crate) async fn cli_proxy_codex_apply_guarded_route_for_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexGuardedRouteApplyRequest,
    operation_kind: CodexRetryGatewayOperationKind,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    let store = codex_route_transition_store(&app).map_err(|err| err.to_string())?;
    let result = blocking::run("cli_proxy_codex_apply_guarded_route", {
        let app = app.clone();
        move || cli_proxy::apply_guarded_route_with_operation(&app, &store, request, operation_kind)
    })
    .await
    .map_err(|err| err.to_string())?;
    resync_codex_mcp_after_route_change(app, db_state).await;
    Ok(result)
}

pub(crate) async fn cli_proxy_codex_apply_direct_aio_route<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexDirectAioRouteApplyRequest,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    cli_proxy_codex_apply_direct_aio_route_for_operation(
        app,
        db_state,
        request,
        CodexRetryGatewayOperationKind::DisableGateway,
    )
    .await
}

pub(crate) async fn cli_proxy_codex_apply_direct_aio_route_for_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexDirectAioRouteApplyRequest,
    operation_kind: CodexRetryGatewayOperationKind,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    let store = codex_route_transition_store(&app).map_err(|err| err.to_string())?;
    let result = blocking::run("cli_proxy_codex_apply_direct_aio_route", {
        let app = app.clone();
        move || {
            cli_proxy::apply_direct_aio_route_with_operation(&app, &store, request, operation_kind)
        }
    })
    .await
    .map_err(|err| err.to_string())?;
    resync_codex_mcp_after_route_change(app, db_state).await;
    Ok(result)
}

pub(crate) async fn cli_proxy_codex_restore_unproxied_route<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexRestoreUnproxiedRouteRequest,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    cli_proxy_codex_restore_unproxied_route_for_operation(
        app,
        db_state,
        request,
        CodexRetryGatewayOperationKind::DisableCliProxy,
    )
    .await
}

pub(crate) async fn cli_proxy_codex_restore_unproxied_route_for_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    request: cli_proxy::CodexRestoreUnproxiedRouteRequest,
    operation_kind: CodexRetryGatewayOperationKind,
) -> Result<cli_proxy::CodexRouteApplyResult, String> {
    let store = codex_route_transition_store(&app).map_err(|err| err.to_string())?;
    let result = blocking::run("cli_proxy_codex_restore_unproxied_route", {
        let app = app.clone();
        move || {
            cli_proxy::restore_unproxied_route_with_operation(&app, &store, request, operation_kind)
        }
    })
    .await
    .map_err(|err| err.to_string())?;
    resync_codex_mcp_after_route_change(app, db_state).await;
    Ok(result)
}

pub(crate) async fn cli_proxy_codex_verify_route<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<cli_proxy::CodexRouteVerifyResult, String> {
    blocking::run("cli_proxy_codex_verify_route", move || {
        cli_proxy::verify_route(&app)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn cli_proxy_codex_reconcile_pending_route<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<cli_proxy::CodexRouteReconcileResult, String> {
    let store = codex_route_transition_store(&app).map_err(|err| err.to_string())?;
    blocking::run("cli_proxy_codex_reconcile_pending_route", {
        let app = app.clone();
        move || cli_proxy::reconcile_pending_route(&app, &store)
    })
    .await
    .map_err(Into::into)
}
