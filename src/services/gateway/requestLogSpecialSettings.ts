import { normalizeClaudeModelMapping, type ClaudeModelMapping } from "./claudeModelMapping";

export type ParsedRequestLogSpecialSetting = {
  type?: string;
  reason?: string;
} & Record<string, unknown>;

export type CodexReasoningGuardSummary = {
  count: number;
  latestRuleMode: string | null;
  latestHitSource: string | null;
  latestRuleLabel: string | null;
  latestReasoningTokens: number | null;
  latestRequestReasoningEffort: string | null;
  latestFinalAnswerOnly: boolean | null;
  latestCommentaryObserved: boolean | null;
  latestHasToolCall: boolean | null;
  latestHasReasoningItem: boolean | null;
  latestPhase: string | null;
  latestActionTaken: string | null;
  latestPostMatchStrategy: string | null;
  latestStrategyOutcome: string | null;
  latestContinuationSentRounds: number | null;
  latestExhaustedAction: string | null;
  latestDelayMs: number | null;
  latestBudgetRemaining: number | null;
  latestBudgetTotal: number | null;
};

export type CodexReasoningFeatureSummary = {
  count: number;
  completeCount: number;
  requestOnlyCount: number;
  finalAnswerOnlyCount: number;
  highXhighFinalAnswerOnlyCount: number;
  highXhighFinalAnswerOnlyCandidateCount: number;
  reasoning516FinalAnswerOnlyNoCommentaryCount: number;
  compactionExemptCount: number;
  latestRuleMode: string | null;
  latestResponseClassification: string | null;
  latestClassificationSkippedReason: string | null;
  latestRequestReasoningEffort: string | null;
  latestReasoningTokens: number | null;
  latestFinalAnswerOnly: boolean | null;
  latestCommentaryObserved: boolean | null;
  latestHasToolCall: boolean | null;
  latestHasReasoningItem: boolean | null;
  latestCompactionExempt: boolean;
  latestCandidate: boolean;
};

export type CodexReasoningContinuationSummary = {
  count: number;
  repairedCount: number;
  nonRepairedCount: number;
  continuationRepairGuardCount: number;
  latestStatus: string | null;
  latestSentRounds: number | null;
  totalSentRounds: number;
  latestReasoningTokens: number | null;
  latestFailureKind: string | null;
  latestReason: string | null;
};

export type CodexReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "unknown";

export type CodexReasoningEffortSource = "request" | "default" | "unknown";

export type CodexReasoningEffortResolution = {
  effort: CodexReasoningEffort;
  source: CodexReasoningEffortSource;
};

const CODEX_REASONING_EFFORTS = new Set<CodexReasoningEffort>([
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
]);

const KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS: Readonly<Record<string, CodexReasoningEffort>> =
  {
    "gpt-5.5": "medium",
    "gpt-5.5-pro": "high",
    "gpt-5.4": "none",
    "gpt-5.4-mini": "none",
    "gpt-5.4-nano": "none",
    "gpt-5.4-pro": "medium",
  };

const CODEX_REASONING_EFFORT_FIELD_NAMES = new Set(["effort", "rawEffort"]);

export function parseRequestLogSpecialSettings(
  specialSettingsJson: string | null | undefined
): ParsedRequestLogSpecialSetting[] {
  if (!specialSettingsJson) return [];

  try {
    const parsed = JSON.parse(specialSettingsJson) as unknown;
    if (Array.isArray(parsed)) {
      return parsed.filter(isParsedRequestLogSpecialSetting);
    }
    return isParsedRequestLogSpecialSetting(parsed) ? [parsed] : [];
  } catch {
    return [];
  }
}

function isParsedRequestLogSpecialSetting(value: unknown): value is ParsedRequestLogSpecialSetting {
  return typeof value === "object" && value !== null;
}

function parsedSettingString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function parsedSettingNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : Number.NaN;
}

function parsedSettingBoolean(value: unknown): boolean {
  return typeof value === "boolean" ? value : false;
}

function parsedSettingNullableBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function parsedSettingNullableNumber(value: unknown): number | null {
  const number = parsedSettingNumber(value);
  return Number.isFinite(number) ? number : null;
}

