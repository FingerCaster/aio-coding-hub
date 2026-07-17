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
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::transport::ImageGenFetchedImage;

const MAX_PERSIST_TOTAL_BYTES: usize = 96 * 1024 * 1024;
const MAX_READ_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TASK_ID_CHARS: usize = 128;
const MAX_LIST_LIMIT: u32 = 100;
const MAX_CURSOR_BYTES: usize = 512;
const CURSOR_VERSION: u8 = 1;
const MAX_TRUSTED_STORAGE_ROOTS: usize = 64;

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
    /// Opaque backend-validated reference (`task-id/filename`), never a path.
    pub path: String,
    /// Opaque backend-validated thumbnail reference.
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
pub(crate) struct ImageGenTasksPage {
    pub items: Vec<ImageGenTaskRow>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageGenTasksCursor {
    v: u8,
    created_at: i64,
    id: String,
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

pub(crate) fn storage_roots_from_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Vec<PathBuf>> {
    let settings = crate::settings::read(app)?;
    let current = storage_dir_from_settings(app)?;
    let mut roots = settings
        .image_gen_storage_roots
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    roots.push(current);
    canonical_storage_roots(&roots)
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
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .map_err(|e| format!("SYSTEM_ERROR: failed to create task file {name}: {e}"))?;
    file.write_all(bytes)
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

pub(crate) fn canonical_storage_roots(storage_roots: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    if storage_roots.len() > MAX_TRUSTED_STORAGE_ROOTS {
        return Err("SEC_INVALID_INPUT: too many image gen storage roots"
            .to_string()
            .into());
    }
    let mut canonical = Vec::with_capacity(storage_roots.len());
    for root in storage_roots {
        if !root.is_absolute() {
            return Err("SEC_INVALID_INPUT: image gen storage root must be absolute"
                .to_string()
                .into());
        }
        if !root.exists() {
            continue;
        }
        let root = canonical_storage_root(root)?;
        if !canonical.contains(&root) {
            canonical.push(root);
        }
    }
    if canonical.is_empty() {
        return Err("SEC_INVALID_INPUT: no trusted image gen storage roots"
            .to_string()
            .into());
    }
    Ok(canonical)
}

fn validate_task_dir(storage_roots: &[PathBuf], dir: &str, id: &str) -> AppResult<PathBuf> {
    let id = validate_task_id(id)?;
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
    if !storage_roots
        .iter()
        .any(|root| candidate.parent() == Some(root.as_path()))
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
    {
        let conn = db.open_connection()?;
        let already_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM image_gen_tasks WHERE id = ?1)",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .map_err(|e| db_err!("failed to check image gen task id: {e}"))?;
        if already_exists {
            return Err("SEC_INVALID_INPUT: image gen task id already exists"
                .to_string()
                .into());
        }
    }
    std::fs::create_dir(&task_dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "SEC_INVALID_INPUT: image gen task directory already exists".to_string()
        } else {
            format!("SYSTEM_ERROR: failed to create task dir: {e}")
        }
    })?;
    if let Err(error) = validate_task_dir(
        std::slice::from_ref(&storage_root),
        &task_dir.to_string_lossy(),
        &id,
    ) {
        let _ = std::fs::remove_dir(&task_dir);
        return Err(error);
    }

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

    task_get(db, &storage_root, &id)?
        .ok_or_else(|| "DB_ERROR: persisted image gen task not found".into())
}

struct RawTaskRow {
    id: String,
    adapter_id: String,
    prompt: String,
    request_json: String,
    status: String,
    error: Option<String>,
    usage_json: Option<String>,
    images_json: String,
    ref_images_json: String,
    dir: String,
    created_at: i64,
    elapsed_ms: Option<i64>,
}

fn row_to_raw_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTaskRow> {
    Ok(RawTaskRow {
        id: row.get("id")?,
        adapter_id: row.get("adapter_id")?,
        prompt: row.get("prompt")?,
        request_json: row.get("request_json")?,
        status: row.get("status")?,
        error: row.get("error")?,
        usage_json: row.get("usage_json")?,
        images_json: row.get("images_json")?,
        ref_images_json: row.get("ref_images_json")?,
        dir: row.get("dir")?,
        created_at: row.get("created_at")?,
        elapsed_ms: row.get("elapsed_ms")?,
    })
}

