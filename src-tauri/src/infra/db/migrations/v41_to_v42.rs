//! Usage: SQLite migration v41->v42 - Persist circuit probe protection deadlines.

use rusqlite::Connection;

const PROBE_COLUMNS: [(&str, &str); 5] = [
    ("probe_reference_at", "INTEGER"),
    ("next_probe_at", "INTEGER"),
    ("natural_probe_due_at", "INTEGER"),
    ("recovery_guard_until", "INTEGER"),
    (
        "state_revision",
        "INTEGER NOT NULL DEFAULT 0 CHECK(state_revision >= 0)",
    ),
];

pub(super) fn migrate_v41_to_v42(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v41->v42: {error}"))?;

    let has_table: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'provider_circuit_breakers'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect provider_circuit_breakers table: {error}"))?;
    if has_table == 0 {
        tx.execute_batch(
            r#"
CREATE TABLE provider_circuit_breakers (
  provider_id INTEGER PRIMARY KEY,
  state TEXT NOT NULL,
  failure_count INTEGER NOT NULL DEFAULT 0,
  failure_timestamps_json TEXT NOT NULL DEFAULT '[]',
  half_open_success_count INTEGER NOT NULL DEFAULT 0,
  open_until INTEGER,
  probe_reference_at INTEGER,
  next_probe_at INTEGER,
  natural_probe_due_at INTEGER,
  recovery_guard_until INTEGER,
  state_revision INTEGER NOT NULL DEFAULT 0 CHECK(state_revision >= 0),
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
"#,
        )
        .map_err(|error| format!("failed to create provider_circuit_breakers table: {error}"))?;
    } else {
        for (column, definition) in PROBE_COLUMNS {
            if !has_column(&tx, column)? {
                tx.execute_batch(&format!(
                    "ALTER TABLE provider_circuit_breakers ADD COLUMN {column} {definition};"
                ))
                .map_err(|error| {
                    format!("failed to add provider_circuit_breakers.{column}: {error}")
                })?;
            }
        }
    }
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_provider_circuit_breakers_state ON provider_circuit_breakers(state);",
    )
    .map_err(|error| format!("failed to index provider_circuit_breakers.state: {error}"))?;

    super::set_user_version(&tx, 42)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v41->v42: {error}"))?;
    Ok(())
}

fn has_column(conn: &Connection, column: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('provider_circuit_breakers') WHERE name = ?1)",
        [column],
        |row| row.get(0),
    )
    .map_err(|error| format!("failed to inspect circuit probe schema: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_legacy_table_is_created_with_probe_deadlines() {
        let mut conn = Connection::open_in_memory().expect("open migration db");
        conn.execute_batch(
            r#"
PRAGMA foreign_keys = ON;
CREATE TABLE providers(id INTEGER PRIMARY KEY);
PRAGMA user_version = 41;
"#,
        )
        .expect("create v41 fixture");

        migrate_v41_to_v42(&mut conn).expect("migrate v41->v42");

        for (column, _) in PROBE_COLUMNS {
            assert!(has_column(&conn, column).expect("inspect migrated column"));
        }
        let index_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_provider_circuit_breakers_state')",
                [],
                |row| row.get(0),
            )
            .expect("inspect state index");
        assert!(index_exists);
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read schema version");
        assert_eq!(version, 42);
    }
}
