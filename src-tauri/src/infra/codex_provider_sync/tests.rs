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
        Some(home.path().to_path_buf()),
        Some(current_backup.clone()),
        false,
    )
    .rollback()
    .expect("rollback provider sync backup");

    assert!(prior_backup.exists());
    assert!(!current_backup.exists());
}
