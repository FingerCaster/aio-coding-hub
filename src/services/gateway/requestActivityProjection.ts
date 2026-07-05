// 契约：请求"进行中"判定的单一真值来源 = activeRequests 注册表成员身份，本模块是唯一判定处。
// requestLogState 只负责持久化终态（completed / interrupted）的分类，不判定进行中。

import {
  isRequestLogActivityInProgress,
  requestLogActiveActivityState,
  requestLogActivityState,
  requestLogCreatedAtMs,
  type RequestLogActivityState,
} from "./requestLogState";
import type { RequestLogSummary } from "./requestLogs";
import { resolveClaudeModelMappingFromSpecialSettings } from "./requestLogSpecialSettings";
import type { TraceSession } from "./traceStore";

export const REALTIME_TRACE_EXIT_START_MS = 600;
const REALTIME_TRACE_EXIT_ANIM_MS = 400;
const REALTIME_TRACE_EXIT_TOTAL_MS =
  REALTIME_TRACE_EXIT_START_MS + REALTIME_TRACE_EXIT_ANIM_MS + 100;

export type ProjectedRealtimeCard = {
  trace: TraceSession;
  // Registry entry backing this card, when the request is known to the gateway
  // active-request registry. Drives the idle ("已静默") notice for live cards.
  activeRequest?: ActiveRequestSnapshotItem | null;
};

export type ProjectedRequestLogRow = {
  log: RequestLogSummary;
  liveTrace: TraceSession | null;
  activityState: RequestLogActivityState;
  activeRequest: ActiveRequestSnapshotItem | null;
};

export type RequestActivityProjection = {
  realtimeCards: ProjectedRealtimeCard[];
  requestRows: ProjectedRequestLogRow[];
  visibleRealtimeTraceIds: Set<string>;
  hasPending: boolean;
  hasLiveRealtimeCards: boolean;
  summaryCount: number;
};

export type BuildRequestActivityProjectionInput = {
  requestLogs: RequestLogSummary[];
  activeRequests?: ActiveRequestSnapshotItem[];
  traces: TraceSession[];
  nowMs: number;
  realtimeCardLimit: number;
  realtimeCandidateLimit: number;
};

export type ActiveRequestSnapshotItem = {
  trace_id: string;
  cli_key: RequestLogSummary["cli_key"] | string;
  session_id?: string | null;
  method: string;
  path: string;
  query?: string | null;
  requested_model?: string | null;
  created_at_ms: number;
  last_activity_ms: number;
};

function normalizeTraceId(traceId: string | null | undefined) {
  return traceId?.trim() || null;
}

// A persisted log with a terminal outcome must never be re-marked in progress,
// even if a stale active-request snapshot still lists its trace id.
function isRequestLogTerminal(log: RequestLogSummary) {
  return log.status != null || log.error_code != null;
}

function requestLogActivitySortRank(
  log: RequestLogSummary,
  activeByTraceId: Map<string, ActiveRequestSnapshotItem>
) {
  const traceId = normalizeTraceId(log.trace_id);
  if (traceId != null && activeByTraceId.has(traceId) && !isRequestLogTerminal(log)) return 0;
  return requestLogActivityState(log) === "interrupted" ? 2 : 1;
}

function sortRequestLogsForActivity(
  activeByTraceId: Map<string, ActiveRequestSnapshotItem>,
  a: RequestLogSummary,
  b: RequestLogSummary
) {
  const aTraceId = normalizeTraceId(a.trace_id);
  const bTraceId = normalizeTraceId(b.trace_id);
  const aRank = requestLogActivitySortRank(a, activeByTraceId);
  const bRank = requestLogActivitySortRank(b, activeByTraceId);
  if (aRank !== bRank) return aRank - bRank;

  const aTsMs = aTraceId
    ? (activeByTraceId.get(aTraceId)?.created_at_ms ?? requestLogCreatedAtMs(a))
    : requestLogCreatedAtMs(a);
  const bTsMs = bTraceId
    ? (activeByTraceId.get(bTraceId)?.created_at_ms ?? requestLogCreatedAtMs(b))
    : requestLogCreatedAtMs(b);
  if (aTsMs !== bTsMs) return bTsMs - aTsMs;
  return b.id - a.id;
}

