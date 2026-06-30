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
fn codex_bridge_types_roundtrip_and_require_explicit_identity() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let codex_source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("codex", "Codex source", "https://upstream.example/v1"),
    )
    .expect("insert codex source");
    let codex_source_id = json_i64(&codex_source, "id");
    let claude_source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("claude", "Claude source", "https://api.anthropic.com/v1"),
    )
    .expect("insert claude source");
    let claude_source_id = json_i64(&claude_source, "id");

    let chat_bridge = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Chat bridge", "")
            },
            source_provider_id: Some(codex_source_id),
            bridge_type: Some("codex_to_openai_chat".to_string()),
        },
    )
    .expect("insert chat bridge provider");

    assert_eq!(json_str(&chat_bridge, "cli_key"), "codex");
    assert_eq!(
        json_i64(&chat_bridge, "source_provider_id"),
        codex_source_id
    );
    assert_eq!(
        json_str(&chat_bridge, "bridge_type"),
        "codex_to_openai_chat"
    );
    assert_eq!(json_array(chat_bridge["base_urls"].clone()).len(), 0);
    let chat_bridge_id = json_i64(&chat_bridge, "id");

    let anthropic_bridge = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Anthropic bridge", "")
            },
            source_provider_id: Some(claude_source_id),
            bridge_type: Some("codex_to_anthropic_messages".to_string()),
        },
    )
    .expect("insert anthropic bridge provider");

    assert_eq!(json_str(&anthropic_bridge, "cli_key"), "codex");
    assert_eq!(
        json_i64(&anthropic_bridge, "source_provider_id"),
        claude_source_id
    );
    assert_eq!(
        json_str(&anthropic_bridge, "bridge_type"),
        "codex_to_anthropic_messages"
    );
    assert_eq!(json_array(anthropic_bridge["base_urls"].clone()).len(), 0);
    let anthropic_bridge_id = json_i64(&anthropic_bridge, "id");

    let db_path = aio_coding_hub_lib::test_support::db_path(&handle).expect("db path");
    let conn = rusqlite::Connection::open(db_path).expect("open db");
    let chat_bridge_key: String = conn
        .query_row(
            "SELECT api_key_plaintext FROM providers WHERE id = ?1",
            rusqlite::params![chat_bridge_id],
            |row| row.get(0),
        )
        .expect("query chat bridge api key");
    let anthropic_bridge_key: String = conn
        .query_row(
            "SELECT api_key_plaintext FROM providers WHERE id = ?1",
            rusqlite::params![anthropic_bridge_id],
            |row| row.get(0),
        )
        .expect("query anthropic bridge api key");
    assert_eq!(chat_bridge_key, "");
    assert_eq!(anthropic_bridge_key, "");

    let list = aio_coding_hub_lib::test_support::providers_list_by_cli_json(&handle, "codex")
        .expect("list codex providers");
    let list = json_array(list);
    assert_eq!(list.len(), 3);
    assert_eq!(json_str(&list[1], "bridge_type"), "codex_to_openai_chat");
    assert_eq!(
        json_str(&list[2], "bridge_type"),
        "codex_to_anthropic_messages"
    );
}

#[test]
fn editing_regular_provider_into_codex_bridge_clears_own_transport_credentials() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let codex_source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("codex", "Codex source", "https://upstream.example/v1"),
    )
    .expect("insert codex source");
    let codex_source_id = json_i64(&codex_source, "id");

    let regular = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("codex", "Becomes bridge", "https://stale.example/v1"),
    )
    .expect("insert regular codex provider");
    let regular_id = json_i64(&regular, "id");

    let updated = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                provider_id: Some(regular_id),
                api_key: None,
                ..provider_input("codex", "Becomes bridge", "")
            },
            source_provider_id: Some(codex_source_id),
            bridge_type: Some("codex_to_openai_chat".to_string()),
        },
    )
    .expect("convert regular provider to codex chat bridge");

    assert_eq!(json_array(updated["base_urls"].clone()).len(), 0);
    assert_eq!(json_str(&updated, "bridge_type"), "codex_to_openai_chat");
    assert_eq!(json_i64(&updated, "source_provider_id"), codex_source_id);

    let db_path = aio_coding_hub_lib::test_support::db_path(&handle).expect("db path");
    let conn = rusqlite::Connection::open(db_path).expect("open db");
    let own_api_key: String = conn
        .query_row(
            "SELECT api_key_plaintext FROM providers WHERE id = ?1",
            rusqlite::params![regular_id],
            |row| row.get(0),
        )
        .expect("query provider api key");
    assert_eq!(own_api_key, "");
}

#[test]
fn codex_bridge_source_cli_must_match_target_endpoint() {
    let app = support::TestApp::new();
    let handle = app.handle();

    let codex_source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("codex", "Codex source", "https://upstream.example/v1"),
    )
    .expect("insert codex source");
    let codex_source_id = json_i64(&codex_source, "id");
    let claude_source = aio_coding_hub_lib::test_support::provider_upsert_json(
        &handle,
        provider_input("claude", "Claude source", "https://api.anthropic.com/v1"),
    )
    .expect("insert claude source");
    let claude_source_id = json_i64(&claude_source, "id");

    let err = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Bad chat bridge", "")
            },
            source_provider_id: Some(claude_source_id),
            bridge_type: Some("codex_to_openai_chat".to_string()),
        },
    )
    .expect_err("chat bridge must reject claude source");
    assert!(
        err.to_string()
            .contains("source provider must belong to codex CLI"),
        "unexpected error: {err}"
    );

    let err = aio_coding_hub_lib::test_support::provider_upsert_bridge_json(
        &handle,
        ProviderUpsertBridgeJsonInput {
            base: ProviderUpsertJsonInput {
                api_key: None,
                ..provider_input("codex", "Bad anthropic bridge", "")
            },
            source_provider_id: Some(codex_source_id),
            bridge_type: Some("codex_to_anthropic_messages".to_string()),
        },
    )
    .expect_err("anthropic messages bridge must reject codex source");
    assert!(
        err.to_string()
            .contains("source provider must belong to claude CLI"),
        "unexpected error: {err}"
    );
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
