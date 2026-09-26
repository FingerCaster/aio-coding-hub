use crate::app::omp_settings_service;
use crate::domain::omp_settings::*;

#[tauri::command]
#[specta::specta]
pub(crate) async fn omp_settings_read(
    app: tauri::AppHandle,
    target_id: String,
) -> Result<OmpSettingsSnapshot, String> {
    omp_settings_service::read(app, target_id).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn omp_settings_save(
    app: tauri::AppHandle,
    input: OmpSettingsSaveInput,
) -> Result<OmpSettingsWriteResult, String> {
    omp_settings_service::save(app, input).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn omp_agent_read(
    app: tauri::AppHandle,
    target_id: String,
    file_name: String,
) -> Result<OmpAgentDocument, String> {
    omp_settings_service::read_agent(app, target_id, file_name).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn omp_agent_save(
    app: tauri::AppHandle,
    input: OmpAgentSaveInput,
) -> Result<OmpSettingsWriteResult, String> {
    omp_settings_service::save_agent(app, input).await
}
