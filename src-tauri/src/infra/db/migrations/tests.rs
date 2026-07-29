use super::*;

#[test]
fn migrate_v38_to_v39_converts_valid_retry_overrides_and_preserves_malformed_rows() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY,
  upstream_retry_policy_json TEXT
);
PRAGMA user_version = 38;
"#,
    )
    .expect("create v38 fixture");
    let legacy = r#"{"enabled":true,"status_codes":[429,503],"transport_errors":["timeout"],"max_retries":2,"backoff_ms":25,"counts_toward_circuit_breaker":false}"#;
    let current = r#"{"enabled":false,"http_rules":[],"transport_errors":[],"max_retries":1,"backoff_ms":0,"counts_toward_circuit_breaker":false}"#;
    let malformed = "{not-json";
    for (id, value) in [(1_i64, legacy), (2, current), (3, malformed)] {
        conn.execute(
            "INSERT INTO providers(id, upstream_retry_policy_json) VALUES (?1, ?2)",
            rusqlite::params![id, value],
        )
        .expect("insert fixture row");
    }
    let non_text = vec![0xff_u8, 0x00, 0x7f];
    conn.execute(
        "INSERT INTO providers(id, upstream_retry_policy_json) VALUES (?1, ?2)",
        rusqlite::params![4_i64, non_text],
    )
    .expect("insert non-text fixture row");

    v38_to_v39::migrate_v38_to_v39(&mut conn).expect("migrate v38->v39");

    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read version");
    assert_eq!(version, 39);
    let migrated: String = conn
        .query_row(
            "SELECT upstream_retry_policy_json FROM providers WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("read migrated row");
    let migrated: serde_json::Value = serde_json::from_str(&migrated).expect("parse migrated row");
    assert!(migrated.get("status_codes").is_none());
    assert_eq!(migrated["http_rules"][0]["status_code"], 429);
    assert_eq!(migrated["http_rules"][1]["status_code"], 503);
    assert_eq!(migrated["transport_errors"], serde_json::json!(["timeout"]));

    let unchanged_current: String = conn
        .query_row(
            "SELECT upstream_retry_policy_json FROM providers WHERE id = 2",
            [],
            |row| row.get(0),
        )
        .expect("read current row");
    let unchanged_malformed: String = conn
        .query_row(
            "SELECT upstream_retry_policy_json FROM providers WHERE id = 3",
            [],
            |row| row.get(0),
        )
        .expect("read malformed row");
    let unchanged_non_text: Vec<u8> = conn
        .query_row(
            "SELECT upstream_retry_policy_json FROM providers WHERE id = 4",
            [],
            |row| row.get(0),
        )
        .expect("read non-text row");
    assert_eq!(unchanged_current, current);
    assert_eq!(unchanged_malformed, malformed);
    assert_eq!(unchanged_non_text, vec![0xff, 0x00, 0x7f]);
}

#[test]
fn migrate_v38_to_v39_adds_missing_retry_policy_column() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(
        r#"
CREATE TABLE providers (id INTEGER PRIMARY KEY);
PRAGMA user_version = 38;
"#,
    )
    .expect("create minimal v38 fixture");

    v38_to_v39::migrate_v38_to_v39(&mut conn).expect("migrate v38->v39");

    let column_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM pragma_table_info('providers') WHERE name = 'upstream_retry_policy_json'",
            [],
            |row| row.get(0),
        )
        .expect("inspect migrated columns");
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read version");
    assert_eq!(column_count, 1);
    assert_eq!(version, 39);
}

#[test]
fn migrate_v32_to_v33_backfills_pool_and_default_route_orders() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  auth_mode TEXT NOT NULL DEFAULT 'api_key',
  oauth_provider_type TEXT,
  oauth_access_token TEXT,
  oauth_refresh_token TEXT,
  oauth_expires_at INTEGER,
  oauth_email TEXT,
  oauth_last_error TEXT,
  limit_5h_usd REAL,
  limit_daily_usd REAL,
  daily_reset_mode TEXT NOT NULL DEFAULT 'fixed',
  daily_reset_time TEXT NOT NULL DEFAULT '00:00:00',
  limit_weekly_usd REAL,
  limit_monthly_usd REAL,
  limit_total_usd REAL,
  tags_json TEXT NOT NULL DEFAULT '[]',
  note TEXT NOT NULL DEFAULT '',
  source_provider_id INTEGER,
  bridge_type TEXT,
  stream_idle_timeout_seconds INTEGER,
  upstream_retry_policy_json TEXT,
  UNIQUE(cli_key, name)
);
"#,
    )
    .expect("create providers table");

    for (id, name, enabled, sort_order) in [
        (1_i64, "p1", 1_i64, 0_i64),
        (2_i64, "p2", 0_i64, 1_i64),
        (3_i64, "p3", 1_i64, 2_i64),
    ] {
        conn.execute(
            r#"
INSERT INTO providers(
  id,
  cli_key,
  name,
  base_url,
  api_key_plaintext,
  enabled,
  created_at,
  updated_at,
  sort_order
) VALUES (?1, 'claude', ?2, 'https://example.com', 'sk', ?3, 1, 1, ?4)
"#,
            rusqlite::params![id, name, enabled, sort_order],
        )
        .expect("insert provider");
    }

    v32_to_v33::migrate_v32_to_v33(&mut conn).expect("migrate v32->v33");

    let pool_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT provider_id FROM provider_pool_order ORDER BY sort_order ASC")
            .expect("prepare pool");
        stmt.query_map([], |row| row.get(0))
            .expect("query pool")
            .map(|row| row.expect("pool row"))
            .collect()
    };
    assert_eq!(pool_ids, vec![1, 2, 3]);

    let default_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT provider_id FROM default_route_providers ORDER BY sort_order ASC")
            .expect("prepare default");
        stmt.query_map([], |row| row.get(0))
            .expect("query default")
            .map(|row| row.expect("default row"))
            .collect()
    };
    assert_eq!(default_ids, vec![1, 3]);
}

#[test]
fn ensure_patches_do_not_repopulate_default_route_members() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    apply_migrations(&mut conn).expect("apply migrations");

    for (id, name, sort_order) in [
        (1_i64, "p1", 0_i64),
        (2_i64, "p2", 1_i64),
        (3_i64, "p3", 2_i64),
    ] {
        let provider_uuid = crate::shared::uuid::new_uuid_v4();
        conn.execute(
            r#"
INSERT INTO providers(
  id,
  provider_uuid,
  cli_key,
  name,
  base_url,
  api_key_plaintext,
  enabled,
  created_at,
  updated_at,
  sort_order
) VALUES (?1, ?2, 'claude', ?3, 'https://example.com', 'sk', 1, 1, 1, ?4)
"#,
            rusqlite::params![id, provider_uuid, name, sort_order],
        )
        .expect("insert provider");
    }

    for (provider_id, sort_order) in [(1_i64, 0_i64), (2_i64, 1_i64), (3_i64, 2_i64)] {
        conn.execute(
            r#"
INSERT INTO default_route_providers(
  cli_key,
  provider_id,
  sort_order,
  created_at,
  updated_at
) VALUES ('claude', ?1, ?2, 1, 1)
"#,
            rusqlite::params![provider_id, sort_order],
        )
        .expect("insert default route provider");
    }
    conn.execute(
        "DELETE FROM default_route_providers WHERE cli_key = 'claude' AND provider_id = 2",
        [],
    )
    .expect("simulate removing provider from default route");

    ensure::apply_ensure_patches(&mut conn).expect("apply ensure patches");

    let default_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT provider_id FROM default_route_providers ORDER BY sort_order ASC")
            .expect("prepare default");
        stmt.query_map([], |row| row.get(0))
            .expect("query default")
            .map(|row| row.expect("default row"))
            .collect()
    };
    assert_eq!(default_ids, vec![1, 3]);

    let pool_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT provider_id FROM provider_pool_order ORDER BY sort_order ASC")
            .expect("prepare pool");
        stmt.query_map([], |row| row.get(0))
            .expect("query pool")
            .map(|row| row.expect("pool row"))
            .collect()
    };
    assert_eq!(pool_ids, vec![1, 2, 3]);
}

