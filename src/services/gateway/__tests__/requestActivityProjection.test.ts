import { describe, expect, expectTypeOf, it } from "vitest";
import type { RequestLogSummary } from "../requestLogs";
import type { GatewayAttemptEvent } from "../gatewayEvents";
import type { TraceSession, TraceSummary } from "../traceStore";
import {
  buildRequestActivityProjection,
  shouldTickRequestActivityClock,
  type ActiveRequestSnapshotItem,
  type ProjectedRealtimeCard,
} from "../requestActivityProjection";

function activeRequest(
  overrides: Partial<ActiveRequestSnapshotItem> = {}
): ActiveRequestSnapshotItem {
  return {
    trace_id: "trace-1",
    cli_key: "claude",
    session_id: null,
    method: "POST",
    path: "/v1/messages",
    query: null,
    requested_model: "claude-3-opus",
    created_at_ms: 1_700_000_000_000,
    last_activity_ms: 1_700_000_000_000,
    current_attempt: null,
    ...overrides,
  };
}

function attempt(traceId: string, attemptIndex: number, providerName: string): GatewayAttemptEvent {
  return {
    trace_id: traceId,
    cli_key: "claude",
    session_id: null,
    method: "POST",
    path: "/v1/messages",
    query: null,
    requested_model: "claude-3-opus",
    special_settings_json: null,
    attempt_index: attemptIndex,
    provider_id: attemptIndex,
    session_reuse: null,
    provider_name: providerName,
    base_url: "https://provider.example",
    outcome: "started",
    status: null,
    attempt_started_ms: attemptIndex * 100,
    attempt_duration_ms: 0,
    circuit_state_before: "CLOSED",
    circuit_state_after: null,
    circuit_failure_count: 0,
    circuit_failure_threshold: 3,
    claude_model_mapping: null,
  };
}

function log(overrides: Partial<RequestLogSummary> = {}): RequestLogSummary {
  return {
    id: 1,
    trace_id: "trace-1",
    cli_key: "claude",
    session_id: null,
    method: "POST",
    path: "/v1/messages",
    query: null,
    status: null,
    error_code: null,
    duration_ms: 0,
    ttfb_ms: null,
    attempts_json: "[]",
    input_tokens: null,
    effective_input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    cache_read_input_tokens: null,
    cache_creation_input_tokens: null,
    cache_creation_5m_input_tokens: null,
    cache_creation_1h_input_tokens: null,
    usage_json: null,
    requested_model: "claude-3-opus",
    cost_usd: null,
    cost_multiplier: 1,
    special_settings_json: null,
    provider_chain_json: null,
    error_details_json: null,
    final_provider_id: null,
    created_at_ms: 1_700_000_000_000,
    created_at: 1_700_000_000,
    ...overrides,
  } as RequestLogSummary;
}

// 事件类型来自生成 bindings（可空字段必填），工厂函数补默认值避免用例手写全量字段。
function summaryOf(traceId: string): TraceSummary {
  return {
    trace_id: traceId,
    cli_key: "claude",
    session_id: null,
    method: "POST",
    path: "/v1/messages",
    query: null,
    requested_model: null,
    special_settings_json: null,
    status: 200,
    error_category: null,
    error_code: null,
    duration_ms: 500,
    ttfb_ms: null,
    visible_ttfb_ms: null,
    attempts: [],
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    cache_read_input_tokens: null,
    cache_creation_input_tokens: null,
    cache_creation_5m_input_tokens: null,
    cache_creation_1h_input_tokens: null,
    effective_input_tokens: null,
    claude_model_mapping: null,
  };
}

function trace(overrides: Partial<TraceSession> = {}): TraceSession {
  return {
    trace_id: "trace-1",
    cli_key: "claude",
    session_id: null,
    method: "POST",
    path: "/v1/messages",
    query: null,
    requested_model: "claude-3-opus",
    claude_model_mapping: null,
    first_seen_ms: 1_700_000_000_000,
    last_seen_ms: 1_700_000_000_000,
    attempts: [],
    ...overrides,
  };
}

