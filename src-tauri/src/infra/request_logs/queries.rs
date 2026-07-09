//! Usage: Request log queries and attempts decoding.

use crate::db;
use crate::shared::error::db_err;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use super::costing::cost_usd_from_femto;
use super::{
    CodexReasoningContinuationStatusStat, CodexReasoningGuardModelEffortStat,
    CodexReasoningGuardModelStat, CodexReasoningGuardStats, RequestLogDetail, RequestLogRouteHop,
    RequestLogSummary,
};

const CLAUDE_VISIBLE_LOG_PATH: &str = "/v1/messages";
const CLAUDE_VISIBLE_LOG_CONDITION: &str = "(cli_key != 'claude' OR path = '/v1/messages')";
const UNKNOWN_CODEX_REQUESTED_MODEL_LABEL: &str = "未识别模型";
const UNKNOWN_CODEX_REASONING_EFFORT_LABEL: &str = "unknown";

/// Common SELECT fields for request_logs queries (summary view).
const REQUEST_LOG_SUMMARY_FIELDS: &str = "
  id,
  trace_id,
  cli_key,
  session_id,
  method,
  path,
  excluded_from_stats,
  special_settings_json,
  requested_model,
  status,
  error_code,
  duration_ms,
  ttfb_ms,
  visible_ttfb_ms,
  attempts_json,
  input_tokens,
  output_tokens,
  total_tokens,
  cache_read_input_tokens,
  cache_creation_input_tokens,
  cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens,
  cost_usd_femto,
  cost_multiplier,
  created_at_ms,
  last_activity_ms,
  activity_details_json,
  created_at,
  provider_chain_json,
  error_details_json
";

/// Common SELECT fields for request_logs queries (detail view).
const REQUEST_LOG_DETAIL_FIELDS: &str = "
  id,
  trace_id,
  cli_key,
  session_id,
  method,
  path,
  query,
  excluded_from_stats,
  special_settings_json,
  status,
  error_code,
  duration_ms,
  ttfb_ms,
  visible_ttfb_ms,
  attempts_json,
  input_tokens,
  output_tokens,
  total_tokens,
  cache_read_input_tokens,
  cache_creation_input_tokens,
  cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens,
  usage_json,
  requested_model,
  cost_usd_femto,
  cost_multiplier,
  created_at_ms,
  last_activity_ms,
  activity_details_json,
  created_at,
  provider_chain_json,
  error_details_json
";

