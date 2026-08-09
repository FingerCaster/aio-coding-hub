//! Usage: Application-level maintenance coordinator for durable reset work.
//!
//! Reset is split across process boundaries. The request path records intent
//! and exits; the next process consumes the marker before normal runtime owners
//! such as logging, SQLite, or the gateway are created.

use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::{
    read_optional_file_with_max_len, rename_file_no_replace, write_file_atomic_create_new,
};
use crate::shared::mutex_ext::MutexExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use tauri::Manager;

const MAINTENANCE_DIR: &str = ".maintenance";
const RESET_MARKER_FILE: &str = "reset-app-data.pending";
const RESET_COMPLETED_FILE: &str = "reset-app-data.completed";
const RESET_MARKER_CONTENT: &[u8] = b"aio-coding-hub-reset-app-data-v1\n";
const RESET_MARKER_MAX_BYTES: usize = 128;
const MAINTENANCE_CLEAN: u8 = 0;
const MAINTENANCE_RUNNING: u8 = 1;
const MAINTENANCE_FAILED: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResetMarkerState {
    None,
    Pending,
    Completed,
}

#[derive(Default)]
pub(crate) struct MaintenanceState {
    phase: AtomicU8,
    reset_exit_requested: AtomicBool,
    reset_registration_required: AtomicBool,
    runtime_started: AtomicBool,
    coordinator_lock: Mutex<()>,
}

impl MaintenanceState {
    fn phase(&self) -> u8 {
        self.phase.load(Ordering::Acquire)
    }

    fn set_phase(&self, phase: u8) {
        self.phase.store(phase, Ordering::Release);
    }

    pub(crate) fn blocks_normal_operation(&self) -> bool {
        self.phase() != MAINTENANCE_CLEAN
    }

    fn can_retry(&self) -> bool {
        self.phase() == MAINTENANCE_FAILED && !self.reset_exit_requested.load(Ordering::Acquire)
    }

    fn allows_invoke(&self, command: &str) -> bool {
        if !self.blocks_normal_operation() {
            return true;
        }

        match command {
            "app_startup_status_get" | "app_exit" => true,
            "app_startup_retry" => self.can_retry(),
            _ => false,
        }
    }

    fn begin_reset_registration(&self) {
        self.reset_registration_required
            .store(true, Ordering::Release);
        self.set_phase(MAINTENANCE_RUNNING);
    }

    fn request_reset_exit(&self) {
        self.reset_registration_required
            .store(false, Ordering::Release);
        self.reset_exit_requested.store(true, Ordering::Release);
        self.set_phase(MAINTENANCE_RUNNING);
    }

    pub(crate) fn should_skip_exit_cleanup(&self) -> bool {
        self.reset_exit_requested.load(Ordering::Acquire) || self.blocks_normal_operation()
    }

    fn try_mark_runtime_started(&self) -> bool {
        if self.blocks_normal_operation() {
            return false;
        }
        self.runtime_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn lock_coordinator(&self) -> std::sync::MutexGuard<'_, ()> {
        self.coordinator_lock.lock_or_recover()
    }
}

pub(crate) fn ensure_normal_operation<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<()> {
    let Some(state) = app.try_state::<MaintenanceState>() else {
        return Ok(());
    };
    if state.blocks_normal_operation() {
        return Err(AppError::new(
            "APP_MAINTENANCE_REQUIRED",
            "应用正在维护中，请重试数据清理或退出",
        ));
    }
    Ok(())
}

pub(crate) fn invoke_allowed_during_maintenance<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    command: &str,
) -> bool {
    app.try_state::<MaintenanceState>()
        .is_none_or(|state| state.allows_invoke(command))
}

fn maintenance_dir_for_data_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(MAINTENANCE_DIR)
}

pub(crate) fn marker_path_for_data_dir(data_dir: &Path) -> PathBuf {
    maintenance_dir_for_data_dir(data_dir).join(RESET_MARKER_FILE)
}