fn validate_stored_file(
    task_dir: &Path,
    task_id: &str,
    stored: StoredFile,
    referenced_paths: &mut Vec<(String, PathBuf)>,
) -> AppResult<ImageGenTaskFileRow> {
    let file = safe_stored_file_name(&stored.file)
        .ok_or_else(|| "SEC_INVALID_INPUT: unsafe stored image filename".to_string())?;
    let path = validate_stored_file_path(task_dir, file)?;
    let reference = format!("{task_id}/{file}");
    referenced_paths.push((reference.clone(), path));

    let thumb_path = if let Some(thumb) = stored.thumb {
        let thumb = safe_stored_file_name(&thumb)
            .ok_or_else(|| "SEC_INVALID_INPUT: unsafe stored thumbnail filename".to_string())?;
        let path = validate_stored_file_path(task_dir, thumb)?;
        let reference = format!("{task_id}/{thumb}");
        referenced_paths.push((reference.clone(), path));
        Some(reference)
    } else {
        None
    };
    Ok(ImageGenTaskFileRow {
        path: reference,
        thumb_path,
        mime: stored.mime,
    })
}

fn validate_stored_file_path(task_dir: &Path, filename: &str) -> AppResult<PathBuf> {
    let candidate = task_dir.join(filename);
    let link_metadata = std::fs::symlink_metadata(&candidate)
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be resolved".to_string())?;
    if link_metadata.file_type().is_symlink() {
        return Err("SEC_INVALID_INPUT: stored image file cannot be a symlink"
            .to_string()
            .into());
    }
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be resolved".to_string())?;
    if canonical.parent() != Some(task_dir) || !canonical.is_file() {
        return Err(
            "SEC_INVALID_INPUT: stored image file is outside its task directory"
                .to_string()
                .into(),
        );
    }
    Ok(canonical)
}

fn validate_raw_task(
    storage_roots: &[PathBuf],
    raw: RawTaskRow,
) -> AppResult<(ImageGenTaskRow, Vec<(String, PathBuf)>)> {
    let id = validate_task_id(&raw.id)?.to_string();
    let task_dir = validate_task_dir(storage_roots, &raw.dir, &id)?;
    let images = serde_json::from_str::<Vec<StoredFile>>(&raw.images_json)
        .map_err(|_| "SEC_INVALID_INPUT: invalid stored image metadata".to_string())?;
    let ref_images = serde_json::from_str::<Vec<StoredFile>>(&raw.ref_images_json)
        .map_err(|_| "SEC_INVALID_INPUT: invalid stored reference metadata".to_string())?;
    let mut referenced_paths = Vec::new();
    let images = images
        .into_iter()
        .map(|stored| validate_stored_file(&task_dir, &id, stored, &mut referenced_paths))
        .collect::<AppResult<Vec<_>>>()?;
    let ref_images = ref_images
        .into_iter()
        .map(|stored| validate_stored_file(&task_dir, &id, stored, &mut referenced_paths))
        .collect::<AppResult<Vec<_>>>()?;
    Ok((
        ImageGenTaskRow {
            id: id.clone(),
            adapter_id: raw.adapter_id,
            prompt: raw.prompt,
            request_json: raw.request_json,
            status: raw.status,
            error: raw.error,
            usage_json: raw.usage_json,
            images,
            ref_images,
            dir: id,
            created_at: raw.created_at,
            elapsed_ms: raw.elapsed_ms,
        },
        referenced_paths,
    ))
}

const TASK_SELECT_COLUMNS: &str = "id, adapter_id, prompt, request_json, status, error, usage_json, images_json, ref_images_json, dir, created_at, elapsed_ms";

