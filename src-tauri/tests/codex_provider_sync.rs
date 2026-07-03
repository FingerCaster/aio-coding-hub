mod support;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const AIO_CONFIG: &str = r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37124/v1"
"#;

const OPENAI_CONFIG: &str = r#"model_provider = "OpenAI"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
"#;

fn codex_home(handle: &tauri::AppHandle<tauri::test::MockRuntime>) -> PathBuf {
    aio_coding_hub_lib::test_support::codex_home_dir_follow_env_or_default(handle)
        .expect("codex home dir")
}

fn write_codex_config(handle: &tauri::AppHandle<tauri::test::MockRuntime>, toml: &str) {
    let path = aio_coding_hub_lib::test_support::codex_config_toml_path(handle)
        .expect("codex config path");
    std::fs::create_dir_all(path.parent().expect("codex parent")).expect("create codex dir");
    std::fs::write(path, toml).expect("write codex config");
}

fn read_codex_config(handle: &tauri::AppHandle<tauri::test::MockRuntime>) -> String {
    let path = aio_coding_hub_lib::test_support::codex_config_toml_path(handle)
        .expect("codex config path");
    std::fs::read_to_string(path).expect("read codex config")
}

fn write_rollout(path: &Path, provider: &str, thread_id: &str) {
    fs::create_dir_all(path.parent().expect("rollout parent")).expect("create rollout parent");
    let session_meta = json!({
        "type": "session_meta",
        "payload": {
            "id": thread_id,
            "model_provider": provider,
            "cwd": "C:/workspace/demo"
        }
    });
    let event = json!({
        "type": "event_msg",
        "payload": {
            "kind": "user_message",
            "text": "hello"
        }
    });
    fs::write(path, format!("{session_meta}\n{event}\n")).expect("write rollout");
}

fn rollout_session_meta_providers(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("read rollout")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row.get("type").and_then(Value::as_str) == Some("session_meta"))
        .filter_map(|row| {
            row.get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn create_threads_db(path: &Path, rows: &[(&str, Option<&str>, Option<i64>)]) {
    fs::create_dir_all(path.parent().expect("db parent")).expect("create db parent");
    let conn = Connection::open(path).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, has_user_event INTEGER)",
        [],
    )
    .expect("create threads table");
    for (id, provider, has_user_event) in rows {
        conn.execute(
            "INSERT INTO threads(id, model_provider, has_user_event) VALUES (?1, ?2, ?3)",
            params![id, provider, has_user_event],
        )
        .expect("insert thread row");
    }
}

fn read_threads_db(path: &Path) -> Vec<(String, Option<String>, Option<i64>)> {
    let conn = Connection::open(path).expect("open sqlite");
    let mut stmt = conn
        .prepare("SELECT id, model_provider, has_user_event FROM threads ORDER BY id")
        .expect("prepare threads select");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .expect("query threads rows");
    rows.map(|row| row.expect("read thread row")).collect()
}

fn write_sqlite_sidecars(path: &Path) {
    fs::write(format!("{}-wal", path.to_string_lossy()), b"wal").expect("write db wal");
    fs::write(format!("{}-shm", path.to_string_lossy()), b"shm").expect("write db shm");
}

fn write_global_state(path: &Path, provider: &str) {
    let state = json!({
        "electron-saved-workspace-roots": ["C:/workspace/demo", "D:/workspace/alt"],
        "project-order": ["C:/workspace/demo", "D:/workspace/alt"],
        "active-workspace-roots": ["C:/workspace/demo"],
        "electron-workspace-root-labels": {
            "C:/workspace/demo": "Demo Workspace"
        },
        "open-in-target-preferences": {
            "perPath": {
                "C:/workspace/demo": "terminal"
            }
        },
        "model_provider": provider,
        "discard_me": {
            "should": "disappear"
        }
    });
    fs::write(
        path,
        serde_json::to_vec(&state).expect("serialize global state"),
    )
    .expect("write global state");
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json file")).expect("parse json file")
}

#[cfg(windows)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

