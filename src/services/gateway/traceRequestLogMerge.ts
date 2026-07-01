import { isPersistedRequestLogInProgress, requestLogCreatedAtMs } from "./requestLogState";
import type { RequestLogDetail, RequestLogSummary } from "./requestLogs";
import {
  hasExplicitCodexReasoningEffortSpecialSetting,
  resolveClaudeModelMappingFromSpecialSettings,
} from "./requestLogSpecialSettings";
import type { TraceSession } from "./traceStore";

export type RequestLogTraceMergeSource = Pick<
  RequestLogSummary | RequestLogDetail,
  | "trace_id"
  | "session_id"
  | "status"
  | "error_code"
  | "duration_ms"
  | "ttfb_ms"
  | "visible_ttfb_ms"
  | "input_tokens"
  | "output_tokens"
  | "total_tokens"
  | "cache_read_input_tokens"
  | "cache_creation_input_tokens"
  | "cache_creation_5m_input_tokens"
  | "cache_creation_1h_input_tokens"
  | "requested_model"
  | "cost_usd"
  | "cost_multiplier"
  | "special_settings_json"
  | "final_provider_id"
  | "created_at_ms"
  | "created_at"
>;

function selectTerminalRequestedModel(
  trace: TraceSession,
  requestLog: RequestLogTraceMergeSource,
  summary: TraceSession["summary"] | undefined
) {
  return requestLog.requested_model ?? trace.requested_model ?? summary?.requested_model ?? null;
}

function selectTerminalSpecialSettingsJson(
  trace: TraceSession,
  requestLog: RequestLogTraceMergeSource,
  summary: TraceSession["summary"] | undefined
) {
  if (requestLog.special_settings_json != null) return requestLog.special_settings_json;
  if (hasExplicitCodexReasoningEffortSpecialSetting(trace.special_settings_json)) {
    return trace.special_settings_json ?? null;
  }
  if (hasExplicitCodexReasoningEffortSpecialSetting(summary?.special_settings_json)) {
    return summary?.special_settings_json ?? null;
  }
  return trace.special_settings_json ?? summary?.special_settings_json ?? null;
}

export function mergeTraceWithRequestLog(
  trace: TraceSession,
  requestLog: RequestLogTraceMergeSource | undefined
): TraceSession {
  if (!requestLog) return trace;

  const requestLogInProgress = isPersistedRequestLogInProgress(requestLog);
  const requestLogTsMs = requestLogCreatedAtMs(requestLog);
  const claudeModelMapping =
    trace.claude_model_mapping ??
    resolveClaudeModelMappingFromSpecialSettings(
      requestLog.special_settings_json,
      requestLog.final_provider_id
    );
  if (!trace.summary && requestLogInProgress) {
    return {
      ...trace,
      session_id: trace.session_id ?? requestLog.session_id ?? null,
      requested_model: trace.requested_model ?? requestLog.requested_model ?? null,
      special_settings_json:
        trace.special_settings_json ?? requestLog.special_settings_json ?? null,
      claude_model_mapping: claudeModelMapping,
      last_seen_ms: Math.max(trace.last_seen_ms, requestLogTsMs),
    };
  }

  const summary = trace.summary;
  const mergedRequestedModel = requestLogInProgress
    ? (trace.requested_model ?? requestLog.requested_model ?? summary?.requested_model ?? null)
    : selectTerminalRequestedModel(trace, requestLog, summary);
  const mergedSpecialSettingsJson = requestLogInProgress
    ? (trace.special_settings_json ??
      requestLog.special_settings_json ??
      summary?.special_settings_json ??
      null)
    : selectTerminalSpecialSettingsJson(trace, requestLog, summary);
  const mergedSummary: NonNullable<TraceSession["summary"]> = {
    trace_id: trace.trace_id,
    cli_key: trace.cli_key,
    method: trace.method,
    path: trace.path,
    query: trace.query,
    requested_model: mergedRequestedModel,
    special_settings_json: mergedSpecialSettingsJson,
    status: requestLog.status ?? summary?.status ?? null,
    error_category: summary?.error_category ?? null,
    error_code: requestLog.error_code ?? summary?.error_code ?? null,
    duration_ms: requestLog.duration_ms ?? summary?.duration_ms ?? 0,
    ttfb_ms: requestLog.ttfb_ms ?? summary?.ttfb_ms ?? null,
    visible_ttfb_ms: requestLog.visible_ttfb_ms ?? summary?.visible_ttfb_ms ?? null,
    attempts: summary?.attempts ?? [],
    input_tokens: requestLog.input_tokens ?? summary?.input_tokens ?? null,
    output_tokens: requestLog.output_tokens ?? summary?.output_tokens ?? null,
    total_tokens: requestLog.total_tokens ?? summary?.total_tokens ?? null,
    cache_read_input_tokens:
      requestLog.cache_read_input_tokens ?? summary?.cache_read_input_tokens ?? null,
    cache_creation_input_tokens:
      requestLog.cache_creation_input_tokens ?? summary?.cache_creation_input_tokens ?? null,
    cache_creation_5m_input_tokens:
      requestLog.cache_creation_5m_input_tokens ?? summary?.cache_creation_5m_input_tokens ?? null,
    cache_creation_1h_input_tokens:
      requestLog.cache_creation_1h_input_tokens ?? summary?.cache_creation_1h_input_tokens ?? null,
    cost_usd: requestLog.cost_usd ?? summary?.cost_usd ?? null,
    cost_multiplier: requestLog.cost_multiplier ?? summary?.cost_multiplier ?? null,
  };

  return {
    ...trace,
    session_id: trace.session_id ?? requestLog.session_id ?? null,
    requested_model: mergedRequestedModel,
    special_settings_json: mergedSpecialSettingsJson,
    claude_model_mapping: claudeModelMapping,
    summary: mergedSummary,
    last_seen_ms: Math.max(trace.last_seen_ms, requestLogTsMs),
  };
}