#[test]
fn migrate_v25_to_v26_backfills_claude_models_json_from_legacy_mapping() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");

    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  UNIQUE(cli_key, name)
);
"#,
    )
    .expect("create providers table");

    let legacy_mapping = serde_json::json!({
        "*": "glm-4-plus",
        "claude-*sonnet*": "glm-4-plus-sonnet",
        "claude-*haiku*": "glm-4-plus-haiku",
        "claude-*thinking*": "glm-4-plus-thinking"
    })
    .to_string();

    conn.execute(
        r#"
INSERT INTO providers(
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  api_key_plaintext,
  enabled,
  priority,
  created_at,
  updated_at,
  sort_order,
  cost_multiplier,
  supported_models_json,
  model_mapping_json
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 100, 1, 1, 0, 1.0, '{}', ?7)
"#,
        rusqlite::params![
            "claude",
            "legacy",
            "https://example.com",
            "[]",
            "order",
            "sk-test",
            legacy_mapping
        ],
    )
    .expect("insert legacy provider");

    v25_to_v26::migrate_v25_to_v26(&mut conn).expect("migrate v25->v26");

    let claude_models_json: String = conn
        .query_row(
            "SELECT claude_models_json FROM providers WHERE name = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("read claude_models_json");

    let value: serde_json::Value =
        serde_json::from_str(&claude_models_json).expect("claude_models_json valid json");

    assert_eq!(value["main_model"], "glm-4-plus");
    assert_eq!(value["sonnet_model"], "glm-4-plus-sonnet");
    assert_eq!(value["haiku_model"], "glm-4-plus-haiku");
    assert_eq!(value["reasoning_model"], "glm-4-plus-thinking");

    let supported_models_json: String = conn
        .query_row(
            "SELECT supported_models_json FROM providers WHERE name = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("read supported_models_json");
    assert_eq!(supported_models_json.trim(), "{}");

    let model_mapping_json: String = conn
        .query_row(
            "SELECT model_mapping_json FROM providers WHERE name = 'legacy'",
            [],
            |row| row.get(0),
        )
        .expect("read model_mapping_json");
    assert_eq!(model_mapping_json.trim(), "{}");
}

#[test]
fn ensure_plugin_tables_is_idempotent() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    apply_migrations(&mut conn).expect("apply migrations once");
    apply_migrations(&mut conn).expect("apply migrations twice");

    for table in [
        "plugins",
        "plugin_versions",
        "plugin_configs",
        "plugin_permissions",
        "plugin_audit_logs",
        "plugin_market_sources",
        "plugin_runtime_failures",
        "plugin_hook_execution_reports",
    ] {
        assert!(
            test_has_table(&conn, table),
            "missing plugin table after ensure patches: {table}"
        );
    }

    assert!(test_has_column(&conn, "plugins", "plugin_id"));
    assert!(test_has_column(&conn, "plugins", "current_version"));
    assert!(test_has_column(&conn, "plugins", "status"));
    assert!(test_has_column(&conn, "plugins", "manifest_json"));
    assert!(test_has_column(&conn, "plugins", "last_error"));
    assert!(test_has_column(&conn, "plugin_configs", "config_json"));
    assert!(test_has_column(
        &conn,
        "plugin_permissions",
        "permissions_json"
    ));
    assert!(test_has_index(
        &conn,
        "idx_plugin_hook_execution_reports_created_at"
    ));
}

#[test]
fn migrations_create_provider_extension_values_table() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    apply_migrations(&mut conn).expect("apply migrations");

    assert!(test_has_table(&conn, "provider_extension_values"));
    assert!(test_has_column(
        &conn,
        "provider_extension_values",
        "provider_id"
    ));
    assert!(test_has_column(
        &conn,
        "provider_extension_values",
        "plugin_id"
    ));
    assert!(test_has_column(
        &conn,
        "provider_extension_values",
        "namespace"
    ));
    assert!(test_has_column(
        &conn,
        "provider_extension_values",
        "values_json"
    ));
}

#[test]
fn ensure_patch_drops_legacy_request_attempt_logs_table() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    apply_migrations(&mut conn).expect("create current schema");

    conn.execute_batch(
        r#"
CREATE TABLE request_attempt_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  trace_id TEXT NOT NULL,
  provider_id INTEGER NOT NULL
);
"#,
    )
    .expect("create legacy request_attempt_logs table");

    assert!(test_has_table(&conn, "request_attempt_logs"));

    apply_migrations(&mut conn).expect("apply migrations");

    assert!(!test_has_table(&conn, "request_attempt_logs"));

    apply_migrations(&mut conn).expect("apply migrations twice");
    assert!(!test_has_table(&conn, "request_attempt_logs"));
}

#[test]
fn ensure_patch_adds_reset_credit_count_to_existing_oauth_snapshot_table() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  UNIQUE(cli_key, name)
);

CREATE TABLE provider_oauth_limit_snapshots (
  provider_id INTEGER PRIMARY KEY,
  limit_short_label TEXT,
  limit_5h_text TEXT,
  limit_weekly_text TEXT,
  limit_5h_reset_at INTEGER,
  limit_weekly_reset_at INTEGER,
  checked_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE TABLE prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

INSERT INTO providers(
  id,
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  claude_models_json,
  api_key_plaintext,
  enabled,
  priority,
  created_at,
  updated_at,
  sort_order,
  cost_multiplier,
  supported_models_json,
  model_mapping_json
) VALUES (1, 'codex', 'legacy oauth', 'https://example.com', '[]', 'order', '{}', '', 1, 100, 1, 1, 0, 1.0, '{}', '{}');

INSERT INTO provider_oauth_limit_snapshots(
  provider_id,
  limit_short_label,
  limit_5h_text,
  limit_weekly_text,
  limit_5h_reset_at,
  limit_weekly_reset_at,
  checked_at,
  updated_at
) VALUES (1, '5h', '25%', '80%', 10, 20, 30, 30);

PRAGMA user_version = 32;
"#,
    )
    .expect("create legacy snapshot schema");

    assert!(!test_has_column(
        &conn,
        "provider_oauth_limit_snapshots",
        "reset_credit_available_count"
    ));

    apply_migrations(&mut conn).expect("apply migrations");

    assert!(test_has_column(
        &conn,
        "provider_oauth_limit_snapshots",
        "reset_credit_available_count"
    ));
    let row: (String, Option<i64>) = conn
        .query_row(
            "SELECT limit_5h_text, reset_credit_available_count FROM provider_oauth_limit_snapshots WHERE provider_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read migrated snapshot");
    assert_eq!(row, ("25%".to_string(), None));

    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn migrate_v27_to_v28_drops_provider_mode_and_deletes_official_providers() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  provider_mode TEXT NOT NULL DEFAULT 'relay',
  UNIQUE(cli_key, name)
);

