//! SQLite migration v43->v44: remove legacy Codex translation providers.

use rusqlite::{params, Connection, Transaction};

const LEGACY_CODEX_BRIDGE_TYPES: [&str; 3] = [
    "codex_to_openai_chat",
    "codex_to_openai_responses",
    "codex_to_anthropic_messages",
];
const MANAGED_PROFILE_REFERENCE_ERROR: &str =
    "CODEX_PROVIDER_TRANSLATION_MANAGED_PROFILE_REFERENCED: a managed Codex profile references a removed Codex translation provider; repair the managed profile reference before retrying";

const PROVIDER_DEPENDENT_TABLES: [&str; 10] = [
    "provider_circuit_breakers",
    "provider_extension_values",
    "provider_model_catalogs",
    "provider_models",
    "provider_account_usage_credentials",
    "provider_pool_order",
    "default_route_providers",
    "sort_mode_providers",
    "claude_model_validation_runs",
    "provider_oauth_limit_snapshots",
];

pub(super) fn migrate_v43_to_v44(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v43->v44: {error}"))?;

    if !table_exists(&tx, "providers")? {
        return Err("v43->v44 requires the providers table".to_string());
    }

    if column_exists(&tx, "providers", "bridge_type")? {
        reject_managed_profile_references(&tx)?;
        clear_provider_dependents(&tx)?;

        if column_exists(&tx, "providers", "source_provider_id")? {
            tx.execute(
                r#"
UPDATE providers
SET source_provider_id = NULL
WHERE source_provider_id IN (
  SELECT id FROM providers
  WHERE bridge_type IN (?1, ?2, ?3)
)
"#,
                params![
                    LEGACY_CODEX_BRIDGE_TYPES[0],
                    LEGACY_CODEX_BRIDGE_TYPES[1],
                    LEGACY_CODEX_BRIDGE_TYPES[2],
                ],
            )
            .map_err(|error| {
                format!("failed to clear legacy Codex translation source references: {error}")
            })?;
        }

        tx.execute(
            "DELETE FROM providers WHERE bridge_type IN (?1, ?2, ?3)",
            params![
                LEGACY_CODEX_BRIDGE_TYPES[0],
                LEGACY_CODEX_BRIDGE_TYPES[1],
                LEGACY_CODEX_BRIDGE_TYPES[2],
            ],
        )
        .map_err(|error| format!("failed to remove legacy Codex translation providers: {error}"))?;
    }

    if column_exists(&tx, "providers", "model_mapping_json")? {
        tx.execute(
            "UPDATE providers SET model_mapping_json = '{}' WHERE model_mapping_json IS NULL OR model_mapping_json <> '{}'",
            [],
        )
        .map_err(|error| format!("failed to clear legacy provider model mappings: {error}"))?;
    }

    super::set_user_version(&tx, 44)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v43->v44: {error}"))?;
    Ok(())
}