#[cfg(not(windows))]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[test]
fn codex_config_raw_save_does_not_run_provider_sync() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(Some(
        true,
    ));
    let result = aio_coding_hub_lib::test_support::codex_config_toml_raw_set(
        &handle,
        "approval_policy = \"on-request\"\n".to_string(),
    );
    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(None);

    result.expect("raw config save should not run provider sync");
    let got = read_codex_config(&handle);
    assert_eq!(got, "approval_policy = \"on-request\"\n");
}

#[test]
fn codex_config_raw_save_accepts_managed_provider() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::codex_config_toml_raw_set(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37124/v1"
"#
        .to_string(),
    )
    .expect("save managed config");

    let got = read_codex_config(&handle);
    assert!(got.contains("model_provider = \"aio\""), "{got}");
}

#[test]
fn codex_config_set_refuses_when_codex_is_running() {
    let app = support::TestApp::new();
    let handle = app.handle();

    write_codex_config(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37124/v1"
"#,
    );

    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(Some(
        true,
    ));
    let result = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({ "features_remote_compaction": true }),
    );
    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(None);

    let err = result.expect_err("running codex should block sync");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn codex_config_set_non_provider_patch_does_not_require_codex_to_be_closed() {
    let app = support::TestApp::new();
    let handle = app.handle();

    write_codex_config(&handle, AIO_CONFIG);

    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(Some(
        true,
    ));
    let result = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({ "model_reasoning_effort": "high" }),
    );
    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(None);

    result.expect("non-provider config save should not run provider sync");
    let got = read_codex_config(&handle);
    assert!(got.contains("model_reasoning_effort = \"high\""), "{got}");
    assert!(got.contains("model_provider = \"aio\""), "{got}");
}

