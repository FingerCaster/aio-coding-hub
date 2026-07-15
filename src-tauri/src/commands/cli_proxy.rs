//! Usage: Thin IPC wrappers for CLI proxy commands.

use crate::app::cli_proxy_service;
use crate::app_state::DbInitState;

pub(crate) async fn cli_proxy_set_disabled_impl<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db_state: Option<&DbInitState>,
    cli_key: String,
) -> Result<crate::cli_proxy::CliProxyResult, String> {
    cli_proxy_service::cli_proxy_set_disabled_impl(app, db_state, cli_key).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_status_all(
    app: tauri::AppHandle,
) -> Result<Vec<crate::cli_proxy::CliProxyStatus>, String> {
    cli_proxy_service::cli_proxy_status_all(app).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_set_enabled(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    enabled: bool,
) -> Result<crate::cli_proxy::CliProxyResult, String> {
    if enabled {
        cli_proxy_service::cli_proxy_set_enabled_impl(app, db_state.inner(), cli_key, true).await
    } else {
        cli_proxy_service::cli_proxy_set_disabled_impl(app, Some(db_state.inner()), cli_key).await
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_sync_enabled(
    app: tauri::AppHandle,
    base_origin: String,
    apply_live: Option<bool>,
) -> Result<Vec<crate::cli_proxy::CliProxyResult>, String> {
    cli_proxy_service::cli_proxy_sync_enabled(app, base_origin, apply_live).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_rebind_codex_home(
    app: tauri::AppHandle,
) -> Result<crate::cli_proxy::CliProxyResult, String> {
    cli_proxy_service::cli_proxy_rebind_codex_home(app).await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_plan_external_enable(
    app: tauri::AppHandle,
    aio_origin: String,
    guarded_origin: String,
) -> Result<crate::cli_proxy::CodexExternalEnablePlan, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_plan_external_enable(app, aio_origin, guarded_origin).await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_apply_guarded_route(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    request: crate::cli_proxy::CodexGuardedRouteApplyRequest,
) -> Result<crate::cli_proxy::CodexRouteApplyResult, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_apply_guarded_route(app, Some(db_state.inner()), request)
        .await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_apply_direct_aio_route(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    request: crate::cli_proxy::CodexDirectAioRouteApplyRequest,
) -> Result<crate::cli_proxy::CodexRouteApplyResult, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_apply_direct_aio_route(app, Some(db_state.inner()), request)
        .await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_restore_unproxied_route(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    request: crate::cli_proxy::CodexRestoreUnproxiedRouteRequest,
) -> Result<crate::cli_proxy::CodexRouteApplyResult, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_restore_unproxied_route(app, Some(db_state.inner()), request)
        .await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_verify_route(
    app: tauri::AppHandle,
) -> Result<crate::cli_proxy::CodexRouteVerifyResult, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_verify_route(app).await
}

#[allow(dead_code)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_proxy_codex_reconcile_pending_route(
    app: tauri::AppHandle,
) -> Result<crate::cli_proxy::CodexRouteReconcileResult, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    cli_proxy_service::cli_proxy_codex_reconcile_pending_route(app).await
}