fn completed_marker_path_for_data_dir(data_dir: &Path) -> PathBuf {
    maintenance_dir_for_data_dir(data_dir).join(RESET_COMPLETED_FILE)
}

fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    #[cfg(not(windows))]
    false
}

fn validate_owned_directory(path: &Path) -> AppResult<()> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| AppError::new("APP_MAINTENANCE_PATH_INVALID", "应用数据维护目录无法验证"))?;
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Err(AppError::new(
            "APP_MAINTENANCE_PATH_INVALID",
            "应用数据维护目录无法验证",
        ));
    }
    Ok(())
}

fn validate_data_dir(data_dir: &Path) -> AppResult<()> {
    validate_owned_directory(data_dir)
}

fn validate_existing_maintenance_dir(data_dir: &Path) -> AppResult<bool> {
    validate_data_dir(data_dir)?;
    let maintenance_dir = maintenance_dir_for_data_dir(data_dir);
    match std::fs::symlink_metadata(&maintenance_dir) {
        Ok(metadata) if metadata.is_dir() && !metadata_is_link_like(&metadata) => Ok(true),
        Ok(_) => Err(AppError::new(
            "APP_MAINTENANCE_PATH_INVALID",
            "应用数据维护目录无法验证",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(AppError::new(
            "APP_MAINTENANCE_PATH_INVALID",
            "应用数据维护目录无法验证",
        )),
    }
}

fn ensure_maintenance_dir(data_dir: &Path) -> AppResult<PathBuf> {
    let maintenance_dir = maintenance_dir_for_data_dir(data_dir);
    if !validate_existing_maintenance_dir(data_dir)? {
        match std::fs::create_dir(&maintenance_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                validate_owned_directory(&maintenance_dir)?;
            }
            Err(_) => {
                return Err(AppError::new(
                    "APP_MAINTENANCE_MARKER_FAILED",
                    "无法创建数据重置维护目录",
                ));
            }
        }
    }

    validate_owned_directory(&maintenance_dir)?;
    // Retry these syncs even when the directories already exist. A previous
    // registration may have created them but failed before their entries were
    // durably committed.
    if let Some(data_parent) = data_dir.parent() {
        sync_directory_for_reset(data_parent)?;
    }
    sync_directory_for_reset(data_dir)?;
    Ok(maintenance_dir)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?
        .sync_all()
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn sync_directory_for_reset(path: &Path) -> AppResult<()> {
    sync_directory(path)
        .map_err(|_| AppError::new("APP_MAINTENANCE_MARKER_FAILED", "数据重置持久化未完成"))
}

fn sync_parent_directory(path: &Path) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("APP_MAINTENANCE_MARKER_FAILED", "维护 marker 路径无效"))?;
    sync_directory_for_reset(parent)
}

fn sync_reset_target_directories(data_dir: &Path, db_path: &Path) -> AppResult<()> {
    sync_directory_for_reset(data_dir)?;
    if let Some(db_parent) = db_path.parent() {
        if db_parent != data_dir {
            sync_directory_for_reset(db_parent)?;
        }
    }
    Ok(())
}

fn marker_bytes(path: &Path) -> AppResult<Option<Vec<u8>>> {
    read_optional_file_with_max_len(path, RESET_MARKER_MAX_BYTES)
        .map_err(|_| AppError::new("APP_MAINTENANCE_MARKER_INVALID", "维护 marker 无法验证"))
}

fn marker_is_pending(path: &Path) -> AppResult<bool> {
    match marker_bytes(path)? {
        None => Ok(false),
        Some(bytes) if bytes == RESET_MARKER_CONTENT => Ok(true),
        Some(_) => Err(AppError::new(
            "APP_MAINTENANCE_MARKER_INVALID",
            "维护 marker 无法验证",
        )),
    }
}

