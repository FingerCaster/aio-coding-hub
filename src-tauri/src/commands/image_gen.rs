//! Usage: Thin IPC wrappers for the image generation page (config + transport
//! proxy commands). The API key never appears in IPC arguments or results.

use crate::app::image_gen_service;
use crate::app_state::DbInitState;
use crate::blocking;
use crate::domain::image_gen::{
    ImageGenConfigView, ImageGenFetchedImage, ImageGenHttpResponse, ImageGenMultipartFile,
    ImageGenStorageView, ImageGenTaskPersistPayload, ImageGenTaskRow, ImageGenTasksPage,
};
use base64::Engine as _;
use std::path::{Path, PathBuf};
use tauri::WebviewWindow;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

const IMAGE_GEN_SAVE_MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;
const IMAGE_GEN_SAVE_MAX_BASE64_BYTES: usize = IMAGE_GEN_SAVE_MAX_DECODED_BYTES.div_ceil(3) * 4;
const IMAGE_GEN_SAVE_MAX_FILENAME_CHARS: usize = 128;

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_config_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
) -> Result<ImageGenConfigView, String> {
    image_gen_service::config_get(app, db_state, adapter_id).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_config_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<ImageGenConfigView, String> {
    image_gen_service::config_set(app, db_state, adapter_id, base_url, model, api_key).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_post_json(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    path: String,
    body: serde_json::Value,
    timeout_secs: Option<u32>,
) -> Result<ImageGenHttpResponse, String> {
    image_gen_service::post_json(app, db_state, adapter_id, path, body, timeout_secs).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_post_multipart(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    adapter_id: String,
    path: String,
    fields: Vec<(String, String)>,
    files: Vec<ImageGenMultipartFile>,
    timeout_secs: Option<u32>,
) -> Result<ImageGenHttpResponse, String> {
    image_gen_service::post_multipart(app, db_state, adapter_id, path, fields, files, timeout_secs)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_fetch_image(
    url: String,
    timeout_secs: Option<u32>,
) -> Result<ImageGenFetchedImage, String> {
    image_gen_service::fetch_image(url, timeout_secs).await
}

fn image_extension_for_mime(mime: &str) -> Result<&'static str, String> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Ok("png"),
        "image/jpeg" | "image/jpg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        _ => Err("SEC_INVALID_INPUT: unsupported image mime".to_string()),
    }
}

fn validate_suggested_filename(name: &str, expected_extension: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > IMAGE_GEN_SAVE_MAX_FILENAME_CHARS {
        return Err("SEC_INVALID_INPUT: suggested filename is invalid".to_string());
    }
    let path = Path::new(name);
    if path.file_name().and_then(|value| value.to_str()) != Some(name)
        || !name
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
    {
        return Err("SEC_INVALID_INPUT: suggested filename is invalid".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case(expected_extension) {
        return Err(
            "SEC_INVALID_INPUT: suggested filename extension does not match mime".to_string(),
        );
    }
    Ok(name.to_string())
}

fn decode_image_gen_save_data(data_b64: &str) -> Result<Vec<u8>, String> {
    let data_b64 = data_b64.trim();
    if data_b64.is_empty() {
        return Err("SEC_INVALID_INPUT: data_b64 is required".to_string());
    }
    if data_b64.len() > IMAGE_GEN_SAVE_MAX_BASE64_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: image data is too large (max {IMAGE_GEN_SAVE_MAX_DECODED_BYTES} decoded bytes)"
        ));
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("SEC_INVALID_INPUT: data_b64 is invalid: {e}"))?;
    if bytes.len() > IMAGE_GEN_SAVE_MAX_DECODED_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: image data is too large (max {IMAGE_GEN_SAVE_MAX_DECODED_BYTES} decoded bytes)"
        ));
    }
    Ok(bytes)
}

