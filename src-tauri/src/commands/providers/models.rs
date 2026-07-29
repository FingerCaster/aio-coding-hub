//! Usage: Provider model catalog and managed Codex profile IPC commands.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_models_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
) -> Result<crate::provider_models::ProviderModelCatalog, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("provider_models_get", move || {
        crate::provider_models::get(&db, provider_id, &provider_uuid)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_models_refresh(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
) -> Result<crate::provider_models::ProviderModelCatalog, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    crate::provider_models::refresh(&db, provider_id, &provider_uuid)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_model_manual_upsert(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
    remote_model_id: String,
) -> Result<crate::provider_models::ProviderModelCatalog, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("provider_model_manual_upsert", move || {
        crate::provider_models::manual_upsert(&db, provider_id, &provider_uuid, &remote_model_id)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_model_manual_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
    model_uuid: String,
) -> Result<crate::provider_models::ProviderModelCatalog, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("provider_model_manual_delete", move || {
        crate::provider_models::manual_delete(&db, provider_id, &provider_uuid, &model_uuid)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_model_capabilities_update(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
    model_uuid: String,
    capabilities: crate::provider_models::ProviderModelCapabilitiesInput,
) -> Result<crate::provider_models::ProviderModelCatalog, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("provider_model_capabilities_update", move || {
        crate::provider_models::update_capabilities(
            &app,
            &db,
            provider_id,
            &provider_uuid,
            &model_uuid,
            &capabilities,
        )
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_managed_profiles_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<Vec<crate::codex_managed_profiles::CodexManagedProfile>, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("codex_managed_profiles_list", move || {
        crate::codex_managed_profiles::list(&db)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_managed_profile_create(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    profile_name: String,
    model_uuid: String,
) -> Result<crate::codex_managed_profiles::CodexManagedProfile, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("codex_managed_profile_create", move || {
        crate::codex_managed_profiles::create(&app, &db, &profile_name, &model_uuid)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_managed_profile_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    profile_uuid: String,
) -> Result<crate::codex_managed_profiles::CodexManagedProfileDeleteResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("codex_managed_profile_delete", move || {
        crate::codex_managed_profiles::delete(&app, &db, &profile_uuid)
    })
    .await
    .map_err(Into::into)
}