pub(super) fn validate_cli_key(cli_key: &str) -> Result<(), String> {
    crate::shared::cli_key::validate_cli_key(cli_key)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(super) struct AttemptRow {
    provider_id: i64,
    provider_name: String,
    outcome: String,
    status: Option<i64>,
    error_code: Option<String>,
    decision: Option<String>,
    reason: Option<String>,
    session_reuse: Option<bool>,
}

pub(super) fn parse_attempts(attempts_json: &str) -> Vec<AttemptRow> {
    serde_json::from_str(attempts_json).unwrap_or_default()
}

pub(super) fn start_provider_from_attempts(attempts: &[AttemptRow]) -> (i64, String) {
    if attempts.iter().all(|a| a.outcome == "skipped") {
        return (0, "Unknown".to_string());
    }

    let first = attempts
        .iter()
        .find(|a| a.outcome != "skipped")
        .or_else(|| attempts.first());

    match first {
        Some(a) => (a.provider_id, a.provider_name.clone()),
        None => (0, "Unknown".to_string()),
    }
}

pub(super) fn final_provider_from_attempts(attempts: &[AttemptRow]) -> (i64, String) {
    if attempts.iter().all(|a| a.outcome == "skipped") {
        return (0, "Unknown".to_string());
    }

    let picked = attempts
        .iter()
        .rev()
        .find(|a| a.outcome == "success")
        .or_else(|| attempts.iter().rev().find(|a| a.outcome != "skipped"))
        .or_else(|| attempts.last());

    match picked {
        Some(a) => (a.provider_id, a.provider_name.clone()),
        None => (0, "Unknown".to_string()),
    }
}

pub(super) fn route_from_attempts(attempts: &[AttemptRow]) -> Vec<RequestLogRouteHop> {
    let mut out: Vec<RequestLogRouteHop> = Vec::new();
    let mut last_provider_id: i64 = 0;
    let mut last_hop_attempt_count: i64 = 0;
    for attempt in attempts {
        if attempt.provider_id <= 0 {
            continue;
        }
        if attempt.provider_id == last_provider_id {
            // 同一 provider 连续尝试，累加计数
            last_hop_attempt_count += 1;
            if let Some(hop) = out.last_mut() {
                hop.attempts = last_hop_attempt_count;
            }
            continue;
        }
        last_provider_id = attempt.provider_id;
        last_hop_attempt_count = 1;

        let skipped = attempt.outcome == "skipped";
        let ok = !skipped
            && attempts
                .iter()
                .any(|row| row.provider_id == attempt.provider_id && row.outcome == "success");

        let picked = if skipped {
            Some(attempt)
        } else if ok {
            attempts
                .iter()
                .find(|row| row.provider_id == attempt.provider_id && row.outcome == "success")
                .or_else(|| {
                    attempts
                        .iter()
                        .rev()
                        .find(|row| row.provider_id == attempt.provider_id)
                })
        } else {
            attempts
                .iter()
                .rev()
                .find(|row| row.provider_id == attempt.provider_id)
        };

        let (status, error_code, decision, reason) = match picked {
            Some(row) => (
                row.status,
                row.error_code.clone(),
                row.decision.clone(),
                row.reason.clone(),
            ),
            None => (None, None, None, None),
        };

        out.push(RequestLogRouteHop {
            provider_id: attempt.provider_id,
            provider_name: attempt.provider_name.clone(),
            ok,
            attempts: 1,
            skipped,
            status,
            error_code,
            decision,
            reason,
        });
    }
    out
}

#[derive(Debug, Clone, Default)]
struct SourceProviderInfo {
    source_provider_id: Option<i64>,
    source_provider_name: Option<String>,
    // Same predicate as the usage-stats SQL: source id present OR cx2cc bridge.
    bridged: bool,
}

fn normalize_source_provider_name(name: Option<String>) -> Option<String> {
    name.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn load_source_provider_info_map(
    conn: &Connection,
    bridge_provider_ids: &[i64],
) -> crate::shared::error::AppResult<HashMap<i64, SourceProviderInfo>> {
    let ids: Vec<i64> = bridge_provider_ids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = crate::db::sql_placeholders(ids.len());
    let sql = format!(
        r#"
SELECT
  bridge.id,
  bridge.source_provider_id,
  source.name,
  bridge.bridge_type
FROM providers bridge
LEFT JOIN providers source ON source.id = bridge.source_provider_id
WHERE bridge.id IN ({placeholders})
"#
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| db_err!("failed to prepare provider source query: {e}"))?;
    let mut rows = stmt
        .query(params_from_iter(ids.iter()))
        .map_err(|e| db_err!("failed to query provider sources: {e}"))?;

    let mut out = HashMap::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| db_err!("failed to read provider source row: {e}"))?
    {
        let bridge_id: i64 = row
            .get(0)
            .map_err(|e| db_err!("invalid provider source bridge id: {e}"))?;
        let source_provider_id: Option<i64> = row
            .get(1)
            .map_err(|e| db_err!("invalid provider source id: {e}"))?;
        let source_provider_name: Option<String> = row
            .get(2)
            .map_err(|e| db_err!("invalid provider source name: {e}"))?;
        let bridge_type: Option<String> = row
            .get(3)
            .map_err(|e| db_err!("invalid provider bridge type: {e}"))?;

        out.insert(
            bridge_id,
            SourceProviderInfo {
                source_provider_id,
                source_provider_name: normalize_source_provider_name(source_provider_name),
                bridged: crate::usage_stats::is_bridged_input_semantics(
                    source_provider_id,
                    bridge_type.as_deref(),
                ),
            },
        );
    }

    Ok(out)
}

fn attach_source_provider_info(
    conn: &Connection,
    items: &mut [RequestLogSummary],
) -> crate::shared::error::AppResult<()> {
    let ids: Vec<i64> = items.iter().map(|item| item.final_provider_id).collect();
    let info_by_bridge_id = load_source_provider_info_map(conn, &ids)?;

    for item in items.iter_mut() {
        let mut bridged = false;
        if let Some(info) = info_by_bridge_id.get(&item.final_provider_id) {
            item.final_provider_source_id = info.source_provider_id;
            item.final_provider_source_name = info.source_provider_name.clone();
            bridged = info.bridged;
        }
        item.effective_input_tokens = crate::usage_stats::effective_input_tokens_display(
            &item.cli_key,
            bridged,
            item.input_tokens,
            item.cache_read_input_tokens,
        );
    }

    Ok(())
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> Result<RequestLogSummary, rusqlite::Error> {
    let attempts_json: String = row.get("attempts_json")?;
    let attempts = parse_attempts(&attempts_json);
    let attempt_count = attempts.len() as i64;
    let (start_provider_id, start_provider_name) = start_provider_from_attempts(&attempts);
    let (final_provider_id, final_provider_name) = final_provider_from_attempts(&attempts);
    let route = route_from_attempts(&attempts);
    // has_failover: 切换过 provider（route 中有多个 hop）。注意 provider_id>0 的
    // skipped attempt 也计入 hop（见 route_includes_skipped_attempts 测试）；前端
    // src/services/gateway/traceRoute.ts 复刻此语义，两侧需保持同步。
    let has_failover = route.len() > 1;
    let session_reuse = attempts
        .iter()
        .any(|row| row.session_reuse.unwrap_or(false));
    let cost_usd = cost_usd_from_femto(row.get("cost_usd_femto")?);

    let status: Option<i64> = row.get("status")?;
    let error_code: Option<String> = row.get("error_code")?;
    let is_interrupted = status.is_none() && error_code.is_none();

    Ok(RequestLogSummary {
        id: row.get("id")?,
        trace_id: row.get("trace_id")?,
        cli_key: row.get("cli_key")?,
        session_id: row.get("session_id")?,
        method: row.get("method")?,
        path: row.get("path")?,
        excluded_from_stats: row.get::<_, i64>("excluded_from_stats").unwrap_or(0) != 0,
        special_settings_json: row.get("special_settings_json")?,
        requested_model: row.get("requested_model")?,
        status,
        error_code,
        is_interrupted,
        duration_ms: row.get("duration_ms")?,
        ttfb_ms: row.get("ttfb_ms")?,
        visible_ttfb_ms: row.get("visible_ttfb_ms")?,
        attempt_count,
        has_failover,
        start_provider_id,
        start_provider_name,
        final_provider_id,
        final_provider_name,
        final_provider_source_id: None,
        final_provider_source_name: None,
        route,
        session_reuse,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        total_tokens: row.get("total_tokens")?,
        cache_read_input_tokens: row.get("cache_read_input_tokens")?,
        cache_creation_input_tokens: row.get("cache_creation_input_tokens")?,
        cache_creation_5m_input_tokens: row.get("cache_creation_5m_input_tokens")?,
        cache_creation_1h_input_tokens: row.get("cache_creation_1h_input_tokens")?,
        // Filled by attach_source_provider_info (needs the providers table).
        effective_input_tokens: None,
        cost_usd,
        cost_multiplier: row.get("cost_multiplier")?,
        created_at_ms: row.get("created_at_ms")?,
        last_activity_ms: row.get("last_activity_ms")?,
        activity_details_json: row.get("activity_details_json").unwrap_or(None),
        created_at: row.get("created_at")?,
        provider_chain_json: row.get("provider_chain_json").unwrap_or(None),
        error_details_json: row.get("error_details_json").unwrap_or(None),
    })
}

fn row_to_detail(row: &rusqlite::Row<'_>) -> Result<RequestLogDetail, rusqlite::Error> {
    let attempts_json: String = row.get("attempts_json")?;
    let attempts = parse_attempts(&attempts_json);
    let (final_provider_id, final_provider_name) = final_provider_from_attempts(&attempts);
    let cost_usd = cost_usd_from_femto(row.get("cost_usd_femto")?);
    let status: Option<i64> = row.get("status")?;
    let error_code: Option<String> = row.get("error_code")?;
    let is_interrupted = status.is_none() && error_code.is_none();

    Ok(RequestLogDetail {
        id: row.get("id")?,
        trace_id: row.get("trace_id")?,
        cli_key: row.get("cli_key")?,
        session_id: row.get("session_id")?,
        method: row.get("method")?,
        path: row.get("path")?,
        query: row.get("query")?,
        excluded_from_stats: row.get::<_, i64>("excluded_from_stats").unwrap_or(0) != 0,
        special_settings_json: row.get("special_settings_json")?,
        status,
        error_code,
        is_interrupted,
        duration_ms: row.get("duration_ms")?,
        ttfb_ms: row.get("ttfb_ms")?,
        visible_ttfb_ms: row.get("visible_ttfb_ms")?,
        attempts_json,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        total_tokens: row.get("total_tokens")?,
        cache_read_input_tokens: row.get("cache_read_input_tokens")?,
        cache_creation_input_tokens: row.get("cache_creation_input_tokens")?,
        cache_creation_5m_input_tokens: row.get("cache_creation_5m_input_tokens")?,
        cache_creation_1h_input_tokens: row.get("cache_creation_1h_input_tokens")?,
        // Filled by attach_source_provider_info_to_detail.
        effective_input_tokens: None,
        usage_json: row.get("usage_json")?,
        requested_model: row.get("requested_model")?,
        final_provider_id,
        final_provider_name,
        final_provider_source_id: None,
        final_provider_source_name: None,
        cost_usd,
        cost_multiplier: row.get("cost_multiplier")?,
        created_at_ms: row.get("created_at_ms")?,
        last_activity_ms: row.get("last_activity_ms")?,
        activity_details_json: row.get("activity_details_json").unwrap_or(None),
        created_at: row.get("created_at")?,
        provider_chain_json: row.get("provider_chain_json").unwrap_or(None),
        error_details_json: row.get("error_details_json").unwrap_or(None),
    })
}

fn attach_source_provider_info_to_detail(
    conn: &Connection,
    item: &mut RequestLogDetail,
) -> crate::shared::error::AppResult<()> {
    let info_by_bridge_id = load_source_provider_info_map(conn, &[item.final_provider_id])?;
    let mut bridged = false;
    if let Some(info) = info_by_bridge_id.get(&item.final_provider_id) {
        item.final_provider_source_id = info.source_provider_id;
        item.final_provider_source_name = info.source_provider_name.clone();
        bridged = info.bridged;
    }
    item.effective_input_tokens = crate::usage_stats::effective_input_tokens_display(
        &item.cli_key,
        bridged,
        item.input_tokens,
        item.cache_read_input_tokens,
    );
    Ok(())
}

pub fn list_recent(
    db: &db::Db,
    cli_key: &str,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<RequestLogSummary>> {
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;

    let sql = if cli_key == "claude" {
        format!(
            "SELECT{}FROM request_logs WHERE cli_key = ?1 AND path = ?2 ORDER BY created_at_ms DESC, id DESC LIMIT ?3",
            REQUEST_LOG_SUMMARY_FIELDS
        )
    } else {
        format!(
            "SELECT{}FROM request_logs WHERE cli_key = ?1 ORDER BY created_at_ms DESC, id DESC LIMIT ?2",
            REQUEST_LOG_SUMMARY_FIELDS
        )
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = if cli_key == "claude" {
        stmt.query_map(
            params![cli_key, CLAUDE_VISIBLE_LOG_PATH, limit as i64],
            row_to_summary,
        )
    } else {
        stmt.query_map(params![cli_key, limit as i64], row_to_summary)
    }
    .map_err(|e| db_err!("failed to list request_logs: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read request_log row: {e}"))?);
    }
    attach_source_provider_info(&conn, &mut items)?;
    Ok(items)
}

pub fn list_recent_all(
    db: &db::Db,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<RequestLogSummary>> {
    let conn = db.open_connection()?;

    let sql = format!(
        "SELECT{}FROM request_logs WHERE {} ORDER BY created_at_ms DESC, id DESC LIMIT ?1",
        REQUEST_LOG_SUMMARY_FIELDS, CLAUDE_VISIBLE_LOG_CONDITION
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map(params![limit as i64], row_to_summary)
        .map_err(|e| db_err!("failed to list request_logs: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read request_log row: {e}"))?);
    }
    attach_source_provider_info(&conn, &mut items)?;
    Ok(items)
}

pub fn list_after_id(
    db: &db::Db,
    cli_key: &str,
    after_id: i64,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<RequestLogSummary>> {
    validate_cli_key(cli_key)?;
    let conn = db.open_connection()?;

    let after_id = after_id.max(0);
    let sql = if cli_key == "claude" {
        format!(
            "SELECT{}FROM request_logs WHERE cli_key = ?1 AND path = ?2 AND id > ?3 ORDER BY id ASC LIMIT ?4",
            REQUEST_LOG_SUMMARY_FIELDS
        )
    } else {
        format!(
            "SELECT{}FROM request_logs WHERE cli_key = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3",
            REQUEST_LOG_SUMMARY_FIELDS
        )
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = if cli_key == "claude" {
        stmt.query_map(
            params![cli_key, CLAUDE_VISIBLE_LOG_PATH, after_id, limit as i64],
            row_to_summary,
        )
    } else {
        stmt.query_map(params![cli_key, after_id, limit as i64], row_to_summary)
    }
    .map_err(|e| db_err!("failed to list request_logs: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read request_log row: {e}"))?);
    }
    attach_source_provider_info(&conn, &mut items)?;
    Ok(items)
}

pub fn list_after_id_all(
    db: &db::Db,
    after_id: i64,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<RequestLogSummary>> {
    let conn = db.open_connection()?;

    let after_id = after_id.max(0);
    let sql = format!(
        "SELECT{}FROM request_logs WHERE {} AND id > ?1 ORDER BY id ASC LIMIT ?2",
        REQUEST_LOG_SUMMARY_FIELDS, CLAUDE_VISIBLE_LOG_CONDITION
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| db_err!("failed to prepare query: {e}"))?;

    let rows = stmt
        .query_map(params![after_id, limit as i64], row_to_summary)
        .map_err(|e| db_err!("failed to list request_logs: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read request_log row: {e}"))?);
    }
    attach_source_provider_info(&conn, &mut items)?;
    Ok(items)
}

pub fn get_by_id(db: &db::Db, log_id: i64) -> crate::shared::error::AppResult<RequestLogDetail> {
    let conn = db.open_connection()?;
    let sql = format!(
        "SELECT{}FROM request_logs WHERE id = ?1 AND {}",
        REQUEST_LOG_DETAIL_FIELDS, CLAUDE_VISIBLE_LOG_CONDITION
    );
    let mut item = conn
        .query_row(&sql, params![log_id], row_to_detail)
        .optional()
        .map_err(|e| db_err!("failed to query request_log: {e}"))?
        .ok_or_else(|| {
            crate::shared::error::AppError::from("DB_NOT_FOUND: request_log not found".to_string())
        })?;
    attach_source_provider_info_to_detail(&conn, &mut item)?;
    Ok(item)
}

pub fn get_by_trace_id(
    db: &db::Db,
    trace_id: &str,
) -> crate::shared::error::AppResult<Option<RequestLogDetail>> {
    if trace_id.trim().is_empty() {
        return Err("SEC_INVALID_INPUT: trace_id is required".to_string().into());
    }

    let conn = db.open_connection()?;
    let sql = format!(
        "SELECT{}FROM request_logs WHERE trace_id = ?1 AND {}",
        REQUEST_LOG_DETAIL_FIELDS, CLAUDE_VISIBLE_LOG_CONDITION
    );
    let mut item = conn
        .query_row(&sql, params![trace_id], row_to_detail)
        .optional()
        .map_err(|e| db_err!("failed to query request_log: {e}"))?;
    if let Some(detail) = item.as_mut() {
        attach_source_provider_info_to_detail(&conn, detail)?;
    }
    Ok(item)
}

pub fn terminal_trace_ids(
    db: &db::Db,
    trace_ids: &[String],
) -> crate::shared::error::AppResult<HashSet<String>> {
    const SQLITE_PARAM_CHUNK: usize = 900;

    let mut terminal = HashSet::new();
    if trace_ids.is_empty() {
        return Ok(terminal);
    }

    let conn = db.open_connection()?;
    for chunk in trace_ids.chunks(SQLITE_PARAM_CHUNK) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT trace_id FROM request_logs \
             WHERE trace_id IN ({placeholders}) \
             AND (status IS NOT NULL OR error_code IS NOT NULL)"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| db_err!("failed to prepare terminal trace query: {e}"))?;
        let rows = stmt
            .query_map(params_from_iter(chunk.iter().map(String::as_str)), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| db_err!("failed to query terminal traces: {e}"))?;
        for row in rows {
            terminal.insert(row.map_err(|e| db_err!("failed to read terminal trace: {e}"))?);
        }
    }

    Ok(terminal)
}

