import { describe, expect, it } from "vitest";
import { GatewayErrorCodes } from "../../../constants/gatewayErrorCodes";
import { createRequestLogRouteHop } from "../../../services/gateway/requestLogFixtures";
import type { TraceSession } from "../../../services/gateway/traceStore";
import {
  buildRequestLogAuditMeta,
  buildRequestRouteMeta,
  computeStatusBadge,
  formatRequestLogModelText,
  hasClaudeModelMappingSpecialSetting,
  resolveRequestLogModelDisplayMeta,
  resolveCacheCreationDisplay,
  resolveLiveTraceDurationMs,
  resolveLiveTraceProvider,
  resolveRequestLogUsageReasoningTokens,
} from "../requestLogPresentation";
import {
  formatClaudeModelMappingText,
  hasPriorityServiceTierSpecialSetting,
  resolveClaudeModelMappingFromSpecialSettings,
} from "../requestLogSpecialSettings";

function createTrace(overrides: Partial<TraceSession> = {}): TraceSession {
  return {
    trace_id: "trace-1",
    cli_key: "claude",
    session_id: "session-1",
    method: "POST",
    path: "/v1/messages",
    query: null,
    requested_model: null,
    first_seen_ms: 1_000,
    last_seen_ms: 1_500,
    attempts: [],
    ...overrides,
  };
}

