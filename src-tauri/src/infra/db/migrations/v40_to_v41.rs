//! Usage: SQLite migration v40->v41 - Provider-model Codex capability metadata.

use rusqlite::Connection;

pub(super) fn migrate_v40_to_v41(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v40->v41: {error}"))?;

    let has_provider_models: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'provider_models'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect provider_models table: {error}"))?;
    if has_provider_models == 0 {
        return Err("v40->v41 requires the provider_models table".to_string());
    }

    let has_capabilities_configured = has_column(&tx, "capabilities_configured")?;
    if !has_capabilities_configured {
        tx.execute_batch(
            r#"
ALTER TABLE provider_models ADD COLUMN capabilities_configured INTEGER NOT NULL DEFAULT 0
  CHECK(capabilities_configured IN (0, 1));
ALTER TABLE provider_models ADD COLUMN supported_reasoning_efforts_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE provider_models ADD COLUMN default_reasoning_effort TEXT
  CHECK(default_reasoning_effort IS NULL OR default_reasoning_effort IN (
    'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'
  ));
ALTER TABLE provider_models ADD COLUMN context_window INTEGER
  CHECK(context_window IS NULL OR (
    typeof(context_window) = 'integer' AND context_window BETWEEN 1024 AND 10000000
  ));

UPDATE provider_models
SET capabilities_configured = 1,
    supported_reasoning_efforts_json = '["low","medium","high"]',
    default_reasoning_effort = 'medium',
    context_window = NULL;
"#,
        )
        .map_err(|error| format!("failed to add provider model capabilities: {error}"))?;
    } else {
        for column in [
            "supported_reasoning_efforts_json",
            "default_reasoning_effort",
            "context_window",
        ] {
            if !has_column(&tx, column)? {
                return Err("existing provider model capability schema is incomplete".to_string());
            }
        }
    }

    super::set_user_version(&tx, 41)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v40->v41: {error}"))?;
    Ok(())
}

fn has_column(conn: &Connection, column: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('provider_models') WHERE name = ?1)",
        [column],
        |row| row.get(0),
    )
    .map_err(|error| format!("failed to inspect provider model capability schema: {error}"))
}
