//! Usage: Image generation orchestration (read config from DB, inject the API
//! key server-side, send via the shared proxy-aware HTTP client) plus task
//! history persistence and asset-protocol scope grants.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::domain::image_gen;

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
    cursor: Option<String>,
    limit: u32,
) -> Result<image_gen::ImageGenTasksPage, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_tasks_list", move || {
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::tasks_page_with_roots(&db, &storage_roots, cursor.as_deref(), limit)
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
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::task_delete_with_roots(&db, &storage_roots, &id)
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
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::tasks_clear_with_roots(&db, &storage_roots)
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
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::read_image_with_roots(&db, &storage_roots, &path)
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn hydrate_images(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    paths: Vec<String>,
) -> Result<Vec<image_gen::ImageGenFetchedImage>, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_hydrate_images", move || {
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::read_images_with_budget_with_roots(
            &db,
            &storage_roots,
            &paths,
            image_gen::HISTORY_HYDRATE_PER_IMAGE_BYTES,
            image_gen::HISTORY_HYDRATE_TOTAL_BYTES,
        )
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
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::storage_stats_with_roots(&db, &storage_dir, &storage_roots)
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
            let canonical_path = std::fs::canonicalize(&path)
                .map_err(|e| format!("SEC_INVALID_INPUT: storage dir cannot be resolved: {e}"))?;
            let default_dir = crate::app_paths::app_data_dir(&app_for_write)?.join("image-gen");
            crate::settings::update(&app_for_write, |settings| {
                let previous_dir = settings
                    .image_gen_storage_dir
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| default_dir.clone());
                let mut roots = settings
                    .image_gen_storage_roots
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>();
                roots.push(previous_dir);
                roots.push(canonical_path.clone());
                let roots = image_gen::canonical_storage_roots(&roots)?;
                let view = image_gen::storage_stats_with_roots(&db, &canonical_path, &roots)?;
                settings.image_gen_storage_dir = Some(canonical_path.to_string_lossy().to_string());
                settings.image_gen_storage_roots = roots
                    .iter()
                    .map(|root| root.to_string_lossy().to_string())
                    .collect();
                Ok(view)
            })
            .map(|(_, view)| view)
        },
    )
    .await
    .map_err(String::from)?;

    Ok(view)
}

pub(crate) async fn storage_cleanup(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    keep_count: u32,
) -> Result<u32, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("image_gen_storage_cleanup", move || {
        let storage_roots = image_gen::storage_roots_from_settings(&app)?;
        image_gen::storage_cleanup_with_roots(&db, &storage_roots, keep_count)
    })
    .await
    .map_err(Into::into)
}
