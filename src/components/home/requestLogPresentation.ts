// Usage:
// - Pure helpers deriving display metadata (status badge / audit tags / route meta / live trace info)
//   from request log rows and trace sessions for the Home page.
// - Designed to keep status badge / error_code label rendering consistent across the Home page.

import { createElement, type ReactNode } from "react";
import { GatewayErrorCodes } from "../../constants/gatewayErrorCodes";
import {
  normalizeClaudeModelMapping,
  type ClaudeModelMapping,
} from "../../services/gateway/claudeModelMapping";
import {
  formatCodexReasoningEffortSource,
  formatModelRouteReasoningEffortSource,
  hasClaudeModelMappingSpecialSetting,
  resolveModelRouteMappingFromSpecialSettings,
  parseRequestLogSpecialSettings,
  resolveCodexReasoningEffort,
  resolveCodexReasoningGuardSummary,
  resolveClaudeModelMappingFromSpecialSettings,
  type ModelRouteMapping,
} from "../../services/gateway/requestLogSpecialSettings";
import type { CliKey } from "../../services/providers/providers";
import type { RequestLogRouteHop } from "../../services/gateway/requestLogs";
import type { TraceSession } from "../../services/gateway/traceStore";
import { RouteTooltipContent } from "./RouteTooltipContent";
import { getErrorCodeLabel } from "./requestLogErrorLabels";

const CLIENT_ABORT_ERROR_CODES: ReadonlySet<string> = new Set([
  GatewayErrorCodes.STREAM_ABORTED,
  GatewayErrorCodes.REQUEST_ABORTED,
]);

const STATUS_TEXT_UNKNOWN = "状态未知";

type RequestLogAuditInput = {
  cli_key: CliKey | string;
  path: string;
  status?: number | null;
  excluded_from_stats?: boolean | null;
  special_settings_json?: string | null;
  final_provider_id?: number | null;
  error_code?: string | null;
};

export type RequestLogAuditTag = {
  label: string;
  className: string;
  title?: string;
};

export type RequestLogAuditMeta = {
  muted: boolean;
  summary: string | null;
  tags: RequestLogAuditTag[];
  providerFallbackText: string | null;
  reasoningTokens: number | null;
};

export { hasClaudeModelMappingSpecialSetting, resolveClaudeModelMappingFromSpecialSettings };
export {
  formatCodexReasoningEffortSource,
  resolveCodexReasoningEffort,
  resolveModelRouteMappingFromSpecialSettings,
};

export function hasCodexReasoningGuardSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  return resolveCodexReasoningGuardSummary(specialSettingsJson).count > 0;
}

function isCodexReasoningContinuationStrategy(value: string | null | undefined): boolean {
  return value === "continuation_repair" || value === "continuation_repair_experimental";
}

function formatCodexReasoningGuardActionText(summary: {
  latestActionTaken: string | null;
  latestDelayMs: number | null;
  latestPostMatchStrategy?: string | null;
  latestStrategyOutcome?: string | null;
  latestContinuationSentRounds?: number | null;
}): string {
  if (
    isCodexReasoningContinuationStrategy(summary.latestPostMatchStrategy) &&
    (summary.latestStrategyOutcome === "continuation_repaired" ||
      summary.latestStrategyOutcome === "repaired")
  ) {
    return `思考续写成功${
      summary.latestContinuationSentRounds != null
        ? `（${summary.latestContinuationSentRounds} 次）`
        : ""
    }`;
  }
  if (isCodexReasoningContinuationStrategy(summary.latestPostMatchStrategy)) {
    const status = formatCodexReasoningContinuationStatus(summary.latestStrategyOutcome);
    return summary.latestStrategyOutcome ? `思考续写后${status}` : "思考续写";
  }
  if (summary.latestActionTaken === "switch_provider_no_circuit") {
    return "预算耗尽后切换供应商";
  }
  if (summary.latestActionTaken === "switch_model_no_circuit") {
    return "预算耗尽后切换模型";
  }
  if (summary.latestActionTaken === "return_guard_error_no_circuit") {
    return "预算耗尽后返回错误";
  }
  if (
    summary.latestActionTaken === "retry_same_provider_delayed_no_circuit" &&
    summary.latestDelayMs != null
  ) {
    return `等待 ${summary.latestDelayMs}ms 后重试`;
  }
  return "继续重试";
}