CREATE TABLE provider_circuit_breakers (
  provider_id INTEGER PRIMARY KEY,
  state TEXT NOT NULL,
  failure_count INTEGER NOT NULL DEFAULT 0,
  open_until INTEGER,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE TABLE sort_modes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(name)
);

CREATE TABLE sort_mode_providers (
  mode_id INTEGER NOT NULL,
  cli_key TEXT NOT NULL,
  provider_id INTEGER NOT NULL,
  sort_order INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(mode_id, cli_key, provider_id),
  FOREIGN KEY(mode_id) REFERENCES sort_modes(id) ON DELETE CASCADE,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE TABLE claude_model_validation_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  request_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
"#,
    )
    .expect("create v27 schema");

    conn.execute(
        r#"
INSERT INTO providers(
  id,
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  claude_models_json,
  api_key_plaintext,
  enabled,
  priority,
  created_at,
  updated_at,
  sort_order,
  cost_multiplier,
  supported_models_json,
  model_mapping_json,
  provider_mode
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
"#,
        rusqlite::params![
            1i64,
            "codex",
            "relay",
            "https://relay.example.com/v1",
            "[\"https://relay.example.com/v1\"]",
            "order",
            "{}",
            "sk-relay",
            1i64,
            100i64,
            1i64,
            1i64,
            0i64,
            1.0f64,
            "{}",
            "{}",
            "relay",
        ],
    )
    .expect("insert relay provider");

    conn.execute(
        r#"
INSERT INTO providers(
  id,
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  claude_models_json,
  api_key_plaintext,
  enabled,
  priority,
  created_at,
  updated_at,
  sort_order,
  cost_multiplier,
  supported_models_json,
  model_mapping_json,
  provider_mode
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
"#,
        rusqlite::params![
            2i64,
            "codex",
            "official",
            "https://api.openai.com/v1",
            "[\"https://api.openai.com/v1\"]",
            "order",
            "{}",
            "",
            1i64,
            100i64,
            1i64,
            1i64,
            1i64,
            1.0f64,
            "{}",
            "{}",
            "official",
        ],
    )
    .expect("insert official provider");

    conn.execute(
            "INSERT INTO provider_circuit_breakers(provider_id, state, failure_count, open_until, updated_at) VALUES (?1, 'CLOSED', 0, NULL, 1)",
            rusqlite::params![1i64],
        )
        .expect("insert relay breaker");
    conn.execute(
            "INSERT INTO provider_circuit_breakers(provider_id, state, failure_count, open_until, updated_at) VALUES (?1, 'CLOSED', 0, NULL, 1)",
            rusqlite::params![2i64],
        )
        .expect("insert official breaker");

    conn.execute(
        "INSERT INTO sort_modes(id, name, created_at, updated_at) VALUES (1, 'mode', 1, 1)",
        [],
    )
    .expect("insert sort mode");
    conn.execute(
            "INSERT INTO sort_mode_providers(mode_id, cli_key, provider_id, sort_order, created_at, updated_at) VALUES (1, 'codex', 1, 0, 1, 1)",
            [],
        )
        .expect("insert relay sort_mode_provider");
    conn.execute(
            "INSERT INTO sort_mode_providers(mode_id, cli_key, provider_id, sort_order, created_at, updated_at) VALUES (1, 'codex', 2, 1, 1, 1)",
            [],
        )
        .expect("insert official sort_mode_provider");

    conn.execute(
            "INSERT INTO claude_model_validation_runs(id, provider_id, created_at, request_json, result_json) VALUES (1, 1, 1, '{}', '{}')",
            [],
        )
        .expect("insert relay validation run");
    conn.execute(
            "INSERT INTO claude_model_validation_runs(id, provider_id, created_at, request_json, result_json) VALUES (2, 2, 1, '{}', '{}')",
            [],
        )
        .expect("insert official validation run");

    v27_to_v28::migrate_v27_to_v28(&mut conn).expect("migrate v27->v28");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, 28);

    let provider_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
        .expect("count providers");
    assert_eq!(provider_count, 1);

    let breaker_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_circuit_breakers",
            [],
            |row| row.get(0),
        )
        .expect("count breakers");
    assert_eq!(breaker_count, 1);

    let sort_mode_provider_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sort_mode_providers", [], |row| {
            row.get(0)
        })
        .expect("count sort_mode_providers");
    assert_eq!(sort_mode_provider_count, 1);

    let validation_run_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM claude_model_validation_runs",
            [],
            |row| row.get(0),
        )
        .expect("count validation runs");
    assert_eq!(validation_run_count, 1);

    let remaining_name: String = conn
        .query_row("SELECT name FROM providers WHERE id = 1", [], |row| {
            row.get(0)
        })
        .expect("read remaining provider name");
    assert_eq!(remaining_name, "relay");

    let mut has_provider_mode = false;
    {
        let mut stmt = conn
            .prepare("PRAGMA table_info(providers)")
            .expect("prepare providers table_info query");
        let mut rows = stmt.query([]).expect("query providers table_info");
        while let Some(row) = rows.next().expect("read table_info row") {
            let name: String = row.get(1).expect("read column name");
            if name == "provider_mode" {
                has_provider_mode = true;
                break;
            }
        }
    }
    assert!(!has_provider_mode);
}

fn test_has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql).expect("prepare table_info");
    let mut rows = stmt.query([]).expect("query table_info");
    while let Some(row) = rows.next().expect("read table_info row") {
        let name: String = row.get(1).expect("read column name");
        if name == column {
            return true;
        }
    }
    false
}

fn test_has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [table],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

fn test_has_index(conn: &Connection, index: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1 LIMIT 1",
        [index],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

fn test_has_trigger(conn: &Connection, trigger: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?1 LIMIT 1",
        [trigger],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

fn assert_no_v40_provider_model_schema(conn: &Connection) {
    for table in [
        "provider_model_catalogs",
        "provider_models",
        "codex_managed_profiles",
    ] {
        assert!(!test_has_table(conn, table), "unexpected {table}");
    }
    assert!(!test_has_index(conn, "idx_providers_provider_uuid"));
    for trigger in [
        "providers_provider_uuid_insert_guard",
        "providers_provider_uuid_update_guard",
    ] {
        assert!(!test_has_trigger(conn, trigger), "unexpected {trigger}");
    }
}

#[test]
fn strict_v29_patch_adds_sort_mode_provider_enabled_column() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    conn.execute_batch(
        r#"
CREATE TABLE prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(cli_key, name)
);

CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  UNIQUE(cli_key, name)
);

CREATE TABLE sort_mode_providers (
  mode_id INTEGER NOT NULL,
  cli_key TEXT NOT NULL,
  provider_id INTEGER NOT NULL,
  sort_order INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(mode_id, cli_key, provider_id)
);

PRAGMA user_version = 29;
"#,
    )
    .expect("create legacy sort_mode_providers schema");

    assert!(!test_has_column(&conn, "sort_mode_providers", "enabled"));

    apply_migrations(&mut conn).expect("apply migrations");
    assert!(test_has_column(&conn, "sort_mode_providers", "enabled"));

    // Idempotent: second run should succeed.
    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn ensure_patch_backfills_oauth_columns_for_legacy_v30_schema() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    conn.execute_batch(
        r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0
);

