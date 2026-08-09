//! Usage: Data reset / disk usage related Tauri commands.

use crate::app_state::{ensure_db_ready, prepare_db_reset, DbInitState};
use crate::shared::ipc_confirm::RiskyIpcConfirm;
use crate::{app_paths, blocking, data_management};

#[tauri::command]
#[specta::specta]
pub(crate) async fn app_data_dir_get(app: tauri::AppHandle) -> Result<String, String> {
    blocking::run(
        "app_data_dir_get",
        move || -> crate::shared::error::AppResult<String> {
            let dir = app_paths::app_data_dir(&app)?;
            Ok(dir.to_string_lossy().to_string())
        },
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn db_disk_usage_get(
    app: tauri::AppHandle,
) -> Result<data_management::DbDiskUsage, String> {
    blocking::run("db_disk_usage_get", move || {
        data_management::db_disk_usage_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn db_compact(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<data_management::DbCompactResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("db_compact", move || data_management::db_compact(&app, &db))
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn request_logs_clear_all(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<data_management::ClearRequestLogsResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("request_logs_clear_all", move || {
        data_management::request_logs_clear_all(&db)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn app_data_reset(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<bool, String> {
    RiskyIpcConfirm::require(confirm, "app_data_reset", "app_data")?;
    // Keep gateway starts and DB initialization out while the durable reset
    // intent is committed and the old process exits. Destructive deletion is
    // owned exclusively by the next startup.
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    crate::app::cleanup::stop_gateway_best_effort_unlocked(&app).await;
    crate::app::cleanup::restore_cli_proxy_keep_state_best_effort(
        &app,
        "app_data_reset_cli_proxy_restore_keep_state",
        "数据重置前",
        false,
    )
    .await;
    let _db_reset_guard = prepare_db_reset(db_state.inner()).await;
    crate::app::maintenance::request_reset_and_exit(app).map_err(Into::into)
}