fn reset_marker_state(data_dir: &Path) -> AppResult<ResetMarkerState> {
    if !validate_existing_maintenance_dir(data_dir)? {
        return Ok(ResetMarkerState::None);
    }

    let pending = marker_path_for_data_dir(data_dir);
    if marker_is_pending(&pending)? {
        return Ok(ResetMarkerState::Pending);
    }

    let completed = completed_marker_path_for_data_dir(data_dir);
    if marker_is_pending(&completed)? {
        return Ok(ResetMarkerState::Completed);
    }
    Ok(ResetMarkerState::None)
}

fn remove_completed_marker_if_present(data_dir: &Path) -> AppResult<()> {
    let completed = completed_marker_path_for_data_dir(data_dir);
    if !marker_is_pending(&completed)? {
        return Ok(());
    }
    match std::fs::remove_file(&completed) {
        Ok(()) => sync_parent_directory(&completed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::new(
            "APP_MAINTENANCE_MARKER_FAILED",
            "无法清理数据重置完成标记",
        )),
    }
}

/// Persist reset intent exactly once. A repeated request validates the existing
/// marker and is treated as an idempotent success.
pub(crate) fn write_reset_marker_at(data_dir: &Path) -> AppResult<bool> {
    let maintenance_dir = ensure_maintenance_dir(data_dir)?;
    let marker = marker_path_for_data_dir(data_dir);

    remove_completed_marker_if_present(data_dir)?;

    match write_file_atomic_create_new(&marker, RESET_MARKER_CONTENT) {
        Ok(()) => {
            sync_directory_for_reset(&maintenance_dir)?;
            Ok(true)
        }
        Err(error) if error.code() == "FS_ALREADY_EXISTS" => {
            if marker_is_pending(&marker)? {
                sync_directory_for_reset(&maintenance_dir)?;
                Ok(false)
            } else {
                Err(AppError::new(
                    "APP_MAINTENANCE_MARKER_INVALID",
                    "维护 marker 无法验证",
                ))
            }
        }
        Err(_) => Err(AppError::new(
            "APP_MAINTENANCE_MARKER_FAILED",
            "无法登记数据重置",
        )),
    }
}

fn remove_reset_marker_at(data_dir: &Path) -> AppResult<()> {
    let marker = marker_path_for_data_dir(data_dir);
    let completed = completed_marker_path_for_data_dir(data_dir);
    remove_completed_marker_if_present(data_dir)?;

    match rename_file_no_replace(&marker, &completed) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(AppError::new(
                "APP_MAINTENANCE_MARKER_FAILED",
                "数据已清理，但维护 marker 无法完成",
            ));
        }
    }

    // The durable rename guarantees that a crash exposes either Pending or
    // Completed. Clearing the tombstone is part of successful maintenance and
    // therefore remains fail-closed.
    sync_parent_directory(&completed)?;
    remove_completed_marker_if_present(data_dir)
}

pub(crate) fn consume_reset_marker_at(data_dir: &Path, db_path: &Path) -> AppResult<bool> {
    match reset_marker_state(data_dir)? {
        ResetMarkerState::None => return Ok(false),
        ResetMarkerState::Completed => {
            remove_completed_marker_if_present(data_dir)?;
            return Ok(true);
        }
        ResetMarkerState::Pending => {}
    }

    crate::infra::data_management::app_data_reset_at(data_dir, db_path)?;
    // Target unlinks must be durable before marker removal becomes durable.
    // Otherwise a power loss could restore old data without a reset marker.
    sync_reset_target_directories(data_dir, db_path)?;
    remove_reset_marker_at(data_dir)?;
    Ok(true)
}

fn maintenance_failure_message(error: &AppError) -> String {
    // Keep filesystem paths and OS error strings out of startup IPC/UI.
    format!("数据重置未完成（{}），只能重试或退出", error.code())
}

fn begin_maintenance<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<MaintenanceState>() {
        state.set_phase(MAINTENANCE_RUNNING);
    }
    crate::app::startup_state::begin_maintenance_run(app);
}

fn fail_maintenance<R: tauri::Runtime>(app: &tauri::AppHandle<R>, error: AppError) {
    if let Some(state) = app.try_state::<MaintenanceState>() {
        state.set_phase(MAINTENANCE_FAILED);
    }
    crate::app::startup_state::fail_maintenance_run(app, maintenance_failure_message(&error));
}