function shouldKeepProjectedRealtimeTraceVisible(trace: TraceSession, nowMs: number) {
  if (!trace.summary) return true;
  return Math.max(0, nowMs - trace.last_seen_ms) < REALTIME_TRACE_EXIT_TOTAL_MS;
}

function mergeTraceWithRequestLog(
  trace: TraceSession,
  requestLog: RequestLogSummary | undefined,
  activeRequest: ActiveRequestSnapshotItem | undefined
): TraceSession {
  if (!requestLog) return trace;

  const requestLogTsMs = requestLogCreatedAtMs(requestLog);
  const claudeModelMapping =
    trace.claude_model_mapping ??
    resolveClaudeModelMappingFromSpecialSettings(
      requestLog.special_settings_json,
      requestLog.final_provider_id
    );
  if (!trace.summary && activeRequest) {
    return {
      ...trace,
      session_id: trace.session_id ?? requestLog.session_id ?? null,
      requested_model: trace.requested_model ?? requestLog.requested_model ?? null,
      claude_model_mapping: claudeModelMapping,
      last_seen_ms: Math.max(trace.last_seen_ms, requestLogTsMs),
    };
  }

  const summary = trace.summary;
  const mergedSummary: NonNullable<TraceSession["summary"]> = {
    trace_id: trace.trace_id,
    cli_key: trace.cli_key,
    session_id: summary?.session_id ?? trace.session_id ?? requestLog.session_id ?? null,
    method: trace.method,
    path: trace.path,
    query: trace.query,
    requested_model:
      summary?.requested_model ?? trace.requested_model ?? requestLog.requested_model ?? null,
    claude_model_mapping: summary?.claude_model_mapping ?? claudeModelMapping ?? null,
    status: summary?.status ?? requestLog.status ?? null,
    error_category: summary?.error_category ?? null,
    error_code: summary?.error_code ?? requestLog.error_code ?? null,
    duration_ms: summary?.duration_ms ?? requestLog.duration_ms ?? 0,
    ttfb_ms: summary?.ttfb_ms ?? requestLog.ttfb_ms ?? null,
    attempts: summary?.attempts ?? [],
    input_tokens: summary?.input_tokens ?? requestLog.input_tokens ?? null,
    output_tokens: summary?.output_tokens ?? requestLog.output_tokens ?? null,
    total_tokens: summary?.total_tokens ?? requestLog.total_tokens ?? null,
    cache_read_input_tokens:
      summary?.cache_read_input_tokens ?? requestLog.cache_read_input_tokens ?? null,
    cache_creation_input_tokens:
      summary?.cache_creation_input_tokens ?? requestLog.cache_creation_input_tokens ?? null,
    cache_creation_5m_input_tokens:
      summary?.cache_creation_5m_input_tokens ?? requestLog.cache_creation_5m_input_tokens ?? null,
    cache_creation_1h_input_tokens:
      summary?.cache_creation_1h_input_tokens ?? requestLog.cache_creation_1h_input_tokens ?? null,
    effective_input_tokens:
      summary?.effective_input_tokens ?? requestLog.effective_input_tokens ?? null,
    cost_usd: summary?.cost_usd ?? requestLog.cost_usd ?? null,
    cost_multiplier: summary?.cost_multiplier ?? requestLog.cost_multiplier ?? null,
  };

  return {
    ...trace,
    session_id: trace.session_id ?? requestLog.session_id ?? null,
    requested_model: trace.requested_model ?? requestLog.requested_model ?? null,
    claude_model_mapping: claudeModelMapping,
    summary: mergedSummary,
    last_seen_ms: Math.max(trace.last_seen_ms, requestLogTsMs),
  };
}