function normalizeCodexReasoningEffort(
  value: unknown
): Exclude<CodexReasoningEffort, "unknown"> | null {
  const effort = parsedSettingString(value).trim().toLowerCase();
  return CODEX_REASONING_EFFORTS.has(effort as CodexReasoningEffort)
    ? (effort as Exclude<CodexReasoningEffort, "unknown">)
    : null;
}

function normalizeRequestedModel(value: string | null | undefined): string | null {
  const model = value?.trim().toLowerCase();
  return model ? model : null;
}

export function resolveCodexReasoningEffort(
  requestedModel: string | null | undefined,
  specialSettingsJson: string | null | undefined
): CodexReasoningEffortResolution {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const explicitSetting = settings
    .slice()
    .reverse()
    .find((setting) => setting.type === "codex_reasoning_effort");
  const explicitEffort = explicitSetting
    ? normalizeCodexReasoningEffort(explicitSetting.effort)
    : null;

  if (explicitEffort) {
    return { effort: explicitEffort, source: "request" };
  }

  if (explicitSetting && hasCodexReasoningEffortField(explicitSetting)) {
    return { effort: "unknown", source: "unknown" };
  }

  const model = normalizeRequestedModel(requestedModel);
  if (model && KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS[model]) {
    return {
      effort: KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS[model],
      source: "default",
    };
  }

  return { effort: "unknown", source: "unknown" };
}

export function hasExplicitCodexReasoningEffortSpecialSetting(
  specialSettingsJson: string | null | undefined
) {
  return parseRequestLogSpecialSettings(specialSettingsJson).some((setting) => {
    if (setting.type !== "codex_reasoning_effort") return false;
    return normalizeCodexReasoningEffort(setting.effort) !== null;
  });
}

function hasCodexReasoningEffortField(setting: ParsedRequestLogSpecialSetting): boolean {
  return Object.keys(setting).some((key) => CODEX_REASONING_EFFORT_FIELD_NAMES.has(key));
}

export function formatCodexReasoningEffortSource(source: CodexReasoningEffortSource): string {
  if (source === "request") return "请求显式";
  if (source === "default") return "默认推断";
  return "未知";
}

export function resolveClaudeModelMappingFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): ClaudeModelMapping | null {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const mappings = settings
    .map((setting) => {
      if (setting.type !== "claude_model_mapping") return null;
      return normalizeClaudeModelMapping({
        requestedModel: parsedSettingString(setting.requestedModel),
        effectiveModel: parsedSettingString(setting.effectiveModel),
        mappingKind: parsedSettingString(setting.mappingKind),
        providerId: parsedSettingNumber(setting.providerId),
        providerName: parsedSettingString(setting.providerName),
        applied: parsedSettingBoolean(setting.applied),
      });
    })
    .filter((mapping): mapping is ClaudeModelMapping => mapping !== null);

  if (mappings.length === 0) return null;

  if (finalProviderId != null) {
    const finalProviderMapping = mappings
      .slice()
      .reverse()
      .find((mapping) => mapping.providerId === finalProviderId);
    if (finalProviderMapping) return finalProviderMapping;
  }

  return mappings[mappings.length - 1] ?? null;
}

export function hasClaudeModelMappingSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  for (const setting of settings) {
    if (setting.type !== "claude_model_mapping") continue;
    return true;
  }
  return false;
}

export function countCodexReasoningGuardSpecialSettings(
  specialSettingsJson: string | null | undefined
): number {
  return resolveCodexReasoningGuardSummary(specialSettingsJson).count;
}

export function countCodexReasoningFeatureSpecialSettings(
  specialSettingsJson: string | null | undefined
): number {
  return resolveCodexReasoningFeatureSummary(specialSettingsJson).count;
}

function normalizeCodexReasoningGuardCompareSymbol(
  compareMode: unknown,
  compareModeSymbol: unknown
): string | null {
  const explicitSymbol = parsedSettingString(compareModeSymbol);
  if (explicitSymbol === "==" || explicitSymbol === "<=") {
    return explicitSymbol;
  }

  const mode = parsedSettingString(compareMode);
  if (mode === "equals") return "==";
  if (mode === "less_than_or_equal") return "<=";
  return null;
}

function normalizeSpecialSettingToken(value: unknown): string | null {
  const token = parsedSettingString(value).trim();
  return token ? token : null;
}

