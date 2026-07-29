import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createRequestLogSummary } from "../requestLogFixtures";
import { resolveCodexReasoningEffort } from "../requestLogSpecialSettings";
import type {
  GatewayAttemptEvent,
  GatewayRequestEvent,
  GatewayRequestStartEvent,
} from "../gatewayEvents";

async function importFreshTraceStore() {
  vi.resetModules();
  return await import("../traceStore");
}

// 事件类型来自生成 bindings（可空字段必填），工厂函数补默认值避免每个用例手写全量字段。
function makeStartEvent(
  overrides: Partial<GatewayRequestStartEvent> = {}
): GatewayRequestStartEvent {
  return {
    trace_id: "trace",
    cli_key: "claude",
    session_id: null,
    method: "GET",
    path: "/",
    query: null,
    requested_model: null,
    special_settings_json: null,
    ts: 0,
    ...overrides,
  };
}

function makeAttemptEvent(overrides: Partial<GatewayAttemptEvent> = {}): GatewayAttemptEvent {
  return {
    trace_id: "trace",
    cli_key: "claude",
    session_id: null,
    method: "GET",
    path: "/",
    query: null,
    requested_model: null,
    special_settings_json: null,
    attempt_index: 1,
    provider_id: 1,
    session_reuse: null,
    provider_name: "P",
    base_url: "https://p",
    outcome: "started",
    status: null,
    attempt_started_ms: 0,
    attempt_duration_ms: 0,
    circuit_state_before: null,
    circuit_state_after: null,
    circuit_failure_count: null,
    circuit_failure_threshold: null,
    claude_model_mapping: null,
    ...overrides,
    requested_upstream_model: overrides.requested_upstream_model ?? null,
  };
}

function makeRequestEvent(overrides: Partial<GatewayRequestEvent> = {}): GatewayRequestEvent {
  return {
    trace_id: "trace",
    cli_key: "claude",
    session_id: null,
    method: "GET",
    path: "/",
    query: null,
    requested_model: null,
    special_settings_json: null,
    status: 200,
    error_category: null,
    error_code: null,
    duration_ms: 0,
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
    ...overrides,
  };
}

