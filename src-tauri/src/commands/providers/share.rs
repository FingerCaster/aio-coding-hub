//! Usage: Backend-owned provider share export/import commands.

use crate::app::app_state::{ensure_db_ready, DbInitState};
use crate::app::provider_share_service::{ProviderShareImportPreview, ProviderShareService};
use crate::blocking;
use crate::providers::{
    export_provider_share_v2, provider_share_default_filename, serialize_provider_share_v2,
    ProviderSummary,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::ipc_confirm::RiskyIpcConfirm;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::WebviewWindow;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

const CLIPBOARD_CLEAR_DELAY: Duration = Duration::from_secs(60);
const COPY_ACTION: &str = "provider_share_copy_to_clipboard";
const SAVE_ACTION: &str = "provider_share_save_to_file";
const IMPORT_ACTION: &str = "provider_share_import_confirm";

fn export_serialized_provider(
    db: &crate::db::Db,
    provider_id: i64,
) -> AppResult<(Vec<u8>, String)> {
    let envelope = export_provider_share_v2(db, provider_id)?;
    let filename =
        provider_share_default_filename(&envelope.provider.cli_key, &envelope.provider.name);
    let bytes = serialize_provider_share_v2(&envelope)?;
    Ok((bytes, filename))
}

fn share_resource(provider_id: i64) -> String {
    format!("provider:{provider_id}:share")
}

fn preview_resource(preview_token: &str) -> String {
    format!("provider-share-preview:{preview_token}")
}

fn clipboard_matches_expected(current: &str, expected: &str) -> bool {
    current == expected
}

fn schedule_conditional_clipboard_clear(app: tauri::AppHandle, expected: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLIPBOARD_CLEAR_DELAY).await;
        let result = blocking::run(
            "provider_share_clipboard_clear",
            move || -> AppResult<bool> {
                let current = app.clipboard().read_text().map_err(|_| {
                    AppError::new(
                        "SYSTEM_ERROR",
                        "failed to inspect clipboard for provider share cleanup",
                    )
                })?;
                if !clipboard_matches_expected(&current, &expected) {
                    return Ok(false);
                }
                app.clipboard().clear().map_err(|_| {
                    AppError::new(
                        "SYSTEM_ERROR",
                        "failed to clear clipboard after provider share copy",
                    )
                })?;
                Ok(true)
            },
        )
        .await;
        if result.is_err() {
            tracing::warn!("provider share clipboard cleanup failed");
        }
    });
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_share_copy_to_clipboard(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<bool, String> {
    RiskyIpcConfirm::require(confirm, COPY_ACTION, share_resource(provider_id))?;
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let (bytes, _) = blocking::run("provider_share_copy_to_clipboard", move || {
        export_serialized_provider(&db, provider_id)
    })
    .await?;
    let content = String::from_utf8(bytes).map_err(|_| {
        "SYSTEM_ERROR: failed to encode provider share clipboard content".to_string()
    })?;
    app.clipboard()
        .write_text(Cow::Owned(content.clone()))
        .map_err(|_| "SYSTEM_ERROR: failed to write provider share to clipboard".to_string())?;
    schedule_conditional_clipboard_clear(app, content);
    tracing::info!(provider_id, "provider share copied to clipboard");
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_share_save_to_file(
    window: WebviewWindow,
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<bool, String> {
    RiskyIpcConfirm::require(confirm, SAVE_ACTION, share_resource(provider_id))?;
    let db = ensure_db_ready(app, db_state.inner()).await?;
    let (bytes, filename) = blocking::run("provider_share_save_to_file_export", move || {
        export_serialized_provider(&db, provider_id)
    })
    .await?;

    let mut dialog = window
        .dialog()
        .file()
        .set_title("保存供应商分享")
        .set_file_name(filename)
        .add_filter("JSON", &["json"]);
    #[cfg(any(windows, target_os = "macos"))]
    {
        dialog = dialog.set_parent(&window);
    }
    let (tx, rx) = oneshot::channel();
    dialog.save_file(move |selection| {
        let _ = tx.send(selection.map(|path| PathBuf::from(path.to_string())));
    });
    let Some(path) = rx
        .await
        .map_err(|_| "SYSTEM_ERROR: provider share save dialog response dropped".to_string())?
    else {
        return Ok(false);
    };

    blocking::run("provider_share_save_to_file_write", move || {
        write_authorized_share_file(&path, &bytes)
    })
    .await?;
    tracing::info!(provider_id, "provider share saved to file");
    Ok(true)
}

fn write_authorized_share_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    crate::shared::fs::write_file_atomic(path, bytes).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to save provider share to the selected file",
        )
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_share_import_preview_from_file(
    window: WebviewWindow,
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    share_state: tauri::State<'_, ProviderShareService>,
) -> Result<Option<ProviderShareImportPreview>, String> {
    let mut dialog = window
        .dialog()
        .file()
        .set_title("导入供应商")
        .add_filter("JSON", &["json"]);
    #[cfg(any(windows, target_os = "macos"))]
    {
        dialog = dialog.set_parent(&window);
    }
    let (tx, rx) = oneshot::channel();
    dialog.pick_file(move |selection| {
        let _ = tx.send(selection.map(|path| PathBuf::from(path.to_string())));
    });
    let Some(path) = rx
        .await
        .map_err(|_| "SYSTEM_ERROR: provider share open dialog response dropped".to_string())?
    else {
        return Ok(None);
    };

    let db = ensure_db_ready(app, db_state.inner()).await?;
    let share_service = share_state.inner().clone();
    let preview = blocking::run("provider_share_import_preview_from_file", move || {
        share_service.preview_file(&db, path)
    })
    .await?;
    Ok(Some(preview))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_share_import_preview_from_content(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    share_state: tauri::State<'_, ProviderShareService>,
    content: String,
) -> Result<ProviderShareImportPreview, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    let share_service = share_state.inner().clone();
    blocking::run("provider_share_import_preview_from_content", move || {
        share_service.preview_content(&db, content.as_bytes())
    })
    .await
    .map_err(String::from)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_share_import_confirm(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    share_state: tauri::State<'_, ProviderShareService>,
    preview_token: String,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<ProviderSummary, String> {
    RiskyIpcConfirm::require(confirm, IMPORT_ACTION, preview_resource(&preview_token))?;
    let pending = share_state.take_preview(&preview_token)?;
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("provider_share_import_confirm", move || {
        pending.verify_and_import(&db)
    })
    .await
    .map_err(String::from)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn provider_share_import_preview_discard(
    share_state: tauri::State<'_, ProviderShareService>,
    preview_token: String,
) -> Result<bool, String> {
    share_state.discard(&preview_token).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_cleanup_only_matches_exact_share_content() {
        assert!(clipboard_matches_expected(
            "{\"share\":1}\n",
            "{\"share\":1}\n"
        ));
        assert!(!clipboard_matches_expected("later copy", "{\"share\":1}\n"));
        assert!(!clipboard_matches_expected(
            "{\"share\":1}",
            "{\"share\":1}\n"
        ));
    }

    #[test]
    fn write_error_does_not_disclose_selected_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("SYNTHETIC_PRIVATE");
        std::fs::create_dir(&path).expect("create target directory");
        std::fs::write(path.join("keep.txt"), b"keep").expect("make target non-empty");
        let error = write_authorized_share_file(&path, b"{}")
            .expect_err("unavailable path must fail")
            .to_string();
        assert!(!error.contains("SYNTHETIC_PRIVATE"));
    }
}