function isCodexReasoningContinuationStrategy(value: unknown): boolean {
  const strategy = normalizeSpecialSettingToken(value);
  return strategy === "continuation_repair" || strategy === "continuation_repair_experimental";
}

export function resolveCodexReasoningGuardSummary(
  specialSettingsJson: string | null | undefined
): CodexReasoningGuardSummary {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  let count = 0;
  let latestRuleMode: string | null = null;
  let latestHitSource: string | null = null;
  let latestRuleLabel: string | null = null;
  let latestReasoningTokens: number | null = null;
  let latestRequestReasoningEffort: string | null = null;
  let latestFinalAnswerOnly: boolean | null = null;
  let latestCommentaryObserved: boolean | null = null;
  let latestHasToolCall: boolean | null = null;
  let latestHasReasoningItem: boolean | null = null;
  let latestPhase: string | null = null;
  let latestActionTaken: string | null = null;
  let latestPostMatchStrategy: string | null = null;
  let latestStrategyOutcome: string | null = null;
  let latestContinuationSentRounds: number | null = null;
  let latestExhaustedAction: string | null = null;
  let latestDelayMs: number | null = null;
  let latestBudgetRemaining: number | null = null;
  let latestBudgetTotal: number | null = null;

  for (const setting of settings) {
    if (setting.type === "codex_reasoning_guard") {
      count += 1;
      latestRuleMode = normalizeSpecialSettingToken(setting.ruleMode);
      latestHitSource = normalizeSpecialSettingToken(setting.hitSource);
      const compareSymbol = normalizeCodexReasoningGuardCompareSymbol(
        setting.compareMode,
        setting.compareModeSymbol
      );
      const matchedRuleValue = parsedSettingNumber(setting.matchedRuleValue);
      const matchedRuleToken = parsedSettingNumber(setting.matchedRuleToken);
      const matchedRuleName = normalizeSpecialSettingToken(setting.matchedRuleName);
      const matchedCondition = normalizeSpecialSettingToken(setting.matchedCondition);
      const reasoningTokens = parsedSettingNumber(setting.reasoningTokens);
      latestRuleLabel =
        matchedCondition ??
        matchedRuleName ??
        (compareSymbol && Number.isFinite(matchedRuleValue)
          ? `${compareSymbol} ${matchedRuleValue}`
          : Number.isFinite(matchedRuleToken)
            ? `token ${matchedRuleToken}`
            : latestHitSource === "final_answer_only_high_xhigh"
              ? "final-only high/xhigh"
              : null);
      latestReasoningTokens = Number.isFinite(reasoningTokens) ? reasoningTokens : null;
      latestRequestReasoningEffort = normalizeSpecialSettingToken(setting.requestReasoningEffort);
      latestFinalAnswerOnly = parsedSettingNullableBoolean(setting.finalAnswerOnly);
      latestCommentaryObserved = parsedSettingNullableBoolean(setting.commentaryObserved);
      latestHasToolCall = parsedSettingNullableBoolean(setting.hasToolCall);
      latestHasReasoningItem = parsedSettingNullableBoolean(setting.hasReasoningItem);
      latestPhase = parsedSettingString(setting.guardRetryPhase) || null;
      latestActionTaken =
        parsedSettingString(setting.actionTaken) || parsedSettingString(setting.action) || null;
      latestPostMatchStrategy = normalizeSpecialSettingToken(setting.guardPostMatchStrategy);
      latestStrategyOutcome = normalizeSpecialSettingToken(setting.guardStrategyOutcome);
      latestContinuationSentRounds = parsedSettingNullableNumber(setting.continuationSentRounds);
      latestExhaustedAction = parsedSettingString(setting.guardExhaustedAction) || null;
      const delayMs = parsedSettingNumber(setting.backoffMs);
      const budgetRemaining = parsedSettingNumber(setting.guardBudgetRemaining);
      const budgetTotal = parsedSettingNumber(setting.guardBudgetTotal);
      latestDelayMs = Number.isFinite(delayMs) ? delayMs : null;
      latestBudgetRemaining = Number.isFinite(budgetRemaining) ? budgetRemaining : null;
      latestBudgetTotal = Number.isFinite(budgetTotal) ? budgetTotal : null;
    }
  }

  return {
    count,
    latestRuleMode,
    latestHitSource,
    latestRuleLabel,
    latestReasoningTokens,
    latestRequestReasoningEffort,
    latestFinalAnswerOnly,
    latestCommentaryObserved,
    latestHasToolCall,
    latestHasReasoningItem,
    latestPhase,
    latestActionTaken,
    latestPostMatchStrategy,
    latestStrategyOutcome,
    latestContinuationSentRounds,
    latestExhaustedAction,
    latestDelayMs,
    latestBudgetRemaining,
    latestBudgetTotal,
  };
}