pub fn codex_reasoning_guard_stats(
    db: &db::Db,
    start_created_at_ms: Option<i64>,
    end_created_at_ms: Option<i64>,
) -> crate::shared::error::AppResult<CodexReasoningGuardStats> {
    let conn = db.open_connection()?;
    if matches!(start_created_at_ms, Some(value) if value <= 0) {
        return Err(db_err!("invalid codex reasoning guard stats cutoff"));
    }
    if matches!(end_created_at_ms, Some(value) if value <= 0) {
        return Err(db_err!("invalid codex reasoning guard stats end cutoff"));
    }
    if matches!(
        (start_created_at_ms, end_created_at_ms),
        (Some(start), Some(end)) if end <= start
    ) {
        return Err(db_err!("invalid codex reasoning guard stats range"));
    }

    let mut request_time_filters: Vec<String> = Vec::new();
    let mut hit_time_filters: Vec<String> = Vec::new();
    if start_created_at_ms.is_some() {
        request_time_filters.push("created_at_ms >= ?1".to_string());
        hit_time_filters.push("request_logs.created_at_ms >= ?1".to_string());
    }
    if end_created_at_ms.is_some() {
        let end_param = if start_created_at_ms.is_some() {
            "?2"
        } else {
            "?1"
        };
        request_time_filters.push(format!("created_at_ms < {end_param}"));
        hit_time_filters.push(format!("request_logs.created_at_ms < {end_param}"));
    }
    let time_filter = if request_time_filters.is_empty() {
        String::new()
    } else {
        format!(" AND {}", request_time_filters.join(" AND "))
    };
    let hit_time_filter = if hit_time_filters.is_empty() {
        String::new()
    } else {
        format!(" AND {}", hit_time_filters.join(" AND "))
    };
    let overall_sql = format!(
        r#"
WITH codex_requests AS (
  SELECT
    id,
    COALESCE(NULLIF(TRIM(requested_model), ''), '{unknown}') AS requested_model,
    duration_ms,
    output_tokens
  FROM request_logs
  WHERE cli_key = 'codex'{time_filter}
),
-- Unified guard hits include continuation_repair strategy records; each matched
-- attempt counts once whether the strategy retried or repaired the stream.
guard_hit_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    codex_requests.requested_model AS requested_model,
    COALESCE(
      NULLIF(TRIM(json_extract(special.value, '$.hitSource')), ''),
      'reasoning_tokens'
    ) AS hit_source
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_guard'
),
feature_samples AS (
  SELECT
    codex_requests.id AS request_id,
    codex_requests.duration_ms AS duration_ms,
    codex_requests.output_tokens AS output_tokens,
    special.value AS feature,
    LOWER(TRIM(COALESCE(
      json_extract(special.value, '$.requestReasoningEffort'),
      json_extract(special.value, '$.rawRequestReasoningEffort'),
      ''
    ))) AS request_reasoning_effort
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_features'
),
unified_continuation_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    CASE COALESCE(
      NULLIF(LOWER(TRIM(json_extract(special.value, '$.guardStrategyOutcome'))), ''),
      'unknown'
    )
      WHEN 'continuation_repaired' THEN 'repaired'
      ELSE COALESCE(
        NULLIF(LOWER(TRIM(json_extract(special.value, '$.guardStrategyOutcome'))), ''),
        'unknown'
      )
    END AS status,
    MAX(COALESCE(CAST(json_extract(special.value, '$.continuationSentRounds') AS INTEGER), 0), 0) AS sent_rounds
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_guard'
    AND LOWER(TRIM(COALESCE(json_extract(special.value, '$.guardPostMatchStrategy'), ''))) IN ('continuation_repair', 'continuation_repair_experimental')
    AND NULLIF(TRIM(COALESCE(json_extract(special.value, '$.guardStrategyOutcome'), '')), '') IS NOT NULL
),
legacy_continuation_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    COALESCE(
      NULLIF(LOWER(TRIM(json_extract(special.value, '$.status'))), ''),
      'unknown'
    ) AS status,
    MAX(COALESCE(CAST(json_extract(special.value, '$.sentRounds') AS INTEGER), 0), 0) AS sent_rounds
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_continuation'
    AND NOT EXISTS (
      SELECT 1
      FROM unified_continuation_attempts AS unified
      WHERE unified.request_id = codex_requests.id
    )
),
continuation_attempts AS (
  SELECT request_id, status, sent_rounds FROM unified_continuation_attempts
  UNION ALL
  SELECT request_id, status, sent_rounds FROM legacy_continuation_attempts
),
guard_hit_requests AS (
  SELECT request_id, requested_model, COUNT(1) AS hit_attempt_count
  FROM guard_hit_attempts
  GROUP BY request_id, requested_model
)
SELECT
  COALESCE((SELECT COUNT(*) FROM guard_hit_requests), 0) AS hit_request_count,
  COALESCE((SELECT SUM(hit_attempt_count) FROM guard_hit_requests), 0) AS hit_attempt_count,
  COALESCE((SELECT SUM(CASE WHEN hit_source = 'reasoning_tokens' THEN 1 ELSE 0 END) FROM guard_hit_attempts), 0) AS token_hit_attempt_count,
  COALESCE((SELECT SUM(CASE WHEN hit_source = 'final_answer_only_high_xhigh' THEN 1 ELSE 0 END) FROM guard_hit_attempts), 0) AS feature_hit_attempt_count,
  COALESCE((SELECT COUNT(DISTINCT CASE WHEN hit_source = 'reasoning_tokens' THEN request_id END) FROM guard_hit_attempts), 0) AS reasoning_token_hit_request_count,
  COALESCE((SELECT COUNT(DISTINCT CASE WHEN hit_source = 'final_answer_only_high_xhigh' THEN request_id END) FROM guard_hit_attempts), 0) AS final_answer_only_high_xhigh_hit_request_count,
  COALESCE((SELECT COUNT(DISTINCT request_id) FROM feature_samples), 0) AS feature_sample_request_count,
  COALESCE((SELECT COUNT(*) FROM feature_samples), 0) AS feature_sample_count,
  COALESCE((SELECT SUM(CASE WHEN json_extract(feature, '$.finalAnswerOnly') = 1 THEN 1 ELSE 0 END) FROM feature_samples), 0) AS final_answer_only_sample_count,
  COALESCE((SELECT SUM(CASE WHEN json_extract(feature, '$.finalAnswerOnly') = 1 AND request_reasoning_effort IN ('high', 'xhigh') THEN 1 ELSE 0 END) FROM feature_samples), 0) AS high_xhigh_final_answer_only_sample_count,
  COALESCE((SELECT SUM(CASE WHEN json_extract(feature, '$.reasoningTokens') = 516 AND json_extract(feature, '$.finalAnswerOnly') = 1 AND COALESCE(json_extract(feature, '$.commentaryObserved'), 0) = 0 THEN 1 ELSE 0 END) FROM feature_samples), 0) AS reasoning_516_final_answer_only_no_commentary_count,
  COALESCE((SELECT SUM(CASE WHEN json_extract(feature, '$.interceptExemptReason') = 'context_compaction' THEN 1 ELSE 0 END) FROM feature_samples), 0) AS compaction_exempt_sample_count,
  COALESCE((SELECT SUM(CASE WHEN json_type(feature, '$.reasoningTokens') IS NOT NULL AND json_type(feature, '$.reasoningTokens') != 'null' THEN 1 ELSE 0 END) FROM feature_samples), 0) AS reasoning_tokens_coverage_count,
  COALESCE((SELECT SUM(CASE WHEN json_type(feature, '$.finalAnswerOnly') IS NOT NULL AND json_type(feature, '$.finalAnswerOnly') != 'null' THEN 1 ELSE 0 END) FROM feature_samples), 0) AS final_answer_only_coverage_count,
  COALESCE((SELECT SUM(CASE WHEN json_type(feature, '$.commentaryObserved') IS NOT NULL AND json_type(feature, '$.commentaryObserved') != 'null' THEN 1 ELSE 0 END) FROM feature_samples), 0) AS commentary_observed_coverage_count,
  COALESCE((SELECT SUM(CASE WHEN request_reasoning_effort != '' THEN 1 ELSE 0 END) FROM feature_samples), 0) AS reasoning_effort_coverage_count,
  COALESCE((SELECT SUM(CASE WHEN duration_ms IS NOT NULL THEN 1 ELSE 0 END) FROM feature_samples), 0) AS duration_ms_coverage_count,
  COALESCE((SELECT SUM(CASE WHEN output_tokens IS NOT NULL THEN 1 ELSE 0 END) FROM feature_samples), 0) AS output_tokens_coverage_count,
  COALESCE((SELECT COUNT(DISTINCT request_id) FROM continuation_attempts), 0) AS continuation_triggered_request_count,
  COALESCE((SELECT COUNT(*) FROM continuation_attempts), 0) AS continuation_triggered_attempt_count,
  COALESCE((SELECT COUNT(DISTINCT CASE WHEN status = 'repaired' THEN request_id END) FROM continuation_attempts), 0) AS continuation_repaired_request_count,
  COALESCE((SELECT SUM(CASE WHEN status = 'repaired' THEN 1 ELSE 0 END) FROM continuation_attempts), 0) AS continuation_repaired_attempt_count,
  COALESCE((SELECT SUM(CASE WHEN status != 'repaired' THEN 1 ELSE 0 END) FROM continuation_attempts), 0) AS continuation_non_repaired_attempt_count,
  COALESCE((SELECT AVG(sent_rounds * 1.0) FROM continuation_attempts), 0.0) AS continuation_average_sent_rounds,
  COALESCE((SELECT COUNT(*) FROM codex_requests), 0) AS total_request_count