CREATE TABLE prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

PRAGMA user_version = 30;
"#,
    )
    .expect("create legacy v30 schema without oauth columns");

    conn.execute(
        r#"
INSERT INTO providers(
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  claude_models_json,
  api_key_plaintext,
  enabled,
  priority,
  created_at,
  updated_at,
  sort_order,
  cost_multiplier
) VALUES ('claude', 'legacy', 'https://example.com', '[]', 'order', '{}', 'sk-test', 1, 100, 1, 1, 0, 1.0)
"#,
        [],
    )
    .expect("insert legacy provider");

    apply_migrations(&mut conn).expect("apply migrations");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);

    for column in [
        "auth_mode",
        "oauth_provider_type",
        "oauth_access_token",
        "oauth_refresh_token",
        "oauth_id_token",
        "oauth_token_uri",
        "oauth_client_id",
        "oauth_client_secret",
        "oauth_expires_at",
        "oauth_email",
        "oauth_last_refreshed_at",
        "oauth_last_error",
        "oauth_refresh_lead_s",
    ] {
        assert!(test_has_column(&conn, "providers", column));
    }

    let (auth_mode, oauth_refresh_lead_s): (String, i64) = conn
        .query_row(
            "SELECT auth_mode, oauth_refresh_lead_s FROM providers WHERE name = 'legacy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read oauth defaults");
    assert_eq!(auth_mode, "api_key");
    assert_eq!(oauth_refresh_lead_s, 3600);

    // Idempotent: second run should succeed.
    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn strict_v29_patch_migrates_legacy_workspace_cluster_tables() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    conn.execute_batch(
        r#"
CREATE TABLE prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(cli_key, name)
);

CREATE TABLE mcp_servers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  server_key TEXT NOT NULL,
  name TEXT NOT NULL,
  transport TEXT NOT NULL,
  command TEXT,
  args_json TEXT NOT NULL DEFAULT '[]',
  env_json TEXT NOT NULL DEFAULT '{}',
  cwd TEXT,
  url TEXT,
  headers_json TEXT NOT NULL DEFAULT '{}',
  enabled_claude INTEGER NOT NULL DEFAULT 0,
  enabled_codex INTEGER NOT NULL DEFAULT 0,
  enabled_gemini INTEGER NOT NULL DEFAULT 0,
  normalized_name TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(server_key)
);

CREATE TABLE skills (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  skill_key TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  source_git_url TEXT NOT NULL,
  source_branch TEXT NOT NULL,
  source_subdir TEXT NOT NULL,
  enabled_claude INTEGER NOT NULL DEFAULT 0,
  enabled_codex INTEGER NOT NULL DEFAULT 0,
  enabled_gemini INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(skill_key)
);

CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  UNIQUE(cli_key, name)
);

PRAGMA user_version = 29;
"#,
    )
    .expect("create legacy v29 tables");

    conn.execute(
        r#"
INSERT INTO prompts(id, cli_key, name, content, enabled, created_at, updated_at)
VALUES (1, 'claude', 'default', 'hello', 1, 1, 1)
"#,
        [],
    )
    .expect("insert prompt");
    conn.execute(
        r#"
INSERT INTO prompts(id, cli_key, name, content, enabled, created_at, updated_at)
VALUES (2, 'codex', 'p2', 'world', 0, 1, 1)
"#,
        [],
    )
    .expect("insert prompt");

    conn.execute(
        r#"
INSERT INTO mcp_servers(
  id,
  server_key,
  name,
  transport,
  command,
  args_json,
  env_json,
  cwd,
  url,
  headers_json,
  enabled_claude,
  enabled_codex,
  enabled_gemini,
  normalized_name,
  created_at,
  updated_at
) VALUES (
  1,
  'srv1',
  'S1',
  'stdio',
  'echo',
  '[]',
  '{}',
  NULL,
  NULL,
  '{}',
  1,
  0,
  0,
  's1',
  1,
  1
)
"#,
        [],
    )
    .expect("insert mcp server");
    conn.execute(
        r#"
INSERT INTO mcp_servers(
  id,
  server_key,
  name,
  transport,
  command,
  args_json,
  env_json,
  cwd,
  url,
  headers_json,
  enabled_claude,
  enabled_codex,
  enabled_gemini,
  normalized_name,
  created_at,
  updated_at
) VALUES (
  2,
  'srv2',
  'S2',
  'stdio',
  'echo',
  '[]',
  '{}',
  NULL,
  NULL,
  '{}',
  0,
  1,
  0,
  's2',
  1,
  1
)
"#,
        [],
    )
    .expect("insert mcp server");

    conn.execute(
        r#"
INSERT INTO skills(
  id,
  skill_key,
  name,
  normalized_name,
  description,
  source_git_url,
  source_branch,
  source_subdir,
  enabled_claude,
  enabled_codex,
  enabled_gemini,
  created_at,
  updated_at
) VALUES (
  1,
  'sk1',
  'Skill 1',
  'skill-1',
  '',
  'https://example.com',
  'main',
  'skills/skill1',
  0,
  1,
  0,
  1,
  1
)
"#,
        [],
    )
    .expect("insert skill");

    apply_migrations(&mut conn).expect("apply migrations");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);

    assert!(test_has_column(&conn, "workspaces", "cli_key"));
    assert!(test_has_column(&conn, "workspace_active", "workspace_id"));

    assert!(test_has_column(&conn, "prompts", "workspace_id"));
    assert!(!test_has_column(&conn, "prompts", "cli_key"));

    assert!(test_has_column(&conn, "providers", "limit_5h_usd"));
    assert!(test_has_column(&conn, "providers", "limit_daily_usd"));
    assert!(test_has_column(&conn, "providers", "daily_reset_mode"));
    assert!(test_has_column(&conn, "providers", "daily_reset_time"));
    assert!(test_has_column(&conn, "providers", "limit_weekly_usd"));
    assert!(test_has_column(&conn, "providers", "limit_monthly_usd"));
    assert!(test_has_column(&conn, "providers", "limit_total_usd"));
    assert!(test_has_column(&conn, "skills", "installed_content_hash"));

    let claude_default_ws_id: i64 = conn
        .query_row(
            "SELECT id FROM workspaces WHERE cli_key = 'claude' AND name = '默认' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("read default claude workspace id");
    let codex_default_ws_id: i64 = conn
        .query_row(
            "SELECT id FROM workspaces WHERE cli_key = 'codex' AND name = '默认' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("read default codex workspace id");

    let p1_cli: String = conn
        .query_row(
            r#"
SELECT w.cli_key
FROM prompts p
JOIN workspaces w ON w.id = p.workspace_id
WHERE p.id = 1
"#,
            [],
            |row| row.get(0),
        )
        .expect("read migrated prompt cli_key");
    assert_eq!(p1_cli, "claude");

    let p2_cli: String = conn
        .query_row(
            r#"
SELECT w.cli_key
FROM prompts p
JOIN workspaces w ON w.id = p.workspace_id
WHERE p.id = 2
"#,
            [],
            |row| row.get(0),
        )
        .expect("read migrated prompt cli_key");
    assert_eq!(p2_cli, "codex");

    let claude_enabled_mcp: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspace_mcp_enabled WHERE workspace_id = ?1 AND server_id = 1",
            [claude_default_ws_id],
            |row| row.get(0),
        )
        .expect("count enabled mcp for claude");
    assert_eq!(claude_enabled_mcp, 1);

    let codex_enabled_mcp: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspace_mcp_enabled WHERE workspace_id = ?1 AND server_id = 2",
            [codex_default_ws_id],
            |row| row.get(0),
        )
        .expect("count enabled mcp for codex");
    assert_eq!(codex_enabled_mcp, 1);

    let legacy_mcp_flags: (i64, i64, i64) = conn
        .query_row(
            "SELECT enabled_claude, enabled_codex, enabled_gemini FROM mcp_servers WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read legacy mcp flags");
    assert_eq!(legacy_mcp_flags, (0, 0, 0));

    let enabled_skill: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspace_skill_enabled WHERE workspace_id = ?1 AND skill_id = 1",
            [codex_default_ws_id],
            |row| row.get(0),
        )
        .expect("count enabled skills");
    assert_eq!(enabled_skill, 1);

    let legacy_skill_flags: (i64, i64, i64) = conn
        .query_row(
            "SELECT enabled_claude, enabled_codex, enabled_gemini FROM skills WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read legacy skill flags");
    assert_eq!(legacy_skill_flags, (0, 0, 0));

    // Idempotent: second run should succeed without changing schema.
    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn ensure_patch_backfills_request_log_activity_columns_for_existing_request_logs_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("request-log-activity-drift.db");

    {
        let mut conn = Connection::open(&db_path).expect("open sqlite file");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign_keys");

        apply_migrations(&mut conn).expect("create current schema");
        conn.execute_batch(
            r#"
ALTER TABLE request_logs DROP COLUMN last_activity_ms;
ALTER TABLE request_logs DROP COLUMN activity_details_json;
"#,
        )
        .expect("remove request log activity columns");

        assert!(!test_has_column(&conn, "request_logs", "last_activity_ms"));
        assert!(!test_has_column(
            &conn,
            "request_logs",
            "activity_details_json"
        ));
    }

    let db = crate::db::init_for_tests(&db_path).expect("repair drifted db");

    {
        let conn = db.open_connection().expect("open repaired db");
        assert!(test_has_column(&conn, "request_logs", "last_activity_ms"));
        assert!(test_has_column(
            &conn,
            "request_logs",
            "activity_details_json"
        ));

        conn.prepare("SELECT last_activity_ms, activity_details_json FROM request_logs LIMIT 1")
            .expect("prepare request log activity column select");
    }

    let summaries =
        crate::request_logs::list_recent_all(&db, 10).expect("list recent all after repair");
    assert!(summaries.is_empty());

    {
        let mut conn = db.open_connection().expect("open repaired db twice");
        apply_migrations(&mut conn).expect("apply migrations twice");
    }
}