#[test]
fn codex_provider_sync_from_config_bytes_updates_rollout_sqlite_global_state_and_backup() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, AIO_CONFIG);

    let rollout_path = home.join("sessions/2026/rollout-provider-sync.jsonl");
    write_rollout(&rollout_path, "aio", "thread-1");

    let sqlite_path = home.join("sqlite/codex-dev.db");
    create_threads_db(
        &sqlite_path,
        &[
            ("thread-1", Some("aio"), Some(0)),
            ("thread-2", Some(""), Some(0)),
            ("thread-3", Some("OpenAI"), Some(1)),
        ],
    );
    write_sqlite_sidecars(&sqlite_path);

    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "aio");

    let result = aio_coding_hub_lib::test_support::codex_provider_sync_from_config_bytes_json(
        &handle,
        "config_save",
        OPENAI_CONFIG.as_bytes().to_vec(),
    )
    .expect("sync from config bytes");

    assert_eq!(support::json_str(&result, "status"), "synced");
    assert_eq!(support::json_str(&result, "target_provider"), "OpenAI");
    assert_eq!(support::json_str(&result, "trigger"), "config_save");
    assert_eq!(
        support::json_u64(&result, "sqlite_provider_rows_updated"),
        2
    );
    assert_eq!(
        support::json_u64(&result, "sqlite_user_event_rows_updated"),
        2
    );
    assert_eq!(support::json_u64(&result, "sqlite_cwd_rows_updated"), 0);

    let changed_session_files = result
        .get("changed_session_files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(changed_session_files.len(), 1, "{changed_session_files:?}");
    assert!(
        changed_session_files[0]
            .as_str()
            .is_some_and(|value| value.ends_with("rollout-provider-sync.jsonl")),
        "{changed_session_files:?}"
    );

    let updated_workspace_roots = result
        .get("updated_workspace_roots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.as_str().map(ToString::to_string))
        .collect::<Vec<_>>();
    assert_eq!(
        updated_workspace_roots,
        vec![
            "C:/workspace/demo".to_string(),
            "D:/workspace/alt".to_string()
        ]
    );

    let config_text = read_codex_config(&handle);
    assert!(
        config_text.contains("model_provider = \"OpenAI\""),
        "{config_text}"
    );
    assert_eq!(
        rollout_session_meta_providers(&rollout_path),
        vec!["OpenAI".to_string()]
    );

    let threads = read_threads_db(&sqlite_path);
    assert_eq!(
        threads,
        vec![
            ("thread-1".to_string(), Some("OpenAI".to_string()), Some(1)),
            ("thread-2".to_string(), Some("OpenAI".to_string()), Some(1)),
            ("thread-3".to_string(), Some("OpenAI".to_string()), Some(1)),
        ]
    );

    let global_state = read_json(&global_state_path);
    assert_eq!(global_state["model_provider"], "OpenAI");
    assert_eq!(
        global_state["electron-saved-workspace-roots"],
        json!(["C:/workspace/demo", "D:/workspace/alt"])
    );
    assert_eq!(
        global_state["open-in-target-preferences"],
        json!({"perPath": {"C:/workspace/demo": "terminal"}})
    );
    assert_eq!(
        global_state["discard_me"],
        json!({"should": "disappear"}),
        "unknown global state keys should be preserved: {global_state}"
    );

    let backup_dir = PathBuf::from(support::json_str(&result, "backup_dir"));
    assert!(
        backup_dir.is_dir(),
        "missing backup dir: {}",
        backup_dir.display()
    );
    assert!(backup_dir.join("config.toml").exists());
    assert!(backup_dir
        .join("sessions/2026/rollout-provider-sync.jsonl")
        .exists());
    assert!(backup_dir.join("sqlite/codex-dev.db").exists());
    assert!(backup_dir.join("sqlite/codex-dev.db-wal").exists());
    assert!(backup_dir.join("sqlite/codex-dev.db-shm").exists());
    assert!(backup_dir.join(".codex-global-state.json").exists());

    let manifest = read_json(&backup_dir.join("provider-sync.json"));
    assert_eq!(manifest["trigger"], "config_save");
    assert_eq!(manifest["target_provider"], "OpenAI");
    assert_eq!(manifest["managed_by"], "Codex provider sync");
    assert_eq!(manifest["version"], 1);
    assert_eq!(
        manifest["sqlite_files"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or_default(),
        3
    );
}

#[test]
fn codex_provider_sync_current_uses_same_rules_and_then_reports_up_to_date() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, OPENAI_CONFIG);

    let rollout_path = home.join("archived_sessions/2026/rollout-manual.jsonl");
    write_rollout(&rollout_path, "aio", "thread-9");

    let sqlite_path = home.join("state_5.sqlite");
    create_threads_db(
        &sqlite_path,
        &[
            ("thread-9", Some("aio"), Some(0)),
            ("thread-10", None, Some(0)),
        ],
    );

    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "aio");

    let first = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect("manual sync current");

    assert_eq!(support::json_str(&first, "status"), "synced");
    assert_eq!(support::json_str(&first, "target_provider"), "OpenAI");
    assert_eq!(support::json_u64(&first, "sqlite_provider_rows_updated"), 2);
    assert_eq!(
        support::json_u64(&first, "sqlite_user_event_rows_updated"),
        2
    );
    assert_eq!(
        rollout_session_meta_providers(&rollout_path),
        vec!["OpenAI".to_string()]
    );
    assert_eq!(
        read_threads_db(&sqlite_path),
        vec![
            ("thread-10".to_string(), Some("OpenAI".to_string()), Some(1)),
            ("thread-9".to_string(), Some("OpenAI".to_string()), Some(1)),
        ]
    );
    let global_state_after_first = read_json(&global_state_path);
    assert_eq!(global_state_after_first["model_provider"], "OpenAI");
    assert!(
        global_state_after_first.get("discard_me").is_some(),
        "unknown global state keys should be preserved: {global_state_after_first}"
    );

    let second = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect("second manual sync");

    assert_eq!(support::json_str(&second, "status"), "up_to_date");
    assert_eq!(support::json_str(&second, "target_provider"), "OpenAI");
    assert!(
        second.get("backup_dir").is_some_and(Value::is_null),
        "{second}"
    );
    assert_eq!(
        support::json_u64(&second, "sqlite_provider_rows_updated"),
        0
    );
    assert_eq!(
        support::json_u64(&second, "sqlite_user_event_rows_updated"),
        0
    );
    assert_eq!(
        second
            .get("changed_session_files")
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or_default(),
        0
    );
    assert_eq!(read_json(&global_state_path)["model_provider"], "OpenAI");
}

