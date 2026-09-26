pub(crate) fn is_bridged_input_semantics(
    source_provider_id: Option<i64>,
    bridge_type: Option<&str>,
) -> bool {
    crate::providers::has_bridged_input_semantics(source_provider_id, bridge_type)
}

fn uses_openai_input_semantics(
    cli_key: &str,
    persisted_openai_semantics: Option<bool>,
    legacy_provider_bridged: bool,
) -> bool {
    if matches!(cli_key, "codex" | "grok") {
        return true;
    }
    if let Some(openai_semantics) = persisted_openai_semantics {
        return openai_semantics;
    }
    legacy_provider_bridged
}

/// Single Rust owner for mutually-exclusive input token buckets.
/// OpenAI input includes cache reads and writes; Gemini only includes reads;
/// Claude and other protocols report cache buckets additively.
pub(crate) fn effective_input_tokens(
    cli_key: &str,
    persisted_openai_semantics: Option<bool>,
    legacy_provider_bridged: bool,
    input_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
) -> i64 {
    let input = input_tokens.unwrap_or(0);
    if uses_openai_input_semantics(cli_key, persisted_openai_semantics, legacy_provider_bridged) {
        input
            .saturating_sub(cache_read_input_tokens.unwrap_or(0))
            .saturating_sub(cache_creation_input_tokens.unwrap_or(0))
            .max(0)
    } else if cli_key == "gemini" {
        input
            .saturating_sub(cache_read_input_tokens.unwrap_or(0))
            .max(0)
    } else {
        input
    }
}

/// Display variant of [`effective_input_tokens`]: keeps "usage unknown"
/// (`input_tokens: None`) as `None` instead of collapsing it to 0, so rows and
/// events without usage render as "—" rather than "0". Aggregates keep the
/// COALESCE(..., 0) semantics of the SQL expression.
pub(crate) fn effective_input_tokens_display(
    cli_key: &str,
    persisted_openai_semantics: Option<bool>,
    legacy_provider_bridged: bool,
    input_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
) -> Option<i64> {
    input_tokens.is_some().then(|| {
        effective_input_tokens(
            cli_key,
            persisted_openai_semantics,
            legacy_provider_bridged,
            input_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        )
    })
}

pub(super) fn effective_total_from_buckets(
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_input_tokens: i64,
    cache_read_input_tokens: i64,
) -> i64 {
    input_tokens
        .saturating_add(output_tokens)
        .saturating_add(cache_creation_input_tokens)
        .saturating_add(cache_read_input_tokens)
}

fn sql_column(alias: Option<&str>, column: &str) -> String {
    alias
        .map(|alias| format!("{alias}.{column}"))
        .unwrap_or_else(|| column.to_string())
}

