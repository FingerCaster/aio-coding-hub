use super::queries::{get_source_provider_for_gateway, pool_order_set};
use super::*;
use rusqlite::OptionalExtension;

// -- ClaudeModels::map_model --

#[test]
fn claude_models_no_config_keeps_original() {
    let models = ClaudeModels::default();
    assert_eq!(
        models.map_model("claude-sonnet-4", false),
        "claude-sonnet-4"
    );
}

#[test]
fn claude_models_type_slot_prevents_thinking_reasoning_override() {
    let models = ClaudeModels {
        main_model: Some("glm-main".to_string()),
        reasoning_model: Some("glm-thinking".to_string()),
        haiku_model: Some("claude-haiku-4-5-20251001".to_string()),
        sonnet_model: Some("glm-sonnet".to_string()),
        opus_model: Some("glm-opus".to_string()),
    }
    .normalized();

    assert_eq!(
        models.map_model("claude-haiku-4-5-20251001", true),
        "claude-haiku-4-5-20251001"
    );
    assert_eq!(models.map_model("claude-sonnet-4", true), "glm-sonnet");
    assert_eq!(models.map_model("claude-opus-4", true), "glm-opus");
}

#[test]
fn claude_models_thinking_uses_reasoning_for_unknown_model() {
    let models = ClaudeModels {
        main_model: Some("glm-main".to_string()),
        reasoning_model: Some("glm-thinking".to_string()),
        haiku_model: Some("glm-haiku".to_string()),
        sonnet_model: Some("glm-sonnet".to_string()),
        opus_model: Some("glm-opus".to_string()),
    }
    .normalized();

    assert_eq!(models.map_model("some-unknown-model", true), "glm-thinking");
}

#[test]
fn claude_models_type_slot_selected_by_substring() {
    let models = ClaudeModels {
        main_model: Some("glm-main".to_string()),
        haiku_model: Some("glm-haiku".to_string()),
        sonnet_model: Some("glm-sonnet".to_string()),
        opus_model: Some("glm-opus".to_string()),
        ..Default::default()
    }
    .normalized();

    assert_eq!(models.map_model("claude-haiku-4", false), "glm-haiku");
    assert_eq!(models.map_model("claude-sonnet-4", false), "glm-sonnet");
    assert_eq!(models.map_model("claude-opus-4", false), "glm-opus");
}

#[test]
fn claude_models_falls_back_to_main_model() {
    let models = ClaudeModels {
        main_model: Some("glm-main".to_string()),
        ..Default::default()
    }
    .normalized();

    assert_eq!(models.map_model("some-unknown-model", false), "glm-main");
}

// -- ClaudeModels::has_any --

#[test]
fn claude_models_has_any_false_for_default() {
    assert!(!ClaudeModels::default().has_any());
}

#[test]
fn claude_models_has_any_true_with_main_model() {
    let models = ClaudeModels {
        main_model: Some("test".to_string()),
        ..Default::default()
    };
    assert!(models.has_any());
}

// -- normalize_model_slot --

#[test]
fn normalize_model_slot_trims_whitespace() {
    assert_eq!(
        normalize_model_slot(Some("  model-name  ".to_string())),
        Some("model-name".to_string())
    );
}

#[test]
fn normalize_model_slot_returns_none_for_empty() {
    assert!(normalize_model_slot(Some("".to_string())).is_none());
}

#[test]
fn normalize_model_slot_returns_none_for_whitespace_only() {
    assert!(normalize_model_slot(Some("   ".to_string())).is_none());
}

#[test]
fn normalize_model_slot_returns_none_for_none() {
    assert!(normalize_model_slot(None).is_none());
}

#[test]
fn normalize_model_slot_truncates_long_names() {
    let long_name = "a".repeat(MAX_MODEL_NAME_LEN + 50);
    let result = normalize_model_slot(Some(long_name));
    assert_eq!(result.as_ref().map(|s| s.len()), Some(MAX_MODEL_NAME_LEN));
}

#[test]
fn normalize_model_slot_truncates_multibyte_without_panic() {
    let long_name = "模".repeat(MAX_MODEL_NAME_LEN + 1);
    let result = normalize_model_slot(Some(long_name)).expect("normalized model");
    assert_eq!(result.chars().count(), MAX_MODEL_NAME_LEN);
}