#[test]
fn baseline_v25_creates_complete_schema_for_fresh_install() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    // Fresh install: user_version = 0
    apply_migrations(&mut conn).expect("apply migrations on fresh db");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);

    // Verify all tables exist
    let tables: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("prepare");
        let rows = stmt.query_map([], |row| row.get(0)).expect("query");
        rows.filter_map(|r| r.ok()).collect()
    };

    // Core tables from baseline
    assert!(tables.contains(&"providers".to_string()));
    assert!(tables.contains(&"request_logs".to_string()));
    assert!(tables.contains(&"prompts".to_string()));
    assert!(tables.contains(&"mcp_servers".to_string()));
    assert!(tables.contains(&"skills".to_string()));
    assert!(tables.contains(&"skill_repos".to_string()));
    assert!(tables.contains(&"model_prices".to_string()));
    assert!(tables.contains(&"provider_pool_order".to_string()));
    assert!(tables.contains(&"provider_account_usage_credentials".to_string()));
    assert!(tables.contains(&"default_route_providers".to_string()));
    assert!(tables.contains(&"sort_modes".to_string()));
    assert!(tables.contains(&"sort_mode_providers".to_string()));
    assert!(tables.contains(&"sort_mode_active".to_string()));
    assert!(tables.contains(&"claude_model_validation_runs".to_string()));
    assert!(tables.contains(&"image_gen_configs".to_string()));
    assert!(tables.contains(&"image_gen_tasks".to_string()));
    assert!(tables.contains(&"plugin_hook_execution_reports".to_string()));
    assert!(tables.contains(&"schema_migrations".to_string()));

    // Tables from ensure patches
    assert!(tables.contains(&"workspaces".to_string()));
    assert!(tables.contains(&"workspace_active".to_string()));
    assert!(tables.contains(&"workspace_mcp_enabled".to_string()));
    assert!(tables.contains(&"workspace_skill_enabled".to_string()));

    // Verify ensure patches ran (provider limit columns)
    assert!(test_has_column(&conn, "providers", "limit_5h_usd"));
    assert!(test_has_column(&conn, "providers", "limit_daily_usd"));
    assert!(test_has_column(&conn, "providers", "tags_json"));
    assert!(test_has_column(&conn, "skills", "installed_commit"));
    assert!(test_has_column(&conn, "skills", "installed_content_hash"));

    // Verify v25->v26 migration ran (claude_models_json)
    assert!(test_has_column(&conn, "providers", "claude_models_json"));

    // Verify sort_mode_providers.enabled from ensure patch
    assert!(test_has_column(&conn, "sort_mode_providers", "enabled"));

    // Verify request log read-path indexes from ensure patches
    assert!(test_has_index(
        &conn,
        "idx_request_logs_cli_path_created_at_ms_id"
    ));
    assert!(test_has_index(
        &conn,
        "idx_request_logs_cli_created_at_ms_id"
    ));
    assert!(test_has_index(
        &conn,
        "idx_request_logs_visible_created_at_ms_id"
    ));
    assert!(test_has_index(&conn, "idx_request_logs_cli_id"));
    assert!(test_has_column(&conn, "request_logs", "last_activity_ms"));
    assert!(test_has_column(
        &conn,
        "request_logs",
        "activity_details_json"
    ));

    // Verify prompts was migrated to workspace_id
    assert!(test_has_column(&conn, "prompts", "workspace_id"));
    assert!(!test_has_column(&conn, "prompts", "cli_key"));

    // Idempotent: second run should succeed
    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn ensure_patches_seed_grok_workspace_once_without_resetting_active_workspace() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    apply_migrations(&mut conn).expect("apply migrations on fresh db");

    let default_id: i64 = conn
        .query_row(
            "SELECT id FROM workspaces WHERE cli_key = 'grok' AND name = '默认'",
            [],
            |row| row.get(0),
        )
        .expect("read Grok default workspace");
    let initial_active_id: i64 = conn
        .query_row(
            "SELECT workspace_id FROM workspace_active WHERE cli_key = 'grok'",
            [],
            |row| row.get(0),
        )
        .expect("read Grok active workspace");
    assert_eq!(initial_active_id, default_id);

    conn.execute(
        "INSERT INTO workspaces(cli_key, name, normalized_name, created_at, updated_at) VALUES ('grok', 'Custom', 'custom', 1, 1)",
        [],
    )
    .expect("insert custom Grok workspace");
    let custom_id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE workspace_active SET workspace_id = ?1, updated_at = 2 WHERE cli_key = 'grok'",
        [custom_id],
    )
    .expect("activate custom Grok workspace");

    apply_migrations(&mut conn).expect("apply migrations twice");

    let default_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspaces WHERE cli_key = 'grok' AND name = '默认'",
            [],
            |row| row.get(0),
        )
        .expect("count Grok default workspaces");
    let active_id: i64 = conn
        .query_row(
            "SELECT workspace_id FROM workspace_active WHERE cli_key = 'grok'",
            [],
            |row| row.get(0),
        )
        .expect("read preserved Grok active workspace");

    assert_eq!(default_count, 1);
    assert_eq!(active_id, custom_id);
}