fn build_sql_effective_input_tokens_expr(alias: Option<&str>) -> String {
    let cli_key = sql_column(alias, "cli_key");
    let final_provider_id = sql_column(alias, "final_provider_id");
    let special_settings_json = sql_column(alias, "special_settings_json");
    let input_tokens = sql_column(alias, "input_tokens");
    let cache_read_input_tokens = sql_column(alias, "cache_read_input_tokens");
    let cache_creation_input_tokens = sql_column(alias, "cache_creation_input_tokens");

    let safe_settings_json = format!(
        "CASE WHEN json_valid({special_settings_json}) THEN CASE WHEN json_type({special_settings_json}) = 'array' THEN {special_settings_json} ELSE '[]' END ELSE '[]' END"
    );
    let trimmed_source_cli_key = "TRIM(json_extract(marker.value, '$.source_cli_key'), char(32) || char(9) || char(10) || char(13))";
    let valid_marker_fields = format!(
        "json_extract(marker.value, '$.type') = 'cx2cc_cost_basis' AND json_type(marker.value, '$.source_cli_key') = 'text' AND {trimmed_source_cli_key} != ''"
    );
    let scoped_marker_condition = format!(
        "CASE WHEN marker.type = 'object' THEN {valid_marker_fields} AND json_type(marker.value, '$.bridge_provider_id') = 'integer' AND typeof(json_extract(marker.value, '$.bridge_provider_id')) = 'integer' AND json_extract(marker.value, '$.bridge_provider_id') > 0 ELSE 0 END"
    );
    let legacy_marker_condition = format!(
        "CASE WHEN marker.type = 'object' THEN {valid_marker_fields} AND json_type(marker.value, '$.bridge_provider_id') IS NULL ELSE 0 END"
    );
    let marker_semantics =
        format!("CASE WHEN {trimmed_source_cli_key} = 'codex' THEN 1 ELSE 0 END");
    let packed_marker_semantics = format!("(CAST(marker.key AS INTEGER) * 2 + {marker_semantics})");
    let persisted_marker_semantics = format!(
        "(SELECT CASE WHEN resolved.exact_match IS NOT NULL THEN resolved.exact_match % 2 WHEN resolved.has_scoped = 1 THEN 0 WHEN resolved.legacy_match IS NOT NULL THEN resolved.legacy_match % 2 ELSE NULL END FROM (SELECT MAX(CASE WHEN {scoped_marker_condition} AND json_extract(marker.value, '$.bridge_provider_id') = {final_provider_id} THEN {packed_marker_semantics} END) AS exact_match, MAX(CASE WHEN {scoped_marker_condition} THEN 1 ELSE 0 END) AS has_scoped, MAX(CASE WHEN {legacy_marker_condition} THEN {packed_marker_semantics} END) AS legacy_match FROM json_each({safe_settings_json}) marker) resolved)"
    );
    let legacy_provider_semantics = format!(
        "CASE WHEN EXISTS (SELECT 1 FROM providers p WHERE p.id = {final_provider_id} AND (p.source_provider_id IS NOT NULL OR p.bridge_type = 'cx2cc')) THEN 1 ELSE 0 END"
    );
    let openai_semantics = format!(
        "({cli_key} IN ('codex', 'grok') OR COALESCE({persisted_marker_semantics}, {legacy_provider_semantics}) = 1)"
    );

    format!(
        "CASE WHEN {openai_semantics} THEN MAX(COALESCE({input_tokens}, 0) - COALESCE({cache_read_input_tokens}, 0) - COALESCE({cache_creation_input_tokens}, 0), 0) WHEN {cli_key} = 'gemini' THEN MAX(COALESCE({input_tokens}, 0) - COALESCE({cache_read_input_tokens}, 0), 0) ELSE COALESCE({input_tokens}, 0) END"
    )
}

pub(super) fn sql_effective_input_tokens_expr() -> String {
    build_sql_effective_input_tokens_expr(None)
}

pub(super) fn sql_effective_input_tokens_expr_with_alias(alias: &str) -> String {
    build_sql_effective_input_tokens_expr(Some(alias))
}