"#,
        unknown = UNKNOWN_CODEX_REQUESTED_MODEL_LABEL,
        time_filter = time_filter
    );

    let range_params = [start_created_at_ms, end_created_at_ms];
    let range_params_iter = range_params.iter().flatten().copied();

    let mut summary_stats = conn
        .query_row(
            &overall_sql,
            params_from_iter(range_params_iter.clone()),
            |row| {
                let hit_request_count = row.get::<_, i64>("hit_request_count")?.max(0);
                let total_request_count = row.get::<_, i64>("total_request_count")?.max(0);
                let normal_request_count = (total_request_count - hit_request_count).max(0);
                let hit_rate = if total_request_count > 0 {
                    hit_request_count as f64 / total_request_count as f64
                } else {
                    0.0
                };
                Ok((
                    hit_request_count,
                    row.get::<_, i64>("hit_attempt_count")?.max(0),
                    row.get::<_, i64>("token_hit_attempt_count")?.max(0),
                    row.get::<_, i64>("feature_hit_attempt_count")?.max(0),
                    row.get::<_, i64>("reasoning_token_hit_request_count")?
                        .max(0),
                    row.get::<_, i64>("final_answer_only_high_xhigh_hit_request_count")?
                        .max(0),
                    normal_request_count,
                    total_request_count,
                    hit_rate,
                    row.get::<_, i64>("feature_sample_request_count")?.max(0),
                    row.get::<_, i64>("feature_sample_count")?.max(0),
                    row.get::<_, i64>("final_answer_only_sample_count")?.max(0),
                    row.get::<_, i64>("high_xhigh_final_answer_only_sample_count")?
                        .max(0),
                    row.get::<_, i64>("reasoning_516_final_answer_only_no_commentary_count")?
                        .max(0),
                    row.get::<_, i64>("compaction_exempt_sample_count")?.max(0),
                    row.get::<_, i64>("reasoning_tokens_coverage_count")?.max(0),
                    row.get::<_, i64>("final_answer_only_coverage_count")?
                        .max(0),
                    row.get::<_, i64>("commentary_observed_coverage_count")?
                        .max(0),
                    row.get::<_, i64>("reasoning_effort_coverage_count")?.max(0),
                    row.get::<_, i64>("duration_ms_coverage_count")?.max(0),
                    row.get::<_, i64>("output_tokens_coverage_count")?.max(0),
                    row.get::<_, i64>("continuation_triggered_request_count")?
                        .max(0),
                    row.get::<_, i64>("continuation_triggered_attempt_count")?
                        .max(0),
                    row.get::<_, i64>("continuation_repaired_request_count")?
                        .max(0),
                    row.get::<_, i64>("continuation_repaired_attempt_count")?
                        .max(0),
                    row.get::<_, i64>("continuation_non_repaired_attempt_count")?
                        .max(0),
                    row.get::<_, f64>("continuation_average_sent_rounds")?
                        .max(0.0),
                ))
            },
        )
        .map(
            |(
                hit_request_count,
                hit_attempt_count,
                token_hit_attempt_count,
                feature_hit_attempt_count,
                reasoning_token_hit_request_count,
                final_answer_only_high_xhigh_hit_request_count,
                normal_request_count,
                total_request_count,
                hit_rate,
                feature_sample_request_count,
                feature_sample_count,
                final_answer_only_sample_count,
                high_xhigh_final_answer_only_sample_count,
                reasoning_516_final_answer_only_no_commentary_count,
                compaction_exempt_sample_count,
                reasoning_tokens_coverage_count,
                final_answer_only_coverage_count,
                commentary_observed_coverage_count,
                reasoning_effort_coverage_count,
                duration_ms_coverage_count,
                output_tokens_coverage_count,
                continuation_triggered_request_count,
                continuation_triggered_attempt_count,
                continuation_repaired_request_count,
                continuation_repaired_attempt_count,
                continuation_non_repaired_attempt_count,
                continuation_average_sent_rounds,
            )| {
                let continuation_repair_rate = if continuation_triggered_request_count > 0 {
                    continuation_repaired_request_count as f64
                        / continuation_triggered_request_count as f64
                } else {
                    0.0
                };
                CodexReasoningGuardStats {
                    hit_request_count,
                    hit_attempt_count,
                    token_hit_attempt_count,
                    feature_hit_attempt_count,
                    reasoning_token_hit_request_count,
                    final_answer_only_high_xhigh_hit_request_count,
                    normal_request_count,
                    total_request_count,
                    hit_rate,
                    feature_sample_request_count,
                    feature_sample_count,
                    final_answer_only_sample_count,
                    high_xhigh_final_answer_only_sample_count,
                    reasoning_516_final_answer_only_no_commentary_count,
                    compaction_exempt_sample_count,
                    reasoning_tokens_coverage_count,
                    final_answer_only_coverage_count,
                    commentary_observed_coverage_count,
                    reasoning_effort_coverage_count,
                    duration_ms_coverage_count,
                    output_tokens_coverage_count,
                    continuation_triggered_request_count,
                    continuation_triggered_attempt_count,
                    continuation_repaired_request_count,
                    continuation_repaired_attempt_count,
                    continuation_non_repaired_attempt_count,
                    continuation_repair_rate,
                    continuation_average_sent_rounds,
                    continuation_by_status: Vec::new(),
                    by_model: Vec::new(),
                    by_model_and_effort: Vec::new(),
                }
            },
        )
        .map_err(|e| db_err!("failed to query codex reasoning guard summary stats: {e}"))?;

    let by_model_sql = format!(
        r#"
WITH codex_requests AS (
  SELECT
    id,
    COALESCE(NULLIF(TRIM(requested_model), ''), '{unknown}') AS requested_model
  FROM request_logs
  WHERE cli_key = 'codex'{time_filter}
),
-- Unified guard hits include continuation_repair strategy records; each matched
-- attempt counts once whether the strategy retried or repaired the stream.
guard_hit_attempts AS (
  SELECT
    request_logs.id AS request_id,
    COALESCE(NULLIF(TRIM(request_logs.requested_model), ''), '{unknown}') AS requested_model
  FROM request_logs
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE request_logs.cli_key = 'codex'
    {hit_time_filter}
    AND json_extract(special.value, '$.type') = 'codex_reasoning_guard'
),
guard_hit_requests AS (
  SELECT request_id, requested_model, COUNT(1) AS hit_attempt_count
  FROM guard_hit_attempts
  GROUP BY request_id, requested_model
),
totals_by_model AS (
  SELECT requested_model, COUNT(*) AS total_request_count
  FROM codex_requests
  GROUP BY requested_model
),
hits_by_model AS (
  SELECT
    requested_model,
    COUNT(*) AS hit_request_count,
    COALESCE(SUM(hit_attempt_count), 0) AS hit_attempt_count
  FROM guard_hit_requests
  GROUP BY requested_model
)
SELECT
  totals_by_model.requested_model AS requested_model,
  totals_by_model.total_request_count AS total_request_count,
  COALESCE(hits_by_model.hit_request_count, 0) AS hit_request_count,
  totals_by_model.total_request_count - COALESCE(hits_by_model.hit_request_count, 0) AS normal_request_count,
  COALESCE(hits_by_model.hit_attempt_count, 0) AS hit_attempt_count
FROM totals_by_model
LEFT JOIN hits_by_model ON hits_by_model.requested_model = totals_by_model.requested_model
ORDER BY
  COALESCE(hits_by_model.hit_request_count, 0) DESC,
  totals_by_model.total_request_count DESC,
  totals_by_model.requested_model ASC
"#,
        unknown = UNKNOWN_CODEX_REQUESTED_MODEL_LABEL,
        time_filter = time_filter,
        hit_time_filter = hit_time_filter
    );

    let mut stmt = conn
        .prepare(&by_model_sql)
        .map_err(|e| db_err!("failed to prepare codex reasoning guard model stats query: {e}"))?;
    let mut rows = stmt
        .query(params_from_iter(range_params.iter().flatten().copied()))
        .map_err(|e| db_err!("failed to run codex reasoning guard model stats query: {e}"))?;

    let mut by_model = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| db_err!("failed to read codex reasoning guard model stats row: {e}"))?
    {
        let total_model_requests = row
            .get::<_, i64>("total_request_count")
            .map_err(|e| db_err!("invalid codex reasoning guard total_request_count: {e}"))?
            .max(0);
        let hit_model_requests = row
            .get::<_, i64>("hit_request_count")
            .map_err(|e| db_err!("invalid codex reasoning guard hit_request_count: {e}"))?
            .max(0);
        let hit_rate = if total_model_requests > 0 {
            hit_model_requests as f64 / total_model_requests as f64
        } else {
            0.0
        };

        by_model.push(CodexReasoningGuardModelStat {
            requested_model: row
                .get("requested_model")
                .map_err(|e| db_err!("invalid codex reasoning guard requested_model: {e}"))?,
            total_request_count: total_model_requests,
            hit_request_count: hit_model_requests,
            normal_request_count: row
                .get::<_, i64>("normal_request_count")
                .map_err(|e| db_err!("invalid codex reasoning guard normal_request_count: {e}"))?
                .max(0),
            hit_attempt_count: row
                .get::<_, i64>("hit_attempt_count")
                .map_err(|e| db_err!("invalid codex reasoning guard hit_attempt_count: {e}"))?
                .max(0),
            hit_rate,
        });
    }

    let by_model_and_effort_sql = format!(
        r#"
WITH codex_requests AS (
  SELECT
    id,
    COALESCE(NULLIF(TRIM(requested_model), ''), '{unknown_model}') AS requested_model,
    COALESCE(
      (
        SELECT
          CASE LOWER(TRIM(COALESCE(
            json_extract(effort.value, '$.effort'),
            json_extract(effort.value, '$.rawEffort')
          )))
            WHEN 'none' THEN 'none'
            WHEN 'minimal' THEN 'minimal'
            WHEN 'low' THEN 'low'
            WHEN 'medium' THEN 'medium'
            WHEN 'high' THEN 'high'
            WHEN 'xhigh' THEN 'xhigh'
            ELSE NULL
          END
        FROM json_each(request_logs.special_settings_json) AS effort
        WHERE json_extract(effort.value, '$.type') = 'codex_reasoning_effort'
          AND (
            json_extract(effort.value, '$.effort') IS NOT NULL
            OR json_extract(effort.value, '$.rawEffort') IS NOT NULL
          )
        ORDER BY CAST(effort.key AS INTEGER) DESC
        LIMIT 1
      ),
      CASE COALESCE(NULLIF(TRIM(requested_model), ''), '')
        WHEN 'gpt-5.5' THEN 'medium'
        WHEN 'gpt-5.5-pro' THEN 'high'
        WHEN 'gpt-5.4' THEN 'none'
        WHEN 'gpt-5.4-mini' THEN 'none'
        WHEN 'gpt-5.4-nano' THEN 'none'
        WHEN 'gpt-5.4-pro' THEN 'medium'
        ELSE '{unknown_effort}'
      END
    ) AS reasoning_effort
  FROM request_logs
  WHERE cli_key = 'codex'{time_filter}
),
-- Unified guard hits include continuation_repair strategy records; each matched
-- attempt counts once whether the strategy retried or repaired the stream.
guard_hit_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    codex_requests.requested_model AS requested_model,
    codex_requests.reasoning_effort AS reasoning_effort
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_guard'
),
guard_hit_requests AS (
  SELECT request_id, requested_model, reasoning_effort, COUNT(1) AS hit_attempt_count
  FROM guard_hit_attempts
  GROUP BY request_id, requested_model, reasoning_effort
),
totals_by_model_effort AS (
  SELECT requested_model, reasoning_effort, COUNT(*) AS total_request_count
  FROM codex_requests
  GROUP BY requested_model, reasoning_effort
),
hits_by_model_effort AS (
  SELECT
    requested_model,
    reasoning_effort,
    COUNT(*) AS hit_request_count,
    COALESCE(SUM(hit_attempt_count), 0) AS hit_attempt_count
  FROM guard_hit_requests
  GROUP BY requested_model, reasoning_effort
)
SELECT
  totals_by_model_effort.requested_model AS requested_model,
  totals_by_model_effort.reasoning_effort AS reasoning_effort,
  totals_by_model_effort.total_request_count AS total_request_count,
  COALESCE(hits_by_model_effort.hit_request_count, 0) AS hit_request_count,
  totals_by_model_effort.total_request_count - COALESCE(hits_by_model_effort.hit_request_count, 0) AS normal_request_count,
  COALESCE(hits_by_model_effort.hit_attempt_count, 0) AS hit_attempt_count
