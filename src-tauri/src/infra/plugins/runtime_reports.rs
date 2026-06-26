//! Usage: Structured plugin hook execution report persistence.

use crate::db;
use crate::domain::plugins::PluginHookExecutionReport;
use crate::shared::error::{db_err, AppResult};
use crate::shared::time::now_unix_seconds;
use rusqlite::{params, params_from_iter, types::Value};

const DEFAULT_PLUGIN_RUNTIME_REPORT_RETENTION_DAYS: i64 = 30;
const DEFAULT_PLUGIN_RUNTIME_REPORTS_PER_PLUGIN: usize = 5_000;
const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Debug, Clone)]
pub(crate) struct RecordPluginHookExecutionReportInput {
    pub(crate) plugin_id: String,
    pub(crate) trace_id: Option<String>,
    pub(crate) hook_name: String,
    pub(crate) runtime_kind: String,
    pub(crate) status: String,
    pub(crate) started_at_ms: i64,
    pub(crate) duration_ms: i64,
    pub(crate) failure_kind: Option<String>,
    pub(crate) error_code: Option<String>,
    pub(crate) failure_policy: Option<String>,
    pub(crate) circuit_state: Option<String>,
    pub(crate) context_budget_json: serde_json::Value,
    pub(crate) output_budget_json: serde_json::Value,
    pub(crate) mutation_summary_json: serde_json::Value,
    pub(crate) replayable: bool,
    pub(crate) replay_export_reason: Option<String>,
}

pub(crate) fn record_hook_execution_report(
    db: &db::Db,
    input: RecordPluginHookExecutionReportInput,
) -> AppResult<PluginHookExecutionReport> {
    let conn = db.open_connection()?;
    record_hook_execution_report_with_conn(&conn, input)
}