export function formatCodexReasoningContinuationStatus(status: string | null | undefined): string {
  if (status === "continuation_repaired") return "已修复";
  if (status === "repaired") return "已修复";
  if (status === "failed") return "补救失败";
  if (status === "still_matched") return "仍命中";
  if (status === "missing_encrypted") return "缺少 encrypted";
  if (status === "capped_max_output_tokens") return "输出上限";
  if (status === "unsupported") return "不支持";
  if (status === "unavailable") return "不可用";
  if (status === "unknown") return "未知状态";
  return status || "未知状态";
}

function formatCodexReasoningGuardSummaryClause(summary: {
  count: number;
  latestRuleLabel: string | null;
}): string {
  return summary.count > 1
    ? `命中了 ${summary.count} 次 Codex 降智拦截${summary.latestRuleLabel ? `（规则 ${summary.latestRuleLabel}）` : ""}`
    : `命中了 Codex 降智拦截${summary.latestRuleLabel ? `（规则 ${summary.latestRuleLabel}）` : ""}`;
}

function finiteJsonNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function resolveReasoningTokensFromJsonValue(value: unknown): number | null {
  if (value == null || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  return (
    finiteJsonNumber(
      (record.output_tokens_details as Record<string, unknown> | undefined)?.reasoning_tokens
    ) ??
    finiteJsonNumber(
      (record.outputTokensDetails as Record<string, unknown> | undefined)?.reasoningTokens
    ) ??
    finiteJsonNumber(
      (record.outputTokensDetails as Record<string, unknown> | undefined)?.reasoningTokenCount
    ) ??
    finiteJsonNumber(
      (record.completion_tokens_details as Record<string, unknown> | undefined)?.reasoning_tokens
    ) ??
    finiteJsonNumber(
      (record.completionTokensDetails as Record<string, unknown> | undefined)?.reasoningTokens
    ) ??
    finiteJsonNumber(
      (record.completionTokensDetails as Record<string, unknown> | undefined)?.reasoningTokenCount
    ) ??
    finiteJsonNumber(record.reasoning_tokens) ??
    finiteJsonNumber(record.reasoningTokens) ??
    finiteJsonNumber(record.reasoningTokenCount) ??
    finiteJsonNumber(record.thinking_tokens) ??
    finiteJsonNumber(record.thinkingTokens) ??
    resolveReasoningTokensFromJsonValue(record.usage) ??
    resolveReasoningTokensFromJsonValue(
      (record.response as Record<string, unknown> | undefined)?.usage
    )
  );
}

export function resolveRequestLogUsageReasoningTokens(
  usageJson: string | null | undefined
): number | null {
  if (!usageJson) return null;
  try {
    return resolveReasoningTokensFromJsonValue(JSON.parse(usageJson) as unknown);
  } catch {
    return null;
  }
}

export function hasCodexReasoningGuardRetryAttempt(
  attempts: Array<{ outcome?: string | null }> | null | undefined
): boolean {
  return (
    attempts?.some((attempt) => {
      const outcome = attempt.outcome?.trim().toLowerCase();
      return !!outcome && outcome.includes("codex_reasoning_guard");
    }) ?? false
  );
}

export function formatClaudeModelMappingText(
  requestedModel: string | null | undefined,
  mapping: ClaudeModelMapping | null | undefined
) {
  const normalized = normalizeClaudeModelMapping(mapping);
  if (normalized) {
    return `${normalized.requestedModel} → ${normalized.effectiveModel}`;
  }

  const fallback = requestedModel?.trim();
  return fallback || "未知";
}

const MODEL_ROUTE_EFFORT_SUFFIXES = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

function formatModelRoutePart(model: string, effort: string | null | undefined) {
  const normalizedModel = model.trim() || "未知";
  const normalizedEffort = effort?.trim().toLowerCase();
  if (!normalizedEffort || normalizedEffort === "unknown") {
    return normalizedModel;
  }
  // Model ids may already embed effort (e.g. codex-auto-review-low); avoid
  // rendering duplicated suffixes like codex-auto-review-low-low.
  const lowerModel = normalizedModel.toLowerCase();
  if (
    lowerModel.endsWith(`-${normalizedEffort}`) ||
    MODEL_ROUTE_EFFORT_SUFFIXES.some((suffix) => lowerModel.endsWith(`-${suffix}`))
  ) {
    return normalizedModel;
  }
  return `${normalizedModel}-${normalizedEffort}`;
}

/** Codex auto-review uses logical model ids like `codex-auto-review` / `codex-auto-review-low`. */
export function isExpectedCodexAutoReviewRequestedModel(
  requestedModel: string | null | undefined
): boolean {
  const model = requestedModel?.trim().toLowerCase() ?? "";
  return model === "codex-auto-review" || model.startsWith("codex-auto-review-");
}

export function isExpectedCodexAutoReviewModelRoute(
  mapping: Pick<ModelRouteMapping, "requestedModel"> | null | undefined
): boolean {
  return isExpectedCodexAutoReviewRequestedModel(mapping?.requestedModel);
}

function resolveModelRouteMismatchLabel(
  mapping: ModelRouteMapping,
  options: { expectedAutoReview?: boolean } = {}
) {
  if (options.expectedAutoReview) return "自动审核模型映射";
  if (mapping.modelMismatch && mapping.effortMismatch) return "模型/思考等级不一致";
  if (mapping.effortMismatch) return "思考等级不一致";
  return "模型路由不一致";
}

function resolveModelRouteTitle(mapping: ModelRouteMapping) {
  const expectedAutoReview = isExpectedCodexAutoReviewModelRoute(mapping);
  const titleParts = [
    resolveModelRouteMismatchLabel(mapping, { expectedAutoReview }),
    `请求 ${formatModelRoutePart(mapping.requestedModel, mapping.requestedReasoningEffort)}`,
    `返回 ${formatModelRoutePart(mapping.actualModel, mapping.actualReasoningEffort)}`,
    `请求等级 ${formatModelRouteReasoningEffortSource(mapping.requestedReasoningEffortSource)}`,
    `返回等级 ${formatModelRouteReasoningEffortSource(mapping.actualReasoningEffortSource)}`,
  ];
  if (mapping.providerName) {
    titleParts.push(`Provider ${mapping.providerName}`);
  }
  return titleParts.join(" · ");
}

export type RequestLogModelDisplayMeta = {
  text: string;
  title: string;
  routeMapping: ModelRouteMapping | null;
  /** True when a request→actual mapping is shown (including expected auto-review). */
  isRouteMismatch: boolean;
  /**
   * True only for unexpected route mismatches that should use severe/rose styling.
   * Expected Codex auto-review mappings stay visible but non-severe.
   */
  isSevereRouteMismatch: boolean;
  isExpectedAutoReviewRoute: boolean;
  mismatchLabel: string | null;
};

export function resolveRequestLogModelDisplayMeta(
  cliKey: CliKey | string,
  requestedModel: string | null | undefined,
  specialSettingsJson?: string | null,
  mapping?: ClaudeModelMapping | null,
  finalProviderId?: number | null
): RequestLogModelDisplayMeta {
  const routeMapping = resolveModelRouteMappingFromSpecialSettings(
    specialSettingsJson,
    finalProviderId
  );

  if (routeMapping) {
    const expectedAutoReview = isExpectedCodexAutoReviewModelRoute(routeMapping);
    const text = `${formatModelRoutePart(
      routeMapping.requestedModel,
      routeMapping.requestedReasoningEffort
    )} -> ${formatModelRoutePart(routeMapping.actualModel, routeMapping.actualReasoningEffort)}`;
    return {
      text,
      title: resolveModelRouteTitle(routeMapping),
      routeMapping,
      isRouteMismatch: true,
      isSevereRouteMismatch: !expectedAutoReview,
      isExpectedAutoReviewRoute: expectedAutoReview,
      mismatchLabel: resolveModelRouteMismatchLabel(routeMapping, { expectedAutoReview }),
    };
  }

  const modelText = formatClaudeModelMappingText(requestedModel, mapping);
  if (cliKey !== "codex") {
    return {
      text: modelText,
      title: modelText,
      routeMapping: null,
      isRouteMismatch: false,
      isSevereRouteMismatch: false,
      isExpectedAutoReviewRoute: false,
      mismatchLabel: null,
    };
  }

  const effort = resolveCodexReasoningEffort(requestedModel, specialSettingsJson).effort;
  const text = `${modelText}-${effort}`;
  return {
    text,
    title: text,
    routeMapping: null,
    isRouteMismatch: false,
    isSevereRouteMismatch: false,
    isExpectedAutoReviewRoute: false,
    mismatchLabel: null,
  };
}

export function formatRequestLogModelText(
  cliKey: CliKey | string,
  requestedModel: string | null | undefined,
  specialSettingsJson?: string | null,
  mapping?: ClaudeModelMapping | null,
  finalProviderId?: number | null
) {
  return resolveRequestLogModelDisplayMeta(
    cliKey,
    requestedModel,
    specialSettingsJson,
    mapping,
    finalProviderId
  ).text;
}

type CodexServiceTierResultSetting = {
  type: "codex_service_tier_result";
  requestedServiceTier?: string | null;
  actualServiceTier?: string | null;
  billingSourcePreference?: string | null;
  resolvedFrom?: string | null;
  effectivePriority?: boolean;
};

function isCodexServiceTierResultSetting(value: unknown): value is CodexServiceTierResultSetting {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as Record<string, unknown>).type === "codex_service_tier_result"
  );
}

