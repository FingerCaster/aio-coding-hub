mod support;

use serde_json::Value;
use std::path::Path;
use support::{json_bool, json_str};

fn read_text(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("read text")
}

fn write_original_codex_config(handle: &tauri::AppHandle<tauri::test::MockRuntime>) {
    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");
}

fn corrupt_codex_config_backup_rel(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    backup_rel: &str,
) -> std::path::PathBuf {
    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let config_entry = manifest
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .and_then(|files| {
            files.iter_mut().find(|entry| {
                entry.get("kind").and_then(Value::as_str) == Some("codex_config_toml")
            })
        })
        .expect("codex config manifest entry");
    config_entry["backup_rel"] = Value::String(backup_rel.to_string());
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write unsafe manifest");
    manifest_path
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
fn cli_proxy_startup_repair_fixes_incomplete_enable_manifest() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";

    // Normal enable path writes live CLI config and marks manifest.enabled=true.
    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    // Simulate a crash window where live config has been applied but manifest.enabled remained false.
    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");

    let mut manifest: Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("read manifest before corruption"),
    )
    .expect("parse manifest before corruption");
    manifest["enabled"] = Value::Bool(false);
    let bytes = serde_json::to_vec_pretty(&manifest).expect("serialize manifest corruption");
    std::fs::write(&manifest_path, bytes).expect("write corrupted manifest");

    let repaired: Value =
        aio_coding_hub_lib::test_support::cli_proxy_startup_repair_incomplete_enable_json(&handle)
            .expect("run startup repair");

    let repaired_list = repaired.as_array().cloned().unwrap_or_default();
    assert!(
        repaired_list.iter().any(|item| {
            json_str(item, "cli_key") == "codex"
                && json_bool(item, "ok")
                && json_bool(item, "enabled")
        }),
        "expected codex to be repaired"
    );

    let manifest_after: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest after repair"))
            .expect("parse manifest after repair");
    assert!(json_bool(&manifest_after, "enabled"));
}

#[test]
fn codex_config_updates_are_preserved_when_cli_proxy_enabled() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";

    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    let _ = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({
            "features_fast_mode": true,
            "model_provider": "OpenAI"
        }),
    )
    .expect("set codex features");

    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    let before_restore = read_text(&config_path);
    assert!(
        before_restore.contains("fast_mode = true"),
        "{before_restore}"
    );
    assert!(
        before_restore.contains("model_provider = \"OpenAI\""),
        "{before_restore}"
    );

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    assert!(json_bool(&manifest, "enabled"));

    // Simulate app exit cleanup path: restore direct config while keeping enabled state.
    let restored =
        aio_coding_hub_lib::test_support::cli_proxy_restore_enabled_keep_state_json(&handle)
            .expect("restore enabled keep state");
    let restored_list = restored.as_array().cloned().unwrap_or_default();
    assert!(
        restored_list.iter().any(|item| {
            json_str(item, "cli_key") == "codex"
                && json_bool(item, "ok")
                && json_bool(item, "enabled")
        }),
        "expected codex restore success"
    );

    let after_restore = read_text(&config_path);
    assert!(
        after_restore.contains("fast_mode = true"),
        "{after_restore}"
    );
    assert!(
        after_restore.contains("model_provider = \"OpenAI\""),
        "{after_restore}"
    );
}

#[test]
fn codex_config_update_fails_when_cli_proxy_backup_cannot_refresh() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";

    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let manifest_before = manifest.clone();
    let backup_rel = manifest
        .get("files")
        .and_then(Value::as_array)
        .and_then(|files| {
            files.iter().find_map(|entry| {
                (entry.get("kind").and_then(Value::as_str) == Some("codex_config_toml"))
                    .then(|| entry.get("backup_rel").and_then(Value::as_str))
                    .flatten()
            })
        })
        .expect("codex config backup rel");
    let backup_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("files")
        .join(backup_rel);
    std::fs::remove_file(&backup_path).expect("remove existing config backup");
    std::fs::create_dir(&backup_path).expect("replace config backup with dir");

    let err = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({
            "model_provider": "OpenAI"
        }),
    )
    .expect_err("backup refresh failure should fail config update");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_CONFIG_BACKUP_REFRESH_FAILED"),
        "unexpected error: {err_text}"
    );

    let manifest_after: Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("read manifest after failure"),
    )
    .expect("parse manifest after failure");
    assert_eq!(manifest_after, manifest_before);
}