#[test]
fn get_source_provider_for_gateway_allows_cross_cli_codex_bridge_sources() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("providers.sqlite3");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut claude_params = default_provider_params("Claude source");
    claude_params.base_urls = vec!["https://api.anthropic.com/v1".to_string()];
    let claude_source = upsert(&db, claude_params).expect("insert claude source");

    let mut codex_params = default_provider_params("Codex source");
    codex_params.cli_key = "codex".to_string();
    codex_params.base_urls = vec!["https://codex.example.com/v1".to_string()];
    let codex_source = upsert(&db, codex_params).expect("insert codex source");

    let (chat_source, chat_cli_key) =
        get_source_provider_for_gateway(&db, claude_source.id, CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE)
            .expect("chat bridge source");
    assert_eq!(chat_source.id, claude_source.id);
    assert_eq!(chat_cli_key, "claude");

    let (anthropic_source, anthropic_cli_key) = get_source_provider_for_gateway(
        &db,
        codex_source.id,
        CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE,
    )
    .expect("anthropic bridge source");
    assert_eq!(anthropic_source.id, codex_source.id);
    assert_eq!(anthropic_cli_key, "codex");
}

#[test]
fn get_source_provider_for_gateway_allows_disabled_source_for_codex_bridge() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("providers-disabled-source.sqlite3");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut source_params = default_provider_params("Disabled Codex source");
    source_params.cli_key = "codex".to_string();
    source_params.enabled = false;
    let source = upsert(&db, source_params).expect("insert codex source");

    let (resolved, cli_key) =
        get_source_provider_for_gateway(&db, source.id, CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE)
            .expect("codex responses bridge source");

    assert_eq!(resolved.id, source.id);
    assert_eq!(cli_key, "codex");
}

#[test]
fn get_source_provider_for_gateway_keeps_cx2cc_codex_source_requirement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("providers.sqlite3");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut claude_params = default_provider_params("Claude source");
    claude_params.base_urls = vec!["https://api.anthropic.com/v1".to_string()];
    let claude_source = upsert(&db, claude_params).expect("insert claude source");

    let err = get_source_provider_for_gateway(&db, claude_source.id, CX2CC_BRIDGE_TYPE)
        .expect_err("cx2cc should still reject non-codex source");
    assert!(err.to_string().contains("source provider not found"));
}

#[test]
fn get_source_provider_for_gateway_keeps_cx2cc_source_enabled_requirement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("providers-cx2cc-disabled-source.sqlite3");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut source_params = default_provider_params("Disabled Codex source");
    source_params.cli_key = "codex".to_string();
    source_params.enabled = false;
    let source = upsert(&db, source_params).expect("insert codex source");

    let err = get_source_provider_for_gateway(&db, source.id, CX2CC_BRIDGE_TYPE)
        .expect_err("cx2cc should still reject disabled source");
    assert!(err.to_string().contains("source provider not found"));
}

// -- ModelMapping JSON compatibility --