#[test]
fn codex_provider_sync_current_rejects_when_lock_exists() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);

    fs::create_dir_all(home.join("tmp/provider-sync.lock")).expect("create sync lock");

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect_err("lock should block sync");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_LOCKED"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn codex_provider_sync_current_rejects_invalid_config_without_mutating_history() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, "model_provider =\n");

    let rollout_path = home.join("sessions/rollout-invalid-config.jsonl");
    write_rollout(&rollout_path, "OpenAI", "thread-invalid-config");
    let original_rollout = fs::read_to_string(&rollout_path).expect("read original rollout");

    let sqlite_path = home.join("state_5.sqlite");
    create_threads_db(
        &sqlite_path,
        &[("thread-invalid-config", Some("OpenAI"), Some(0))],
    );
    let original_threads = read_threads_db(&sqlite_path);

    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "OpenAI");
    let original_global_state =
        fs::read_to_string(&global_state_path).expect("read original global state");

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect_err("invalid config should fail closed");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_INVALID_CONFIG"),
        "unexpected error: {err_text}"
    );
    assert_eq!(
        fs::read_to_string(&rollout_path).expect("read rollout after failure"),
        original_rollout
    );
    assert_eq!(read_threads_db(&sqlite_path), original_threads);
    assert_eq!(
        fs::read_to_string(&global_state_path).expect("read global state after failure"),
        original_global_state
    );
}

#[test]
fn codex_provider_sync_current_rejects_invalid_utf8_config_without_mutating_history() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    let config_path = aio_coding_hub_lib::test_support::codex_config_toml_path(&handle)
        .expect("codex config path");
    fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    fs::write(
        &config_path,
        b"model_provider = \"OpenAI\"\ninvalid_utf8 = \xFF\n",
    )
    .expect("write invalid utf8 config");

    let rollout_path = home.join("sessions/rollout-invalid-utf8-config.jsonl");
    write_rollout(&rollout_path, "OpenAI", "thread-invalid-utf8-config");
    let original_rollout = fs::read_to_string(&rollout_path).expect("read original rollout");

    let sqlite_path = home.join("state_invalid_utf8.sqlite");
    create_threads_db(
        &sqlite_path,
        &[("thread-invalid-utf8-config", Some("OpenAI"), Some(0))],
    );
    let original_threads = read_threads_db(&sqlite_path);

    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "OpenAI");
    let original_global_state =
        fs::read_to_string(&global_state_path).expect("read original global state");

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect_err("invalid utf8 config should fail closed");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_INVALID_CONFIG"),
        "unexpected error: {err_text}"
    );
    assert_eq!(
        fs::read_to_string(&rollout_path).expect("read rollout after failure"),
        original_rollout
    );
    assert_eq!(read_threads_db(&sqlite_path), original_threads);
    assert_eq!(
        fs::read_to_string(&global_state_path).expect("read global state after failure"),
        original_global_state
    );
}

#[test]
fn codex_provider_sync_rejects_symlinked_session_root() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, OPENAI_CONFIG);

    let outside = app.home_dir().join("outside-sessions");
    fs::create_dir_all(&outside).expect("create outside sessions dir");
    write_rollout(
        &outside.join("rollout-outside.jsonl"),
        "aio",
        "thread-outside",
    );

    if let Err(err) = symlink_dir(&outside, &home.join("sessions")) {
        eprintln!("skipping symlinked session root test: symlink creation unavailable: {err}");
        return;
    }

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle)
        .expect_err("symlinked session root should fail closed");
    let err_text = err.to_string();
    assert!(
        err_text.contains("SEC_INVALID_INPUT") && err_text.contains("symlink"),
        "unexpected error: {err_text}"
    );
    assert_eq!(
        rollout_session_meta_providers(&outside.join("rollout-outside.jsonl")),
        vec!["aio".to_string()]
    );
}

