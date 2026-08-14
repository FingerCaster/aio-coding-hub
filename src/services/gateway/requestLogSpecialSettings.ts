import { normalizeClaudeModelMapping, type ClaudeModelMapping } from "./claudeModelMapping";

export type ParsedRequestLogSpecialSetting = {
  type?: string;
  reason?: string;
} & Record<string, unknown>;

export type UpstreamErrorResponseRuleMarker = {
  type: "upstream_error_response_rule";
  ruleId: string;
  ruleName: string;
  providerId: number;
  providerName: string;
  upstreamStatus: number;
  clientStatus: number;
  statusMode: "passthrough" | "override";
  messageMode: "passthrough" | "override";
};
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
  | "configured_route"
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

export type AioManagedModelRoute = {
  canonicalModel: string;
  providerId: number;
  providerUuid: string | null;
  remoteModelId: string;
  requestedUpstreamModel: string;
  pricedModel: string | null;
  applied: true;
};

export type ConfiguredModelRoute = {
  providerId: number;
  providerName: string | null;
  policySource: "global" | "provider";
  sourceModel: string;
  targetModel: string | null;
  effectiveModel: string;
  reasoningEffort: string | null;
  pricedCliKey: string | null;
  modelApplied: boolean;
  reasoningEffortApplied: boolean;
  applied: true;
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

function normalizeUpstreamErrorResponseRuleMarker(
  setting: ParsedRequestLogSpecialSetting
): UpstreamErrorResponseRuleMarker | null {
  if (setting.type !== "upstream_error_response_rule") return null;
  const ruleId = parsedSettingString(setting.ruleId).trim();
  const ruleName = parsedSettingString(setting.ruleName).trim();
  const providerId = parsedSettingNumber(setting.providerId);
  const providerName = parsedSettingString(setting.providerName).trim();
  const upstreamStatus = parsedSettingNumber(setting.upstreamStatus);
  const clientStatus = parsedSettingNumber(setting.clientStatus);
  const statusMode = parsedSettingString(setting.statusMode);
  const messageMode = parsedSettingString(setting.messageMode);

  if (
    !ruleId ||
    !ruleName ||
    !Number.isSafeInteger(providerId) ||
    providerId <= 0 ||
    !providerName ||
    !Number.isSafeInteger(upstreamStatus) ||
    upstreamStatus < 400 ||
    upstreamStatus > 599 ||
    !Number.isSafeInteger(clientStatus) ||
    clientStatus < 400 ||
    clientStatus > 599 ||
    (statusMode !== "passthrough" && statusMode !== "override") ||
    (messageMode !== "passthrough" && messageMode !== "override")
  ) {
    return null;
  }

  return {
    type: "upstream_error_response_rule",
    ruleId,
    ruleName,
    providerId,
    providerName,
    upstreamStatus,
    clientStatus,
    statusMode,
    messageMode,
  };
}

export function resolveUpstreamErrorResponseRuleMarker(
  specialSettingsJson: string | null | undefined
): UpstreamErrorResponseRuleMarker | null {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  for (let index = settings.length - 1; index >= 0; index -= 1) {
    const marker = normalizeUpstreamErrorResponseRuleMarker(settings[index]!);
    if (marker) return marker;
  }
  return null;
}

export function formatUpstreamErrorResponseRuleTooltip(
  marker: UpstreamErrorResponseRuleMarker
): string {
  return [
    `响应规则：${marker.ruleName}`,
    `供应商：${marker.providerName} (#${String(marker.providerId)})`,
    `状态码：${String(marker.upstreamStatus)} → ${String(marker.clientStatus)}`,
    `状态行为：${marker.statusMode === "override" ? "自定义" : "透传"}`,
    `信息行为：${marker.messageMode === "override" ? "自定义" : "提取并透传"}`,
  ].join("\n");
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
  if (source === "configured_route") return "configured_route";
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

export function resolveRequestLogReasoningEffort({
  observedReasoningEffort,
  cliKey,
  requestedModel,
  specialSettingsJson,
}: {
  observedReasoningEffort: string | null | undefined;
  cliKey: string | null | undefined;
  requestedModel: string | null | undefined;
  specialSettingsJson: string | null | undefined;
}): string | null {
  const observed = observedReasoningEffort?.trim();
  if (observed) return observed;
  if (cliKey?.trim().toLowerCase() !== "codex") return null;

  const fallback = resolveCodexReasoningEffort(requestedModel, specialSettingsJson).effort;
  return fallback === "unknown" ? null : fallback;
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
  if (source === "configured_route") return "配置路由";
  if (source === "response") return "返回显式";
  return "未知";
}

function normalizeRouteText(value: unknown): string | null {
  const text = parsedSettingString(value).trim();
  return text ? text : null;
}

function normalizeConfiguredRouteText(
  value: unknown,
  options: { maxBytes: number; maxChars?: number }
): string | null {
  const text = normalizeRouteText(value);
  if (!text) return null;
  if (options.maxChars != null && [...text].length > options.maxChars) return null;
  if (new TextEncoder().encode(text).byteLength > options.maxBytes) return null;
  if (
    [...text].some((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
    })
  ) {
    return null;
  }
  return text;
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

function normalizeConfiguredModelRouteSetting(
  setting: ParsedRequestLogSpecialSetting
): ConfiguredModelRoute | null {
  if (setting.type !== "configured_model_route" || setting.applied !== true) return null;

  const providerId = normalizeRouteNumber(setting.providerId);
  const sourceModel = normalizeConfiguredRouteText(setting.sourceModel, { maxBytes: 256 });
  const targetModel = normalizeConfiguredRouteText(setting.targetModel, { maxBytes: 256 });
  const effectiveModel = normalizeConfiguredRouteText(setting.effectiveModel, { maxBytes: 256 });
  const reasoningEffort = normalizeConfiguredRouteText(setting.reasoningEffort, {
    maxBytes: 256,
    maxChars: 64,
  });
  const policySource = setting.policySource;
  const modelApplied = setting.modelApplied === true;
  const reasoningEffortApplied = setting.reasoningEffortApplied === true;

  if (
    providerId == null ||
    !Number.isSafeInteger(providerId) ||
    providerId <= 0 ||
    !sourceModel ||
    !effectiveModel ||
    (policySource !== "global" && policySource !== "provider") ||
    (!modelApplied && !reasoningEffortApplied) ||
    (modelApplied && (!targetModel || targetModel !== effectiveModel)) ||
    (!modelApplied && targetModel != null) ||
    (reasoningEffortApplied && !reasoningEffort)
  ) {
    return null;
  }

  return {
    providerId,
    providerName: normalizeConfiguredRouteText(setting.providerName, {
      maxBytes: 256,
      maxChars: 128,
    }),
    policySource,
    sourceModel,
    targetModel,
    effectiveModel,
    reasoningEffort,
    pricedCliKey: normalizeConfiguredRouteText(setting.pricedCliKey, {
      maxBytes: 32,
      maxChars: 32,
    }),
    modelApplied,
    reasoningEffortApplied,
    applied: true,
  };
}

export function resolveConfiguredModelRouteFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): ConfiguredModelRoute | null {
  const routes = parseRequestLogSpecialSettings(specialSettingsJson)
    .map(normalizeConfiguredModelRouteSetting)
    .filter((route): route is ConfiguredModelRoute => route !== null);
  if (routes.length === 0) return null;

  if (finalProviderId != null) {
    return (
      routes
        .slice()
        .reverse()
        .find((route) => route.providerId === finalProviderId) ?? null
    );
  }

  return routes[routes.length - 1] ?? null;
}

export function hasConfiguredModelRouteSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  return resolveConfiguredModelRouteFromSpecialSettings(specialSettingsJson) !== null;
}

function normalizeAioManagedModelRouteSetting(
  setting: ParsedRequestLogSpecialSetting
): AioManagedModelRoute | null {
  if (setting.type !== "aio_managed_model_route" || setting.applied !== true) return null;

  const canonicalModel = normalizeRouteText(setting.canonicalModel);
  const providerId = normalizeRouteNumber(setting.providerId);
  const remoteModelId = normalizeRouteText(setting.remoteModelId);
  if (!canonicalModel || providerId == null || providerId <= 0 || !remoteModelId) return null;

  const requestedUpstreamModel =
    normalizeRouteText(setting.requestedUpstreamModel) ??
    normalizeRouteText(setting.wireModel) ??
    remoteModelId;

  return {
    canonicalModel,
    providerId,
    providerUuid: normalizeRouteText(setting.providerUuid),
    remoteModelId,
    requestedUpstreamModel,
    pricedModel: normalizeRouteText(setting.pricedModel),
    applied: true,
  };
}

export function resolveAioManagedModelRouteFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): AioManagedModelRoute | null {
  const routes = parseRequestLogSpecialSettings(specialSettingsJson)
    .map(normalizeAioManagedModelRouteSetting)
    .filter((route): route is AioManagedModelRoute => route !== null);
  if (routes.length === 0) return null;

  if (finalProviderId != null) {
    return (
      routes
        .slice()
        .reverse()
        .find((route) => route.providerId === finalProviderId) ?? null
    );
  }

  return routes[routes.length - 1] ?? null;
}

function hasValidSpecialSettingsJson(value: string | null | undefined): boolean {
  return parseRequestLogSpecialSettings(value).length > 0;
}

export function chooseModelRouteAwareSpecialSettingsJson(
  preferredSettings: string | null | undefined,
  fallbackSettings: string | null | undefined
): string | null {
  if (hasConfiguredModelRouteSpecialSetting(preferredSettings)) return preferredSettings ?? null;
  if (hasConfiguredModelRouteSpecialSetting(fallbackSettings)) return fallbackSettings ?? null;

  if (hasModelRouteMappingSpecialSetting(preferredSettings)) return preferredSettings ?? null;
  if (hasModelRouteMappingSpecialSetting(fallbackSettings)) return fallbackSettings ?? null;

  if (resolveAioManagedModelRouteFromSpecialSettings(preferredSettings) !== null) {
    return preferredSettings ?? null;
  }
  if (resolveAioManagedModelRouteFromSpecialSettings(fallbackSettings) !== null) {
    return fallbackSettings ?? null;
  }

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