FROM totals_by_model_effort
LEFT JOIN hits_by_model_effort
  ON hits_by_model_effort.requested_model = totals_by_model_effort.requested_model
  AND hits_by_model_effort.reasoning_effort = totals_by_model_effort.reasoning_effort
ORDER BY
  COALESCE(hits_by_model_effort.hit_request_count, 0) DESC,
  totals_by_model_effort.total_request_count DESC,
  totals_by_model_effort.requested_model ASC,
  totals_by_model_effort.reasoning_effort ASC
"#,
        unknown_model = UNKNOWN_CODEX_REQUESTED_MODEL_LABEL,
        unknown_effort = UNKNOWN_CODEX_REASONING_EFFORT_LABEL,
        time_filter = time_filter
    );

    let mut stmt = conn.prepare(&by_model_and_effort_sql).map_err(|e| {
        db_err!("failed to prepare codex reasoning guard model effort stats query: {e}")
    })?;
    let mut rows = stmt
        .query(params_from_iter(range_params.iter().flatten().copied()))
        .map_err(|e| {
            db_err!("failed to run codex reasoning guard model effort stats query: {e}")
        })?;

    let mut by_model_and_effort = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| db_err!("failed to read codex reasoning guard model effort stats row: {e}"))?
    {
        let total_model_effort_requests = row
            .get::<_, i64>("total_request_count")
            .map_err(|e| {
                db_err!("invalid codex reasoning guard model effort total_request_count: {e}")
            })?
            .max(0);
        let hit_model_effort_requests = row
            .get::<_, i64>("hit_request_count")
            .map_err(|e| {
                db_err!("invalid codex reasoning guard model effort hit_request_count: {e}")
            })?
            .max(0);
        let hit_rate = if total_model_effort_requests > 0 {
            hit_model_effort_requests as f64 / total_model_effort_requests as f64
        } else {
            0.0
        };

        by_model_and_effort.push(CodexReasoningGuardModelEffortStat {
            requested_model: row.get("requested_model").map_err(|e| {
                db_err!("invalid codex reasoning guard model effort requested_model: {e}")
            })?,
            reasoning_effort: row.get("reasoning_effort").map_err(|e| {
                db_err!("invalid codex reasoning guard model effort reasoning_effort: {e}")
            })?,
            total_request_count: total_model_effort_requests,
            hit_request_count: hit_model_effort_requests,
            normal_request_count: row
                .get::<_, i64>("normal_request_count")
                .map_err(|e| {
                    db_err!("invalid codex reasoning guard model effort normal_request_count: {e}")
                })?
                .max(0),
            hit_attempt_count: row
                .get::<_, i64>("hit_attempt_count")
                .map_err(|e| {
                    db_err!("invalid codex reasoning guard model effort hit_attempt_count: {e}")
                })?
                .max(0),
            hit_rate,
        });
    }

    let continuation_by_status_sql = format!(
        r#"
WITH codex_requests AS (
  SELECT id
  FROM request_logs
  WHERE cli_key = 'codex'{time_filter}
),
unified_continuation_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    CASE COALESCE(
      NULLIF(LOWER(TRIM(json_extract(special.value, '$.guardStrategyOutcome'))), ''),
      'unknown'
    )
      WHEN 'continuation_repaired' THEN 'repaired'
      ELSE COALESCE(
        NULLIF(LOWER(TRIM(json_extract(special.value, '$.guardStrategyOutcome'))), ''),
        'unknown'
      )
    END AS status,
    MAX(COALESCE(CAST(json_extract(special.value, '$.continuationSentRounds') AS INTEGER), 0), 0) AS sent_rounds
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_guard'
    AND LOWER(TRIM(COALESCE(json_extract(special.value, '$.guardPostMatchStrategy'), ''))) IN ('continuation_repair', 'continuation_repair_experimental')
    AND NULLIF(TRIM(COALESCE(json_extract(special.value, '$.guardStrategyOutcome'), '')), '') IS NOT NULL
),
legacy_continuation_attempts AS (
  SELECT
    codex_requests.id AS request_id,
    COALESCE(
      NULLIF(LOWER(TRIM(json_extract(special.value, '$.status'))), ''),
      'unknown'
    ) AS status,
    MAX(COALESCE(CAST(json_extract(special.value, '$.sentRounds') AS INTEGER), 0), 0) AS sent_rounds
  FROM codex_requests
  JOIN request_logs ON request_logs.id = codex_requests.id
  JOIN json_each(request_logs.special_settings_json) AS special
  WHERE json_extract(special.value, '$.type') = 'codex_reasoning_continuation'
    AND NOT EXISTS (
      SELECT 1
      FROM unified_continuation_attempts AS unified
      WHERE unified.request_id = codex_requests.id
    )
),
continuation_attempts AS (
  SELECT request_id, status, sent_rounds FROM unified_continuation_attempts
  UNION ALL
  SELECT request_id, status, sent_rounds FROM legacy_continuation_attempts
)
SELECT
  status,
  COUNT(DISTINCT request_id) AS request_count,
  COUNT(*) AS attempt_count,
  COALESCE(AVG(sent_rounds * 1.0), 0.0) AS average_sent_rounds
FROM continuation_attempts
GROUP BY status
ORDER BY attempt_count DESC, status ASC
"#,
        time_filter = time_filter
    );

    let mut stmt = conn.prepare(&continuation_by_status_sql).map_err(|e| {
        db_err!("failed to prepare codex reasoning continuation status stats query: {e}")
    })?;
    let mut rows = stmt
        .query(params_from_iter(range_params.iter().flatten().copied()))
        .map_err(|e| {
            db_err!("failed to run codex reasoning continuation status stats query: {e}")
        })?;

    let mut continuation_by_status = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| db_err!("failed to read codex reasoning continuation status stats row: {e}"))?
    {
        continuation_by_status.push(CodexReasoningContinuationStatusStat {
            status: row
                .get("status")
                .map_err(|e| db_err!("invalid codex reasoning continuation status: {e}"))?,
            request_count: row
                .get::<_, i64>("request_count")
                .map_err(|e| db_err!("invalid codex reasoning continuation request_count: {e}"))?
                .max(0),
            attempt_count: row
                .get::<_, i64>("attempt_count")
                .map_err(|e| db_err!("invalid codex reasoning continuation attempt_count: {e}"))?
                .max(0),
            average_sent_rounds: row
                .get::<_, f64>("average_sent_rounds")
                .map_err(|e| {
                    db_err!("invalid codex reasoning continuation average_sent_rounds: {e}")
                })?
                .max(0.0),
        });
    }

    summary_stats.continuation_by_status = continuation_by_status;
    summary_stats.by_model = by_model;
    summary_stats.by_model_and_effort = by_model_and_effort;

    Ok(summary_stats)
}