#[test]
fn codex_config_update_rejects_unsafe_cli_proxy_backup_rel() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";
    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let config_entry = manifest
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .and_then(|files| {
            files.iter_mut().find(|entry| {
                entry.get("kind").and_then(Value::as_str) == Some("codex_config_toml")
            })
        })
        .expect("codex config manifest entry");
    config_entry["backup_rel"] = Value::String("../escape.toml".to_string());
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write unsafe manifest");

    let err = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({
            "model_provider": "OpenAI"
        }),
    )
    .expect_err("unsafe backup_rel should fail");
    let err_text = err.to_string();
    assert!(
        err_text.contains("SEC_INVALID_INPUT") && err_text.contains("backup_rel"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn codex_config_update_rejects_symlinked_cli_proxy_backup_parent() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";
    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let proxy_root = app_data_dir.join("cli-proxy").join("codex");
    let files_dir = proxy_root.join("files");
    let outside = app.home_dir().join("outside-cli-proxy-backup");
    std::fs::create_dir_all(&outside).expect("create outside backup dir");
    let link_path = files_dir.join("link");
    if let Err(err) = symlink_dir(&outside, &link_path) {
        eprintln!(
            "skipping symlinked CLI proxy backup parent test: symlink creation unavailable: {err}"
        );
        return;
    }

    let manifest_path = proxy_root.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let config_entry = manifest
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .and_then(|files| {
            files.iter_mut().find(|entry| {
                entry.get("kind").and_then(Value::as_str) == Some("codex_config_toml")
            })
        })
        .expect("codex config manifest entry");
    config_entry["backup_rel"] = Value::String("link/config.toml".to_string());
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write unsafe manifest");

    let err = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({
            "model_provider": "OpenAI"
        }),
    )
    .expect_err("symlinked backup parent should fail");
    let err_text = err.to_string();
    assert!(
        err_text.contains("SEC_INVALID_INPUT") && err_text.contains("symlink"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn cli_proxy_disable_rejects_unsafe_backup_rel() {
    let app = support::TestApp::new();
    let handle = app.handle();

    write_original_codex_config(&handle);
    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        "http://127.0.0.1:37123",
    )
    .expect("enable codex cli proxy");
    corrupt_codex_config_backup_rel(&handle, "../escape.toml");

    let result = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        false,
        "http://127.0.0.1:37123",
    )
    .expect("disable returns failure result");
    assert!(!json_bool(&result, "ok"), "{result}");
    assert_eq!(json_str(&result, "error_code"), "CLI_PROXY_DISABLE_FAILED");
    assert!(
        json_str(&result, "message").contains("backup_rel"),
        "{result}"
    );
}

#[test]
fn cli_proxy_restore_enabled_rejects_symlinked_backup_parent() {
    let app = support::TestApp::new();
    let handle = app.handle();

    write_original_codex_config(&handle);
    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        "http://127.0.0.1:37123",
    )
    .expect("enable codex cli proxy");

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let files_dir = app_data_dir.join("cli-proxy").join("codex").join("files");
    let outside = app.home_dir().join("outside-cli-proxy-restore");
    std::fs::create_dir_all(&outside).expect("create outside backup dir");
    let link_path = files_dir.join("link");
    if let Err(err) = symlink_dir(&outside, &link_path) {
        eprintln!("skipping symlinked CLI proxy restore test: symlink creation unavailable: {err}");
        return;
    }

    corrupt_codex_config_backup_rel(&handle, "link/config.toml");

    let restored =
        aio_coding_hub_lib::test_support::cli_proxy_restore_enabled_keep_state_json(&handle)
            .expect("restore enabled keep state returns per-cli results");
    let restored_list = restored.as_array().cloned().unwrap_or_default();
    let codex = restored_list
        .iter()
        .find(|item| json_str(item, "cli_key") == "codex")
        .expect("codex restore result");
    assert!(!json_bool(codex, "ok"), "{codex}");
    assert_eq!(json_str(codex, "error_code"), "CLI_PROXY_RESTORE_FAILED");
    assert!(json_str(codex, "message").contains("symlink"), "{codex}");
}

