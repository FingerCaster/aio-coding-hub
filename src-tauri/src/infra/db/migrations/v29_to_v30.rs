//! Usage: SQLite migration v29->v30 - add OAuth account schema.

use crate::shared::time::now_unix_seconds;
use rusqlite::Connection;

pub(super) fn migrate_v29_to_v30(conn: &mut Connection) -> Result<(), String> {
    const VERSION: i64 = 30;

    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start sqlite transaction: {e}"))?;

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS oauth_accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  label TEXT NOT NULL,
  email TEXT,
  provider_type TEXT NOT NULL,
  access_token TEXT NOT NULL,
  refresh_token TEXT,
  id_token TEXT,
  token_uri TEXT,
  client_id TEXT,
  client_secret TEXT,
  expires_at INTEGER,
  refresh_lead_s INTEGER NOT NULL DEFAULT 3600,
  status TEXT NOT NULL DEFAULT 'active',
  last_error TEXT,
  refresh_success_count INTEGER NOT NULL DEFAULT 0,
  refresh_failure_count INTEGER NOT NULL DEFAULT 0,
  last_refreshed_at INTEGER,
  quota_exceeded INTEGER NOT NULL DEFAULT 0,
  quota_recover_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(cli_key, label)
);

CREATE INDEX IF NOT EXISTS idx_oauth_accounts_cli_key
  ON oauth_accounts(cli_key);
CREATE INDEX IF NOT EXISTS idx_oauth_accounts_status
  ON oauth_accounts(status);
"#,
    )
    .map_err(|e| format!("failed to create oauth_accounts schema: {e}"))?;

    if !column_exists(&tx, "oauth_accounts", "id_token")? {
        tx.execute_batch("ALTER TABLE oauth_accounts ADD COLUMN id_token TEXT;")
            .map_err(|e| format!("failed to add oauth_accounts.id_token: {e}"))?;
    }

    if table_exists(&tx, "providers")? {
        if !column_exists(&tx, "providers", "auth_mode")? {
            tx.execute_batch(
                "ALTER TABLE providers ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'api_key';",
            )
            .map_err(|e| format!("failed to add providers.auth_mode: {e}"))?;
        }
        if !column_exists(&tx, "providers", "oauth_account_id")? {
            tx.execute_batch("ALTER TABLE providers ADD COLUMN oauth_account_id INTEGER;")
                .map_err(|e| format!("failed to add providers.oauth_account_id: {e}"))?;
        }
    }
    if table_exists(&tx, "request_logs")? {
        if !column_exists(&tx, "request_logs", "oauth_account_id")? {
            tx.execute_batch("ALTER TABLE request_logs ADD COLUMN oauth_account_id INTEGER;")
                .map_err(|e| format!("failed to add request_logs.oauth_account_id: {e}"))?;
        }

        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_oauth_account ON request_logs(oauth_account_id, created_at);",
        )
        .map_err(|e| format!("failed to create idx_request_logs_oauth_account: {e}"))?;
    }

    let applied_at = now_unix_seconds();
    tx.execute(
        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
        (VERSION, applied_at),
    )
    .map_err(|e| format!("failed to record migration: {e}"))?;

    super::set_user_version(&tx, VERSION)?;

    tx.commit()
        .map_err(|e| format!("failed to commit migration: {e}"))?;

    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    use rusqlite::OptionalExtension;
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| format!("failed to check table existence for {table}: {e}"))?
        .unwrap_or(false);
    Ok(exists)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("failed to prepare table_info for {table}: {e}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| format!("failed to query table_info for {table}: {e}"))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| format!("failed to read table_info for {table}: {e}"))?
    {
        let name: String = row
            .get(1)
            .map_err(|e| format!("failed to read table_info column name: {e}"))?;
        if name == column {
            return Ok(true);
        }
    }

    Ok(false)
}
