import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  cliManagerCodexConfigTomlValidate,
  type CodexConfigPatch,
  type CodexConfigState,
  type CodexConfigTomlState,
  type CodexConfigTomlValidationResult,
  type SimpleCliInfo,
} from "../../../services/cli/cliManager";
import type {
  AppSettings,
  CodexHomeMode,
  CodexReasoningGuardExhaustedAction,
  CodexReasoningGuardPostMatchStrategy,
  CodexReasoningGuardRetryPolicy,
  CodexReasoningGuardTemplateFilter,
  CodexReasoningGuardTemplateFilterField,
  CodexReasoningGuardTemplateFilterOperator,
  CodexReasoningGuardTemplateRule,
  CodexReasoningGuardRuleTemplate,
  CodexReasoningGuardRuleMode,
} from "../../../services/settings/settings";
import {
  CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID,
  CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
  CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID,
  DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS,
  DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX,
  DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS,
  DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS,
  DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET,
  DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS,
  DEFAULT_CODEX_REASONING_GUARD_EXHAUSTED_ACTION,
  DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET,
  DEFAULT_CODEX_REASONING_GUARD_POST_MATCH_STRATEGY,
  DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS,
  DEFAULT_CODEX_REASONING_GUARD_RETRY_POLICY,
  DEFAULT_CODEX_REASONING_GUARD_RULE_MODE,
  MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES,
  MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE,
  MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS,
  MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES,
  MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS,
  MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX,
  MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS,
  MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS,
  MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET,
  MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS,
  MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET,
  MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN,
  MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN,
  validateSettingsSetInput,
} from "../../../services/settings/settingsValidation";
import { normalizeCustomCodexHome, buildConfigTomlPath } from "../../../utils/codexPaths";
import {
  unixSecondsAtLocalStartOfDay,
  unixSecondsAtLocalStartOfNextDay,
} from "../../../utils/localDate";
import { isWindowsRuntime } from "../../../utils/platform";
import { cn } from "../../../utils/cn";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { useCliManagerCodexReasoningGuardStatsQuery } from "../../../query/cliManager";
import { CliVersionBadge } from "../CliVersionBadge";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { Switch } from "../../../ui/Switch";
import { RadioGroup } from "../../../ui/RadioGroup";
import { Textarea } from "../../../ui/Textarea";
import { Popover } from "../../../ui/Popover";
import {
  AlertTriangle,
  BarChart3,
  Calendar,
  CheckCircle2,
  ChevronDown,
  ExternalLink,
  FileJson,
  FolderOpen,
  Plus,
  RefreshCw,
  Trash2,
  Terminal,
  Settings,
} from "lucide-react";

const LazyCodeEditor = lazy(() =>
  import("../../../ui/CodeEditor").then((m) => ({ default: m.CodeEditor }))
);

const GPT_54_MODEL = "gpt-5.4";
const DEFAULT_CODEX_PROVIDER_TEST_MODEL = "gpt-5.4-mini";
const GPT_54_CONTEXT_WINDOW = 1_000_000;
const GPT_54_AUTO_COMPACT_TOKEN_LIMIT = 900_000;
const FAST_SERVICE_TIER = "fast";
type PersistConfigLocationResult = "saved" | "validation_failed" | "persist_failed";
type CodexReasoningGuardDetailsTab = "rules" | "stats";
type CodexReasoningGuardStatsPreset =
  | "today"
  | "yesterday"
  | "last24h"
  | "last7"
  | "last14"
  | "last30"
  | "thisMonth"
  | "lastMonth";
type CodexReasoningGuardStatsDateRange = {
  startDate: string;
  endDate: string;
};
type CodexReasoningGuardStatsRangePopoverScope = "overview" | "details";
type CodexReasoningGuardTemplateOption = CodexReasoningGuardRuleTemplate & {
  source: "builtin" | "custom";
  readOnly: boolean;
};

const CODEX_REASONING_GUARD_BUILTIN_TEMPLATES: CodexReasoningGuardTemplateOption[] = [
  {
    id: CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID,
    name: "Reasoning tokens 518*N-2",
    description: "拦截 reasoning_tokens 满足 518*N-2 的 Codex 截断续写特征。",
    source: "builtin",
    readOnly: true,
    rules: [
      {
        id: "builtin-518n-minus-2",
        name: "reasoning_tokens == 518*N-2",
        reasoning_tokens: null,
        reasoning_tokens_formula: "reasoning_tokens_518n_minus_2",
        action: "intercept",
        logic: "and",
        filters: [],
      },
    ],
  },
  {
    id: CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
    name: "Legacy reasoning tokens",
    description: "拦截 reasoning_tokens 等于 516、1034、1552 的旧默认规则。",
    source: "builtin",
    readOnly: true,
    rules: DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.map((value) => ({
      id: `builtin-token-${value}`,
      name: `reasoning_tokens == ${value}`,
      reasoning_tokens: value,
      reasoning_tokens_formula: null,
      action: "intercept",
      logic: "and",
      filters: [],
    })),
  },
  {
    id: CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID,
    name: "Final answer only high/xhigh",
    description:
      "请求 reasoning effort 为 high/xhigh 且响应只有 final answer 时命中；仅 reasoning_tokens 为 0 的 context_compaction 豁免。",
    source: "builtin",
    readOnly: true,
    rules: [
      {
        id: "builtin-reasoning-zero-allow",
        name: "reasoning_tokens == 0 allow",
        reasoning_tokens: 0,
        reasoning_tokens_formula: null,
        action: "no_intercept",
        logic: "and",
        filters: [],
      },
      {
        id: "builtin-final-answer-only-high-xhigh",
        name: "final answer only high/xhigh",
        reasoning_tokens: null,
        reasoning_tokens_formula: null,
        action: "intercept",
        logic: "and",
        filters: [
          {
            id: "request-reasoning-effort-high-xhigh",
            field: "request_reasoning_effort",
            operator: "in",
            number_value: null,
            bool_value: null,
            string_value: null,
            string_values: ["high", "xhigh"],
          },
          {
            id: "final-answer-only",
            field: "final_answer_only",
            operator: "equals",
            number_value: null,
            bool_value: true,
            string_value: null,
            string_values: [],
          },
          {
            id: "no-commentary",
            field: "commentary_observed",
            operator: "not_equals",
            number_value: null,
            bool_value: true,
            string_value: null,
            string_values: [],
          },
          {
            id: "no-tool-call",
            field: "has_tool_call",
            operator: "not_equals",
            number_value: null,
            bool_value: true,
            string_value: null,
            string_values: [],
          },
          {
            id: "no-reasoning-item",
            field: "has_reasoning_item",
            operator: "not_equals",
            number_value: null,
            bool_value: true,
            string_value: null,
            string_values: [],
          },
        ],
      },
    ],
  },
];

const CODEX_REASONING_GUARD_PERCENT_FORMATTER = new Intl.NumberFormat("zh-CN", {
  style: "percent",
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});
const CODEX_REASONING_GUARD_DECIMAL_FORMATTER = new Intl.NumberFormat("zh-CN", {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});

function formatCodexReasoningGuardHitRate(value: number | null | undefined) {
  return CODEX_REASONING_GUARD_PERCENT_FORMATTER.format(value ?? 0);
}

function formatCodexReasoningGuardDecimal(value: number | null | undefined) {
  return CODEX_REASONING_GUARD_DECIMAL_FORMATTER.format(value ?? 0);
}

function formatCodexReasoningContinuationStatusLabel(status: string | null | undefined) {
  switch ((status ?? "").trim()) {
    case "continuation_repaired":
    case "repaired":
      return "已修复";
    case "missing_encrypted":
      return "缺少 encrypted";
    case "capped_max_output_tokens":
      return "达到 output 上限";
    case "still_matched":
      return "仍命中";
    case "failed":
      return "补救失败";
    case "unsupported":
      return "不支持";
    case "unavailable":
      return "不可用";
    case "unknown":
      return "未知状态";
    default:
      return status ? status : "未知状态";
  }
}

function formatCodexReasoningGuardRuleModeLabel(mode: CodexReasoningGuardRuleMode) {
  switch (mode) {
    case "final_answer_only_high_xhigh":
      return "final answer only";
    case "reasoning_tokens":
    default:
      return "reasoning tokens";
  }
}

function formatCodexReasoningGuardExhaustedActionLabel(action: CodexReasoningGuardExhaustedAction) {
  switch (action) {
    case "switch_provider":
      return "切换供应商";
    case "switch_model":
      return "切换模型";
    case "return_error":
    default:
      return "返回错误";
  }
}

function formatCodexReasoningGuardRetryPolicyLabel(policy: CodexReasoningGuardRetryPolicy) {
  return policy === "concurrent" ? "并发重试" : "单路重试";
}

function formatCodexReasoningGuardPostMatchStrategyLabel(
  strategy: CodexReasoningGuardPostMatchStrategy
) {
  return strategy === "continuation_repair" ? "思考续写" : "自动重试";
}

function formatCodexReasoningGuardModelFallbacks(models: string[] | null | undefined) {
  return (models ?? []).join("\n");
}

const CODEX_REASONING_GUARD_FILTER_FIELD_OPTIONS: Array<{
  value: CodexReasoningGuardTemplateFilterField;
  label: string;
  kind: "number" | "boolean" | "string";
}> = [
  { value: "duration_ms", label: "duration_ms", kind: "number" },
  { value: "tps", label: "tps", kind: "number" },
  { value: "output_tokens", label: "output_tokens", kind: "number" },
  { value: "input_tokens", label: "input_tokens", kind: "number" },
  { value: "total_tokens", label: "total_tokens", kind: "number" },
  { value: "reasoning_tokens", label: "reasoning_tokens", kind: "number" },
  { value: "final_answer_only", label: "final_answer_only", kind: "boolean" },
  { value: "has_tool_call", label: "has_tool_call", kind: "boolean" },
  { value: "has_reasoning_item", label: "has_reasoning_item", kind: "boolean" },
  { value: "commentary_observed", label: "commentary_observed", kind: "boolean" },
  { value: "request_reasoning_effort", label: "request_reasoning_effort", kind: "string" },
  { value: "requested_model", label: "requested_model", kind: "string" },
];

const CODEX_REASONING_GUARD_NUMBER_OPERATORS: CodexReasoningGuardTemplateFilterOperator[] = [
  "equals",
  "not_equals",
  "less_than",
  "less_than_or_equal",
  "greater_than",
  "greater_than_or_equal",
];
const CODEX_REASONING_GUARD_BOOLEAN_OPERATORS: CodexReasoningGuardTemplateFilterOperator[] = [
  "equals",
  "not_equals",
];
const CODEX_REASONING_GUARD_STRING_OPERATORS: CodexReasoningGuardTemplateFilterOperator[] = [
  "equals",
  "not_equals",
  "in",
  "not_in",
];

function codexReasoningGuardFilterKind(field: CodexReasoningGuardTemplateFilterField) {
  return (
    CODEX_REASONING_GUARD_FILTER_FIELD_OPTIONS.find((option) => option.value === field)?.kind ??
    "number"
  );
}

function codexReasoningGuardFilterOperators(field: CodexReasoningGuardTemplateFilterField) {
  switch (codexReasoningGuardFilterKind(field)) {
    case "boolean":
      return CODEX_REASONING_GUARD_BOOLEAN_OPERATORS;
    case "string":
      return CODEX_REASONING_GUARD_STRING_OPERATORS;
    case "number":
    default:
      return CODEX_REASONING_GUARD_NUMBER_OPERATORS;
  }
}

function normalizeCodexReasoningGuardFilterForField(
  filter: CodexReasoningGuardTemplateFilter,
  field: CodexReasoningGuardTemplateFilterField
): CodexReasoningGuardTemplateFilter {
  const kind = codexReasoningGuardFilterKind(field);
  const operators = codexReasoningGuardFilterOperators(field);
  const operator = operators.includes(filter.operator) ? filter.operator : operators[0];
  return {
    id: filter.id,
    field,
    operator,
    number_value: kind === "number" ? (filter.number_value ?? 0) : null,
    bool_value: kind === "boolean" ? (filter.bool_value ?? true) : null,
    string_value:
      kind === "string" && (operator === "equals" || operator === "not_equals")
        ? (filter.string_value ?? "")
        : null,
    string_values:
      kind === "string" && (operator === "in" || operator === "not_in") ? filter.string_values : [],
  };
}

function buildCodexReasoningGuardFilter(id: string): CodexReasoningGuardTemplateFilter {
  return {
    id,
    field: "reasoning_tokens",
    operator: "less_than_or_equal",
    number_value: 516,
    bool_value: null,
    string_value: null,
    string_values: [],
  };
}

function buildCodexReasoningGuardRule(id: string): CodexReasoningGuardTemplateRule {
  return {
    id,
    name: "New rule",
    reasoning_tokens: null,
    reasoning_tokens_formula: null,
    action: "intercept",
    logic: "and",
    filters: [buildCodexReasoningGuardFilter(`${id}-filter-1`)],
  };
}

function resolveCodexReasoningGuardTemplateOption(
  id: string,
  customTemplates: CodexReasoningGuardRuleTemplate[]
): CodexReasoningGuardTemplateOption | null {
  const builtin = CODEX_REASONING_GUARD_BUILTIN_TEMPLATES.find((template) => template.id === id);
  if (builtin) return builtin;
  const custom = customTemplates.find((template) => template.id === id);
  return custom ? { ...custom, source: "custom", readOnly: false } : null;
}

function uniqueCodexReasoningGuardTemplateId(
  templates: CodexReasoningGuardRuleTemplate[],
  preferredId: string
) {
  const builtinIds = new Set(
    CODEX_REASONING_GUARD_BUILTIN_TEMPLATES.map((template) => template.id)
  );
  const existingIds = new Set([...builtinIds, ...templates.map((template) => template.id.trim())]);
  const base =
    preferredId
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]+/gu, "-")
      .replace(/^-+|-+$/gu, "")
      .slice(0, 72) || "custom-template";
  if (!existingIds.has(base)) return base;
  for (let index = 2; index < 100; index += 1) {
    const candidate = `${base}-${index}`;
    if (!existingIds.has(candidate)) return candidate;
  }
  return `${base}-${templates.length + 1}`;
}

function buildNewCodexReasoningGuardTemplate(
  customTemplates: CodexReasoningGuardRuleTemplate[]
): CodexReasoningGuardRuleTemplate {
  return {
    id: uniqueCodexReasoningGuardTemplateId(customTemplates, "custom-reasoning-guard"),
    name: "Custom reasoning guard",
    description: "自定义 Codex 降智拦截规则模板。",
    rules: [
      {
        id: "token-516",
        name: "reasoning_tokens == 516",
        reasoning_tokens: 516,
        reasoning_tokens_formula: null,
        action: "intercept",
        logic: "and",
        filters: [],
      },
    ],
  };
}

function copyCodexReasoningGuardTemplateAsCustom(
  template: CodexReasoningGuardRuleTemplate,
  customTemplates: CodexReasoningGuardRuleTemplate[]
): CodexReasoningGuardRuleTemplate {
  return {
    rules: structuredClone(template.rules),
    id: uniqueCodexReasoningGuardTemplateId(customTemplates, `custom-${template.id}`),
    name: `${template.name} copy`,
    description: template.description || "Copied from an existing Codex reasoning guard template.",
  };
}

function formatLocalDateInputValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function buildCodexReasoningGuardStatsRange(startDate: string, endDate: string) {
  const startTs = unixSecondsAtLocalStartOfDay(startDate);
  const endTs = unixSecondsAtLocalStartOfNextDay(endDate);
  if (startTs == null || endTs == null) {
    return null;
  }
  if (startTs >= endTs) {
    return null;
  }
  return {
    startCreatedAtMs: startTs * 1000,
    endCreatedAtMs: endTs * 1000,
  };
}

function formatCodexReasoningGuardStatsDateRangeLabel(
  dateRange: CodexReasoningGuardStatsDateRange
) {
  if (dateRange.startDate === dateRange.endDate) {
    return `${dateRange.startDate} 当天`;
  }
  return `${dateRange.startDate} 至 ${dateRange.endDate}`;
}