#[cfg(test)]
mod tests {
    use super::{
        codex_reasoning_guard_stats, final_provider_from_attempts, get_by_id, get_by_trace_id,
        list_after_id_all, list_recent, list_recent_all, load_source_provider_info_map,
        parse_attempts, route_from_attempts, start_provider_from_attempts, terminal_trace_ids,
    };
    use crate::db;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn seed_request_log(conn: &Connection, id: i64, trace_id: &str, cli_key: &str, path: &str) {
        conn.execute(
            r#"
INSERT INTO request_logs (
  id, trace_id, cli_key, session_id, method, path, query, excluded_from_stats,
  special_settings_json, status, error_code, duration_ms, ttfb_ms, attempts_json,
  input_tokens, output_tokens, total_tokens, cache_read_input_tokens,
  cache_creation_input_tokens, cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens, usage_json, requested_model, cost_usd_femto,
  cost_multiplier, created_at_ms, created_at, final_provider_id
) VALUES (?1, ?2, ?3, NULL, 'POST', ?4, NULL, 0, NULL, 200, NULL, 10, 5, '[]',
  NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'model', NULL, 1.0, ?5, ?6, 0)
"#,
            rusqlite::params![id, trace_id, cli_key, path, id * 1000, id],
        )
        .unwrap();
    }

    fn seed_codex_request_log_with_special_settings(
        conn: &Connection,
        id: i64,
        trace_id: &str,
        requested_model: Option<&str>,
        special_settings_json: Option<&str>,
    ) {
        conn.execute(
            r#"
INSERT INTO request_logs (
  id, trace_id, cli_key, session_id, method, path, query, excluded_from_stats,
  special_settings_json, status, error_code, duration_ms, ttfb_ms, attempts_json,
  input_tokens, output_tokens, total_tokens, cache_read_input_tokens,
  cache_creation_input_tokens, cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens, usage_json, requested_model, cost_usd_femto,
  cost_multiplier, created_at_ms, created_at, final_provider_id
) VALUES (?1, ?2, 'codex', NULL, 'POST', '/v1/responses', NULL, 0, ?3, 200, NULL, 10, 5, '[]',
  NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, ?4, NULL, 1.0, ?5, ?6, 0)
"#,
            rusqlite::params![
                id,
                trace_id,
                special_settings_json,
                requested_model,
                id * 1000,
                id
            ],
        )
        .unwrap();
    }

    #[test]
    fn route_includes_skipped_attempts() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"skipped","status":null,"error_code":"GW_PROVIDER_RATE_LIMITED","decision":"skip","reason":"provider skipped by rate limit"},
                {"provider_id":2,"provider_name":"B","outcome":"success","status":200,"error_code":null,"decision":"success","reason":null}
            ]"#,
        );
        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 2);
        assert_eq!(route[0].provider_id, 1);
        assert!(route[0].skipped);
        assert!(!route[0].ok);
        assert_eq!(route[0].attempts, 1);
        assert_eq!(
            route[0].error_code.as_deref(),
            Some("GW_PROVIDER_RATE_LIMITED")
        );
        assert_eq!(route[0].decision.as_deref(), Some("skip"));
        assert_eq!(
            route[0].reason.as_deref(),
            Some("provider skipped by rate limit")
        );
        assert_eq!(route[1].provider_id, 2);
        assert!(!route[1].skipped);
        assert!(route[1].ok);
        assert_eq!(route[1].attempts, 1);
    }

    #[test]
    fn route_includes_gate_only_skip_attempts() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"skipped","status":null,"error_code":"GW_PROVIDER_CIRCUIT_OPEN","decision":"skip","reason":"provider skipped by circuit breaker"}
            ]"#,
        );
        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].provider_id, 1);
        assert!(route[0].skipped);
        assert!(!route[0].ok);
        assert_eq!(route[0].attempts, 1);
    }

    #[test]
    fn start_and_final_provider_prefer_non_skipped_attempts() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"skipped","status":null,"error_code":"GW_PROVIDER_RATE_LIMITED","decision":"skip","reason":"provider skipped by rate limit"},
                {"provider_id":2,"provider_name":"B","outcome":"failed","status":429,"error_code":"GW_UPSTREAM_4XX","decision":"abort","reason":"status=429"}
            ]"#,
        );

        let (start_id, start_name) = start_provider_from_attempts(&attempts);
        assert_eq!(start_id, 2);
        assert_eq!(start_name, "B");

        let (final_id, final_name) = final_provider_from_attempts(&attempts);
        assert_eq!(final_id, 2);
        assert_eq!(final_name, "B");
    }

    #[test]
    fn start_and_final_provider_hide_gate_only_skips() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"skipped","status":null,"error_code":"GW_PROVIDER_CIRCUIT_OPEN","decision":"skip","reason":"provider skipped by circuit breaker"}
            ]"#,
        );

        let (start_id, start_name) = start_provider_from_attempts(&attempts);
        assert_eq!(start_id, 0);
        assert_eq!(start_name, "Unknown");

        let (final_id, final_name) = final_provider_from_attempts(&attempts);
        assert_eq!(final_id, 0);
        assert_eq!(final_name, "Unknown");

        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 1);
        assert!(route[0].skipped);
        assert!(!route[0].ok);
    }

    #[test]
    fn route_counts_consecutive_same_provider_attempts() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"failed","status":500,"error_code":"GW_UPSTREAM_5XX","decision":"retry","reason":"status=500"},
                {"provider_id":1,"provider_name":"A","outcome":"failed","status":500,"error_code":"GW_UPSTREAM_5XX","decision":"retry","reason":"status=500"},
                {"provider_id":1,"provider_name":"A","outcome":"failed","status":500,"error_code":"GW_UPSTREAM_5XX","decision":"failover","reason":"status=500"},
                {"provider_id":2,"provider_name":"B","outcome":"success","status":200,"error_code":null,"decision":"success","reason":null}
            ]"#,
        );
        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 2);
        assert_eq!(route[0].provider_id, 1);
        assert_eq!(route[0].attempts, 3);
        assert_eq!(route[0].provider_name, "A");
        assert!(!route[0].ok);
        assert_eq!(route[1].provider_id, 2);
        assert_eq!(route[1].attempts, 1);
        assert_eq!(route[1].provider_name, "B");
        assert!(route[1].ok);
    }

    #[test]
    fn route_single_provider_single_attempt() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":1,"provider_name":"A","outcome":"success","status":200,"error_code":null,"decision":"success","reason":null}
            ]"#,
        );
        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].provider_id, 1);
        assert_eq!(route[0].attempts, 1);
        assert!(route[0].ok);
    }

    #[test]
    fn started_attempt_still_resolves_provider_for_abort_logs() {
        let attempts = parse_attempts(
            r#"[
                {"provider_id":12,"provider_name":"Claude Bridge","outcome":"started","status":null,"error_code":null,"decision":null,"reason":null}
            ]"#,
        );

        let (final_id, final_name) = final_provider_from_attempts(&attempts);
        assert_eq!(final_id, 12);
        assert_eq!(final_name, "Claude Bridge");

        let route = route_from_attempts(&attempts);
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].provider_id, 12);
        assert_eq!(route[0].provider_name, "Claude Bridge");
        assert!(!route[0].ok);
    }

    #[test]
    fn loads_source_provider_names_for_bridge_providers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