// In-progress requests must always render as realtime cards. When the trace
// store has no live trace (e.g. the request started before this webview
// loaded), synthesize a minimal TraceSession from the registry entry.
function traceFromActiveRequest(activeRequest: ActiveRequestSnapshotItem): TraceSession {
  const createdAtMs = Number.isFinite(activeRequest.created_at_ms)
    ? Math.max(0, activeRequest.created_at_ms)
    : 0;
  return {
    trace_id: activeRequest.trace_id,
    cli_key: activeRequest.cli_key,
    session_id: activeRequest.session_id ?? null,
    method: activeRequest.method,
    path: activeRequest.path,
    query: activeRequest.query ?? null,
    requested_model: activeRequest.requested_model ?? null,
    first_seen_ms: createdAtMs,
    last_seen_ms: Math.max(createdAtMs, activeRequest.last_activity_ms ?? 0),
    attempts: [],
  };
}

// Keep every in-progress candidate as a card so the in-progress style never
// forks into the log-row layout; completed (exiting) traces fill what remains
// of the card limit. Candidate order is preserved.
function selectRealtimeCards(
  candidates: TraceSession[],
  limit: number,
  activeByTraceId: Map<string, ActiveRequestSnapshotItem>
): ProjectedRealtimeCard[] {
  const inProgressCount = candidates.reduce((count, t) => count + (t.summary ? 0 : 1), 0);
  let completedBudget = Math.max(0, limit - inProgressCount);
  const selected: ProjectedRealtimeCard[] = [];
  for (const trace of candidates) {
    if (trace.summary) {
      if (completedBudget <= 0) continue;
      completedBudget -= 1;
    }
    const traceId = normalizeTraceId(trace.trace_id);
    selected.push({
      trace,
      activeRequest: traceId ? (activeByTraceId.get(traceId) ?? null) : null,
    });
  }
  return selected;
}

function requestLogFromActiveRequest(
  activeRequest: ActiveRequestSnapshotItem,
  syntheticIndex: number
): RequestLogSummary {
  const createdAtMs = Number.isFinite(activeRequest.created_at_ms)
    ? Math.max(0, activeRequest.created_at_ms)
    : 0;
  return {
    id: -(syntheticIndex + 1),
    trace_id: activeRequest.trace_id,
    cli_key: activeRequest.cli_key as RequestLogSummary["cli_key"],
    session_id: activeRequest.session_id ?? null,
    method: activeRequest.method,
    path: activeRequest.path,
    excluded_from_stats: false,
    special_settings_json: null,
    requested_model: activeRequest.requested_model ?? null,
    status: null,
    error_code: null,
    // Synthetic in-progress rows are by definition not interrupted.
    is_interrupted: false,
    duration_ms: 0,
    ttfb_ms: null,
    attempt_count: 0,
    has_failover: false,
    start_provider_id: 0,
    start_provider_name: "Unknown",
    final_provider_id: 0,
    final_provider_name: "Unknown",
    final_provider_source_id: null,
    final_provider_source_name: null,
    route: [],
    session_reuse: false,
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    cache_read_input_tokens: null,
    cache_creation_input_tokens: null,
    cache_creation_5m_input_tokens: null,
    cache_creation_1h_input_tokens: null,
    effective_input_tokens: null,
    cost_usd: null,
    provider_chain_json: null,
    error_details_json: null,
    cost_multiplier: 1,
    created_at_ms: createdAtMs,
    last_activity_ms: activeRequest.last_activity_ms,
    activity_details_json: null,
    created_at: Math.floor(createdAtMs / 1000),
  };
}