function addLocalDays(date: Date, days: number) {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

function startOfLocalMonth(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

function endOfLocalMonth(date: Date) {
  return new Date(date.getFullYear(), date.getMonth() + 1, 0);
}

function buildCodexReasoningGuardStatsPresetRange(
  preset: CodexReasoningGuardStatsPreset,
  now = new Date()
): CodexReasoningGuardStatsDateRange {
  switch (preset) {
    case "yesterday": {
      const yesterday = formatLocalDateInputValue(addLocalDays(now, -1));
      return { startDate: yesterday, endDate: yesterday };
    }
    case "last24h":
      return {
        startDate: formatLocalDateInputValue(addLocalDays(now, -1)),
        endDate: formatLocalDateInputValue(now),
      };
    case "last7":
      return {
        startDate: formatLocalDateInputValue(addLocalDays(now, -6)),
        endDate: formatLocalDateInputValue(now),
      };
    case "last14":
      return {
        startDate: formatLocalDateInputValue(addLocalDays(now, -13)),
        endDate: formatLocalDateInputValue(now),
      };
    case "last30":
      return {
        startDate: formatLocalDateInputValue(addLocalDays(now, -29)),
        endDate: formatLocalDateInputValue(now),
      };
    case "thisMonth":
      return {
        startDate: formatLocalDateInputValue(startOfLocalMonth(now)),
        endDate: formatLocalDateInputValue(now),
      };
    case "lastMonth": {
      const lastMonth = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      return {
        startDate: formatLocalDateInputValue(startOfLocalMonth(lastMonth)),
        endDate: formatLocalDateInputValue(endOfLocalMonth(lastMonth)),
      };
    }
    case "today":
    default: {
      const today = formatLocalDateInputValue(now);
      return { startDate: today, endDate: today };
    }
  }
}

function buildModelPatch(
  model: string,
  contextWindow?: string,
  autoCompactLimit?: string
): CodexConfigPatch {
  const trimmed = model.trim();
  const isGpt54 = trimmed === GPT_54_MODEL;

  return {
    model: trimmed,
    model_context_window: isGpt54 ? parsePositiveInt(contextWindow) : null,
    model_auto_compact_token_limit: isGpt54 ? parsePositiveInt(autoCompactLimit) : null,
  };
}

/** Parse a string to a positive integer; return null on empty / NaN / <= 0. */
function parsePositiveInt(v: string | undefined): number | null {
  if (v == null) return null;
  const n = Number(v.trim());
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.round(n);
}

function buildFastModePatch(enabled: boolean): CodexConfigPatch {
  return {
    features_fast_mode: enabled,
    service_tier: enabled ? FAST_SERVICE_TIER : "",
  };
}

function buildPersonalityPatch(value: string): CodexConfigPatch {
  return {
    personality: value === "none" ? "" : value,
  };
}

function isGpt54Model(model: string | null | undefined) {
  return (model ?? "").trim() === GPT_54_MODEL;
}

function validateCustomCodexHome(value: string): string | null {
  const trimmed = value.trim();
  const normalized = normalizeCustomCodexHome(trimmed);
  if (!normalized) return "请输入 .codex 目录路径。";

  const lower = trimmed.replace(/\\/g, "/").toLowerCase();
  if (lower.includes("://")) {
    return "这里填写的是本地目录路径，不要包含协议头。";
  }
  if (/[\r\n\u0000]/.test(trimmed)) {
    return "路径中不能包含换行或控制字符。";
  }
  if (lower.endsWith(".toml") && lower !== "config.toml" && !lower.endsWith("/config.toml")) {
    return "这里填写的是 .codex 目录，不是其他 TOML 文件。";
  }

  return null;
}

function normalizeComparablePath(path: string) {
  return path
    .trim()
    .replace(/[\\/]+$/, "")
    .replace(/\\/g, "/")
    .toLowerCase();
}

export type CliManagerAvailability = "checking" | "available" | "unavailable";

export type CliManagerCodexTabProps = {
  codexAvailable: CliManagerAvailability;
  codexLoading: boolean;
  codexConfigLoading: boolean;
  codexConfigSaving: boolean;
  codexConfigTomlLoading: boolean;
  codexConfigTomlSaving: boolean;
  codexProviderSyncing?: boolean;
  codexInfo: SimpleCliInfo | null;
  codexConfig: CodexConfigState | null;
  codexConfigToml: CodexConfigTomlState | null;
  appSettings?: AppSettings | null;
  commonSettingsSaving?: boolean;
  codexHomeSettingsSaving?: boolean;
  refreshCodex: () => Promise<void> | void;
  openCodexConfigDir: () => Promise<void> | void;
  persistCodexConfig: (patch: CodexConfigPatch) => Promise<void> | void;
  persistCodexConfigToml: (toml: string) => Promise<boolean> | boolean;
  syncCodexProvider?: () => Promise<void> | void;
  persistCommonSettings?: (
    patch: Partial<AppSettings>
  ) => Promise<AppSettings | null> | AppSettings | null;
  persistCodexReasoningGuardSettings?: (
    patch: Partial<
      Pick<
        AppSettings,
        | "codex_reasoning_guard_enabled"
        | "codex_reasoning_guard_hit_label"
        | "codex_reasoning_guard_rule_mode"
        | "codex_reasoning_guard_compare_mode"
        | "codex_reasoning_guard_reasoning_equals"
        | "codex_reasoning_guard_model_rules"
        | "codex_reasoning_guard_active_template_id"
        | "codex_reasoning_guard_custom_templates"
        | "codex_reasoning_guard_post_match_strategy"
        | "codex_reasoning_guard_immediate_retry_budget"
        | "codex_reasoning_guard_delayed_retry_budget"
        | "codex_reasoning_guard_delayed_retry_ms"
        | "codex_reasoning_guard_exhausted_action"
        | "codex_reasoning_guard_retry_policy"
        | "codex_reasoning_guard_concurrent_max"
        | "codex_reasoning_guard_concurrent_interval_ms"
        | "codex_reasoning_guard_concurrent_max_attempts"
        | "codex_reasoning_guard_model_fallbacks"
        | "codex_reasoning_guard_continuation_repair_enabled"
        | "codex_reasoning_guard_continuation_max_rounds"
        | "codex_reasoning_guard_continuation_max_output_tokens"
      >
    >
  ) => Promise<boolean> | boolean;
  persistCodexHomeSettings?: (
    codexHomeMode: CodexHomeMode,
    codexHomeOverride: string
  ) => Promise<boolean> | boolean;
  persistCodexOauthCompatibleProxyMode?: (enabled: boolean) => Promise<boolean> | boolean;
  pickCodexHomeDirectory?: (initialPath?: string) => Promise<string | null> | string | null;
};

function SettingItem({
  label,
  subtitle,
  children,
  className,
}: {
  label: string;
  subtitle: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-2 py-3 sm:flex-row sm:items-start sm:justify-between",
        className
      )}
    >
      <div className="min-w-0">
        <div className="text-sm text-secondary-foreground">{label}</div>
        <div className="mt-1 text-xs text-muted-foreground leading-relaxed">{subtitle}</div>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">{children}</div>
    </div>
  );
}

function CodexReasoningGuardStatsRangeControls({
  ariaLabel = "降智拦截统计时间范围",
  dateInputLabelPrefix = "降智拦截统计",
  label,
  startDate,
  endDate,
  appliedLabel,
  error,
  popoverOpen,
  fetching,
  onPopoverOpenChange,
  onStartDateChange,
  onEndDateChange,
  onPreset,
  onApply,
  onRefresh,
  popoverPortalled = true,
}: {
  ariaLabel?: string;
  dateInputLabelPrefix?: string;
  label?: string;
  startDate: string;
  endDate: string;
  appliedLabel: string;
  error: string | null;
  popoverOpen: boolean;
  fetching: boolean;
  onPopoverOpenChange: (open: boolean) => void;
  onStartDateChange: (value: string) => void;
  onEndDateChange: (value: string) => void;
  onPreset: (preset: CodexReasoningGuardStatsPreset) => void;
  onApply: () => void;
  onRefresh: () => void;
  popoverPortalled?: boolean;
}) {
  const presets: Array<[CodexReasoningGuardStatsPreset, string]> = [
    ["today", "今天"],
    ["yesterday", "昨天"],
    ["last24h", "近24小时"],
    ["last7", "近7天"],
    ["last14", "近14天"],
    ["last30", "近30天"],
    ["thisMonth", "本月"],
    ["lastMonth", "上月"],
  ];

  return (
    <div className="flex flex-wrap items-center gap-2" aria-label={ariaLabel}>
      {label ? (
        <span className="text-sm font-medium text-secondary-foreground">{label}</span>
      ) : null}
      <Popover
        open={popoverOpen}
        onOpenChange={onPopoverOpenChange}
        align="start"
        placement="bottom"
        className="rounded-lg"
        contentClassName="w-[min(30rem,calc(100vw-2rem))] border-border bg-card p-0"
        portalled={popoverPortalled}
        trigger={
          <span className="inline-flex h-10 items-center gap-2 rounded-lg border border-border bg-background px-3 text-sm font-medium text-foreground shadow-sm transition-colors hover:bg-muted data-[state=open]:border-accent">
            <Calendar className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
            {appliedLabel}
            <ChevronDown className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
          </span>
        }
      >
        <div className="overflow-hidden rounded-lg">
          <div className="grid grid-cols-2 gap-1 p-3">
            {presets.map(([preset, presetLabel]) => (
              <button
                key={preset}
                type="button"
                className="rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                onClick={() => onPreset(preset)}
              >
                {presetLabel}
              </button>
            ))}
          </div>
          <div className="border-t border-border p-3">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_1.5rem_minmax(0,1fr)] sm:items-end">
              <label className="text-xs font-medium text-muted-foreground">
                <span className="mb-1 block">开始日期</span>
                <Input
                  aria-label={`${dateInputLabelPrefix}开始日期`}
                  type="date"
                  value={startDate}
                  onChange={(e) => onStartDateChange(e.currentTarget.value)}
                  className="text-sm"
                />
              </label>
              <div className="hidden pb-2 text-center text-muted-foreground sm:block">→</div>
              <label className="text-xs font-medium text-muted-foreground">
                <span className="mb-1 block">结束日期</span>
                <Input
                  aria-label={`${dateInputLabelPrefix}结束日期`}
                  type="date"
                  value={endDate}
                  onChange={(e) => onEndDateChange(e.currentTarget.value)}
                  className="text-sm"
                />
              </label>
            </div>
            {error ? (
              <div className="mt-2 text-xs text-rose-600 dark:text-rose-400">{error}</div>
            ) : null}
            <div className="mt-3 flex justify-end">
              <Button size="sm" onClick={onApply}>
                应用
              </Button>
            </div>
          </div>
        </div>
      </Popover>
      <Button variant="secondary" size="sm" className="h-10 gap-2" onClick={onRefresh}>
        <RefreshCw className={cn("h-3.5 w-3.5", fetching && "animate-spin")} aria-hidden="true" />
        刷新
      </Button>
    </div>
  );
}

function boolOrDefault(value: boolean | null, fallback: boolean) {
  return value ?? fallback;
}

function enumOrDefault(value: string | null, fallback: string) {
  return (value ?? fallback).trim();
}