CREATE TABLE providers (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  source_provider_id INTEGER,
  bridge_type TEXT
);
INSERT INTO providers (id, name, source_provider_id, bridge_type) VALUES (7, 'OpenAI Primary', NULL, NULL);
INSERT INTO providers (id, name, source_provider_id, bridge_type) VALUES (12, 'Claude Bridge', 7, 'cx2cc');
"#,
        )
        .unwrap();

        let info = load_source_provider_info_map(&conn, &[7, 12, 99]).unwrap();
        let bridge = info.get(&12).expect("bridge provider source info");

        assert_eq!(bridge.source_provider_id, Some(7));
        assert_eq!(
            bridge.source_provider_name.as_deref(),
            Some("OpenAI Primary")
        );
        assert!(bridge.bridged);

        let plain = info.get(&7).expect("plain provider info");
        assert_eq!(plain.source_provider_id, None);
        assert!(!plain.bridged);

        assert!(!info.contains_key(&99));
    }

    #[test]
    fn list_queries_hide_claude_non_messages_rows() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_request_log(&conn, 1, "trace-claude-messages", "claude", "/v1/messages");
        seed_request_log(
            &conn,
            2,
            "trace-claude-count",
            "claude",
            "/v1/messages/count_tokens",
        );
        seed_request_log(&conn, 3, "trace-codex", "codex", "/v1/responses");
        drop(conn);

        let all = list_recent_all(&db, 10).unwrap();
        assert_eq!(
            all.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![3, 1]
        );

        let claude = list_recent(&db, "claude", 10).unwrap();
        assert_eq!(
            claude.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![1]
        );

        let after = list_after_id_all(&db, 1, 10).unwrap();
        assert_eq!(
            after.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![3]
        );
    }

    #[test]
    fn terminal_trace_ids_returns_only_persisted_terminal_logs() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_request_log(&conn, 1, "trace-status-terminal", "codex", "/v1/responses");
        seed_request_log(&conn, 2, "trace-error-terminal", "codex", "/v1/responses");
        seed_request_log(&conn, 3, "trace-pending", "codex", "/v1/responses");
        conn.execute(
            "UPDATE request_logs SET status = NULL, error_code = 'GW_REQUEST_ABORTED' WHERE trace_id = ?1",
            rusqlite::params!["trace-error-terminal"],
        )
        .unwrap();
        conn.execute(
            "UPDATE request_logs SET status = NULL, error_code = NULL WHERE trace_id = ?1",
            rusqlite::params!["trace-pending"],
        )
        .unwrap();
        drop(conn);

        let terminal = terminal_trace_ids(
            &db,
            &[
                "trace-status-terminal".to_string(),
                "trace-error-terminal".to_string(),
                "trace-pending".to_string(),
                "trace-missing".to_string(),
            ],
        )
        .unwrap();

        assert!(terminal.contains("trace-status-terminal"));
        assert!(terminal.contains("trace-error-terminal"));
        assert!(!terminal.contains("trace-pending"));
        assert!(!terminal.contains("trace-missing"));
    }

    #[test]
    fn detail_queries_hide_claude_non_messages_rows() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_request_log(&conn, 1, "trace-claude-messages", "claude", "/v1/messages");
        seed_request_log(
            &conn,
            2,
            "trace-claude-count",
            "claude",
            "/v1/messages/count_tokens",
        );
        seed_request_log(&conn, 3, "trace-codex", "codex", "/v1/responses");
        drop(conn);

        let visible = get_by_id(&db, 1).unwrap();
        assert_eq!(visible.id, 1);

        let hidden = get_by_id(&db, 2).unwrap_err().to_string();
        assert!(hidden.contains("request_log not found"));

        let hidden_by_trace = get_by_trace_id(&db, "trace-claude-count").unwrap();
        assert!(hidden_by_trace.is_none());

        let visible_by_trace = get_by_trace_id(&db, "trace-codex").unwrap();
        assert_eq!(visible_by_trace.as_ref().map(|item| item.id), Some(3));
    }

    #[test]
    fn summary_and_detail_expose_session_id() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        conn.execute(
            r#"
INSERT INTO request_logs (
  id, trace_id, cli_key, session_id, method, path, query, excluded_from_stats,
  special_settings_json, status, error_code, duration_ms, ttfb_ms, attempts_json,
  input_tokens, output_tokens, total_tokens, cache_read_input_tokens,
  cache_creation_input_tokens, cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens, usage_json, requested_model, cost_usd_femto,
  cost_multiplier, created_at_ms, created_at, final_provider_id
) VALUES (?1, ?2, ?3, ?4, 'POST', ?5, NULL, 0, NULL, 200, NULL, 10, 5, '[]',
  NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'model', NULL, 1.0, ?6, ?7, 0)
"#,
            rusqlite::params![
                11_i64,
                "trace-session-id",
                "codex",
                "sess-123",
                "/v1/responses",
                11_000_i64,
                11_i64
            ],
        )
        .unwrap();
        drop(conn);

        let summary = list_recent_all(&db, 10).unwrap();
        assert_eq!(summary[0].session_id.as_deref(), Some("sess-123"));

        let detail = get_by_id(&db, 11).unwrap();
        assert_eq!(detail.session_id.as_deref(), Some("sess-123"));
    }

    #[test]
    fn codex_reasoning_guard_stats_split_by_model_and_ratio() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-codex-hit-a",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_effort","effort":"high"},{"type":"codex_reasoning_guard"},{"type":"codex_reasoning_guard"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            2,
            "trace-codex-normal-a",
            Some("gpt-5-codex"),
            None,
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            3,
            "trace-codex-hit-b",
            Some("gpt-5-mini-codex"),
            Some(r#"[{"type":"codex_reasoning_guard"}]"#),
        );
        seed_codex_request_log_with_special_settings(&conn, 4, "trace-codex-unknown", None, None);
        seed_codex_request_log_with_special_settings(
            &conn,
            5,
            "trace-codex-default-effort",
            Some("gpt-5.4-mini"),
            None,
        );
        drop(conn);

        let stats = codex_reasoning_guard_stats(&db, None, None).unwrap();
        assert_eq!(stats.hit_request_count, 2);
        assert_eq!(stats.hit_attempt_count, 3);
        assert_eq!(stats.token_hit_attempt_count, 3);
        assert_eq!(stats.feature_hit_attempt_count, 0);
        assert_eq!(stats.reasoning_token_hit_request_count, 2);
        assert_eq!(stats.final_answer_only_high_xhigh_hit_request_count, 0);
        assert_eq!(stats.normal_request_count, 3);
        assert_eq!(stats.total_request_count, 5);
        assert!((stats.hit_rate - 0.4).abs() < f64::EPSILON);
        assert_eq!(stats.feature_sample_request_count, 0);
        assert_eq!(stats.feature_sample_count, 0);
        assert_eq!(stats.final_answer_only_sample_count, 0);
        assert_eq!(stats.high_xhigh_final_answer_only_sample_count, 0);
        assert_eq!(stats.reasoning_516_final_answer_only_no_commentary_count, 0);
        assert_eq!(stats.compaction_exempt_sample_count, 0);

        assert_eq!(stats.by_model.len(), 4);
        assert_eq!(stats.by_model[0].requested_model, "gpt-5-codex");
        assert_eq!(stats.by_model[0].total_request_count, 2);
        assert_eq!(stats.by_model[0].hit_request_count, 1);
        assert_eq!(stats.by_model[0].normal_request_count, 1);
        assert_eq!(stats.by_model[0].hit_attempt_count, 2);
        assert!((stats.by_model[0].hit_rate - 0.5).abs() < f64::EPSILON);

        assert_eq!(stats.by_model[1].requested_model, "gpt-5-mini-codex");
        assert_eq!(stats.by_model[1].total_request_count, 1);
        assert_eq!(stats.by_model[1].hit_request_count, 1);
        assert_eq!(stats.by_model[1].normal_request_count, 0);
        assert_eq!(stats.by_model[1].hit_attempt_count, 1);
        assert!((stats.by_model[1].hit_rate - 1.0).abs() < f64::EPSILON);

        assert_eq!(stats.by_model[2].requested_model, "gpt-5.4-mini");
        assert_eq!(stats.by_model[2].total_request_count, 1);
        assert_eq!(stats.by_model[2].hit_request_count, 0);
        assert_eq!(stats.by_model[2].normal_request_count, 1);
        assert_eq!(stats.by_model[2].hit_attempt_count, 0);
        assert!((stats.by_model[2].hit_rate - 0.0).abs() < f64::EPSILON);

        assert_eq!(
            stats.by_model[3].requested_model,
            super::UNKNOWN_CODEX_REQUESTED_MODEL_LABEL
        );
        assert_eq!(stats.by_model[3].total_request_count, 1);
        assert_eq!(stats.by_model[3].hit_request_count, 0);
        assert_eq!(stats.by_model[3].normal_request_count, 1);
        assert_eq!(stats.by_model[3].hit_attempt_count, 0);
        assert!((stats.by_model[3].hit_rate - 0.0).abs() < f64::EPSILON);

        assert_eq!(stats.by_model_and_effort.len(), 5);
        let codex_high = stats
            .by_model_and_effort
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex" && row.reasoning_effort == "high")
            .expect("gpt-5-codex high stat");
        assert_eq!(codex_high.total_request_count, 1);
        assert_eq!(codex_high.hit_request_count, 1);
        assert_eq!(codex_high.hit_attempt_count, 2);
        assert!((codex_high.hit_rate - 1.0).abs() < f64::EPSILON);

        let mini_unknown = stats
            .by_model_and_effort
            .iter()
            .find(|row| {
                row.requested_model == "gpt-5-mini-codex" && row.reasoning_effort == "unknown"
            })
            .expect("gpt-5-mini-codex unknown stat");
        assert_eq!(mini_unknown.hit_request_count, 1);

        let mini_default_none = stats
            .by_model_and_effort
            .iter()
            .find(|row| row.requested_model == "gpt-5.4-mini" && row.reasoning_effort == "none")
            .expect("gpt-5.4-mini none stat");
        assert_eq!(mini_default_none.normal_request_count, 1);
    }

    #[test]
    fn codex_reasoning_guard_stats_ignore_no_intercept_decision_records() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-decision-only",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_effort","effort":"high"},{"type":"codex_reasoning_guard_decision","hit":false,"matchedRuleAction":"no_intercept"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            2,
            "trace-hit-with-decision",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_effort","effort":"high"},{"type":"codex_reasoning_guard","hit":true},{"type":"codex_reasoning_guard_decision","hit":false,"matchedRuleAction":"no_intercept"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            3,
            "trace-normal",
            Some("gpt-5-mini-codex"),
            None,
        );
        drop(conn);

        let stats = codex_reasoning_guard_stats(&db, None, None).unwrap();

        assert_eq!(stats.total_request_count, 3);
        assert_eq!(stats.hit_request_count, 1);
        assert_eq!(stats.hit_attempt_count, 1);
        assert_eq!(stats.normal_request_count, 2);
        assert_eq!(stats.token_hit_attempt_count, 1);

        let codex_model = stats
            .by_model
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex")
            .expect("gpt-5-codex model stats");
        assert_eq!(codex_model.total_request_count, 2);
        assert_eq!(codex_model.hit_request_count, 1);
        assert_eq!(codex_model.normal_request_count, 1);
        assert_eq!(codex_model.hit_attempt_count, 1);

        let codex_high = stats
            .by_model_and_effort
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex" && row.reasoning_effort == "high")
            .expect("gpt-5-codex high stats");
        assert_eq!(codex_high.total_request_count, 2);
        assert_eq!(codex_high.hit_request_count, 1);
        assert_eq!(codex_high.normal_request_count, 1);
        assert_eq!(codex_high.hit_attempt_count, 1);
    }

    #[test]
    fn codex_reasoning_guard_stats_counts_feature_samples_separately() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-feature-active",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_features","ruleMode":"final_answer_only_high_xhigh","reasoningTokens":516,"requestReasoningEffort":"high","responseClassification":"complete","finalAnswerOnly":true,"commentaryObserved":false,"interceptExemptReason":null},{"type":"codex_reasoning_guard","hitSource":"final_answer_only_high_xhigh"}]"#,
            ),
        );
        conn.execute(
            "UPDATE request_logs SET output_tokens = 42 WHERE id = 1",
            [],
        )
        .unwrap();
        seed_codex_request_log_with_special_settings(
            &conn,
            2,
            "trace-feature-compaction",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_features","ruleMode":"final_answer_only_high_xhigh","reasoningTokens":null,"requestReasoningEffort":"xhigh","responseClassification":"request_only","classificationSkippedReason":"guard_disabled_stream_not_buffered","finalAnswerOnly":null,"commentaryObserved":null,"interceptExemptReason":"context_compaction"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            3,
            "trace-token-active",
            Some("gpt-5-mini-codex"),
            Some(
                r#"[{"type":"codex_reasoning_features","ruleMode":"reasoning_tokens","reasoningTokens":516,"requestReasoningEffort":"low","responseClassification":"complete","finalAnswerOnly":true,"commentaryObserved":false,"interceptExemptReason":null},{"type":"codex_reasoning_guard"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            4,
            "trace-normal",
            Some("gpt-5-mini-codex"),
            None,
        );
        drop(conn);

        let stats = codex_reasoning_guard_stats(&db, None, None).unwrap();
        assert_eq!(stats.total_request_count, 4);
        assert_eq!(stats.hit_request_count, 2);
        assert_eq!(stats.hit_attempt_count, 2);
        assert_eq!(stats.token_hit_attempt_count, 1);
        assert_eq!(stats.feature_hit_attempt_count, 1);
        assert_eq!(stats.reasoning_token_hit_request_count, 1);
        assert_eq!(stats.final_answer_only_high_xhigh_hit_request_count, 1);
        assert_eq!(stats.normal_request_count, 2);

        assert_eq!(stats.feature_sample_request_count, 3);
        assert_eq!(stats.feature_sample_count, 3);
        assert_eq!(stats.final_answer_only_sample_count, 2);
        assert_eq!(stats.high_xhigh_final_answer_only_sample_count, 1);
        assert_eq!(stats.reasoning_516_final_answer_only_no_commentary_count, 2);
        assert_eq!(stats.compaction_exempt_sample_count, 1);
        assert_eq!(stats.reasoning_tokens_coverage_count, 2);
        assert_eq!(stats.final_answer_only_coverage_count, 2);
        assert_eq!(stats.commentary_observed_coverage_count, 2);
        assert_eq!(stats.reasoning_effort_coverage_count, 3);
        assert_eq!(stats.duration_ms_coverage_count, 3);
        assert_eq!(stats.output_tokens_coverage_count, 1);
    }

    #[test]
    fn codex_reasoning_guard_stats_counts_continuation_repair_separately() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-continuation-repaired",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_continuation","status":"repaired","sentRounds":1},{"type":"codex_reasoning_guard","ruleSource":"template_builtin","hitSource":"reasoning_tokens","guardPostMatchStrategy":"continuation_repair","guardStrategyOutcome":"continuation_repaired","continuationSentRounds":1,"matchedRuleName":"reasoning_tokens == 518*n-2"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            2,
            "trace-continuation-multi-failure",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_continuation","status":"still_matched","sentRounds":3},{"type":"codex_reasoning_guard","ruleSource":"continuation_repair","hitSource":"reasoning_tokens","matchedRuleName":"reasoning_tokens == 518*n-2"},{"type":"codex_reasoning_continuation","status":"failed","sentRounds":2},{"type":"codex_reasoning_guard","ruleSource":"continuation_repair","hitSource":"reasoning_tokens","matchedRuleName":"reasoning_tokens == 518*n-2"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            3,
            "trace-continuation-missing",
            Some("gpt-5-mini-codex"),
            Some(
                r#"[{"type":"codex_reasoning_continuation","status":"missing_encrypted","sentRounds":0},{"type":"codex_reasoning_guard","ruleSource":"continuation_repair","hitSource":"reasoning_tokens","matchedRuleName":"reasoning_tokens == 518*n-2"}]"#,
            ),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            4,
            "trace-no-continuation",
            Some("gpt-5-mini-codex"),
            None,
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            5,
            "trace-continuation-experimental-repaired",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_continuation","status":"repaired","sentRounds":2,"clientContractVersion":"bplus_protocol_reconstruction_v8"},{"type":"codex_reasoning_guard","ruleSource":"template_builtin","hitSource":"reasoning_tokens","guardPostMatchStrategy":"continuation_repair_experimental","guardStrategyOutcome":"continuation_repaired","continuationSentRounds":2,"matchedRuleName":"reasoning_tokens == 518*n-2"}]"#,
            ),
        );
        drop(conn);

        let stats = codex_reasoning_guard_stats(&db, None, None).unwrap();

        assert_eq!(stats.total_request_count, 5);
        assert_eq!(stats.hit_request_count, 4);
        assert_eq!(stats.hit_attempt_count, 5);
        assert_eq!(stats.token_hit_attempt_count, 5);
        assert_eq!(stats.feature_hit_attempt_count, 0);
        assert_eq!(stats.reasoning_token_hit_request_count, 4);
        assert_eq!(stats.final_answer_only_high_xhigh_hit_request_count, 0);
        assert_eq!(stats.normal_request_count, 1);
        let codex_model = stats
            .by_model
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex")
            .expect("gpt-5-codex model stats");
        assert_eq!(codex_model.hit_request_count, 3);
        assert_eq!(codex_model.hit_attempt_count, 4);
        let mini_model = stats
            .by_model
            .iter()
            .find(|row| row.requested_model == "gpt-5-mini-codex")
            .expect("gpt-5-mini-codex model stats");
        assert_eq!(mini_model.hit_request_count, 1);
        assert_eq!(mini_model.hit_attempt_count, 1);
        assert_eq!(stats.continuation_triggered_request_count, 4);
        assert_eq!(stats.continuation_triggered_attempt_count, 5);
        assert_eq!(stats.continuation_repaired_request_count, 2);
        assert_eq!(stats.continuation_repaired_attempt_count, 2);
        assert_eq!(stats.continuation_non_repaired_attempt_count, 3);
        assert!((stats.continuation_repair_rate - 0.5).abs() < f64::EPSILON);
        assert!((stats.continuation_average_sent_rounds - 1.6).abs() < f64::EPSILON);

        let repaired = stats
            .continuation_by_status
            .iter()
            .find(|row| row.status == "repaired")
            .expect("repaired continuation stat");
        assert_eq!(repaired.request_count, 2);
        assert_eq!(repaired.attempt_count, 2);
        assert!((repaired.average_sent_rounds - 1.5).abs() < f64::EPSILON);

        let still_matched = stats
            .continuation_by_status
            .iter()
            .find(|row| row.status == "still_matched")
            .expect("still_matched continuation stat");
        assert_eq!(still_matched.request_count, 1);
        assert_eq!(still_matched.attempt_count, 1);
        assert!((still_matched.average_sent_rounds - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn codex_reasoning_guard_stats_counts_mixed_guard_and_continuation_once() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-mixed-guard-continuation",
            Some("gpt-5-codex"),
            Some(
                r#"[{"type":"codex_reasoning_effort","effort":"high"},{"type":"codex_reasoning_guard","hitSource":"reasoning_tokens"},{"type":"codex_reasoning_continuation","status":"repaired","sentRounds":2},{"type":"codex_reasoning_guard","ruleSource":" Continuation_Repair ","hitSource":"reasoning_tokens","matchedRuleName":"reasoning_tokens == 518*n-2"}]"#,
            ),
        );
        drop(conn);

        let stats = codex_reasoning_guard_stats(&db, None, None).unwrap();

        assert_eq!(stats.total_request_count, 1);
        assert_eq!(stats.hit_request_count, 1);
        assert_eq!(stats.hit_attempt_count, 2);
        assert_eq!(stats.token_hit_attempt_count, 2);
        assert_eq!(stats.feature_hit_attempt_count, 0);
        assert_eq!(stats.reasoning_token_hit_request_count, 1);
        assert_eq!(stats.continuation_triggered_request_count, 1);
        assert_eq!(stats.continuation_triggered_attempt_count, 1);
        assert_eq!(stats.continuation_repaired_request_count, 1);
        assert_eq!(stats.continuation_repaired_attempt_count, 1);
        assert_eq!(stats.continuation_non_repaired_attempt_count, 0);

        let model_stats = stats
            .by_model
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex")
            .expect("gpt-5-codex model stats");
        assert_eq!(model_stats.hit_request_count, 1);
        assert_eq!(model_stats.hit_attempt_count, 2);

        let high_stats = stats
            .by_model_and_effort
            .iter()
            .find(|row| row.requested_model == "gpt-5-codex" && row.reasoning_effort == "high")
            .expect("gpt-5-codex high stats");
        assert_eq!(high_stats.hit_request_count, 1);
        assert_eq!(high_stats.hit_attempt_count, 2);
    }

    #[test]
    fn codex_reasoning_guard_stats_filters_by_created_at_ms_cutoff() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();
        let conn = db.open_connection().unwrap();

        seed_codex_request_log_with_special_settings(
            &conn,
            0,
            "trace-codex-legacy",
            Some("gpt-5-codex"),
            Some(r#"[{"type":"codex_reasoning_guard"}]"#),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            1,
            "trace-codex-before-cutoff",
            Some("gpt-5-codex"),
            None,
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            2,
            "trace-codex-after-cutoff-hit",
            Some("gpt-5-codex"),
            Some(r#"[{"type":"codex_reasoning_guard"},{"type":"codex_reasoning_guard"}]"#),
        );
        seed_codex_request_log_with_special_settings(
            &conn,
            3,
            "trace-codex-after-cutoff-normal",
            Some("gpt-5-mini-codex"),
            None,
        );
        drop(conn);

        let all_stats = codex_reasoning_guard_stats(&db, None, None).unwrap();
        assert_eq!(all_stats.hit_request_count, 2);
        assert_eq!(all_stats.hit_attempt_count, 3);
        assert_eq!(all_stats.normal_request_count, 2);
        assert_eq!(all_stats.total_request_count, 4);

        let session_stats = codex_reasoning_guard_stats(&db, Some(2_000), None).unwrap();
        assert_eq!(session_stats.hit_request_count, 1);
        assert_eq!(session_stats.hit_attempt_count, 2);
        assert_eq!(session_stats.normal_request_count, 1);
        assert_eq!(session_stats.total_request_count, 2);
        assert_eq!(session_stats.by_model.len(), 2);
        assert_eq!(session_stats.by_model[0].requested_model, "gpt-5-codex");
        assert_eq!(session_stats.by_model[0].hit_request_count, 1);
        assert_eq!(
            session_stats.by_model[1].requested_model,
            "gpt-5-mini-codex"
        );
        assert_eq!(session_stats.by_model[1].normal_request_count, 1);
    }

    #[test]
    fn codex_reasoning_guard_stats_rejects_non_positive_cutoff() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("request-logs.db");
        let db = db::init_for_tests(&db_path).unwrap();

        let zero_error = codex_reasoning_guard_stats(&db, Some(0), None)
            .unwrap_err()
            .to_string();
        assert!(zero_error.contains("invalid codex reasoning guard stats cutoff"));

        let negative_error = codex_reasoning_guard_stats(&db, Some(-1), None)
            .unwrap_err()
            .to_string();
        assert!(negative_error.contains("invalid codex reasoning guard stats cutoff"));
    }
}