#[test]
fn migrate_v35_to_v36_creates_image_gen_configs_and_is_idempotent() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");

    v35_to_v36::migrate_v35_to_v36(&mut conn).expect("migrate v35->v36");

    assert!(test_has_table(&conn, "image_gen_configs"));
    for column in [
        "adapter_id",
        "base_url",
        "api_key_plaintext",
        "model",
        "created_at",
        "updated_at",
    ] {
        assert!(
            test_has_column(&conn, "image_gen_configs", column),
            "missing image_gen_configs column: {column}"
        );
    }

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, 36);

    // Idempotent: second run should succeed.
    v35_to_v36::migrate_v35_to_v36(&mut conn).expect("migrate v35->v36 twice");
}

#[test]
fn apply_migrations_upgrades_v35_schema_to_v36() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    apply_migrations(&mut conn).expect("create current schema");

    // Simulate a v35 database (before image_gen_configs existed).
    conn.execute_batch(
        r#"
DROP TABLE image_gen_configs;
PRAGMA user_version = 35;
"#,
    )
    .expect("simulate v35 schema");
    assert!(!test_has_table(&conn, "image_gen_configs"));

    apply_migrations(&mut conn).expect("apply migrations from v35");

    assert!(test_has_table(&conn, "image_gen_configs"));
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn migrate_v36_to_v37_creates_image_gen_tasks_and_is_idempotent() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");

    v36_to_v37::migrate_v36_to_v37(&mut conn).expect("migrate v36->v37");

    assert!(test_has_table(&conn, "image_gen_tasks"));
    for column in [
        "id",
        "adapter_id",
        "prompt",
        "request_json",
        "status",
        "error",
        "usage_json",
        "images_json",
        "ref_images_json",
        "dir",
        "created_at",
        "elapsed_ms",
    ] {
        assert!(
            test_has_column(&conn, "image_gen_tasks", column),
            "missing image_gen_tasks column: {column}"
        );
    }
    assert!(test_has_index(&conn, "idx_image_gen_tasks_created"));

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, 37);

    // Idempotent: second run should succeed.
    v36_to_v37::migrate_v36_to_v37(&mut conn).expect("migrate v36->v37 twice");
}

#[test]
fn apply_migrations_upgrades_v36_schema_to_v37() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    apply_migrations(&mut conn).expect("create current schema");

    // Simulate a v36 database (before image_gen_tasks existed).
    conn.execute_batch(
        r#"
DROP TABLE image_gen_tasks;
PRAGMA user_version = 36;
"#,
    )
    .expect("simulate v36 schema");
    assert!(!test_has_table(&conn, "image_gen_tasks"));

    apply_migrations(&mut conn).expect("apply migrations from v36");

    assert!(test_has_table(&conn, "image_gen_tasks"));
    assert!(test_has_index(&conn, "idx_image_gen_tasks_created"));
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);
}

#[test]
fn rejects_unsupported_old_schema_version() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA user_version = 10;")
        .expect("set old version");

    let result = apply_migrations(&mut conn);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("minimum supported: 25"));
}

#[test]
fn strict_v29_patch_accepts_dev_schema_and_normalizes_user_version_to_29() {
    let mut conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign_keys");

    conn.execute_batch(
        r#"
CREATE TABLE workspaces (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(cli_key, normalized_name)
);

CREATE TABLE workspace_active (
  cli_key TEXT PRIMARY KEY,
  workspace_id INTEGER,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL
);

CREATE TABLE prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  UNIQUE(workspace_id, name)
);

CREATE TABLE providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  base_urls_json TEXT NOT NULL DEFAULT '[]',
  base_url_mode TEXT NOT NULL DEFAULT 'order',
  claude_models_json TEXT NOT NULL DEFAULT '{}',
  api_key_plaintext TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  cost_multiplier REAL NOT NULL DEFAULT 1.0,
  supported_models_json TEXT NOT NULL DEFAULT '{}',
  model_mapping_json TEXT NOT NULL DEFAULT '{}',
  UNIQUE(cli_key, name)
);

CREATE TABLE mcp_servers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  server_key TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL DEFAULT '',
  transport TEXT NOT NULL,
  command TEXT,
  args_json TEXT NOT NULL DEFAULT '[]',
  env_json TEXT NOT NULL DEFAULT '{}',
  cwd TEXT,
  url TEXT,
  headers_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(server_key)
);

CREATE TABLE workspace_mcp_enabled (
  workspace_id INTEGER NOT NULL,
  server_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(workspace_id, server_id),
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY(server_id) REFERENCES mcp_servers(id) ON DELETE CASCADE
);

CREATE TABLE skills (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  skill_key TEXT NOT NULL,
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  source_git_url TEXT NOT NULL,
  source_branch TEXT NOT NULL,
  source_subdir TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(skill_key)
);

CREATE TABLE workspace_skill_enabled (
  workspace_id INTEGER NOT NULL,
  skill_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(workspace_id, skill_id),
  FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
);