fn decode_tasks_cursor(cursor: Option<&str>) -> AppResult<Option<ImageGenTasksCursor>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let cursor = cursor.trim();
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES {
        return Err("SEC_INVALID_INPUT: invalid image gen tasks cursor"
            .to_string()
            .into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor.as_bytes())
        .map_err(|_| "SEC_INVALID_INPUT: invalid image gen tasks cursor".to_string())?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err("SEC_INVALID_INPUT: invalid image gen tasks cursor"
            .to_string()
            .into());
    }
    let decoded: ImageGenTasksCursor = serde_json::from_slice(&bytes)
        .map_err(|_| "SEC_INVALID_INPUT: invalid image gen tasks cursor".to_string())?;
    if decoded.v != CURSOR_VERSION {
        return Err(
            "SEC_INVALID_INPUT: unsupported image gen tasks cursor version"
                .to_string()
                .into(),
        );
    }
    validate_task_id(&decoded.id)?;
    Ok(Some(decoded))
}

fn encode_tasks_cursor(row: &ImageGenTaskRow) -> AppResult<String> {
    let bytes = serde_json::to_vec(&ImageGenTasksCursor {
        v: CURSOR_VERSION,
        created_at: row.created_at,
        id: row.id.clone(),
    })
    .map_err(|e| format!("SYSTEM_ERROR: failed to encode image gen tasks cursor: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn task_get(db: &db::Db, storage_root: &Path, id: &str) -> AppResult<Option<ImageGenTaskRow>> {
    let roots = canonical_storage_roots(&[storage_root.to_path_buf()])?;
    let conn = db.open_connection()?;
    let raw = conn
        .query_row(
            &format!("SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks WHERE id = ?1"),
            rusqlite::params![id],
            row_to_raw_task,
        )
        .optional()
        .map_err(|e| db_err!("failed to query image gen task: {e}"))?;
    raw.map(|raw| validate_raw_task(&roots, raw).map(|(row, _)| row))
        .transpose()
}

/// Newest-first page. `before_created_at = None` starts from the newest row.
#[cfg(test)]
pub(crate) fn tasks_list(
    db: &db::Db,
    storage_root: &Path,
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>> {
    tasks_list_with_roots(db, &[storage_root.to_path_buf()], before_created_at, limit)
}

#[cfg(test)]
pub(crate) fn tasks_list_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
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
        .query_map(rusqlite::params![before_created_at, limit], row_to_raw_task)
        .map_err(|e| db_err!("failed to query image gen tasks: {e}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| db_err!("failed to read image gen task row: {e}"))?;
        tasks.push(validate_raw_task(&storage_roots, raw)?.0);
    }
    Ok(tasks)
}

pub(crate) fn tasks_page_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    cursor: Option<&str>,
    limit: u32,
) -> AppResult<ImageGenTasksPage> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let cursor = decode_tasks_cursor(cursor)?;
    let before_created_at = cursor.as_ref().map(|cursor| cursor.created_at);
    let before_id = cursor.as_ref().map(|cursor| cursor.id.as_str());
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let query_limit = i64::from(limit) + 1;
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare(&format!(
            r#"
SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks
WHERE (
  ?1 IS NULL
  OR created_at < ?1
  OR (created_at = ?1 AND id < ?2)
)
ORDER BY created_at DESC, id DESC
LIMIT ?3
"#
        ))
        .map_err(|e| db_err!("failed to prepare image gen tasks page query: {e}"))?;
    let rows = stmt
        .query_map(
            rusqlite::params![before_created_at, before_id, query_limit],
            row_to_raw_task,
        )
        .map_err(|e| db_err!("failed to query image gen tasks page: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| db_err!("failed to read image gen task page row: {e}"))?;
        items.push(validate_raw_task(&storage_roots, raw)?.0);
    }
    let has_more = items.len() > limit as usize;
    if has_more {
        items.pop();
    }
    let next_cursor = if has_more {
        items.last().map(encode_tasks_cursor).transpose()?
    } else {
        None
    };
    Ok(ImageGenTasksPage { items, next_cursor })
}

/// Deletes the DB row and the task directory. Missing row / missing dir are
/// both tolerated (idempotent delete).
#[cfg(test)]
pub(crate) fn task_delete(db: &db::Db, storage_root: &Path, id: &str) -> AppResult<()> {
    task_delete_with_roots(db, &[storage_root.to_path_buf()], id)
}

pub(crate) fn task_delete_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    id: &str,
) -> AppResult<()> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
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

    let validated = validate_task_dir(&storage_roots, &dir, id)?;
    remove_validated_task_dir(&validated)?;
    conn.execute(
        "DELETE FROM image_gen_tasks WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| db_err!("failed to delete image gen task: {e}"))?;
    Ok(())
}

/// Deletes every task row and its directory. Returns the number of deleted rows.
#[cfg(test)]
pub(crate) fn tasks_clear(db: &db::Db, storage_root: &Path) -> AppResult<u32> {
    tasks_clear_with_roots(db, &[storage_root.to_path_buf()])
}

pub(crate) fn tasks_clear_with_roots(db: &db::Db, storage_roots: &[PathBuf]) -> AppResult<u32> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        "SELECT id, dir FROM image_gen_tasks",
        rusqlite::params![],
    )?;

    let validated = pairs
        .iter()
        .map(|(id, dir)| validate_task_dir(&storage_roots, dir, id))
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
#[cfg(test)]
pub(crate) fn storage_cleanup(db: &db::Db, storage_root: &Path, keep_count: u32) -> AppResult<u32> {
    storage_cleanup_with_roots(db, &[storage_root.to_path_buf()], keep_count)
}

