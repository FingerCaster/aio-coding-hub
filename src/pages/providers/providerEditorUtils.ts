import { FREE_TAG } from "../../constants/providers";
import type { ProviderEditorDialogFormInput } from "../../schemas/providerEditorDialog";
import {
  getProviderTypeInfo,
  type ClaudeModels,
  type CliKey,
  type ProviderSummary,
} from "../../services/providers/providers";
import { cliLongLabel } from "../../constants/clis";
import type { ProviderEditorInitialValues } from "./providerDuplicate";
import type { BaseUrlRow } from "./types";

export type DailyResetMode = "fixed" | "rolling";

export const DEFAULT_FORM_VALUES: ProviderEditorDialogFormInput = {
  name: "",
  api_key: "",
  auth_mode: "api_key",
  cost_multiplier: "1.0",
  limit_5h_usd: "",
  limit_daily_usd: "",
  limit_weekly_usd: "",
  limit_monthly_usd: "",
  limit_total_usd: "",
  daily_reset_mode: "fixed",
  daily_reset_time: "00:00:00",
  enabled: true,
  note: "",
};

export const CX2CC_GLOBAL_SOURCE_VALUE = "__codex_gateway__";
export const CX2CC_PROXY_TOKEN = "aio-coding-hub";
export const CX2CC_DEFAULT_MODEL = "gpt-5.5";
export const CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE = "codex_to_openai_chat";
export const CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE = "codex_to_openai_responses";
export const CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE = "codex_to_anthropic_messages";
const CX2CC_MODEL_MAPPING_KEYS = [
  "main_model",
  "reasoning_model",
  "haiku_model",
  "sonnet_model",
  "opus_model",
] as const;

export type CodexBridgeTarget = "openai_chat" | "openai_responses";

export function cliNameFromKey(cliKey: CliKey) {
  return cliLongLabel(cliKey);
}

export function valueOrEmpty(value: number | null | undefined) {
  return value != null ? String(value) : "";
}

export function isZeroMultiplier(value: string | null | undefined) {
  if (!value?.trim()) return false;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed === 0;
}

export function isNonZeroMultiplier(value: string | null | undefined) {
  if (!value?.trim()) return false;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed !== 0;
}

export function moveFreeTagToFront(tags: string[]) {
  const withoutFreeTag = tags.filter((tag) => tag !== FREE_TAG);
  return [FREE_TAG, ...withoutFreeTag];
}

export function areTagsEqual(left: string[], right: string[]) {
  if (left.length !== right.length) return false;
  return left.every((tag, index) => tag === right[index]);
}

export function normalizeTagsForCostMultiplier(tags: string[], costMultiplierValue: string) {
  const hasFreeTag = tags.includes(FREE_TAG);

  if (isZeroMultiplier(costMultiplierValue)) {
    const next = hasFreeTag ? moveFreeTagToFront(tags) : [FREE_TAG, ...tags];
    return areTagsEqual(tags, next) ? tags : next;
  }

  if (isNonZeroMultiplier(costMultiplierValue) && hasFreeTag) {
    return tags.filter((tag) => tag !== FREE_TAG);
  }

  return tags;
}

export function normalizeCx2ccModelName(value: string | null | undefined) {
  const trimmed = value?.trim();
  return trimmed || null;
}

export function withCx2ccDefaultModel(
  value: ClaudeModels,
  defaultModel = CX2CC_DEFAULT_MODEL
): ClaudeModels {
  return {
    ...value,
    main_model: normalizeCx2ccModelName(value.main_model) ?? defaultModel,
    reasoning_model: normalizeCx2ccModelName(value.reasoning_model) ?? defaultModel,
    haiku_model: normalizeCx2ccModelName(value.haiku_model) ?? defaultModel,
    sonnet_model: normalizeCx2ccModelName(value.sonnet_model) ?? defaultModel,
    opus_model: normalizeCx2ccModelName(value.opus_model) ?? defaultModel,
  };
}

export function resolveCx2ccDefaultModelSelectValue(value: ClaudeModels) {
  const normalizedValues = CX2CC_MODEL_MAPPING_KEYS.map((key) =>
    normalizeCx2ccModelName(value[key])
  );
  const configuredValues = normalizedValues.filter((model): model is string => model != null);
  if (configuredValues.length === 0) return CX2CC_DEFAULT_MODEL;

  const firstModel = normalizedValues[0];
  if (firstModel && normalizedValues.every((model) => model === firstModel)) {
    return firstModel;
  }

  return "__manual__";
}

export function tagBadgeClassName(tag: string) {
  if (tag === FREE_TAG) {
    return "inline-flex items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300";
  }
  return "inline-flex items-center gap-1 rounded-full bg-accent/10 px-2 py-0.5 text-xs font-medium text-accent";
}

export function tagRemoveButtonClassName(tag: string) {
  if (tag === FREE_TAG) {
    return "inline-flex h-3.5 w-3.5 items-center justify-center rounded-full hover:bg-emerald-200/70 dark:hover:bg-emerald-800/60";
  }
  return "inline-flex h-3.5 w-3.5 items-center justify-center rounded-full hover:bg-accent/20";
}

export function buildFormValues(initialValues: ProviderEditorInitialValues | null) {
  if (!initialValues) {
    return { ...DEFAULT_FORM_VALUES };
  }

  return {
    name: initialValues.name,
    api_key: initialValues.api_key,
    auth_mode: initialValues.auth_mode,
    cost_multiplier: String(initialValues.cost_multiplier),
    limit_5h_usd: valueOrEmpty(initialValues.limit_5h_usd),
    limit_daily_usd: valueOrEmpty(initialValues.limit_daily_usd),
    limit_weekly_usd: valueOrEmpty(initialValues.limit_weekly_usd),
    limit_monthly_usd: valueOrEmpty(initialValues.limit_monthly_usd),
    limit_total_usd: valueOrEmpty(initialValues.limit_total_usd),
    daily_reset_mode: initialValues.daily_reset_mode,
    daily_reset_time: initialValues.daily_reset_time,
    enabled: initialValues.enabled,
    note: initialValues.note,
  };
}

export function buildBaseUrlRows(
  initialValues: ProviderEditorInitialValues | null,
  newBaseUrlRow: (url?: string) => BaseUrlRow
) {
  const baseUrls = initialValues?.base_urls ?? [];
  if (baseUrls.length > 0) {
    return baseUrls.map((url) => newBaseUrlRow(url));
  }
  if (initialValues?.auth_mode === "oauth") {
    return [] as BaseUrlRow[];
  }
  return [newBaseUrlRow()];
}

export function deriveAuthMode(
  provider: ProviderSummary | null | undefined
): "api_key" | "oauth" | "cx2cc" {
  return getProviderTypeInfo(provider).effectiveAuthMode;
}

export function deriveCx2ccSourceValue(
  source:
    | Pick<ProviderSummary, "source_provider_id" | "bridge_type">
    | Pick<ProviderEditorInitialValues, "source_provider_id" | "bridge_type">
    | null
    | undefined
) {
  if (!source) return "";
  if (source.source_provider_id != null) return String(source.source_provider_id);
  if (source.bridge_type === "cx2cc") return CX2CC_GLOBAL_SOURCE_VALUE;
  return "";
}

export function deriveCodexBridgeTarget(
  provider: Pick<ProviderSummary, "bridge_type"> | null | undefined
): CodexBridgeTarget {
  if (provider?.bridge_type === CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE) {
    return "openai_responses";
  }
  if (provider?.bridge_type === CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE) {
    return "openai_responses";
  }
  return "openai_chat";
}