#[test]
fn model_mapping_from_json_reads_legacy_flat_exact_mapping() {
    let mapping =
        model_mapping_from_json(r#"{" gpt-5.5 ":" deepseek-chat ","":"ignored","gpt-5.4":""}"#);

    assert_eq!(mapping.default_model, None);
    assert_eq!(mapping.exact.len(), 1);
    assert_eq!(
        mapping.exact.get("gpt-5.5"),
        Some(&"deepseek-chat".to_string())
    );
}

#[test]
fn model_mapping_from_json_reads_structured_mapping() {
    let mapping = model_mapping_from_json(
        r#"{"default_model":" deepseek-reasoner ","exact":{" gpt-5.5 ":" deepseek-chat "}}"#,
    );

    assert_eq!(mapping.default_model.as_deref(), Some("deepseek-reasoner"));
    assert_eq!(
        mapping.exact.get("gpt-5.5"),
        Some(&"deepseek-chat".to_string())
    );
}

// -- DailyResetMode::parse --

#[test]
fn daily_reset_mode_parse_fixed() {
    let mode = DailyResetMode::parse("fixed").unwrap();
    assert_eq!(mode.as_str(), "fixed");
}

#[test]
fn daily_reset_mode_parse_rolling() {
    let mode = DailyResetMode::parse("rolling").unwrap();
    assert_eq!(mode.as_str(), "rolling");
}

#[test]
fn daily_reset_mode_parse_invalid() {
    assert!(DailyResetMode::parse("invalid").is_none());
}

#[test]
fn daily_reset_mode_parse_trims_whitespace() {
    assert!(DailyResetMode::parse(" fixed ").is_some());
}

// -- ProviderBaseUrlMode::parse --

#[test]
fn base_url_mode_parse_order() {
    let mode = ProviderBaseUrlMode::parse("order").unwrap();
    assert_eq!(mode.as_str(), "order");
}

#[test]
fn base_url_mode_parse_ping() {
    let mode = ProviderBaseUrlMode::parse("ping").unwrap();
    assert_eq!(mode.as_str(), "ping");
}

#[test]
fn base_url_mode_parse_invalid() {
    assert!(ProviderBaseUrlMode::parse("random").is_none());
}

// -- parse_reset_time_hms --

#[test]
fn parse_reset_time_valid_hm() {
    assert_eq!(parse_reset_time_hms("08:30"), Some((8, 30, 0)));
}

#[test]
fn parse_reset_time_valid_hms() {
    assert_eq!(parse_reset_time_hms("23:59:59"), Some((23, 59, 59)));
}

#[test]
fn parse_reset_time_single_digit_hour() {
    assert_eq!(parse_reset_time_hms("8:30"), Some((8, 30, 0)));
}

#[test]
fn parse_reset_time_midnight() {
    assert_eq!(parse_reset_time_hms("00:00"), Some((0, 0, 0)));
}

#[test]
fn parse_reset_time_rejects_invalid_hour() {
    assert!(parse_reset_time_hms("25:00").is_none());
}

#[test]
fn parse_reset_time_rejects_invalid_minute() {
    assert!(parse_reset_time_hms("12:60").is_none());
}

#[test]
fn parse_reset_time_rejects_empty() {
    assert!(parse_reset_time_hms("").is_none());
}

#[test]
fn parse_reset_time_rejects_no_colon() {
    assert!(parse_reset_time_hms("1234").is_none());
}

#[test]
fn parse_reset_time_rejects_three_digit_hour() {
    assert!(parse_reset_time_hms("123:00").is_none());
}

// -- normalize_reset_time_hms_lossy --

#[test]
fn normalize_reset_time_lossy_valid_input() {
    assert_eq!(normalize_reset_time_hms_lossy("8:30"), "08:30:00");
}

#[test]
fn normalize_reset_time_lossy_invalid_falls_back() {
    assert_eq!(normalize_reset_time_hms_lossy("invalid"), "00:00:00");
}

// -- normalize_reset_time_hms_strict --

#[test]
fn normalize_reset_time_strict_valid_input() {
    assert_eq!(
        normalize_reset_time_hms_strict("daily_reset_time", "8:30").unwrap(),
        "08:30:00"
    );
}

#[test]
fn normalize_reset_time_strict_rejects_invalid() {
    assert!(normalize_reset_time_hms_strict("daily_reset_time", "invalid").is_err());
}

// -- validate_limit_usd --

#[test]
fn validate_limit_usd_none_passes() {
    assert_eq!(validate_limit_usd("test", None).unwrap(), None);
}

#[test]
fn validate_limit_usd_zero_passes() {
    assert_eq!(validate_limit_usd("test", Some(0.0)).unwrap(), Some(0.0));
}

#[test]
fn validate_limit_usd_positive_passes() {
    assert_eq!(
        validate_limit_usd("test", Some(100.0)).unwrap(),
        Some(100.0)
    );
}

#[test]
fn validate_limit_usd_rejects_negative() {
    assert!(validate_limit_usd("test", Some(-1.0)).is_err());
}

#[test]
fn validate_limit_usd_rejects_infinity() {
    assert!(validate_limit_usd("test", Some(f64::INFINITY)).is_err());
}

#[test]
fn validate_limit_usd_rejects_nan() {
    assert!(validate_limit_usd("test", Some(f64::NAN)).is_err());
}

#[test]
fn validate_limit_usd_rejects_over_max() {
    assert!(validate_limit_usd("test", Some(MAX_LIMIT_USD + 1.0)).is_err());
}

#[test]
fn validate_limit_usd_accepts_max() {
    assert_eq!(
        validate_limit_usd("test", Some(MAX_LIMIT_USD)).unwrap(),
        Some(MAX_LIMIT_USD)
    );
}

// -- normalize_base_urls --

#[test]
fn normalize_base_urls_valid_single() {
    let result = normalize_base_urls(vec!["https://api.example.com".to_string()]).unwrap();
    assert_eq!(result, vec!["https://api.example.com"]);
}

#[test]
fn normalize_base_urls_deduplicates() {
    let result = normalize_base_urls(vec![
        "https://api.example.com".to_string(),
        "https://api.example.com".to_string(),
    ])
    .unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn normalize_base_urls_trims_whitespace() {
    let result = normalize_base_urls(vec!["  https://api.example.com  ".to_string()]).unwrap();
    assert_eq!(result, vec!["https://api.example.com"]);
}

#[test]
fn normalize_base_urls_skips_empty_entries() {
    let result = normalize_base_urls(vec![
        "".to_string(),
        "https://api.example.com".to_string(),
        "  ".to_string(),
    ])
    .unwrap();
    assert_eq!(result, vec!["https://api.example.com"]);
}

#[test]
fn normalize_base_urls_rejects_all_empty() {
    assert!(normalize_base_urls(vec!["".to_string(), "  ".to_string()]).is_err());
}

#[test]
fn normalize_base_urls_rejects_invalid_url() {
    assert!(normalize_base_urls(vec!["not a url".to_string()]).is_err());
}

#[test]
fn normalize_base_urls_rejects_too_many_urls() {
    let urls: Vec<String> = (0..=MAX_PROVIDER_BASE_URLS)
        .map(|idx| format!("https://api-{idx}.example.com"))
        .collect();
    let err = normalize_base_urls(urls).expect_err("too many urls");
    assert!(err.to_string().contains("base_urls must contain at most"));
}

#[test]
fn normalize_base_urls_rejects_overlong_url() {
    let url = format!(
        "https://example.com/{}",
        "a".repeat(MAX_PROVIDER_BASE_URL_CHARS)
    );
    let err = normalize_base_urls(vec![url]).expect_err("overlong url");
    assert!(err.to_string().contains("base_url must be at most"));
}

// -- base_urls_from_row --

#[test]
fn base_urls_from_row_parses_json_array() {
    let result = base_urls_from_row(
        "https://fallback.com",
        r#"["https://a.com","https://b.com"]"#,
    );
    assert_eq!(result, vec!["https://a.com", "https://b.com"]);
}

#[test]
fn base_urls_from_row_falls_back_to_base_url() {
    let result = base_urls_from_row("https://fallback.com", "[]");
    assert_eq!(result, vec!["https://fallback.com"]);
}

#[test]
fn base_urls_from_row_handles_invalid_json() {
    let result = base_urls_from_row("https://fallback.com", "not json");
    assert_eq!(result, vec!["https://fallback.com"]);
}

#[test]
fn base_urls_from_row_deduplicates() {
    let result = base_urls_from_row("", r#"["https://a.com","https://a.com","https://b.com"]"#);
    assert_eq!(result, vec!["https://a.com", "https://b.com"]);
}

#[test]
fn base_urls_from_row_returns_empty_vec_when_all_empty() {
    let result = base_urls_from_row("", "[]");
    assert!(result.is_empty());
}

// -- claude_models_from_json --

#[test]
fn claude_models_from_json_valid() {
    let models = claude_models_from_json(r#"{"main_model":"test-model"}"#);
    assert_eq!(models.main_model, Some("test-model".to_string()));
}

#[test]
fn claude_models_from_json_invalid_returns_default() {
    let models = claude_models_from_json("not json");
    assert!(!models.has_any());
}

#[test]
fn claude_models_from_json_empty_object() {
    let models = claude_models_from_json("{}");
    assert!(!models.has_any());
}

fn default_provider_params(name: &str) -> ProviderUpsertParams {
    ProviderUpsertParams {
        provider_id: None,
        cli_key: "claude".to_string(),
        name: name.to_string(),
        base_urls: vec!["https://api.example.com".to_string()],
        base_url_mode: ProviderBaseUrlMode::Order,
        auth_mode: Some(ProviderAuthMode::ApiKey),
        api_key: Some("sk-test".to_string()),
        enabled: true,
        cost_multiplier: 1.0,
        priority: Some(100),
        claude_models: None,
        model_mapping: None,
        availability_test_model: None,
        limit_5h_usd: None,
        limit_daily_usd: None,
        daily_reset_mode: Some(DailyResetMode::Fixed),
        daily_reset_time: Some("00:00:00".to_string()),
        limit_weekly_usd: None,
        limit_monthly_usd: None,
        limit_total_usd: None,
        tags: None,
        note: None,
        source_provider_id: None,
        bridge_type: None,
        stream_idle_timeout_seconds: None,
        extension_values: None,
        upstream_retry_policy_override: None,
        upstream_retry_policy_override_specified: false,
    }
}

#[test]
fn upsert_seeds_provider_account_usage_extension_owner_without_visible_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_account_usage_extension.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut params = default_provider_params("account-usage-extension-owner");
    params.extension_values = Some(vec![ProviderExtensionValuesInput {
        plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
        namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
        values: serde_json::json!({ "adapterKind": "sub2api" }),
    }]);

    let saved = upsert(&db, params).expect("save provider with account usage config");

    assert_eq!(saved.extension_values.len(), 1);
    assert_eq!(
        saved.extension_values[0].plugin_id,
        crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
    );
    assert_eq!(
        saved.extension_values[0].namespace,
        crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
    );
    assert_eq!(saved.extension_values[0].values["adapterKind"], "sub2api");

    let plugins = crate::infra::plugins::repository::list_plugins(&db).expect("list plugins");
    assert!(
        plugins.iter().all(|plugin| plugin.plugin_id
            != crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID),
        "internal owner must remain hidden from the visible plugin list"
    );
}

#[test]
fn upsert_accepts_unicode_note_at_character_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_note_limit.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut params = default_provider_params("unicode-note-limit");
    params.note = Some("注".repeat(MAX_PROVIDER_NOTE_CHARS));

    let saved = upsert(&db, params).expect("save provider");
    assert_eq!(saved.note.chars().count(), MAX_PROVIDER_NOTE_CHARS);
}

#[test]
fn upsert_rejects_unicode_note_over_character_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_note_over_limit.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut params = default_provider_params("unicode-note-over-limit");
    params.note = Some("注".repeat(MAX_PROVIDER_NOTE_CHARS + 1));

    let err = upsert(&db, params).expect_err("note over limit");
    assert!(err.to_string().contains("note must be at most"));
}

#[test]
fn upsert_oauth_provider_drops_submitted_base_urls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_oauth_base_urls.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let mut params = default_provider_params("oauth-drops-base-urls");
    params.auth_mode = Some(ProviderAuthMode::Oauth);
    params.api_key = None;
    params.base_urls = vec!["ftp://malicious.invalid".to_string()];

    let saved = upsert(&db, params).expect("save oauth provider");
    assert!(saved.base_urls.is_empty());
}