export function resolveCodexReasoningContinuationSummary(
  specialSettingsJson: string | null | undefined
): CodexReasoningContinuationSummary {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const hasUnifiedContinuationRecords = settings.some(
    (setting) =>
      setting.type === "codex_reasoning_guard" &&
      isCodexReasoningContinuationStrategy(setting.guardPostMatchStrategy) &&
      normalizeSpecialSettingToken(setting.guardStrategyOutcome) != null
  );
  let count = 0;
  let repairedCount = 0;
  let nonRepairedCount = 0;
  let continuationRepairGuardCount = 0;
  let latestStatus: string | null = null;
  let latestSentRounds: number | null = null;
  let totalSentRounds = 0;
  let latestReasoningTokens: number | null = null;
  let latestFailureKind: string | null = null;
  let latestReason: string | null = null;

  for (const setting of settings) {
    const guardPostMatchStrategy = normalizeSpecialSettingToken(setting.guardPostMatchStrategy);
    if (
      setting.type === "codex_reasoning_guard" &&
      isCodexReasoningContinuationStrategy(guardPostMatchStrategy)
    ) {
      continuationRepairGuardCount += 1;
      const outcome = normalizeSpecialSettingToken(setting.guardStrategyOutcome);
      if (outcome) {
        count += 1;
        const status = outcome === "continuation_repaired" ? "repaired" : outcome;
        const sentRounds = parsedSettingNullableNumber(setting.continuationSentRounds);
        const nonNegativeSentRounds =
          sentRounds != null && sentRounds > 0
            ? Math.floor(sentRounds)
            : sentRounds === 0
              ? 0
              : null;
        if (status === "repaired") {
          repairedCount += 1;
        } else {
          nonRepairedCount += 1;
        }
        latestStatus = status;
        latestSentRounds = nonNegativeSentRounds;
        if (nonNegativeSentRounds != null) totalSentRounds += nonNegativeSentRounds;
        latestReasoningTokens = parsedSettingNullableNumber(setting.reasoningTokens);
        latestFailureKind = normalizeSpecialSettingToken(setting.continuationFailureKind);
        latestReason =
          normalizeSpecialSettingToken(setting.strategyReason) ??
          normalizeSpecialSettingToken(setting.reason);
      }
      continue;
    }

    if (setting.type !== "codex_reasoning_continuation" || hasUnifiedContinuationRecords) continue;

    count += 1;
    const status = normalizeSpecialSettingToken(setting.status) ?? "unknown";
    const sentRounds = parsedSettingNullableNumber(setting.sentRounds);
    const nonNegativeSentRounds =
      sentRounds != null && sentRounds > 0 ? Math.floor(sentRounds) : sentRounds === 0 ? 0 : null;
    const reasoningTokens = parsedSettingNullableNumber(setting.reasoningTokens);

    if (status === "repaired") {
      repairedCount += 1;
    } else {
      nonRepairedCount += 1;
    }

    latestStatus = status;
    latestSentRounds = nonNegativeSentRounds;
    if (nonNegativeSentRounds != null) totalSentRounds += nonNegativeSentRounds;
    latestReasoningTokens = reasoningTokens;
    latestFailureKind = normalizeSpecialSettingToken(setting.failureKind);
    latestReason = normalizeSpecialSettingToken(setting.reason);
  }

  return {
    count,
    repairedCount,
    nonRepairedCount,
    continuationRepairGuardCount,
    latestStatus,
    latestSentRounds,
    totalSentRounds,
    latestReasoningTokens,
    latestFailureKind,
    latestReason,
  };
}

