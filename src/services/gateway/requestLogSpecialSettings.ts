import { normalizeClaudeModelMapping, type ClaudeModelMapping } from "./claudeModelMapping";

export type ParsedRequestLogSpecialSetting = {
  type?: string;
  reason?: string;
} & Record<string, unknown>;

export type CodexReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "ultra"
  | "unknown";

export type CodexReasoningEffortSource = "request" | "default" | "unknown";
export type ModelRouteReasoningEffortSource =
  | CodexReasoningEffortSource
  | "model_default"
  | "response";

export type CodexReasoningEffortResolution = {
  effort: CodexReasoningEffort;
  source: CodexReasoningEffortSource;
};

export type ModelRouteMapping = {
  cliKey: string;
  requestedModel: string;
  requestedReasoningEffort: CodexReasoningEffort;
  requestedReasoningEffortSource: ModelRouteReasoningEffortSource;
  actualModel: string;
  actualReasoningEffort: CodexReasoningEffort;
  actualReasoningEffortSource: ModelRouteReasoningEffortSource;
  modelMismatch: boolean;
  effortMismatch: boolean;
  mismatch: boolean;
  providerId: number | null;
  providerName: string | null;
};

type KnownCodexReasoningEffort = Exclude<CodexReasoningEffort, "unknown">;

const CODEX_REASONING_EFFORTS = new Set<KnownCodexReasoningEffort>([
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
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

export const CODEX_SYSTEM_REQUEST_SPECIAL_SETTING = {
  type: "codex_system_request",
  threadSource: "system",
} as const;

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

function normalizeCodexReasoningEffort(value: unknown): KnownCodexReasoningEffort | null {
  const effort = parsedSettingString(value).trim().toLowerCase();
  return CODEX_REASONING_EFFORTS.has(effort as KnownCodexReasoningEffort)
    ? (effort as KnownCodexReasoningEffort)
    : null;
}

function normalizeModelRouteReasoningEffort(value: unknown): CodexReasoningEffort {
  return normalizeCodexReasoningEffort(value) ?? "unknown";
}

function normalizeModelRouteReasoningEffortSource(value: unknown): ModelRouteReasoningEffortSource {
  const source = parsedSettingString(value).trim().toLowerCase();
  if (source === "request") return "request";
  if (source === "default") return "default";
  if (source === "model_default") return "model_default";
  if (source === "response") return "response";
  return "unknown";
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
    ? (normalizeCodexReasoningEffort(explicitSetting.effort) ??
      normalizeCodexReasoningEffort(explicitSetting.rawEffort))
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
    return (
      (normalizeCodexReasoningEffort(setting.effort) ??
        normalizeCodexReasoningEffort(setting.rawEffort)) !== null
    );
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

export function formatModelRouteReasoningEffortSource(
  source: ModelRouteReasoningEffortSource
): string {
  if (source === "request") return "请求显式";
  if (source === "default") return "默认推断";
  if (source === "model_default") return "模型默认推断";
  if (source === "response") return "返回显式";
  return "未知";
}

function normalizeRouteText(value: unknown): string | null {
  const text = parsedSettingString(value).trim();
  return text ? text : null;
}

function normalizeRouteNumber(value: unknown): number | null {
  const number = parsedSettingNumber(value);
  return Number.isFinite(number) ? number : null;
}

function sameRouteText(left: string, right: string): boolean {
  return left.trim().toLowerCase() === right.trim().toLowerCase();
}

function normalizeModelRouteMappingSetting(
  setting: ParsedRequestLogSpecialSetting
): ModelRouteMapping | null {
  if (setting.type !== "model_route_mapping") return null;

  const requestedModel = normalizeRouteText(setting.requestedModel);
  const actualModel = normalizeRouteText(setting.actualModel);
  if (!requestedModel || !actualModel) return null;

  const requestedReasoningEffort = normalizeModelRouteReasoningEffort(
    setting.requestedReasoningEffort
  );
  const actualReasoningEffort = normalizeModelRouteReasoningEffort(setting.actualReasoningEffort);
  const modelMismatch =
    parsedSettingNullableBoolean(setting.modelMismatch) ??
    !sameRouteText(requestedModel, actualModel);
  const inferredEffortMismatch =
    requestedReasoningEffort !== "unknown" &&
    actualReasoningEffort !== "unknown" &&
    requestedReasoningEffort !== actualReasoningEffort;
  const effortMismatch =
    parsedSettingNullableBoolean(setting.effortMismatch) ?? inferredEffortMismatch;
  const mismatch =
    parsedSettingNullableBoolean(setting.mismatch) ?? (modelMismatch || effortMismatch);

  if (!mismatch && !modelMismatch && !effortMismatch) return null;

  return {
    cliKey: normalizeRouteText(setting.cliKey) ?? "",
    requestedModel,
    requestedReasoningEffort,
    requestedReasoningEffortSource: normalizeModelRouteReasoningEffortSource(
      setting.requestedReasoningEffortSource
    ),
    actualModel,
    actualReasoningEffort,
    actualReasoningEffortSource: normalizeModelRouteReasoningEffortSource(
      setting.actualReasoningEffortSource
    ),
    modelMismatch,
    effortMismatch,
    mismatch: true,
    providerId: normalizeRouteNumber(setting.providerId),
    providerName: normalizeRouteText(setting.providerName),
  };
}

export function resolveModelRouteMappingFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): ModelRouteMapping | null {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const mappings = settings
    .map(normalizeModelRouteMappingSetting)
    .filter((mapping): mapping is ModelRouteMapping => mapping !== null);

  if (mappings.length === 0) return null;

  if (finalProviderId != null) {
    const finalProviderMapping = mappings
      .slice()
      .reverse()
      .find((mapping) => mapping.providerId === finalProviderId);
    if (finalProviderMapping) return finalProviderMapping;

    if (mappings.some((mapping) => mapping.providerId != null)) {
      return null;
    }
  }

  return mappings[mappings.length - 1] ?? null;
}

export function hasModelRouteMappingSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  return resolveModelRouteMappingFromSpecialSettings(specialSettingsJson) !== null;
}

function hasValidSpecialSettingsJson(value: string | null | undefined): boolean {
  return parseRequestLogSpecialSettings(value).length > 0;
}

export function chooseModelRouteAwareSpecialSettingsJson(
  preferredSettings: string | null | undefined,
  fallbackSettings: string | null | undefined
): string | null {
  const preferredHasRoute = hasModelRouteMappingSpecialSetting(preferredSettings);
  const fallbackHasRoute = hasModelRouteMappingSpecialSetting(fallbackSettings);
  if (preferredHasRoute) return preferredSettings ?? null;
  if (fallbackHasRoute) return fallbackSettings ?? null;

  if (hasValidSpecialSettingsJson(preferredSettings)) return preferredSettings ?? null;
  if (hasValidSpecialSettingsJson(fallbackSettings)) return fallbackSettings ?? null;

  return preferredSettings ?? fallbackSettings ?? null;
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

export function hasCodexSystemRequestSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  return parseRequestLogSpecialSettings(specialSettingsJson).some(
    (setting) =>
      setting.type === CODEX_SYSTEM_REQUEST_SPECIAL_SETTING.type &&
      setting.threadSource === CODEX_SYSTEM_REQUEST_SPECIAL_SETTING.threadSource
  );
}
