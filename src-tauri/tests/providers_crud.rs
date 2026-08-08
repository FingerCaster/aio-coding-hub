mod support;

use aio_coding_hub_lib::test_support::{ProviderUpsertBridgeJsonInput, ProviderUpsertJsonInput};
use support::{json_array, json_bool, json_f64, json_i64, json_str};

fn provider_input(cli_key: &str, name: &str, base_url: &str) -> ProviderUpsertJsonInput {
    ProviderUpsertJsonInput {
        provider_id: None,
        cli_key: cli_key.to_string(),
        name: name.to_string(),
        base_urls: if base_url.is_empty() {
            Vec::new()
        } else {
            vec![base_url.to_string()]
        },
        base_url_mode: "order".to_string(),
        api_key: Some("test-key".to_string()),
        enabled: true,
        cost_multiplier: 1.0,
        priority: Some(100),
        claude_models: None,
        limit_5h_usd: None,
        limit_daily_usd: None,
        daily_reset_mode: None,
        daily_reset_time: None,
        limit_weekly_usd: None,
        limit_monthly_usd: None,
        limit_total_usd: None,
    }
}

#[test]
fn claude_model_overrides_reject_over_length_values_on_create_and_update() {
    let app = support::TestApp::new();
    let handle = app.handle();
    let too_long_model = "x".repeat(201);

    let mut create_input = provider_input("claude", "Too long create", "https://api.anthropic.com");
    create_input.claude_models = Some(serde_json::json!({
        "main_model": too_long_model,
    }));
    let err = aio_coding_hub_lib::test_support::provider_upsert_json(&handle, create_input)
        .expect_err("create should reject over-length Claude model override")
        .to_string();
    assert!(
        err.contains("SEC_INVALID_INPUT") && err.contains("main_model"),
        "unexpected error: {err}"
    );

    let provider = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("claude", "Valid provider", "https://api.anthropic.com"),
    )
    .expect("insert valid provider");
    let mut update_input = provider_input("claude", "Valid provider", "https://api.anthropic.com");
    update_input.provider_id = Some(json_i64(&provider, "id"));
    update_input.api_key = None;
    update_input.claude_models = Some(serde_json::json!({
        "reasoning_model": "模".repeat(201),
    }));

    let err = aio_coding_hub_lib::test_support::provider_upsert_json(&handle, update_input)
        .expect_err("update should reject over-length Claude model override")
        .to_string();
    assert!(
        err.contains("SEC_INVALID_INPUT") && err.contains("reasoning_model"),
        "unexpected error: {err}"
    );
}