#[test]
fn invalid_retry_policy_override_json_disables_override_instead_of_inheriting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_invalid_retry_override.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let saved =
        upsert(&db, default_provider_params("invalid-retry-override")).expect("save provider");
    default_route_set_order(&db, "claude", vec![saved.id]).expect("set default route");
    {
        let conn = db.open_connection().expect("open db");
        conn.execute(
            "UPDATE providers SET upstream_retry_policy_json = ?1 WHERE id = ?2",
            rusqlite::params!["not json", saved.id],
        )
        .expect("seed invalid retry override");
    }

    let conn = db.open_connection().expect("open db");
    let summary = get_by_id(&conn, saved.id).expect("read provider");
    let override_policy = summary
        .upstream_retry_policy_override
        .expect("invalid override should remain explicit");
    assert!(!override_policy.enabled);
    drop(conn);

    let gateway_provider =
        list_enabled_for_gateway_using_active_mode(&db, "claude").expect("list gateway providers");
    let override_policy = gateway_provider.providers[0]
        .upstream_retry_policy_override
        .as_ref()
        .expect("gateway provider should keep explicit disabled override");
    assert!(!override_policy.enabled);
}

#[test]
fn reorder_rejects_invalid_duplicate_and_oversized_provider_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_reorder_bounds.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let saved = upsert(&db, default_provider_params("reorder-bound-p1")).expect("save provider");

    let invalid = reorder(&db, "claude", vec![saved.id, 0]).expect_err("invalid provider id");
    assert!(invalid.to_string().contains("invalid provider_id=0"));

    let duplicate =
        reorder(&db, "claude", vec![saved.id, saved.id]).expect_err("duplicate provider id");
    assert!(duplicate.to_string().contains("duplicate provider_id"));

    let oversized_ids: Vec<i64> = (1..=(MAX_PROVIDER_ORDER_IDS as i64 + 1)).collect();
    let oversized = reorder(&db, "claude", oversized_ids).expect_err("too many provider ids");
    assert!(oversized
        .to_string()
        .contains("ordered_provider_ids must contain at most"));
}

