import { describe, expect, it } from "vitest";
import {
  countCodexReasoningGuardSpecialSettings,
  formatCodexReasoningEffortSource,
  hasClaudeModelMappingSpecialSetting,
  resolveCodexReasoningEffort,
  resolveCodexReasoningGuardSummary,
  resolveClaudeModelMappingFromSpecialSettings,
} from "../requestLogSpecialSettings";

describe("services/gateway/requestLogSpecialSettings", () => {
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

  it("ignores invalid, unapplied, and identity mappings", () => {
    expect(resolveClaudeModelMappingFromSpecialSettings(null)).toBeNull();
    expect(resolveClaudeModelMappingFromSpecialSettings("bad-json")).toBeNull();
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
          {
            type: "claude_model_mapping",
            requestedModel: "claude-sonnet",
            effectiveModel: "gpt-5.4",
            mappingKind: "sonnet",
            providerId: 2,
            providerName: "Provider B",
            applied: false,
          },
        ])
      )
    ).toBeNull();
    expect(hasClaudeModelMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("counts Codex reasoning guard special settings", () => {
    expect(
      countCodexReasoningGuardSpecialSettings(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            compareModeSymbol: "==",
            matchedRuleValue: 516,
            reasoningTokens: 516,
          },
          { type: "noop" },
          {
            type: "codex_reasoning_guard",
            compareMode: "less_than_or_equal",
            compareModeSymbol: "<=",
            matchedRuleValue: 516,
            reasoningTokens: 300,
          },
        ])
      )
    ).toBe(2);
    expect(
      countCodexReasoningGuardSpecialSettings(JSON.stringify({ type: "codex_reasoning_guard" }))
    ).toBe(1);
    expect(countCodexReasoningGuardSpecialSettings("bad-json")).toBe(0);
  });

  it("resolves Codex reasoning guard summary with latest rule label", () => {
    expect(
      resolveCodexReasoningGuardSummary(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            matchedRuleValue: 516,
            reasoningTokens: 516,
          },
          {
            type: "codex_reasoning_guard",
            compareMode: "less_than_or_equal",
            compareModeSymbol: "<=",
            matchedRuleValue: 516,
            reasoningTokens: 300,
          },
        ])
      )
    ).toEqual({
      count: 2,
      latestRuleLabel: "<= 516",
      latestReasoningTokens: 300,
    });
  });

  it("resolves explicit Codex reasoning effort from special settings", () => {
    const settings = JSON.stringify([
      {
        type: "codex_reasoning_effort",
        source: "request",
        effort: " HIGH ",
      },
    ]);

    expect(resolveCodexReasoningEffort("gpt-5.5", settings)).toEqual({
      effort: "high",
      source: "request",
    });
  });

  it("uses conservative Codex effort defaults and unknown fallback", () => {
    expect(resolveCodexReasoningEffort(" gpt-5.5 ", null)).toEqual({
      effort: "medium",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.4-mini", "bad-json")).toEqual({
      effort: "none",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.5-pro", null)).toEqual({
      effort: "high",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.4-pro", null)).toEqual({
      effort: "medium",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-future", null)).toEqual({
      effort: "unknown",
      source: "unknown",
    });
  });

  it("does not use defaults when an explicit Codex reasoning effort is invalid", () => {
    const settings = JSON.stringify([
      {
        type: "codex_reasoning_effort",
        source: "request",
        rawEffort: "turbo",
      },
    ]);

    expect(resolveCodexReasoningEffort("gpt-5.5", settings)).toEqual({
      effort: "unknown",
      source: "unknown",
    });
  });

  it("formats Codex reasoning effort source labels", () => {
    expect(formatCodexReasoningEffortSource("request")).toBe("请求显式");
    expect(formatCodexReasoningEffortSource("default")).toBe("默认推断");
    expect(formatCodexReasoningEffortSource("unknown")).toBe("未知");
  });
});