describe("services/gateway/traceStore", () => {
  it("ingestTraceStart creates traces and resets completed traces", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();

    const { result } = renderHook(() => useTraceStore());
    expect(result.current.traces).toEqual([]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "t1",
          cli_key: "claude",
          method: "GET",
          path: "/v1/test",
          query: null,
          requested_model: "claude-3",
          ts: 0,
        })
      );
    });
    expect(result.current.traces[0]?.trace_id).toBe("t1");
    expect(result.current.traces[0]?.summary).toBeUndefined();

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t1",
          cli_key: "claude",
          method: "GET",
          path: "/v1/test",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 12,
          attempts: [],
        })
      );
    });
    expect(result.current.traces[0]?.summary?.status).toBe(200);

    vi.setSystemTime(1000);
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "t1",
          cli_key: "claude",
          method: "POST",
          path: "/v1/again",
          query: "x=1",
          requested_model: "claude-3-opus",
          ts: 1,
        })
      );
    });
    expect(result.current.traces[0]?.method).toBe("POST");
    expect(result.current.traces[0]?.path).toBe("/v1/again");
    expect(result.current.traces[0]?.summary).toBeUndefined();

    vi.useRealTimers();
  });

  it("keeps store subscribers isolated when one listener throws", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, subscribeTraceStore } = await importFreshTraceStore();
    const failingListener = vi.fn(() => {
      throw new Error("listener boom");
    });
    const healthyListener = vi.fn();

    const unsubscribeFailing = subscribeTraceStore(failingListener);
    const unsubscribeHealthy = subscribeTraceStore(healthyListener);

    expect(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "subscriber-isolation",
          cli_key: "claude",
          method: "GET",
          path: "/v1/test",
          query: null,
          requested_model: "claude-3",
          ts: 0,
        })
      );
    }).not.toThrow();

    expect(failingListener).toHaveBeenCalledTimes(1);
    expect(healthyListener).toHaveBeenCalledTimes(1);

    unsubscribeFailing();
    unsubscribeHealthy();
    vi.useRealTimers();
  });

  it("ingestTraceAttempt upserts attempts and moves trace to front", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceAttempt, useTraceStore } = await importFreshTraceStore();

    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "tA",
          cli_key: "codex",
          method: "GET",
          path: "/x",
          query: null,
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
    });
    expect(result.current.traces[0]?.trace_id).toBe("tA");
    expect(result.current.traces[0]?.attempts).toHaveLength(1);

    // Upsert same index replaces.
    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "tA",
          cli_key: "codex",
          method: "GET",
          path: "/x",
          query: null,
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "failed",
          status: 500,
          attempt_started_ms: 0,
          attempt_duration_ms: 12,
        })
      );
    });
    expect(result.current.traces[0]?.attempts).toHaveLength(1);
    expect(result.current.traces[0]?.attempts[0]?.status).toBe(500);

    // New trace moves to front.
    vi.setSystemTime(1000);
    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "tB",
          cli_key: "claude",
          method: "POST",
          path: "/y",
          query: null,
          attempt_index: 1,
          provider_id: 2,
          provider_name: "P2",
          base_url: "https://p2",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
    });
    expect(result.current.traces[0]?.trace_id).toBe("tB");
    expect(result.current.traces[1]?.trace_id).toBe("tA");

    vi.useRealTimers();
  });

  it("ingestTraceAttempt backfills requested_model when request_start is missing", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceAttempt, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-model-from-attempt",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-opus-4-6",
          attempt_index: 1,
          provider_id: 2,
          provider_name: "SSAiCode",
          base_url: "https://provider.example",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
    });

    expect(result.current.traces[0]?.requested_model).toBe("claude-opus-4-6");

    vi.useRealTimers();
  });

  it("keeps request special settings from start events across attempt and completion updates", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceAttempt, ingestTraceRequest, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const startSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "t-effort",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: startSettings,
          ts: 0,
        })
      );
    });
    expect(result.current.traces[0]?.special_settings_json).toBe(startSettings);

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-effort",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
    });
    expect(result.current.traces[0]?.special_settings_json).toBe(startSettings);

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-effort",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
        })
      );
    });
    expect(result.current.traces[0]?.special_settings_json).toBe(startSettings);

    vi.useRealTimers();
  });

  it("lets terminal model route mapping special settings replace stale start settings", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const startSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);
    const routeSettings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        mismatch: true,
      },
    ]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "t-route-settings",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: startSettings,
          ts: 0,
        })
      );
    });

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-route-settings",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: routeSettings,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
        })
      );
    });

    expect(result.current.traces[0]?.special_settings_json).toBe(routeSettings);

    vi.useRealTimers();
  });

  it("keeps terminal mismatch when a managed-only attempt arrives out of order", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceAttempt, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const mismatchSettings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "grok-4.5",
        actualModel: "grok-4.5-preview",
        mismatch: true,
      },
    ]);
    const managedOnlySettings = JSON.stringify([
      {
        type: "aio_managed_model_route",
        canonicalModel: "aio/model-a",
        providerId: 11,
        providerUuid: "22222222-2222-4222-8222-222222222222",
        remoteModelId: "grok-4.5",
        requestedUpstreamModel: "grok-4.5",
        applied: true,
      },
    ]);

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-managed-route-order",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          requested_model: "aio/model-a",
          special_settings_json: mismatchSettings,
          status: 200,
        })
      );
    });

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-managed-route-order",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          requested_model: "aio/model-a",
          special_settings_json: managedOnlySettings,
          attempt_index: 1,
          provider_id: 11,
        })
      );
    });

    expect(result.current.traces[0]?.special_settings_json).toBe(mismatchSettings);
    expect(result.current.traces[0]?.summary?.special_settings_json).toBe(mismatchSettings);

    vi.useRealTimers();
  });

  it("stores Claude model mapping from attempts and lets completion override it", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceAttempt, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-mapping",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-sonnet",
          attempt_index: 1,
          provider_id: 1,
          provider_name: "Provider A",
          base_url: "https://provider-a.example",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
          claude_model_mapping: {
            requestedModel: " claude-sonnet ",
            effectiveModel: " gpt-4.1 ",
            mappingKind: " sonnet ",
            providerId: 1,
            providerName: " Provider A ",
            applied: true,
          },
        })
      );
    });

    expect(result.current.traces[0]?.claude_model_mapping?.effectiveModel).toBe("gpt-4.1");

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-mapping",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-sonnet",
          attempt_index: 1,
          provider_id: 1,
          provider_name: "Provider A",
          base_url: "https://provider-a.example",
          outcome: "success",
          status: 200,
          attempt_started_ms: 0,
          attempt_duration_ms: 42,
        })
      );
    });

    expect(result.current.traces[0]?.claude_model_mapping?.effectiveModel).toBe("gpt-4.1");
    expect(result.current.traces[0]?.attempts[0]?.claude_model_mapping?.effectiveModel).toBe(
      " gpt-4.1 "
    );

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-mapping",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-sonnet",
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
          claude_model_mapping: {
            requestedModel: "claude-sonnet",
            effectiveModel: "gpt-5.4",
            mappingKind: "sonnet",
            providerId: 2,
            providerName: "Provider B",
            applied: true,
          },
        })
      );
    });

    expect(result.current.traces[0]?.claude_model_mapping?.providerId).toBe(2);
    expect(result.current.traces[0]?.claude_model_mapping?.effectiveModel).toBe("gpt-5.4");

    vi.useRealTimers();
  });

  it("clears Claude model mapping when completion explicitly has no valid mapping", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceAttempt, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-mapping-clear",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-sonnet",
          attempt_index: 1,
          provider_id: 1,
          provider_name: "Provider A",
          base_url: "https://provider-a.example",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
          claude_model_mapping: {
            requestedModel: "claude-sonnet",
            effectiveModel: "gpt-4.1",
            mappingKind: "sonnet",
            providerId: 1,
            providerName: "Provider A",
            applied: true,
          },
        })
      );
    });

    expect(result.current.traces[0]?.claude_model_mapping?.effectiveModel).toBe("gpt-4.1");

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-mapping-clear",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-sonnet",
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
          claude_model_mapping: null,
        })
      );
    });

    expect(result.current.traces[0]?.claude_model_mapping).toBeNull();

    vi.useRealTimers();
  });

  it("ingestTraceRequest creates new trace when trace_id not found", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5000);

    const { ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    expect(result.current.traces).toEqual([]);

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "new-req",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
        })
      );
    });

    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("new-req");
    expect(result.current.traces[0]?.summary).toBeDefined();
    expect(result.current.traces[0]?.summary?.status).toBe(200);
    expect(result.current.traces[0]?.summary?.duration_ms).toBe(50);
    expect(result.current.traces[0]?.attempts).toEqual([]);

    vi.useRealTimers();
  });

  it("bounds completion summary attempts retained in the trace store", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5000);

    const { MAX_ATTEMPTS_PER_TRACE } = await import("../traceLimits");
    const { ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const attempts = Array.from({ length: MAX_ATTEMPTS_PER_TRACE + 50 }, (_, index) => ({
      provider_id: index,
      provider_name: `P${index}`,
      base_url: `https://p${index}.example`,
      outcome: "failed",
      status: 500,
      requested_upstream_model: null,
    }));

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "large-summary",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          status: 500,
          error_category: "upstream",
          error_code: "GW_UPSTREAM_ERROR",
          duration_ms: 50,
          attempts,
        })
      );
    });

    const retainedAttempts = result.current.traces[0]?.summary?.attempts ?? [];
    expect(retainedAttempts).toHaveLength(MAX_ATTEMPTS_PER_TRACE);
    expect(retainedAttempts[0]?.provider_id).toBe(50);
    expect(retainedAttempts[retainedAttempts.length - 1]?.provider_id).toBe(
      MAX_ATTEMPTS_PER_TRACE + 49
    );
    expect(attempts).toHaveLength(MAX_ATTEMPTS_PER_TRACE + 50);

    vi.useRealTimers();
  });

  it("ingestTraceRequest updates existing trace with summary", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "existing-req",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-3-opus",
          ts: 0,
        })
      );
    });

    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.summary).toBeUndefined();

    vi.setSystemTime(100);
    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "existing-req",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 100,
          attempts: [],
        })
      );
    });

    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("existing-req");
    expect(result.current.traces[0]?.summary).toBeDefined();
    expect(result.current.traces[0]?.summary?.status).toBe(200);

    vi.useRealTimers();
  });

  it("ingestTraceRequest backfills requested_model when summary arrives first", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "summary-first",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-opus-4-6",
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
        })
      );
    });

    expect(result.current.traces[0]?.requested_model).toBe("claude-opus-4-6");

    vi.useRealTimers();
  });

  it("preserves and backfills session_id across realtime event updates", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceAttempt, ingestTraceRequest, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "t-session",
          cli_key: "codex",
          session_id: "session-from-start",
          method: "POST",
          path: "/v1/responses",
          query: null,
          ts: 0,
        })
      );
    });
    expect(result.current.traces[0]?.session_id).toBe("session-from-start");

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "t-session",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
    });
    expect(result.current.traces[0]?.session_id).toBe("session-from-start");

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "t-session-2",
          cli_key: "claude",
          session_id: "session-from-summary",
          method: "POST",
          path: "/v1/messages",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 50,
          attempts: [],
        })
      );
    });
    expect(
      result.current.traces.find((trace) => trace.trace_id === "t-session-2")?.session_id
    ).toBe("session-from-summary");

    vi.useRealTimers();
  });

  it("ignores payloads with missing trace_id", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceAttempt, ingestTraceRequest, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    expect(result.current.traces).toEqual([]);

    // null/undefined payloads
    act(() => {
      ingestTraceStart(null as never);
      ingestTraceAttempt(undefined as never);
      ingestTraceRequest(null as never);
    });
    expect(result.current.traces).toEqual([]);

    // payloads with empty trace_id
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "",
          cli_key: "claude",
          method: "GET",
          path: "/",
          query: null,
          ts: 0,
        })
      );
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "",
          cli_key: "claude",
          method: "GET",
          path: "/",
          query: null,
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P",
          base_url: "https://p",
          outcome: "started",
          status: null,
          attempt_started_ms: 0,
          attempt_duration_ms: 0,
        })
      );
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "",
          cli_key: "claude",
          method: "GET",
          path: "/",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 0,
          attempts: [],
        })
      );
    });
    expect(result.current.traces).toEqual([]);

    vi.useRealTimers();
  });

  it("keeps long-running traces without summary after newer traces arrive", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    // Create a trace at time 0 (no summary = "in progress")
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "stale-trace",
          cli_key: "claude",
          method: "GET",
          path: "/v1/old",
          query: null,
          requested_model: "claude-3",
          ts: 0,
        })
      );
    });
    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("stale-trace");

    // Advance time past the previous stale threshold (5 minutes = 300000ms).
    vi.setSystemTime(300_001);

    // Ingest another trace; the older in-progress trace must remain visible until completion.
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "fresh-trace",
          cli_key: "claude",
          method: "POST",
          path: "/v1/new",
          query: null,
          requested_model: "claude-3",
          ts: 300_001,
        })
      );
    });

    expect(result.current.traces).toHaveLength(2);
    expect(result.current.traces[0]?.trace_id).toBe("fresh-trace");
    expect(result.current.traces[1]?.trace_id).toBe("stale-trace");
    expect(result.current.traces[1]?.summary).toBeUndefined();
  });

  it("ingestTraceRequest updates a long-running trace when summary arrives later", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceRequest, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    // Create a trace at time 0 (no summary)
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "long-running-trace",
          cli_key: "claude",
          method: "GET",
          path: "/v1/stale",
          query: null,
          requested_model: "claude-3",
          ts: 0,
        })
      );
    });
    expect(result.current.traces).toHaveLength(1);

    // Advance past the previous stale threshold.
    vi.setSystemTime(300_001);

    // ingestTraceRequest for the same trace_id should update the existing trace.
    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "long-running-trace",
          cli_key: "claude",
          method: "GET",
          path: "/v1/stale",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 300_001,
          attempts: [],
        })
      );
    });

    // The trace should exist with summary.
    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("long-running-trace");
    expect(result.current.traces[0]?.summary).toBeDefined();
    expect(result.current.traces[0]?.summary?.status).toBe(200);

    vi.useRealTimers();
  });

  it("reconciles a long-running trace from a terminal persisted request log", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "reconciled-trace",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          ts: 0,
        })
      );
    });

    vi.setSystemTime(83 * 60 * 1000);

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "reconciled-trace",
          cli_key: "codex",
          status: 200,
          error_code: null,
          duration_ms: 83 * 60 * 1000,
          created_at_ms: 83 * 60 * 1000,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.summary?.status).toBe(200);
    expect(result.current.traces[0]?.summary?.duration_ms).toBe(83 * 60 * 1000);

    vi.useRealTimers();
  });

  it("lets a terminal persisted request log replace stale live Codex settings", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const staleSettings = JSON.stringify([{ type: "legacy_observation" }]);
    const terminalEffortSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "codex-effort-terminal",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: staleSettings,
          ts: 0,
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "codex-effort-terminal",
          cli_key: "codex",
          status: 200,
          requested_model: "gpt-5.5",
          special_settings_json: terminalEffortSettings,
          created_at_ms: 1_000,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces[0]?.special_settings_json).toBe(terminalEffortSettings);
    expect(result.current.traces[0]?.summary?.special_settings_json).toBe(terminalEffortSettings);
    expect(
      resolveCodexReasoningEffort(
        result.current.traces[0]?.requested_model,
        result.current.traces[0]?.special_settings_json
      )
    ).toEqual({ effort: "high", source: "request" });

    vi.useRealTimers();
  });

  it("lets a terminal persisted request log replace stale live Codex requested_model", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "codex-model-terminal",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          ts: 0,
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "codex-model-terminal",
          cli_key: "codex",
          status: 200,
          requested_model: "gpt-5.5-pro",
          special_settings_json: null,
          created_at_ms: 1_000,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces[0]?.requested_model).toBe("gpt-5.5-pro");
    expect(result.current.traces[0]?.summary?.requested_model).toBe("gpt-5.5-pro");
    expect(
      resolveCodexReasoningEffort(
        result.current.traces[0]?.requested_model,
        result.current.traces[0]?.special_settings_json
      )
    ).toEqual({ effort: "high", source: "default" });

    vi.useRealTimers();
  });

  it("lets a live Codex fallback attempt replace requested_model", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceAttempt, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const requestEffortSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "medium" },
    ]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "codex-live-model-fallback",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: requestEffortSettings,
          ts: 0,
        })
      );
    });

    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "codex-live-model-fallback",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.4",
          attempt_index: 2,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "started",
          status: null,
          attempt_started_ms: 1_000,
          attempt_duration_ms: 0,
        })
      );
    });

    expect(result.current.traces[0]?.requested_model).toBe("gpt-5.4");
    expect(result.current.traces[0]?.special_settings_json).toBe(requestEffortSettings);
    expect(
      resolveCodexReasoningEffort(
        result.current.traces[0]?.requested_model,
        result.current.traces[0]?.special_settings_json
      )
    ).toEqual({ effort: "medium", source: "request" });

    vi.useRealTimers();
  });

  it("keeps explicit live Codex effort when terminal request log settings are missing", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const liveEffortSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "xhigh" },
    ]);

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "codex-live-effort-terminal-missing",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: liveEffortSettings,
          ts: 0,
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "codex-live-effort-terminal-missing",
          cli_key: "codex",
          status: 200,
          requested_model: "gpt-5.5-pro",
          special_settings_json: null,
          created_at_ms: 1_000,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces[0]?.requested_model).toBe("gpt-5.5-pro");
    expect(result.current.traces[0]?.special_settings_json).toBe(liveEffortSettings);
    expect(result.current.traces[0]?.summary?.special_settings_json).toBe(liveEffortSettings);
    expect(
      resolveCodexReasoningEffort(
        result.current.traces[0]?.requested_model,
        result.current.traces[0]?.special_settings_json
      )
    ).toEqual({ effort: "xhigh", source: "request" });

    vi.useRealTimers();
  });

  it("does not reconcile a trace from a pending persisted request log", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "still-running",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-live",
          special_settings_json: "live-settings",
          ts: 0,
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "still-running",
          status: null,
          error_code: null,
          requested_model: "claude-persisted",
          special_settings_json: "persisted-settings",
        })
      );
      expect(reconciled).toBe(false);
    });

    expect(result.current.traces[0]?.summary).toBeUndefined();
    expect(result.current.traces[0]?.requested_model).toBe("claude-live");
    expect(result.current.traces[0]?.special_settings_json).toBe("live-settings");

    vi.useRealTimers();
  });

  it("lets persisted terminal request logs correct an existing realtime summary", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceRequest, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "corrected-summary",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 10,
          ttfb_ms: 5,
          attempts: [],
          input_tokens: 1,
          output_tokens: 1,
          total_tokens: 2,
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "corrected-summary",
          cli_key: "codex",
          status: 502,
          error_code: "GW_UPSTREAM_ERROR",
          duration_ms: 2_000,
          ttfb_ms: 100,
          input_tokens: 10,
          output_tokens: 20,
          total_tokens: 30,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces[0]?.summary).toMatchObject({
      status: 502,
      error_code: "GW_UPSTREAM_ERROR",
      duration_ms: 2_000,
      ttfb_ms: 100,
      input_tokens: 10,
      output_tokens: 20,
      total_tokens: 30,
    });

    vi.useRealTimers();
  });

  it("lets terminal request logs correct stale Codex model settings on an existing realtime summary", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceRequest, reconcileTraceFromRequestLog, useTraceStore } =
      await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());
    const staleSettings = JSON.stringify([{ type: "legacy_observation" }]);
    const terminalEffortSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    act(() => {
      ingestTraceRequest(
        makeRequestEvent({
          trace_id: "corrected-codex-summary",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          query: null,
          requested_model: "gpt-5.5",
          special_settings_json: staleSettings,
          status: 200,
          error_category: null,
          error_code: null,
          duration_ms: 10,
          attempts: [],
        })
      );
    });

    act(() => {
      const reconciled = reconcileTraceFromRequestLog(
        createRequestLogSummary({
          trace_id: "corrected-codex-summary",
          cli_key: "codex",
          status: 200,
          requested_model: "gpt-5.5-pro",
          special_settings_json: terminalEffortSettings,
          duration_ms: 2_000,
        })
      );
      expect(reconciled).toBe(true);
    });

    expect(result.current.traces[0]?.requested_model).toBe("gpt-5.5-pro");
    expect(result.current.traces[0]?.special_settings_json).toBe(terminalEffortSettings);
    expect(result.current.traces[0]?.summary?.requested_model).toBe("gpt-5.5-pro");
    expect(result.current.traces[0]?.summary?.special_settings_json).toBe(terminalEffortSettings);

    vi.useRealTimers();
  });

  it("moveTraceToFront returns early when trace is already at front or not found", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);

    const { ingestTraceStart, ingestTraceAttempt, useTraceStore } = await importFreshTraceStore();
    const { result } = renderHook(() => useTraceStore());

    // Create a single trace
    act(() => {
      ingestTraceStart(
        makeStartEvent({
          trace_id: "only-trace",
          cli_key: "claude",
          method: "GET",
          path: "/v1/single",
          query: null,
          requested_model: "claude-3",
          ts: 0,
        })
      );
    });
    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("only-trace");

    // Update the same trace (already at front, moveTraceToFront index === 0 => returns early)
    vi.setSystemTime(100);
    act(() => {
      ingestTraceAttempt(
        makeAttemptEvent({
          trace_id: "only-trace",
          cli_key: "claude",
          method: "GET",
          path: "/v1/single",
          query: null,
          attempt_index: 1,
          provider_id: 1,
          provider_name: "P1",
          base_url: "https://p1",
          outcome: "started",
          status: null,
          attempt_started_ms: 100,
          attempt_duration_ms: 0,
        })
      );
    });

    // Trace is still at front, only one trace
    expect(result.current.traces).toHaveLength(1);
    expect(result.current.traces[0]?.trace_id).toBe("only-trace");
    expect(result.current.traces[0]?.attempts).toHaveLength(1);

    vi.useRealTimers();
  });
});
