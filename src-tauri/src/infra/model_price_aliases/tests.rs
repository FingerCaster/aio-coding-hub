use super::*;
use std::ffi::OsString;

struct ScopedEnvVar {
    key: &'static str,
    value: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: impl Into<OsString>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value.into());
        Self {
            key,
            value: previous,
        }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.value.take() {
            Some(previous) => std::env::set_var(self.key, previous),
            None => std::env::remove_var(self.key),
        }
    }
}

fn with_test_aliases_path<T>(
    test: impl FnOnce(&tauri::AppHandle<tauri::test::MockRuntime>, &Path) -> T,
) -> T {
    let _guard = crate::test_support::test_env_lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = ScopedEnvVar::set("AIO_CODING_HUB_TEST_HOME", temp.path());
    let app = tauri::test::mock_app();
    let handle = app.handle().clone();
    let path = aliases_path(&handle).expect("aliases path");
    test(&handle, &path)
}

fn read_fixture_error(bytes: &[u8]) -> String {
    with_test_aliases_path(|handle, path| {
        std::fs::write(path, bytes).expect("write aliases fixture");
        read(handle)
            .expect_err("strict aliases read must reject invalid content")
            .to_string()
    })
}

#[test]
fn wildcard_single_star_matches_prefix_suffix() {
    assert!(match_wildcard_single("a*b", "axxb"));
    assert!(match_wildcard_single("*b", "b"));
    assert!(match_wildcard_single("a*", "a"));
    assert!(!match_wildcard_single("a*b", "abx"));
    assert!(!match_wildcard_single("a*b*c", "abc"));
}

#[test]
fn default_alias_resolves_grok_build_price_model() {
    let aliases = ModelPriceAliasesV1::default();

    assert_eq!(
        aliases.resolve_target_model("grok", "grok-build"),
        Some("grok-build-0.1")
    );
}

#[test]
fn v1_aliases_migrate_grok_build_default_without_losing_user_rules() {
    let aliases = ModelPriceAliasesV1 {
        version: 1,
        rules: vec![ModelPriceAliasRuleV1 {
            cli_key: "gemini".to_string(),
            match_type: ModelPriceAliasMatchTypeV1::Prefix,
            pattern: "gemini-3".to_string(),
            target_model: "gemini-3-preview".to_string(),
            enabled: true,
        }],
    };

    let migrated = validate_aliases(aliases).expect("v1 aliases should migrate");

    assert_eq!(migrated.version, 2);
    assert_eq!(
        migrated.resolve_target_model("gemini", "gemini-3-flash"),
        Some("gemini-3-preview")
    );
    assert_eq!(
        migrated.resolve_target_model("grok", "grok-build"),
        Some("grok-build-0.1")
    );
}

#[test]
fn v2_aliases_preserve_a_user_deleted_grok_build_default() {
    let aliases = ModelPriceAliasesV1 {
        version: 2,
        rules: Vec::new(),
    };

    let validated = validate_aliases(aliases).expect("v2 aliases should remain valid");

    assert_eq!(validated.version, 2);
    assert_eq!(validated.resolve_target_model("grok", "grok-build"), None);
}

#[test]
fn resolves_exact_over_wildcard_over_prefix() {
    let aliases = ModelPriceAliasesV1 {
        version: 1,
        rules: vec![
            ModelPriceAliasRuleV1 {
                cli_key: "gemini".to_string(),
                match_type: ModelPriceAliasMatchTypeV1::Prefix,
                pattern: "gemini-3".to_string(),
                target_model: "gemini-3-any".to_string(),
                enabled: true,
            },
            ModelPriceAliasRuleV1 {
                cli_key: "gemini".to_string(),
                match_type: ModelPriceAliasMatchTypeV1::Wildcard,
                pattern: "gemini-3-*".to_string(),
                target_model: "gemini-3-wild".to_string(),
                enabled: true,
            },
            ModelPriceAliasRuleV1 {
                cli_key: "gemini".to_string(),
                match_type: ModelPriceAliasMatchTypeV1::Exact,
                pattern: "gemini-3-flash".to_string(),
                target_model: "gemini-3-flash-preview".to_string(),
                enabled: true,
            },
        ],
    };

    assert_eq!(
        aliases.resolve_target_model("gemini", "gemini-3-flash"),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        aliases.resolve_target_model("gemini", "gemini-3-pro"),
        Some("gemini-3-wild")
    );
}

