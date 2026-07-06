import { describe, expect, it } from "vitest";
import {
  countCodexReasoningGuardSpecialSettings,
  formatCodexReasoningEffortSource,
  hasClaudeModelMappingSpecialSetting,
  resolveCodexReasoningContinuationSummary,
  resolveCodexReasoningFeatureSummary,
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

  it("does not count Codex reasoning guard no-intercept decisions as hits", () => {
    const specialSettings = JSON.stringify([
      {
        type: "codex_reasoning_guard_decision",
        hit: false,
        matchedRuleName: "allow fast final answer",
        matchedRuleAction: "no_intercept",
      },
      {
        type: "codex_reasoning_guard",
        hit: true,
        matchedRuleToken: 516,
        matchedRuleName: "custom token 516",
      },
    ]);

    expect(countCodexReasoningGuardSpecialSettings(specialSettings)).toBe(1);
    expect(resolveCodexReasoningGuardSummary(specialSettings)).toMatchObject({
      count: 1,
      latestRuleLabel: "custom token 516",
    });
  });

  it("counts continuation repair guard records as ordinary Codex reasoning guard hits", () => {
    const specialSettings = JSON.stringify([
      {
        type: "codex_reasoning_guard",
        ruleSource: "continuation_repair",
        matchedRuleName: "reasoning_tokens == 518*n-2",
        reasoningTokens: 516,
      },
      {
        type: "codex_reasoning_guard",
        compareModeSymbol: "<=",
        matchedRuleValue: 300,
        reasoningTokens: 300,
      },
    ]);

    expect(countCodexReasoningGuardSpecialSettings(specialSettings)).toBe(2);
    expect(resolveCodexReasoningGuardSummary(specialSettings)).toMatchObject({
      count: 2,
      latestRuleLabel: "<= 300",
      latestReasoningTokens: 300,
    });
    expect(
      countCodexReasoningGuardSpecialSettings(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            ruleSource: " Continuation_Repair ",
            matchedRuleName: "reasoning_tokens == 518*n-2",
          },
        ])
      )
    ).toBe(1);

    const mixedSpecialSettings = JSON.stringify([
      {
        type: "codex_reasoning_guard",
        compareModeSymbol: "<=",
        matchedRuleValue: 516,
        reasoningTokens: 300,
      },
      {
        type: "codex_reasoning_continuation",
        status: "repaired",
        sentRounds: 2,
        reasoningTokens: 51,
      },
      {
        type: "codex_reasoning_guard",
        ruleSource: " Continuation_Repair ",
        matchedRuleName: "reasoning_tokens == 518*n-2",
        reasoningTokens: 516,
        guardPostMatchStrategy: "continuation_repair",
        guardStrategyOutcome: "continuation_repaired",
        continuationSentRounds: 2,
      },
    ]);
    expect(countCodexReasoningGuardSpecialSettings(mixedSpecialSettings)).toBe(2);
    expect(resolveCodexReasoningGuardSummary(mixedSpecialSettings)).toMatchObject({
      count: 2,
      latestRuleLabel: "reasoning_tokens == 518*n-2",
      latestReasoningTokens: 516,
      latestPostMatchStrategy: "continuation_repair",
      latestStrategyOutcome: "continuation_repaired",
      latestContinuationSentRounds: 2,
    });
    expect(resolveCodexReasoningContinuationSummary(mixedSpecialSettings)).toMatchObject({
      count: 1,
      repairedCount: 1,
      continuationRepairGuardCount: 1,
      latestStatus: "repaired",
    });
  });

  it("resolves Codex reasoning continuation repair summary", () => {
    const specialSettings = JSON.stringify([
      {
        type: "codex_reasoning_continuation",
        status: "failed",
        sentRounds: 1,
        reasoningTokens: 2070,
        failureKind: "aggregate",
        reason: "still matches",
      },
      {
        type: "codex_reasoning_guard",
        ruleSource: "continuation_repair",
        matchedRuleName: "reasoning_tokens == 518*n-2",
        reasoningTokens: 516,
        guardPostMatchStrategy: "continuation_repair",
        guardStrategyOutcome: "continuation_repaired",
        continuationSentRounds: 2,
      },
      {
        type: "codex_reasoning_continuation",
        status: "repaired",
        sentRounds: 2,
        reasoningTokens: 51,
      },
    ]);

    expect(resolveCodexReasoningContinuationSummary(specialSettings)).toEqual({
      count: 1,
      repairedCount: 1,
      nonRepairedCount: 0,
      continuationRepairGuardCount: 1,
      latestStatus: "repaired",
      latestSentRounds: 2,
      totalSentRounds: 2,
      latestReasoningTokens: 516,
      latestFailureKind: null,
      latestReason: null,
    });
    expect(resolveCodexReasoningContinuationSummary("bad-json").count).toBe(0);
  });

  it("counts experimental Codex reasoning continuation repair guard records", () => {
    const specialSettings = JSON.stringify([
      {
        type: "codex_reasoning_guard",
        ruleSource: "continuation_repair",
        matchedRuleName: "reasoning_tokens == 518*n-2",
        reasoningTokens: 516,
        guardPostMatchStrategy: "continuation_repair_experimental",
        guardStrategyOutcome: "continuation_repaired",
        continuationSentRounds: 2,
      },
    ]);

    expect(resolveCodexReasoningGuardSummary(specialSettings)).toMatchObject({
      count: 1,
      latestPostMatchStrategy: "continuation_repair_experimental",
      latestStrategyOutcome: "continuation_repaired",
      latestContinuationSentRounds: 2,
    });
    expect(resolveCodexReasoningContinuationSummary(specialSettings)).toMatchObject({
      count: 1,
      repairedCount: 1,
      nonRepairedCount: 0,
      continuationRepairGuardCount: 1,
      latestStatus: "repaired",
      latestSentRounds: 2,
      totalSentRounds: 2,
    });
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
            guardRetryPhase: "immediate",
            actionTaken: "retry_same_provider_no_circuit",
            guardBudgetRemaining: 9,
            guardBudgetTotal: 10,
          },
          {
            type: "codex_reasoning_guard",
            compareMode: "less_than_or_equal",
            compareModeSymbol: "<=",
            matchedRuleValue: 516,
            reasoningTokens: 300,
            guardRetryPhase: "delayed",
            actionTaken: "retry_same_provider_delayed_no_circuit",
            guardExhaustedAction: "return_error",
            backoffMs: 1000,
            guardBudgetRemaining: 4,
            guardBudgetTotal: 10,
          },
        ])
      )
    ).toEqual({
      count: 2,
      latestRuleMode: null,
      latestHitSource: null,
      latestRuleLabel: "<= 516",
      latestReasoningTokens: 300,
      latestRequestReasoningEffort: null,
      latestFinalAnswerOnly: null,
      latestCommentaryObserved: null,
      latestHasToolCall: null,
      latestHasReasoningItem: null,
      latestPhase: "delayed",
      latestActionTaken: "retry_same_provider_delayed_no_circuit",
      latestPostMatchStrategy: null,
      latestStrategyOutcome: null,
      latestContinuationSentRounds: null,
      latestExhaustedAction: "return_error",
      latestDelayMs: 1000,
      latestBudgetRemaining: 4,
      latestBudgetTotal: 10,
    });
  });

  it("keeps latest switch-model action in Codex reasoning guard summary", () => {
    expect(
      resolveCodexReasoningGuardSummary(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            matchedRuleValue: 516,
            reasoningTokens: 516,
            actionTaken: "retry_same_provider_no_circuit",
          },
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            matchedRuleValue: 516,
            reasoningTokens: 516,
            actionTaken: "switch_model_no_circuit",
            guardExhaustedAction: "switch_model",
          },
        ])
      )
    ).toMatchObject({
      count: 2,
      latestActionTaken: "switch_model_no_circuit",
      latestExhaustedAction: "switch_model",
    });
  });

  it("resolves Codex reasoning guard feature-hit metadata", () => {
    expect(
      resolveCodexReasoningGuardSummary(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            ruleMode: "final_answer_only_high_xhigh",
            hitSource: "final_answer_only_high_xhigh",
            requestReasoningEffort: "xhigh",
            finalAnswerOnly: true,
            commentaryObserved: false,
            hasToolCall: false,
            hasReasoningItem: false,
          },
        ])
      )
    ).toMatchObject({
      count: 1,
      latestRuleMode: "final_answer_only_high_xhigh",
      latestHitSource: "final_answer_only_high_xhigh",
      latestRuleLabel: "final-only high/xhigh",
      latestRequestReasoningEffort: "xhigh",
      latestFinalAnswerOnly: true,
      latestCommentaryObserved: false,
      latestHasToolCall: false,
      latestHasReasoningItem: false,
    });
  });

  it("resolves Codex reasoning passive feature samples", () => {
    const summary = resolveCodexReasoningFeatureSummary(
      JSON.stringify([
        {
          type: "codex_reasoning_features",
          ruleMode: "final_answer_only_high_xhigh",
          reasoningTokens: 516,
          requestReasoningEffort: "high",
          responseClassification: "complete",
          finalAnswerOnly: true,
          commentaryObserved: false,
          hasToolCall: false,
          hasReasoningItem: false,
        },
        {
          type: "codex_reasoning_features",
          ruleMode: "final_answer_only_high_xhigh",
          requestReasoningEffort: "xhigh",
          responseClassification: "request_only",
          classificationSkippedReason: "guard_disabled_stream_not_buffered",
          finalAnswerOnly: null,
          commentaryObserved: null,
          interceptExemptReason: "context_compaction",
        },
      ])
    );

    expect(summary).toMatchObject({
      count: 2,
      completeCount: 1,
      requestOnlyCount: 1,
      finalAnswerOnlyCount: 1,
      highXhighFinalAnswerOnlyCount: 1,
      highXhighFinalAnswerOnlyCandidateCount: 1,
      reasoning516FinalAnswerOnlyNoCommentaryCount: 1,
      compactionExemptCount: 1,
      latestRuleMode: "final_answer_only_high_xhigh",
      latestResponseClassification: "request_only",
      latestClassificationSkippedReason: "guard_disabled_stream_not_buffered",
      latestRequestReasoningEffort: "xhigh",
      latestReasoningTokens: null,
      latestFinalAnswerOnly: null,
      latestCommentaryObserved: null,
      latestCompactionExempt: true,
      latestCandidate: false,
    });
    expect(resolveCodexReasoningFeatureSummary("bad-json").count).toBe(0);
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