/**
 * Check if the request has priority service tier applied (Codex fast mode).
 */
export function hasPriorityServiceTierSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  if (!specialSettingsJson) return false;

  try {
    const settings = JSON.parse(specialSettingsJson) as unknown;
    if (!Array.isArray(settings)) return false;

    const codexTierSetting = [...settings].reverse().find(isCodexServiceTierResultSetting);
    if (!codexTierSetting) return false;

    // Legacy compatibility: if no billingSourcePreference, check actualServiceTier
    if (
      codexTierSetting.billingSourcePreference == null &&
      codexTierSetting.resolvedFrom == null &&
      codexTierSetting.actualServiceTier != null
    ) {
      return codexTierSetting.actualServiceTier === "priority";
    }

    return codexTierSetting.effectivePriority === true;
  } catch {
    return false;
  }
}

export type CacheCreationDisplay = {
  tokens: number;
  ttl: "5m" | "1h" | null;
};

export function resolveCacheCreationDisplay(input: {
  cache_creation_input_tokens?: number | null;
  cache_creation_5m_input_tokens?: number | null;
  cache_creation_1h_input_tokens?: number | null;
}): CacheCreationDisplay | null {
  if (input.cache_creation_5m_input_tokens != null && input.cache_creation_5m_input_tokens > 0) {
    return { tokens: input.cache_creation_5m_input_tokens, ttl: "5m" };
  }
  if (input.cache_creation_1h_input_tokens != null && input.cache_creation_1h_input_tokens > 0) {
    return { tokens: input.cache_creation_1h_input_tokens, ttl: "1h" };
  }
  if (input.cache_creation_input_tokens != null && input.cache_creation_input_tokens > 0) {
    return { tokens: input.cache_creation_input_tokens, ttl: null };
  }
  if (input.cache_creation_5m_input_tokens != null) {
    return { tokens: input.cache_creation_5m_input_tokens, ttl: "5m" };
  }
  if (input.cache_creation_1h_input_tokens != null) {
    return { tokens: input.cache_creation_1h_input_tokens, ttl: "1h" };
  }
  if (input.cache_creation_input_tokens != null) {
    return { tokens: input.cache_creation_input_tokens, ttl: null };
  }
  return null;
}