export function resolveCodexReasoningFeatureSummary(
  specialSettingsJson: string | null | undefined
): CodexReasoningFeatureSummary {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  let count = 0;
  let completeCount = 0;
  let requestOnlyCount = 0;
  let finalAnswerOnlyCount = 0;
  let highXhighFinalAnswerOnlyCount = 0;
  let highXhighFinalAnswerOnlyCandidateCount = 0;
  let reasoning516FinalAnswerOnlyNoCommentaryCount = 0;
  let compactionExemptCount = 0;
  let latestRuleMode: string | null = null;
  let latestResponseClassification: string | null = null;
  let latestClassificationSkippedReason: string | null = null;
  let latestRequestReasoningEffort: string | null = null;
  let latestReasoningTokens: number | null = null;
  let latestFinalAnswerOnly: boolean | null = null;
  let latestCommentaryObserved: boolean | null = null;
  let latestHasToolCall: boolean | null = null;
  let latestHasReasoningItem: boolean | null = null;
  let latestCompactionExempt = false;
  let latestCandidate = false;

  for (const setting of settings) {
    if (setting.type !== "codex_reasoning_features") continue;

    count += 1;
    const ruleMode = normalizeSpecialSettingToken(setting.ruleMode);
    const responseClassification = normalizeSpecialSettingToken(setting.responseClassification);
    const skippedReason = normalizeSpecialSettingToken(setting.classificationSkippedReason);
    const requestReasoningEffort =
      normalizeCodexReasoningEffort(setting.requestReasoningEffort) ??
      normalizeCodexReasoningEffort(setting.rawRequestReasoningEffort);
    const reasoningTokens = parsedSettingNullableNumber(setting.reasoningTokens);
    const finalAnswerOnly = parsedSettingNullableBoolean(setting.finalAnswerOnly);
    const commentaryObserved = parsedSettingNullableBoolean(setting.commentaryObserved);
    const hasToolCall = parsedSettingNullableBoolean(setting.hasToolCall);
    const hasReasoningItem = parsedSettingNullableBoolean(setting.hasReasoningItem);
    const compactionExempt =
      normalizeSpecialSettingToken(setting.interceptExemptReason) === "context_compaction";
    const highXhighFinalAnswerOnly =
      finalAnswerOnly === true &&
      (requestReasoningEffort === "high" || requestReasoningEffort === "xhigh");
    const candidate =
      responseClassification === "complete" && highXhighFinalAnswerOnly && !compactionExempt;

    if (responseClassification === "complete") completeCount += 1;
    if (responseClassification === "request_only") requestOnlyCount += 1;
    if (finalAnswerOnly === true) finalAnswerOnlyCount += 1;
    if (highXhighFinalAnswerOnly) highXhighFinalAnswerOnlyCount += 1;
    if (candidate) highXhighFinalAnswerOnlyCandidateCount += 1;
    if (reasoningTokens === 516 && finalAnswerOnly === true && commentaryObserved !== true) {
      reasoning516FinalAnswerOnlyNoCommentaryCount += 1;
    }
    if (compactionExempt) compactionExemptCount += 1;

    latestRuleMode = ruleMode;
    latestResponseClassification = responseClassification;
    latestClassificationSkippedReason = skippedReason;
    latestRequestReasoningEffort = requestReasoningEffort;
    latestReasoningTokens = reasoningTokens;
    latestFinalAnswerOnly = finalAnswerOnly;
    latestCommentaryObserved = commentaryObserved;
    latestHasToolCall = hasToolCall;
    latestHasReasoningItem = hasReasoningItem;
    latestCompactionExempt = compactionExempt;
    latestCandidate = candidate;
  }

  return {
    count,
    completeCount,
    requestOnlyCount,
    finalAnswerOnlyCount,
    highXhighFinalAnswerOnlyCount,
    highXhighFinalAnswerOnlyCandidateCount,
    reasoning516FinalAnswerOnlyNoCommentaryCount,
    compactionExemptCount,
    latestRuleMode,
    latestResponseClassification,
    latestClassificationSkippedReason,
    latestRequestReasoningEffort,
    latestReasoningTokens,
    latestFinalAnswerOnly,
    latestCommentaryObserved,
    latestHasToolCall,
    latestHasReasoningItem,
    latestCompactionExempt,
    latestCandidate,
  };
}