#[test]
fn pool_order_is_independent_from_default_route_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_pool_order.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let (p1_id, p2_id, p3_id) = {
        let p1 = upsert(&db, default_provider_params("pool-p1")).expect("save p1");
        let p2 = upsert(&db, default_provider_params("pool-p2")).expect("save p2");
        let p3 = upsert(&db, default_provider_params("pool-p3")).expect("save p3");
        (p1.id, p2.id, p3.id)
    };

    default_route_set_order(&db, "claude", vec![p1_id, p2_id]).expect("set default route");
    pool_order_set(&db, "claude", vec![p3_id, p1_id]).expect("set pool order");

    let pool_ids: Vec<i64> = list_by_cli(&db, "claude")
        .expect("list providers")
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(pool_ids, vec![p3_id, p1_id, p2_id]);

    let default_ids: Vec<i64> = default_route_list(&db, "claude")
        .expect("list default route")
        .into_iter()
        .map(|row| row.provider_id)
        .collect();
    assert_eq!(default_ids, vec![p1_id, p2_id]);
}

#[test]
fn default_route_gateway_uses_membership_and_global_enabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_default_route_gateway.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let (p1_id, p2_id, p3_enabled) = {
        let p1 = upsert(&db, default_provider_params("default-p1")).expect("save p1");
        let mut p2_params = default_provider_params("default-p2");
        p2_params.enabled = false;
        let p2 = upsert(&db, p2_params).expect("save p2");
        let p3 = upsert(&db, default_provider_params("default-p3")).expect("save p3");
        (p1.id, p2.id, p3.enabled)
    };

    default_route_set_order(&db, "claude", vec![p2_id, p1_id]).expect("set default route");

    let selection =
        list_enabled_for_gateway_using_active_mode(&db, "claude").expect("list gateway providers");
    assert_eq!(selection.sort_mode_id, None);
    assert_eq!(
        selection
            .providers
            .into_iter()
            .map(|provider| provider.id)
            .collect::<Vec<_>>(),
        vec![p1_id]
    );

    // p3 remains globally enabled but is not a Default member, so it is not routed.
    assert!(p3_enabled);
}