describe("components/home/requestLogPresentation", () => {
  it("resolves Claude model mapping special settings with final provider preference", () => {
    const settings = JSON.stringify([
      { type: "noop" },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-5.4 ",
        mappingKind: " sonnet ",
        providerId: 1,
        providerName: " Provider A ",
        applied: true,
      },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-opus ",
        effectiveModel: " gpt-5.5 ",
        mappingKind: " opus ",
        providerId: 2,
        providerName: " Provider B ",
        applied: true,
      },
    ]);

    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 1)).toEqual({
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      mappingKind: "sonnet",
      providerId: 1,
      providerName: "Provider A",
      applied: true,
    });
    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 99)?.providerId).toBe(2);
    expect(resolveClaudeModelMappingFromSpecialSettings("not-json")).toBeNull();
    expect(
      resolveClaudeModelMappingFromSpecialSettings(JSON.stringify({ type: "noop" }))
    ).toBeNull();
    expect(
      resolveClaudeModelMappingFromSpecialSettings(
        JSON.stringify([
          {
            type: "claude_model_mapping",
            requestedModel: "same",
            effectiveModel: "same",
            mappingKind: "sonnet",
            providerId: 1,
            providerName: "Provider A",
            applied: true,
          },
        ])
      )
    ).toBeNull();

    expect(hasClaudeModelMappingSpecialSetting(settings)).toBe(true);
    expect(hasClaudeModelMappingSpecialSetting(JSON.stringify([{ type: "noop" }]))).toBe(false);
    expect(hasClaudeModelMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("formats model mapping text and priority service tier settings", () => {
    expect(
      formatClaudeModelMappingText(" fallback-model ", {
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-5.4 ",
        mappingKind: "sonnet",
        providerId: 1,
        providerName: "Provider A",
        applied: true,
      })
    ).toBe("claude-sonnet → gpt-5.4");
    expect(formatClaudeModelMappingText(" fallback-model ", null)).toBe("fallback-model");
    expect(formatClaudeModelMappingText("   ", null)).toBe("未知");
    expect(formatRequestLogModelText("codex", "gpt-5.5", null)).toBe("gpt-5.5-medium");
    expect(
      formatRequestLogModelText(
        "codex",
        "gpt-5.5",
        JSON.stringify([{ type: "codex_reasoning_effort", effort: "high" }])
      )
    ).toBe("gpt-5.5-high");
    expect(
      formatRequestLogModelText(
        "codex",
        "gpt-5.5",
        JSON.stringify([{ type: "codex_reasoning_effort", rawEffort: "turbo" }])
      )
    ).toBe("gpt-5.5-unknown");
    expect(formatRequestLogModelText("codex", "gpt-future", null)).toBe("gpt-future-unknown");
    expect(formatRequestLogModelText("claude", "claude-sonnet", null)).toBe("claude-sonnet");
    expect(formatRequestLogModelText("codex", "gpt-5.4-mini", null)).toBe("gpt-5.4-mini-none");

    expect(hasPriorityServiceTierSpecialSetting(null)).toBe(false);
    expect(hasPriorityServiceTierSpecialSetting("bad-json")).toBe(false);
    expect(
      hasPriorityServiceTierSpecialSetting(JSON.stringify({ type: "codex_service_tier_result" }))
    ).toBe(false);
    expect(hasPriorityServiceTierSpecialSetting(JSON.stringify([{ type: "noop" }]))).toBe(false);
    expect(
      hasPriorityServiceTierSpecialSetting(
        JSON.stringify([{ type: "codex_service_tier_result", actualServiceTier: "priority" }])
      )
    ).toBe(true);
    expect(
      hasPriorityServiceTierSpecialSetting(
        JSON.stringify([
          {
            type: "codex_service_tier_result",
            billingSourcePreference: "auto",
            effectivePriority: true,
          },
        ])
      )
    ).toBe(true);
    expect(
      hasPriorityServiceTierSpecialSetting(
        JSON.stringify([{ type: "codex_service_tier_result", effectivePriority: false }])
      )
    ).toBe(false);
  });

  it("formats Codex model route mismatch display meta and audit tag", () => {
    const specialSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        actualReasoningEffortSource: "response",
        modelMismatch: true,
        effortMismatch: true,
        mismatch: true,
        providerId: 2,
        providerName: "Provider B",
      },
    ]);

    const display = resolveRequestLogModelDisplayMeta(
      "codex",
      "gpt-5.5",
      specialSettingsJson,
      null,
      2
    );
    expect(display).toMatchObject({
      text: "gpt-5.5-high -> gpt-5.4-mini-low",
      isRouteMismatch: true,
      mismatchLabel: "模型/思考等级不一致",
    });
    expect(display.title).toContain("请求等级 请求显式");
    expect(display.title).toContain("返回等级 返回显式");

    const audit = buildRequestLogAuditMeta({
      cli_key: "codex",
      path: "/v1/responses",
      status: 200,
      special_settings_json: specialSettingsJson,
      final_provider_id: 2,
    });
    expect(audit.muted).toBe(false);
    expect(audit.tags.map((tag) => tag.label)).toContain("模型路由");
    expect(audit.tags.find((tag) => tag.label === "模型路由")?.className).toContain("rose");
    expect(audit.summary).toBe("模型路由检测：模型/思考等级不一致。");
  });

  it("formats non-Codex model route mismatches from special settings", () => {
    const specialSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "claude",
        requestedModel: "claude-sonnet-4",
        requestedReasoningEffort: "unknown",
        requestedReasoningEffortSource: "unknown",
        actualModel: "gpt-5.4",
        actualReasoningEffort: "unknown",
        actualReasoningEffortSource: "unknown",
        modelMismatch: true,
        effortMismatch: false,
        mismatch: true,
        providerId: 4,
        providerName: "Provider Claude Bridge",
      },
    ]);

    const display = resolveRequestLogModelDisplayMeta(
      "claude",
      "claude-sonnet-4",
      specialSettingsJson,
      null,
      4
    );

    expect(display).toMatchObject({
      text: "claude-sonnet-4 -> gpt-5.4",
      isRouteMismatch: true,
      mismatchLabel: "模型路由不一致",
    });
    expect(display.title).toContain("请求 claude-sonnet-4");
    expect(display.title).toContain("返回 gpt-5.4");
  });

  it("formats Codex model mismatches with unknown response effort", () => {
    const specialSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: null,
        actualReasoningEffortSource: "unknown",
        modelMismatch: true,
        effortMismatch: false,
        mismatch: true,
        providerId: 2,
      },
    ]);

    const display = resolveRequestLogModelDisplayMeta(
      "codex",
      "gpt-5.5",
      specialSettingsJson,
      null,
      2
    );

    expect(display).toMatchObject({
      text: "gpt-5.5-high -> gpt-5.4-mini",
      isRouteMismatch: true,
      mismatchLabel: "模型路由不一致",
      routeMapping: {
        actualReasoningEffort: "unknown",
        actualReasoningEffortSource: "unknown",
      },
    });
    expect(display.title).toContain("返回等级 未知");
  });

  it("labels effort-only model route mismatches", () => {
    const specialSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.5",
        actualReasoningEffort: "medium",
        actualReasoningEffortSource: "response",
        modelMismatch: false,
        effortMismatch: true,
        mismatch: true,
        providerId: 1,
      },
    ]);

    const display = resolveRequestLogModelDisplayMeta(
      "codex",
      "gpt-5.5",
      specialSettingsJson,
      null,
      1
    );
    expect(display.text).toBe("gpt-5.5-high -> gpt-5.5-medium");
    expect(display.mismatchLabel).toBe("思考等级不一致");
  });

  it("hides provider-scoped route mappings when final provider does not match", () => {
    const specialSettingsJson = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        actualReasoningEffortSource: "response",
        modelMismatch: true,
        effortMismatch: true,
        mismatch: true,
        providerId: 2,
      },
    ]);

    const display = resolveRequestLogModelDisplayMeta(
      "codex",
      "gpt-5.5",
      specialSettingsJson,
      null,
      1
    );

    expect(display).toMatchObject({
      text: "gpt-5.5-medium",
      isRouteMismatch: false,
      mismatchLabel: null,
    });
  });

  it("resolves cache creation priority without collapsing missing values into zero", () => {
    expect(resolveCacheCreationDisplay({})).toBeNull();
    expect(
      resolveCacheCreationDisplay({
        cache_creation_input_tokens: null,
        cache_creation_5m_input_tokens: null,
        cache_creation_1h_input_tokens: null,
      })
    ).toBeNull();

    expect(resolveCacheCreationDisplay({ cache_creation_input_tokens: 0 })).toEqual({
      tokens: 0,
      ttl: null,
    });
    expect(resolveCacheCreationDisplay({ cache_creation_1h_input_tokens: 0 })).toEqual({
      tokens: 0,
      ttl: "1h",
    });
    expect(
      resolveCacheCreationDisplay({
        cache_creation_input_tokens: 30,
        cache_creation_5m_input_tokens: 10,
        cache_creation_1h_input_tokens: 20,
      })
    ).toEqual({ tokens: 10, ttl: "5m" });
    expect(
      resolveCacheCreationDisplay({
        cache_creation_input_tokens: 30,
        cache_creation_5m_input_tokens: 0,
        cache_creation_1h_input_tokens: 20,
      })
    ).toEqual({ tokens: 20, ttl: "1h" });
    expect(
      resolveCacheCreationDisplay({
        cache_creation_input_tokens: 30,
        cache_creation_5m_input_tokens: 0,
        cache_creation_1h_input_tokens: 0,
      })
    ).toEqual({ tokens: 30, ttl: null });
    expect(
      resolveCacheCreationDisplay({
        cache_creation_input_tokens: 0,
        cache_creation_5m_input_tokens: 0,
        cache_creation_1h_input_tokens: 0,
      })
    ).toEqual({ tokens: 0, ttl: "5m" });
  });

  it("builds audit meta for muted request log categories", () => {
    const warmup = buildRequestLogAuditMeta({
      cli_key: "claude",
      path: "/v1/messages",
      status: 200,
      special_settings_json: JSON.stringify([{ type: "warmup_intercept" }]),
    });
    expect(warmup.muted).toBe(true);
    expect(warmup.providerFallbackText).toBe("Warmup");
    expect(warmup.tags.map((tag) => tag.label)).toContain("Warmup");

    const guard = buildRequestLogAuditMeta({
      cli_key: "codex",
      path: "/v1/responses",
      status: 200,
      special_settings_json: JSON.stringify([{ type: "cli_proxy_guard" }]),
    });
    expect(guard.providerFallbackText).toBe("CLI 守卫");
    expect(guard.summary).toContain("CLI 代理守卫");

    const clientAbort = buildRequestLogAuditMeta({
      cli_key: "claude",
      path: "/v1/messages",
      status: 499,
      error_code: GatewayErrorCodes.STREAM_ABORTED,
      excluded_from_stats: true,
    });
    expect(clientAbort.tags.map((tag) => tag.label)).toEqual(["客户端中断", "不计统计"]);
    expect(clientAbort.summary).toContain("客户端");

    const allUnavailable = buildRequestLogAuditMeta({
      cli_key: "claude",
      path: "/v1/messages",
      status: 503,
      error_code: GatewayErrorCodes.ALL_PROVIDERS_UNAVAILABLE,
    });
    expect(allUnavailable.providerFallbackText).toBe("无可用供应商");
    expect(allUnavailable.tags.map((tag) => tag.label)).toContain("全部不可用");

    const plain = buildRequestLogAuditMeta({
      cli_key: "claude",
      path: "/v1/messages",
      status: 200,
      special_settings_json: "bad-json",
    });
    expect(plain).toMatchObject({
      muted: false,
      summary: null,
      providerFallbackText: null,
    });
  });

  it("computes status badges across success, failover, errors, and client aborts", () => {
    expect(computeStatusBadge({ status: null, errorCode: null, inProgress: true })).toMatchObject({
      text: "进行中",
      isError: false,
    });
    expect(computeStatusBadge({ status: 200, errorCode: null, hasFailover: true })).toMatchObject({
      text: "200 切换后成功",
      semanticText: "切换供应商后成功",
      hasFailover: true,
    });
    expect(computeStatusBadge({ status: 204, errorCode: null })).toMatchObject({
      text: "204 成功",
      semanticText: "请求成功",
    });
    expect(computeStatusBadge({ status: 500, errorCode: null })).toMatchObject({
      text: "500 失败",
      isError: true,
    });
    expect(
      computeStatusBadge({ status: 200, errorCode: GatewayErrorCodes.STREAM_ERROR })
    ).toMatchObject({
      text: "200 失败",
      semanticText: "请求失败",
      isError: true,
    });
    expect(
      computeStatusBadge({ status: 499, errorCode: GatewayErrorCodes.REQUEST_ABORTED })
    ).toMatchObject({
      text: "499 已中断",
      semanticText: "客户端已中断",
      isClientAbort: true,
    });
    expect(computeStatusBadge({ status: null, errorCode: "CUSTOM" })).toMatchObject({
      text: "失败",
      title: "请求失败 · CUSTOM (CUSTOM)",
    });
    expect(computeStatusBadge({ status: null, errorCode: null })).toMatchObject({
      text: "状态未知",
      title: "状态未知",
    });
  });

  it("resolves reasoning tokens from final usage json shapes", () => {
    expect(
      resolveRequestLogUsageReasoningTokens(
        JSON.stringify({
          output_tokens_details: { reasoning_tokens: 321 },
        })
      )
    ).toBe(321);
    expect(
      resolveRequestLogUsageReasoningTokens(
        JSON.stringify({
          usage: {
            completion_tokens_details: { reasoning_tokens: 654 },
          },
        })
      )
    ).toBe(654);
    expect(
      resolveRequestLogUsageReasoningTokens(
        JSON.stringify({
          reasoning_tokens: 777,
        })
      )
    ).toBe(777);
    expect(
      resolveRequestLogUsageReasoningTokens(
        JSON.stringify({
          outputTokensDetails: { reasoningTokens: 888 },
        })
      )
    ).toBe(888);
    expect(resolveRequestLogUsageReasoningTokens("not-json")).toBeNull();
  });

  it("covers malformed special settings and trace ordering edge cases", () => {
    expect(resolveClaudeModelMappingFromSpecialSettings(null)).toBeNull();
    expect(resolveClaudeModelMappingFromSpecialSettings("123")).toBeNull();
    expect(
      resolveClaudeModelMappingFromSpecialSettings(
        JSON.stringify([
          {
            type: "claude_model_mapping",
            requestedModel: 123,
            effectiveModel: null,
            mappingKind: 0,
            providerId: "bad",
            providerName: 5,
            applied: "yes",
          },
        ])
      )
    ).toBeNull();

    expect(
      computeStatusBadge({ status: null, errorCode: GatewayErrorCodes.REQUEST_ABORTED })
    ).toMatchObject({
      text: "已中断",
      semanticText: "客户端已中断",
    });

    expect(
      resolveLiveTraceProvider(
        createTrace({
          attempts: [
            { attempt_index: 2, provider_name: "Provider A", provider_id: 11 },
            { attempt_index: 1, provider_name: "Provider B", provider_id: 12 },
          ] as TraceSession["attempts"],
        })
      )
    ).toEqual({ providerId: 11, providerName: "Provider A" });
  });

  it("resolves live trace providers and durations", () => {
    expect(resolveLiveTraceProvider(null)).toBeNull();
    expect(resolveLiveTraceProvider(createTrace())).toBeNull();
    expect(
      resolveLiveTraceProvider(
        createTrace({
          attempts: [
            { attempt_index: 0, provider_name: "Unknown" },
            { attempt_index: 1, provider_name: " Provider A ", provider_id: 11 },
            { attempt_index: 2, provider_name: "Provider B" },
          ] as TraceSession["attempts"],
        })
      )
    ).toEqual({ providerId: null, providerName: "Provider B" });
    expect(resolveLiveTraceDurationMs(null)).toBeNull();
    expect(resolveLiveTraceDurationMs(createTrace({ first_seen_ms: 1_000 }), 2_500)).toBe(1_500);
    expect(resolveLiveTraceDurationMs(createTrace({ first_seen_ms: 3_000 }), 2_500)).toBe(0);
  });

  it("builds request route meta summaries and tooltip text", () => {
    expect(
      buildRequestRouteMeta({ route: null, status: null, hasFailover: false, attemptCount: 0 })
    ).toMatchObject({
      hasRoute: false,
      label: "链路",
      summary: "暂无链路信息",
      tooltipText: null,
    });

    const direct = buildRequestRouteMeta({
      route: [createRequestLogRouteHop({ provider_name: "Provider A", ok: true, status: 200 })],
      status: 200,
      hasFailover: false,
      attemptCount: 1,
    });
    expect(direct).toMatchObject({
      hasRoute: true,
      label: "直连完成",
      summary: "直连完成",
      tooltipText: "Provider A（200，成功）",
    });

    const retry = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({
          provider_name: "Provider A",
          ok: false,
          status: 500,
          attempts: 2,
        }),
      ],
      status: 500,
      hasFailover: false,
      attemptCount: 2,
    });
    expect(retry.label).toBe("重试 2 次");
    expect(retry.tooltipText).toBe("Provider A（500，失败，尝试 2 次）");

    const skippedAndRetry = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({
          provider_name: "Unknown",
          ok: false,
          skipped: true,
          status: null,
          attempts: 2,
        }),
        createRequestLogRouteHop({
          provider_name: "Provider B",
          ok: false,
          error_code: GatewayErrorCodes.UPSTREAM_TIMEOUT,
          status: 504,
          attempts: 3,
        }),
      ],
      status: 504,
      hasFailover: false,
      attemptCount: 5,
    });
    expect(skippedAndRetry.label).toBe("跳过 2 个 + 重试");
    expect(skippedAndRetry.summary).toBe("跳过 2 个候选，并重试 3 次");
    expect(skippedAndRetry.tooltipText).toContain("未知（已跳过，尝试 2 次）");
    expect(skippedAndRetry.tooltipText).toContain("Provider B（504，上游超时，尝试 3 次）");

    const failover = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({ provider_name: "Provider A", ok: false, status: 500 }),
        createRequestLogRouteHop({ provider_name: "Provider B", ok: true, status: 200 }),
      ],
      status: 200,
      hasFailover: true,
      attemptCount: 2,
    });
    expect(failover.label).toBe("切换 2 次");
    expect(failover.summary).toBe("切换 2 次后成功");

    const failedFailover = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({ provider_name: "Provider A", ok: false, status: 500 }),
        createRequestLogRouteHop({ provider_name: "Provider B", ok: false, status: 502 }),
      ],
      status: 502,
      hasFailover: true,
      attemptCount: 2,
    });
    expect(failedFailover.summary).toBe("切换 2 次后结束");

    const skippedOnly = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({
          provider_name: "Provider A",
          ok: false,
          skipped: true,
          status: null,
          attempts: 2,
        }),
        createRequestLogRouteHop({ provider_name: "Provider B", ok: true, status: 200 }),
      ],
      status: 200,
      hasFailover: false,
      attemptCount: 3,
    });
    expect(skippedOnly.label).toBe("跳过 2 个");
    expect(skippedOnly.summary).toBe("跳过 2 个候选");

    const implicitAttempts = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({
          provider_name: "Provider C",
          ok: true,
          status: 200,
          attempts: undefined,
        }),
      ],
      status: 200,
      hasFailover: false,
      attemptCount: 1,
    });
    expect(implicitAttempts.label).toBe("直连完成");
    expect(implicitAttempts.tooltipText).toBe("Provider C（200，成功）");

    const retryOnly = buildRequestRouteMeta({
      route: [
        createRequestLogRouteHop({
          provider_name: "Provider A",
          ok: false,
          status: null,
          attempts: 3,
        }),
      ],
      status: null,
      hasFailover: false,
      attemptCount: 3,
    });
    expect(retryOnly.label).toBe("重试 3 次");
    expect(retryOnly.tooltipText).toBe("Provider A（状态未知，失败，尝试 3 次）");
  });
});
