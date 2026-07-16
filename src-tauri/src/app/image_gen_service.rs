//! Usage: Image generation orchestration (read config from DB, inject the API
//! key server-side, send via the shared proxy-aware HTTP client) plus task
//! history persistence and asset-protocol scope grants.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::domain::image_gen;
use std::path::Path;
use tauri::Manager;

pub(crate) async fn config_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
) -> Result<image_gen::ImageGenConfigView, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("image_gen_config_get", move || {
        image_gen::config_get(&db, &adapter_id)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn config_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<image_gen::ImageGenConfigView, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("image_gen_config_set", move || {
        image_gen::config_set(&db, &adapter_id, &base_url, &model, api_key.as_deref())
    })
    .await
    .map_err(Into::into)
}

async fn connection(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
) -> Result<(String, String), String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("image_gen_connection_get", move || {
        image_gen::config_connection(&db, &adapter_id)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn post_json(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    path: String,
    body: serde_json::Value,
    timeout_secs: Option<u32>,
) -> Result<image_gen::ImageGenHttpResponse, String> {
    let (base_url, api_key) = connection(app, db_state, adapter_id).await?;
    let client = crate::gateway::http_client::get();
    image_gen::post_json(&client, &base_url, &api_key, &path, &body, timeout_secs).await
}

pub(crate) async fn post_multipart(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    path: String,
    fields: Vec<(String, String)>,
    files: Vec<image_gen::ImageGenMultipartFile>,
    timeout_secs: Option<u32>,
) -> Result<image_gen::ImageGenHttpResponse, String> {
    let (base_url, api_key) = connection(app, db_state, adapter_id).await?;
    let client = crate::gateway::http_client::get();
    image_gen::post_multipart(
        &client,
        &base_url,
        &api_key,
        &path,
        &fields,
        &files,
        timeout_secs,
    )
    .await
}

pub(crate) async fn fetch_image(
    url: String,
    timeout_secs: Option<u32>,
) -> Result<image_gen::ImageGenFetchedImage, String> {
    image_gen::fetch_image(&url, timeout_secs).await
}

// --- Task history persistence ---

/// Grants the asset protocol access to an image gen directory. Failure is
/// non-fatal (images will not render until restart); path-only warn log.
fn allow_asset_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &Path) {
    let Ok(root) = image_gen::canonical_storage_root(dir) else {
        tracing::warn!(dir = %dir.display(), "image gen asset scope rejected invalid storage root");
        return;
    };
    if let Err(err) = app.asset_protocol_scope().allow_directory(&root, true) {
        tracing::warn!(
            dir = %root.display(),
            "image gen asset scope allow_directory failed: {err}"
        );
    }
}

/// Startup grant: only the canonical current storage root. DB task paths never
/// grant additional filesystem authority.
pub(crate) async fn allow_startup_asset_scope(app: &tauri::AppHandle, db: &crate::db::Db) {
    let app_for_read = app.clone();
    let _ = db;
    let root = blocking::run("image_gen_asset_scope_root", move || {
        let root = image_gen::storage_dir_from_settings(&app_for_read)?;
        image_gen::ensure_writable_dir(&root)?;
        image_gen::canonical_storage_root(&root)
    })
    .await;

    match root {
        Ok(root) => allow_asset_dir(app, &root),
        Err(err) => {
            tracing::warn!("image gen asset scope startup init failed: {err}");
        }
    }
}

pub(crate) async fn task_persist(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    payload: image_gen::ImageGenTaskPersistPayload,
) -> Result<image_gen::ImageGenTaskRow, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_task_persist", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::task_persist(&db, &storage_dir, payload)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn tasks_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    before_created_at: Option<i64>,
    limit: u32,
) -> Result<Vec<image_gen::ImageGenTaskRow>, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("image_gen_tasks_list", move || {
        image_gen::tasks_list(&db, before_created_at, limit)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn task_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: String,
) -> Result<(), String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_task_delete", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::task_delete(&db, &storage_dir, &id)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn tasks_clear(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<u32, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_tasks_clear", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::tasks_clear(&db, &storage_dir)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn read_image(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    path: String,
) -> Result<image_gen::ImageGenFetchedImage, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_read_image", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::read_image(&db, &storage_dir, &path)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn storage_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<image_gen::ImageGenStorageView, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_storage_get", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::storage_stats(&db, &storage_dir)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn storage_set_dir(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    dir: String,
) -> Result<image_gen::ImageGenStorageView, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let app_for_write = app.clone();
    let view = blocking::run(
        "image_gen_storage_set_dir",
        move || -> crate::shared::error::AppResult<_> {
            let dir = dir.trim();
            if dir.is_empty() {
                return Err("SEC_INVALID_INPUT: storage dir is required"
                    .to_string()
                    .into());
            }
            let path = std::path::PathBuf::from(dir);
            if !path.is_absolute() {
                return Err("SEC_INVALID_INPUT: storage dir must be an absolute path"
                    .to_string()
                    .into());
            }
            image_gen::ensure_writable_dir(&path)?;
            let view = image_gen::storage_stats(&db, &path)?;

            let mut settings = crate::settings::read(&app_for_write)?;
            settings.image_gen_storage_dir = Some(path.to_string_lossy().to_string());
            crate::settings::write(&app_for_write, &settings)?;

            Ok(view)
        },
    )
    .await
    .map_err(String::from)?;

    allow_asset_dir(&app, Path::new(&view.dir));
    Ok(view)
}

pub(crate) async fn storage_cleanup(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    keep_count: u32,
) -> Result<u32, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_storage_cleanup", move || {
        let storage_dir = image_gen::storage_dir_from_settings(&app)?;
        image_gen::storage_cleanup(&db, &storage_dir, keep_count)
    })
    .await
    .map_err(Into::into)
}
