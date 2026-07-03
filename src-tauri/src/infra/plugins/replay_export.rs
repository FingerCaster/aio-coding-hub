//! Usage: Export host-owned plugin replay fixtures from request traces.

use crate::db;
use crate::domain::plugins::{
    PluginReplayFixture, PluginReplayFixtureAttempt, PluginReplayFixtureLog,
    PluginReplayFixtureRequest, PluginReplayFixtureResponse, PluginReplayFixtureSource,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::time::now_unix_millis;

const PLUGIN_REPLAY_FIXTURE_SCHEMA_VERSION: u32 = 1;
const PLUGIN_REPLAY_ATTEMPT_LIMIT: usize = 200;
const PLUGIN_REPLAY_RUNTIME_REPORT_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub(crate) struct ExportPluginReplayFixtureInput {
    pub(crate) trace_id: String,
    pub(crate) hook_name: String,
    pub(crate) plugin_id: Option<String>,
}

pub(crate) fn export_plugin_replay_fixture(
    db: &db::Db,
    input: ExportPluginReplayFixtureInput,
) -> AppResult<PluginReplayFixture> {
    let trace_id = normalize_required_text("trace_id", input.trace_id)?;
    let hook_name = normalize_required_text("hook_name", input.hook_name)?;
    if !crate::gateway::plugins::contract::is_active_hook(&hook_name) {
        return Err(AppError::new(
            "PLUGIN_REPLAY_UNAVAILABLE",
            format!("hook does not support replay export: {hook_name}"),
        ));
    }
    let plugin_id = input
        .plugin_id
        .map(|raw| normalize_required_text("plugin_id", raw))
        .transpose()?;

    let request_log = crate::request_logs::get_by_trace_id(db, &trace_id)?.ok_or_else(|| {
        AppError::new(
            "PLUGIN_REPLAY_UNAVAILABLE",
            format!("request log not found for trace_id: {trace_id}"),
        )
    })?;
    let attempts =
        crate::request_attempt_logs::list_by_trace_id(db, &trace_id, PLUGIN_REPLAY_ATTEMPT_LIMIT)?;
    let runtime_reports = crate::infra::plugins::runtime_reports::list_hook_execution_reports(
        db,
        plugin_id.as_deref(),
        Some(&hook_name),
        Some(&trace_id),
        PLUGIN_REPLAY_RUNTIME_REPORT_LIMIT,
    )?;

    let mut notes = vec![
        "request body is not persisted in request_logs; fixture includes host metadata, attempts, and plugin runtime reports only".to_string(),
        "response body, stream chunks, and log body are not persisted in request_logs".to_string(),
    ];
    if runtime_reports.is_empty() {
        notes.push(match plugin_id.as_deref() {
            Some(plugin_id) => format!(
                "no runtime reports were found for plugin {plugin_id}, hook {hook_name}, trace {trace_id}"
            ),
            None => format!("no runtime reports were found for hook {hook_name}, trace {trace_id}"),
        });
    }

    let provider_chain = parse_optional_json(request_log.provider_chain_json.as_deref());
    let error_details = parse_optional_json(request_log.error_details_json.as_deref());
    let usage = parse_optional_json(request_log.usage_json.as_deref());
    let special_settings = parse_optional_json(request_log.special_settings_json.as_deref());

    Ok(PluginReplayFixture {
        schema_version: PLUGIN_REPLAY_FIXTURE_SCHEMA_VERSION,
        trace_id: trace_id.clone(),
        source: PluginReplayFixtureSource {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            trace_id: trace_id.clone(),
            exported_at_ms: now_unix_millis(),
            request_log_id: request_log.id,
            created_at_ms: request_log.created_at_ms,
        },
        hook_name,
        plugin_id,
        request: PluginReplayFixtureRequest {
            cli_key: request_log.cli_key.clone(),
            session_id: request_log.session_id.clone(),
            method: Some(request_log.method.clone()),
            path: Some(request_log.path.clone()),
            query: request_log.query.clone(),
            provider: Some(request_log.final_provider_name.clone()),
            provider_source: request_log.final_provider_source_name.clone(),
            model: request_log.requested_model.clone(),
            headers: None,
            body: None,
            normalized_messages: Vec::new(),
            meta: serde_json::json!({
                "excludedFromStats": request_log.excluded_from_stats,
                "durationMs": request_log.duration_ms,
                "ttfbMs": request_log.ttfb_ms,
                "inputTokens": request_log.input_tokens,
                "outputTokens": request_log.output_tokens,
                "totalTokens": request_log.total_tokens,
                "cacheReadInputTokens": request_log.cache_read_input_tokens,
                "cacheCreationInputTokens": request_log.cache_creation_input_tokens,
                "cacheCreation5mInputTokens": request_log.cache_creation_5m_input_tokens,
                "cacheCreation1hInputTokens": request_log.cache_creation_1h_input_tokens,
                "costUsd": request_log.cost_usd,
                "costMultiplier": request_log.cost_multiplier,
                "providerChain": provider_chain,
                "specialSettings": special_settings,
            }),
        },
        response: PluginReplayFixtureResponse {
            status: request_log.status,
            error_code: request_log.error_code.clone(),
            headers: None,
            body: None,
            chunks: Vec::new(),
            meta: serde_json::json!({
                "errorDetails": error_details,
                "usage": usage,
            }),
        },
        log: PluginReplayFixtureLog {
            body: None,
            meta: serde_json::json!({
                "requestLogCreatedAt": request_log.created_at,
                "requestLogCreatedAtMs": request_log.created_at_ms,
            }),
        },
        attempts: attempts
            .into_iter()
            .map(|attempt| PluginReplayFixtureAttempt {
                id: attempt.id,
                trace_id: attempt.trace_id,
                cli_key: attempt.cli_key,
                attempt_index: attempt.attempt_index,
                provider_id: attempt.provider_id,
                provider_name: attempt.provider_name,
                base_url: attempt.base_url,
                outcome: attempt.outcome,
                status: attempt.status,
                attempt_started_ms: attempt.attempt_started_ms,
                attempt_duration_ms: attempt.attempt_duration_ms,
                created_at: attempt.created_at,
            })
            .collect(),
        runtime_reports,
        notes,
    })
}

fn normalize_required_text(label: &str, raw: String) -> AppResult<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            format!("{label} is required"),
        ));
    }
    Ok(value.to_string())
}