fn seed_usage_request_log(db: &crate::db::Db, trace_id: &str, provider_id: i64) {
    let conn = db.open_connection().expect("open db connection");
    conn.execute(
        r#"
INSERT INTO request_logs (
  trace_id, cli_key, method, path, duration_ms, attempts_json, created_at,
  input_tokens, output_tokens, total_tokens, excluded_from_stats, final_provider_id
) VALUES (?1, 'claude', 'POST', '/v1/messages', 12, '[]', 100, 10, 5, 15, 0, ?2)
"#,
        rusqlite::params![trace_id, provider_id],
    )
    .expect("insert request log");
}

fn request_log_exists(db: &crate::db::Db, trace_id: &str) -> bool {
    let conn = db.open_connection().expect("open db connection");
    conn.query_row(
        "SELECT 1 FROM request_logs WHERE trace_id = ?1",
        rusqlite::params![trace_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .expect("read request log")
    .is_some()
}

#[test]
fn delete_keeps_request_logs_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_delete_keep_logs.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let saved = upsert(&db, default_provider_params("delete-keep-logs")).expect("save provider");
    seed_usage_request_log(&db, "trace-delete-keep", saved.id);

    delete(&db, saved.id, false).expect("delete provider");

    assert!(request_log_exists(&db, "trace-delete-keep"));
}

#[test]
fn delete_removes_provider_request_logs_when_requested() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_delete_clear_logs.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let saved = upsert(&db, default_provider_params("delete-clear-logs")).expect("save provider");
    let other =
        upsert(&db, default_provider_params("delete-clear-other")).expect("save other provider");
    seed_usage_request_log(&db, "trace-delete-clear", saved.id);
    seed_usage_request_log(&db, "trace-delete-other", other.id);

    delete(&db, saved.id, true).expect("delete provider");

    assert!(!request_log_exists(&db, "trace-delete-clear"));
    assert!(request_log_exists(&db, "trace-delete-other"));
}