#[test]
fn providers_crud_roundtrip() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let list = aio_coding_hub_lib::test_support::providers_list_by_cli_json(&handle, "claude")
        .expect("list claude providers");
    assert_eq!(json_array(list).len(), 0);

    let p1 = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        ProviderUpsertJsonInput {
            provider_id: None,
            cli_key: "claude".to_string(),
            name: "P1".to_string(),
            base_urls: vec!["https://api.anthropic.com".to_string()],
            base_url_mode: "order".to_string(),
            api_key: Some("k1".to_string()),
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(100),
            claude_models: None,
            limit_5h_usd: Some(5.0),
            limit_daily_usd: Some(100.0),
            daily_reset_mode: Some("fixed".to_string()),
            daily_reset_time: Some("01:02:03".to_string()),
            limit_weekly_usd: Some(300.0),
            limit_monthly_usd: Some(1000.0),
            limit_total_usd: Some(10000.0),
        },
    )
    .expect("insert provider 1");

    let p2 = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        ProviderUpsertJsonInput {
            provider_id: None,
            cli_key: "claude".to_string(),
            name: "P2".to_string(),
            base_urls: vec![
                "https://api.anthropic.com".to_string(),
                "https://api.anthropic.com/v2".to_string(),
            ],
            base_url_mode: "ping".to_string(),
            api_key: Some("k2".to_string()),
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(100),
            claude_models: None,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: None,
            daily_reset_time: None,
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
        },
    )
    .expect("insert provider 2");

    assert_eq!(json_str(&p1, "cli_key"), "claude");
    assert_eq!(json_str(&p2, "cli_key"), "claude");

    let id1 = json_i64(&p1, "id");
    let id2 = json_i64(&p2, "id");
    assert!(id1 > 0);
    assert!(id2 > 0);

    assert_eq!(json_str(&p1, "daily_reset_mode"), "fixed");
    assert_eq!(json_str(&p1, "daily_reset_time"), "01:02:03");
    assert_eq!(json_f64(&p1, "limit_5h_usd"), Some(5.0));
    assert_eq!(json_f64(&p1, "limit_daily_usd"), Some(100.0));
    assert_eq!(json_f64(&p1, "limit_weekly_usd"), Some(300.0));
    assert_eq!(json_f64(&p1, "limit_monthly_usd"), Some(1000.0));
    assert_eq!(json_f64(&p1, "limit_total_usd"), Some(10000.0));

    let list = aio_coding_hub_lib::test_support::providers_list_by_cli_json(&handle, "claude")
        .expect("list providers after insert");
    let list = json_array(list);
    assert_eq!(list.len(), 2);
    assert_eq!(json_str(&list[0], "name"), "P1");
    assert_eq!(json_str(&list[1], "name"), "P2");

    let updated = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        ProviderUpsertJsonInput {
            provider_id: Some(id1),
            cli_key: "claude".to_string(),
            name: "P1-renamed".to_string(),
            base_urls: vec!["https://api.anthropic.com".to_string()],
            base_url_mode: "order".to_string(),
            api_key: None,
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(101),
            claude_models: None,
            limit_5h_usd: Some(5.0),
            limit_daily_usd: Some(100.0),
            daily_reset_mode: Some("fixed".to_string()),
            daily_reset_time: Some("01:02:03".to_string()),
            limit_weekly_usd: Some(300.0),
            limit_monthly_usd: Some(1000.0),
            limit_total_usd: Some(10000.0),
        },
    )
    .expect("update provider 1");
    assert_eq!(json_str(&updated, "name"), "P1-renamed");

    let updated = aio_coding_hub_lib::test_support::provider_set_enabled_json(&handle, id1, false)
        .expect("disable provider 1");
    assert_eq!(json_i64(&updated, "id"), id1);
    assert!(!json_bool(&updated, "enabled"));

    let reordered =
        aio_coding_hub_lib::test_support::providers_reorder_json(&handle, "claude", vec![id2, id1])
            .expect("reorder providers");
    let reordered = json_array(reordered);
    assert_eq!(json_i64(&reordered[0], "id"), id2);

    assert!(
        aio_coding_hub_lib::test_support::provider_delete(&handle, id1).expect("delete provider")
    );

    let list = aio_coding_hub_lib::test_support::providers_list_by_cli_json(&handle, "claude")
        .expect("list providers after delete");
    assert_eq!(json_array(list).len(), 1);

    let err =
        aio_coding_hub_lib::test_support::providers_reorder_json(&handle, "claude", vec![id2, id2])
            .expect_err("duplicate reorder should fail");
    let err = err.to_string();
    assert!(
        err.contains("duplicate provider_id"),
        "unexpected error: {err}"
    );
}

#[test]
fn legacy_codex_translation_bridge_types_are_rejected() {
    let app = support::TestApp::new();
    let handle = app.handle();

    for bridge_type in [
        "codex_to_openai_chat",
        "codex_to_openai_responses",
        "codex_to_anthropic_messages",
    ] {
        let err = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
            &handle,
            ProviderUpsertBridgeJsonInput {
                base: ProviderUpsertJsonInput {
                    api_key: None,
                    ..provider_input("codex", "Removed translation bridge", "")
                },
                source_provider_id: None,
                bridge_type: Some(bridge_type.to_string()),
            },
        )
        .expect_err("legacy Codex translation bridge must be rejected");
        let err = err.to_string();
        assert!(
            err.contains(&format!("unsupported bridge_type: {bridge_type}")),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn source_provider_id_without_bridge_type_is_rejected() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("codex", "Codex source", "https://upstream.example/v1"),
    )
    .expect("insert codex source");
    let source_id = json_i64(&source, "id");

    let err = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Legacy implicit bridge", "")
            },
            source_provider_id: Some(source_id),
            bridge_type: None,
        },
    )
    .expect_err("source_provider_id without bridge_type should fail");
    let err = err.to_string();
    assert!(
        err.contains("bridge_type is required when source_provider_id is set"),
        "unexpected error: {err}"
    );
}

#[test]
fn unsupported_bridge_type_is_rejected() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let err = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Unknown bridge", "")
            },
            source_provider_id: None,
            bridge_type: Some("unknown_bridge".to_string()),
        },
    )
    .expect_err("unknown bridge_type should fail");
    let err = err.to_string();
    assert!(
        err.contains("unsupported bridge_type: unknown_bridge"),
        "unexpected error: {err}"
    );
}

#[test]
fn cx2cc_still_roundtrips_with_explicit_bridge_type() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let bridge = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("claude", "CX2CC", "")
            },
            source_provider_id: None,
            bridge_type: Some("cx2cc".to_string()),
        },
    )
    .expect("insert explicit cx2cc bridge provider");

    assert_eq!(json_str(&bridge, "cli_key"), "claude");
    assert_eq!(json_str(&bridge, "bridge_type"), "cx2cc");
    assert!(bridge["source_provider_id"].is_null());
    assert_eq!(json_array(bridge["base_urls"].clone()).len(), 0);
}
