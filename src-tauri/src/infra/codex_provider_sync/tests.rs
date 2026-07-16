use super::*;

#[test]
fn provider_sync_preflight_rejects_running_codex_before_mutation() {
    let error = reject_running_codex_for_provider_sync(true).unwrap_err();
    assert!(error
        .to_string()
        .contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"));
    reject_running_codex_for_provider_sync(false).unwrap();
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
fn provider_identity_from_config_text_accepts_unmanaged_canonical_provider() {
    assert_eq!(
        codex_provider_identity_from_config_text(
            "model_provider = \"Anthropic\"\n[model_providers.Anthropic]\nname = \"Anthropic\"\n",
        )
        .expect("Anthropic canonical provider"),
        "Anthropic"
    );
}

#[test]
fn provider_identity_from_config_text_rejects_missing_provider() {
    let err = codex_provider_identity_from_config_text("approval_policy = \"on-request\"\n")
        .expect_err("missing canonical provider should fail closed");
    assert!(
        err.to_string()
            .contains("CODEX_PROVIDER_SYNC_INVALID_TARGET"),
        "{err}"
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
fn trusted_provider_sync_plan_accepts_unmanaged_restore_target() {
    let plan = codex_provider_sync_plan_for_trusted_target(
        "model_provider = \"aio\"\n[model_providers.aio]\nname = \"aio\"\n",
        "Anthropic",
    )
    .expect("trusted restore plan");

    assert_eq!(plan.current_provider.as_deref(), Some("aio"));
    assert_eq!(plan.target_provider, "Anthropic");
    assert!(plan.change_required, "{plan:?}");
    assert!(plan.codex_must_be_closed, "{plan:?}");
}

#[test]
fn provider_sync_plan_marks_noop_when_current_and_target_provider_match() {
    let plan = codex_provider_sync_plan_for_target(
        "model_provider = \"aio\"\n[model_providers.aio]\nname = \"aio\"\n",
        "aio",
    )
    .expect("provider sync plan");

    assert_eq!(plan.current_provider.as_deref(), Some("aio"));
    assert_eq!(plan.target_provider, "aio");
    assert!(!plan.change_required, "{plan:?}");
    assert!(!plan.codex_must_be_closed, "{plan:?}");
}

#[test]
fn provider_sync_plan_reports_provider_change_and_close_requirement() {
    let plan = codex_provider_sync_plan_for_config_text(
        "model_provider = \"Anthropic\"\n[model_providers.Anthropic]\nname = \"Anthropic\"\n",
        "model_provider = \"OpenAI\"\n[model_providers.OpenAI]\nname = \"OpenAI\"\n",
    )
    .expect("provider sync plan");

    assert_eq!(plan.current_provider.as_deref(), Some("Anthropic"));
    assert_eq!(plan.target_provider, "OpenAI");
    assert!(plan.change_required, "{plan:?}");
    assert!(plan.codex_must_be_closed, "{plan:?}");
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
fn rollout_provider_rewrite_allows_length_changes_and_preserves_other_segments() {
    let session_meta = r#"{"type":"session_meta","payload":{"model_provider":"x","note":"keep"}}"#;
    let event = r#"{"type":"event_msg","payload":{"text":"model_provider=x"}}"#;
    let malformed = "not-json-without-final-newline";
    let input = format!("{session_meta}\r\n{event}\r\n{malformed}");

    let output = rewrite_rollout_session_meta_providers(input.as_bytes(), "Anthropic")
        .expect("rewrite rollout provider");
    let output = String::from_utf8(output).expect("rewritten rollout remains UTF-8");
    let mut segments = output.split_inclusive('\n');
    let rewritten_meta = segments.next().expect("session meta segment");
    let preserved_event = segments.next().expect("event segment");
    let preserved_malformed = segments.next().expect("malformed segment");

    let rewritten_value: Value =
        serde_json::from_str(rewritten_meta.trim_end_matches(['\r', '\n']))
            .expect("parse rewritten session meta");
    assert_eq!(
        rewritten_value["payload"]["model_provider"],
        Value::String("Anthropic".to_string())
    );
    assert_eq!(rewritten_value["payload"]["note"], "keep");
    assert_eq!(preserved_event, format!("{event}\r\n"));
    assert_eq!(preserved_malformed, malformed);
    assert!(segments.next().is_none());
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

fn rollback_token_fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    CodexProviderSyncRollback,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("provider-state.jsonl");
    std::fs::write(&path, b"before").expect("write original state");
    let token = CodexProviderSyncRollback::new(
        vec![FileSnapshot {
            path: path.clone(),
            existed: true,
            bytes: Some(b"before".to_vec()),
        }],
        None,
        None,
        None,
        false,
    );
    std::fs::write(&path, b"after").expect("write mutated state");
    (dir, path, token)
}

#[test]
fn provider_sync_rollback_token_commits_without_restoring() {
    let (_dir, path, token) = rollback_token_fixture();
    token.commit();
    assert_eq!(std::fs::read(path).expect("read committed state"), b"after");
}

#[test]
fn provider_sync_rollback_token_restores_explicitly() {
    let (_dir, path, token) = rollback_token_fixture();
    token.rollback().expect("explicit rollback");
    assert_eq!(std::fs::read(path).expect("read restored state"), b"before");
}

#[test]
fn provider_sync_rollback_token_restores_when_dropped_unfinished() {
    let (_dir, path, token) = rollback_token_fixture();
    drop(token);
    assert_eq!(std::fs::read(path).expect("read restored state"), b"before");
}

#[test]
fn provider_sync_rollback_token_removes_only_the_uncommitted_backup() {
    let home = tempfile::tempdir().expect("home");
    let backup_root = home.path().join(PROVIDER_SYNC_BACKUP_ROOT);
    let prior_backup = backup_root.join("prior");
    let current_backup = backup_root.join("current");
    for (path, created_at) in [(&prior_backup, "1"), (&current_backup, "2")] {
        std::fs::create_dir_all(path).expect("create managed backup");
        std::fs::write(
            path.join(PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST),
            serde_json::to_vec(&serde_json::json!({
                "managed_by": "Codex provider sync",
                "created_at": created_at,
            }))
            .expect("serialize backup manifest"),
        )
        .expect("write backup manifest");
    }

    CodexProviderSyncRollback::new(
        Vec::new(),
        None,
        Some(home.path().to_path_buf()),
        Some(current_backup.clone()),
        false,
    )
    .rollback()
    .expect("rollback provider sync backup");

    assert!(prior_backup.exists());
    assert!(!current_backup.exists());
}

fn managed_backup_fixture(home: &Path, name: &str) -> PathBuf {
    let backup = home.join(PROVIDER_SYNC_BACKUP_ROOT).join(name);
    std::fs::create_dir_all(&backup).expect("create managed backup");
    std::fs::write(
        backup.join(PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST),
        serde_json::to_vec(&serde_json::json!({
            "managed_by": "Codex provider sync",
            "created_at": "1",
        }))
        .expect("serialize managed backup manifest"),
    )
    .expect("write managed backup manifest");
    backup
}

fn persistent_snapshot_corruption_fixture() -> (tempfile::TempDir, [PathBuf; 2]) {
    let home = tempfile::tempdir().expect("Codex home");
    let targets = [
        home.path().join("config.toml"),
        home.path().join("sessions/2026/rollout.jsonl"),
    ];
    for (index, target) in targets.iter().enumerate() {
        std::fs::create_dir_all(target.parent().expect("target parent"))
            .expect("create target parent");
        std::fs::write(target, format!("before-{index}")).expect("write snapshot source");
    }
    let snapshots = targets
        .iter()
        .map(|target| snapshot_path(target).expect("snapshot target"))
        .collect::<Vec<_>>();
    let backup = managed_backup_fixture(home.path(), "corrupt-preflight");
    prepare_provider_sync_transaction(
        home.path(),
        "corrupt-preflight",
        "aio",
        &sha256_hex(b"target-config"),
        None,
        &backup,
        true,
        &snapshots,
    )
    .expect("prepare persistent snapshot fixture");
    for (index, target) in targets.iter().enumerate() {
        std::fs::write(target, format!("after-{index}")).expect("mutate target after prepare");
    }
    (home, targets)
}

fn assert_persistent_snapshot_targets_remain_unmodified(targets: &[PathBuf; 2]) {
    for (index, target) in targets.iter().enumerate() {
        assert_eq!(
            std::fs::read(target).expect("read target after failed preflight"),
            format!("after-{index}").as_bytes(),
            "{} must not be partially restored",
            target.display()
        );
    }
}

#[test]
fn persistent_snapshot_recovery_quarantines_hash_corruption_before_any_restore() {
    let (home, targets) = persistent_snapshot_corruption_fixture();
    let backup = provider_sync_transaction_root(home.path()).join("files/00000001.bin");
    std::fs::write(&backup, b"corrupt!" /* same length as before-1 */)
        .expect("corrupt backup without changing length");

    let outcome = recover_interrupted_provider_sync_from_home(home.path(), None, Some(false))
        .expect("hash-corrupt snapshot must be quarantined");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::Quarantined);
    assert_persistent_snapshot_targets_remain_unmodified(&targets);
    assert_provider_sync_transaction_quarantined(home.path());
}

#[test]
fn persistent_snapshot_recovery_quarantines_missing_and_truncated_backups_before_restore() {
    for corruption in ["missing", "truncated"] {
        let (home, targets) = persistent_snapshot_corruption_fixture();
        let backup = provider_sync_transaction_root(home.path()).join("files/00000001.bin");
        if corruption == "missing" {
            std::fs::remove_file(&backup).expect("remove backup");
        } else {
            std::fs::write(&backup, b"short").expect("truncate backup");
        }

        let outcome = recover_interrupted_provider_sync_from_home(home.path(), None, Some(false))
            .expect("invalid snapshot backup must be quarantined");

        assert_eq!(
            outcome,
            CodexProviderSyncRecoveryOutcome::Quarantined,
            "{corruption}"
        );
        assert_persistent_snapshot_targets_remain_unmodified(&targets);
        assert_provider_sync_transaction_quarantined(home.path());
    }
}

#[test]
fn persistent_snapshot_recovery_quarantines_oversized_declared_snapshot_before_read() {
    let (home, targets) = persistent_snapshot_corruption_fixture();
    let manifest_path = provider_sync_transaction_manifest_path(home.path());
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read transaction manifest"))
            .expect("parse transaction manifest");
    manifest["snapshots"][0]["byte_len"] =
        serde_json::Value::from(PROVIDER_SYNC_SNAPSHOT_MAX_BYTES as u64 + 1);
    write_file_atomic(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).expect("serialize oversized manifest"),
    )
    .expect("write oversized declaration");

    let outcome = recover_interrupted_provider_sync_from_home(home.path(), None, Some(false))
        .expect("oversized declared snapshot must be quarantined");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::Quarantined);
    assert_persistent_snapshot_targets_remain_unmodified(&targets);
    assert_provider_sync_transaction_quarantined(home.path());
}

fn assert_provider_sync_transaction_quarantined(home: &Path) {
    assert!(!provider_sync_transaction_root(home).exists());
    let quarantine_root = home.join(PROVIDER_SYNC_TRANSACTION_QUARANTINE_ROOT);
    let quarantines = std::fs::read_dir(&quarantine_root)
        .expect("read provider sync quarantine root")
        .collect::<Result<Vec<_>, _>>()
        .expect("read provider sync quarantine entries");
    assert_eq!(quarantines.len(), 1);
    let quarantine = quarantines[0].path();
    assert!(quarantine.join("transaction").is_dir());
    assert!(quarantine.join("quarantine.json").is_file());
}

#[test]
fn interrupted_prepared_transaction_restores_every_snapshot_and_stale_lock() {
    let home = tempfile::tempdir().expect("Codex home");
    let home = home.path();
    let existing_paths = [
        home.join("config.toml"),
        home.join("config.toml.bak"),
        home.join("sessions/2026/rollout.jsonl"),
        home.join("sqlite/codex-dev.db"),
        home.join("sqlite/codex-dev.db-wal"),
        home.join("sqlite/codex-dev.db-shm"),
        home.join(".codex-global-state.json"),
        home.join(".codex-global-state.json.bak"),
    ];
    for (index, path) in existing_paths.iter().enumerate() {
        std::fs::create_dir_all(path.parent().expect("snapshot parent"))
            .expect("create snapshot parent");
        std::fs::write(path, format!("before-{index}")).expect("write original snapshot");
    }
    let newly_created = home.join("sessions/2026/new-rollout.jsonl");
    let mut snapshots = existing_paths
        .iter()
        .map(|path| snapshot_path(path).expect("snapshot existing path"))
        .collect::<Vec<_>>();
    snapshots.push(snapshot_path(&newly_created).expect("snapshot missing path"));
    let backup = managed_backup_fixture(home, "interrupted");

    prepare_provider_sync_transaction(
        home,
        "route-operation",
        "OpenAI",
        &sha256_hex(b"after-config"),
        Some(CodexProviderSyncRouteContext {
            operation_id: "route-operation".to_string(),
            target_generation: 9,
            target_mode: CodexRouteMode::Guarded,
            target_live_config_sha256: "target-live-hash".to_string(),
        }),
        &backup,
        false,
        &snapshots,
    )
    .expect("prepare persistent transaction");

    for path in &existing_paths {
        std::fs::write(path, b"after").expect("mutate snapshot target");
    }
    std::fs::write(&newly_created, b"created-after-prepare")
        .expect("create previously missing target");
    std::fs::create_dir_all(home.join(PROVIDER_SYNC_LOCK_FILE)).expect("create stale lock");
    let orphan_staging = home
        .join("tmp")
        .join(format!("{PROVIDER_SYNC_TRANSACTION_STAGING_PREFIX}orphan"));
    std::fs::create_dir_all(&orphan_staging).expect("create orphan staging");

    let outcome = recover_interrupted_provider_sync_from_home(
        home,
        Some(&CodexProviderSyncCurrentRoute {
            generation: 9,
            mode: CodexRouteMode::Guarded,
            live_config_sha256: "target-live-hash".to_string(),
            live_matches_projection: true,
            auth_matches_projection: true,
            pending_operation_id: Some("route-operation".to_string()),
        }),
        Some(false),
    )
    .expect("recover interrupted provider sync");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::Restored);
    for (index, path) in existing_paths.iter().enumerate() {
        assert_eq!(
            std::fs::read(path).expect("read restored snapshot"),
            format!("before-{index}").as_bytes()
        );
    }
    assert!(!newly_created.exists());
    assert!(!provider_sync_transaction_root(home).exists());
    assert!(!home.join(PROVIDER_SYNC_LOCK_FILE).exists());
    assert!(!orphan_staging.exists());
    assert!(!home.join(PROVIDER_SYNC_BACKUP_ROOT).exists());
}

#[test]
fn interrupted_applied_route_transaction_finalizes_only_when_route_matches() {
    let home = tempfile::tempdir().expect("Codex home");
    let home = home.path();
    let config = home.join("config.toml");
    std::fs::write(&config, b"before").expect("write original config");
    let snapshots = vec![snapshot_path(&config).expect("snapshot config")];
    let backup = managed_backup_fixture(home, "applied");
    let target = b"after";
    prepare_provider_sync_transaction(
        home,
        "route-operation",
        "OpenAI",
        &sha256_hex(target),
        Some(CodexProviderSyncRouteContext {
            operation_id: "route-operation".to_string(),
            target_generation: 10,
            target_mode: CodexRouteMode::Guarded,
            target_live_config_sha256: "target-live-hash".to_string(),
        }),
        &backup,
        true,
        &snapshots,
    )
    .expect("prepare transaction");
    std::fs::write(&config, target).expect("write target config");
    mark_provider_sync_transaction_applied(home, "route-operation")
        .expect("mark transaction applied");
    std::fs::create_dir_all(home.join(PROVIDER_SYNC_LOCK_FILE)).expect("create stale lock");

    let outcome = recover_interrupted_provider_sync_from_home(
        home,
        Some(&CodexProviderSyncCurrentRoute {
            generation: 10,
            mode: CodexRouteMode::Guarded,
            live_config_sha256: "target-live-hash".to_string(),
            live_matches_projection: true,
            auth_matches_projection: true,
            pending_operation_id: Some("route-operation".to_string()),
        }),
        Some(false),
    )
    .expect("finalize applied transaction");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::Finalized);
    assert_eq!(std::fs::read(&config).expect("read target config"), target);
    assert!(backup.exists(), "committed user backup must be retained");
    assert!(!provider_sync_transaction_root(home).exists());
    assert!(!home.join(PROVIDER_SYNC_LOCK_FILE).exists());
}