fn record_hook_execution_report_with_conn(
    conn: &rusqlite::Connection,
    input: RecordPluginHookExecutionReportInput,
) -> AppResult<PluginHookExecutionReport> {
    let context_budget_json = serde_json::to_string(&input.context_budget_json).map_err(|e| {
        format!("PLUGIN_RUNTIME_REPORT_INVALID: failed to serialize context budget: {e}")
    })?;
    let output_budget_json = serde_json::to_string(&input.output_budget_json).map_err(|e| {
        format!("PLUGIN_RUNTIME_REPORT_INVALID: failed to serialize output budget: {e}")
    })?;
    let mutation_summary_json =
        serde_json::to_string(&input.mutation_summary_json).map_err(|e| {
            format!("PLUGIN_RUNTIME_REPORT_INVALID: failed to serialize mutation summary: {e}")
        })?;
    let now = now_unix_seconds();

    conn.execute(
        r#"
INSERT INTO plugin_hook_execution_reports(
  plugin_id,
  trace_id,
  hook_name,
  runtime_kind,
  status,
  started_at_ms,
  duration_ms,
  failure_kind,
  error_code,
  failure_policy,
  circuit_state,
  context_budget_json,
  output_budget_json,
  mutation_summary_json,
  replayable,
  replay_export_reason,
  created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
"#,
        params![
            input.plugin_id,
            input.trace_id,
            input.hook_name,
            input.runtime_kind,
            input.status,
            input.started_at_ms,
            input.duration_ms,
            input.failure_kind,
            input.error_code,
            input.failure_policy,
            input.circuit_state,
            context_budget_json,
            output_budget_json,
            mutation_summary_json,
            if input.replayable { 1 } else { 0 },
            input.replay_export_reason,
            now,
        ],
    )
    .map_err(|e| db_err!("failed to record plugin hook execution report: {e}"))?;

    let id = conn.last_insert_rowid();
    let report = get_hook_execution_report_by_id(conn, id)?;
    prune_after_insert_best_effort(conn, &report.plugin_id, now);
    Ok(report)
}

fn prune_hook_execution_reports_for_plugin_with_conn(
    conn: &rusqlite::Connection,
    plugin_id: &str,
    max_reports: usize,
) -> AppResult<usize> {
    let deleted = conn
        .execute(
            r#"
DELETE FROM plugin_hook_execution_reports
WHERE id IN (
  SELECT id
  FROM plugin_hook_execution_reports
  WHERE plugin_id = ?1
  ORDER BY created_at DESC, id DESC
  LIMIT -1 OFFSET ?2
)
"#,
            params![plugin_id, max_reports as i64],
        )
        .map_err(|e| db_err!("failed to prune plugin hook execution reports by cap: {e}"))?;
    Ok(deleted)
}

fn prune_hook_execution_reports_before_with_conn(
    conn: &rusqlite::Connection,
    cutoff_created_at: i64,
) -> AppResult<usize> {
    let deleted = conn
        .execute(
            "DELETE FROM plugin_hook_execution_reports WHERE created_at < ?1",
            params![cutoff_created_at],
        )
        .map_err(|e| db_err!("failed to prune old plugin hook execution reports: {e}"))?;
    Ok(deleted)
}

fn prune_after_insert_best_effort(conn: &rusqlite::Connection, plugin_id: &str, now: i64) {
    let cutoff = now - DEFAULT_PLUGIN_RUNTIME_REPORT_RETENTION_DAYS * SECONDS_PER_DAY;
    if let Err(err) = prune_hook_execution_reports_before_with_conn(conn, cutoff) {
        tracing::warn!(error = %err, "failed to prune old plugin hook execution reports");
    }
    if let Err(err) = prune_hook_execution_reports_for_plugin_with_conn(
        conn,
        plugin_id,
        DEFAULT_PLUGIN_RUNTIME_REPORTS_PER_PLUGIN,
    ) {
        tracing::warn!(
            plugin_id,
            error = %err,
            "failed to prune plugin hook execution reports by cap"
        );
    }
}

pub(crate) fn list_hook_execution_reports(
    db: &db::Db,
    plugin_id: Option<&str>,
    hook_name: Option<&str>,
    trace_id: Option<&str>,
    limit: usize,
) -> AppResult<Vec<PluginHookExecutionReport>> {
    let conn = db.open_connection()?;
    let limit = limit.clamp(1, 500) as i64;

    let mut sql = String::from(
        r#"
SELECT
  id,
  plugin_id,
  trace_id,
  hook_name,
  runtime_kind,
  status,
  started_at_ms,
  duration_ms,
  failure_kind,
  error_code,
  failure_policy,
  circuit_state,
  context_budget_json,
  output_budget_json,
  mutation_summary_json,
  replayable,
  replay_export_reason,
  created_at
FROM plugin_hook_execution_reports
"#,
    );
    let mut conditions = Vec::new();
    if plugin_id.is_some() {
        conditions.push("plugin_id = ?");
    }
    if hook_name.is_some() {
        conditions.push("hook_name = ?");
    }
    if trace_id.is_some() {
        conditions.push("trace_id = ?");
    }
    if !conditions.is_empty() {
        sql.push_str("WHERE ");
        sql.push_str(&conditions.join(" AND "));
        sql.push('\n');
    }
    sql.push_str("ORDER BY created_at DESC, id DESC\nLIMIT ?");

    let mut stmt = conn
        .prepare_cached(&sql)
        .map_err(|e| db_err!("failed to prepare plugin hook execution report query: {e}"))?;
    let mut values = Vec::new();
    if let Some(plugin_id) = plugin_id {
        values.push(Value::Text(plugin_id.to_string()));
    }
    if let Some(hook_name) = hook_name {
        values.push(Value::Text(hook_name.to_string()));
    }
    if let Some(trace_id) = trace_id {
        values.push(Value::Text(trace_id.to_string()));
    }
    values.push(Value::Integer(limit));

    let rows = stmt
        .query_map(params_from_iter(values), hook_execution_report_from_row)
        .map_err(|e| db_err!("failed to query plugin hook execution reports: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| db_err!("failed to read plugin hook execution report: {e}"))?);
    }
    Ok(out)
}

fn get_hook_execution_report_by_id(
    conn: &rusqlite::Connection,
    id: i64,
) -> AppResult<PluginHookExecutionReport> {
    conn.query_row(
        r#"
SELECT
  id,
  plugin_id,
  trace_id,
  hook_name,
  runtime_kind,
  status,
  started_at_ms,
  duration_ms,
  failure_kind,
  error_code,
  failure_policy,
  circuit_state,
  context_budget_json,
  output_budget_json,
  mutation_summary_json,
  replayable,
  replay_export_reason,
  created_at
FROM plugin_hook_execution_reports
WHERE id = ?1
"#,
        params![id],
        hook_execution_report_from_row,
    )
    .map_err(|e| db_err!("failed to query inserted plugin hook execution report: {e}"))
}

fn hook_execution_report_from_row(
    row: &rusqlite::Row<'_>,
) -> Result<PluginHookExecutionReport, rusqlite::Error> {
    let context_budget_json: String = row.get("context_budget_json")?;
    let output_budget_json: String = row.get("output_budget_json")?;
    let mutation_summary_json: String = row.get("mutation_summary_json")?;
    let replayable: i64 = row.get("replayable")?;
    Ok(PluginHookExecutionReport {
        id: row.get("id")?,
        plugin_id: row.get("plugin_id")?,
        trace_id: row.get("trace_id")?,
        hook_name: row.get("hook_name")?,
        runtime_kind: row.get("runtime_kind")?,
        status: row.get("status")?,
        started_at_ms: row.get("started_at_ms")?,
        duration_ms: row.get("duration_ms")?,
        failure_kind: row.get("failure_kind")?,
        error_code: row.get("error_code")?,
        failure_policy: row.get("failure_policy")?,
        circuit_state: row.get("circuit_state")?,
        context_budget: parse_json_value(&context_budget_json),
        output_budget: parse_json_value(&output_budget_json),
        mutation_summary: parse_json_value(&mutation_summary_json),
        replayable: replayable != 0,
        replay_export_reason: row.get("replay_export_reason")?,
        created_at: row.get("created_at")?,
    })
}

fn parse_json_value(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_input(plugin_id: &str, trace_id: &str) -> RecordPluginHookExecutionReportInput {
        RecordPluginHookExecutionReportInput {
            plugin_id: plugin_id.to_string(),
            trace_id: Some(trace_id.to_string()),
            hook_name: "gateway.request.afterBodyRead".to_string(),
            runtime_kind: "declarativeRules".to_string(),
            status: "completed".to_string(),
            started_at_ms: 1000,
            duration_ms: 5,
            failure_kind: None,
            error_code: None,
            failure_policy: Some("fail-open".to_string()),
            circuit_state: Some("closed".to_string()),
            context_budget_json: serde_json::json!({ "messages": 1 }),
            output_budget_json: serde_json::json!({ "bytes": 1 }),
            mutation_summary_json: serde_json::json!({ "changed": false }),
            replayable: true,
            replay_export_reason: None,
        }
    }

    #[test]
    fn repository_prunes_plugin_hook_execution_reports_by_plugin_cap() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        for index in 0..5 {
            record_hook_execution_report(
                &db,
                report_input("community.cap", &format!("trace-{index}")),
            )
            .unwrap();
        }

        let conn = db.open_connection().unwrap();
        let deleted =
            prune_hook_execution_reports_for_plugin_with_conn(&conn, "community.cap", 3).unwrap();
        drop(conn);

        let reports =
            list_hook_execution_reports(&db, Some("community.cap"), None, None, 10).unwrap();

        assert_eq!(deleted, 2);
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].trace_id.as_deref(), Some("trace-4"));
        assert_eq!(reports[2].trace_id.as_deref(), Some("trace-2"));
    }

    #[test]
    fn repository_prunes_plugin_hook_execution_reports_before_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        record_hook_execution_report(&db, report_input("community.age", "trace-old")).unwrap();
        record_hook_execution_report(&db, report_input("community.age", "trace-new")).unwrap();

        let conn = db.open_connection().unwrap();
        conn.execute(
            "UPDATE plugin_hook_execution_reports SET created_at = ?1 WHERE trace_id = ?2",
            rusqlite::params![1_i64, "trace-old"],
        )
        .unwrap();
        drop(conn);

        let conn = db.open_connection().unwrap();
        let deleted = prune_hook_execution_reports_before_with_conn(&conn, 2).unwrap();
        drop(conn);

        let reports =
            list_hook_execution_reports(&db, Some("community.age"), None, None, 10).unwrap();

        assert_eq!(deleted, 1);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].trace_id.as_deref(), Some("trace-new"));
    }

    #[test]
    fn repository_auto_prunes_old_plugin_hook_execution_reports_after_recording() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        record_hook_execution_report(&db, report_input("community.auto-age", "trace-old")).unwrap();

        let conn = db.open_connection().unwrap();
        conn.execute(
            "UPDATE plugin_hook_execution_reports SET created_at = ?1 WHERE trace_id = ?2",
            rusqlite::params![1_i64, "trace-old"],
        )
        .unwrap();
        drop(conn);

        let new_report =
            record_hook_execution_report(&db, report_input("community.auto-age", "trace-new"))
                .unwrap();
        let reports =
            list_hook_execution_reports(&db, Some("community.auto-age"), None, None, 10).unwrap();

        assert_eq!(new_report.trace_id.as_deref(), Some("trace-new"));
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].trace_id.as_deref(), Some("trace-new"));
    }

    #[test]
    fn repository_records_and_lists_plugin_hook_execution_reports() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        let report = record_hook_execution_report(
            &db,
            RecordPluginHookExecutionReportInput {
                plugin_id: "community.prompt-helper".to_string(),
                trace_id: Some("trace-report-1".to_string()),
                hook_name: "gateway.request.afterBodyRead".to_string(),
                runtime_kind: "declarativeRules".to_string(),
                status: "completed".to_string(),
                started_at_ms: 1_000,
                duration_ms: 17,
                failure_kind: None,
                error_code: None,
                failure_policy: Some("fail-open".to_string()),
                circuit_state: Some("closed".to_string()),
                context_budget_json: serde_json::json!({"bodyBytes": 4096}),
                output_budget_json: serde_json::json!({"bodyBytes": 2048}),
                mutation_summary_json: serde_json::json!({"changed": true, "field": "requestBody"}),
                replayable: true,
                replay_export_reason: None,
            },
        )
        .unwrap();

        let list = list_hook_execution_reports(
            &db,
            Some("community.prompt-helper"),
            Some("gateway.request.afterBodyRead"),
            Some("trace-report-1"),
            50,
        )
        .unwrap();

        assert_eq!(report.plugin_id, "community.prompt-helper");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, "completed");
        assert_eq!(list[0].mutation_summary["field"], "requestBody");
    }
}
