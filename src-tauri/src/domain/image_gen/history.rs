//! Usage: image_gen_tasks history persistence: task files on disk (original +
//! thumbnail + reference images per task directory) and metadata rows in SQLite.
//!
//! Path safety: `read_image` only serves files whose canonical path lives inside
//! the current storage dir or a task dir recorded in the DB; task dir removal is
//! guarded by a dir-name == task-id check.

use crate::db;
use crate::shared::error::{db_err, AppResult};
use base64::Engine as _;
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

use super::transport::ImageGenFetchedImage;

const MAX_PERSIST_TOTAL_BYTES: usize = 96 * 1024 * 1024;
const MAX_READ_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TASK_ID_CHARS: usize = 128;
const MAX_LIST_LIMIT: u32 = 100;

pub(crate) const IMAGE_GEN_STORAGE_DIR_NAME: &str = "image-gen";

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskFilePayload {
    pub mime: String,
    pub data_b64: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskPersistPayload {
    pub id: String,
    pub adapter_id: Option<String>,
    pub prompt: String,
    pub request_json: String,
    pub status: String,
    pub error: Option<String>,
    pub usage_json: Option<String>,
    pub created_at: i64,
    pub elapsed_ms: Option<i64>,
    pub images: Vec<ImageGenTaskFilePayload>,
    /// Frontend-generated thumbnails, paired with `images` by index. Fewer
    /// thumbs than images is tolerated (missing thumb -> no thumb path).
    pub thumbs: Vec<ImageGenTaskFilePayload>,
    pub ref_images: Vec<ImageGenTaskFilePayload>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskFileRow {
    /// Absolute path of the stored file.
    pub path: String,
    /// Absolute path of the thumbnail (generated images only).
    pub thumb_path: Option<String>,
    pub mime: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskRow {
    pub id: String,
    pub adapter_id: String,
    pub prompt: String,
    pub request_json: String,
    pub status: String,
    pub error: Option<String>,
    pub usage_json: Option<String>,
    pub images: Vec<ImageGenTaskFileRow>,
    pub ref_images: Vec<ImageGenTaskFileRow>,
    pub dir: String,
    pub created_at: i64,
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenStorageView {
    pub dir: String,
    pub total_bytes: i64,
    pub task_count: u32,
}

/// On-disk file reference stored inside images_json / ref_images_json
/// (file names relative to the task dir).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredFile {
    file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumb: Option<String>,
    mime: String,
}

/// Resolves the effective storage directory: settings override or the default
/// `<app data dir>/image-gen`.
pub(crate) fn storage_dir_from_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<PathBuf> {
    let settings = crate::settings::read(app)?;
    match settings
        .image_gen_storage_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
    {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(crate::app_paths::app_data_dir(app)?.join(IMAGE_GEN_STORAGE_DIR_NAME)),
    }
}

fn validate_task_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("SEC_INVALID_INPUT: task id is required".to_string());
    }
    if id.len() > MAX_TASK_ID_CHARS {
        return Err("SEC_INVALID_INPUT: task id is too long".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("SEC_INVALID_INPUT: task id contains invalid characters".to_string());
    }
    Ok(id)
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

fn mime_for_ext(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

fn decode_payload_files(
    label: &str,
    files: &[ImageGenTaskFilePayload],
    total_bytes: &mut usize,
) -> Result<Vec<Vec<u8>>, String> {
    let mut decoded = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        // Cheap pre-check on the encoded length so oversized payloads are
        // rejected before allocating the decoded buffer.
        let estimated_bytes = file.data_b64.len() / 4 * 3;
        if total_bytes.saturating_add(estimated_bytes) > MAX_PERSIST_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: persist payload exceeds {MAX_PERSIST_TOTAL_BYTES} bytes limit"
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.data_b64.as_bytes())
            .map_err(|e| format!("SEC_INVALID_INPUT: {label} #{index} data_b64 is invalid: {e}"))?;
        *total_bytes = total_bytes.saturating_add(bytes.len());
        if *total_bytes > MAX_PERSIST_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: persist payload exceeds {MAX_PERSIST_TOTAL_BYTES} bytes limit"
            ));
        }
        decoded.push(bytes);
    }
    Ok(decoded)
}

fn write_task_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(dir.join(name), bytes)
        .map_err(|e| format!("SYSTEM_ERROR: failed to write task file {name}: {e}"))
}