pub(super) fn sql_effective_total_tokens_expr() -> String {
    let effective_input_expr = sql_effective_input_tokens_expr();
    format!(
        "({effective_input_expr}) + COALESCE(output_tokens, 0) + COALESCE(cache_creation_input_tokens, 0) + COALESCE(cache_read_input_tokens, 0)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_protocol_json_and_sse_usage_reconcile_with_sql_and_cost() {
        use crate::domain::{cost, usage};
        use crate::shared::gateway_protocol::GatewayProtocol as P;
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE providers (id INTEGER PRIMARY KEY, source_provider_id INTEGER, bridge_type TEXT);
            CREATE TABLE r (cli_key TEXT, final_provider_id INTEGER, special_settings_json TEXT, input_tokens INTEGER, output_tokens INTEGER, cache_read_input_tokens INTEGER, cache_creation_input_tokens INTEGER);").unwrap();
        let cases = [
            (
                P::AnthropicMessages,
                "claude",
                serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":70,"output_tokens":30,"total_tokens":130,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}}),
                70,
                70,
                10,
            ),
            (
                P::OpenaiCompletions,
                "grok",
                serde_json::json!({"usage":{"prompt_tokens":100,"completion_tokens":30,"total_tokens":130,"prompt_tokens_details":{"cached_tokens":20},"cache_creation_input_tokens":10}}),
                100,
                70,
                10,
            ),
            (
                P::OpenaiResponses,
                "codex",
                serde_json::json!({"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":30,"total_tokens":130,"input_tokens_details":{"cached_tokens":20},"cache_creation_input_tokens":10}}}),
                100,
                70,
                10,
            ),
            (
                P::GoogleGenerativeAi,
                "gemini",
                serde_json::json!({"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":30,"totalTokenCount":130,"cachedContentTokenCount":20}}),
                100,
                80,
                0,
            ),
        ];
        let prices = r#"{"input_cost_per_token":0.000001,"output_cost_per_token":0.000002,"cache_read_input_token_cost":0.0000005,"cache_creation_input_token_cost":0.0000015}"#;
        for (protocol, legacy_cli, wire, raw_input, exclusive_input, cache_write) in cases {
            // Anthropic's JSON response is the message itself; only SSE
            // message_start wraps that message in a separate event envelope.
            let json = if protocol == P::AnthropicMessages {
                wire["message"].to_string()
            } else {
                wire.to_string()
            };
            let sse = format!("data: {wire}\n\n");
            let json_usage = usage::parse_usage_for_protocol(protocol, json.as_bytes())
                .unwrap_or_else(|| panic!("missing JSON usage for {protocol:?}: {json}"));
            let mut tracker = usage::SseUsageTracker::for_protocol(protocol);
            for chunk in sse.as_bytes().chunks(7) {
                tracker.ingest_chunk(chunk);
            }
            let stream_usage = tracker.finalize().unwrap();
            assert_eq!(
                tracker.finalize().unwrap().metrics.input_tokens,
                Some(exclusive_input),
                "repeated finalize: {protocol:?}"
            );
            let reparsed = usage::parse_usage_for_protocol(protocol, sse.as_bytes()).unwrap();
            for extract in [&json_usage, &stream_usage, &reparsed] {
                let m = &extract.metrics;
                assert_eq!(m.input_tokens, Some(exclusive_input), "{protocol:?}");
                assert_eq!(m.total_tokens, Some(130));
                assert_eq!(m.cache_read_input_tokens, Some(20));
                assert_eq!(m.cache_creation_input_tokens.unwrap_or(0), cache_write);
                let audit: serde_json::Value = serde_json::from_str(&extract.usage_json).unwrap();
                assert_eq!(
                    audit["input_tokens"], raw_input,
                    "wire input must remain auditable"
                );
                for cli in ["pi", "omp"] {
                    let marker = serde_json::json!([{"type":"gateway_protocol","protocol":protocol.as_str(),"sourceCli":cli,"input_semantics":"exclusive","wire_input_tokens":raw_input}]).to_string();
                    conn.execute("DELETE FROM r", []).unwrap();
                    conn.execute(
                        "INSERT INTO r VALUES (?1,NULL,?2,?3,?4,?5,?6)",
                        rusqlite::params![
                            cli,
                            marker,
                            m.input_tokens,
                            m.output_tokens,
                            m.cache_read_input_tokens,
                            m.cache_creation_input_tokens
                        ],
                    )
                    .unwrap();
                    let query = format!(
                        "SELECT cli_key, {}, {} FROM r",
                        sql_effective_input_tokens_expr(),
                        sql_effective_total_tokens_expr()
                    );
                    let result: (String, i64, i64) = conn
                        .query_row(&query, [], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .unwrap();
                    assert_eq!(result, (cli.into(), exclusive_input, 130));
                    let buckets = cost::CostUsage {
                        input_tokens: m.input_tokens.unwrap(),
                        output_tokens: m.output_tokens.unwrap(),
                        cache_read_input_tokens: m.cache_read_input_tokens.unwrap(),
                        cache_creation_input_tokens: m.cache_creation_input_tokens.unwrap_or(0),
                        ..Default::default()
                    };
                    let expected = exclusive_input * 1_000_000_000
                        + 30 * 2_000_000_000
                        + 20 * 500_000_000
                        + cache_write * 1_500_000_000;
                    assert_eq!(
                        cost::calculate_cost_usd_femto(&buckets, prices, 1.0, cli, "w0-model"),
                        Some(expected)
                    );
                }
            }
            // Existing clients still store their original wire input and subtract
            // cache at their established aggregation/billing boundary.
            let legacy = usage::parse_usage_from_json_bytes(legacy_cli, json.as_bytes())
                .unwrap()
                .metrics;
            assert_eq!(legacy.input_tokens, Some(raw_input));
            assert_eq!(
                effective_input_tokens(
                    legacy_cli,
                    None,
                    false,
                    legacy.input_tokens,
                    legacy.cache_read_input_tokens,
                    legacy.cache_creation_input_tokens
                ),
                exclusive_input
            );
            let buckets = cost::CostUsage {
                input_tokens: raw_input,
                output_tokens: 30,
                cache_read_input_tokens: 20,
                cache_creation_input_tokens: cache_write,
                ..Default::default()
            };
            let expected = exclusive_input * 1_000_000_000
                + 30 * 2_000_000_000
                + 20 * 500_000_000
                + cache_write * 1_500_000_000;
            assert_eq!(
                cost::calculate_cost_usd_femto(&buckets, prices, 1.0, legacy_cli, "w0-model"),
                Some(expected)
            );
        }
    }

    #[test]
    fn native_protocol_normalization_preserves_unknown_input_and_clamps_underflow() {
        use crate::domain::usage;
        use crate::shared::gateway_protocol::GatewayProtocol as P;
        for protocol in [
            P::AnthropicMessages,
            P::OpenaiCompletions,
            P::OpenaiResponses,
            P::GoogleGenerativeAi,
        ] {
            let raw = br#"{"usage":{"output_tokens":3,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}"#;
            assert_eq!(
                usage::parse_usage_for_protocol(protocol, raw)
                    .unwrap()
                    .metrics
                    .input_tokens,
                None
            );
            let mut tracker = usage::SseUsageTracker::for_protocol(protocol);
            tracker.ingest_chunk(
                format!("data: {}\n\n", std::str::from_utf8(raw).unwrap()).as_bytes(),
            );
            assert_eq!(tracker.finalize().unwrap().metrics.input_tokens, None);
            assert_eq!(tracker.finalize().unwrap().metrics.input_tokens, None);
        }
        for protocol in [
            P::OpenaiCompletions,
            P::OpenaiResponses,
            P::GoogleGenerativeAi,
        ] {
            let raw = br#"{"usage":{"input_tokens":1,"output_tokens":3,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}"#;
            assert_eq!(
                usage::parse_usage_for_protocol(protocol, raw)
                    .unwrap()
                    .metrics
                    .input_tokens,
                Some(0)
            );
        }
    }

    #[test]
    fn effective_input_tokens_uses_protocol_specific_buckets_and_preserves_unknown() {
        assert_eq!(
            effective_input_tokens_display("claude", None, false, None, None, None),
            None
        );
        assert_eq!(
            effective_input_tokens_display("codex", None, false, None, Some(100), Some(200)),
            None
        );
        assert_eq!(
            effective_input_tokens_display("codex", None, false, Some(1000), Some(100), Some(200)),
            Some(700)
        );
        assert_eq!(
            effective_input_tokens_display("grok", None, false, Some(1000), Some(100), Some(200)),
            Some(700)
        );
        assert_eq!(
            effective_input_tokens_display(
                "claude",
                Some(true),
                false,
                Some(1000),
                Some(100),
                Some(200)
            ),
            Some(700)
        );
        assert_eq!(
            effective_input_tokens_display("gemini", None, false, Some(1000), Some(100), Some(200)),
            Some(900)
        );
        assert_eq!(
            effective_input_tokens_display("claude", None, false, Some(1000), Some(100), Some(200)),
            Some(1000)
        );
        assert_eq!(
            effective_input_tokens_display(
                "claude",
                Some(false),
                true,
                Some(1000),
                Some(100),
                Some(200)
            ),
            Some(1000),
            "a persisted non-Codex marker must block provider fallback"
        );
        assert_eq!(
            effective_input_tokens_display("codex", None, false, Some(100), Some(80), Some(50)),
            Some(0)
        );
    }

    #[test]
    fn effective_input_tokens_matches_plain_and_aliased_sql_expressions() {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            r#"
CREATE TABLE providers (id INTEGER PRIMARY KEY, source_provider_id INTEGER, bridge_type TEXT);
INSERT INTO providers VALUES (1, NULL, NULL);
INSERT INTO providers VALUES (2, 99, NULL);
INSERT INTO providers VALUES (3, NULL, 'cx2cc');
INSERT INTO providers VALUES (4, 99, 'cx2cc');
CREATE TABLE r (
  cli_key TEXT,
  final_provider_id INTEGER,
  special_settings_json TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER
);
"#,
        )
        .expect("create schema");

        let provider_cases: [(i64, Option<i64>, Option<&str>); 5] = [
            (1, None, None),
            (2, Some(99), None),
            (3, None, Some("cx2cc")),
            (4, Some(99), Some("cx2cc")),
            // provider id 5 does not exist in the providers table at all
            (5, None, None),
        ];
        let marker_cases: [Option<&str>; 14] = [
            None,
            Some(r#"[{"type":"cx2cc_cost_basis","source_cli_key":"codex"}]"#),
            Some(r#"[{"type":"cx2cc_cost_basis","source_cli_key":"claude"}]"#),
            Some(r#"[{"type":"cx2cc_cost_basis","source_cli_key":"\tcodex\t"}]"#),
            Some(r#"[{"type":"cx2cc_cost_basis","source_cli_key":"\r\ncodex\n"}]"#),
            Some(
                r#"[{"type":"cx2cc_cost_basis","bridge_provider_id":2,"source_cli_key":"codex"}]"#,
            ),
            Some(
                r#"[{"type":"cx2cc_cost_basis","bridge_provider_id":99,"source_cli_key":"codex"}]"#,
            ),
            Some("not-json"),
            Some(r#"[1,"text",false,null]"#),
            Some(r#"[{"type":"cx2cc_cost_basis"}]"#),
            Some(r#"[1,"text",{"type":"cx2cc_cost_basis","source_cli_key":"codex"},false]"#),
            Some(
                r#"[{"type":"cx2cc_cost_basis","source_cli_key":"codex"},{"type":"cx2cc_cost_basis","source_cli_key":""}]"#,
            ),
            Some(
                r#"[{"type":"cx2cc_cost_basis","bridge_provider_id":null,"source_cli_key":"codex"}]"#,
            ),
            Some(
                r#"[{"type":"cx2cc_cost_basis","bridge_provider_id":9223372036854775808,"source_cli_key":"codex"}]"#,
            ),
        ];
        let token_cases: [(Option<i64>, Option<i64>, Option<i64>); 5] = [
            (None, None, None),
            (Some(1200), None, None),
            (Some(1000), Some(100), Some(200)),
            (Some(100), Some(80), Some(50)),
            (Some(0), Some(0), Some(0)),
        ];

        let plain_sql = format!("SELECT {} FROM r", sql_effective_input_tokens_expr());
        let aliased_sql = format!(
            "SELECT {} FROM r",
            sql_effective_input_tokens_expr_with_alias("r")
        );
        for cli_key in ["claude", "codex", "gemini", "grok"] {
            for (provider_id, source_provider_id, bridge_type) in provider_cases {
                for special_settings_json in marker_cases {
                    for (input, cache_read, cache_creation) in token_cases {
                        conn.execute("DELETE FROM r", []).expect("clear r");
                        conn.execute(
                            "INSERT INTO r VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
                            rusqlite::params![
                                cli_key,
                                provider_id,
                                special_settings_json,
                                input,
                                cache_read,
                                cache_creation
                            ],
                        )
                        .expect("insert row");

                        let plain_value: i64 = conn
                            .query_row(&plain_sql, [], |row| row.get(0))
                            .expect("evaluate plain SQL expression");
                        let aliased_value: i64 = conn
                            .query_row(&aliased_sql, [], |row| row.get(0))
                            .expect("evaluate aliased SQL expression");
                        let bridged = is_bridged_input_semantics(source_provider_id, bridge_type);
                        let persisted_openai_semantics =
                            crate::request_logs::cx2cc_openai_input_semantics_override(
                                special_settings_json,
                                Some(provider_id),
                            );
                        let rust_value = effective_input_tokens(
                            cli_key,
                            persisted_openai_semantics,
                            bridged,
                            input,
                            cache_read,
                            cache_creation,
                        );

                        assert_eq!(plain_value, rust_value, "plain SQL: cli_key={cli_key} provider={provider_id} marker={special_settings_json:?} input={input:?} read={cache_read:?} creation={cache_creation:?}");
                        assert_eq!(aliased_value, rust_value, "aliased SQL: cli_key={cli_key} provider={provider_id} marker={special_settings_json:?} input={input:?} read={cache_read:?} creation={cache_creation:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn effective_total_tokens_preserves_openai_token_conservation() {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            r#"
CREATE TABLE providers (id INTEGER PRIMARY KEY, source_provider_id INTEGER, bridge_type TEXT);
CREATE TABLE r (
  cli_key TEXT,
  final_provider_id INTEGER,
  special_settings_json TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER
);
INSERT INTO r VALUES ('codex', 1, NULL, 1000, 50, 100, 200);
"#,
        )
        .expect("create token conservation fixture");

        let sql = format!("SELECT {} FROM r", sql_effective_total_tokens_expr());
        let total: i64 = conn
            .query_row(&sql, [], |row| row.get(0))
            .expect("evaluate effective total");
        assert_eq!(total, 1050);
    }

    #[test]
    fn effective_input_sql_uses_one_unsorted_marker_scan() {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            r#"
CREATE TABLE providers (id INTEGER PRIMARY KEY, source_provider_id INTEGER, bridge_type TEXT);
CREATE TABLE r (
  cli_key TEXT,
  final_provider_id INTEGER,
  special_settings_json TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_input_tokens INTEGER,
  cache_creation_input_tokens INTEGER
);
"#,
        )
        .expect("create explain schema");

        for expression in [
            sql_effective_input_tokens_expr(),
            sql_effective_input_tokens_expr_with_alias("r"),
        ] {
            let sql = format!("EXPLAIN QUERY PLAN SELECT {expression} FROM r");
            let mut stmt = conn.prepare(&sql).expect("prepare query plan");
            let details = stmt
                .query_map([], |row| row.get::<_, String>(3))
                .expect("query plan")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("read query plan");
            let marker_scans = details
                .iter()
                .filter(|detail| detail.contains("SCAN marker VIRTUAL TABLE"))
                .count();

            assert_eq!(marker_scans, 1, "query plan: {details:?}");
            assert!(
                details
                    .iter()
                    .all(|detail| !detail.contains("USE TEMP B-TREE FOR ORDER BY")),
                "query plan: {details:?}"
            );
        }
    }
}