#[test]
fn interrupted_applied_route_transaction_restores_on_route_mismatch() {
    let home = tempfile::tempdir().expect("Codex home");
    let home = home.path();
    let config = home.join("config.toml");
    std::fs::write(&config, b"before").expect("write original config");
    let snapshots = vec![snapshot_path(&config).expect("snapshot config")];
    let backup = managed_backup_fixture(home, "mismatch");
    prepare_provider_sync_transaction(
        home,
        "route-operation",
        "OpenAI",
        &sha256_hex(b"after"),
        Some(CodexProviderSyncRouteContext {
            operation_id: "route-operation".to_string(),
            target_generation: 10,
            target_mode: CodexRouteMode::Guarded,
            target_live_config_sha256: "target-live-hash".to_string(),
        }),
        &backup,
        false,
        &snapshots,
    )
    .expect("prepare transaction");
    std::fs::write(&config, b"after").expect("write target config");
    mark_provider_sync_transaction_applied(home, "route-operation")
        .expect("mark transaction applied");

    let outcome = recover_interrupted_provider_sync_from_home(
        home,
        Some(&CodexProviderSyncCurrentRoute {
            generation: 10,
            mode: CodexRouteMode::Guarded,
            live_config_sha256: "target-live-hash".to_string(),
            live_matches_projection: true,
            auth_matches_projection: true,
            pending_operation_id: Some("different-route-operation".to_string()),
        }),
        Some(false),
    )
    .expect("restore mismatched applied transaction");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::Restored);
    assert_eq!(
        std::fs::read(&config).expect("read restored config"),
        b"before"
    );
    assert!(!backup.exists());
}