fn parse_optional_json(raw: Option<&str>) -> Option<serde_json::Value> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(raw)
        .ok()
        .or_else(|| Some(serde_json::Value::String(raw.to_string())))
}

#[cfg(test)]
mod tests {
    use crate::infra::plugins::runtime_reports::{
        record_hook_execution_report, RecordPluginHookExecutionReportInput,
    };

    #[test]
    fn export_replay_fixture_uses_trace_attempts_and_runtime_reports() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let conn = db.open_connection().unwrap();

        conn.execute(
            r#"
INSERT INTO request_logs (
  id, trace_id, cli_key, session_id, method, path, query, excluded_from_stats,
  special_settings_json, status, error_code, duration_ms, ttfb_ms, attempts_json,
  input_tokens, output_tokens, total_tokens, cache_read_input_tokens,
  cache_creation_input_tokens, cache_creation_5m_input_tokens,
  cache_creation_1h_input_tokens, usage_json, requested_model, cost_usd_femto,
  cost_multiplier, created_at_ms, created_at, final_provider_id,
  provider_chain_json, error_details_json
) VALUES (
  1, 'trace-replay-1', 'codex', 'session-replay', 'POST', '/v1/responses', '?stream=false', 0,
  '{"temperature":0.2}', 200, NULL, 42, 11,
  '[{"provider_id":7,"provider_name":"OpenAI Primary","base_url":"https://api.openai.example/v1","outcome":"success","status":200,"attempt_started_ms":1000,"attempt_duration_ms":41}]',
  10, 20, 30, NULL, NULL, NULL, NULL, '{"total_tokens":30}', 'gpt-5-mini',
  NULL, 1.0, 1700000000000, 1700000000, 7,
  '[{"provider_id":7,"provider_name":"OpenAI Primary","outcome":"success"}]',
  NULL
)
"#,
            [],
        )
        .unwrap();
        drop(conn);

        record_hook_execution_report(
            &db,
            RecordPluginHookExecutionReportInput {
                plugin_id: "community.prompt-helper".to_string(),
                trace_id: Some("trace-replay-1".to_string()),
                hook_name: "gateway.request.afterBodyRead".to_string(),
                runtime_kind: "extensionHost".to_string(),
                status: "completed".to_string(),
                started_at_ms: 1_700_000_000_001,
                duration_ms: 8,
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

        let fixture = super::export_plugin_replay_fixture(
            &db,
            super::ExportPluginReplayFixtureInput {
                trace_id: " trace-replay-1 ".to_string(),
                hook_name: "gateway.request.afterBodyRead".to_string(),
                plugin_id: Some("community.prompt-helper".to_string()),
            },
        )
        .unwrap();

        assert_eq!(fixture.trace_id, "trace-replay-1");
        assert_eq!(fixture.hook_name, "gateway.request.afterBodyRead");
        assert_eq!(fixture.source.trace_id, "trace-replay-1");
        assert_eq!(fixture.request.method.as_deref(), Some("POST"));
        assert_eq!(fixture.request.path.as_deref(), Some("/v1/responses"));
        assert_eq!(fixture.request.model.as_deref(), Some("gpt-5-mini"));
        assert!(fixture.request.body.is_none());
        assert!(!fixture.attempts.is_empty());
        assert_eq!(fixture.runtime_reports.len(), 1);
        assert_eq!(
            fixture.runtime_reports[0].plugin_id,
            "community.prompt-helper"
        );
        assert!(fixture
            .notes
            .iter()
            .any(|note| note.contains("request body is not persisted")));
    }
}
