import { describe, expect, it } from "vitest";
import type { RequestLogSummary } from "../requestLogs";
import type { TraceSession, TraceSummary } from "../traceStore";
import { buildRequestActivityProjection } from "../requestActivityProjection";

function activeRequest(overrides: Record<string, unknown> = {}) {
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
    ...overrides,
  };
}

function log(overrides: Partial<RequestLogSummary> = {}): RequestLogSummary {
  // Mirror the backend derivation for realistic rows.
  const status = "status" in overrides ? (overrides.status ?? null) : null;
  const isInterrupted =
    overrides.is_interrupted ?? (status == null && (overrides.error_code ?? null) == null);
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
    effective_input_tokens: null,
    ...overrides,
    is_interrupted: isInterrupted,
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
    status: 200,
    error_category: null,
    error_code: null,
    duration_ms: 500,
    ttfb_ms: null,
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
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual(["old-pending"]);
    expect(projection.requestRows[0]?.liveTrace).toBeNull();
    expect(projection.requestRows[0]?.activityState).toBe("interrupted");
    expect(projection.hasPending).toBe(false);
  });

  it("renders registry-active logs without live traces as synthesized realtime cards", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "active" })],
      activeRequests: [
        activeRequest({
          trace_id: "active",
          created_at_ms: nowMs - 120_000,
          last_activity_ms: nowMs - 60_000,
        }),
      ],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual(["active"]);
    expect(projection.realtimeCards[0]?.trace.summary).toBeUndefined();
    expect(projection.realtimeCards[0]?.trace.first_seen_ms).toBe(nowMs - 120_000);
    expect(projection.requestRows).toHaveLength(0);
    expect(projection.hasPending).toBe(true);
  });

  it("keeps terminal logs out of the in-progress projection despite a stale registry entry", () => {
    const nowMs = 1_700_000_900_000;
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "finished", status: 200, duration_ms: 1_234 })],
      activeRequests: [activeRequest({ trace_id: "finished", last_activity_ms: nowMs - 1_000 })],
      traces: [],
      nowMs,
      realtimeCardLimit: 5,
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards).toHaveLength(0);
    expect(projection.requestRows.map((row) => row.log.trace_id)).toEqual(["finished"]);
    expect(projection.requestRows[0]?.activityState).toBe("completed");
    expect(projection.hasPending).toBe(false);
  });

  it("renders a pending log with a visible trace as one realtime card and no duplicate row", () => {
    const projection = buildRequestActivityProjection({
      requestLogs: [log({ trace_id: "live-pending" })],
      activeRequests: [activeRequest({ trace_id: "live-pending" })],
      traces: [trace({ trace_id: "live-pending" })],
      nowMs: 1_700_000_000_000 + 10 * 60 * 1000,
      realtimeCardLimit: 5,
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual(["live-pending"]);
    expect(projection.requestRows).toHaveLength(0);
    expect(projection.visibleRealtimeTraceIds.has("live-pending")).toBe(true);
    expect(projection.hasLiveRealtimeCards).toBe(true);
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
      realtimeCandidateLimit: 20,
    });
    expect(duringExit.realtimeCards.map((card) => card.trace.trace_id)).toEqual(["completed"]);
    expect(duringExit.requestRows).toHaveLength(0);

    const afterExit = buildRequestActivityProjection({
      requestLogs: [completedLog],
      activeRequests: [],
      traces: [completedTrace],
      nowMs: 1_700_000_002_000,
      realtimeCardLimit: 5,
      realtimeCandidateLimit: 20,
    });
    expect(afterExit.realtimeCards).toHaveLength(0);
    expect(afterExit.requestRows.map((row) => row.log.trace_id)).toEqual(["completed"]);
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
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards[0]?.trace.claude_model_mapping).toMatchObject({
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      providerId: 2,
    });
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
      realtimeCandidateLimit: 20,
    });

    expect(projection.realtimeCards[0]?.trace.claude_model_mapping).toMatchObject({
      effectiveModel: "gpt-4.1",
      providerId: 1,
    });
  });

  it("projects active requests as realtime cards even before their placeholder log is persisted", () => {
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
      realtimeCandidateLimit: 20,
    });

    expect(projection.hasPending).toBe(true);
    expect(projection.realtimeCards.map((card) => card.trace.trace_id)).toEqual([
      "active-without-log",
    ]);
    expect(projection.requestRows).toHaveLength(0);
  });

  it("orders interrupted audit rows after terminal history while active requests become cards", () => {
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
      realtimeCandidateLimit: 20,
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
      realtimeCandidateLimit: 20,
    });

    const cardIds = projection.realtimeCards.map((card) => card.trace.trace_id);
    expect(cardIds).toContain("live-old");
    expect(cardIds).toContain("live-older");
    expect(cardIds).toHaveLength(3);
  });
});