fn create_oauth_provider_for_cas_test(db: &crate::db::Db, name: &str) -> i64 {
    upsert(
        db,
        ProviderUpsertParams {
            provider_id: None,
            cli_key: "codex".to_string(),
            name: name.to_string(),
            base_urls: vec![],
            base_url_mode: ProviderBaseUrlMode::Order,
            auth_mode: Some(ProviderAuthMode::Oauth),
            api_key: None,
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(100),
            claude_models: None,
            availability_test_model: None,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: Some(DailyResetMode::Fixed),
            daily_reset_time: Some("00:00:00".to_string()),
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
            tags: None,
            note: None,
            source_provider_id: None,
            bridge_type: None,
            stream_idle_timeout_seconds: None,
            model_mapping: None,
            extension_values: None,
            upstream_retry_policy_override: None,
            upstream_retry_policy_override_specified: false,
        },
    )
    .expect("create oauth provider")
    .id
}

#[test]
fn update_oauth_tokens_cas_rejects_stale_writer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_oauth_cas_stale.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let provider_id = create_oauth_provider_for_cas_test(&db, "oauth-cas-stale");
    update_oauth_tokens(
        &db,
        provider_id,
        "oauth",
        "codex_oauth",
        "seed_access",
        Some("seed_refresh"),
        Some("seed_id"),
        "https://auth.openai.com/oauth/token",
        "client_seed",
        None,
        Some(2_000_000_000),
        Some("seed@example.com"),
    )
    .expect("seed oauth tokens");

    let details = get_oauth_details(&db, provider_id).expect("get oauth details");
    let expected_last_refreshed_at = details.oauth_last_refreshed_at;
    assert!(expected_last_refreshed_at.is_some());

    let first = update_oauth_tokens_if_last_refreshed_matches(
        &db,
        provider_id,
        "oauth",
        "codex_oauth",
        "access_first",
        Some("refresh_first"),
        Some("id_first"),
        "https://auth.openai.com/oauth/token",
        "client_first",
        None,
        Some(2_000_000_100),
        Some("first@example.com"),
        expected_last_refreshed_at,
    )
    .expect("first cas update");
    assert!(first);

    let second = update_oauth_tokens_if_last_refreshed_matches(
        &db,
        provider_id,
        "oauth",
        "codex_oauth",
        "access_second",
        Some("refresh_second"),
        Some("id_second"),
        "https://auth.openai.com/oauth/token",
        "client_second",
        None,
        Some(2_000_000_200),
        Some("second@example.com"),
        expected_last_refreshed_at,
    )
    .expect("second cas update");
    assert!(!second);

    let after = get_oauth_details(&db, provider_id).expect("get oauth details after cas");
    assert_eq!(after.oauth_access_token, "access_first");
    assert_eq!(after.oauth_refresh_token.as_deref(), Some("refresh_first"));
}

#[test]
fn update_oauth_tokens_cas_allows_initial_null_then_blocks_repeat_null() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("providers_oauth_cas_null.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let provider_id = create_oauth_provider_for_cas_test(&db, "oauth-cas-null");
    let details = get_oauth_details(&db, provider_id).expect("get oauth details");
    assert_eq!(details.oauth_last_refreshed_at, None);

    let first = update_oauth_tokens_if_last_refreshed_matches(
        &db,
        provider_id,
        "oauth",
        "codex_oauth",
        "null_first_access",
        Some("null_first_refresh"),
        Some("null_first_id"),
        "https://auth.openai.com/oauth/token",
        "null_first_client",
        None,
        Some(2_000_000_300),
        Some("nullfirst@example.com"),
        None,
    )
    .expect("first cas from null");
    assert!(first);

    let second = update_oauth_tokens_if_last_refreshed_matches(
        &db,
        provider_id,
        "oauth",
        "codex_oauth",
        "null_second_access",
        Some("null_second_refresh"),
        Some("null_second_id"),
        "https://auth.openai.com/oauth/token",
        "null_second_client",
        None,
        Some(2_000_000_400),
        Some("nullsecond@example.com"),
        None,
    )
    .expect("second cas from null");
    assert!(!second);

    let after = get_oauth_details(&db, provider_id).expect("get oauth details after null cas");
    assert_eq!(after.oauth_access_token, "null_first_access");
    assert!(after.oauth_last_refreshed_at.is_some());
}