function auditTag(label: string, className: string, title?: string): RequestLogAuditTag {
  return { label, className, title };
}

type RequestLogAuditMetaOptions = {
  codexReasoningGuardHitLabel?: string | null;
};

export function buildRequestLogAuditMeta(
  log: RequestLogAuditInput,
  options: RequestLogAuditMetaOptions = {}
): RequestLogAuditMeta {
  const codexReasoningGuardHitLabel = options.codexReasoningGuardHitLabel?.trim() || "降智命中";
  const settings = parseRequestLogSpecialSettings(log.special_settings_json);
  const settingTypes = new Set(settings.flatMap((item) => (item.type ? [item.type] : [])));
  const isWarmupIntercept = settingTypes.has("warmup_intercept");
  const isCliProxyGuard = settingTypes.has("cli_proxy_guard");
  const isSuccessful = typeof log.status === "number" && log.status >= 200 && log.status < 300;
  const isClientAbort =
    !isSuccessful &&
    (!!(log.error_code && CLIENT_ABORT_ERROR_CODES.has(log.error_code)) ||
      settingTypes.has("client_abort"));
  const isAllProvidersUnavailable = log.error_code === GatewayErrorCodes.ALL_PROVIDERS_UNAVAILABLE;
  const excludedFromStats = !!log.excluded_from_stats;
  const codexReasoningGuard = resolveCodexReasoningGuardSummary(log.special_settings_json);
  const codexReasoningGuardHitCount = codexReasoningGuard.count;
  const modelRouteMapping = resolveModelRouteMappingFromSpecialSettings(
    log.special_settings_json,
    log.final_provider_id
  );
  const codexReasoningGuardRuleSuffix = codexReasoningGuard.latestRuleLabel
    ? ` ${codexReasoningGuard.latestRuleLabel}`
    : "";
  const codexReasoningGuardActionText = formatCodexReasoningGuardActionText(codexReasoningGuard);
  const codexReasoningGuardBudgetSuffix =
    codexReasoningGuard.latestBudgetTotal != null
      ? `，剩余预算 ${codexReasoningGuard.latestBudgetRemaining ?? 0}/${codexReasoningGuard.latestBudgetTotal}`
      : "";

  const tags: RequestLogAuditTag[] = [];

  if (isWarmupIntercept) {
    tags.push(
      auditTag(
        "Warmup",
        "bg-sky-50/80 text-sky-700 ring-1 ring-inset ring-sky-500/10 dark:bg-sky-500/15 dark:text-sky-200 dark:ring-sky-400/20",
        "Anthropic warmup 命中后直接由网关拦截"
      )
    );
  }

  if (isCliProxyGuard) {
    tags.push(
      auditTag(
        "代理守卫",
        "bg-orange-50/80 text-orange-700 ring-1 ring-inset ring-orange-500/10 dark:bg-orange-500/15 dark:text-orange-200 dark:ring-orange-400/20",
        "CLI 代理守卫提前处理了这次请求"
      )
    );
  }

  if (isClientAbort) {
    tags.push(
      auditTag(
        "客户端中断",
        "bg-amber-50/80 text-amber-700 ring-1 ring-inset ring-amber-500/10 dark:bg-amber-500/15 dark:text-amber-200 dark:ring-amber-400/20",
        "请求已被客户端主动中断"
      )
    );
  }

  if (isAllProvidersUnavailable) {
    tags.push(
      auditTag(
        "全部不可用",
        "bg-rose-50/80 text-rose-700 ring-1 ring-inset ring-rose-500/10 dark:bg-rose-500/15 dark:text-rose-200 dark:ring-rose-400/20",
        "当前没有可用 Provider，通常是全部处于熔断、冷却或限流状态"
      )
    );
  }

  if (codexReasoningGuardHitCount > 0) {
    tags.push(
      auditTag(
        codexReasoningGuardHitCount > 1
          ? `${codexReasoningGuardHitLabel} ${codexReasoningGuardHitCount}${codexReasoningGuardRuleSuffix}`
          : `${codexReasoningGuardHitLabel}${codexReasoningGuardRuleSuffix}`,
        "bg-violet-50/80 text-violet-700 ring-1 ring-inset ring-violet-500/10 dark:bg-violet-500/15 dark:text-violet-200 dark:ring-violet-400/20",
        codexReasoningGuard.latestRuleLabel
          ? `命中 Codex 降智拦截规则 ${codexReasoningGuard.latestRuleLabel} 后${codexReasoningGuardActionText}，不计入熔断${codexReasoningGuardBudgetSuffix}`
          : `命中 Codex 降智拦截后${codexReasoningGuardActionText}，不计入熔断${codexReasoningGuardBudgetSuffix}`
      )
    );
  }

  if (modelRouteMapping) {
    const expectedAutoReview = isExpectedCodexAutoReviewModelRoute(modelRouteMapping);
    tags.push(
      auditTag(
        expectedAutoReview ? "自动审核映射" : "模型路由",
        expectedAutoReview
          ? "bg-sky-50/80 text-sky-700 ring-1 ring-inset ring-sky-500/10 dark:bg-sky-500/15 dark:text-sky-200 dark:ring-sky-400/20"
          : "bg-rose-50/80 text-rose-700 ring-1 ring-inset ring-rose-500/15 dark:bg-rose-500/15 dark:text-rose-200 dark:ring-rose-400/25",
        resolveModelRouteTitle(modelRouteMapping)
      )
    );
  }

  if (excludedFromStats) {
    tags.push(
      auditTag(
        "不计统计",
        "bg-secondary/90 text-muted-foreground ring-1 ring-inset ring-border dark:bg-secondary/70 dark:text-foreground dark:ring-border",
        "这条记录保留在审计列表中，但不会进入 usage/cost/provider 聚合"
      )
    );
  }

  let summary: string | null = null;
  if (isWarmupIntercept) {
    summary = "Warmup 命中后由网关直接应答，仅保留审计记录，不进入统计。";
  } else if (isCliProxyGuard) {
    summary = "这次请求由 CLI 代理守卫提前处理，保留为审计行。";
  } else if (codexReasoningGuardHitCount > 0) {
    summary = `本次请求${formatCodexReasoningGuardSummaryClause(
      codexReasoningGuard
    )}，${codexReasoningGuardActionText}。`;
  } else if (modelRouteMapping) {
    const expectedAutoReview = isExpectedCodexAutoReviewModelRoute(modelRouteMapping);
    summary = expectedAutoReview
      ? `自动审核模型映射：${formatModelRoutePart(
          modelRouteMapping.requestedModel,
          modelRouteMapping.requestedReasoningEffort
        )} -> ${formatModelRoutePart(
          modelRouteMapping.actualModel,
          modelRouteMapping.actualReasoningEffort
        )}（预期行为，非路由故障）。`
      : `模型路由检测：${resolveModelRouteMismatchLabel(modelRouteMapping)}。`;
  } else if (isAllProvidersUnavailable) {
    summary = "当前没有可用 Provider，网关未继续向已熔断或冷却中的供应商发起上游请求。";
  } else if (isClientAbort) {
    summary = "客户端中途中断了请求，系统保留这条审计记录但不计入统计。";
  } else if (excludedFromStats) {
    summary = "这条记录仅用于审计可见性，不参与统计聚合。";
  }

  return {
    muted:
      isWarmupIntercept ||
      isCliProxyGuard ||
      isClientAbort ||
      isAllProvidersUnavailable ||
      codexReasoningGuardHitCount > 0 ||
      excludedFromStats,
    summary,
    tags,
    providerFallbackText: isWarmupIntercept
      ? "Warmup"
      : isCliProxyGuard
        ? "CLI 守卫"
        : isAllProvidersUnavailable
          ? "无可用供应商"
          : null,
    reasoningTokens: codexReasoningGuard.latestReasoningTokens,
  };
}