describe("services/gateway/requestActivityProjection", () => {
  const persistedMappingSettings = JSON.stringify([
    {
      type: "claude_model_mapping",
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      mappingKind: "sonnet",
      providerId: 2,
      providerName: "Provider B",
      applied: true,
    },
  ]);

  it("keeps old pending logs visible as fallback rows without live traces", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "old-pending",
          created_at_ms: 1_700_000_000_000 - 10 * 60 * 1000,
        }),
      ],
      activeRequests: [],
      traces: [],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual(["old-pending"]);
    expect(projection.requestRows[0]?.liveTrace).toBeNull();
    expect(projection.requestRows[0]?.activityState).toBe("interrupted");
  });

  it("projects active request logs as realtime cards from active registry activity", () => {
    const nowMs = 1_700_000_900_000;
    const active = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "active", last_activity_ms: nowMs - 11 * 60_000 } as any)],
      activeRequests: [activeRequest({ trace_id: "active", last_activity_ms: nowMs - 60_000 })],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });
    expect(active.requestRows).toHaveLength(0);
    expect(active.realtimeCards[0]?.trace.trace_id).toBe("active");
    expect(active.realtimeCards[0]?.trace.last_seen_ms).toBe(nowMs - 60_000);
    expect(active.realtimeCards[0]?.activeRequest?.last_activity_ms).toBe(nowMs - 60_000);
    expect(active.realtimeCards[0]?.kind).toBe("active");

    const idle = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "idle", last_activity_ms: nowMs - 60_000 } as any)],
      activeRequests: [activeRequest({ trace_id: "idle", last_activity_ms: nowMs - 11 * 60_000 })],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });
    expect(idle.requestRows).toHaveLength(0);
    expect(idle.realtimeCards[0]?.trace.trace_id).toBe("idle");
    expect(idle.realtimeCards[0]?.trace.last_seen_ms).toBe(nowMs - 11 * 60_000);
    expect(idle.realtimeCards[0]?.activeRequest?.last_activity_ms).toBe(nowMs - 11 * 60_000);
  });

  it("replays the current attempt for a registry-only active trace", () => {
    const nowMs = 1_700_000_900_000;
    const currentAttempt = attempt("active-background", 1, "78code");
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({
          trace_id: "active-background",
          created_at_ms: nowMs - 3_000,
          last_activity_ms: nowMs - 240,
          current_attempt: currentAttempt,
        }),
      ],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.kind).toBe("active");
    expect(projection.realtimeCards[0]?.trace.attempts).toEqual([currentAttempt]);
    expect(projection.realtimeCards[0]?.trace.last_seen_ms).toBe(nowMs - 240);
  });

  it("fills a missing newer attempt from the active snapshot without replaying older progress", () => {
    const nowMs = 1_700_000_900_000;
    const firstAttempt = attempt("active-gap", 1, "Provider A");
    const latestAttempt = attempt("active-gap", 3, "Provider C");
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({
          trace_id: "active-gap",
          last_activity_ms: nowMs - 100,
          current_attempt: latestAttempt,
        }),
      ],
      traces: [
        trace({
          trace_id: "active-gap",
          last_seen_ms: nowMs - 1_000,
          attempts: [firstAttempt],
        }),
      ],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.attempts).toEqual([firstAttempt, latestAttempt]);
    expect(projection.realtimeCards[0]?.trace.last_seen_ms).toBe(nowMs - 100);

    const staleSnapshot = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({
          trace_id: "active-gap",
          current_attempt: firstAttempt,
        }),
      ],
      traces: [trace({ trace_id: "active-gap", attempts: [latestAttempt] })],
      nowMs,
      realtimeCardLimit: 5,
    });
    expect(staleSnapshot.realtimeCards[0]?.trace.attempts).toEqual([latestAttempt]);
  });

  it("does not apply active attempt progress after terminal evidence", () => {
    const terminalAttempt = attempt("terminal-progress", 2, "Provider B");
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "terminal-progress", status: 200 })],
      activeRequests: [
        activeRequest({
          trace_id: "terminal-progress",
          current_attempt: terminalAttempt,
        }),
      ],
      traces: [trace({ trace_id: "terminal-progress", attempts: [] })],
      nowMs: 1_700_000_000_100,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.kind).toBe("settling");
    expect(projection.realtimeCards[0]?.trace.attempts).toEqual([]);
  });

  it("keeps terminal logs out of the in-progress projection despite a stale registry entry", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "finished", status: 200, duration_ms: 1_234 })],
      activeRequests: [activeRequest({ trace_id: "finished", last_activity_ms: nowMs - 1_000 })],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual(["finished"]);
    expect(projection.requestRows[0]?.activityState).toBe("completed");
  });

  it("renders a pending log with a visible trace as one realtime card and no duplicate row", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "live-pending" })],
      activeRequests: [activeRequest({ trace_id: "live-pending" })],
      traces: [trace({ trace_id: "live-pending" })],
      nowMs: 1_700_000_000_000 + 10 * 60 * 1000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual(["live-pending"]);
    expect(projection.requestRows).toHaveLength(0);
    expect(projection.realtimeCards[0]?.kind).toBe("active");
  });

  it("hides terminal rows only while their completed realtime card is in the exit window", () => {
    const completedTrace = trace({
      trace_id: "completed",
      last_seen_ms: 1_700_000_000_000,
      summary: summaryOf("completed"),
    });
    const completedLog = log({ trace_id: "completed", status: 200, duration_ms: 500 });

    const duringExit = buildRequestActivityProjection({
      requestLogs: [completedLog],
      activeRequests: [],
      traces: [completedTrace],
      nowMs: 1_700_000_000_500,
      realtimeCardLimit: 5,
    });
    expect(duringExit.realtimeCards.map((card) => card.trace.trace_id)).toEqual(["completed"]);
    expect(duringExit.requestRows).toHaveLength(0);

    const afterExit = buildRequestActivityProjection({
      requestLogs: [completedLog],
      activeRequests: [],
      traces: [completedTrace],
      nowMs: 1_700_000_002_000,
      realtimeCardLimit: 5,
    });
    expect(afterExit.realtimeCards).toHaveLength(0);
    expect(afterExit.requestRows.map((row) => row.log.trace_id)).toEqual(["completed"]);
  });

  it("merged summary falls back per field: summary over trace over request log", () => {
    // 覆盖 mergeTraceWithRequestLog 三条新增回退链的优先级：
    // session_id 取 summary；requested_model 在 summary 缺失时取 trace（而非 log）；
    // claude_model_mapping 在 summary 缺失时取 trace。
    const traceMapping = {
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-4.1",
      mappingKind: "sonnet",
      providerId: 1,
      providerName: "Provider A",
      applied: true,
    };
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "merged",
          status: 200,
          session_id: "sess-log",
          requested_model: "model-log",
          effective_input_tokens: 42,
        }),
      ],
      activeRequests: [],
      traces: [
        trace({
          trace_id: "merged",
          session_id: null,
          requested_model: "model-trace",
          claude_model_mapping: traceMapping,
          last_seen_ms: 1_700_000_000_000,
          summary: {
            ...summaryOf("merged"),
            session_id: "sess-summary",
            requested_model: null,
            claude_model_mapping: null,
          },
        }),
      ],
      nowMs: 1_700_000_000_500,
      realtimeCardLimit: 5,
    });

    const merged = projection.realtimeCards[0]?.trace.summary;
    expect(merged?.session_id).toBe("sess-summary");
    expect(merged?.requested_model).toBe("model-trace");
    expect(merged?.claude_model_mapping).toMatchObject({
      effectiveModel: "gpt-4.1",
      providerId: 1,
    });
    expect(merged?.effective_input_tokens).toBe(42);
  });

  it("does not promote persisted incomplete logs without active registry membership", () => {
    const nowMs = 1_700_000_600_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "stale-incomplete",
          status: null,
          error_code: null,
          created_at_ms: nowMs - 10 * 60_000,
          created_at: Math.floor((nowMs - 10 * 60_000) / 1000),
        }),
      ],
      activeRequests: [],
      traces: [
        trace({
          trace_id: "stale-incomplete",
          last_seen_ms: nowMs - 10 * 60_000,
        }),
      ],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual(["stale-incomplete"]);
    expect(projection.requestRows[0]?.activityState).toBe("interrupted");
    expect(projection.hasPending).toBe(false);
  });

  it("backfills realtime card model mapping from persisted request log settings", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "mapped-pending",
          requested_model: "claude-sonnet",
          special_settings_json: persistedMappingSettings,
          final_provider_id: 2,
        }),
      ],
      activeRequests: [activeRequest({ trace_id: "mapped-pending" })],
      traces: [
        trace({
          trace_id: "mapped-pending",
          requested_model: "claude-sonnet",
          claude_model_mapping: null,
        }),
      ],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.claude_model_mapping).toMatchObject({
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      providerId: 2,
    });
  });

  it("backfills realtime card special settings from persisted request log settings", () => {
    const specialSettingsJson = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "codex-pending",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: specialSettingsJson,
        }),
      ],
      activeRequests: [activeRequest({ trace_id: "codex-pending" })],
      traces: [
        trace({
          trace_id: "codex-pending",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: null,
        }),
      ],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.special_settings_json).toBe(specialSettingsJson);
  });

  it("prefers persisted model route mapping settings over stale trace start settings", () => {
    const startSettingsJson = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);
    const routeSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        mismatch: true,
      },
    ]);

    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "codex-route",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: routeSettingsJson,
          status: 200,
        }),
      ],
      traces: [
        trace({
          trace_id: "codex-route",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: startSettingsJson,
          summary: {
            trace_id: "codex-route",
            cli_key: "codex",
            method: "POST",
            path: "/v1/responses",
            query: null,
            status: 200,
            error_category: null,
            error_code: null,
            duration_ms: 500,
            ttfb_ms: null,
            attempts: [],
          } as unknown as TraceSession["summary"],
        }),
      ],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.special_settings_json).toBe(routeSettingsJson);
  });

  it("uses persisted model route mapping when trace settings are invalid", () => {
    const routeSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        mismatch: true,
      },
    ]);

    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "codex-truncated-route",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: routeSettingsJson,
          status: 200,
        }),
      ],
      traces: [
        trace({
          trace_id: "codex-truncated-route",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: '[{"type":"codex_reasoning_effort"',
        }),
      ],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.special_settings_json).toBe(routeSettingsJson);
  });

  it("uses terminal Codex settings instead of stale live settings", () => {
    const staleSettings = JSON.stringify([{ type: "legacy_observation" }]);
    const terminalEffortSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "codex-terminal-guard",
          cli_key: "codex",
          status: 200,
          requested_model: "gpt-5.5-pro",
          special_settings_json: terminalEffortSettings,
          duration_ms: 1_200,
        }),
      ],
      traces: [
        trace({
          trace_id: "codex-terminal-guard",
          cli_key: "codex",
          requested_model: "gpt-5.5",
          special_settings_json: staleSettings,
        }),
      ],
      nowMs: 1_700_000_000_500,
      realtimeCardLimit: 5,
    });

    const projectedTrace = projection.realtimeCards[0]?.trace;
    expect(projectedTrace?.requested_model).toBe("gpt-5.5-pro");
    expect(projectedTrace?.special_settings_json).toBe(terminalEffortSettings);
    expect(projectedTrace?.summary?.requested_model).toBe("gpt-5.5-pro");
    expect(projectedTrace?.summary?.special_settings_json).toBe(terminalEffortSettings);
  });

  it("keeps live trace model mapping ahead of persisted request log settings", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "mapped-live",
          special_settings_json: persistedMappingSettings,
          final_provider_id: 2,
        }),
      ],
      activeRequests: [activeRequest({ trace_id: "mapped-live" })],
      traces: [
        trace({
          trace_id: "mapped-live",
          claude_model_mapping: {
            requestedModel: "claude-sonnet",
            effectiveModel: "gpt-4.1",
            mappingKind: "sonnet",
            providerId: 1,
            providerName: "Provider A",
            applied: true,
          },
        }),
      ],
      nowMs: 1_700_000_000_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.claude_model_mapping).toMatchObject({
      effectiveModel: "gpt-4.1",
      providerId: 1,
    });
  });

  it("projects active requests even before their placeholder log is persisted", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({
          trace_id: "active-without-log",
          created_at_ms: 1_700_000_900_000,
          last_activity_ms: 1_700_000_899_500,
        }),
      ],
      traces: [],
      nowMs: 1_700_000_900_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual([
      "active-without-log",
    ]);
    expect(projection.realtimeCards[0]?.kind).toBe("active");
    expect(projection.requestRows).toHaveLength(0);
  });

  it("does not let a stale active snapshot override a persisted terminal request log", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          trace_id: "completed-with-stale-active",
          status: 200,
          error_code: null,
          created_at_ms: nowMs - 132 * 60_000,
          duration_ms: 12_345,
        }),
      ],
      activeRequests: [
        activeRequest({
          trace_id: "completed-with-stale-active",
          created_at_ms: nowMs - 132 * 60_000,
          last_activity_ms: nowMs - 131 * 60_000,
        }),
      ],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.hasPending).toBe(false);
    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual([
      "completed-with-stale-active",
    ]);
    expect(projection.requestRows[0]?.activityState).toBe("completed");
  });

  it("orders active rows first and interrupted audit rows after terminal history", () => {
    const nowMs = 1_700_001_000_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [
        log({
          id: 1,
          trace_id: "interrupted-newer",
          status: null,
          error_code: null,
          created_at_ms: nowMs,
          created_at: Math.floor(nowMs / 1000),
        }),
        log({
          id: 2,
          trace_id: "completed-older",
          status: 200,
          error_code: null,
          created_at_ms: nowMs - 60_000,
          created_at: Math.floor((nowMs - 60_000) / 1000),
        }),
      ],
      activeRequests: [
        activeRequest({
          trace_id: "active-without-log",
          created_at_ms: nowMs - 120_000,
          last_activity_ms: nowMs - 500,
        }),
      ],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual([
      "active-without-log",
    ]);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual([
      "completed-older",
      "interrupted-newer",
    ]);
    expect(projection.requestRows.map((row) => row.activityState)).toEqual([
      "completed",
      "interrupted",
    ]);
  });

  it("never evicts in-progress cards in favor of completed exiting cards at the card limit", () => {
    const nowMs = 1_700_000_500_000;
    const summary = summaryOf;
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({ trace_id: "live-old", created_at_ms: nowMs - 60_000 }),
        activeRequest({ trace_id: "live-older", created_at_ms: nowMs - 120_000 }),
      ],
      traces: [
        trace({
          trace_id: "done-new",
          first_seen_ms: nowMs - 100,
          last_seen_ms: nowMs,
          summary: summary("done-new"),
        }),
        trace({
          trace_id: "done-newer",
          first_seen_ms: nowMs - 50,
          last_seen_ms: nowMs,
          summary: summary("done-newer"),
        }),
      ],
      nowMs,
      realtimeCardLimit: 3,
    });

    const cardIds = projection.realtimeCards.map((card) => card.trace.trace_id);
    expect(cardIds).toContain("live-old");
    expect(cardIds).toContain("live-older");
    expect(cardIds).toHaveLength(3);
  });

  it("never evicts an older in-progress card at the realtime soft limit", () => {
    const nowMs = 1_700_000_500_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [activeRequest({ trace_id: "live-old", created_at_ms: nowMs - 120_000 })],
      traces: [
        trace({
          trace_id: "done-newest",
          first_seen_ms: nowMs - 10,
          last_seen_ms: nowMs,
          summary: summaryOf("done-newest"),
        }),
        trace({
          trace_id: "done-newer",
          first_seen_ms: nowMs - 20,
          last_seen_ms: nowMs,
          summary: summaryOf("done-newer"),
        }),
        trace({
          trace_id: "done-new",
          first_seen_ms: nowMs - 30,
          last_seen_ms: nowMs,
          summary: summaryOf("done-new"),
        }),
      ],
      nowMs,
      realtimeCardLimit: 2,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual([
      "done-newest",
      "live-old",
    ]);
    expect(projection.requestRows).toHaveLength(0);
  });

  it("ignores an inactive summaryless trace instead of keeping it in progress forever", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [],
      traces: [trace({ trace_id: "stale-orphan" })],
      nowMs: 1_700_000_900_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows).toHaveLength(0);
  });

  it("keeps active requests as realtime cards when newer stale traces exceed the candidate limit", () => {
    const nowMs = 1_700_000_900_000;
    const staleTraces = Array.from({ length: 25 }, (_, index) =>
      trace({
        trace_id: `stale-${index}`,
        first_seen_ms: nowMs - index,
        last_seen_ms: nowMs - index,
      })
    );
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: [
        activeRequest({
          trace_id: "real-active",
          created_at_ms: nowMs - 60_000,
          last_activity_ms: nowMs - 1_000,
        }),
      ],
      traces: staleTraces,
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toContain("real-active");
    expect(projection.requestRows.map((row) => row.log.trace_id)).not.toContain("real-active");
  });

  it("keeps every active card when active requests exceed the realtime soft limit", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [],
      activeRequests: Array.from({ length: 4 }, (_, index) =>
        activeRequest({
          trace_id: `active-${index}`,
          created_at_ms: nowMs - index * 1_000,
        })
      ),
      traces: [],
      nowMs,
      realtimeCardLimit: 2,
    });

    expect(projection.realtimeCards).toHaveLength(4);
    expect(projection.realtimeCards.every((card) => card.kind === "active")).toBe(true);
    expect(projection.requestRows).toHaveLength(0);
  });

  it("normalizes realtime trace ids when removing duplicate history rows", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "normalized-active" })],
      activeRequests: [
        activeRequest({
          trace_id: " normalized-active ",
          created_at_ms: nowMs - 1_000,
        }),
      ],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(1);
    expect(projection.requestRows).toHaveLength(0);
  });

  it("renders an interrupted row when an inactive placeholder has a stale trace", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "interrupted-stale" })],
      activeRequests: [],
      traces: [trace({ trace_id: "interrupted-stale" })],
      nowMs: 1_700_000_900_000,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows).toHaveLength(1);
    expect(projection.requestRows[0]?.activityState).toBe("interrupted");
  });

  it("lets a terminal log finish a summaryless trace despite a stale active snapshot", () => {
    const nowMs = 1_700_000_000_500;
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "terminal-wins", status: 200, duration_ms: 500 })],
      activeRequests: [activeRequest({ trace_id: "terminal-wins" })],
      traces: [
        trace({
          trace_id: "terminal-wins",
          first_seen_ms: 1_700_000_000_000,
          last_seen_ms: 1_700_000_000_000,
        }),
      ],
      nowMs,
      realtimeCardLimit: 5,
    });

    expect(projection.realtimeCards[0]?.trace.summary?.status).toBe(200);
    expect(projection.realtimeCards[0]?.kind).toBe("settling");
  });

  it("encodes active and settling summary invariants in the card type", () => {
    type ActiveCard = Extract<ProjectedRealtimeCard, { kind: "active" }>;
    type SettlingCard = Extract<ProjectedRealtimeCard, { kind: "settling" }>;

    expectTypeOf<ActiveCard["trace"]["summary"]>().toEqualTypeOf<undefined>();
    expectTypeOf<SettlingCard["trace"]["summary"]>().toEqualTypeOf<TraceSummary>();
  });

  it("ticks only for canonical active or settling cards", () => {
    const nowMs = 1_700_000_000_500;
    const staleActiveInput = {
      requestLogs: [log({ trace_id: "terminal-wins", status: 200 })],
      activeRequests: [activeRequest({ trace_id: "terminal-wins" })],
      traces: [trace({ trace_id: "terminal-wins", last_seen_ms: nowMs })],
      nowMs,
    };
    expect(shouldTickRequestActivityClock(staleActiveInput)).toBe(true);

    expect(
      shouldTickRequestActivityClock({
        ...staleActiveInput,
        traces: [],
      })
    ).toBe(false);

    expect(
      shouldTickRequestActivityClock({
        ...staleActiveInput,
        nowMs: nowMs + 2_000,
      })
    ).toBe(false);

    expect(
      shouldTickRequestActivityClock({
        requestLogs: [],
        activeRequests: [],
        traces: [trace({ trace_id: "stale-orphan" })],
        nowMs,
      })
    ).toBe(false);

    expect(
      shouldTickRequestActivityClock({
        requestLogs: [],
        activeRequests: [activeRequest({ trace_id: "active" })],
        traces: [],
        nowMs,
      })
    ).toBe(true);
  });
});