export function CliManagerCodexTab({
  codexAvailable,
  codexLoading,
  codexConfigLoading,
  codexConfigSaving,
  codexConfigTomlLoading,
  codexConfigTomlSaving,
  codexProviderSyncing = false,
  codexInfo,
  codexConfig,
  codexConfigToml,
  appSettings,
  commonSettingsSaving = false,
  codexHomeSettingsSaving = false,
  refreshCodex,
  openCodexConfigDir,
  persistCodexConfig,
  persistCodexConfigToml,
  syncCodexProvider,
  persistCommonSettings,
  persistCodexReasoningGuardSettings,
  persistCodexHomeSettings,
  persistCodexOauthCompatibleProxyMode,
  pickCodexHomeDirectory,
}: CliManagerCodexTabProps) {
  const todayDate = useMemo(() => formatLocalDateInputValue(new Date()), []);
  const [versionRefreshToken, setVersionRefreshToken] = useState(0);
  const [modelText, setModelText] = useState("");
  const [providerTestModelText, setProviderTestModelText] = useState(
    DEFAULT_CODEX_PROVIDER_TEST_MODEL
  );
  const [contextWindowText, setContextWindowText] = useState("");
  const [autoCompactLimitText, setAutoCompactLimitText] = useState("");
  const [sandboxModeText, setSandboxModeText] = useState("");
  const [webSearchText, setWebSearchText] = useState("");
  const [personalityText, setPersonalityText] = useState("none");
  const [reasoningEffortText, setReasoningEffortText] = useState("");
  const [planModeReasoningEffortText, setPlanModeReasoningEffortText] = useState("");
  const [configLocationMode, setConfigLocationMode] = useState<CodexHomeMode>("user_home_default");
  const [customHomeText, setCustomHomeText] = useState("");
  const [configLocationError, setConfigLocationError] = useState<string | null>(null);
  const [selectingCodexHomeDir, setSelectingCodexHomeDir] = useState(false);
  const [codexReasoningGuardHitLabelText, setCodexReasoningGuardHitLabelText] =
    useState("降智命中");
  const [codexReasoningGuardRuleMode, setCodexReasoningGuardRuleMode] =
    useState<CodexReasoningGuardRuleMode>(DEFAULT_CODEX_REASONING_GUARD_RULE_MODE);
  const [codexReasoningGuardImmediateBudgetText, setCodexReasoningGuardImmediateBudgetText] =
    useState(String(DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET));
  const [codexReasoningGuardDelayedBudgetText, setCodexReasoningGuardDelayedBudgetText] = useState(
    String(DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET)
  );
  const [codexReasoningGuardDelayedMsText, setCodexReasoningGuardDelayedMsText] = useState(
    String(DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS)
  );
  const [codexReasoningGuardExhaustedAction, setCodexReasoningGuardExhaustedAction] =
    useState<CodexReasoningGuardExhaustedAction>(DEFAULT_CODEX_REASONING_GUARD_EXHAUSTED_ACTION);
  const [codexReasoningGuardPostMatchStrategy, setCodexReasoningGuardPostMatchStrategy] =
    useState<CodexReasoningGuardPostMatchStrategy>(
      DEFAULT_CODEX_REASONING_GUARD_POST_MATCH_STRATEGY
    );
  const [codexReasoningGuardRetryPolicy, setCodexReasoningGuardRetryPolicy] =
    useState<CodexReasoningGuardRetryPolicy>(DEFAULT_CODEX_REASONING_GUARD_RETRY_POLICY);
  const [codexReasoningGuardConcurrentMaxText, setCodexReasoningGuardConcurrentMaxText] = useState(
    String(DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX)
  );
  const [
    codexReasoningGuardConcurrentIntervalMsText,
    setCodexReasoningGuardConcurrentIntervalMsText,
  ] = useState(String(DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS));
  const [
    codexReasoningGuardConcurrentMaxAttemptsText,
    setCodexReasoningGuardConcurrentMaxAttemptsText,
  ] = useState(String(DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS));
  const [codexReasoningGuardModelFallbacksText, setCodexReasoningGuardModelFallbacksText] =
    useState("");
  const [
    codexReasoningGuardContinuationMaxOutputTokensText,
    setCodexReasoningGuardContinuationMaxOutputTokensText,
  ] = useState(String(DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS));
  const [codexReasoningGuardBudgetError, setCodexReasoningGuardBudgetError] = useState<
    string | null
  >(null);
  const [codexReasoningGuardContinuationError, setCodexReasoningGuardContinuationError] = useState<
    string | null
  >(null);
  const [codexReasoningGuardModelFallbacksError, setCodexReasoningGuardModelFallbacksError] =
    useState<string | null>(null);
  const [codexReasoningGuardDetailsOpen, setCodexReasoningGuardDetailsOpen] = useState(false);
  const [codexReasoningGuardDetailsTab, setCodexReasoningGuardDetailsTab] =
    useState<CodexReasoningGuardDetailsTab>("rules");
  const [codexReasoningGuardStartDate, setCodexReasoningGuardStartDate] = useState(todayDate);
  const [codexReasoningGuardEndDate, setCodexReasoningGuardEndDate] = useState(todayDate);
  const [codexReasoningGuardAppliedDateRange, setCodexReasoningGuardAppliedDateRange] =
    useState<CodexReasoningGuardStatsDateRange>({
      startDate: todayDate,
      endDate: todayDate,
    });
  const [codexReasoningGuardStatsRangeError, setCodexReasoningGuardStatsRangeError] = useState<
    string | null
  >(null);
  const [codexReasoningGuardStatsRangePopoverScope, setCodexReasoningGuardStatsRangePopoverScope] =
    useState<CodexReasoningGuardStatsRangePopoverScope | null>(null);
  const [codexReasoningGuardActiveTemplateId, setCodexReasoningGuardActiveTemplateId] = useState(
    CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
  );
  const [codexReasoningGuardCustomTemplates, setCodexReasoningGuardCustomTemplates] = useState<
    CodexReasoningGuardRuleTemplate[]
  >([]);
  const [codexReasoningGuardTemplateError, setCodexReasoningGuardTemplateError] = useState<
    string | null
  >(null);

  const [tomlAdvancedOpen, setTomlAdvancedOpen] = useState(false);
  const [tomlEditEnabled, setTomlEditEnabled] = useState(false);
  const [tomlDraft, setTomlDraft] = useState("");
  const [tomlDirty, setTomlDirty] = useState(false);
  const [tomlValidating, setTomlValidating] = useState(false);
  const [tomlValidation, setTomlValidation] = useState<CodexConfigTomlValidationResult | null>(
    null
  );

  const validateSeqRef = useRef(0);
  const validateTimerRef = useRef<number | null>(null);
  const lastTomlConfigPathRef = useRef<string | null>(null);

  const validateToml = useCallback(
    async (toml: string): Promise<CodexConfigTomlValidationResult | null> => {
      const seq = validateSeqRef.current + 1;
      validateSeqRef.current = seq;
      setTomlValidating(true);
      try {
        const result = await cliManagerCodexConfigTomlValidate(toml);
        if (seq !== validateSeqRef.current) return null;
        if (!result) return null;
        setTomlValidation(result);
        return result;
      } finally {
        if (seq === validateSeqRef.current) {
          setTomlValidating(false);
        }
      }
    },
    []
  );

  useEffect(() => {
    if (!codexConfig) return;
    setModelText(codexConfig.model ?? "");
    setContextWindowText(
      codexConfig.model_context_window != null ? String(codexConfig.model_context_window) : ""
    );
    setAutoCompactLimitText(
      codexConfig.model_auto_compact_token_limit != null
        ? String(codexConfig.model_auto_compact_token_limit)
        : ""
    );
    setSandboxModeText(codexConfig.sandbox_mode ?? "");
    setWebSearchText(codexConfig.web_search ?? "cached");
    setPersonalityText(codexConfig.personality?.trim() || "none");
    setReasoningEffortText(codexConfig.model_reasoning_effort ?? "");
    setPlanModeReasoningEffortText(codexConfig.plan_mode_reasoning_effort ?? "");
  }, [codexConfig]);

  useEffect(() => {
    setProviderTestModelText(
      appSettings?.codex_provider_test_model?.trim() || DEFAULT_CODEX_PROVIDER_TEST_MODEL
    );
  }, [appSettings?.codex_provider_test_model]);

  useEffect(() => {
    const savedOverride = appSettings?.codex_home_override?.trim() ?? "";
    const savedMode =
      appSettings?.codex_home_mode ?? (savedOverride ? "custom" : "user_home_default");
    setConfigLocationMode(savedMode);
    setCustomHomeText(savedOverride);
    setConfigLocationError(null);
  }, [appSettings?.codex_home_mode, appSettings?.codex_home_override]);

  const syncCodexReasoningGuardDrafts = useCallback(
    (
      source: AppSettings | null | undefined = appSettings,
      options: { includeGuard?: boolean } = {}
    ) => {
      const includeGuard = options.includeGuard ?? true;

      if (includeGuard) {
        setCodexReasoningGuardHitLabelText(
          source?.codex_reasoning_guard_hit_label?.trim() || "降智命中"
        );
        setCodexReasoningGuardRuleMode(
          source?.codex_reasoning_guard_rule_mode ?? DEFAULT_CODEX_REASONING_GUARD_RULE_MODE
        );
        setCodexReasoningGuardImmediateBudgetText(
          String(
            source?.codex_reasoning_guard_immediate_retry_budget ??
              DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
          )
        );
        setCodexReasoningGuardDelayedBudgetText(
          String(
            source?.codex_reasoning_guard_delayed_retry_budget ??
              DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
          )
        );
        setCodexReasoningGuardDelayedMsText(
          String(
            source?.codex_reasoning_guard_delayed_retry_ms ??
              DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS
          )
        );
        setCodexReasoningGuardExhaustedAction(
          source?.codex_reasoning_guard_exhausted_action ??
            DEFAULT_CODEX_REASONING_GUARD_EXHAUSTED_ACTION
        );
        setCodexReasoningGuardPostMatchStrategy(
          source?.codex_reasoning_guard_post_match_strategy ??
            DEFAULT_CODEX_REASONING_GUARD_POST_MATCH_STRATEGY
        );
        setCodexReasoningGuardRetryPolicy(
          source?.codex_reasoning_guard_retry_policy ?? DEFAULT_CODEX_REASONING_GUARD_RETRY_POLICY
        );
        setCodexReasoningGuardConcurrentMaxText(
          String(
            source?.codex_reasoning_guard_concurrent_max ??
              DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX
          )
        );
        setCodexReasoningGuardConcurrentIntervalMsText(
          String(
            source?.codex_reasoning_guard_concurrent_interval_ms ??
              DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
          )
        );
        setCodexReasoningGuardConcurrentMaxAttemptsText(
          String(
            source?.codex_reasoning_guard_concurrent_max_attempts ??
              DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
          )
        );
        setCodexReasoningGuardModelFallbacksText(
          formatCodexReasoningGuardModelFallbacks(source?.codex_reasoning_guard_model_fallbacks)
        );
        setCodexReasoningGuardContinuationMaxOutputTokensText(
          String(
            source?.codex_reasoning_guard_continuation_max_output_tokens ??
              DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
          )
        );
        setCodexReasoningGuardBudgetError(null);
        setCodexReasoningGuardContinuationError(null);
        setCodexReasoningGuardModelFallbacksError(null);
        const customTemplates = source?.codex_reasoning_guard_custom_templates ?? [];
        const savedTemplateId =
          source?.codex_reasoning_guard_active_template_id?.trim() ||
          CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID;
        const activeTemplateId =
          resolveCodexReasoningGuardTemplateOption(savedTemplateId, customTemplates)?.id ??
          CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID;
        setCodexReasoningGuardCustomTemplates(customTemplates);
        setCodexReasoningGuardActiveTemplateId(activeTemplateId);
        setCodexReasoningGuardTemplateError(null);
      }
    },
    [appSettings]
  );

  useEffect(() => {
    if (codexReasoningGuardDetailsOpen) return;
    syncCodexReasoningGuardDrafts(appSettings);
  }, [codexReasoningGuardDetailsOpen, appSettings, syncCodexReasoningGuardDrafts]);

  function readSavedConfigLocationState() {
    const savedOverride = appSettings?.codex_home_override?.trim() ?? "";
    const savedMode =
      appSettings?.codex_home_mode ?? (savedOverride ? "custom" : "user_home_default");
    return { savedMode, savedOverride };
  }

  function restoreSavedConfigLocationState() {
    const { savedMode, savedOverride } = readSavedConfigLocationState();
    setConfigLocationMode(savedMode);
    setCustomHomeText(savedOverride);
    setConfigLocationError(null);
  }

  const saving = codexConfigSaving;
  const loading = codexLoading || codexConfigLoading;
  const tomlBusy = codexConfigTomlLoading || codexConfigTomlSaving;
  const providerSyncControlsDisabled =
    codexConfigSaving || codexConfigTomlSaving || codexProviderSyncing || !syncCodexProvider;
  const configLocationBusy = saving || codexHomeSettingsSaving;
  const configLocationControlsDisabled = configLocationBusy || selectingCodexHomeDir;
  const commonSettingsControlsDisabled = codexHomeSettingsSaving || !appSettings;
  const providerTestModelControlsDisabled =
    commonSettingsSaving || !appSettings || !persistCommonSettings;
  const proxyModeControlsDisabled =
    commonSettingsControlsDisabled || !persistCodexOauthCompatibleProxyMode;
  const reasoningGuardControlsDisabled =
    commonSettingsControlsDisabled || !persistCodexReasoningGuardSettings;
  const codexReasoningGuardTemplateOptions = useMemo<CodexReasoningGuardTemplateOption[]>(
    () => [
      ...CODEX_REASONING_GUARD_BUILTIN_TEMPLATES,
      ...codexReasoningGuardCustomTemplates.map((template) => ({
        ...template,
        source: "custom" as const,
        readOnly: false,
      })),
    ],
    [codexReasoningGuardCustomTemplates]
  );
  const codexReasoningGuardSelectedTemplate = useMemo(
    () =>
      resolveCodexReasoningGuardTemplateOption(
        codexReasoningGuardActiveTemplateId,
        codexReasoningGuardCustomTemplates
      ) ?? CODEX_REASONING_GUARD_BUILTIN_TEMPLATES[0],
    [codexReasoningGuardActiveTemplateId, codexReasoningGuardCustomTemplates]
  );
  const codexReasoningGuardStatsRange = useMemo(
    () =>
      buildCodexReasoningGuardStatsRange(
        codexReasoningGuardAppliedDateRange.startDate,
        codexReasoningGuardAppliedDateRange.endDate
      ),
    [codexReasoningGuardAppliedDateRange]
  );
  const codexReasoningGuardStatsQuery = useCliManagerCodexReasoningGuardStatsQuery(
    codexReasoningGuardStatsRange,
    {
      enabled: codexReasoningGuardStatsRange != null,
    }
  );
  const codexReasoningGuardStatsRangeLabel = formatCodexReasoningGuardStatsDateRangeLabel(
    codexReasoningGuardAppliedDateRange
  );
  const codexReasoningGuardStatsRangeDescription =
    codexReasoningGuardAppliedDateRange.startDate === codexReasoningGuardAppliedDateRange.endDate
      ? "只统计当天产生的 Codex 请求，方便快速判断今日的降智拦截情况。"
      : "按自然日统计所选日期范围内的 Codex 请求，包含结束日期当天。";
  const codexReasoningGuardStats = codexReasoningGuardStatsQuery.data ?? null;
  const codexReasoningGuardStatsLoading = codexReasoningGuardStatsQuery.isFetching;

  async function refreshCodexStatus() {
    try {
      await refreshCodex();
    } finally {
      setVersionRefreshToken((value) => value + 1);
    }
  }

  async function saveProviderTestModel(nextValue: string) {
    if (!persistCommonSettings || !appSettings || providerTestModelControlsDisabled) {
      return;
    }
    const normalized = nextValue.trim() || DEFAULT_CODEX_PROVIDER_TEST_MODEL;
    setProviderTestModelText(normalized);
    try {
      const updated = await persistCommonSettings({
        codex_provider_test_model: normalized,
      });
      const saved = updated?.codex_provider_test_model?.trim() || normalized;
      setProviderTestModelText(saved);
    } catch {
      setProviderTestModelText(appSettings.codex_provider_test_model || normalized);
    }
  }

  // sandbox_mode 的本地 text 已由上方 codexConfig 整体同步 effect 更新，
  // 此处不再需要额外的 saving 守卫同步——之前的实现会在 saving 从
  // true→false 时用旧的 codexConfig 覆盖本地状态，导致 danger-full-access
  // 选择后被重置为默认值。

  const defaults = useMemo(() => {
    return {
      sandbox_mode: "workspace-write",
    };
  }, []);

  const effectiveSandboxMode = useMemo(() => {
    return enumOrDefault(sandboxModeText.trim() || null, defaults.sandbox_mode);
  }, [sandboxModeText, defaults.sandbox_mode]);

  const effectiveFastModeEnabled = useMemo(() => {
    if (!codexConfig) return false;
    return (
      boolOrDefault(codexConfig.features_fast_mode, false) ||
      codexConfig.service_tier === FAST_SERVICE_TIER
    );
  }, [codexConfig]);

  const showsGpt54LinkedSettings = useMemo(() => {
    return isGpt54Model(modelText);
  }, [modelText]);

  const configLocationPreviewPath = useMemo(() => {
    return buildConfigTomlPath(customHomeText);
  }, [customHomeText]);

  const userDefaultResolvedHomeDir = useMemo(() => {
    return codexConfig?.user_home_default_dir?.trim() || "~/.codex";
  }, [codexConfig?.user_home_default_dir]);

  const followCodexHomeResolvedDir = useMemo(() => {
    return codexConfig?.follow_codex_home_dir?.trim() || "~/.codex";
  }, [codexConfig?.follow_codex_home_dir]);

  const followModeMatchesDefault = useMemo(() => {
    return (
      normalizeComparablePath(followCodexHomeResolvedDir) ===
      normalizeComparablePath(userDefaultResolvedHomeDir)
    );
  }, [followCodexHomeResolvedDir, userDefaultResolvedHomeDir]);

  const followModeLabel = useMemo(() => {
    return followModeMatchesDefault
      ? "跟随环境变量 $CODEX_HOME（当前路径与固定目录一致）"
      : "跟随环境变量 $CODEX_HOME";
  }, [followModeMatchesDefault]);

  const configLocationBrowsePath = useMemo(() => {
    const trimmedCustomHome = customHomeText.trim();
    if (trimmedCustomHome) {
      return normalizeCustomCodexHome(trimmedCustomHome);
    }

    const savedOverride = appSettings?.codex_home_override?.trim();
    if (configLocationMode === "custom" && savedOverride) {
      return savedOverride;
    }

    if (configLocationMode === "follow_codex_home") {
      return codexConfig?.follow_codex_home_dir?.trim() || "";
    }

    return codexConfig?.user_home_default_dir?.trim() || "";
  }, [
    appSettings?.codex_home_override,
    codexConfig?.follow_codex_home_dir,
    codexConfig?.user_home_default_dir,
    configLocationMode,
    customHomeText,
  ]);

  const configLocationSummaryText = useMemo(() => {
    if (configLocationMode === "custom") {
      return customHomeText.trim()
        ? "自定义模式已启用。应用会在你指定的 .codex 目录下读写 config.toml。"
        : "自定义模式待保存。请输入一个 .codex 目录路径后按 Enter 或移出输入框保存。";
    }

    if (configLocationMode === "follow_codex_home") {
      return `跟随模式已启用。当前将使用 ${followCodexHomeResolvedDir}；如果没有设置 $CODEX_HOME，则回退到 Windows 用户目录下的 .codex，后续也会随环境变量变化。`;
    }

    return `固定模式已启用。当前固定使用 Windows 用户目录下的 .codex：${userDefaultResolvedHomeDir}；不会跟随当前的 $CODEX_HOME。`;
  }, [configLocationMode, customHomeText, followCodexHomeResolvedDir, userDefaultResolvedHomeDir]);

  const activeConfigDirSummaryText = useMemo(() => {
    if (configLocationMode === "custom") {
      return "当前为手动指定目录模式。";
    }

    if (configLocationMode === "follow_codex_home") {
      return "当前路径跟随 $CODEX_HOME 解析；后续会随环境变量变化。";
    }

    return isWindowsRuntime()
      ? "当前固定使用 Windows 用户目录下的 .codex。"
      : "当前固定使用用户主目录下的 .codex。";
  }, [configLocationMode]);

  const activeConfigModeBadgeText = useMemo(() => {
    if (configLocationMode === "custom") {
      return "手动指定";
    }

    if (configLocationMode === "follow_codex_home") {
      return "跟随变量";
    }

    return "固定目录";
  }, [configLocationMode]);

  const activeConfigDirPrimaryText = useMemo(() => {
    if (configLocationMode === "custom") {
      return customHomeText.trim() || codexConfig?.config_dir || "";
    }

    if (configLocationMode === "follow_codex_home") {
      return followCodexHomeResolvedDir;
    }

    return userDefaultResolvedHomeDir;
  }, [
    codexConfig?.config_dir,
    configLocationMode,
    customHomeText,
    followCodexHomeResolvedDir,
    userDefaultResolvedHomeDir,
  ]);

  useEffect(() => {
    const nextPath = codexConfigToml?.config_path ?? null;
    const prevPath = lastTomlConfigPathRef.current;

    if (!nextPath) {
      lastTomlConfigPathRef.current = null;
      return;
    }

    if (prevPath && prevPath !== nextPath) {
      if (validateTimerRef.current) {
        window.clearTimeout(validateTimerRef.current);
        validateTimerRef.current = null;
      }

      validateSeqRef.current += 1;
      setTomlDraft(codexConfigToml?.toml ?? "");
      setTomlDirty(false);
      setTomlValidating(false);
      setTomlValidation(null);
      setTomlEditEnabled(false);
    }

    lastTomlConfigPathRef.current = nextPath;
  }, [codexConfigToml?.config_path, codexConfigToml?.toml]);

  useEffect(() => {
    if (!codexConfigToml) return;
    if (tomlDirty) return;
    setTomlDraft(codexConfigToml.toml ?? "");
  }, [codexConfigToml, tomlDirty]);

  useEffect(() => {
    if (!tomlAdvancedOpen) return;
    if (!tomlEditEnabled) return;
    if (!tomlDirty) return;

    if (validateTimerRef.current) {
      window.clearTimeout(validateTimerRef.current);
    }

    validateTimerRef.current = window.setTimeout(() => {
      void validateToml(tomlDraft);
    }, 500);

    return () => {
      if (validateTimerRef.current) {
        window.clearTimeout(validateTimerRef.current);
        validateTimerRef.current = null;
      }
    };
  }, [tomlDraft, tomlDirty, tomlAdvancedOpen, tomlEditEnabled, validateToml]);

  async function saveTomlDraft() {
    if (tomlBusy) return;
    const result = await validateToml(tomlDraft);
    if (!result) return;
    if (!result.ok) return;

    const ok = await persistCodexConfigToml(tomlDraft);
    if (!ok) return;

    setTomlEditEnabled(false);
    setTomlDirty(false);
  }

  async function saveSandboxMode(next: string) {
    if (next === "danger-full-access") {
      const ok = await confirmDesktopDialog(
        "你选择了 danger-full-access（危险：完全访问）。确认要继续吗？"
      );
      if (!ok) {
        setSandboxModeText(codexConfig?.sandbox_mode ?? "");
        return;
      }
    }
    setSandboxModeText(next);
    void persistCodexConfig({ sandbox_mode: next });
  }

  async function persistConfigLocation(
    nextMode: CodexHomeMode,
    nextCustomHome = customHomeText
  ): Promise<PersistConfigLocationResult> {
    if (!persistCodexHomeSettings) return "persist_failed";

    const trimmed = nextCustomHome.trim();
    const normalized = normalizeCustomCodexHome(trimmed);
    if (nextMode === "custom") {
      const error = validateCustomCodexHome(trimmed);
      setConfigLocationError(error);
      if (error) return "validation_failed";
    } else {
      setConfigLocationError(null);
    }

    const nextOverride = nextMode === "custom" ? normalized : "";
    const saved = Boolean(await persistCodexHomeSettings(nextMode, nextOverride));
    if (!saved) {
      return "persist_failed";
    }

    setConfigLocationMode(nextMode);
    setCustomHomeText(nextMode === "custom" ? nextOverride : "");
    setConfigLocationError(null);
    return "saved";
  }

  async function handleConfigLocationModeChange(nextMode: CodexHomeMode) {
    setConfigLocationMode(nextMode);

    if (nextMode !== "custom") {
      setCustomHomeText("");
      setConfigLocationError(null);
      const result = await persistConfigLocation(nextMode, "");
      if (result === "persist_failed") {
        restoreSavedConfigLocationState();
      }
      return;
    }

    const error = validateCustomCodexHome(customHomeText);
    setConfigLocationError(error);
    if (error) {
      return;
    }

    const result = await persistConfigLocation("custom", customHomeText);
    if (result === "persist_failed") {
      restoreSavedConfigLocationState();
    }
  }

  async function resetConfigLocation() {
    setConfigLocationMode("user_home_default");
    setCustomHomeText("");
    setConfigLocationError(null);
    const result = await persistConfigLocation("user_home_default", "");
    if (result === "persist_failed") {
      restoreSavedConfigLocationState();
    }
  }

  async function handlePickCustomHome() {
    if (!pickCodexHomeDirectory) return;
    if (configLocationControlsDisabled) return;

    setSelectingCodexHomeDir(true);
    try {
      const picked = await pickCodexHomeDirectory(configLocationBrowsePath || undefined);
      if (!picked) return;

      const normalized = normalizeCustomCodexHome(picked);
      setConfigLocationMode("custom");
      setCustomHomeText(normalized);

      const error = validateCustomCodexHome(normalized);
      setConfigLocationError(error);
      if (error) {
        return;
      }

      const result = await persistConfigLocation("custom", normalized);
      if (result === "persist_failed") {
        restoreSavedConfigLocationState();
      }
    } finally {
      setSelectingCodexHomeDir(false);
    }
  }

  function parseCodexReasoningGuardInteger(
    raw: string,
    label: string,
    max: number,
    min = 0
  ): { ok: true; value: number } | { ok: false; message: string } {
    const valueText = raw.trim();
    if (!valueText) {
      return { ok: false, message: `${label}不能为空。` };
    }
    if (!/^\d+$/u.test(valueText)) {
      return { ok: false, message: `${label}必须是非负整数。` };
    }
    const value = Number(valueText);
    if (!Number.isSafeInteger(value) || value < min || value > max) {
      return { ok: false, message: `${label}必须在 ${min} 到 ${max} 之间。` };
    }
    return { ok: true, value };
  }

  function parseCodexReasoningGuardModelFallbacks(raw: string):
    | { ok: true; models: string[] }
    | {
        ok: false;
        message: string;
      } {
    const rawModels = raw
      .split(/\r?\n|,/u)
      .map((item) => item.trim())
      .filter(Boolean);
    const models: string[] = [];
    const seenModels = new Set<string>();
    for (const model of rawModels) {
      if (seenModels.has(model)) continue;
      seenModels.add(model);
      models.push(model);
    }
    if (models.length > MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN) {
      return {
        ok: false,
        message: `最多支持 ${MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN} 个回退模型。`,
      };
    }

    for (const model of models) {
      if (model.length > MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN) {
        return {
          ok: false,
          message: `模型名必须 <= ${MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN} 字符。`,
        };
      }
      if (/[\u0000-\u001f\u007f-\u009f]/u.test(model)) {
        return { ok: false, message: "模型名不能包含控制字符。" };
      }
    }

    return { ok: true, models };
  }

  function selectCodexReasoningGuardTemplate(
    templateId: string,
    customTemplates = codexReasoningGuardCustomTemplates
  ) {
    const nextTemplate =
      resolveCodexReasoningGuardTemplateOption(templateId, customTemplates) ??
      CODEX_REASONING_GUARD_BUILTIN_TEMPLATES[0];
    setCodexReasoningGuardActiveTemplateId(nextTemplate.id);
    setCodexReasoningGuardTemplateError(null);
    setCodexReasoningGuardRuleMode(
      nextTemplate.id === CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
        ? "final_answer_only_high_xhigh"
        : "reasoning_tokens"
    );
  }

  function addCodexReasoningGuardCustomTemplate() {
    if (codexReasoningGuardCustomTemplates.length >= MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES) {
      setCodexReasoningGuardTemplateError(
        `最多支持 ${MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES} 个自定义模板。`
      );
      return;
    }
    const nextTemplate = buildNewCodexReasoningGuardTemplate(codexReasoningGuardCustomTemplates);
    const nextTemplates = [...codexReasoningGuardCustomTemplates, nextTemplate];
    setCodexReasoningGuardCustomTemplates(nextTemplates);
    selectCodexReasoningGuardTemplate(nextTemplate.id, nextTemplates);
  }

  function copySelectedCodexReasoningGuardTemplate() {
    if (codexReasoningGuardCustomTemplates.length >= MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES) {
      setCodexReasoningGuardTemplateError(
        `最多支持 ${MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES} 个自定义模板。`
      );
      return;
    }
    const nextTemplate = copyCodexReasoningGuardTemplateAsCustom(
      codexReasoningGuardSelectedTemplate,
      codexReasoningGuardCustomTemplates
    );
    const nextTemplates = [...codexReasoningGuardCustomTemplates, nextTemplate];
    setCodexReasoningGuardCustomTemplates(nextTemplates);
    selectCodexReasoningGuardTemplate(nextTemplate.id, nextTemplates);
  }

  function deleteSelectedCodexReasoningGuardTemplate() {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    const nextTemplates = codexReasoningGuardCustomTemplates.filter(
      (template) => template.id !== codexReasoningGuardSelectedTemplate.id
    );
    setCodexReasoningGuardCustomTemplates(nextTemplates);
    selectCodexReasoningGuardTemplate(
      CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
      nextTemplates
    );
  }

  function updateSelectedCodexReasoningGuardTemplate(
    patch: Partial<CodexReasoningGuardRuleTemplate>
  ) {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    setCodexReasoningGuardCustomTemplates((prev) =>
      prev.map((template) =>
        template.id === codexReasoningGuardSelectedTemplate.id
          ? { ...template, ...patch }
          : template
      )
    );
    if (patch.id != null) {
      setCodexReasoningGuardActiveTemplateId(patch.id);
    }
    setCodexReasoningGuardTemplateError(null);
  }

  function updateSelectedCodexReasoningGuardTemplateRule(
    ruleIndex: number,
    patch: Partial<CodexReasoningGuardTemplateRule>
  ) {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    updateSelectedCodexReasoningGuardTemplate({
      rules: codexReasoningGuardSelectedTemplate.rules.map((rule, index) =>
        index === ruleIndex ? { ...rule, ...patch } : rule
      ),
    });
  }

  function updateSelectedCodexReasoningGuardTemplateRuleToken(ruleIndex: number, value: string) {
    const raw = value.trim();
    if (raw === "") {
      updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
        reasoning_tokens: null,
        reasoning_tokens_formula: null,
      });
      return;
    }
    const parsed = Number(raw);
    if (
      !Number.isInteger(parsed) ||
      parsed < 0 ||
      parsed > MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE
    ) {
      setCodexReasoningGuardTemplateError(
        `token 匹配必须是 0 到 ${MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE} 之间的整数；留空才表示 wildcard。`
      );
      return;
    }
    updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
      reasoning_tokens: parsed,
      reasoning_tokens_formula: null,
    });
  }

  function addCodexReasoningGuardTemplateRule() {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    if (
      codexReasoningGuardSelectedTemplate.rules.length >= MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES
    ) {
      setCodexReasoningGuardTemplateError(
        `最多支持 ${MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES} 条规则。`
      );
      return;
    }
    const nextIndex = codexReasoningGuardSelectedTemplate.rules.length + 1;
    updateSelectedCodexReasoningGuardTemplate({
      rules: [
        ...codexReasoningGuardSelectedTemplate.rules,
        buildCodexReasoningGuardRule(`rule-${nextIndex}`),
      ],
    });
  }

  function removeCodexReasoningGuardTemplateRule(ruleIndex: number) {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    updateSelectedCodexReasoningGuardTemplate({
      rules: codexReasoningGuardSelectedTemplate.rules.filter((_, index) => index !== ruleIndex),
    });
  }

  function moveCodexReasoningGuardTemplateRule(ruleIndex: number, direction: -1 | 1) {
    if (codexReasoningGuardSelectedTemplate.readOnly) return;
    const targetIndex = ruleIndex + direction;
    if (targetIndex < 0 || targetIndex >= codexReasoningGuardSelectedTemplate.rules.length) return;
    const nextRules = [...codexReasoningGuardSelectedTemplate.rules];
    const [rule] = nextRules.splice(ruleIndex, 1);
    nextRules.splice(targetIndex, 0, rule);
    updateSelectedCodexReasoningGuardTemplate({ rules: nextRules });
  }

  function updateCodexReasoningGuardTemplateFilter(
    ruleIndex: number,
    filterIndex: number,
    patch: Partial<CodexReasoningGuardTemplateFilter>
  ) {
    const rule = codexReasoningGuardSelectedTemplate.rules[ruleIndex];
    if (!rule || codexReasoningGuardSelectedTemplate.readOnly) return;
    updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
      filters: rule.filters.map((filter, index) => {
        if (index !== filterIndex) return filter;
        const nextFilter = { ...filter, ...patch };
        return normalizeCodexReasoningGuardFilterForField(nextFilter, nextFilter.field);
      }),
    });
  }

  function addCodexReasoningGuardTemplateFilter(ruleIndex: number) {
    const rule = codexReasoningGuardSelectedTemplate.rules[ruleIndex];
    if (!rule || codexReasoningGuardSelectedTemplate.readOnly) return;
    if (rule.filters.length >= MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS) {
      setCodexReasoningGuardTemplateError(
        `每条规则最多支持 ${MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS} 个过滤器。`
      );
      return;
    }
    updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
      filters: [
        ...rule.filters,
        buildCodexReasoningGuardFilter(
          `${rule.id || `rule-${ruleIndex + 1}`}-filter-${rule.filters.length + 1}`
        ),
      ],
    });
  }

  function removeCodexReasoningGuardTemplateFilter(ruleIndex: number, filterIndex: number) {
    const rule = codexReasoningGuardSelectedTemplate.rules[ruleIndex];
    if (!rule || codexReasoningGuardSelectedTemplate.readOnly) return;
    updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
      filters: rule.filters.filter((_, index) => index !== filterIndex),
    });
  }

  function applyCodexReasoningGuardStatsDateRange() {
    const nextRange = buildCodexReasoningGuardStatsRange(
      codexReasoningGuardStartDate,
      codexReasoningGuardEndDate
    );
    if (!nextRange) {
      setCodexReasoningGuardStatsRangeError("日期范围无效：结束日期必须不早于开始日期");
      return;
    }
    setCodexReasoningGuardStatsRangeError(null);
    setCodexReasoningGuardAppliedDateRange({
      startDate: codexReasoningGuardStartDate,
      endDate: codexReasoningGuardEndDate,
    });
    setCodexReasoningGuardStatsRangePopoverScope(null);
  }

  function applyCodexReasoningGuardStatsPreset(preset: CodexReasoningGuardStatsPreset) {
    const nextRange = buildCodexReasoningGuardStatsPresetRange(preset);
    setCodexReasoningGuardStartDate(nextRange.startDate);
    setCodexReasoningGuardEndDate(nextRange.endDate);
    setCodexReasoningGuardAppliedDateRange(nextRange);
    setCodexReasoningGuardStatsRangeError(null);
    setCodexReasoningGuardStatsRangePopoverScope(null);
  }

  function updateCodexReasoningGuardStatsStartDate(value: string) {
    setCodexReasoningGuardStartDate(value);
    if (codexReasoningGuardStatsRangeError) {
      setCodexReasoningGuardStatsRangeError(null);
    }
  }

  function updateCodexReasoningGuardStatsEndDate(value: string) {
    setCodexReasoningGuardEndDate(value);
    if (codexReasoningGuardStatsRangeError) {
      setCodexReasoningGuardStatsRangeError(null);
    }
  }

  function renderCodexReasoningGuardStatsRangeControls(
    label = "时间范围:",
    options?: {
      ariaLabel?: string;
      dateInputLabelPrefix?: string;
      popoverPortalled?: boolean;
      scope?: CodexReasoningGuardStatsRangePopoverScope;
    }
  ) {
    const scope = options?.scope ?? "overview";
    return (
      <CodexReasoningGuardStatsRangeControls
        ariaLabel={options?.ariaLabel}
        dateInputLabelPrefix={options?.dateInputLabelPrefix}
        label={label}
        startDate={codexReasoningGuardStartDate}
        endDate={codexReasoningGuardEndDate}
        appliedLabel={codexReasoningGuardStatsRangeLabel}
        error={codexReasoningGuardStatsRangeError}
        popoverOpen={codexReasoningGuardStatsRangePopoverScope === scope}
        fetching={codexReasoningGuardStatsQuery.isFetching}
        onPopoverOpenChange={(open) =>
          setCodexReasoningGuardStatsRangePopoverScope((current) =>
            open ? scope : current === scope ? null : current
          )
        }
        onStartDateChange={updateCodexReasoningGuardStatsStartDate}
        onEndDateChange={updateCodexReasoningGuardStatsEndDate}
        onPreset={applyCodexReasoningGuardStatsPreset}
        onApply={applyCodexReasoningGuardStatsDateRange}
        onRefresh={() => void codexReasoningGuardStatsQuery.refetch()}
        popoverPortalled={options?.popoverPortalled}
      />
    );
  }

  async function saveCodexReasoningGuardRules() {
    if (!appSettings || !persistCodexReasoningGuardSettings) return;
    if (codexReasoningGuardTemplateError) return;
    const normalizedHitLabel = codexReasoningGuardHitLabelText.trim() || "降智命中";

    const parsedImmediateBudget = parseCodexReasoningGuardInteger(
      codexReasoningGuardImmediateBudgetText,
      codexReasoningGuardPostMatchStrategy === "continuation_repair"
        ? "思考续写次数"
        : "立即重试预算",
      MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
    );
    if (!parsedImmediateBudget.ok) {
      setCodexReasoningGuardBudgetError(parsedImmediateBudget.message);
      return;
    }

    const parsedDelayedBudget = parseCodexReasoningGuardInteger(
      codexReasoningGuardDelayedBudgetText,
      "等待重试预算",
      MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
    );
    if (!parsedDelayedBudget.ok) {
      setCodexReasoningGuardBudgetError(parsedDelayedBudget.message);
      return;
    }

    const parsedDelayedMs = parseCodexReasoningGuardInteger(
      codexReasoningGuardDelayedMsText,
      "等待时间",
      MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS
    );
    if (!parsedDelayedMs.ok) {
      setCodexReasoningGuardBudgetError(parsedDelayedMs.message);
      return;
    }

    const parsedConcurrentMax = parseCodexReasoningGuardInteger(
      codexReasoningGuardConcurrentMaxText,
      "并发数量",
      MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX,
      1
    );
    if (!parsedConcurrentMax.ok) {
      setCodexReasoningGuardBudgetError(parsedConcurrentMax.message);
      return;
    }

    const parsedConcurrentIntervalMs = parseCodexReasoningGuardInteger(
      codexReasoningGuardConcurrentIntervalMsText,
      "并发间隔",
      MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
    );
    if (!parsedConcurrentIntervalMs.ok) {
      setCodexReasoningGuardBudgetError(parsedConcurrentIntervalMs.message);
      return;
    }

    const parsedConcurrentMaxAttempts = parseCodexReasoningGuardInteger(
      codexReasoningGuardConcurrentMaxAttemptsText,
      "并发最大尝试次数",
      MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
    );
    if (!parsedConcurrentMaxAttempts.ok) {
      setCodexReasoningGuardBudgetError(parsedConcurrentMaxAttempts.message);
      return;
    }

    const parsedFallbackModels = parseCodexReasoningGuardModelFallbacks(
      codexReasoningGuardModelFallbacksText
    );
    if (!parsedFallbackModels.ok) {
      setCodexReasoningGuardModelFallbacksError(parsedFallbackModels.message);
      return;
    }

    const parsedContinuationMaxOutputTokens = parseCodexReasoningGuardInteger(
      codexReasoningGuardContinuationMaxOutputTokensText,
      "继续思考最大 output tokens",
      MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
    );
    if (!parsedContinuationMaxOutputTokens.ok) {
      setCodexReasoningGuardContinuationError(parsedContinuationMaxOutputTokens.message);
      return;
    }

    const nextActiveTemplateId =
      resolveCodexReasoningGuardTemplateOption(
        codexReasoningGuardActiveTemplateId,
        codexReasoningGuardCustomTemplates
      )?.id ?? CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID;
    const validationMessage = validateSettingsSetInput({
      codexReasoningGuardActiveTemplateId: nextActiveTemplateId,
      codexReasoningGuardCustomTemplates: codexReasoningGuardCustomTemplates,
      codexReasoningGuardPostMatchStrategy: codexReasoningGuardPostMatchStrategy,
      codexReasoningGuardContinuationMaxOutputTokens: parsedContinuationMaxOutputTokens.value,
    });
    if (validationMessage) {
      setCodexReasoningGuardTemplateError(validationMessage);
      return;
    }
    let nextRuleMode: CodexReasoningGuardRuleMode = codexReasoningGuardRuleMode;
    if (nextActiveTemplateId === CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID) {
      nextRuleMode = "final_answer_only_high_xhigh";
    } else {
      nextRuleMode = "reasoning_tokens";
    }

    setCodexReasoningGuardBudgetError(null);
    setCodexReasoningGuardContinuationError(null);
    setCodexReasoningGuardModelFallbacksError(null);
    setCodexReasoningGuardTemplateError(null);
    setCodexReasoningGuardHitLabelText(normalizedHitLabel);
    setCodexReasoningGuardImmediateBudgetText(String(parsedImmediateBudget.value));
    setCodexReasoningGuardDelayedBudgetText(String(parsedDelayedBudget.value));
    setCodexReasoningGuardDelayedMsText(String(parsedDelayedMs.value));
    setCodexReasoningGuardConcurrentMaxText(String(parsedConcurrentMax.value));
    setCodexReasoningGuardConcurrentIntervalMsText(String(parsedConcurrentIntervalMs.value));
    setCodexReasoningGuardConcurrentMaxAttemptsText(String(parsedConcurrentMaxAttempts.value));
    setCodexReasoningGuardModelFallbacksText(
      formatCodexReasoningGuardModelFallbacks(parsedFallbackModels.models)
    );
    setCodexReasoningGuardContinuationMaxOutputTokensText(
      String(parsedContinuationMaxOutputTokens.value)
    );
    setCodexReasoningGuardActiveTemplateId(nextActiveTemplateId);
    setCodexReasoningGuardRuleMode(nextRuleMode);

    const saved = await persistCodexReasoningGuardSettings({
      codex_reasoning_guard_hit_label: normalizedHitLabel,
      codex_reasoning_guard_rule_mode: nextRuleMode,
      codex_reasoning_guard_active_template_id: nextActiveTemplateId,
      codex_reasoning_guard_custom_templates: codexReasoningGuardCustomTemplates,
      codex_reasoning_guard_post_match_strategy: codexReasoningGuardPostMatchStrategy,
      codex_reasoning_guard_immediate_retry_budget: parsedImmediateBudget.value,
      codex_reasoning_guard_delayed_retry_budget: parsedDelayedBudget.value,
      codex_reasoning_guard_delayed_retry_ms: parsedDelayedMs.value,
      codex_reasoning_guard_exhausted_action: codexReasoningGuardExhaustedAction,
      codex_reasoning_guard_retry_policy: codexReasoningGuardRetryPolicy,
      codex_reasoning_guard_concurrent_max: parsedConcurrentMax.value,
      codex_reasoning_guard_concurrent_interval_ms: parsedConcurrentIntervalMs.value,
      codex_reasoning_guard_concurrent_max_attempts: parsedConcurrentMaxAttempts.value,
      codex_reasoning_guard_model_fallbacks: parsedFallbackModels.models,
      codex_reasoning_guard_continuation_repair_enabled:
        codexReasoningGuardPostMatchStrategy === "continuation_repair",
      codex_reasoning_guard_continuation_max_output_tokens: parsedContinuationMaxOutputTokens.value,
    });
    if (!saved) {
      syncCodexReasoningGuardDrafts(appSettings);
    }
  }

  const codexReasoningGuardModelStats = useMemo(() => {
    return [...(codexReasoningGuardStats?.by_model ?? [])].sort((left, right) => {
      if (left.hit_request_count !== right.hit_request_count) {
        return right.hit_request_count - left.hit_request_count;
      }
      if (left.total_request_count !== right.total_request_count) {
        return right.total_request_count - left.total_request_count;
      }
      return left.requested_model.localeCompare(right.requested_model);
    });
  }, [codexReasoningGuardStats?.by_model]);

  const codexReasoningGuardModelEffortStats = useMemo(() => {
    return [...(codexReasoningGuardStats?.by_model_and_effort ?? [])].sort((left, right) => {
      if (left.hit_request_count !== right.hit_request_count) {
        return right.hit_request_count - left.hit_request_count;
      }
      if (left.total_request_count !== right.total_request_count) {
        return right.total_request_count - left.total_request_count;
      }
      const modelOrder = left.requested_model.localeCompare(right.requested_model);
      if (modelOrder !== 0) {
        return modelOrder;
      }
      return left.reasoning_effort.localeCompare(right.reasoning_effort);
    });
  }, [codexReasoningGuardStats?.by_model_and_effort]);

  const codexReasoningGuardTopHitModel = useMemo(() => {
    return codexReasoningGuardModelStats.find((row) => row.hit_request_count > 0) ?? null;
  }, [codexReasoningGuardModelStats]);

  const codexReasoningContinuationStatusStats = useMemo(() => {
    return [...(codexReasoningGuardStats?.continuation_by_status ?? [])].sort((left, right) => {
      if (left.attempt_count !== right.attempt_count) {
        return right.attempt_count - left.attempt_count;
      }
      if (left.request_count !== right.request_count) {
        return right.request_count - left.request_count;
      }
      return left.status.localeCompare(right.status);
    });
  }, [codexReasoningGuardStats?.continuation_by_status]);

  return (
    <div className="space-y-6">
      <Card className="overflow-hidden">
        <div className="border-b border-border">
          <div className="flex flex-col gap-4 p-6">
            <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
              <div className="flex items-center gap-4">
                <div className="h-14 w-14 rounded-xl bg-card/5 dark:bg-secondary flex items-center justify-center text-secondary-foreground">
                  <Terminal className="h-8 w-8" />
                </div>
                <div>
                  <h2 className="text-base font-semibold text-foreground">Codex</h2>
                  <div className="flex items-center gap-2 mt-1">
                    {codexAvailable === "available" && codexInfo?.found ? (
                      <>
                        <span className="inline-flex items-center gap-1.5 rounded-full bg-green-50 dark:bg-green-900/30 px-2.5 py-0.5 text-xs font-medium text-green-700 dark:text-green-400 ring-1 ring-inset ring-green-600/20">
                          <CheckCircle2 className="h-3 w-3" />
                          已安装 {codexInfo.version}
                        </span>
                        <CliVersionBadge
                          cliKey="codex"
                          installedVersion={codexInfo.version}
                          refreshToken={versionRefreshToken}
                          onUpdateComplete={refreshCodexStatus}
                        />
                      </>
                    ) : codexAvailable === "checking" || loading ? (
                      <span className="inline-flex items-center gap-1.5 rounded-full bg-blue-50 dark:bg-blue-900/30 px-2.5 py-0.5 text-xs font-medium text-blue-700 dark:text-blue-400 ring-1 ring-inset ring-blue-600/20">
                        <RefreshCw className="h-3 w-3 animate-spin" />
                        加载中...
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1.5 rounded-full bg-secondary px-2.5 py-0.5 text-xs font-medium text-muted-foreground ring-1 ring-inset ring-border">
                        未检测到
                      </span>
                    )}
                  </div>
                </div>
              </div>

              <Button
                onClick={() => void refreshCodexStatus()}
                variant="secondary"
                size="sm"
                disabled={loading}
                className="gap-2"
              >
                <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
                刷新
              </Button>
            </div>

            {codexConfig && (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3 mt-2">
                <div className="bg-secondary rounded-lg p-3 border border-border">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-1.5">
                    <FolderOpen className="h-3 w-3" />
                    当前 .codex 目录
                  </div>
                  <div className="flex items-center gap-1.5">
                    <div
                      className="font-mono text-xs text-secondary-foreground truncate flex-1"
                      title={codexConfig.config_dir}
                    >
                      {codexConfig.config_dir}
                    </div>
                    <Button
                      onClick={() => void openCodexConfigDir()}
                      disabled={!codexConfig.can_open_config_dir}
                      size="sm"
                      variant="ghost"
                      className="shrink-0 h-6 w-6 p-0 hover:bg-muted dark:hover:bg-secondary"
                      title={
                        codexConfig.can_open_config_dir
                          ? "打开当前生效目录"
                          : "受权限限制，无法自动打开该目录"
                      }
                    >
                      <ExternalLink className="h-3 w-3" />
                    </Button>
                  </div>
                  <div className="mt-1 text-[11px] text-muted-foreground">
                    {activeConfigDirSummaryText}
                  </div>
                  {!codexConfig.can_open_config_dir ? (
                    <div className="mt-1 text-[11px] text-amber-700 dark:text-amber-400">
                      受权限限制，无法自动打开该目录；请手动打开该路径。
                    </div>
                  ) : null}
                </div>

                <div className="bg-secondary rounded-lg p-3 border border-border">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-1.5">
                    <FileJson className="h-3 w-3" />
                    config.toml
                  </div>
                  <div
                    className="font-mono text-xs text-secondary-foreground truncate"
                    title={codexConfig.config_path}
                  >
                    {codexConfig.config_path}
                  </div>
                  <div className="mt-1 text-[11px] text-muted-foreground">
                    {codexConfig.exists ? "已存在" : "不存在（将自动创建）"}
                  </div>
                </div>

                <div className="bg-secondary rounded-lg p-3 border border-border">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-1.5">
                    <Terminal className="h-3 w-3" />
                    可执行文件
                  </div>
                  <div
                    className="font-mono text-xs text-secondary-foreground truncate"
                    title={codexInfo?.executable_path ?? "—"}
                  >
                    {codexInfo?.executable_path ?? "—"}
                  </div>
                </div>

                <div className="bg-secondary rounded-lg p-3 border border-border">
                  <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-1.5">
                    <Settings className="h-3 w-3" />
                    解析方式
                  </div>
                  <div
                    className="font-mono text-xs text-secondary-foreground truncate"
                    title={codexInfo?.resolved_via ?? "—"}
                  >
                    {codexInfo?.resolved_via ?? "—"}
                  </div>
                  <div className="mt-1 text-[11px] text-muted-foreground">
                    SHELL: {codexInfo?.shell ?? "—"}
                  </div>
                </div>
              </div>
            )}

            {codexConfig && isWindowsRuntime() ? (
              <div className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20">
                <div className="flex flex-col gap-4">
                  <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-foreground">Windows 本机配置</div>
                      <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                        仅影响 Windows 本机上的 Codex 用户级{" "}
                        <span className="font-mono">.codex</span> 目录，不改写 WSL 内各 distro 的
                        目标路径。
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <span className="inline-flex items-center rounded-full border border-border bg-secondary px-2.5 py-1 text-[11px] font-medium text-secondary-foreground dark:border-border dark:bg-secondary dark:text-foreground">
                        {activeConfigModeBadgeText}
                      </span>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={resetConfigLocation}
                        disabled={
                          configLocationControlsDisabled ||
                          (configLocationMode === "user_home_default" &&
                            customHomeText.trim().length === 0)
                        }
                      >
                        恢复默认
                      </Button>
                    </div>
                  </div>

                  <div className="rounded-lg border border-border/70 bg-secondary/80 p-3 dark:border-border dark:bg-secondary/80">
                    <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
                      <div className="min-w-0">
                        <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                          当前会使用
                        </div>
                        <div
                          className="mt-1 break-all font-mono text-xs text-secondary-foreground"
                          title={activeConfigDirPrimaryText}
                        >
                          {activeConfigDirPrimaryText}
                        </div>
                        <div className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                          {configLocationSummaryText}
                        </div>
                      </div>

                      <div className="min-w-0 md:max-w-[320px]">
                        <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                          config.toml
                        </div>
                        <div
                          className="mt-1 break-all font-mono text-xs text-secondary-foreground"
                          title={codexConfig.config_path}
                        >
                          {codexConfig.config_path}
                        </div>
                        <div className="mt-1 text-[11px] text-muted-foreground">
                          {activeConfigDirSummaryText}
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="rounded-lg border border-border/70 bg-white/70 p-3 dark:border-border dark:bg-card/20">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      目录来源
                    </div>
                    <div className="mt-2">
                      <RadioGroup
                        name="codex_config_location_mode"
                        value={configLocationMode}
                        onChange={(value) =>
                          handleConfigLocationModeChange(
                            value === "follow_codex_home"
                              ? "follow_codex_home"
                              : value === "custom"
                                ? "custom"
                                : "user_home_default"
                          )
                        }
                        options={[
                          { value: "user_home_default", label: "固定到 Windows 用户目录" },
                          { value: "follow_codex_home", label: followModeLabel },
                          { value: "custom", label: "手动指定目录" },
                        ]}
                        disabled={configLocationControlsDisabled}
                      />
                    </div>
                    <div className="mt-2 space-y-1 text-[11px] leading-relaxed text-muted-foreground">
                      <div>
                        固定目录：
                        <span className="ml-1 font-mono">{userDefaultResolvedHomeDir}</span>
                      </div>
                      <div>
                        <span className="font-mono">$CODEX_HOME</span> 当前解析：
                        <span className="ml-1 font-mono">{followCodexHomeResolvedDir}</span>
                        {followModeMatchesDefault ? (
                          <span className="ml-2 text-amber-700 dark:text-amber-400">
                            当前路径相同，但后续会随 $CODEX_HOME 变化。
                          </span>
                        ) : null}
                      </div>
                    </div>
                  </div>

                  {configLocationMode === "custom" ? (
                    <div className="rounded-lg border border-border/70 bg-secondary/80 p-3 dark:border-border dark:bg-secondary/80">
                      <label className="text-xs font-medium text-secondary-foreground">
                        自定义 .codex 目录
                      </label>

                      <div className="mt-3 flex flex-col gap-2 lg:flex-row">
                        <Input
                          value={customHomeText}
                          onChange={(e) => {
                            const next = e.currentTarget.value;
                            setCustomHomeText(next);
                            if (configLocationError) {
                              setConfigLocationError(validateCustomCodexHome(next));
                            }
                          }}
                          onBlur={() => {
                            if (configLocationMode !== "custom") return;
                            void persistConfigLocation("custom", customHomeText).then((result) => {
                              if (result === "persist_failed") {
                                restoreSavedConfigLocationState();
                              }
                            });
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") e.currentTarget.blur();
                          }}
                          placeholder="例如：D:\\Users\\you\\.codex"
                          className={cn(
                            "font-mono text-xs lg:flex-1",
                            configLocationError &&
                              "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                          )}
                          disabled={configLocationControlsDisabled}
                        />

                        <div className="flex flex-wrap gap-2">
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => void handlePickCustomHome()}
                            disabled={configLocationControlsDisabled}
                          >
                            <FolderOpen className="mr-1.5 h-3.5 w-3.5" />
                            {selectingCodexHomeDir ? "选择中..." : "选择目录"}
                          </Button>
                        </div>
                      </div>

                      <div
                        className={cn(
                          "mt-2 text-[11px] leading-relaxed",
                          configLocationError
                            ? "text-rose-600 dark:text-rose-400"
                            : "text-muted-foreground"
                        )}
                      >
                        {configLocationError
                          ? configLocationError
                          : configLocationPreviewPath
                            ? `保存后将使用 ${configLocationPreviewPath}。支持普通 Windows 路径、UNC 路径，也可以点“选择目录”。`
                            : "请输入一个 .codex 目录路径，然后按 Enter、移出输入框，或直接使用目录选择器保存。"}
                      </div>
                    </div>
                  ) : (
                    <div className="rounded-lg border border-dashed border-border/80 bg-secondary/50 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground dark:border-border dark:bg-secondary/40 dark:text-muted-foreground">
                      {configLocationMode === "follow_codex_home"
                        ? `当前为跟随模式，手动目录选择器已收起；现在会使用 ${followCodexHomeResolvedDir}。`
                        : `当前为默认模式，手动目录选择器已收起；固定使用 ${userDefaultResolvedHomeDir}。`}
                    </div>
                  )}
                </div>
              </div>
            ) : null}

            <div className="text-xs text-muted-foreground">
              提示：Codex 还会读取 Team Config（例如 repo 内 `.codex/`），其优先级可能高于这里的
              用户级目录设置。
            </div>

            {appSettings ? (
              <div className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20">
                <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                  <div className="min-w-0">
                    <div className="text-sm font-semibold text-foreground">OAuth 兼容代理模式</div>
                    <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                      开启后，AIO 接管 Codex 代理时只写入{" "}
                      <span className="font-mono">config.toml</span> 的 AIO
                      provider，不创建、不备份、 不恢复 <span className="font-mono">auth.json</span>
                      。适合继续使用 Codex 自己的 ChatGPT/OAuth 登录状态。
                    </div>
                    <div className="mt-2 text-[11px] leading-relaxed text-amber-700 dark:text-amber-400">
                      该模式不会写入{" "}
                      <span className="font-mono">preferred_auth_method = "chatgpt"</span>
                      ；会在配置中保留
                      <span className="font-mono"> requires_openai_auth = true</span>。
                    </div>
                  </div>
                  <Switch
                    aria-label="切换 Codex OAuth 兼容代理模式"
                    checked={appSettings.codex_oauth_compatible_proxy_mode}
                    onCheckedChange={(checked) =>
                      void persistCodexOauthCompatibleProxyMode?.(checked)
                    }
                    disabled={proxyModeControlsDisabled}
                  />
                </div>
              </div>
            ) : null}

            {appSettings ? (
              <div className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20">
                <div className="flex flex-col gap-4">
                  <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-foreground">降智拦截</div>
                      <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                        支持按 <span className="font-mono">reasoning_tokens</span> 或{" "}
                        <span className="font-mono">final-answer-only high/xhigh</span>{" "}
                        特征拦截；命中后按策略执行思考续写或自动重试，并且不计入熔断。
                      </div>
                      <div className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
                        当前统计：{codexReasoningGuardStatsRangeLabel}，
                        {codexReasoningGuardStatsRangeDescription}
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center gap-2 self-start">
                      {renderCodexReasoningGuardStatsRangeControls()}
                      <Button
                        variant="secondary"
                        size="sm"
                        className="gap-2"
                        aria-label="查看降智拦截详情"
                        onClick={() => {
                          setCodexReasoningGuardDetailsOpen(true);
                        }}
                      >
                        <BarChart3 className="h-3.5 w-3.5" />
                        详情
                      </Button>
                      <Switch
                        aria-label="切换 Codex 降智拦截"
                        checked={appSettings.codex_reasoning_guard_enabled}
                        onCheckedChange={(checked) =>
                          void persistCodexReasoningGuardSettings?.({
                            codex_reasoning_guard_enabled: checked,
                          })
                        }
                        disabled={reasoningGuardControlsDisabled}
                      />
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
                    <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                        总请求数
                      </div>
                      <div className="mt-1 text-2xl font-semibold text-foreground">
                        {codexReasoningGuardStatsLoading
                          ? "..."
                          : String(codexReasoningGuardStats?.total_request_count ?? 0)}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                        命中请求数
                      </div>
                      <div className="mt-1 text-2xl font-semibold text-foreground">
                        {codexReasoningGuardStatsLoading
                          ? "..."
                          : String(codexReasoningGuardStats?.hit_request_count ?? 0)}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                        命中次数
                      </div>
                      <div className="mt-1 text-2xl font-semibold text-foreground">
                        {codexReasoningGuardStatsLoading
                          ? "..."
                          : String(codexReasoningGuardStats?.hit_attempt_count ?? 0)}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                        降智命中率
                      </div>
                      <div className="mt-1 text-2xl font-semibold text-foreground">
                        {codexReasoningGuardStatsLoading
                          ? "..."
                          : formatCodexReasoningGuardHitRate(codexReasoningGuardStats?.hit_rate)}
                      </div>
                      <div className="mt-1 text-[11px] text-muted-foreground">
                        降智 {codexReasoningGuardStats?.hit_request_count ?? 0} / 正常{" "}
                        {codexReasoningGuardStats?.normal_request_count ?? 0}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                        续写成功率
                      </div>
                      <div className="mt-1 text-2xl font-semibold text-foreground">
                        {codexReasoningGuardStatsLoading
                          ? "..."
                          : formatCodexReasoningGuardHitRate(
                              codexReasoningGuardStats?.continuation_repair_rate
                            )}
                      </div>
                      <div className="mt-1 text-[11px] text-muted-foreground">
                        续写 {codexReasoningGuardStats?.continuation_repaired_request_count ?? 0} /{" "}
                        {codexReasoningGuardStats?.continuation_triggered_request_count ?? 0}
                      </div>
                    </div>
                  </div>

                  <div className="grid gap-2 rounded-lg border border-border/70 bg-secondary/80 p-3 text-xs text-muted-foreground dark:border-border dark:bg-secondary/80">
                    <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                      <span>当前模板</span>
                      <span className="font-mono text-secondary-foreground">
                        {resolveCodexReasoningGuardTemplateOption(
                          appSettings.codex_reasoning_guard_active_template_id,
                          appSettings.codex_reasoning_guard_custom_templates
                        )?.name ?? CODEX_REASONING_GUARD_BUILTIN_TEMPLATES[0].name}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                      <span>规则模式</span>
                      <span className="font-mono text-secondary-foreground">
                        {formatCodexReasoningGuardRuleModeLabel(
                          appSettings.codex_reasoning_guard_rule_mode
                        )}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                      <span>最高命中模型</span>
                      <span className="text-secondary-foreground">
                        {codexReasoningGuardTopHitModel
                          ? `${codexReasoningGuardTopHitModel.requested_model} · ${formatCodexReasoningGuardHitRate(
                              codexReasoningGuardTopHitModel.hit_rate
                            )}`
                          : "暂无命中"}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                      <span>Guard 策略</span>
                      <span className="font-mono text-secondary-foreground">
                        {appSettings.codex_reasoning_guard_post_match_strategy ===
                        "continuation_repair"
                          ? `${formatCodexReasoningGuardPostMatchStrategyLabel(
                              appSettings.codex_reasoning_guard_post_match_strategy
                            )} / ${appSettings.codex_reasoning_guard_immediate_retry_budget} / ${formatCodexReasoningGuardExhaustedActionLabel(
                              appSettings.codex_reasoning_guard_exhausted_action
                            )}`
                          : `${formatCodexReasoningGuardPostMatchStrategyLabel(
                              appSettings.codex_reasoning_guard_post_match_strategy
                            )} / ${formatCodexReasoningGuardRetryPolicyLabel(
                              appSettings.codex_reasoning_guard_retry_policy
                            )} / ${appSettings.codex_reasoning_guard_immediate_retry_budget}+${appSettings.codex_reasoning_guard_delayed_retry_budget} / ${appSettings.codex_reasoning_guard_delayed_retry_ms}ms / ${formatCodexReasoningGuardExhaustedActionLabel(
                              appSettings.codex_reasoning_guard_exhausted_action
                            )}`}
                      </span>
                    </div>
                    {appSettings.codex_reasoning_guard_post_match_strategy ===
                      "retry_same_provider" &&
                    appSettings.codex_reasoning_guard_retry_policy === "concurrent" ? (
                      <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                        <span>并发重试</span>
                        <span className="font-mono text-secondary-foreground">
                          {`max=${appSettings.codex_reasoning_guard_concurrent_max} / interval=${appSettings.codex_reasoning_guard_concurrent_interval_ms}ms / attempts=${appSettings.codex_reasoning_guard_concurrent_max_attempts}`}
                        </span>
                      </div>
                    ) : null}
                    {appSettings.codex_reasoning_guard_exhausted_action === "switch_model" ? (
                      <div className="flex flex-col gap-1 md:flex-row md:items-center md:justify-between">
                        <span>模型回退</span>
                        <span className="font-mono text-secondary-foreground">
                          {appSettings.codex_reasoning_guard_model_fallbacks.length > 0
                            ? appSettings.codex_reasoning_guard_model_fallbacks.join(" -> ")
                            : "未配置"}
                        </span>
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            ) : null}
            <Dialog
              open={codexReasoningGuardDetailsOpen}
              onOpenChange={(next) => {
                setCodexReasoningGuardDetailsOpen(next);
                if (!next) {
                  setCodexReasoningGuardStatsRangePopoverScope((current) =>
                    current === "details" ? null : current
                  );
                  syncCodexReasoningGuardDrafts(appSettings);
                }
              }}
              title="降智拦截详情"
              description="外层页面只保留总览；完整规则编辑和按模型统计放在这里。"
              className="max-w-5xl"
            >
              <div className="space-y-4">
                <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
                  <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      总请求数
                    </div>
                    <div className="mt-1 text-2xl font-semibold text-foreground">
                      {codexReasoningGuardStatsLoading
                        ? "..."
                        : String(codexReasoningGuardStats?.total_request_count ?? 0)}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      命中请求数
                    </div>
                    <div className="mt-1 text-2xl font-semibold text-foreground">
                      {codexReasoningGuardStatsLoading
                        ? "..."
                        : String(codexReasoningGuardStats?.hit_request_count ?? 0)}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      命中次数
                    </div>
                    <div className="mt-1 text-2xl font-semibold text-foreground">
                      {codexReasoningGuardStatsLoading
                        ? "..."
                        : String(codexReasoningGuardStats?.hit_attempt_count ?? 0)}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      降智命中率
                    </div>
                    <div className="mt-1 text-2xl font-semibold text-foreground">
                      {codexReasoningGuardStatsLoading
                        ? "..."
                        : formatCodexReasoningGuardHitRate(codexReasoningGuardStats?.hit_rate)}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border/70 bg-secondary/70 p-3">
                    <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                      续写成功率
                    </div>
                    <div className="mt-1 text-2xl font-semibold text-foreground">
                      {codexReasoningGuardStatsLoading
                        ? "..."
                        : formatCodexReasoningGuardHitRate(
                            codexReasoningGuardStats?.continuation_repair_rate
                          )}
                    </div>
                  </div>
                </div>

                <div className="inline-flex rounded-lg border border-border bg-secondary/40 p-1">
                  <button
                    type="button"
                    className={cn(
                      "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                      codexReasoningGuardDetailsTab === "rules"
                        ? "bg-background text-foreground shadow-sm"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                    onClick={() => setCodexReasoningGuardDetailsTab("rules")}
                  >
                    规则
                  </button>
                  <button
                    type="button"
                    className={cn(
                      "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                      codexReasoningGuardDetailsTab === "stats"
                        ? "bg-background text-foreground shadow-sm"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                    onClick={() => setCodexReasoningGuardDetailsTab("stats")}
                  >
                    统计
                  </button>
                </div>

                <div className="flex flex-col gap-2 rounded-lg border border-border/70 bg-secondary/60 p-3 text-xs text-muted-foreground md:flex-row md:items-center md:justify-between">
                  <div>
                    <span className="font-medium text-secondary-foreground">
                      {codexReasoningGuardStatsRangeLabel}
                    </span>
                    <span className="ml-2">{codexReasoningGuardStatsRangeDescription}</span>
                  </div>
                  <div className="shrink-0">
                    {renderCodexReasoningGuardStatsRangeControls("", {
                      popoverPortalled: false,
                      scope: "details",
                    })}
                  </div>
                </div>

                {codexReasoningGuardDetailsTab === "rules" ? (
                  <div className="space-y-4">
                    <div className="rounded-lg border border-border/70 bg-secondary/60 p-4">
                      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                        <div>
                          <div className="text-sm font-semibold text-foreground">规则模板</div>
                          <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                            当前请求开始时会固定读取一次 active
                            模板；进行中的请求不会被这里的草稿改动影响。
                          </div>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          <Button
                            variant="secondary"
                            size="sm"
                            className="gap-2"
                            onClick={addCodexReasoningGuardCustomTemplate}
                            disabled={
                              reasoningGuardControlsDisabled ||
                              codexReasoningGuardCustomTemplates.length >=
                                MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES
                            }
                          >
                            <Plus className="h-3.5 w-3.5" />
                            新建模板
                          </Button>
                          <Button
                            variant="secondary"
                            size="sm"
                            className="gap-2"
                            onClick={copySelectedCodexReasoningGuardTemplate}
                            disabled={
                              reasoningGuardControlsDisabled ||
                              codexReasoningGuardCustomTemplates.length >=
                                MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES
                            }
                          >
                            <FileJson className="h-3.5 w-3.5" />
                            复制为自定义
                          </Button>
                          <Button
                            variant="secondary"
                            size="sm"
                            aria-label="删除模板"
                            onClick={deleteSelectedCodexReasoningGuardTemplate}
                            disabled={
                              reasoningGuardControlsDisabled ||
                              codexReasoningGuardSelectedTemplate.readOnly
                            }
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      </div>

                      <div className="mt-4 grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                        <label className="text-xs font-medium text-secondary-foreground">
                          <span className="block">active 模板</span>
                          <Select
                            aria-label="规则模板"
                            value={codexReasoningGuardSelectedTemplate.id}
                            onChange={(e) =>
                              selectCodexReasoningGuardTemplate(e.currentTarget.value)
                            }
                            disabled={reasoningGuardControlsDisabled}
                            className="mt-3 font-mono text-xs"
                          >
                            {codexReasoningGuardTemplateOptions.map((template) => (
                              <option key={template.id} value={template.id}>
                                {template.source === "custom" ? "custom" : template.source}:{" "}
                                {template.name}
                              </option>
                            ))}
                          </Select>
                        </label>
                        <div className="rounded-lg border border-border/70 bg-background/60 p-3 text-xs leading-relaxed text-muted-foreground">
                          <div className="font-medium text-secondary-foreground">
                            {codexReasoningGuardSelectedTemplate.name}
                          </div>
                          <div className="mt-1">
                            {codexReasoningGuardSelectedTemplate.description || "无描述"}
                          </div>
                          <div className="mt-2 font-mono text-[11px]">
                            {codexReasoningGuardSelectedTemplate.id}
                          </div>
                        </div>
                      </div>

                      <label className="mt-4 block text-xs font-medium text-secondary-foreground">
                        <span className="block">降智命中标签</span>
                        <Input
                          aria-label="降智命中标签"
                          value={codexReasoningGuardHitLabelText}
                          onChange={(e) =>
                            setCodexReasoningGuardHitLabelText(e.currentTarget.value)
                          }
                          placeholder="降智命中"
                          className="mt-3 text-xs"
                          disabled={reasoningGuardControlsDisabled}
                        />
                      </label>

                      <div className="mt-4 space-y-4">
                        {codexReasoningGuardSelectedTemplate.readOnly ? (
                          <div className="rounded-lg border border-border/70 bg-background/60 p-3 text-xs leading-relaxed text-muted-foreground">
                            内置模板只读，复制为自定义模板后可编辑规则。
                          </div>
                        ) : (
                          <div className="grid gap-3 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1fr)]">
                            <label className="text-xs font-medium text-secondary-foreground">
                              <span className="block">模板 ID</span>
                              <Input
                                aria-label="模板 ID"
                                value={codexReasoningGuardSelectedTemplate.id}
                                onChange={(e) =>
                                  updateSelectedCodexReasoningGuardTemplate({
                                    id: e.currentTarget.value,
                                  })
                                }
                                className="mt-3 font-mono text-xs"
                                disabled={reasoningGuardControlsDisabled}
                              />
                            </label>
                            <label className="text-xs font-medium text-secondary-foreground">
                              <span className="block">模板名称</span>
                              <Input
                                aria-label="模板名称"
                                value={codexReasoningGuardSelectedTemplate.name}
                                onChange={(e) =>
                                  updateSelectedCodexReasoningGuardTemplate({
                                    name: e.currentTarget.value,
                                  })
                                }
                                className="mt-3 text-xs"
                                disabled={reasoningGuardControlsDisabled}
                              />
                            </label>
                            <label className="text-xs font-medium text-secondary-foreground lg:col-span-2">
                              <span className="block">模板描述</span>
                              <Textarea
                                aria-label="模板描述"
                                value={codexReasoningGuardSelectedTemplate.description}
                                onChange={(e) =>
                                  updateSelectedCodexReasoningGuardTemplate({
                                    description: e.currentTarget.value,
                                  })
                                }
                                className="mt-3 min-h-20 text-xs"
                                disabled={reasoningGuardControlsDisabled}
                              />
                            </label>
                          </div>
                        )}

                        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                          <div className="text-sm font-semibold text-foreground">模板规则</div>
                          <Button
                            variant="secondary"
                            size="sm"
                            className="gap-2"
                            onClick={addCodexReasoningGuardTemplateRule}
                            disabled={
                              reasoningGuardControlsDisabled ||
                              codexReasoningGuardSelectedTemplate.readOnly ||
                              codexReasoningGuardSelectedTemplate.rules.length >=
                                MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES
                            }
                          >
                            <Plus className="h-3.5 w-3.5" />
                            新增规则
                          </Button>
                        </div>

                        <div className="space-y-3">
                          {codexReasoningGuardSelectedTemplate.rules.map((rule, ruleIndex) => (
                            <div
                              key={`${rule.id || "rule"}-${ruleIndex}`}
                              className="rounded-lg border border-border/70 bg-background/70 p-3"
                            >
                              <div className="grid gap-3 xl:grid-cols-[minmax(0,0.8fr)_minmax(0,1fr)_150px_120px_160px_auto]">
                                <label className="text-xs font-medium text-secondary-foreground">
                                  <span className="block">规则 ID</span>
                                  <Input
                                    value={rule.id}
                                    onChange={(e) =>
                                      updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
                                        id: e.currentTarget.value,
                                      })
                                    }
                                    className="mt-3 font-mono text-xs"
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly
                                    }
                                  />
                                </label>
                                <label className="text-xs font-medium text-secondary-foreground">
                                  <span className="block">规则名称</span>
                                  <Input
                                    value={rule.name}
                                    onChange={(e) =>
                                      updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
                                        name: e.currentTarget.value,
                                      })
                                    }
                                    className="mt-3 text-xs"
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly
                                    }
                                  />
                                </label>
                                <label className="text-xs font-medium text-secondary-foreground">
                                  <span className="block">动作</span>
                                  <Select
                                    value={rule.action}
                                    onChange={(e) =>
                                      updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
                                        action: e.currentTarget
                                          .value as CodexReasoningGuardTemplateRule["action"],
                                      })
                                    }
                                    className="mt-3 font-mono text-xs"
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly
                                    }
                                  >
                                    <option value="intercept">intercept</option>
                                    <option value="no_intercept">no_intercept</option>
                                  </Select>
                                </label>
                                <label className="text-xs font-medium text-secondary-foreground">
                                  <span className="block">逻辑</span>
                                  <Select
                                    value={rule.logic}
                                    onChange={(e) =>
                                      updateSelectedCodexReasoningGuardTemplateRule(ruleIndex, {
                                        logic: e.currentTarget
                                          .value as CodexReasoningGuardTemplateRule["logic"],
                                      })
                                    }
                                    className="mt-3 font-mono text-xs"
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly
                                    }
                                  >
                                    <option value="and">AND</option>
                                    <option value="or">OR</option>
                                  </Select>
                                </label>
                                <label className="text-xs font-medium text-secondary-foreground">
                                  <span className="block">token 匹配</span>
                                  <Input
                                    value={
                                      rule.reasoning_tokens_formula ===
                                      "reasoning_tokens_518n_minus_2"
                                        ? "518*N-2"
                                        : rule.reasoning_tokens == null
                                          ? ""
                                          : String(rule.reasoning_tokens)
                                    }
                                    onChange={(e) => {
                                      updateSelectedCodexReasoningGuardTemplateRuleToken(
                                        ruleIndex,
                                        e.currentTarget.value
                                      );
                                    }}
                                    placeholder="空为 wildcard"
                                    className="mt-3 font-mono text-xs"
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly ||
                                      rule.reasoning_tokens_formula != null
                                    }
                                  />
                                </label>
                                <div className="flex items-end gap-1">
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    aria-label={`上移规则 ${ruleIndex + 1}`}
                                    onClick={() =>
                                      moveCodexReasoningGuardTemplateRule(ruleIndex, -1)
                                    }
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly ||
                                      ruleIndex === 0
                                    }
                                  >
                                    ↑
                                  </Button>
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    aria-label={`下移规则 ${ruleIndex + 1}`}
                                    onClick={() =>
                                      moveCodexReasoningGuardTemplateRule(ruleIndex, 1)
                                    }
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly ||
                                      ruleIndex ===
                                        codexReasoningGuardSelectedTemplate.rules.length - 1
                                    }
                                  >
                                    ↓
                                  </Button>
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    aria-label={`删除规则 ${ruleIndex + 1}`}
                                    onClick={() => removeCodexReasoningGuardTemplateRule(ruleIndex)}
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly
                                    }
                                  >
                                    <Trash2 className="h-3.5 w-3.5" />
                                  </Button>
                                </div>
                              </div>

                              <div className="mt-4 space-y-2">
                                <div className="flex items-center justify-between gap-3">
                                  <div className="text-xs font-medium text-secondary-foreground">
                                    过滤条件
                                  </div>
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    className="gap-2"
                                    onClick={() => addCodexReasoningGuardTemplateFilter(ruleIndex)}
                                    disabled={
                                      reasoningGuardControlsDisabled ||
                                      codexReasoningGuardSelectedTemplate.readOnly ||
                                      rule.filters.length >=
                                        MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS
                                    }
                                  >
                                    <Plus className="h-3.5 w-3.5" />
                                    新增条件
                                  </Button>
                                </div>
                                {rule.filters.length === 0 ? (
                                  <div className="rounded-md border border-dashed border-border/70 px-3 py-2 text-xs text-muted-foreground">
                                    无过滤条件。
                                  </div>
                                ) : (
                                  rule.filters.map((filter, filterIndex) => {
                                    const kind = codexReasoningGuardFilterKind(filter.field);
                                    const operators = codexReasoningGuardFilterOperators(
                                      filter.field
                                    );
                                    return (
                                      <div
                                        key={`${filter.id || "filter"}-${filterIndex}`}
                                        className="grid gap-2 rounded-md border border-border/70 bg-secondary/40 p-2 xl:grid-cols-[minmax(0,0.8fr)_minmax(0,1fr)_160px_minmax(0,1fr)_auto]"
                                      >
                                        <Input
                                          aria-label={`条件 ID ${filterIndex + 1}`}
                                          value={filter.id}
                                          onChange={(e) =>
                                            updateCodexReasoningGuardTemplateFilter(
                                              ruleIndex,
                                              filterIndex,
                                              { id: e.currentTarget.value }
                                            )
                                          }
                                          className="font-mono text-xs"
                                          disabled={
                                            reasoningGuardControlsDisabled ||
                                            codexReasoningGuardSelectedTemplate.readOnly
                                          }
                                        />
                                        <Select
                                          aria-label={`条件字段 ${filterIndex + 1}`}
                                          value={filter.field}
                                          onChange={(e) =>
                                            updateCodexReasoningGuardTemplateFilter(
                                              ruleIndex,
                                              filterIndex,
                                              {
                                                field: e.currentTarget
                                                  .value as CodexReasoningGuardTemplateFilterField,
                                              }
                                            )
                                          }
                                          className="font-mono text-xs"
                                          disabled={
                                            reasoningGuardControlsDisabled ||
                                            codexReasoningGuardSelectedTemplate.readOnly
                                          }
                                        >
                                          {CODEX_REASONING_GUARD_FILTER_FIELD_OPTIONS.map(
                                            (option) => (
                                              <option key={option.value} value={option.value}>
                                                {option.label}
                                              </option>
                                            )
                                          )}
                                        </Select>
                                        <Select
                                          aria-label={`条件操作符 ${filterIndex + 1}`}
                                          value={filter.operator}
                                          onChange={(e) =>
                                            updateCodexReasoningGuardTemplateFilter(
                                              ruleIndex,
                                              filterIndex,
                                              {
                                                operator: e.currentTarget
                                                  .value as CodexReasoningGuardTemplateFilterOperator,
                                              }
                                            )
                                          }
                                          className="font-mono text-xs"
                                          disabled={
                                            reasoningGuardControlsDisabled ||
                                            codexReasoningGuardSelectedTemplate.readOnly
                                          }
                                        >
                                          {operators.map((operator) => (
                                            <option key={operator} value={operator}>
                                              {operator}
                                            </option>
                                          ))}
                                        </Select>
                                        {kind === "boolean" ? (
                                          <Select
                                            aria-label={`条件值 ${filterIndex + 1}`}
                                            value={filter.bool_value ? "true" : "false"}
                                            onChange={(e) =>
                                              updateCodexReasoningGuardTemplateFilter(
                                                ruleIndex,
                                                filterIndex,
                                                { bool_value: e.currentTarget.value === "true" }
                                              )
                                            }
                                            className="font-mono text-xs"
                                            disabled={
                                              reasoningGuardControlsDisabled ||
                                              codexReasoningGuardSelectedTemplate.readOnly
                                            }
                                          >
                                            <option value="true">true</option>
                                            <option value="false">false</option>
                                          </Select>
                                        ) : (
                                          <Input
                                            aria-label={`条件值 ${filterIndex + 1}`}
                                            value={
                                              kind === "number"
                                                ? String(filter.number_value ?? "")
                                                : filter.operator === "in" ||
                                                    filter.operator === "not_in"
                                                  ? filter.string_values.join(", ")
                                                  : (filter.string_value ?? "")
                                            }
                                            onChange={(e) => {
                                              const raw = e.currentTarget.value;
                                              if (kind === "number") {
                                                updateCodexReasoningGuardTemplateFilter(
                                                  ruleIndex,
                                                  filterIndex,
                                                  {
                                                    number_value:
                                                      raw.trim() === "" ? null : Number(raw),
                                                  }
                                                );
                                              } else if (
                                                filter.operator === "in" ||
                                                filter.operator === "not_in"
                                              ) {
                                                updateCodexReasoningGuardTemplateFilter(
                                                  ruleIndex,
                                                  filterIndex,
                                                  {
                                                    string_values: raw
                                                      .split(",")
                                                      .map((item) => item.trim())
                                                      .filter(Boolean),
                                                  }
                                                );
                                              } else {
                                                updateCodexReasoningGuardTemplateFilter(
                                                  ruleIndex,
                                                  filterIndex,
                                                  { string_value: raw }
                                                );
                                              }
                                            }}
                                            className="font-mono text-xs"
                                            disabled={
                                              reasoningGuardControlsDisabled ||
                                              codexReasoningGuardSelectedTemplate.readOnly
                                            }
                                          />
                                        )}
                                        <Button
                                          variant="secondary"
                                          size="sm"
                                          aria-label={`删除条件 ${filterIndex + 1}`}
                                          onClick={() =>
                                            removeCodexReasoningGuardTemplateFilter(
                                              ruleIndex,
                                              filterIndex
                                            )
                                          }
                                          disabled={
                                            reasoningGuardControlsDisabled ||
                                            codexReasoningGuardSelectedTemplate.readOnly
                                          }
                                        >
                                          <Trash2 className="h-3.5 w-3.5" />
                                        </Button>
                                      </div>
                                    );
                                  })
                                )}
                              </div>
                            </div>
                          ))}
                        </div>

                        <div
                          className={cn(
                            "text-[11px] leading-relaxed",
                            codexReasoningGuardTemplateError
                              ? "text-rose-600 dark:text-rose-400"
                              : "text-muted-foreground"
                          )}
                        >
                          {codexReasoningGuardTemplateError ??
                            "保存时会校验模板 ID、规则、wildcard、过滤字段和值类型。"}
                        </div>
                      </div>
                    </div>

                    <div className="rounded-lg border border-border/70 bg-secondary/60 p-4">
                      <div className="text-sm font-semibold text-foreground">重试策略与预算</div>
                      <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                        命中模板后先执行命中后策略；策略失败或预算耗尽后执行失败策略。
                      </div>
                      <div className="mt-4 grid gap-3 lg:grid-cols-[220px_minmax(0,1fr)]">
                        <label className="text-xs font-medium text-secondary-foreground">
                          <span className="block">命中后策略</span>
                          <Select
                            value={codexReasoningGuardPostMatchStrategy}
                            onChange={(e) =>
                              setCodexReasoningGuardPostMatchStrategy(
                                e.currentTarget.value as CodexReasoningGuardPostMatchStrategy
                              )
                            }
                            disabled={reasoningGuardControlsDisabled}
                            className="mt-3 font-mono text-xs"
                          >
                            <option value="continuation_repair">思考续写</option>
                            <option value="retry_same_provider">自动重试</option>
                          </Select>
                        </label>
                        <div className="rounded-lg border border-border/70 bg-background/60 p-3 text-[11px] leading-relaxed text-muted-foreground">
                          {codexReasoningGuardPostMatchStrategy === "continuation_repair"
                            ? "命中后使用当前次数预算发送 continuation；不使用等待重试和并发重试。"
                            : "命中后按立即预算、等待预算和并发设置重试同一 provider。"}
                        </div>
                      </div>
                      {codexReasoningGuardPostMatchStrategy === "retry_same_provider" ? (
                        <div className="mt-4 grid gap-3 lg:grid-cols-[220px_minmax(0,1fr)]">
                          <label className="text-xs font-medium text-secondary-foreground">
                            <span className="block">重试策略</span>
                            <Select
                              value={codexReasoningGuardRetryPolicy}
                              onChange={(e) =>
                                setCodexReasoningGuardRetryPolicy(
                                  e.currentTarget.value as CodexReasoningGuardRetryPolicy
                                )
                              }
                              disabled={reasoningGuardControlsDisabled}
                              className="mt-3 font-mono text-xs"
                            >
                              <option value="single">单路重试</option>
                              <option value="concurrent">并发重试</option>
                            </Select>
                          </label>
                          <div className="rounded-lg border border-border/70 bg-background/60 p-3 text-[11px] leading-relaxed text-muted-foreground">
                            {codexReasoningGuardRetryPolicy === "concurrent"
                              ? `遇到降智命中后先走 1 路；如果仍然命中，下一轮升到 2 路，直到最大 ${MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX} 路。任一路拿到非降智响应后继续，其他路会被丢弃。`
                              : "完全沿用现有行为：每次只发起一路重试，按立即预算和等待预算顺序执行。"}
                          </div>
                        </div>
                      ) : null}
                      <div className="mt-4 grid gap-3 lg:grid-cols-4">
                        <label className="text-xs font-medium text-secondary-foreground">
                          <span className="block">
                            {codexReasoningGuardPostMatchStrategy === "continuation_repair"
                              ? "思考续写次数"
                              : "立即重试次数"}
                          </span>
                          <Input
                            type="number"
                            min={0}
                            max={MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET}
                            step={1}
                            value={codexReasoningGuardImmediateBudgetText}
                            onChange={(e) => {
                              setCodexReasoningGuardImmediateBudgetText(e.currentTarget.value);
                              setCodexReasoningGuardBudgetError(null);
                            }}
                            placeholder={String(
                              DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
                            )}
                            className={cn(
                              "mt-3 font-mono text-xs",
                              codexReasoningGuardBudgetError &&
                                "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                            )}
                            disabled={reasoningGuardControlsDisabled}
                          />
                        </label>
                        {codexReasoningGuardPostMatchStrategy === "continuation_repair" ? (
                          <label className="text-xs font-medium text-secondary-foreground">
                            <span className="block">最大 output tokens</span>
                            <Input
                              type="number"
                              min={0}
                              max={MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS}
                              step={1}
                              value={codexReasoningGuardContinuationMaxOutputTokensText}
                              onChange={(e) => {
                                setCodexReasoningGuardContinuationMaxOutputTokensText(
                                  e.currentTarget.value
                                );
                                setCodexReasoningGuardContinuationError(null);
                              }}
                              placeholder={String(
                                DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
                              )}
                              className={cn(
                                "mt-3 font-mono text-xs",
                                codexReasoningGuardContinuationError &&
                                  "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                              )}
                              disabled={reasoningGuardControlsDisabled}
                            />
                          </label>
                        ) : null}
                        {codexReasoningGuardPostMatchStrategy === "retry_same_provider" ? (
                          <>
                            <label className="text-xs font-medium text-secondary-foreground">
                              <span className="block">等待重试次数</span>
                              <Input
                                type="number"
                                min={0}
                                max={MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET}
                                step={1}
                                value={codexReasoningGuardDelayedBudgetText}
                                onChange={(e) => {
                                  setCodexReasoningGuardDelayedBudgetText(e.currentTarget.value);
                                  setCodexReasoningGuardBudgetError(null);
                                }}
                                placeholder={String(
                                  DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
                                )}
                                className={cn(
                                  "mt-3 font-mono text-xs",
                                  codexReasoningGuardBudgetError &&
                                    "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                                )}
                                disabled={reasoningGuardControlsDisabled}
                              />
                            </label>
                            <label className="text-xs font-medium text-secondary-foreground">
                              <span className="block">等待毫秒数</span>
                              <Input
                                type="number"
                                min={0}
                                max={MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS}
                                step={100}
                                value={codexReasoningGuardDelayedMsText}
                                onChange={(e) => {
                                  setCodexReasoningGuardDelayedMsText(e.currentTarget.value);
                                  setCodexReasoningGuardBudgetError(null);
                                }}
                                placeholder={String(DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS)}
                                className={cn(
                                  "mt-3 font-mono text-xs",
                                  codexReasoningGuardBudgetError &&
                                    "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                                )}
                                disabled={reasoningGuardControlsDisabled}
                              />
                            </label>
                          </>
                        ) : null}
                        <label className="text-xs font-medium text-secondary-foreground">
                          <span className="block">预算耗尽后</span>
                          <Select
                            value={codexReasoningGuardExhaustedAction}
                            onChange={(e) =>
                              setCodexReasoningGuardExhaustedAction(
                                e.currentTarget.value as CodexReasoningGuardExhaustedAction
                              )
                            }
                            disabled={reasoningGuardControlsDisabled}
                            className="mt-3 font-mono text-xs"
                          >
                            <option value="return_error">返回错误</option>
                            <option value="switch_provider">切换供应商</option>
                            <option value="switch_model">切换模型</option>
                          </Select>
                        </label>
                      </div>
                      {codexReasoningGuardPostMatchStrategy === "retry_same_provider" &&
                      codexReasoningGuardRetryPolicy === "concurrent" ? (
                        <div className="mt-4 grid gap-3 rounded-lg border border-border/70 bg-background/60 p-3 lg:grid-cols-3">
                          <label className="text-xs font-medium text-secondary-foreground">
                            <span className="block">最大并发数</span>
                            <Input
                              type="number"
                              min={1}
                              max={MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX}
                              step={1}
                              value={codexReasoningGuardConcurrentMaxText}
                              onChange={(e) => {
                                setCodexReasoningGuardConcurrentMaxText(e.currentTarget.value);
                                setCodexReasoningGuardBudgetError(null);
                              }}
                              placeholder={String(DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX)}
                              className={cn(
                                "mt-3 font-mono text-xs",
                                codexReasoningGuardBudgetError &&
                                  "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                              )}
                              disabled={reasoningGuardControlsDisabled}
                            />
                          </label>
                          <label className="text-xs font-medium text-secondary-foreground">
                            <span className="block">并发启动间隔 ms</span>
                            <Input
                              type="number"
                              min={0}
                              max={MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS}
                              step={100}
                              value={codexReasoningGuardConcurrentIntervalMsText}
                              onChange={(e) => {
                                setCodexReasoningGuardConcurrentIntervalMsText(
                                  e.currentTarget.value
                                );
                                setCodexReasoningGuardBudgetError(null);
                              }}
                              placeholder={String(
                                DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
                              )}
                              className={cn(
                                "mt-3 font-mono text-xs",
                                codexReasoningGuardBudgetError &&
                                  "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                              )}
                              disabled={reasoningGuardControlsDisabled}
                            />
                          </label>
                          <label className="text-xs font-medium text-secondary-foreground">
                            <span className="block">最大尝试次数</span>
                            <Input
                              type="number"
                              min={0}
                              max={MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS}
                              step={1}
                              value={codexReasoningGuardConcurrentMaxAttemptsText}
                              onChange={(e) => {
                                setCodexReasoningGuardConcurrentMaxAttemptsText(
                                  e.currentTarget.value
                                );
                                setCodexReasoningGuardBudgetError(null);
                              }}
                              placeholder={String(
                                DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
                              )}
                              className={cn(
                                "mt-3 font-mono text-xs",
                                codexReasoningGuardBudgetError &&
                                  "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                              )}
                              disabled={reasoningGuardControlsDisabled}
                            />
                          </label>
                        </div>
                      ) : null}
                      {codexReasoningGuardExhaustedAction === "switch_model" ? (
                        <label className="mt-4 block text-xs font-medium text-secondary-foreground">
                          <span className="block">模型回退优先级</span>
                          <Textarea
                            aria-label="模型回退优先级"
                            value={codexReasoningGuardModelFallbacksText}
                            onChange={(e) => {
                              setCodexReasoningGuardModelFallbacksText(e.currentTarget.value);
                              setCodexReasoningGuardModelFallbacksError(null);
                            }}
                            placeholder={"gpt-5.4\ngpt-5.4-mini"}
                            className={cn(
                              "mt-3 min-h-24 font-mono text-xs",
                              codexReasoningGuardModelFallbacksError &&
                                "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                            )}
                            disabled={reasoningGuardControlsDisabled}
                          />
                          <span
                            className={cn(
                              "mt-2 block text-[11px] leading-relaxed",
                              codexReasoningGuardModelFallbacksError
                                ? "text-rose-600 dark:text-rose-400"
                                : "text-muted-foreground"
                            )}
                          >
                            {codexReasoningGuardModelFallbacksError ??
                              `每行一个模型，按从高到低优先级切换；最多 ${MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN} 个。切换仅影响当前请求，下一次请求仍使用原模型。`}
                          </span>
                        </label>
                      ) : null}
                      <div
                        className={cn(
                          "mt-2 text-[11px] leading-relaxed",
                          codexReasoningGuardBudgetError || codexReasoningGuardContinuationError
                            ? "text-rose-600 dark:text-rose-400"
                            : "text-muted-foreground"
                        )}
                      >
                        {codexReasoningGuardBudgetError ??
                          codexReasoningGuardContinuationError ??
                          (codexReasoningGuardPostMatchStrategy === "continuation_repair"
                            ? `默认 ${DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET} 次思考续写；output tokens 为 ${DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS} 时不设额外上限。`
                            : `默认 ${DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET} 次立即重试 + ${DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET} 次等待 ${DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS}ms，耗尽后返回 GW_CODEX_REASONING_GUARD。`)}
                      </div>
                      <div className="mt-4 flex justify-end">
                        <Button
                          size="sm"
                          className="gap-2"
                          onClick={() => void saveCodexReasoningGuardRules()}
                          disabled={reasoningGuardControlsDisabled}
                        >
                          <Settings className="h-3.5 w-3.5" />
                          保存规则
                        </Button>
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="space-y-4">
                    <div className="rounded-lg border border-border/70 bg-secondary/60 p-4 text-xs text-muted-foreground">
                      共统计 {codexReasoningGuardStats?.total_request_count ?? 0} 个 Codex
                      请求；命中降智 {codexReasoningGuardStats?.hit_request_count ?? 0} 次，不命中{" "}
                      {codexReasoningGuardStats?.normal_request_count ?? 0} 次。
                    </div>
                    {codexReasoningGuardModelStats.length === 0 ? (
                      <div className="rounded-lg border border-dashed border-border/70 bg-background/60 px-4 py-5 text-center text-xs text-muted-foreground">
                        还没有可展示的 Codex 请求统计。
                      </div>
                    ) : (
                      <div className="space-y-4">
                        <div className="overflow-x-auto rounded-lg border border-border/70">
                          <table className="min-w-full divide-y divide-border text-sm">
                            <thead className="bg-secondary/50 text-left text-xs uppercase tracking-wide text-muted-foreground">
                              <tr>
                                <th className="px-3 py-2 font-medium">模型</th>
                                <th className="px-3 py-2 font-medium">思考等级</th>
                                <th className="px-3 py-2 font-medium">命中请求</th>
                                <th className="px-3 py-2 font-medium">正常请求</th>
                                <th className="px-3 py-2 font-medium">命中率</th>
                                <th className="px-3 py-2 font-medium">命中次数</th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-border bg-background/80">
                              {codexReasoningGuardModelEffortStats.map((row) => (
                                <tr key={`${row.requested_model}:${row.reasoning_effort}`}>
                                  <td className="px-3 py-2 font-mono text-xs text-secondary-foreground">
                                    {row.requested_model}
                                  </td>
                                  <td className="px-3 py-2 font-mono text-xs text-secondary-foreground">
                                    {row.reasoning_effort}
                                  </td>
                                  <td className="px-3 py-2">{row.hit_request_count}</td>
                                  <td className="px-3 py-2">{row.normal_request_count}</td>
                                  <td className="px-3 py-2">
                                    {formatCodexReasoningGuardHitRate(row.hit_rate)}
                                  </td>
                                  <td className="px-3 py-2">{row.hit_attempt_count}</td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>

                        <div className="overflow-x-auto rounded-lg border border-border/70">
                          <table className="min-w-full divide-y divide-border text-sm">
                            <thead className="bg-secondary/50 text-left text-xs uppercase tracking-wide text-muted-foreground">
                              <tr>
                                <th className="px-3 py-2 font-medium">模型汇总</th>
                                <th className="px-3 py-2 font-medium">命中请求</th>
                                <th className="px-3 py-2 font-medium">正常请求</th>
                                <th className="px-3 py-2 font-medium">命中率</th>
                                <th className="px-3 py-2 font-medium">命中次数</th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-border bg-background/80">
                              {codexReasoningGuardModelStats.map((row) => (
                                <tr key={row.requested_model}>
                                  <td className="px-3 py-2 font-mono text-xs text-secondary-foreground">
                                    {row.requested_model}
                                  </td>
                                  <td className="px-3 py-2">{row.hit_request_count}</td>
                                  <td className="px-3 py-2">{row.normal_request_count}</td>
                                  <td className="px-3 py-2">
                                    {formatCodexReasoningGuardHitRate(row.hit_rate)}
                                  </td>
                                  <td className="px-3 py-2">{row.hit_attempt_count}</td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>

                        <div className="overflow-x-auto rounded-lg border border-border/70">
                          <table className="min-w-full divide-y divide-border text-sm">
                            <thead className="bg-secondary/50 text-left text-xs uppercase tracking-wide text-muted-foreground">
                              <tr>
                                <th className="px-3 py-2 font-medium">续写状态</th>
                                <th className="px-3 py-2 font-medium">触发请求</th>
                                <th className="px-3 py-2 font-medium">触发次数</th>
                                <th className="px-3 py-2 font-medium">平均续写轮数</th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-border bg-background/80">
                              {codexReasoningContinuationStatusStats.length === 0 ? (
                                <tr>
                                  <td
                                    className="px-3 py-4 text-center text-xs text-muted-foreground"
                                    colSpan={4}
                                  >
                                    还没有可展示的思考续写记录。
                                  </td>
                                </tr>
                              ) : (
                                codexReasoningContinuationStatusStats.map((row) => (
                                  <tr key={row.status}>
                                    <td className="px-3 py-2">
                                      <div className="font-medium text-secondary-foreground">
                                        {formatCodexReasoningContinuationStatusLabel(row.status)}
                                      </div>
                                      <div className="font-mono text-[11px] text-muted-foreground">
                                        {row.status}
                                      </div>
                                    </td>
                                    <td className="px-3 py-2">{row.request_count}</td>
                                    <td className="px-3 py-2">{row.attempt_count}</td>
                                    <td className="px-3 py-2">
                                      {formatCodexReasoningGuardDecimal(row.average_sent_rounds)}
                                    </td>
                                  </tr>
                                ))
                              )}
                            </tbody>
                          </table>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </Dialog>
          </div>
        </div>

        {codexAvailable === "unavailable" ? (
          <div className="text-sm text-muted-foreground text-center py-8">数据不可用</div>
        ) : !codexConfig ? (
          <div className="text-sm text-muted-foreground text-center py-8">暂无配置，请尝试刷新</div>
        ) : (
          <div className="p-6 space-y-6">
            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-3">
                <Settings className="h-4 w-4 text-muted-foreground" />
                基础配置
              </h3>
              <div className="divide-y divide-border">
                <SettingItem
                  label="默认模型 (model)"
                  subtitle="设置 Codex 默认使用的模型（例如 gpt-5-codex）。留空表示不设置（交由 Codex 默认/上层配置决定）。"
                >
                  <Input
                    value={modelText}
                    onChange={(e) => setModelText(e.currentTarget.value)}
                    onBlur={() =>
                      void persistCodexConfig(
                        buildModelPatch(modelText, contextWindowText, autoCompactLimitText)
                      )
                    }
                    placeholder="例如：gpt-5-codex"
                    className="font-mono w-[280px] max-w-full"
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="供应商测试默认模型"
                  subtitle={`Codex 供应商做“可用性测试”时使用的全局模型。Provider 编辑页不单独填写时，会回退到这里；默认值是 ${DEFAULT_CODEX_PROVIDER_TEST_MODEL}。`}
                >
                  <Input
                    value={providerTestModelText}
                    onChange={(e) => setProviderTestModelText(e.currentTarget.value)}
                    onBlur={() => void saveProviderTestModel(providerTestModelText)}
                    placeholder={DEFAULT_CODEX_PROVIDER_TEST_MODEL}
                    className="font-mono w-[280px] max-w-full"
                    disabled={providerTestModelControlsDisabled}
                  />
                </SettingItem>

                <SettingItem
                  label="Provider Sync"
                  subtitle="手动同步 Codex 历史到当前受管理的 provider。保存配置或同步进行中时不可重复触发。"
                >
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void syncCodexProvider?.()}
                    disabled={providerSyncControlsDisabled}
                  >
                    <RefreshCw
                      className={cn("h-4 w-4", codexProviderSyncing ? "animate-spin" : "")}
                    />
                    {codexProviderSyncing ? "同步中…" : "手动 Provider Sync"}
                  </Button>
                </SettingItem>

                {showsGpt54LinkedSettings ? (
                  <>
                    <SettingItem
                      label="model_context_window"
                      subtitle={`模型上下文窗口大小。仅当 model=${GPT_54_MODEL} 时生效；切换到其他模型时自动删除。留空则不写入配置，默认参考值 ${GPT_54_CONTEXT_WINDOW.toLocaleString()}。`}
                    >
                      <Input
                        type="number"
                        value={contextWindowText}
                        onChange={(e) => setContextWindowText(e.currentTarget.value)}
                        onBlur={() =>
                          void persistCodexConfig({
                            model_context_window: parsePositiveInt(contextWindowText),
                          })
                        }
                        placeholder={String(GPT_54_CONTEXT_WINDOW)}
                        className="font-mono w-[220px] max-w-full"
                        disabled={saving}
                      />
                    </SettingItem>

                    <SettingItem
                      label="model_auto_compact_token_limit"
                      subtitle={`自动压缩 token 上限。仅当 model=${GPT_54_MODEL} 时生效；切换到其他模型时自动删除。留空则不写入配置，默认参考值 ${GPT_54_AUTO_COMPACT_TOKEN_LIMIT.toLocaleString()}。`}
                    >
                      <Input
                        type="number"
                        value={autoCompactLimitText}
                        onChange={(e) => setAutoCompactLimitText(e.currentTarget.value)}
                        onBlur={() =>
                          void persistCodexConfig({
                            model_auto_compact_token_limit: parsePositiveInt(autoCompactLimitText),
                          })
                        }
                        placeholder={String(GPT_54_AUTO_COMPACT_TOKEN_LIMIT)}
                        className="font-mono w-[220px] max-w-full"
                        disabled={saving}
                      />
                    </SettingItem>
                  </>
                ) : null}

                <SettingItem
                  label="审批策略 (approval_policy)"
                  subtitle="控制何时需要你确认才会执行命令。推荐 on-request（默认）或 on-failure。"
                >
                  <Select
                    value={codexConfig.approval_policy ?? ""}
                    onChange={(e) =>
                      void persistCodexConfig({ approval_policy: e.currentTarget.value })
                    }
                    disabled={saving}
                    className="w-[220px] max-w-full font-mono"
                  >
                    <option value="">默认（不设置）</option>
                    <option value="untrusted">不信任（untrusted）</option>
                    <option value="on-failure">失败时（on-failure）</option>
                    <option value="on-request">请求时（on-request）</option>
                    <option value="never">从不询问（never）</option>
                  </Select>
                </SettingItem>

                <SettingItem
                  label="沙箱模式 (sandbox_mode)"
                  subtitle="控制文件/网络访问策略。danger-full-access 风险极高，仅在完全信任的环境使用。"
                >
                  <Select
                    value={sandboxModeText}
                    onChange={(e) => void saveSandboxMode(e.currentTarget.value)}
                    disabled={saving}
                    className="w-[220px] max-w-full font-mono"
                  >
                    <option value="">默认（不设置）</option>
                    <option value="read-only">只读（read-only）</option>
                    <option value="workspace-write">工作区写入（workspace-write）</option>
                    <option value="danger-full-access">危险：完全访问（danger-full-access）</option>
                  </Select>
                </SettingItem>

                <SettingItem
                  label="推理强度 (model_reasoning_effort)"
                  subtitle="调整推理强度（仅对支持的模型/Responses API 生效）。值越高通常越稳健但更慢。"
                >
                  <RadioGroup
                    name="model_reasoning_effort"
                    value={reasoningEffortText}
                    onChange={(value) => {
                      setReasoningEffortText(value);
                      void persistCodexConfig({ model_reasoning_effort: value });
                    }}
                    options={[
                      { value: "", label: "默认" },
                      { value: "minimal", label: "最低 (minimal)" },
                      { value: "low", label: "低 (low)" },
                      { value: "medium", label: "中 (medium)" },
                      { value: "high", label: "高 (high)" },
                      { value: "xhigh", label: "极高 (xhigh)" },
                    ]}
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="计划模式推理强度 (plan_mode_reasoning_effort)"
                  subtitle="调整计划模式下的推理强度。值越高通常规划越充分但更慢。"
                >
                  <RadioGroup
                    name="plan_mode_reasoning_effort"
                    value={planModeReasoningEffortText}
                    onChange={(value) => {
                      setPlanModeReasoningEffortText(value);
                      void persistCodexConfig({ plan_mode_reasoning_effort: value });
                    }}
                    options={[
                      { value: "", label: "默认" },
                      { value: "low", label: "低 (low)" },
                      { value: "medium", label: "中 (medium)" },
                      { value: "high", label: "高 (high)" },
                      { value: "xhigh", label: "极高 (xhigh)" },
                    ]}
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="网络搜索模式 (web_search)"
                  subtitle="控制 Web Search 工具的行为。cached：使用缓存结果；live：获取最新数据；disabled：禁用。"
                >
                  <RadioGroup
                    name="web_search"
                    value={webSearchText}
                    onChange={(value) => {
                      setWebSearchText(value);
                      void persistCodexConfig({ web_search: value });
                    }}
                    options={[
                      { value: "cached", label: "缓存 (cached)" },
                      { value: "live", label: "实时 (live)" },
                      { value: "disabled", label: "禁用 (disabled)" },
                    ]}
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="输出风格 (personality)"
                  subtitle="控制 web_search 结果的输出风格。pragmatic 更务实，friendly 更友好；none 会删除该配置，交给 Codex 默认行为。"
                >
                  <RadioGroup
                    name="personality"
                    value={personalityText}
                    onChange={(value) => {
                      setPersonalityText(value);
                      void persistCodexConfig(buildPersonalityPatch(value));
                    }}
                    options={[
                      { value: "pragmatic", label: "务实 (pragmatic)" },
                      { value: "friendly", label: "友好 (friendly)" },
                      { value: "none", label: "默认 / 删除配置 (none)" },
                    ]}
                    disabled={saving}
                  />
                </SettingItem>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-3">
                <Settings className="h-4 w-4 text-muted-foreground" />
                Sandbox（workspace-write）
              </h3>
              <div className="divide-y divide-border">
                <SettingItem
                  label="允许联网 (sandbox_workspace_write.network_access)"
                  subtitle="仅在 sandbox_mode=workspace-write 时生效。开启写入 network_access=true；关闭删除该项（不写 false）。"
                >
                  <Switch
                    checked={boolOrDefault(
                      codexConfig.sandbox_workspace_write_network_access,
                      false
                    )}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ sandbox_workspace_write_network_access: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>
              </div>
              {effectiveSandboxMode !== "workspace-write" ? (
                <div className="mt-3 rounded-lg bg-amber-50 dark:bg-amber-900/30 p-3 text-xs text-amber-700 dark:text-amber-400 flex items-start gap-2">
                  <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
                  <div>
                    当前 sandbox_mode 不是 <span className="font-mono">workspace-write</span>
                    ，此分区设置可能不会生效。
                  </div>
                </div>
              ) : null}
            </div>

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-3">
                <Settings className="h-4 w-4 text-muted-foreground" />
                Features（实验/可选能力）
              </h3>
              <div className="divide-y divide-border">
                <SettingItem
                  label="shell_snapshot"
                  subtitle="测试版：快照 shell 环境以加速重复命令。开启写入 shell_snapshot=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_shell_snapshot, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_shell_snapshot: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="unified_exec"
                  subtitle="测试版：使用统一的、基于 PTY 的 exec 工具。开启写入 unified_exec=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_unified_exec, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_unified_exec: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="shell_tool"
                  subtitle="稳定：启用默认 shell 工具。开启写入 shell_tool=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_shell_tool, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_shell_tool: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="exec_policy"
                  subtitle="实验性：对 shell/unified_exec 强制执行规则检查。开启写入 exec_policy=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_exec_policy, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_exec_policy: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="apply_patch_freeform"
                  subtitle="实验性：启用自由格式 apply_patch 工具。开启写入 apply_patch_freeform=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_apply_patch_freeform, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_apply_patch_freeform: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="remote_compaction"
                  subtitle="实验性：启用 remote compaction（需要 ChatGPT 身份验证）。开启写入 remote_compaction=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_remote_compaction, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_remote_compaction: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="fast_mode"
                  subtitle={
                    '实验性：启用快速模式。开启同时写入 fast_mode=true 与 service_tier="fast"；关闭删除这两项。'
                  }
                >
                  <Switch
                    checked={effectiveFastModeEnabled}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig(buildFastModePatch(checked))
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="responses_websockets_v2"
                  subtitle="实验性：启用 Responses API websocket 支持（需要中转站支持）。开启写入 responses_websockets_v2=true；关闭删除该项。"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_responses_websockets_v2, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_responses_websockets_v2: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>

                <SettingItem
                  label="multi_agent"
                  subtitle="实验性：通过并行生成多个专门化代理来协作完成复杂任务，最后整合结果。开启写入 multi_agent=true；"
                >
                  <Switch
                    checked={boolOrDefault(codexConfig.features_multi_agent, false)}
                    onCheckedChange={(checked) =>
                      void persistCodexConfig({ features_multi_agent: checked })
                    }
                    disabled={saving}
                  />
                </SettingItem>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <details
                className="group"
                onToggle={(e) => setTomlAdvancedOpen((e.currentTarget as HTMLDetailsElement).open)}
              >
                <summary className="cursor-pointer select-none text-sm font-semibold text-foreground flex items-center justify-between">
                  <span className="flex items-center gap-2">
                    <Settings className="h-4 w-4 text-muted-foreground" />
                    高级配置（config.toml）
                  </span>
                  <span className="text-xs font-normal text-muted-foreground">
                    仅在需要编辑原始 TOML 时使用
                  </span>
                </summary>

                {tomlAdvancedOpen ? (
                  <div className="mt-4 space-y-3">
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                      <div className="min-w-0">
                        <div className="text-xs text-muted-foreground">路径</div>
                        <div className="mt-1 font-mono text-xs text-secondary-foreground truncate">
                          {codexConfig?.config_path ?? codexConfigToml?.config_path ?? "—"}
                        </div>
                      </div>
                      <div className="flex items-center justify-end gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => {
                            setTomlDraft(codexConfigToml?.toml ?? "");
                            setTomlDirty(false);
                            setTomlValidation(null);
                          }}
                          disabled={tomlBusy || tomlEditEnabled}
                        >
                          重新加载
                        </Button>

                        {!tomlEditEnabled ? (
                          <Button
                            type="button"
                            size="sm"
                            onClick={() => {
                              setTomlEditEnabled(true);
                              setTomlDraft(codexConfigToml?.toml ?? "");
                              setTomlDirty(false);
                              setTomlValidation(null);
                              void validateToml(codexConfigToml?.toml ?? "");
                            }}
                            disabled={tomlBusy}
                          >
                            编辑
                          </Button>
                        ) : (
                          <>
                            <Button
                              type="button"
                              size="sm"
                              variant="ghost"
                              onClick={() => {
                                setTomlEditEnabled(false);
                                setTomlDraft(codexConfigToml?.toml ?? "");
                                setTomlDirty(false);
                                setTomlValidation(null);
                              }}
                              disabled={tomlBusy}
                            >
                              取消
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              onClick={() => void saveTomlDraft()}
                              disabled={
                                tomlBusy ||
                                tomlValidating ||
                                !tomlDirty ||
                                (tomlValidation ? !tomlValidation.ok : false)
                              }
                            >
                              {tomlValidating ? "校验中…" : "保存"}
                            </Button>
                          </>
                        )}
                      </div>
                    </div>

                    {codexConfigTomlLoading ? (
                      <div className="text-sm text-muted-foreground py-6 text-center">加载中…</div>
                    ) : (
                      <Suspense
                        fallback={
                          <div className="text-sm text-muted-foreground py-6 text-center">
                            加载编辑器…
                          </div>
                        }
                      >
                        <LazyCodeEditor
                          value={tomlDraft}
                          onChange={
                            tomlEditEnabled
                              ? (next) => {
                                  setTomlDraft(next);
                                  setTomlDirty(true);
                                }
                              : undefined
                          }
                          readOnly={!tomlEditEnabled || tomlBusy}
                          language="toml"
                          minHeight="260px"
                          placeholder='例如：approval_policy = "on-request"'
                        />
                      </Suspense>
                    )}

                    {tomlValidation?.ok === false && tomlValidation.error ? (
                      <div className="rounded-lg bg-rose-50 dark:bg-rose-900/30 p-3 text-xs text-rose-700 dark:text-rose-400 flex items-start gap-2">
                        <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
                        <div className="min-w-0">
                          <div className="font-semibold">TOML 校验失败</div>
                          <div className="mt-1 break-words">
                            {tomlValidation.error.message}
                            {tomlValidation.error.line ? (
                              <span className="ml-2 font-mono text-rose-600">
                                (line {tomlValidation.error.line}
                                {tomlValidation.error.column
                                  ? `, column ${tomlValidation.error.column}`
                                  : ""}
                                )
                              </span>
                            ) : null}
                          </div>
                        </div>
                      </div>
                    ) : (
                      <div className="text-xs text-muted-foreground">
                        保存前会进行后端 TOML 校验；校验失败不会写入文件。
                      </div>
                    )}
                  </div>
                ) : null}
              </details>
            </div>
          </div>
        )}

        {codexInfo?.error && (
          <div className="mt-4 rounded-lg bg-rose-50 dark:bg-rose-900/30 p-4 text-sm text-rose-600 dark:text-rose-400 flex items-start gap-2">
            <AlertTriangle className="h-5 w-5 shrink-0" />
            <div>
              <span className="font-semibold">检测失败：</span>
              {codexInfo.error}
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}