PRAGMA user_version = 33;
"#,
    )
    .expect("create dev schema");

    conn.execute(
        "INSERT INTO workspaces(id, cli_key, name, normalized_name, created_at, updated_at) VALUES (1, 'claude', 'Dev', 'dev', 1, 1)",
        [],
    )
    .expect("insert workspace");
    conn.execute(
        "INSERT INTO workspace_active(cli_key, workspace_id, updated_at) VALUES ('claude', 1, 1)",
        [],
    )
    .expect("insert workspace_active");
    conn.execute(
        "INSERT INTO prompts(id, workspace_id, name, content, enabled, created_at, updated_at) VALUES (1, 1, 'default', 'hello', 1, 1, 1)",
        [],
    )
    .expect("insert prompt");
    conn.execute(
        "INSERT INTO mcp_servers(id, server_key, name, normalized_name, transport, command, args_json, env_json, cwd, url, headers_json, created_at, updated_at) VALUES (1, 'srv1', 'S1', 's1', 'stdio', 'echo', '[]', '{}', NULL, NULL, '{}', 1, 1)",
        [],
    )
    .expect("insert mcp server");
    conn.execute(
        "INSERT INTO workspace_mcp_enabled(workspace_id, server_id, created_at, updated_at) VALUES (1, 1, 1, 1)",
        [],
    )
    .expect("insert mcp enabled");
    conn.execute(
        "INSERT INTO skills(id, skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir, created_at, updated_at) VALUES (1, 'sk1', 'Skill 1', 'skill-1', '', 'https://example.com', 'main', 'skills/skill1', 1, 1)",
        [],
    )
    .expect("insert skill");
    conn.execute(
        "INSERT INTO workspace_skill_enabled(workspace_id, skill_id, created_at, updated_at) VALUES (1, 1, 1, 1)",
        [],
    )
    .expect("insert skill enabled");

    apply_migrations(&mut conn).expect("apply migrations");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version");
    assert_eq!(user_version, LATEST_SCHEMA_VERSION);

    assert!(test_has_column(&conn, "providers", "limit_5h_usd"));
    assert!(test_has_column(&conn, "providers", "limit_daily_usd"));
    assert!(test_has_column(&conn, "providers", "daily_reset_mode"));
    assert!(test_has_column(&conn, "providers", "daily_reset_time"));
    assert!(test_has_column(&conn, "providers", "limit_weekly_usd"));
    assert!(test_has_column(&conn, "providers", "limit_monthly_usd"));
    assert!(test_has_column(&conn, "providers", "limit_total_usd"));
    assert!(test_has_column(&conn, "skills", "installed_content_hash"));
    assert!(test_has_table(&conn, "plugin_hook_execution_reports"));

    let active_id: i64 = conn
        .query_row(
            "SELECT workspace_id FROM workspace_active WHERE cli_key = 'claude'",
            [],
            |row| row.get(0),
        )
        .expect("read active workspace");
    assert_eq!(active_id, 1);

    let enabled_mcp: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspace_mcp_enabled WHERE workspace_id = 1 AND server_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("count enabled mcp");
    assert_eq!(enabled_mcp, 1);

    apply_migrations(&mut conn).expect("apply migrations twice");
}

#[test]
fn migrate_v37_to_v38_moves_valid_user_ids_and_sanitizes_extension_values() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch(
        r#"
PRAGMA foreign_keys = ON;
CREATE TABLE providers(id INTEGER PRIMARY KEY);
CREATE TABLE provider_extension_values(
  provider_id INTEGER NOT NULL,
  plugin_id TEXT NOT NULL,
  namespace TEXT NOT NULL,
  values_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(provider_id, plugin_id, namespace),
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
INSERT INTO providers(id) VALUES (1), (2), (3);
INSERT INTO provider_extension_values(provider_id, plugin_id, namespace, values_json, updated_at)
VALUES
  (1, 'core.provider-account-usage', 'accountUsage', '{"adapterKind":"newapi","newApiUserId":"00042","newApiAccessToken":"SYNTHETIC_PRIVATE_A"}', 1),
  (2, 'core.provider-account-usage', 'accountUsage', '{"adapterKind":"newapi","newApiUserId":"invalid","systemAccessToken":"SYNTHETIC_PRIVATE_B"}', 1),
  (3, 'core.provider-account-usage', 'accountUsage', '{"adapterKind":"newapi","newApiQueryMode":"account","timedRefreshEnabled":false,"refreshIntervalSeconds":120}', 1);
PRAGMA user_version = 37;
"#,
    )
    .expect("create v37 fixture");

    v37_to_v38::migrate_v37_to_v38(&mut conn).expect("migrate v37->v38");
    v37_to_v38::migrate_v37_to_v38(&mut conn).expect("repeat v37->v38");

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version");
    assert_eq!(user_version, 38);
    assert!(test_has_table(&conn, "provider_account_usage_credentials"));
    let migrated_user_id: String = conn
        .query_row(
            "SELECT newapi_user_id FROM provider_account_usage_credentials WHERE provider_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("migrated user id");
    assert_eq!(migrated_user_id, "42");
    let invalid_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_account_usage_credentials WHERE provider_id = 2",
            [],
            |row| row.get(0),
        )
        .expect("invalid user id count");
    assert_eq!(invalid_count, 0);

    let mut statement = conn
        .prepare("SELECT values_json FROM provider_extension_values ORDER BY provider_id")
        .expect("extension query");
    let values = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("extension rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("extension values");
    assert_eq!(values.len(), 3);
    for value in &values {
        assert!(!value.contains("UserId"));
        assert!(!value.contains("AccessToken"));
        assert!(!value.contains("SYNTHETIC_PRIVATE"));
    }
    assert!(values[0].contains("\"newApiQueryMode\":\"billing\""));
    assert!(values[2].contains("\"newApiQueryMode\":\"account\""));
    drop(statement);

    conn.execute("DELETE FROM providers WHERE id = 1", [])
        .expect("delete provider");
    let credential_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_account_usage_credentials",
            [],
            |row| row.get(0),
        )
        .expect("credential count after cascade");
    assert_eq!(credential_count, 0);
}

#[test]
fn migrate_v39_to_v40_rejects_missing_providers_without_advancing() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch("PRAGMA user_version = 39;")
        .expect("create missing-provider fixture");

    let error =
        v39_to_v40::migrate_v39_to_v40(&mut conn).expect_err("missing providers table must fail");
    assert!(error.contains("requires the providers table"));
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version after failure");
    assert_eq!(user_version, 39);
    assert_no_v40_provider_model_schema(&conn);
}

#[test]
fn migrate_v39_to_v40_rejects_existing_invalid_provider_uuids_without_repair() {
    for invalid in [None, Some(""), Some("not-a-canonical-uuid")] {
        let mut conn = Connection::open_in_memory().expect("open migration db");
        conn.execute_batch(
            r#"
CREATE TABLE providers(
  id INTEGER PRIMARY KEY,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  provider_uuid TEXT
);
PRAGMA user_version = 39;
"#,
        )
        .expect("create dirty v39 fixture");
        conn.execute(
            "INSERT INTO providers(id, cli_key, name, provider_uuid) VALUES (1, 'codex', 'dirty', ?1)",
            rusqlite::params![invalid],
        )
        .expect("insert dirty provider UUID");

        let error = v39_to_v40::migrate_v39_to_v40(&mut conn)
            .expect_err("existing invalid provider UUID must fail");
        assert_eq!(error, "existing provider UUID is invalid");
        if let Some(invalid) = invalid.filter(|value| !value.is_empty()) {
            assert!(!error.contains(invalid), "error must not echo stored UUID");
        }
        let user_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("user version after failure");
        assert_eq!(user_version, 39);
        assert_no_v40_provider_model_schema(&conn);
    }
}

