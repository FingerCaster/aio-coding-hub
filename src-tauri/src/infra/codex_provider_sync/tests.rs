use super::*;

#[test]
fn session_change_retains_only_the_rollout_path() {
    assert_eq!(
        std::mem::size_of::<SessionChange>(),
        std::mem::size_of::<PathBuf>()
    );
}

#[test]
fn rollout_rewrite_streams_file_and_preserves_non_session_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("rollout-streaming.jsonl");
    let untouched = "x".repeat(2 * 1024 * 1024);
    std::fs::write(
        &path,
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"model_provider\":\"aio\"}}}}\r\n{untouched}\n"
        ),
    )
    .expect("write rollout");

    assert!(rollout_needs_provider_rewrite(&path, "OpenAI").expect("discover change"));
    rewrite_rollout_session_meta_providers(&path, "OpenAI").expect("rewrite rollout");

    let rewritten = std::fs::read_to_string(&path).expect("read rollout");
    assert!(rewritten.starts_with(
        "{\"payload\":{\"model_provider\":\"OpenAI\"},\"type\":\"session_meta\"}\r\n"
    ));
    assert!(rewritten.ends_with(&format!("{untouched}\n")));
    assert!(!rollout_needs_provider_rewrite(&path, "OpenAI").expect("idempotent discover"));
}

#[test]
fn history_only_rollout_rewrites_every_non_target_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("rollout-all-non-target.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"aio\"}}\n",
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"third-party\"}}\n",
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"OpenAI\"}}\n"
        ),
    )
    .expect("write rollout");

    rewrite_rollout_session_meta_providers(&path, "OpenAI").expect("rewrite every non-target");

    let rewritten = std::fs::read_to_string(&path).expect("read rollout");
    assert_eq!(
        rewritten.matches("\"model_provider\":\"OpenAI\"").count(),
        3
    );
    assert!(!rewritten.contains("\"model_provider\":\"aio\""));
    assert!(!rewritten.contains("\"model_provider\":\"third-party\""));
}

#[test]
fn history_only_sqlite_rewrites_every_non_target_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("history.db");
    let conn = Connection::open(&path).expect("open sqlite");
    conn.execute_batch(
        "CREATE TABLE threads (model_provider TEXT, has_user_event INTEGER);\n\
         INSERT INTO threads VALUES ('aio', 1);\n\
         INSERT INTO threads VALUES ('third-party', 1);\n\
         INSERT INTO threads VALUES ('OpenAI', 1);",
    )
    .expect("seed sqlite");
    drop(conn);

    let change = collect_sqlite_change(&path, Some("aio"), "OpenAI")
        .expect("collect every non-target change");
    assert_eq!(change.provider_rows_updated, 2);
    let counts = apply_sqlite_changes(&[change], "OpenAI").expect("apply every non-target change");
    assert_eq!(counts.provider_rows_updated, 2);

    let conn = Connection::open(&path).expect("reopen sqlite");
    let providers = conn
        .prepare("SELECT model_provider FROM threads ORDER BY rowid")
        .expect("prepare query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query providers")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect providers");
    assert_eq!(providers, vec!["OpenAI", "OpenAI", "OpenAI"]);
}

#[test]
fn history_only_global_state_rewrites_any_non_target_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(".codex-global-state.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "model_provider": "third-party",
            "future": {"preserved": true}
        }))
        .expect("serialize global state"),
    )
    .expect("write global state");

    let change = collect_global_state_change(temp.path(), Some("aio"), "OpenAI")
        .expect("collect global state change")
        .expect("non-target global state must change");
    let next: Value = serde_json::from_slice(
        change
            .next_bytes
            .as_deref()
            .expect("global state replacement bytes"),
    )
    .expect("parse rewritten global state");

    assert_eq!(next["model_provider"], "OpenAI");
    assert_eq!(next["future"]["preserved"], true);
}