pub(crate) fn canonical_storage_root(storage_dir: &Path) -> AppResult<PathBuf> {
    let root = std::fs::canonicalize(storage_dir)
        .map_err(|_| "SEC_INVALID_INPUT: image gen storage root cannot be resolved".to_string())?;
    let metadata = std::fs::metadata(&root)
        .map_err(|_| "SEC_INVALID_INPUT: image gen storage root cannot be inspected".to_string())?;
    if !metadata.is_dir() {
        return Err(
            "SEC_INVALID_INPUT: image gen storage root is not a directory"
                .to_string()
                .into(),
        );
    }
    Ok(root)
}

fn validate_task_dir(storage_root: &Path, dir: &str, id: &str) -> AppResult<PathBuf> {
    let id = validate_task_id(id)?;
    let root = canonical_storage_root(storage_root)?;
    let original = PathBuf::from(dir);
    let link_metadata = std::fs::symlink_metadata(&original)
        .map_err(|_| "SEC_INVALID_INPUT: image gen task dir cannot be resolved".to_string())?;
    if link_metadata.file_type().is_symlink() {
        return Err("SEC_INVALID_INPUT: image gen task dir cannot be a symlink"
            .to_string()
            .into());
    }
    let candidate = std::fs::canonicalize(&original)
        .map_err(|_| "SEC_INVALID_INPUT: image gen task dir cannot be resolved".to_string())?;
    if candidate.parent() != Some(root.as_path())
        || candidate.file_name().and_then(|value| value.to_str()) != Some(id)
        || !candidate.is_dir()
    {
        return Err(
            "SEC_INVALID_INPUT: image gen task dir is outside the trusted storage root"
                .to_string()
                .into(),
        );
    }
    Ok(candidate)
}

fn remove_validated_task_dir(path: &Path) -> AppResult<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("SYSTEM_ERROR: failed to remove task dir: {e}").into()),
    }
}