#[test]
fn resolves_longer_patterns_first_within_same_type() {
    let aliases = ModelPriceAliasesV1 {
        version: 1,
        rules: vec![
            ModelPriceAliasRuleV1 {
                cli_key: "claude".to_string(),
                match_type: ModelPriceAliasMatchTypeV1::Prefix,
                pattern: "claude-opus".to_string(),
                target_model: "a".to_string(),
                enabled: true,
            },
            ModelPriceAliasRuleV1 {
                cli_key: "claude".to_string(),
                match_type: ModelPriceAliasMatchTypeV1::Prefix,
                pattern: "claude-opus-4-5".to_string(),
                target_model: "b".to_string(),
                enabled: true,
            },
        ],
    };

    assert_eq!(
        aliases.resolve_target_model("claude", "claude-opus-4-5-thinking"),
        Some("b")
    );
}

#[test]
fn validate_aliases_rejects_too_many_rules() {
    let rule = ModelPriceAliasRuleV1 {
        cli_key: "claude".to_string(),
        match_type: ModelPriceAliasMatchTypeV1::Exact,
        pattern: "claude-opus-4-5".to_string(),
        target_model: "claude-opus-4-5".to_string(),
        enabled: true,
    };
    let aliases = ModelPriceAliasesV1 {
        version: 1,
        rules: vec![rule; ALIASES_RULES_MAX + 1],
    };

    let err = validate_aliases(aliases).unwrap_err().to_string();

    assert!(err.contains("too many price alias rules"));
}

#[test]
fn write_json_atomically_rejects_oversized_aliases_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("price-aliases.json");

    let err = write_json_atomically(&path, vec![b'x'; ALIASES_FILE_MAX_BYTES + 1])
        .unwrap_err()
        .to_string();

    assert!(err.contains("price aliases file too large"));
    assert!(!path.exists());
}

#[test]
fn strict_read_rejects_malformed_json() {
    let err = read_fixture_error(b"{invalid json");

    assert!(err.contains("failed to parse aliases"));
}

#[test]
fn strict_read_rejects_invalid_utf8() {
    let err = read_fixture_error(&[0xff, 0xfe, 0xfd]);

    assert!(err.contains("invalid price aliases UTF-8"));
}

#[test]
fn strict_read_rejects_oversized_file() {
    let err = read_fixture_error(&vec![b'x'; ALIASES_FILE_MAX_BYTES + 1]);

    assert!(err.contains("too large"));
}

#[test]
fn strict_read_uses_defaults_only_when_alias_file_is_missing() {
    let aliases = with_test_aliases_path(|handle, path| {
        assert!(!path.exists());
        read(handle).expect("missing aliases file should use defaults")
    });

    assert_eq!(aliases.version, ALIASES_SCHEMA_VERSION_CURRENT);
    assert_eq!(
        aliases.resolve_target_model("grok", "grok-build"),
        Some("grok-build-0.1")
    );
}

#[cfg(unix)]
#[test]
fn strict_read_rejects_broken_alias_symlink_instead_of_treating_it_as_missing() {
    let err = with_test_aliases_path(|handle, path| {
        std::os::unix::fs::symlink(path.with_extension("missing"), path)
            .expect("create broken aliases symlink");
        assert!(
            !path.exists(),
            "broken symlink must reproduce the exists() trap"
        );
        read(handle)
            .expect_err("strict aliases read must reject a broken symlink")
            .to_string()
    });

    assert!(err.contains("failed to read aliases"));
}

#[test]
fn strict_read_rejects_unsupported_schema() {
    let err = read_fixture_error(br#"{"version":3,"rules":[]}"#);

    assert!(err.contains("unsupported aliases version 3"));
}

#[test]
fn strict_read_propagates_unreadable_alias_path() {
    let err = with_test_aliases_path(|handle, path| {
        std::fs::create_dir(path).expect("create unreadable aliases path");
        read(handle)
            .expect_err("strict aliases read must propagate filesystem errors")
            .to_string()
    });

    assert!(err.contains("failed to read aliases"));
}

#[test]
fn fail_open_read_keeps_cost_calculation_defaults() {
    let aliases = with_test_aliases_path(|handle, path| {
        std::fs::write(path, b"{invalid json").expect("write aliases fixture");
        read_fail_open(handle)
    });

    assert_eq!(aliases.version, ALIASES_SCHEMA_VERSION_CURRENT);
    assert_eq!(
        aliases.resolve_target_model("grok", "grok-build"),
        Some("grok-build-0.1")
    );
}