fn finish_maintenance<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(state) = app.try_state::<MaintenanceState>() {
        state.reset_exit_requested.store(false, Ordering::Release);
        state
            .reset_registration_required
            .store(false, Ordering::Release);
        state.set_phase(MAINTENANCE_CLEAN);
    }
    crate::app::startup_state::finish_maintenance_run(app);
}

/// Consume pending reset work synchronously before logging, DB initialization,
/// gateway startup, or normal background owners exist.
pub(crate) fn run_before_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<MaintenanceState>() else {
        return true;
    };
    let _lock = state.lock_coordinator();

    let data_dir = match crate::app_paths::app_data_dir(app) {
        Ok(path) => path,
        Err(error) => {
            fail_maintenance(app, error);
            return false;
        }
    };
    let marker_state = match reset_marker_state(&data_dir) {
        Ok(value) => value,
        Err(error) => {
            fail_maintenance(app, error);
            return false;
        }
    };
    if marker_state == ResetMarkerState::None {
        if state.phase() == MAINTENANCE_FAILED {
            // Tombstone removal can succeed before its parent-directory sync
            // fails. Retry that sync before treating marker absence as clean.
            match validate_existing_maintenance_dir(&data_dir) {
                Ok(true) => {
                    if let Err(error) =
                        sync_directory_for_reset(&maintenance_dir_for_data_dir(&data_dir))
                    {
                        fail_maintenance(app, error);
                        return false;
                    }
                }
                Ok(false) => {
                    fail_maintenance(
                        app,
                        AppError::new("APP_MAINTENANCE_PATH_INVALID", "应用数据维护目录无法验证"),
                    );
                    return false;
                }
                Err(error) => {
                    fail_maintenance(app, error);
                    return false;
                }
            }
        }
        finish_maintenance(app);
        return true;
    }

    begin_maintenance(app);
    let db_path = match crate::db::db_path(app) {
        Ok(path) => path,
        Err(error) => {
            fail_maintenance(app, error);
            return false;
        }
    };
    match consume_reset_marker_at(&data_dir, &db_path) {
        Ok(_) => {
            finish_maintenance(app);
            true
        }
        Err(error) => {
            fail_maintenance(app, error);
            false
        }
    }
}

pub(crate) async fn retry_pending_reset<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> bool {
    let retry_registration = {
        let Some(state) = app.try_state::<MaintenanceState>() else {
            return false;
        };
        if !state.can_retry() {
            return false;
        }
        state.reset_registration_required.load(Ordering::Acquire)
    };

    if retry_registration {
        let app_for_work = app.clone();
        return match crate::blocking::run("maintenance_reset_register_retry", move || {
            request_reset_and_exit(app_for_work)
        })
        .await
        {
            Ok(result) => result,
            Err(error) => {
                fail_maintenance(&app, error);
                false
            }
        };
    }

    let app_for_work = app.clone();
    match crate::blocking::run("maintenance_reset_retry", move || {
        Ok::<_, AppError>(run_before_startup(&app_for_work))
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            fail_maintenance(&app, error);
            false
        }
    }
}

pub(crate) fn request_reset_and_exit<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> AppResult<bool> {
    let Some(state) = app.try_state::<MaintenanceState>() else {
        return Err(AppError::new(
            "APP_MAINTENANCE_UNAVAILABLE",
            "应用维护状态不可用",
        ));
    };
    let _lock = state.lock_coordinator();
    state.begin_reset_registration();
    crate::app::startup_state::begin_maintenance_run(&app);

    let data_dir = match crate::app_paths::app_data_dir(&app) {
        Ok(path) => path,
        Err(error) => {
            fail_maintenance(&app, error.clone());
            if let Some(resident) = app.try_state::<crate::app::resident::ResidentState>() {
                resident.begin_exit();
            }
            return Err(error);
        }
    };
    if let Err(error) = write_reset_marker_at(&data_dir) {
        fail_maintenance(&app, error.clone());
        if let Some(resident) = app.try_state::<crate::app::resident::ResidentState>() {
            resident.begin_exit();
        }
        return Err(error);
    }
    state.request_reset_exit();
    drop(_lock);

    if let Some(state) = app.try_state::<crate::app::resident::ResidentState>() {
        state.begin_exit();
    }
    // Do not return to the old process after the durable marker exists. Normal
    // cleanup may reopen SQLite, while detached owners cannot be atomically
    // drained; the next process exclusively owns destructive deletion.
    std::process::exit(0)
}

