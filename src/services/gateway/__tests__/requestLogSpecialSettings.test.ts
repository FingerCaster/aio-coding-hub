import { describe, expect, it } from "vitest";
import {
  chooseModelRouteAwareSpecialSettingsJson,
  formatCodexReasoningEffortSource,
  formatUpstreamErrorResponseRuleTooltip,
  hasClaudeModelMappingSpecialSetting,
  hasCodexSystemRequestSpecialSetting,
  hasExplicitCodexReasoningEffortSpecialSetting,
  hasModelRouteMappingSpecialSetting,
  resolveClaudeModelMappingFromSpecialSettings,
  resolveAioManagedModelRouteFromSpecialSettings,
  resolveCodexReasoningEffort,
  resolveConfiguredModelRouteFromSpecialSettings,
  resolveModelRouteMappingFromSpecialSettings,
  resolveRequestLogReasoningEffort,
  resolveUpstreamErrorResponseRuleMarker,
} from "../requestLogSpecialSettings";

describe("services/gateway/requestLogSpecialSettings", () => {
  it("resolves only an applied configured route for the final provider", () => {
    const settings = JSON.stringify([
      {
        type: "configured_model_route",
        providerId: 1,
        providerName: "Primary",
        policySource: "global",
        sourceModel: "source",
        targetModel: "target",
        effectiveModel: "target",
        reasoningEffort: null,
        pricedCliKey: "claude",
        applied: true,
        modelApplied: true,
        reasoningEffortApplied: false,
      },
      {
        type: "configured_model_route",
        providerId: 2,
        providerName: "Backup",
        policySource: "provider",
        sourceModel: "source",
        targetModel: null,
        effectiveModel: "source",
        reasoningEffort: "low",
        pricedCliKey: "claude",
        applied: true,
        modelApplied: false,
        reasoningEffortApplied: true,
      },
    ]);

    expect(resolveConfiguredModelRouteFromSpecialSettings(settings, 2)).toEqual({
      providerId: 2,
      providerName: "Backup",
      policySource: "provider",
      sourceModel: "source",
      targetModel: null,
      effectiveModel: "source",
      reasoningEffort: "low",
      pricedCliKey: "claude",
      modelApplied: false,
      reasoningEffortApplied: true,
      applied: true,
    });
    expect(resolveConfiguredModelRouteFromSpecialSettings(settings, 99)).toBeNull();
  });

  it("accepts effort-only routing after an earlier model rewrite", () => {
    const settings = JSON.stringify([
      {
        type: "configured_model_route",
        providerId: 7,
        policySource: "provider",
        sourceModel: "source",
        targetModel: null,
        effectiveModel: "bridge-target",
        reasoningEffort: "low",
        pricedCliKey: "codex",
        applied: true,
        modelApplied: false,
        reasoningEffortApplied: true,
      },
    ]);

    expect(resolveConfiguredModelRouteFromSpecialSettings(settings, 7)).toMatchObject({
      sourceModel: "source",
      targetModel: null,
      effectiveModel: "bridge-target",
      reasoningEffort: "low",
      modelApplied: false,
      reasoningEffortApplied: true,
    });
  });

  it("rejects pending, malformed, unbounded, and internally inconsistent route markers", () => {
    const invalid = [
      "bad-json",
      JSON.stringify([{ type: "configured_model_route", applied: false }]),
      JSON.stringify([
        {
          type: "configured_model_route",
          providerId: 1,
          policySource: "future",
          sourceModel: "source",
          targetModel: "target",
          effectiveModel: "target",
          applied: true,
          modelApplied: true,
          reasoningEffortApplied: false,
        },
      ]),
      JSON.stringify([
        {
          type: "configured_model_route",
          providerId: 1,
          policySource: "global",
          sourceModel: "source",
          targetModel: "target",
          effectiveModel: "different",
          applied: true,
          modelApplied: true,
          reasoningEffortApplied: false,
        },
      ]),
      JSON.stringify([
        {
          type: "configured_model_route",
          providerId: 1,
          policySource: "global",
          sourceModel: "模".repeat(86),
          targetModel: null,
          effectiveModel: "模".repeat(86),
          reasoningEffort: "low",
          applied: true,
          modelApplied: false,
          reasoningEffortApplied: true,
        },
      ]),
    ];

    for (const value of invalid) {
      expect(resolveConfiguredModelRouteFromSpecialSettings(value)).toBeNull();
    }
  });

  it("parses response-rule audit markers without exposing response content", () => {
    const marker = resolveUpstreamErrorResponseRuleMarker(
      JSON.stringify([
        {
          type: "upstream_error_response_rule",
          ruleId: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51",
          ruleName: "限额响应",
          providerId: 7,
          providerName: "中转站",
          upstreamStatus: 429,
          clientStatus: 503,
          statusMode: "override",
          messageMode: "passthrough",
        },
      ])
    );

    expect(marker).not.toBeNull();
    expect(formatUpstreamErrorResponseRuleTooltip(marker!)).toContain("状态码：429 → 503");
    expect(formatUpstreamErrorResponseRuleTooltip(marker!)).toContain("信息行为：提取并透传");
    expect(marker).not.toHaveProperty("message");
  });

  it("fails open for malformed or future response-rule markers", () => {
    for (const marker of [
      "bad-json",
      JSON.stringify({ type: "upstream_error_response_rule" }),
      JSON.stringify({
        type: "upstream_error_response_rule",
        ruleId: "id",
        ruleName: "rule",
        providerId: 1,
        providerName: "provider",
        upstreamStatus: 429,
        clientStatus: 200,
        statusMode: "override",
        messageMode: "passthrough",
      }),
      JSON.stringify({
        type: "upstream_error_response_rule",
        ruleId: "id",
        ruleName: "rule",
        providerId: 1,
        providerName: "provider",
        upstreamStatus: 429,
        clientStatus: 503,
        statusMode: "future",
        messageMode: "passthrough",
      }),
    ]) {
      expect(resolveUpstreamErrorResponseRuleMarker(marker)).toBeNull();
    }
  });

  it("resolves Claude model mapping with final provider preference", () => {
    const settings = JSON.stringify([
      { type: "noop" },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-4.1 ",
        mappingKind: " sonnet ",
        providerId: 1,
        providerName: " Provider A ",
        applied: true,
      },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-5.4 ",
        mappingKind: " sonnet ",
        providerId: 2,
        providerName: " Provider B ",
        applied: true,
      },
    ]);

    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 2)).toEqual({
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      mappingKind: "sonnet",
      providerId: 2,
      providerName: "Provider B",
      applied: true,
    });
    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 99)?.providerId).toBe(2);
    expect(hasClaudeModelMappingSpecialSetting(settings)).toBe(true);
  });

  it("ignores invalid, unapplied, and identity Claude mappings", () => {
    const settings = JSON.stringify([
      {
        type: "claude_model_mapping",
        requestedModel: "same",
        effectiveModel: "same",
        mappingKind: "sonnet",
        providerId: 1,
        providerName: "Provider A",
        applied: true,
      },
      {
        type: "claude_model_mapping",
        requestedModel: "claude-sonnet",
        effectiveModel: "gpt-5.4",
        mappingKind: "sonnet",
        providerId: 2,
        providerName: "Provider B",
        applied: false,
      },
    ]);

    expect(resolveClaudeModelMappingFromSpecialSettings(settings)).toBeNull();
    expect(resolveClaudeModelMappingFromSpecialSettings("bad-json")).toBeNull();
    expect(hasClaudeModelMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("resolves explicit Codex reasoning effort", () => {
    expect(
      resolveCodexReasoningEffort(
        "gpt-5.5",
        JSON.stringify([{ type: "codex_reasoning_effort", effort: " HIGH " }])
      )
    ).toEqual({ effort: "high", source: "request" });
    expect(
      resolveCodexReasoningEffort(
        "gpt-5.5",
        JSON.stringify([{ type: "codex_reasoning_effort", rawEffort: "Ultra" }])
      )
    ).toEqual({ effort: "ultra", source: "request" });
    expect(
      hasExplicitCodexReasoningEffortSpecialSetting(
        JSON.stringify([{ type: "codex_reasoning_effort", rawEffort: "Ultra" }])
      )
    ).toBe(true);
  });

  it("uses conservative Codex effort defaults and rejects invalid explicit values", () => {
    expect(resolveCodexReasoningEffort(" gpt-5.5 ", null)).toEqual({
      effort: "medium",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.4-mini", "bad-json")).toEqual({
      effort: "none",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-future", null)).toEqual({
      effort: "unknown",
      source: "unknown",
    });
    expect(
      resolveCodexReasoningEffort(
        "gpt-5.5",
        JSON.stringify([{ type: "codex_reasoning_effort", rawEffort: "turbo" }])
      )
    ).toEqual({ effort: "unknown", source: "unknown" });
    expect(formatCodexReasoningEffortSource("request")).toBe("请求显式");
    expect(formatCodexReasoningEffortSource("default")).toBe("默认推断");
    expect(formatCodexReasoningEffortSource("unknown")).toBe("未知");
  });

  it("prefers observed effort and only falls back for compatible Codex records", () => {
    const settings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);

    expect(
      resolveRequestLogReasoningEffort({
        observedReasoningEffort: " Future-Level ",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        specialSettingsJson: settings,
      })
    ).toBe("Future-Level");
    expect(
      resolveRequestLogReasoningEffort({
        observedReasoningEffort: null,
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        specialSettingsJson: settings,
      })
    ).toBe("high");
    expect(
      resolveRequestLogReasoningEffort({
        observedReasoningEffort: null,
        cliKey: "claude",
        requestedModel: "claude-sonnet",
        specialSettingsJson: settings,
      })
    ).toBeNull();
    expect(
      resolveRequestLogReasoningEffort({
        observedReasoningEffort: null,
        cliKey: "codex",
        requestedModel: "gpt-future",
        specialSettingsJson: null,
      })
    ).toBeNull();
  });

  it("resolves model route mapping with final provider preference", () => {
    const settings = JSON.stringify([
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
        providerName: "Provider A",
      },
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        modelMismatch: true,
        effortMismatch: true,
        mismatch: true,
        providerId: 2,
        providerName: "Provider B",
      },
    ]);

    expect(resolveModelRouteMappingFromSpecialSettings(settings, 1)).toMatchObject({
      actualModel: "gpt-5.5",
      effortMismatch: true,
      providerId: 1,
    });
    expect(resolveModelRouteMappingFromSpecialSettings(settings, 99)).toBeNull();
    expect(hasModelRouteMappingSpecialSetting(settings)).toBe(true);
  });

  it("resolves only applied provider-scoped AIO managed routes", () => {
    const settings = JSON.stringify([
      {
        type: "aio_managed_model_route",
        canonicalModel: " aio/model-a ",
        providerId: 11,
        remoteModelId: " grok-4.5 ",
        requestedUpstreamModel: " grok-4.5 ",
        pricedModel: " grok-4.5 ",
        applied: true,
      },
      {
        type: "aio_managed_model_route",
        canonicalModel: "aio/model-b",
        providerId: 12,
        remoteModelId: "gpt-5.5",
        wireModel: "gpt-5.5-wire",
        applied: true,
      },
    ]);

    expect(resolveAioManagedModelRouteFromSpecialSettings(settings, 11)).toEqual({
      canonicalModel: "aio/model-a",
      providerId: 11,
      providerUuid: null,
      remoteModelId: "grok-4.5",
      requestedUpstreamModel: "grok-4.5",
      pricedModel: "grok-4.5",
      applied: true,
    });
    expect(
      resolveAioManagedModelRouteFromSpecialSettings(settings, 12)?.requestedUpstreamModel
    ).toBe("gpt-5.5-wire");
    expect(resolveAioManagedModelRouteFromSpecialSettings(settings, 99)).toBeNull();
    expect(
      resolveAioManagedModelRouteFromSpecialSettings(
        JSON.stringify([
          {
            type: "aio_managed_model_route",
            canonicalModel: "aio/model-a",
            providerId: 11,
            remoteModelId: "grok-4.5",
            applied: false,
          },
        ])
      )
    ).toBeNull();
  });

  it("ignores invalid and identity model route mappings", () => {
    const settings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "GPT-5.5",
        actualModel: "gpt-5.5",
        requestedReasoningEffort: "medium",
        actualReasoningEffort: "medium",
        mismatch: false,
      },
      { type: "model_route_mapping", requestedModel: "", actualModel: "gpt-5.4", mismatch: true },
    ]);

    expect(resolveModelRouteMappingFromSpecialSettings(settings)).toBeNull();
    expect(resolveModelRouteMappingFromSpecialSettings("bad-json")).toBeNull();
    expect(hasModelRouteMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("chooses model-route-aware special settings ahead of start settings", () => {
    const startSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);
    const terminalSettings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "gpt-5.5",
        actualModel: "gpt-5.4-mini",
        mismatch: true,
      },
    ]);

    expect(chooseModelRouteAwareSpecialSettingsJson(startSettings, terminalSettings)).toBe(
      terminalSettings
    );
    expect(chooseModelRouteAwareSpecialSettingsJson("bad-json", startSettings)).toBe(startSettings);
  });

  it("preserves an applied AIO managed route when start settings arrive late", () => {
    const startSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);
    const managedRouteSettings = JSON.stringify([
      {
        type: "aio_managed_model_route",
        canonicalModel: "aio/model-a",
        providerId: 11,
        remoteModelId: "grok-4.5",
        requestedUpstreamModel: "grok-4.5",
        applied: true,
      },
    ]);

    expect(chooseModelRouteAwareSpecialSettingsJson(startSettings, managedRouteSettings)).toBe(
      managedRouteSettings
    );
    expect(chooseModelRouteAwareSpecialSettingsJson(managedRouteSettings, startSettings)).toBe(
      managedRouteSettings
    );
  });

  it("never lets a managed-only attempt hide a real model route mismatch", () => {
    const managedRouteSettings = JSON.stringify([
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
    const mismatchSettings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "grok-4.5",
        actualModel: "grok-4.5-preview",
        mismatch: true,
      },
    ]);

    expect(chooseModelRouteAwareSpecialSettingsJson(managedRouteSettings, mismatchSettings)).toBe(
      mismatchSettings
    );
    expect(chooseModelRouteAwareSpecialSettingsJson(mismatchSettings, managedRouteSettings)).toBe(
      mismatchSettings
    );
  });

  it("identifies only the structured Codex system request marker", () => {
    expect(
      hasCodexSystemRequestSpecialSetting(
        JSON.stringify([{ type: "codex_system_request", threadSource: "system" }])
      )
    ).toBe(true);

    for (const settings of [
      [{ type: "codex_system_request" }],
      [{ type: "codex_system_request", threadSource: "user" }],
      [{ type: "other", threadSource: "system" }],
    ]) {
      expect(hasCodexSystemRequestSpecialSetting(JSON.stringify(settings))).toBe(false);
    }
    expect(hasCodexSystemRequestSpecialSetting("bad-json")).toBe(false);
  });

  it("keeps Codex system, effort, and model-route settings composable", () => {
    const settings = JSON.stringify([
      { type: "codex_system_request", threadSource: "system" },
      { type: "codex_reasoning_effort", effort: "high" },
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        actualModel: "gpt-5.4-mini",
        mismatch: true,
        providerId: 2,
      },
    ]);

    expect(hasCodexSystemRequestSpecialSetting(settings)).toBe(true);
    expect(resolveCodexReasoningEffort("gpt-5.5", settings)).toEqual({
      effort: "high",
      source: "request",
    });
    expect(resolveModelRouteMappingFromSpecialSettings(settings, 2)).toMatchObject({
      requestedModel: "gpt-5.5",
      actualModel: "gpt-5.4-mini",
      providerId: 2,
    });
  });
});