pub(crate) fn storage_cleanup_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    keep_count: u32,
) -> AppResult<u32> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
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
        .map(|(id, dir)| validate_task_dir(&storage_roots, dir, id))
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
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
        || path.components().count() != 1
    {
        return None;
    }
    Some(value)
}

/// Reads a stored image back as base64. Security boundary: the canonicalized
/// path must live strictly inside the current storage dir or a DB-recorded task
/// dir (canonicalization also rejects `..` traversal and symlink escapes).
#[cfg(test)]
pub(crate) fn read_image(
    db: &db::Db,
    storage_dir: &Path,
    reference: &str,
) -> AppResult<ImageGenFetchedImage> {
    read_image_with_roots(db, &[storage_dir.to_path_buf()], reference)
}

pub(crate) fn read_image_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    reference: &str,
) -> AppResult<ImageGenFetchedImage> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let reference = reference.trim();
    let (task_id, filename) = reference
        .split_once('/')
        .ok_or_else(|| "SEC_INVALID_INPUT: invalid image reference".to_string())?;
    validate_task_id(task_id)?;
    safe_stored_file_name(filename)
        .ok_or_else(|| "SEC_INVALID_INPUT: invalid image reference".to_string())?;
    let conn = db.open_connection()?;
    let raw = conn
        .query_row(
            &format!("SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks WHERE id = ?1"),
            rusqlite::params![task_id],
            row_to_raw_task,
        )
        .optional()
        .map_err(|e| db_err!("failed to query image gen task: {e}"))?
        .ok_or_else(|| "SEC_INVALID_INPUT: image reference task was not found".to_string())?;
    let (_, referenced_paths) = validate_raw_task(&storage_roots, raw)?;
    let canonical = referenced_paths
        .into_iter()
        .find_map(|(candidate, path)| (candidate == reference).then_some(path))
        .ok_or_else(|| {
            "SEC_INVALID_INPUT: image reference is not present in trusted metadata".to_string()
        })?;

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
#[cfg(test)]
pub(crate) fn storage_stats(db: &db::Db, storage_dir: &Path) -> AppResult<ImageGenStorageView> {
    storage_stats_with_roots(db, storage_dir, &[storage_dir.to_path_buf()])
}

pub(crate) fn storage_stats_with_roots(
    db: &db::Db,
    current_storage_dir: &Path,
    storage_roots: &[PathBuf],
) -> AppResult<ImageGenStorageView> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
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
        let dir = validate_task_dir(&storage_roots, &dir, &id)?;
        total_bytes = total_bytes.saturating_add(dir_size_bytes(&dir));
    }

    Ok(ImageGenStorageView {
        dir: current_storage_dir.to_string_lossy().to_string(),
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
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| format!("SEC_INVALID_INPUT: storage dir is not writable: {e}"))?;
    let write_result = file.write_all(b"probe");
    drop(file);
    let _ = std::fs::remove_file(&probe);
    write_result.map_err(|e| format!("SEC_INVALID_INPUT: storage dir is not writable: {e}"))?;
    Ok(())
}
