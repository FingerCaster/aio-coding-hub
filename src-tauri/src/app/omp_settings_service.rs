//! Settings orchestration stays tied to the currently selected native target.
use super::native_cli_service::{resolve_target, verify_selected_target};
use crate::domain::omp_settings::*;
use crate::infra::native_cli::{self, omp_settings};
use crate::shared::blocking;

pub(crate) async fn read(
    app: tauri::AppHandle,
    target_id: String,
) -> Result<OmpSettingsSnapshot, String> {
    blocking::run("omp_settings_read", move || {
        let target = resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |session| {
            verify_selected_target(&app, &target)?;
            let mut snapshot = omp_settings::read(&target, session)?;
            let mut seen: std::collections::HashSet<_> =
                snapshot.models.iter().map(|m| m.selector.clone()).collect();
            for model in super::native_model_defaults::omp_settings_models() {
                if seen.insert(model.selector.clone()) {
                    snapshot.models.push(model);
                }
            }
            Ok(snapshot)
        })
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn save(
    app: tauri::AppHandle,
    input: OmpSettingsSaveInput,
) -> Result<OmpSettingsWriteResult, String> {
    blocking::run("omp_settings_save", move || {
        let target = resolve_target(&app, &input.target_id)?;
        native_cli::with_target_lock(&target, |_| {
            verify_selected_target(&app, &target)?;
            omp_settings::save(&target, &input)
        })
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn read_agent(
    app: tauri::AppHandle,
    target_id: String,
    file_name: String,
) -> Result<OmpAgentDocument, String> {
    blocking::run("omp_agent_read", move || {
        let target = resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |_| {
            verify_selected_target(&app, &target)?;
            omp_settings::read_agent(&target, &file_name)
        })
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn save_agent(
    app: tauri::AppHandle,
    input: OmpAgentSaveInput,
) -> Result<OmpSettingsWriteResult, String> {
    blocking::run("omp_agent_save", move || {
        let target = resolve_target(&app, &input.target_id)?;
        native_cli::with_target_lock(&target, |_| {
            verify_selected_target(&app, &target)?;
            omp_settings::save_agent(&target, &input)
        })
    })
    .await
    .map_err(Into::into)
}