export type LiveTraceProvider = {
  providerId: number | null;
  providerName: string;
};

export function resolveLiveTraceProvider(
  trace: TraceSession | null | undefined
): LiveTraceProvider | null {
  const attempts = trace?.attempts;
  if (!attempts?.length) return null;

  let best: (typeof attempts)[number] | null = null;
  for (const attempt of attempts) {
    const name = attempt.provider_name?.trim();
    if (!name || name === "Unknown") continue;
    if (!best || attempt.attempt_index > best.attempt_index) {
      best = attempt;
    }
  }
  if (!best) return null;
  return {
    providerId: typeof best.provider_id === "number" ? best.provider_id : null,
    providerName: (best.provider_name ?? "").trim(),
  };
}

export function resolveLiveTraceDurationMs(
  trace: TraceSession | null | undefined,
  nowMs = Date.now()
) {
  if (!trace) return null;
  return Math.max(0, nowMs - trace.first_seen_ms);
}

export type StatusBadge = {
  text: string;
  semanticText: string;
  tone: string;
  title?: string;
  isError: boolean;
  isClientAbort: boolean;
  hasFailover: boolean;
};

export function computeStatusBadge(input: {
  status: number | null;
  errorCode: string | null;
  inProgress?: boolean;
  hasFailover?: boolean;
}): StatusBadge {
  if (input.inProgress) {
    return {
      text: "进行中",
      semanticText: "请求进行中",
      tone: "bg-accent/10 text-accent ring-1 ring-inset ring-accent/15",
      isError: false,
      isClientAbort: false,
      hasFailover: !!input.hasFailover,
    };
  }

  const isClientAbort = !!(input.errorCode && CLIENT_ABORT_ERROR_CODES.has(input.errorCode));
  const hasFailover = !!input.hasFailover;
  const hasTerminalErrorCode = !!input.errorCode && !isClientAbort;
  const isSuccessStatus =
    !hasTerminalErrorCode && input.status != null && input.status >= 200 && input.status < 400;
  const isError =
    hasTerminalErrorCode || (input.status != null ? input.status >= 400 : input.errorCode != null);

  let text = STATUS_TEXT_UNKNOWN;
  let semanticText = STATUS_TEXT_UNKNOWN;

  if (isClientAbort) {
    text = input.status == null ? "已中断" : `${input.status} 已中断`;
    semanticText = "客户端已中断";
  } else if (isSuccessStatus && hasFailover) {
    text = input.status == null ? "切换后成功" : `${input.status} 切换后成功`;
    semanticText = "切换供应商后成功";
  } else if (isSuccessStatus) {
    text = input.status == null ? "成功" : `${input.status} 成功`;
    semanticText = "请求成功";
  } else if (isError) {
    text = input.status == null ? "失败" : `${input.status} 失败`;
    semanticText = "请求失败";
  }

  const tone = isClientAbort
    ? "bg-amber-50 text-amber-600 ring-1 ring-inset ring-amber-500/15 dark:bg-amber-500/15 dark:text-amber-400 dark:ring-amber-400/25"
    : isSuccessStatus
      ? hasFailover
        ? "text-emerald-600 bg-emerald-50/60 ring-1 ring-inset ring-amber-400/30 dark:text-emerald-400 dark:bg-emerald-500/15 dark:ring-amber-500/30"
        : "text-emerald-600 bg-emerald-50/60 ring-1 ring-inset ring-emerald-500/10 dark:text-emerald-400 dark:bg-emerald-500/15 dark:ring-emerald-400/20"
      : isError
        ? "text-rose-600 bg-rose-50/60 ring-1 ring-inset ring-rose-500/10 dark:text-rose-400 dark:bg-rose-500/15 dark:ring-rose-400/20"
        : "text-muted-foreground bg-secondary ring-1 ring-inset ring-border dark:text-muted-foreground dark:bg-secondary dark:ring-border";

  const title = input.errorCode
    ? `${semanticText} · ${getErrorCodeLabel(input.errorCode)} (${input.errorCode})`
    : semanticText;

  return { text, semanticText, tone, title, isError, isClientAbort, hasFailover };
}

