//! Usage: Tauri commands for checking and updating CLI installations.

use crate::cli_update as cli_update_infra;
use crate::domain::native_cli::NativeClient;

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_check_latest_version(
    app: tauri::AppHandle,
    client: NativeClient,
) -> Result<cli_update_infra::native::NativeCliVersionCheck, String> {
    cli_update_infra::native::check(&app, client).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_update(
    app: tauri::AppHandle,
    client: NativeClient,
    plan_id: String,
) -> Result<cli_update_infra::CliUpdateResult, String> {
    cli_update_infra::native::update(&app, client, plan_id).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_check_latest_version(
    app: tauri::AppHandle,
    cli_key: String,
) -> Result<cli_update_infra::CliVersionCheck, String> {
    Ok(cli_update_infra::cli_check_latest_version(&app, cli_key).await)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_update(
    app: tauri::AppHandle,
    cli_key: String,
) -> Result<cli_update_infra::CliUpdateResult, String> {
    Ok(cli_update_infra::cli_update(&app, cli_key).await)
}