fn reject_managed_profile_references(tx: &Transaction<'_>) -> Result<(), String> {
    let has_provider_models = table_exists(tx, "provider_models")?;
    let has_managed_profiles = table_exists(tx, "codex_managed_profiles")?;
    if has_provider_models != has_managed_profiles {
        return Err("v43->v44 managed profile schema is incomplete".to_string());
    }
    if !has_provider_models {
        return Ok(());
    }

    for (table, column) in [
        ("provider_models", "model_uuid"),
        ("provider_models", "provider_id"),
        ("codex_managed_profiles", "model_uuid"),
    ] {
        if !column_exists(tx, table, column)? {
            return Err("v43->v44 managed profile schema is incomplete".to_string());
        }
    }

    let referenced: bool = tx
        .query_row(
            r#"
SELECT EXISTS(
  SELECT 1
  FROM codex_managed_profiles profile
  JOIN provider_models model ON model.model_uuid = profile.model_uuid
  JOIN providers provider ON provider.id = model.provider_id
  WHERE provider.bridge_type IN (?1, ?2, ?3)
)
"#,
            params![
                LEGACY_CODEX_BRIDGE_TYPES[0],
                LEGACY_CODEX_BRIDGE_TYPES[1],
                LEGACY_CODEX_BRIDGE_TYPES[2],
            ],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect managed Codex profile references: {error}"))?;
    if referenced {
        return Err(MANAGED_PROFILE_REFERENCE_ERROR.to_string());
    }
    Ok(())
}

fn clear_provider_dependents(tx: &Transaction<'_>) -> Result<(), String> {
    for table in PROVIDER_DEPENDENT_TABLES {
        if !table_exists(tx, table)? || !column_exists(tx, table, "provider_id")? {
            continue;
        }
        tx.execute(
            &format!(
                "DELETE FROM {table} WHERE provider_id IN (SELECT id FROM providers WHERE bridge_type IN (?1, ?2, ?3))"
            ),
            params![
                LEGACY_CODEX_BRIDGE_TYPES[0],
                LEGACY_CODEX_BRIDGE_TYPES[1],
                LEGACY_CODEX_BRIDGE_TYPES[2],
            ],
        )
        .map_err(|error| {
            format!("failed to clear legacy Codex translation references in {table}: {error}")
        })?;
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(|error| format!("failed to inspect sqlite table {table}: {error}"))
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let sql = format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)");
    conn.query_row(&sql, [column], |row| row.get(0))
        .map_err(|error| format!("failed to inspect sqlite column {table}.{column}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_schema() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open migration database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        super::super::apply_migrations(&mut conn).expect("create current schema");
        conn
    }

    fn insert_provider(
        conn: &Connection,
        id: i64,
        cli_key: &str,
        name: &str,
        bridge_type: Option<&str>,
        source_provider_id: Option<i64>,
        model_mapping_json: &str,
    ) {
        conn.execute(
            r#"
INSERT INTO providers(
  id, provider_uuid, cli_key, name, base_url, api_key_plaintext,
  bridge_type, source_provider_id, model_mapping_json, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, 'https://example.invalid/v1', 'synthetic-key', ?5, ?6, ?7, 1, 1)
"#,
            params![
                id,
                crate::shared::uuid::new_uuid_v4(),
                cli_key,
                name,
                bridge_type,
                source_provider_id,
                model_mapping_json,
            ],
        )
        .expect("insert provider fixture");
    }

    fn set_v43(conn: &Connection) {
        conn.pragma_update(None, "user_version", 43_i64)
            .expect("set v43 user version");
    }

    #[test]
    fn removes_all_legacy_bridges_and_dependents_but_preserves_logs_and_survivors() {
        let mut conn = current_schema();
        insert_provider(
            &conn,
            1,
            "codex",
            "Source",
            None,
            None,
            r#"{"source":"legacy"}"#,
        );
        insert_provider(
            &conn,
            2,
            "codex",
            "Chat Bridge",
            Some(LEGACY_CODEX_BRIDGE_TYPES[0]),
            Some(1),
            r#"{"bridge":"chat"}"#,
        );
        insert_provider(
            &conn,
            3,
            "codex",
            "Responses Bridge",
            Some(LEGACY_CODEX_BRIDGE_TYPES[1]),
            Some(1),
            r#"{"bridge":"responses"}"#,
        );
        insert_provider(
            &conn,
            4,
            "codex",
            "Anthropic Bridge",
            Some(LEGACY_CODEX_BRIDGE_TYPES[2]),
            Some(1),
            r#"{"bridge":"anthropic"}"#,
        );
        insert_provider(
            &conn,
            5,
            "codex",
            "Direct",
            None,
            None,
            r#"{"direct":true}"#,
        );
        insert_provider(
            &conn,
            6,
            "claude",
            "CX2CC",
            Some("cx2cc"),
            Some(1),
            r#"{"cx2cc":true}"#,
        );
        insert_provider(
            &conn,
            7,
            "claude",
            "Ordinary",
            None,
            None,
            r#"{"ordinary":true}"#,
        );
        insert_provider(
            &conn,
            8,
            "codex",
            "Nested Reference",
            Some("plugin_bridge"),
            Some(2),
            r#"{"plugin":true}"#,
        );

        conn.execute_batch(
            r#"
INSERT INTO sort_modes(id, name, created_at, updated_at) VALUES (1, 'Legacy refs', 1, 1);
INSERT INTO provider_pool_order(cli_key, provider_id, sort_order, created_at, updated_at)
  VALUES ('codex', 2, 0, 1, 1), ('codex', 3, 1, 1, 1);
INSERT INTO default_route_providers(cli_key, provider_id, sort_order, created_at, updated_at)
  VALUES ('codex', 2, 0, 1, 1);
INSERT INTO sort_mode_providers(mode_id, cli_key, provider_id, sort_order, created_at, updated_at)
  VALUES (1, 'codex', 3, 0, 1, 1);
INSERT INTO provider_circuit_breakers(provider_id, state, updated_at)
  VALUES (2, 'closed', 1);
INSERT INTO provider_account_usage_credentials(provider_id, newapi_user_id, updated_at)
  VALUES (3, '42', 1);
INSERT INTO provider_model_catalogs(provider_id) VALUES (4);
INSERT INTO provider_models(model_uuid, provider_id, remote_model_id, source, created_at, updated_at)
  VALUES ('legacy-model', 4, 'legacy-model', 'manual', 1, 1);
INSERT INTO claude_model_validation_runs(provider_id, created_at, request_json, result_json)
  VALUES (2, 1, '{}', '{}');
INSERT INTO provider_oauth_limit_snapshots(
  provider_id, limit_short_label, limit_5h_text, limit_weekly_text,
  limit_5h_reset_at, limit_weekly_reset_at, checked_at, updated_at
) VALUES (3, '5h', '50%', '75%', NULL, NULL, 1, 1);
INSERT INTO request_logs(
  trace_id, cli_key, method, path, attempts_json, created_at,
  final_provider_id, duration_ms
) VALUES (
  'legacy-log', 'codex', 'POST', '/v1/responses',
  '[{"provider_id":2,"provider_name":"Chat Bridge"}]', 1, 2, 1
);
"#,
        )
        .expect("seed dependent fixtures");
        set_v43(&conn);

        migrate_v43_to_v44(&mut conn).expect("migrate v43->v44");
        migrate_v43_to_v44(&mut conn).expect("repeat v43->v44");

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read user version");
        assert_eq!(version, 44);
        let surviving_ids = conn
            .prepare("SELECT id FROM providers ORDER BY id")
            .expect("prepare survivor query")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query survivors")
            .collect::<Result<Vec<_>, _>>()
            .expect("read survivors");
        assert_eq!(surviving_ids, vec![1, 5, 6, 7, 8]);

        let cx2cc: (Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT bridge_type, source_provider_id FROM providers WHERE id = 6",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read CX2CC provider");
        assert_eq!(cx2cc, (Some("cx2cc".to_string()), Some(1)));
        let nested_source: Option<i64> = conn
            .query_row(
                "SELECT source_provider_id FROM providers WHERE id = 8",
                [],
                |row| row.get(0),
            )
            .expect("read nested source reference");
        assert_eq!(nested_source, None);

        let non_empty_mappings: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE model_mapping_json <> '{}'",
                [],
                |row| row.get(0),
            )
            .expect("count legacy mappings");
        assert_eq!(non_empty_mappings, 0);

        let log: (i64, String) = conn
            .query_row(
                "SELECT final_provider_id, attempts_json FROM request_logs WHERE trace_id = 'legacy-log'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read preserved request log");
        assert_eq!(log.0, 2);
        assert!(log.1.contains("Chat Bridge"));
        let foreign_key_violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .expect("check foreign keys");
        assert_eq!(foreign_key_violations, 0);
    }

    #[test]
    fn managed_profile_reference_aborts_the_entire_migration_without_sensitive_details() {
        let mut conn = current_schema();
        insert_provider(
            &conn,
            1,
            "codex",
            "Legacy Bridge",
            Some(LEGACY_CODEX_BRIDGE_TYPES[1]),
            None,
            r#"{"legacy":true}"#,
        );
        insert_provider(&conn, 2, "codex", "Direct", None, None, r#"{"keep":true}"#);
        conn.execute_batch(
            r#"
INSERT INTO provider_models(model_uuid, provider_id, remote_model_id, source, created_at, updated_at)
  VALUES ('sensitive-model-marker', 1, 'sensitive-remote-marker', 'manual', 1, 1);
INSERT INTO codex_managed_profiles(
  profile_uuid, profile_name, profile_name_key, model_uuid, codex_home_path,
  content_sha256, created_at, updated_at
) VALUES (
  'sensitive-profile-uuid', 'sensitive-profile-name', 'sensitive-profile-key',
  'sensitive-model-marker', 'C:/sensitive/codex/home', 'sensitive-hash', 1, 1
);
"#,
        )
        .expect("seed managed profile reference");
        set_v43(&conn);

        let error = migrate_v43_to_v44(&mut conn)
            .expect_err("managed profile reference must block migration");
        assert_eq!(error, MANAGED_PROFILE_REFERENCE_ERROR);
        for sensitive in [
            "sensitive-model-marker",
            "sensitive-profile-name",
            "C:/sensitive/codex/home",
        ] {
            assert!(!error.contains(sensitive));
        }
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read user version after failure");
        assert_eq!(version, 43);
        let provider_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .expect("count preserved providers");
        assert_eq!(provider_count, 2);
        let profile_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM codex_managed_profiles", [], |row| {
                row.get(0)
            })
            .expect("count preserved profiles");
        assert_eq!(profile_count, 1);
    }

    #[test]
    fn tolerates_a_minimal_v43_provider_shape_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("open minimal migration database");
        conn.execute_batch(
            r#"
CREATE TABLE providers(id INTEGER PRIMARY KEY, bridge_type TEXT);
INSERT INTO providers(id, bridge_type) VALUES (1, 'codex_to_openai_chat'), (2, NULL);
PRAGMA user_version = 43;
"#,
        )
        .expect("create minimal v43 fixture");

        migrate_v43_to_v44(&mut conn).expect("migrate minimal v43");
        migrate_v43_to_v44(&mut conn).expect("repeat minimal migration");
        let ids = conn
            .prepare("SELECT id FROM providers ORDER BY id")
            .expect("prepare minimal survivors")
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query minimal survivors")
            .collect::<Result<Vec<_>, _>>()
            .expect("read minimal survivors");
        assert_eq!(ids, vec![2]);
    }
}