export function buildRequestRouteMeta(input: {
  route: RequestLogRouteHop[] | null | undefined;
  status: number | null;
  hasFailover: boolean;
  attemptCount: number;
}) {
  const hops = input.route ?? [];
  if (hops.length === 0) {
    return {
      hasRoute: false,
      label: "链路",
      summary: "暂无链路信息",
      tooltipText: null as string | null,
      tooltipContent: null as ReactNode,
    };
  }

  const skippedCount = hops.filter((h) => h.skipped).reduce((sum, h) => sum + (h.attempts ?? 1), 0);
  const activeAttemptCount = hops
    .filter((h) => !h.skipped)
    .reduce((sum, h) => sum + (h.attempts ?? 1), 0);
  const hasRetry = hops.some((h) => !h.skipped && (h.attempts ?? 1) > 1);

  const summary = input.hasFailover
    ? `切换 ${input.attemptCount} 次后${input.status != null && input.status < 400 ? "成功" : "结束"}`
    : skippedCount > 0 && hasRetry
      ? `跳过 ${skippedCount} 个候选，并重试 ${activeAttemptCount} 次`
      : skippedCount > 0
        ? `跳过 ${skippedCount} 个候选`
        : hasRetry
          ? `同一供应商重试 ${input.attemptCount} 次`
          : "直连完成";

  // 纯文本 fallback（用于 title 属性）
  const tooltipText = hops
    .map((hop, idx) => {
      const rawProviderName = hop.provider_name?.trim();
      const providerName =
        !rawProviderName || rawProviderName === "Unknown" ? "未知" : rawProviderName;
      const status = hop.status ?? (idx === hops.length - 1 ? input.status : null) ?? null;
      const statusText = status == null ? "状态未知" : String(status);
      const attemptsSuffix = hop.attempts && hop.attempts > 1 ? `，尝试 ${hop.attempts} 次` : "";
      if (hop.ok) return `${providerName}（${statusText}，成功${attemptsSuffix}）`;
      if (hop.skipped) return `${providerName}（已跳过${attemptsSuffix}）`;
      const errorCode = hop.error_code ?? null;
      const errorLabel = errorCode ? getErrorCodeLabel(errorCode) : "失败";
      return `${providerName}（${statusText}，${errorLabel}${attemptsSuffix}）`;
    })
    .join(" → ");

  let label = summary;
  if (input.hasFailover) {
    label = `切换 ${input.attemptCount} 次`;
  } else if (skippedCount > 0 && hasRetry) {
    label = `跳过 ${skippedCount} 个 + 重试`;
  } else if (skippedCount > 0) {
    label = `跳过 ${skippedCount} 个`;
  } else if (hasRetry) {
    label = `重试 ${input.attemptCount} 次`;
  }

  // 无 JSX 约束：此文件为 .ts，等价于 <RouteTooltipContent ... /> 的 createElement 调用
  const tooltipContent = createElement(RouteTooltipContent, {
    hops,
    finalStatus: input.status,
    summary,
    skippedCount,
  });

  return {
    hasRoute: true,
    label,
    summary,
    tooltipText,
    tooltipContent,
  };
}
