mod support;

use rusqlite::{params, Connection};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn write_codex_direct_files(app: &support::TestApp, config: &str, auth: &str) {
    let codex_dir = app.home_dir().join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(codex_dir.join("config.toml"), config).expect("write config");
    std::fs::write(codex_dir.join("auth.json"), auth).expect("write auth");
}

fn codex_home(app: &support::TestApp) -> PathBuf {
    app.home_dir().join(".codex")
}

fn write_rollout(path: &Path, provider: &str, thread_id: &str) {
    fs::create_dir_all(path.parent().expect("rollout parent")).expect("create rollout parent");
    let session_meta = serde_json::json!({
        "type": "session_meta",
        "payload": {
            "id": thread_id,
            "model_provider": provider,
            "cwd": "C:/workspace/demo"
        }
    });
    let event = serde_json::json!({
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

fn write_global_state(path: &Path, provider: &str) {
    let state = serde_json::json!({
        "electron-saved-workspace-roots": ["C:/workspace/demo"],
        "project-order": ["C:/workspace/demo"],
        "active-workspace-roots": ["C:/workspace/demo"],
        "model_provider": provider
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

#[test]
fn codex_route_service_roundtrip_plan_apply_verify_and_restore() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";

    write_codex_direct_files(
        &app,
        r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = aio_coding_hub_lib::test_support::cli_proxy_codex_plan_external_enable_json(
        &handle,
        aio_origin,
        guarded_origin,
    )
    .expect("plan external enable");
    assert_eq!(support::json_str(&plan, "current_route_mode"), "unproxied");
    assert!(support::json_bool(&plan, "cli_proxy_enable_required"));
    assert!(plan
        .get("provider_sync")
        .and_then(|value| value.get("change_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false));

    let guarded = aio_coding_hub_lib::test_support::cli_proxy_codex_apply_guarded_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": support::json_u64(&plan, "generation"),
            "expectedCanonicalSha256": support::json_str(&plan, "canonical_config_sha256"),
            "aioOrigin": aio_origin,
            "guardedOrigin": guarded_origin,
            "desiredEnabled": true,
            "sourceCommit": "0123456789abcdef0123456789abcdef01234567",
            "processShouldRun": true
        }),
    )
    .expect("apply guarded route");
    assert_eq!(
        guarded
            .get("route")
            .and_then(|value| value.get("route_mode"))
            .and_then(Value::as_str),
        Some("guarded")
    );

    let verify_guarded =
        aio_coding_hub_lib::test_support::cli_proxy_codex_verify_route_json(&handle)
            .expect("verify guarded route");
    assert_eq!(
        verify_guarded
            .get("effective_origin")
            .and_then(Value::as_str),
        Some(guarded_origin)
    );

    let direct = aio_coding_hub_lib::test_support::cli_proxy_codex_apply_direct_aio_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": guarded
                .get("route")
                .and_then(|value| value.get("generation"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            "expectedCanonicalSha256": guarded
                .get("route")
                .and_then(|value| value.get("canonical_config_sha256"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "aioOrigin": aio_origin,
            "desiredEnabled": true,
            "sourceCommit": null,
            "processShouldRun": false
        }),
    )
    .expect("apply direct aio route");
    assert_eq!(
        direct
            .get("route")
            .and_then(|value| value.get("effective_origin"))
            .and_then(Value::as_str),
        Some(aio_origin)
    );

    let restore = aio_coding_hub_lib::test_support::cli_proxy_codex_restore_unproxied_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": direct
                .get("route")
                .and_then(|value| value.get("generation"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            "expectedCanonicalSha256": direct
                .get("route")
                .and_then(|value| value.get("canonical_config_sha256"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "aioOrigin": aio_origin,
            "desiredEnabled": false,
            "keepCliProxyEnabled": false,
            "sourceCommit": null,
            "processShouldRun": false
        }),
    )
    .expect("restore unproxied route");
    assert_eq!(
        restore
            .get("route")
            .and_then(|value| value.get("route_mode"))
            .and_then(Value::as_str),
        Some("unproxied")
    );
    assert_eq!(
        restore
            .get("route")
            .and_then(|value| value.get("cli_proxy_enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn codex_route_service_restores_canonical_provider_sync_state() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    let original_config = r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#;
    write_codex_direct_files(
        &app,
        original_config,
        r#"{
  "profile": "local"
}"#,
    );

    let home = codex_home(&app);
    fs::create_dir_all(&home).expect("create codex home");
    let rollout_path = home.join("sessions/2026/rollout-route-provider-sync.jsonl");
    write_rollout(&rollout_path, "Anthropic", "thread-1");
    let sqlite_path = home.join("sqlite/codex-dev.db");
    create_threads_db(
        &sqlite_path,
        &[
            ("thread-1", Some("Anthropic"), Some(0)),
            ("thread-2", None, Some(0)),
        ],
    );
    let global_state_path = home.join(".codex-global-state.json");
    write_global_state(&global_state_path, "Anthropic");

    let plan = aio_coding_hub_lib::test_support::cli_proxy_codex_plan_external_enable_json(
        &handle,
        aio_origin,
        guarded_origin,
    )
    .expect("plan external enable");
    let plan_provider_sync = plan.get("provider_sync").expect("provider sync plan");
    assert_eq!(
        plan_provider_sync
            .get("current_provider")
            .and_then(Value::as_str),
        Some("Anthropic")
    );
    assert_eq!(
        plan_provider_sync
            .get("target_provider")
            .and_then(Value::as_str),
        Some("aio")
    );
    assert_eq!(
        plan_provider_sync
            .get("codex_must_be_closed")
            .and_then(Value::as_bool),
        Some(true)
    );

    let guarded = aio_coding_hub_lib::test_support::cli_proxy_codex_apply_guarded_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": support::json_u64(&plan, "generation"),
            "expectedCanonicalSha256": support::json_str(&plan, "canonical_config_sha256"),
            "aioOrigin": aio_origin,
            "guardedOrigin": guarded_origin,
            "desiredEnabled": true,
            "sourceCommit": "0123456789abcdef0123456789abcdef01234567",
            "processShouldRun": true
        }),
    )
    .expect("apply guarded route");
    assert_eq!(
        guarded
            .get("provider_sync")
            .and_then(|value| value.get("target_provider"))
            .and_then(Value::as_str),
        Some("aio")
    );
    assert_eq!(
        rollout_session_meta_providers(&rollout_path),
        vec!["aio".to_string()]
    );
    assert_eq!(
        read_threads_db(&sqlite_path),
        vec![
            ("thread-1".to_string(), Some("aio".to_string()), Some(1)),
            ("thread-2".to_string(), Some("aio".to_string()), Some(1)),
        ]
    );
    assert_eq!(read_json(&global_state_path)["model_provider"], "aio");

    let restore = aio_coding_hub_lib::test_support::cli_proxy_codex_restore_unproxied_route_json(
        &handle,
        serde_json::json!({
            "expectedGeneration": guarded
                .get("route")
                .and_then(|value| value.get("generation"))
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            "expectedCanonicalSha256": guarded
                .get("route")
                .and_then(|value| value.get("canonical_config_sha256"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "aioOrigin": aio_origin,
            "desiredEnabled": false,
            "keepCliProxyEnabled": false,
            "sourceCommit": null,
            "processShouldRun": false
        }),
    )
    .expect("restore unproxied route");
    assert_eq!(
        restore
            .get("provider_sync")
            .and_then(|value| value.get("target_provider"))
            .and_then(Value::as_str),
        Some("Anthropic")
    );
    assert_eq!(
        rollout_session_meta_providers(&rollout_path),
        vec!["Anthropic".to_string()]
    );
    assert_eq!(
        read_threads_db(&sqlite_path),
        vec![
            (
                "thread-1".to_string(),
                Some("Anthropic".to_string()),
                Some(1)
            ),
            (
                "thread-2".to_string(),
                Some("Anthropic".to_string()),
                Some(1)
            ),
        ]
    );
    assert_eq!(read_json(&global_state_path)["model_provider"], "Anthropic");
    assert_eq!(
        fs::read_to_string(home.join("config.toml"))
            .expect("read restored config")
            .trim_end(),
        original_config.trim_end()
    );
}

#[test]
fn codex_route_reconcile_reports_no_pending_transition_after_commit() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let reconcile =
        aio_coding_hub_lib::test_support::cli_proxy_codex_reconcile_pending_route_json(&handle)
            .expect("reconcile route");
    assert!(reconcile
        .get("pending_transition_reconciled")
        .is_some_and(Value::is_null));
}