#[test]
fn disk_backup_restores_sqlite_bytes_and_removes_new_sidecars() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target_dir = temp.path().join("target");
    let backup_dir = temp.path().join("backup");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");

    let db = target_dir.join("state.db");
    let wal = target_dir.join("state.db-wal");
    let shm = target_dir.join("state.db-shm");
    let new_wal = target_dir.join("other.db-wal");
    let before = [
        (&db, backup_dir.join("state.db"), b"db-before".as_slice()),
        (
            &wal,
            backup_dir.join("state.db-wal"),
            b"wal-before".as_slice(),
        ),
        (
            &shm,
            backup_dir.join("state.db-shm"),
            b"shm-before".as_slice(),
        ),
    ];
    let mut entries = Vec::new();
    for (target, backup_path, bytes) in &before {
        std::fs::write(target, bytes).expect("write original sqlite bytes");
        entries.push(
            create_disk_backup_entry(target, backup_path, "sqlite test file")
                .expect("create sqlite backup"),
        );
        std::fs::write(target, b"changed").expect("mutate sqlite file");
    }
    entries.push(DiskBackupEntry {
        target_path: new_wal.clone(),
        backup_path: backup_dir.join("other.db-wal"),
        existed: false,
    });
    std::fs::write(&new_wal, b"created-during-sync").expect("create new sidecar");

    restore_backup(&ProviderSyncBackup {
        backup_dir,
        entries,
    })
    .expect("restore sqlite backup");

    for (target, _, bytes) in before {
        assert_eq!(std::fs::read(target).expect("read restored bytes"), bytes);
    }
    assert!(!new_wal.exists(), "new WAL sidecar should be removed");
}

#[test]
fn target_provider_rejects_unmanaged_raw_toml() {
    let err = codex_provider_target_from_config_text(
        "model_provider = \"Anthropic\"\n[model_providers.Anthropic]\nname = \"Anthropic\"\n",
    )
    .expect_err("unsupported raw config should fail");

    assert!(
        err.to_string()
            .contains("CODEX_PROVIDER_SYNC_INVALID_TARGET"),
        "{err}"
    );
}

#[test]
fn target_provider_parses_toml_comments() {
    assert_eq!(
        codex_provider_target_from_config_text(
            "model_provider = \"OpenAI\" # keep remote compaction provider\n\
             [model_providers.OpenAI]\n\
             name = \"OpenAI\"\n",
        )
        .expect("commented model_provider should parse"),
        "OpenAI"
    );
}

#[test]
fn current_config_provider_defaults_to_aio_when_missing() {
    assert_eq!(
        codex_provider_target_from_current_config_text("approval_policy = \"on-request\"\n")
            .expect("valid missing-provider config should default"),
        "aio"
    );
}

#[test]
fn current_config_provider_rejects_invalid_toml() {
    let err = codex_provider_target_from_current_config_text("model_provider =")
        .expect_err("invalid TOML should fail closed");
    assert!(
        err.to_string()
            .contains("CODEX_PROVIDER_SYNC_INVALID_CONFIG"),
        "{err}"
    );
}

#[test]
fn backup_pruning_keeps_only_latest_five_managed_backups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path();
    let root = home.join(PROVIDER_SYNC_BACKUP_ROOT);
    std::fs::create_dir_all(&root).expect("create backup root");

    for idx in 1..=6 {
        let dir = root.join(format!("{idx}"));
        std::fs::create_dir_all(&dir).expect("create backup dir");
        std::fs::write(
            dir.join(PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST),
            serde_json::json!({
                "managed_by": "Codex provider sync",
                "created_at": format!("{idx:02}")
            })
            .to_string(),
        )
        .expect("write manifest");
    }

    let warning = prune_managed_backups(home).expect("prune");
    assert!(warning.is_none(), "{warning:?}");

    let remaining: Vec<String> = std::fs::read_dir(&root)
        .expect("read root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(remaining.len(), 5, "{remaining:?}");
    assert!(!remaining.contains(&"1".to_string()), "{remaining:?}");
}

#[test]
fn running_app_override_blocks_sync() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path();
    std::fs::create_dir_all(home.join("tmp")).expect("create tmp");

    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(true));
    let is_running = codex_app_is_running().expect("override should not query process list");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    assert!(is_running, "override should force running state");
}

#[test]
fn process_check_failed_message_explains_next_step() {
    let message = codex_process_check_failed_message("tasklist", "exit status 1");

    assert!(
        message.contains("CODEX_PROVIDER_SYNC_PROCESS_CHECK_FAILED"),
        "{message}"
    );
    assert!(
        message.contains("unable to verify whether Codex App is closed"),
        "{message}"
    );
    assert!(message.contains("tasklist"), "{message}");
    assert!(
        message.contains("Please confirm Codex App is fully closed, then retry."),
        "{message}"
    );
}