#[test]
fn interrupted_transaction_waits_for_codex_to_close_before_restoring() {
    let home = tempfile::tempdir().expect("Codex home");
    let home = home.path();
    let config = home.join("config.toml");
    std::fs::write(&config, b"before").expect("write original config");
    let snapshots = vec![snapshot_path(&config).expect("snapshot config")];
    let backup = managed_backup_fixture(home, "running");
    prepare_provider_sync_transaction(
        home,
        "route-operation",
        "OpenAI",
        &sha256_hex(b"after"),
        None,
        &backup,
        false,
        &snapshots,
    )
    .expect("prepare transaction");
    std::fs::write(&config, b"after").expect("write partial target config");
    std::fs::create_dir_all(home.join(PROVIDER_SYNC_LOCK_FILE)).expect("create stale lock");

    let error = recover_interrupted_provider_sync_from_home(home, None, Some(true))
        .expect_err("running Codex must block snapshot restoration");

    assert!(
        error
            .to_string()
            .contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(&config).expect("read unchanged partial config"),
        b"after"
    );
    assert!(provider_sync_transaction_root(home).exists());
    assert!(home.join(PROVIDER_SYNC_LOCK_FILE).exists());
    assert!(backup.exists());
}

#[test]
fn startup_recovery_removes_stale_lock_without_a_transaction() {
    let home = tempfile::tempdir().expect("Codex home");
    let lock = home.path().join(PROVIDER_SYNC_LOCK_FILE);
    std::fs::create_dir_all(&lock).expect("create stale lock");
    std::fs::write(lock.join("owner.json"), b"{}").expect("write stale owner");

    let outcome = recover_interrupted_provider_sync_from_home(home.path(), None, Some(false))
        .expect("recover stale lock");

    assert_eq!(outcome, CodexProviderSyncRecoveryOutcome::StaleLockRemoved);
    assert!(!lock.exists());
}