export function buildRequestActivityProjection({
  requestLogs,
  activeRequests = [],
  traces,
  nowMs,
  realtimeCardLimit,
  realtimeCandidateLimit,
}: BuildRequestActivityProjectionInput): RequestActivityProjection {
  const activeByTraceId = new Map<string, ActiveRequestSnapshotItem>();
  for (const activeRequest of activeRequests) {
    const traceId = normalizeTraceId(activeRequest.trace_id);
    if (!traceId || activeByTraceId.has(traceId)) continue;
    activeByTraceId.set(traceId, activeRequest);
  }

  const requestRowsSorted = requestLogs
    .slice()
    .sort((a, b) => sortRequestLogsForActivity(activeByTraceId, a, b));
  const logsByTraceId = new Map<string, RequestLogSummary>();
  for (const log of requestRowsSorted) {
    const traceId = normalizeTraceId(log.trace_id);
    if (!traceId || logsByTraceId.has(traceId)) continue;
    logsByTraceId.set(traceId, log);
  }

  const mergedTraceMap = new Map<string, TraceSession>();
  for (const trace of traces) {
    const traceId = normalizeTraceId(trace.trace_id);
    if (!traceId || mergedTraceMap.has(traceId)) continue;
    mergedTraceMap.set(
      traceId,
      mergeTraceWithRequestLog(trace, logsByTraceId.get(traceId), activeByTraceId.get(traceId))
    );
  }

  for (const [traceId, activeRequest] of activeByTraceId) {
    if (mergedTraceMap.has(traceId)) continue;
    const log = logsByTraceId.get(traceId);
    if (log && isRequestLogTerminal(log)) continue;
    mergedTraceMap.set(
      traceId,
      mergeTraceWithRequestLog(traceFromActiveRequest(activeRequest), log, activeRequest)
    );
  }

  const realtimeCandidates = Array.from(mergedTraceMap.values())
    .filter((trace) => shouldKeepProjectedRealtimeTraceVisible(trace, nowMs))
    .sort((a, b) => b.first_seen_ms - a.first_seen_ms)
    .slice(0, realtimeCandidateLimit);

  const realtimeCards = selectRealtimeCards(realtimeCandidates, realtimeCardLimit, activeByTraceId);
  const visibleRealtimeTraceIds = new Set(
    realtimeCards
      .map((card) => normalizeTraceId(card.trace.trace_id))
      .filter((traceId): traceId is string => traceId != null)
  );

  const requestRows = [];
  for (const log of requestRowsSorted) {
    const traceId = normalizeTraceId(log.trace_id);
    if (traceId && visibleRealtimeTraceIds.has(traceId)) continue;
    const activeRequest = traceId ? (activeByTraceId.get(traceId) ?? null) : null;
    const liveTrace = traceId ? (mergedTraceMap.get(traceId) ?? null) : null;
    const hasEnded = isRequestLogTerminal(log) || Boolean(liveTrace?.summary);
    const activityState =
      activeRequest && !hasEnded
        ? requestLogActiveActivityState(activeRequest.last_activity_ms, nowMs)
        : requestLogActivityState(log);
    requestRows.push({
      log,
      liveTrace,
      activityState,
      activeRequest,
    });
  }

  let syntheticIndex = 0;
  for (const activeRequest of Array.from(activeByTraceId.values()).sort(
    (a, b) => b.created_at_ms - a.created_at_ms
  )) {
    const traceId = normalizeTraceId(activeRequest.trace_id);
    if (!traceId || logsByTraceId.has(traceId) || visibleRealtimeTraceIds.has(traceId)) continue;
    requestRows.push({
      log: requestLogFromActiveRequest(activeRequest, syntheticIndex),
      liveTrace: mergedTraceMap.get(traceId) ?? null,
      activityState: requestLogActiveActivityState(activeRequest.last_activity_ms, nowMs),
      activeRequest,
    });
    syntheticIndex += 1;
  }
  requestRows.sort((a, b) => sortRequestLogsForActivity(activeByTraceId, a.log, b.log));

  const summaryTraceIds = new Set<string>();
  for (const log of requestRowsSorted) {
    const traceId = normalizeTraceId(log.trace_id);
    if (traceId) summaryTraceIds.add(traceId);
  }
  for (const activeRequest of activeByTraceId.values()) {
    const traceId = normalizeTraceId(activeRequest.trace_id);
    if (traceId) summaryTraceIds.add(traceId);
  }

  return {
    realtimeCards,
    requestRows,
    visibleRealtimeTraceIds,
    hasPending:
      realtimeCards.some((card) => !card.trace.summary) ||
      requestRows.some((row) => isRequestLogActivityInProgress(row.activityState)),
    hasLiveRealtimeCards: realtimeCards.length > 0,
    summaryCount: summaryTraceIds.size,
  };
}