#[test]
fn codex_config_update_restores_manifest_when_backup_snapshot_fails() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";
    let config_path =
        aio_coding_hub_lib::test_support::codex_config_toml_path(&handle).expect("codex path");
    std::fs::create_dir_all(config_path.parent().expect("codex config parent"))
        .expect("create codex config parent");
    std::fs::write(
        &config_path,
        "model_provider = \"aio\"\n\n[model_providers.aio]\nname = \"aio\"\n",
    )
    .expect("write original codex config");

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "codex",
        true,
        base_origin,
    )
    .expect("enable codex cli proxy");

    let app_data_dir = aio_coding_hub_lib::test_support::app_data_dir(&handle).expect("app_data");
    let manifest_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    let config_entry = manifest
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .and_then(|files| {
            files.iter_mut().find(|entry| {
                entry.get("kind").and_then(Value::as_str) == Some("codex_config_toml")
            })
        })
        .expect("codex config manifest entry");
    config_entry["backup_rel"] = Value::Null;
    config_entry["existed"] = Value::Bool(false);
    let manifest_before = serde_json::to_vec_pretty(&manifest).expect("serialize manifest");
    std::fs::write(&manifest_path, &manifest_before).expect("write stale manifest");

    let backup_path = app_data_dir
        .join("cli-proxy")
        .join("codex")
        .join("files")
        .join("config.toml");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path).expect("remove existing backup");
    }
    std::fs::create_dir(&backup_path).expect("replace future backup path with dir");

    let err = aio_coding_hub_lib::test_support::cli_manager_codex_config_set_json(
        &handle,
        serde_json::json!({
            "model_provider": "OpenAI"
        }),
    )
    .expect_err("backup snapshot failure should fail config update");
    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_CONFIG_BACKUP_REFRESH_FAILED"),
        "unexpected error: {err_text}"
    );
    assert_eq!(
        std::fs::read(&manifest_path).expect("read manifest after failure"),
        manifest_before
    );
}

#[test]
fn claude_settings_updates_are_preserved_when_cli_proxy_enabled() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let base_origin = "http://127.0.0.1:37123";

    let _ = aio_coding_hub_lib::test_support::cli_proxy_set_enabled_json(
        &handle,
        "claude",
        true,
        base_origin,
    )
    .expect("enable claude cli proxy");

    let _ = aio_coding_hub_lib::test_support::cli_manager_claude_settings_set_json(
        &handle,
        serde_json::json!({
            "always_thinking_enabled": true
        }),
    )
    .expect("set claude settings");

    let settings_path = app.home_dir().join(".claude").join("settings.json");
    let before_restore = read_text(&settings_path);
    assert!(
        before_restore.contains("\"alwaysThinkingEnabled\": true"),
        "{before_restore}"
    );

    let restored =
        aio_coding_hub_lib::test_support::cli_proxy_restore_enabled_keep_state_json(&handle)
            .expect("restore enabled keep state");
    let restored_list = restored.as_array().cloned().unwrap_or_default();
    assert!(
        restored_list.iter().any(|item| {
            json_str(item, "cli_key") == "claude"
                && json_bool(item, "ok")
                && json_bool(item, "enabled")
        }),
        "expected claude restore success"
    );

    let after_restore = read_text(&settings_path);
    assert!(
        after_restore.contains("\"alwaysThinkingEnabled\": true"),
        "{after_restore}"
    );
}
