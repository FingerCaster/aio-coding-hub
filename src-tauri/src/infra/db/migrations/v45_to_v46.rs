//! SQLite migration v45->v46: bounded Provider usage attribution for Codex infinite retry.

use rusqlite::Connection;

pub(super) fn migrate_v45_to_v46(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v45->v46: {error}"))?;
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS infinite_retry_provider_usage (
  trace_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  provider_id INTEGER,
  provider_name TEXT NOT NULL,
  attempt_count TEXT NOT NULL,
  usage_known_attempts TEXT NOT NULL,
  usage_unknown_attempts TEXT NOT NULL,
  input_tokens TEXT,
  output_tokens TEXT,
  total_tokens TEXT,
  reasoning_tokens TEXT,
  cache_read_input_tokens TEXT,
  cache_creation_input_tokens TEXT,
  cache_creation_5m_input_tokens TEXT,
  cache_creation_1h_input_tokens TEXT,
  cost_usd_femto TEXT,
  unpriced_attempts TEXT NOT NULL,
  complete INTEGER NOT NULL DEFAULT 0 CHECK(complete IN (0, 1)),
  overflowed INTEGER NOT NULL DEFAULT 0 CHECK(overflowed IN (0, 1)),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(trace_id, provider_key)
);
CREATE INDEX IF NOT EXISTS idx_infinite_retry_provider_usage_provider
  ON infinite_retry_provider_usage(provider_id, updated_at);
"#,
    )
    .map_err(|error| format!("failed to create infinite retry provider usage table: {error}"))?;
    super::set_user_version(&tx, 46)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v45->v46: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_bounded_attribution_table_idempotently() {
        let mut conn = Connection::open_in_memory().expect("open migration database");
        conn.execute_batch("PRAGMA user_version = 45;")
            .expect("create v45 fixture");

        migrate_v45_to_v46(&mut conn).expect("migrate v45->v46");
        migrate_v45_to_v46(&mut conn).expect("repeat v45->v46");

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read version");
        let table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='infinite_retry_provider_usage')",
                [],
                |row| row.get(0),
            )
            .expect("inspect table");
        assert_eq!(version, 46);
        assert!(table_exists);
    }
}
