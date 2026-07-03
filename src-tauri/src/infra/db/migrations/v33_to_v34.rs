//! Usage: SQLite migration v33->v34 - Add structured plugin hook execution reports.

use crate::shared::time::now_unix_seconds;
use rusqlite::Connection;

pub(super) fn migrate_v33_to_v34(conn: &mut Connection) -> Result<(), String> {
    const VERSION: i64 = 34;
    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start sqlite transaction: {e}"))?;
    let now = now_unix_seconds();

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS plugin_hook_execution_reports (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  plugin_id TEXT NOT NULL,
  trace_id TEXT,
  hook_name TEXT NOT NULL,
  runtime_kind TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at_ms INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  failure_kind TEXT,
  error_code TEXT,
  failure_policy TEXT,
  circuit_state TEXT,
  context_budget_json TEXT NOT NULL DEFAULT '{}',
  output_budget_json TEXT NOT NULL DEFAULT '{}',
  mutation_summary_json TEXT NOT NULL DEFAULT '{}',
  replayable INTEGER NOT NULL DEFAULT 0,
  replay_export_reason TEXT,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_plugin_hook_execution_reports_plugin_created_at
  ON plugin_hook_execution_reports(plugin_id, created_at);
CREATE INDEX IF NOT EXISTS idx_plugin_hook_execution_reports_created_at
  ON plugin_hook_execution_reports(created_at);
CREATE INDEX IF NOT EXISTS idx_plugin_hook_execution_reports_trace_id
  ON plugin_hook_execution_reports(trace_id);
CREATE INDEX IF NOT EXISTS idx_plugin_hook_execution_reports_plugin_hook_created_at
  ON plugin_hook_execution_reports(plugin_id, hook_name, created_at);
"#,
    )
    .map_err(|e| format!("failed to create plugin hook execution report table: {e}"))?;

    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )
    .map_err(|e| format!("failed to create schema_migrations table: {e}"))?;
    tx.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?, ?)",
        [VERSION, now],
    )
    .map_err(|e| format!("failed to insert schema_migrations row for v{VERSION}: {e}"))?;

    super::set_user_version(&tx, VERSION)?;

    tx.commit()
        .map_err(|e| format!("failed to commit sqlite transaction: {e}"))?;

    Ok(())
}