#[test]
fn migrate_v39_to_v40_rejects_duplicate_existing_provider_uuids_without_partial_schema() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch(
        r#"
CREATE TABLE providers(
  id INTEGER PRIMARY KEY,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL,
  provider_uuid TEXT
);
PRAGMA user_version = 39;
"#,
    )
    .expect("create duplicate v39 fixture");
    let duplicate = "550e8400-e29b-41d4-a716-446655440000";
    for id in [1_i64, 2_i64] {
        conn.execute(
            "INSERT INTO providers(id, cli_key, name, provider_uuid) VALUES (?1, 'codex', 'duplicate', ?2)",
            rusqlite::params![id, duplicate],
        )
        .expect("insert duplicate provider UUID");
    }

    let error =
        v39_to_v40::migrate_v39_to_v40(&mut conn).expect_err("duplicate provider UUIDs must fail");
    assert_eq!(error, "existing provider UUIDs are not unique");
    assert!(
        !error.contains(duplicate),
        "error must not echo stored UUID"
    );
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version after failure");
    assert_eq!(user_version, 39);
    assert_no_v40_provider_model_schema(&conn);
}

#[test]
fn migrate_v39_to_v40_backfills_canonical_provider_uuids_and_is_idempotent() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch(
        r#"
PRAGMA foreign_keys = ON;
CREATE TABLE providers(
  id INTEGER PRIMARY KEY,
  cli_key TEXT NOT NULL,
  name TEXT NOT NULL
);
INSERT INTO providers(id, cli_key, name) VALUES
  (1, 'codex', 'one'),
  (2, 'claude', 'two');
PRAGMA user_version = 39;
"#,
    )
    .expect("create v39 fixture");

    v39_to_v40::migrate_v39_to_v40(&mut conn).expect("migrate v39->v40");
    v39_to_v40::migrate_v39_to_v40(&mut conn).expect("repeat v39->v40");

    let provider_uuids = conn
        .prepare("SELECT provider_uuid FROM providers ORDER BY id ASC")
        .expect("prepare UUID query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query UUIDs")
        .collect::<Result<Vec<_>, _>>()
        .expect("read UUIDs");
    assert_eq!(provider_uuids.len(), 2);
    assert_ne!(provider_uuids[0], provider_uuids[1]);
    assert!(provider_uuids
        .iter()
        .all(|value| crate::shared::uuid::is_canonical_uuid_v4(value)));
    for table in [
        "provider_model_catalogs",
        "provider_models",
        "codex_managed_profiles",
    ] {
        assert!(test_has_table(&conn, table), "missing {table}");
    }
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version");
    assert_eq!(user_version, 40);

    let update_error = conn
        .execute(
            "UPDATE providers SET provider_uuid = ?1 WHERE id = 1",
            rusqlite::params![crate::shared::uuid::new_uuid_v4()],
        )
        .expect_err("provider UUID must be immutable");
    assert!(update_error
        .to_string()
        .contains("provider_uuid is immutable"));

    let extra_hyphen = "550e8400-e29b-41d4-a716-44665544000-";
    let insert_error = conn
        .execute(
            "INSERT INTO providers(id, cli_key, name, provider_uuid) VALUES (3, 'codex', 'bad', ?1)",
            rusqlite::params![extra_hyphen],
        )
        .expect_err("incremental trigger must reject an extra UUID hyphen");
    assert!(insert_error
        .to_string()
        .contains("provider_uuid must be a canonical UUID"));
}

#[test]
fn migrate_v40_to_v41_requires_provider_models_without_advancing() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch("PRAGMA user_version = 40;")
        .expect("create missing-model fixture");

    let error =
        v40_to_v41::migrate_v40_to_v41(&mut conn).expect_err("missing provider_models must fail");
    assert!(error.contains("requires the provider_models table"));
    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version after failure");
    assert_eq!(user_version, 40);
}

#[test]
fn migrate_v40_to_v41_backfills_existing_models_and_defaults_new_rows_unconfigured() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    conn.execute_batch(
        r#"
CREATE TABLE provider_models (
  model_uuid TEXT PRIMARY KEY,
  provider_id INTEGER NOT NULL,
  remote_model_id TEXT NOT NULL,
  source TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0,
  last_seen_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(provider_id, remote_model_id)
);
INSERT INTO provider_models(
  model_uuid, provider_id, remote_model_id, source, stale, created_at, updated_at
) VALUES ('old-model', 1, 'grok-4.5', 'manual', 0, 1, 1);
PRAGMA user_version = 40;
"#,
    )
    .expect("create v40 fixture");

    v40_to_v41::migrate_v40_to_v41(&mut conn).expect("migrate v40->v41");
    v40_to_v41::migrate_v40_to_v41(&mut conn).expect("repeat v40->v41");

    let existing = conn
        .query_row(
            r#"
SELECT capabilities_configured, supported_reasoning_efforts_json,
       default_reasoning_effort, context_window
FROM provider_models
WHERE model_uuid = 'old-model'
"#,
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .expect("read backfilled model");
    assert_eq!(
        existing,
        (
            1,
            r#"["low","medium","high"]"#.to_string(),
            Some("medium".to_string()),
            None,
        )
    );

    conn.execute(
        r#"
INSERT INTO provider_models(
  model_uuid, provider_id, remote_model_id, source, stale, created_at, updated_at
) VALUES ('new-model', 1, 'gpt-new', 'manual', 0, 2, 2)
"#,
        [],
    )
    .expect("insert new model");
    let new_model = conn
        .query_row(
            r#"
SELECT capabilities_configured, supported_reasoning_efforts_json,
       default_reasoning_effort, context_window
FROM provider_models
WHERE model_uuid = 'new-model'
"#,
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .expect("read new model defaults");
    assert_eq!(new_model, (0, "[]".to_string(), None, None));

    let context_error = conn
        .execute(
            "UPDATE provider_models SET context_window = 100 WHERE model_uuid = 'new-model'",
            [],
        )
        .expect_err("bounded context window must reject tiny values");
    assert!(context_error
        .to_string()
        .contains("CHECK constraint failed"));

    let user_version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user version");
    assert_eq!(user_version, 41);
}

#[test]
fn fresh_v41_schema_rejects_missing_or_noncanonical_provider_uuid() {
    let mut conn = Connection::open_in_memory().expect("open migration db");
    apply_migrations(&mut conn).expect("apply migrations");

    let missing = conn
        .execute(
            r#"
INSERT INTO providers(cli_key, name, base_url, api_key_plaintext, created_at, updated_at)
VALUES ('codex', 'missing', 'https://example.invalid/v1', 'key', 1, 1)
"#,
            [],
        )
        .expect_err("missing UUID must fail");
    assert!(missing.to_string().contains("provider_uuid"));

    for invalid in [
        "550E8400-E29B-41D4-A716-446655440000",
        "550e8400-e29b-11d4-a716-446655440000",
        "550e8400-e29b-41d4-c716-446655440000",
        "550e8400-e29b-41d4-a716-44665544000z",
        "550e8400-e29b-41d4-a716-44665544000-",
    ] {
        let error = conn
            .execute(
                r#"
INSERT INTO providers(
  provider_uuid, cli_key, name, base_url, api_key_plaintext, created_at, updated_at
) VALUES (?1, 'codex', ?1, 'https://example.invalid/v1', 'key', 1, 1)
"#,
                rusqlite::params![invalid],
            )
            .expect_err("invalid UUID must fail");
        assert!(
            error
                .to_string()
                .contains("provider_uuid must be a canonical UUID"),
            "unexpected error for {invalid}: {error}"
        );
    }
}