pub(crate) fn should_skip_exit_cleanup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.try_state::<MaintenanceState>()
        .is_some_and(|state| state.should_skip_exit_cleanup())
}

pub(crate) fn start_normal_runtime_once<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<MaintenanceState>() else {
        return true;
    };
    state.try_mark_runtime_started()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvRestore {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn mock_app_with_maintenance_state() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        assert!(app.manage(MaintenanceState::default()));
        assert!(app.manage(crate::app::startup_state::StartupState::default()));
        assert!(app.manage(crate::app::heartbeat_watchdog::HeartbeatWatchdogState::default()));
        app
    }

    #[test]
    fn marker_write_is_idempotent_and_validates_existing_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(write_reset_marker_at(dir.path()).expect("write marker"));
        assert!(!write_reset_marker_at(dir.path()).expect("repeat marker"));
        assert_eq!(
            std::fs::read(marker_path_for_data_dir(dir.path())).expect("read marker"),
            RESET_MARKER_CONTENT
        );
    }

    #[test]
    fn malformed_marker_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = marker_path_for_data_dir(dir.path());
        std::fs::create_dir_all(marker.parent().expect("marker parent")).expect("mkdir");
        std::fs::write(&marker, b"not-a-reset").expect("write malformed marker");
        let error = write_reset_marker_at(dir.path()).expect_err("malformed marker must fail");
        assert_eq!(error.code(), "APP_MAINTENANCE_MARKER_INVALID");
        assert!(marker.exists());
    }

    #[test]
    fn non_directory_maintenance_path_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(maintenance_dir_for_data_dir(dir.path()), b"not a directory")
            .expect("write blocking path");

        let error = write_reset_marker_at(dir.path()).expect_err("unsafe path must fail");
        assert_eq!(error.code(), "APP_MAINTENANCE_PATH_INVALID");
    }

    #[test]
    fn marker_survives_failed_consumption_and_can_be_retried() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_reset_marker_at(dir.path()).expect("write marker");
        let db_path = dir.path().join("blocked.db");
        std::fs::create_dir_all(&db_path).expect("create blocking directory");
        let error = consume_reset_marker_at(dir.path(), &db_path).expect_err("reset fails");
        assert_eq!(error.code(), "APP_DATA_RESET_INCOMPLETE");
        assert!(marker_path_for_data_dir(dir.path()).exists());

        std::fs::remove_dir(&db_path).expect("remove blocking directory");
        std::fs::write(dir.path().join("settings.json"), b"stale settings")
            .expect("write reset target");
        assert!(consume_reset_marker_at(dir.path(), &db_path).expect("retry reset"));
        assert!(!marker_path_for_data_dir(dir.path()).exists());
        assert!(!dir.path().join("settings.json").exists());
    }

    #[test]
    fn marker_clear_failure_stays_pending_until_retry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("app.db");
        std::fs::write(&db_path, b"db").expect("write db");
        write_reset_marker_at(dir.path()).expect("write marker");
        let completed = completed_marker_path_for_data_dir(dir.path());
        std::fs::create_dir(&completed).expect("block completed marker");

        let error = consume_reset_marker_at(dir.path(), &db_path)
            .expect_err("marker clear failure must block startup");
        assert_eq!(error.code(), "APP_MAINTENANCE_MARKER_INVALID");
        assert!(marker_path_for_data_dir(dir.path()).exists());
        assert!(
            !db_path.exists(),
            "data deletion remains idempotently complete"
        );

        std::fs::remove_dir(&completed).expect("unblock completed marker");
        assert!(consume_reset_marker_at(dir.path(), &db_path).expect("retry clear"));
        assert!(!marker_path_for_data_dir(dir.path()).exists());
        assert!(!completed.exists());
    }

    #[test]
    fn reset_rejects_database_path_outside_app_data_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let db_path = outside.path().join("outside.db");
        std::fs::write(&db_path, b"outside").expect("write outside db");
        write_reset_marker_at(dir.path()).expect("write marker");

        let error = consume_reset_marker_at(dir.path(), &db_path)
            .expect_err("outside database path must be rejected");
        assert_eq!(error.code(), "APP_DATA_RESET_PATH_INVALID");
        assert!(db_path.exists());
        assert!(marker_path_for_data_dir(dir.path()).exists());
    }

    #[test]
    fn maintenance_invoke_gate_only_allows_status_retry_and_exit() {
        let state = MaintenanceState::default();
        assert!(state.allows_invoke("provider_upsert"));

        state.set_phase(MAINTENANCE_FAILED);
        assert!(state.allows_invoke("app_startup_status_get"));
        assert!(state.allows_invoke("app_startup_retry"));
        assert!(state.allows_invoke("app_exit"));
        assert!(!state.allows_invoke("cli_proxy_sync_enabled"));
        assert!(!state.allows_invoke("provider_upsert"));

        state.request_reset_exit();
        assert!(!state.allows_invoke("app_startup_retry"));
        assert!(state.should_skip_exit_cleanup());
    }

    #[test]
    fn registration_failure_remains_retryable_and_blocks_normal_operation() {
        let state = MaintenanceState::default();
        state.begin_reset_registration();
        state.set_phase(MAINTENANCE_FAILED);

        assert!(state.blocks_normal_operation());
        assert!(state.can_retry());
        assert!(state.reset_registration_required.load(Ordering::Acquire));
    }

    #[test]
    fn startup_with_invalid_marker_stays_in_maintenance() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home tempdir");
        let _env = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let app = mock_app_with_maintenance_state();
        let handle = app.handle().clone();
        let data_dir = crate::app_paths::app_data_dir(&handle).expect("app data dir");
        let marker = marker_path_for_data_dir(&data_dir);
        std::fs::create_dir_all(marker.parent().expect("marker parent")).expect("mkdir");
        std::fs::write(&marker, b"corrupt").expect("write corrupt marker");

        assert!(!run_before_startup(&handle));
        assert!(handle.state::<MaintenanceState>().blocks_normal_operation());
        assert!(!start_normal_runtime_once(&handle));
        let status = crate::app::startup_state::startup_status_snapshot(&handle);
        assert!(status.maintenance_mode);
        assert_eq!(
            status.current_stage,
            crate::app::startup_state::AppStartupStage::Failed
        );
        assert_eq!(
            status.failed_stage,
            Some(crate::app::startup_state::AppStartupStage::ResettingData)
        );
        assert!(status.can_retry);
    }

    #[test]
    fn startup_consumes_pending_reset_before_runtime_starts_once() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home tempdir");
        let _env = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let app = mock_app_with_maintenance_state();
        let handle = app.handle().clone();
        let data_dir = crate::app_paths::app_data_dir(&handle).expect("app data dir");
        let db_path = crate::db::db_path(&handle).expect("db path");
        std::fs::write(data_dir.join("settings.json"), b"stale").expect("write settings");
        std::fs::write(&db_path, b"stale db").expect("write db");
        write_reset_marker_at(&data_dir).expect("write marker");

        assert!(run_before_startup(&handle));
        assert!(!data_dir.join("settings.json").exists());
        assert!(!db_path.exists());
        assert!(!marker_path_for_data_dir(&data_dir).exists());
        assert!(!handle.state::<MaintenanceState>().blocks_normal_operation());
        assert!(start_normal_runtime_once(&handle));
        assert!(!start_normal_runtime_once(&handle));
    }
}
