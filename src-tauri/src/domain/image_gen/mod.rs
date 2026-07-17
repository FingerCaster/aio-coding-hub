//! Usage: Image generation adapter config persistence, pure HTTP transport
//! helpers, and task history persistence (files on disk + rows in SQLite).
//!
//! The API key is read from SQLite and injected into outbound requests here; it
//! never crosses the IPC boundary in either direction.

mod config;
mod history;
mod transport;

pub(crate) use config::{config_connection, config_get, config_set, ImageGenConfigView};
pub(crate) use history::{
    canonical_storage_roots, ensure_writable_dir, read_image_with_roots,
    read_images_with_budget_with_roots, storage_cleanup_with_roots, storage_dir_from_settings,
    storage_roots_from_settings, storage_stats_with_roots, task_delete_with_roots, task_persist,
    tasks_clear_with_roots, tasks_page_with_roots, ImageGenStorageView, ImageGenTaskPersistPayload,
    ImageGenTaskRow, ImageGenTasksPage, HISTORY_HYDRATE_PER_IMAGE_BYTES,
    HISTORY_HYDRATE_TOTAL_BYTES,
};
pub(crate) use transport::{
    fetch_image, post_json, post_multipart, ImageGenFetchedImage, ImageGenHttpResponse,
    ImageGenMultipartFile,
};

#[cfg(test)]
mod tests;
