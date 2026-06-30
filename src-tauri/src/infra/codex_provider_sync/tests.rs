use super::*;

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