/// Writes task files to `{storage_dir}/{id}` then upserts the DB row.
/// Files are written first; if the DB write fails the task dir is removed
/// so no orphan directory survives a partial persist.
pub(crate) fn task_persist(
    db: &db::Db,
    storage_dir: &Path,
    payload: ImageGenTaskPersistPayload,
) -> AppResult<ImageGenTaskRow> {
    let id = validate_task_id(&payload.id)?.to_string();
    let adapter_id = payload
        .adapter_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gpt-image")
        .to_string();
    if payload.status != "done" && payload.status != "error" {
        return Err("SEC_INVALID_INPUT: status must be 'done' or 'error'"
            .to_string()
            .into());
    }
    if payload.thumbs.len() > payload.images.len() {
        return Err("SEC_INVALID_INPUT: more thumbs than images"
            .to_string()
            .into());
    }

    let mut total_bytes = 0usize;
    let images = decode_payload_files("image", &payload.images, &mut total_bytes)?;
    let thumbs = decode_payload_files("thumb", &payload.thumbs, &mut total_bytes)?;
    let ref_images = decode_payload_files("ref image", &payload.ref_images, &mut total_bytes)?;

    ensure_writable_dir(storage_dir)?;
    let storage_root = canonical_storage_root(storage_dir)?;
    let task_dir = storage_root.join(&id);
    if task_dir.exists()
        && std::fs::symlink_metadata(&task_dir)
            .map_err(|e| format!("SYSTEM_ERROR: failed to inspect task dir: {e}"))?
            .file_type()
            .is_symlink()
    {
        return Err("SEC_INVALID_INPUT: task dir cannot be a symlink"
            .to_string()
            .into());
    }
    std::fs::create_dir_all(&task_dir)
        .map_err(|e| format!("SYSTEM_ERROR: failed to create task dir: {e}"))?;
    validate_task_dir(&storage_root, &task_dir.to_string_lossy(), &id)?;

    let write_and_insert = || -> AppResult<()> {
        let mut stored_images = Vec::with_capacity(images.len());
        for (index, bytes) in images.iter().enumerate() {
            let mime = &payload.images[index].mime;
            let file = format!("image-{}.{}", index + 1, ext_for_mime(mime));
            write_task_file(&task_dir, &file, bytes)?;
            let thumb = match thumbs.get(index) {
                Some(thumb_bytes) => {
                    let thumb_mime = &payload.thumbs[index].mime;
                    let thumb_file = format!("thumb-{}.{}", index + 1, ext_for_mime(thumb_mime));
                    write_task_file(&task_dir, &thumb_file, thumb_bytes)?;
                    Some(thumb_file)
                }
                None => None,
            };
            stored_images.push(StoredFile {
                file,
                thumb,
                mime: mime.clone(),
            });
        }

        let mut stored_refs = Vec::with_capacity(ref_images.len());
        for (index, bytes) in ref_images.iter().enumerate() {
            let mime = &payload.ref_images[index].mime;
            let file = format!("ref-{}.{}", index + 1, ext_for_mime(mime));
            write_task_file(&task_dir, &file, bytes)?;
            stored_refs.push(StoredFile {
                file,
                thumb: None,
                mime: mime.clone(),
            });
        }

        let images_json = serde_json::to_string(&stored_images)
            .map_err(|e| format!("SYSTEM_ERROR: failed to serialize images_json: {e}"))?;
        let ref_images_json = serde_json::to_string(&stored_refs)
            .map_err(|e| format!("SYSTEM_ERROR: failed to serialize ref_images_json: {e}"))?;

        let conn = db.open_connection()?;
        conn.execute(
            r#"
INSERT INTO image_gen_tasks(
  id, adapter_id, prompt, request_json, status, error, usage_json,
  images_json, ref_images_json, dir, created_at, elapsed_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(id) DO UPDATE SET
  adapter_id = excluded.adapter_id,
  prompt = excluded.prompt,
  request_json = excluded.request_json,
  status = excluded.status,
  error = excluded.error,
  usage_json = excluded.usage_json,
  images_json = excluded.images_json,
  ref_images_json = excluded.ref_images_json,
  dir = excluded.dir,
  created_at = excluded.created_at,
  elapsed_ms = excluded.elapsed_ms
"#,
            rusqlite::params![
                id,
                adapter_id,
                payload.prompt,
                payload.request_json,
                payload.status,
                payload.error,
                payload.usage_json,
                images_json,
                ref_images_json,
                task_dir.to_string_lossy().to_string(),
                payload.created_at,
                payload.elapsed_ms,
            ],
        )
        .map_err(|e| db_err!("failed to upsert image gen task: {e}"))?;
        Ok(())
    };

    if let Err(err) = write_and_insert() {
        // Roll back the on-disk side so no orphan dir survives a failed persist.
        let _ = std::fs::remove_dir_all(&task_dir);
        return Err(err);
    }

    task_get(db, &id)?.ok_or_else(|| "DB_ERROR: persisted image gen task not found".into())
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImageGenTaskRow> {
    let images_json: String = row.get("images_json")?;
    let ref_images_json: String = row.get("ref_images_json")?;
    let dir: String = row.get("dir")?;
    let dir_path = PathBuf::from(&dir);

    let to_rows = |json: &str| -> Vec<ImageGenTaskFileRow> {
        serde_json::from_str::<Vec<StoredFile>>(json)
            .unwrap_or_default()
            .into_iter()
            .map(|stored| ImageGenTaskFileRow {
                path: dir_path.join(&stored.file).to_string_lossy().to_string(),
                thumb_path: stored
                    .thumb
                    .map(|thumb| dir_path.join(thumb).to_string_lossy().to_string()),
                mime: stored.mime,
            })
            .collect()
    };

    Ok(ImageGenTaskRow {
        id: row.get("id")?,
        adapter_id: row.get("adapter_id")?,
        prompt: row.get("prompt")?,
        request_json: row.get("request_json")?,
        status: row.get("status")?,
        error: row.get("error")?,
        usage_json: row.get("usage_json")?,
        images: to_rows(&images_json),
        ref_images: to_rows(&ref_images_json),
        dir,
        created_at: row.get("created_at")?,
        elapsed_ms: row.get("elapsed_ms")?,
    })
}

const TASK_SELECT_COLUMNS: &str = "id, adapter_id, prompt, request_json, status, error, usage_json, images_json, ref_images_json, dir, created_at, elapsed_ms";

fn task_get(db: &db::Db, id: &str) -> AppResult<Option<ImageGenTaskRow>> {
    let conn = db.open_connection()?;
    conn.query_row(
        &format!("SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks WHERE id = ?1"),
        rusqlite::params![id],
        row_to_task,
    )
    .optional()
    .map_err(|e| db_err!("failed to query image gen task: {e}"))
}

/// Newest-first page. `before_created_at = None` starts from the newest row.
pub(crate) fn tasks_list(
    db: &db::Db,
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare(&format!(
            r#"
SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks
WHERE (?1 IS NULL OR created_at < ?1)
ORDER BY created_at DESC, id DESC
LIMIT ?2
"#
        ))
        .map_err(|e| db_err!("failed to prepare image gen tasks query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![before_created_at, limit], row_to_task)
        .map_err(|e| db_err!("failed to query image gen tasks: {e}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row.map_err(|e| db_err!("failed to read image gen task row: {e}"))?);
    }
    Ok(tasks)
}

/// Deletes the DB row and the task directory. Missing row / missing dir are
/// both tolerated (idempotent delete).
pub(crate) fn task_delete(db: &db::Db, storage_root: &Path, id: &str) -> AppResult<()> {
    let id = validate_task_id(id)?;
    let conn = db.open_connection()?;
    let dir: Option<String> = conn
        .query_row(
            "SELECT dir FROM image_gen_tasks WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| db_err!("failed to query image gen task dir: {e}"))?;

    let Some(dir) = dir else {
        return Ok(());
    };

    let validated = validate_task_dir(storage_root, &dir, id)?;
    remove_validated_task_dir(&validated)?;
    conn.execute(
        "DELETE FROM image_gen_tasks WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| db_err!("failed to delete image gen task: {e}"))?;
    Ok(())
}

/// Deletes every task row and its directory. Returns the number of deleted rows.
pub(crate) fn tasks_clear(db: &db::Db, storage_root: &Path) -> AppResult<u32> {
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        "SELECT id, dir FROM image_gen_tasks",
        rusqlite::params![],
    )?;

    let validated = pairs
        .iter()
        .map(|(id, dir)| validate_task_dir(storage_root, dir, id))
        .collect::<AppResult<Vec<_>>>()?;
    for path in &validated {
        remove_validated_task_dir(path)?;
    }
    conn.execute("DELETE FROM image_gen_tasks", [])
        .map_err(|e| db_err!("failed to clear image gen tasks: {e}"))?;
    Ok(pairs.len() as u32)
}

/// Keeps the most recent `keep_count` tasks and deletes the rest (rows + dirs).
/// Returns the number of deleted tasks.
pub(crate) fn storage_cleanup(db: &db::Db, storage_root: &Path, keep_count: u32) -> AppResult<u32> {
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        r#"
SELECT id, dir FROM image_gen_tasks
ORDER BY created_at DESC, id DESC
LIMIT -1 OFFSET ?1
"#,
        rusqlite::params![keep_count],
    )?;

    let validated = pairs
        .iter()
        .map(|(id, dir)| validate_task_dir(storage_root, dir, id))
        .collect::<AppResult<Vec<_>>>()?;
    for ((id, _), path) in pairs.iter().zip(validated.iter()) {
        remove_validated_task_dir(path)?;
        conn.execute(
            "DELETE FROM image_gen_tasks WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| db_err!("failed to delete image gen task during cleanup: {e}"))?;
    }
    Ok(pairs.len() as u32)
}

fn query_id_dir_pairs(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| db_err!("failed to prepare image gen task id/dir query: {e}"))?;
    let rows = stmt
        .query_map(params, |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| db_err!("failed to query image gen task id/dir pairs: {e}"))?;
    let mut pairs = Vec::new();
    for row in rows {
        pairs.push(row.map_err(|e| db_err!("failed to read image gen task id/dir row: {e}"))?);
    }
    Ok(pairs)
}

fn safe_stored_file_name(value: &str) -> Option<&str> {
    let path = Path::new(value);
    if value.is_empty() || path.file_name().and_then(|name| name.to_str()) != Some(value) {
        return None;
    }
    Some(value)
}

fn referenced_image_paths(db: &db::Db, storage_root: &Path) -> AppResult<Vec<PathBuf>> {
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare("SELECT id, dir, images_json, ref_images_json FROM image_gen_tasks")
        .map_err(|e| db_err!("failed to prepare image gen file references: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| db_err!("failed to query image gen file references: {e}"))?;
    let mut paths = Vec::new();
    for row in rows {
        let (id, dir, images_json, refs_json) =
            row.map_err(|e| db_err!("failed to read image gen file reference: {e}"))?;
        let task_dir = validate_task_dir(storage_root, &dir, &id)?;
        for stored in serde_json::from_str::<Vec<StoredFile>>(&images_json)
            .map_err(|_| "SEC_INVALID_INPUT: invalid stored image metadata".to_string())?
            .into_iter()
            .chain(
                serde_json::from_str::<Vec<StoredFile>>(&refs_json).map_err(|_| {
                    "SEC_INVALID_INPUT: invalid stored reference metadata".to_string()
                })?,
            )
        {
            let file = safe_stored_file_name(&stored.file)
                .ok_or_else(|| "SEC_INVALID_INPUT: unsafe stored image filename".to_string())?;
            paths.push(task_dir.join(file));
            if let Some(thumb) = stored.thumb {
                let thumb = safe_stored_file_name(&thumb).ok_or_else(|| {
                    "SEC_INVALID_INPUT: unsafe stored thumbnail filename".to_string()
                })?;
                paths.push(task_dir.join(thumb));
            }
        }
    }
    Ok(paths)
}

/// Reads a stored image back as base64. Security boundary: the canonicalized
/// path must live strictly inside the current storage dir or a DB-recorded task
/// dir (canonicalization also rejects `..` traversal and symlink escapes).
pub(crate) fn read_image(
    db: &db::Db,
    storage_dir: &Path,
    path: &str,
) -> AppResult<ImageGenFetchedImage> {
    let path = path.trim();
    if path.is_empty() {
        return Err("SEC_INVALID_INPUT: path is required".to_string().into());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| String::from("SEC_INVALID_INPUT: image path cannot be resolved"))?;

    let root = canonical_storage_root(storage_dir)?;
    if !canonical.starts_with(&root) || canonical == root {
        return Err(
            "SEC_INVALID_INPUT: image path is outside the image gen storage"
                .to_string()
                .into(),
        );
    }
    let referenced = referenced_image_paths(db, &root)?
        .into_iter()
        .filter_map(|candidate| std::fs::canonicalize(candidate).ok())
        .any(|candidate| candidate == canonical);
    if !referenced {
        return Err(
            "SEC_INVALID_INPUT: image path is not referenced by a trusted task"
                .to_string()
                .into(),
        );
    }

    let metadata = std::fs::metadata(&canonical)
        .map_err(|e| format!("SYSTEM_ERROR: failed to stat image file: {e}"))?;
    if !metadata.is_file() {
        return Err("SEC_INVALID_INPUT: image path is not a file"
            .to_string()
            .into());
    }
    if metadata.len() > MAX_READ_IMAGE_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: image file exceeds {MAX_READ_IMAGE_BYTES} bytes limit"
        )
        .into());
    }

    let bytes = std::fs::read(&canonical)
        .map_err(|e| format!("SYSTEM_ERROR: failed to read image file: {e}"))?;
    Ok(ImageGenFetchedImage {
        mime: mime_for_ext(&canonical).to_string(),
        data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// Storage usage view. Size is computed by walking DB-recorded task dirs.
// ponytail: orphan dirs (files written but row never landed) are not counted
// and cannot be reclaimed by cleanup; add an orphan scan if this ever matters.
pub(crate) fn storage_stats(db: &db::Db, storage_dir: &Path) -> AppResult<ImageGenStorageView> {
    let conn = db.open_connection()?;
    let task_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .map_err(|e| db_err!("failed to count image gen tasks: {e}"))?;
    drop(conn);

    let mut total_bytes: i64 = 0;
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        "SELECT id, dir FROM image_gen_tasks",
        rusqlite::params![],
    )?;
    for (id, dir) in pairs {
        let dir = validate_task_dir(storage_dir, &dir, &id)?;
        total_bytes = total_bytes.saturating_add(dir_size_bytes(&dir));
    }

    Ok(ImageGenStorageView {
        dir: storage_dir.to_string_lossy().to_string(),
        total_bytes,
        task_count,
    })
}

fn dir_size_bytes(dir: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total: i64 = 0;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            total = total.saturating_add(dir_size_bytes(&entry.path()));
        } else {
            total = total.saturating_add(metadata.len() as i64);
        }
    }
    total
}

/// Ensures the directory exists and is writable (probe file round-trip).
pub(crate) fn ensure_writable_dir(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("SEC_INVALID_INPUT: storage dir cannot be created: {e}"))?;
    let probe = dir.join(".aio-write-probe");
    std::fs::write(&probe, b"probe")
        .map_err(|e| format!("SEC_INVALID_INPUT: storage dir is not writable: {e}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