#[test]
fn codex_provider_sync_process_running_error_takes_precedence_over_lock() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);

    fs::create_dir_all(home.join("tmp/provider-sync.lock")).expect("create sync lock");

    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(Some(
        true,
    ));
    let result = aio_coding_hub_lib::test_support::codex_provider_sync_current_json(&handle);
    aio_coding_hub_lib::test_support::codex_provider_sync_set_running_override_for_tests(Some(
        false,
    ));

    let err = result.expect_err("running process should be reported before lock");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn codex_provider_sync_rejects_symlinked_backup_root() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, AIO_CONFIG);
    let rollout_path = home.join("sessions/rollout-backup-root.jsonl");
    write_rollout(&rollout_path, "aio", "thread-backup-root");

    let outside = app.home_dir().join("outside-backups-state");
    fs::create_dir_all(&outside).expect("create outside backup root");
    if let Err(err) = symlink_dir(&outside, &home.join("backups_state")) {
        eprintln!("skipping symlinked backup root test: symlink creation unavailable: {err}");
        return;
    }

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_from_config_bytes_json(
        &handle,
        "config_save",
        OPENAI_CONFIG.as_bytes().to_vec(),
    )
    .expect_err("symlinked backup root should fail closed");
    let err_text = err.to_string();
    assert!(
        err_text.contains("SEC_INVALID_INPUT") && err_text.contains("symlink"),
        "unexpected error: {err_text}"
    );
    assert_eq!(
        rollout_session_meta_providers(&rollout_path),
        vec!["aio".to_string()]
    );
}

#[test]
fn codex_provider_sync_from_config_bytes_rolls_back_when_sqlite_write_fails() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(&home).expect("create codex home");

    write_codex_config(&handle, AIO_CONFIG);

    let rollout_path = home.join("sessions/rollout-rollback.jsonl");
    write_rollout(&rollout_path, "aio", "thread-rollback");
    let original_rollout = fs::read_to_string(&rollout_path).expect("read original rollout");

    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "aio");
    let original_global_state =
        fs::read_to_string(&global_state_path).expect("read original state");

    let sqlite_path = home.join("state_5.sqlite");
    create_threads_db(&sqlite_path, &[("thread-rollback", Some("aio"), Some(0))]);
    let conn = Connection::open(&sqlite_path).expect("open sqlite for trigger");
    conn.execute(
        "CREATE TRIGGER fail_provider_sync_update BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .expect("create failing trigger");
    drop(conn);

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_from_config_bytes_json(
        &handle,
        "config_save",
        OPENAI_CONFIG.as_bytes().to_vec(),
    )
    .expect_err("sqlite failure should bubble up");
    let err_text = err.to_string();
    assert!(
        err_text.contains("failed to update sqlite provider rows"),
        "unexpected error: {err_text}"
    );

    assert_eq!(read_codex_config(&handle), AIO_CONFIG);
    assert_eq!(
        fs::read_to_string(&rollout_path).expect("read restored rollout"),
        original_rollout
    );
    assert_eq!(
        fs::read_to_string(&global_state_path).expect("read restored state"),
        original_global_state
    );
}

#[test]
fn codex_provider_sync_rolls_back_first_sqlite_when_later_sqlite_fails() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let home = codex_home(&handle);
    fs::create_dir_all(home.join("sqlite")).expect("create sqlite dir");

    write_codex_config(&handle, AIO_CONFIG);

    let first_db = home.join("sqlite/codex-dev.db");
    create_threads_db(&first_db, &[("thread-first", Some("aio"), Some(0))]);
    let first_before = read_threads_db(&first_db);

    let second_db = home.join("sqlite/z-failing.db");
    create_threads_db(&second_db, &[("thread-second", Some("aio"), Some(0))]);
    let conn = Connection::open(&second_db).expect("open failing sqlite");
    conn.execute(
        "CREATE TRIGGER fail_provider_sync_update BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .expect("create failing trigger");
    drop(conn);
    let second_before = read_threads_db(&second_db);

    let err = aio_coding_hub_lib::test_support::codex_provider_sync_from_config_bytes_json(
        &handle,
        "config_save",
        OPENAI_CONFIG.as_bytes().to_vec(),
    )
    .expect_err("later sqlite failure should bubble up");
    let err_text = err.to_string();
    assert!(
        err_text.contains("failed to update sqlite provider rows"),
        "unexpected error: {err_text}"
    );

    assert_eq!(read_threads_db(&first_db), first_before);
    assert_eq!(read_threads_db(&second_db), second_before);
    assert_eq!(read_codex_config(&handle), AIO_CONFIG);
}