fn write_authorized_image_file(
    file_path: &Path,
    expected_extension: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let extension = file_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case(expected_extension) {
        return Err(
            "SEC_INVALID_INPUT: selected file extension does not match image mime".to_string(),
        );
    }
    std::fs::write(file_path, bytes)
        .map_err(|err| format!("SYSTEM_ERROR: failed to write image file: {err}"))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_save_image(
    window: WebviewWindow,
    suggested_filename: String,
    mime: String,
    data_b64: String,
) -> Result<bool, String> {
    let extension = image_extension_for_mime(&mime)?;
    let suggested_filename = validate_suggested_filename(&suggested_filename, extension)?;
    let bytes = decode_image_gen_save_data(&data_b64)?;

    let dialog = window
        .dialog()
        .file()
        .set_title("保存图片")
        .set_file_name(&suggested_filename)
        .add_filter("Image", &[extension]);
    #[cfg(any(windows, target_os = "macos"))]
    let dialog = dialog.set_parent(&window);
    let (tx, rx) = oneshot::channel();
    dialog.save_file(move |selection| {
        let _ = tx.send(selection.map(|path| PathBuf::from(path.to_string())));
    });
    let Some(file_path) = rx
        .await
        .map_err(|_| "SYSTEM_ERROR: image save dialog response channel dropped".to_string())?
    else {
        return Ok(false);
    };

    blocking::run("image_gen_save_image", move || {
        write_authorized_image_file(&file_path, extension, &bytes)
    })
    .await
    .map_err(String::from)?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_task_persist(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    payload: ImageGenTaskPersistPayload,
) -> Result<ImageGenTaskRow, String> {
    image_gen_service::task_persist(app, db_state, payload).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_tasks_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cursor: Option<String>,
    limit: u32,
) -> Result<ImageGenTasksPage, String> {
    image_gen_service::tasks_list(app, db_state, cursor, limit).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_task_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: String,
) -> Result<(), String> {
    image_gen_service::task_delete(app, db_state, id).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_tasks_clear(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<u32, String> {
    image_gen_service::tasks_clear(app, db_state).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_read_image(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    path: String,
) -> Result<ImageGenFetchedImage, String> {
    image_gen_service::read_image(app, db_state, path).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_hydrate_images(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    paths: Vec<String>,
) -> Result<Vec<ImageGenFetchedImage>, String> {
    image_gen_service::hydrate_images(app, db_state, paths).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_storage_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
) -> Result<ImageGenStorageView, String> {
    image_gen_service::storage_get(app, db_state).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_storage_set_dir(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    dir: String,
) -> Result<ImageGenStorageView, String> {
    image_gen_service::storage_set_dir(app, db_state, dir).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn image_gen_storage_cleanup(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    keep_count: u32,
) -> Result<u32, String> {
    image_gen_service::storage_cleanup(app, db_state, keep_count).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_gen_save_image_writes_only_matching_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.png");

        let bytes = decode_image_gen_save_data("aGVsbG8=").expect("decode");
        write_authorized_image_file(&path, "png", &bytes).expect("save should succeed");

        assert_eq!(std::fs::read(path).expect("read file"), b"hello");
        let err = write_authorized_image_file(&dir.path().join("out.txt"), "png", &bytes)
            .expect_err("mismatched extension should fail");
        assert!(err.contains("extension"));
    }

    #[test]
    fn image_gen_save_image_rejects_invalid_filename_mime_and_empty_data() {
        assert!(validate_suggested_filename("../out.png", "png").is_err());
        assert!(validate_suggested_filename("out.jpg", "png").is_err());
        assert!(image_extension_for_mime("text/plain").is_err());
        let err = decode_image_gen_save_data("  ").expect_err("empty data should fail");
        assert!(err.contains("SEC_INVALID_INPUT: data_b64 is required"));
    }

    #[test]
    fn image_gen_save_image_rejects_oversized_and_invalid_base64() {
        let err = decode_image_gen_save_data(&"A".repeat(IMAGE_GEN_SAVE_MAX_BASE64_BYTES + 1))
            .expect_err("oversized data should fail");
        assert!(err.contains("SEC_INVALID_INPUT: image data is too large"));

        let err = decode_image_gen_save_data("!!bad!!").expect_err("invalid base64 should fail");
        assert!(err.contains("SEC_INVALID_INPUT: data_b64 is invalid"));
    }
}
