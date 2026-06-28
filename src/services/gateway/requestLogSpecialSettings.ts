import { normalizeClaudeModelMapping, type ClaudeModelMapping } from "./claudeModelMapping";

export type ParsedRequestLogSpecialSetting = {
  type?: string;
  reason?: string;
} & Record<string, unknown>;

export type CodexReasoningGuardSummary = {
  count: number;
  latestRuleLabel: string | null;
  latestReasoningTokens: number | null;
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

export function resolveCodexReasoningGuardSummary(
  specialSettingsJson: string | null | undefined
): CodexReasoningGuardSummary {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  let count = 0;
  let latestRuleLabel: string | null = null;
  let latestReasoningTokens: number | null = null;

  for (const setting of settings) {
    if (setting.type === "codex_reasoning_guard") {
      count += 1;
      const compareSymbol = normalizeCodexReasoningGuardCompareSymbol(
        setting.compareMode,
        setting.compareModeSymbol
      );
      const matchedRuleValue = parsedSettingNumber(setting.matchedRuleValue);
      const reasoningTokens = parsedSettingNumber(setting.reasoningTokens);
      latestRuleLabel =
        compareSymbol && Number.isFinite(matchedRuleValue)
          ? `${compareSymbol} ${matchedRuleValue}`
          : null;
      latestReasoningTokens = Number.isFinite(reasoningTokens) ? reasoningTokens : null;
    }
  }

  return {
    count,
    latestRuleLabel,
    latestReasoningTokens,
  };
}
