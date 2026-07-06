import type { RequestLogDetail } from "../../services/gateway/requestLogs";
import type { RequestLogErrorObservation } from "./requestLogErrorDetails";
import { Card } from "../../ui/Card";
import { cn } from "../../utils/cn";
import {
  computeOutputTokensPerSecond,
  formatDurationMs,
  formatTokensPerSecond,
  formatUsd,
  resolveTtfbDisplayMetrics,
} from "../../utils/formatters";
import { RequestLogErrorObservationCard } from "./RequestLogErrorObservationCard";
import { resolveCodexReasoningGuardSummary } from "../../services/gateway/requestLogSpecialSettings";
import {
  buildRequestLogAuditMeta,
  computeStatusBadge,
  FastModeBadge,
  formatCodexReasoningContinuationStatus,
  formatCodexReasoningEffortSource,
  hasCodexReasoningGuardSpecialSetting,
  hasPriorityServiceTierSpecialSetting,
  resolveCodexReasoningEffort,
  resolveRequestLogUsageReasoningTokens,
} from "./HomeLogShared";

export type RequestLogDetailSummaryTabProps = {
  selectedLog: RequestLogDetail;
  errorObservation: RequestLogErrorObservation | null;
  statusBadge: ReturnType<typeof computeStatusBadge> | null;
  hasTokens: boolean;
  displayDurationMs: number;
  isInProgress: boolean;
  attemptCount: number;
  codexReasoningGuardHitLabel?: string;
};

export function RequestLogDetailSummaryTab({
  selectedLog,
  errorObservation,
  statusBadge,
  hasTokens,
  displayDurationMs,
  isInProgress: _isInProgress,
  attemptCount: _attemptCount,
  codexReasoningGuardHitLabel,
}: RequestLogDetailSummaryTabProps) {
  const auditMeta = buildRequestLogAuditMeta(selectedLog, { codexReasoningGuardHitLabel });
  const usageReasoningTokens = resolveRequestLogUsageReasoningTokens(selectedLog.usage_json);
  const codexReasoningEffort =
    selectedLog.cli_key === "codex"
      ? resolveCodexReasoningEffort(selectedLog.requested_model, selectedLog.special_settings_json)
      : null;
  const codexReasoningGuard =
    selectedLog.cli_key === "codex"
      ? resolveCodexReasoningGuardSummary(selectedLog.special_settings_json)
      : null;
  const showCodexReasoningSignals = (codexReasoningGuard?.count ?? 0) > 0;
  const showKeyMetrics = hasTokens || codexReasoningEffort != null || showCodexReasoningSignals;
  const isPriorityServiceTier =
    selectedLog.cli_key === "codex" &&
    hasPriorityServiceTierSpecialSetting(selectedLog.special_settings_json);
  const ttfbMetrics = resolveTtfbDisplayMetrics(
    selectedLog.ttfb_ms,
    selectedLog.visible_ttfb_ms ?? null,
    displayDurationMs,
    hasCodexReasoningGuardSpecialSetting(selectedLog.special_settings_json)
  );

  return (
    <div className="space-y-3">
      {/* Error observation card (request-level) */}
      <RequestLogErrorObservationCard observation={errorObservation} />

      {/* Audit meta */}
      {auditMeta && auditMeta.tags.length > 0 ? (
        <Card padding="sm">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="text-sm font-semibold text-foreground">审计语义</div>
            <div className="flex flex-wrap items-center gap-2">
              {auditMeta.tags.map((tag) => (
                <span
                  key={tag.label}
                  className={cn("rounded-full px-2.5 py-1 text-xs font-medium", tag.className)}
                  title={tag.title}
                >
                  {tag.label}
                </span>
              ))}
            </div>
          </div>
          {auditMeta.summary ? (
            <div className="mt-3 text-sm text-muted-foreground dark:text-secondary-foreground">
              {auditMeta.summary}
            </div>
          ) : null}
        </Card>
      ) : null}

      {/* Key metrics */}
      {showKeyMetrics ? (
        <Card padding="sm">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="text-sm font-semibold text-foreground">关键指标</div>
            <div className="flex flex-wrap items-center gap-2">
              {isPriorityServiceTier ? <FastModeBadge showCustomTooltip={false} /> : null}
              {statusBadge ? (
                <span
                  className={cn("rounded-full px-2.5 py-1 text-xs font-medium", statusBadge.tone)}
                  title={statusBadge.title}
                >
                  {statusBadge.text}
                </span>
              ) : null}
            </div>
          </div>

          <div className="mt-3 grid gap-2 grid-cols-2 sm:grid-cols-3 lg:grid-cols-4">
            <MetricCard label="输入 Token" value={selectedLog.input_tokens} />
            <MetricCard label="输出 Token" value={selectedLog.output_tokens} />
            <MetricCard label="思考 Token" value={usageReasoningTokens} />
            {codexReasoningEffort ? (
              <>
                <MetricCard label="思考等级" value={codexReasoningEffort.effort} />
                <MetricCard
                  label="等级来源"
                  value={formatCodexReasoningEffortSource(codexReasoningEffort.source)}
                />
              </>
            ) : null}
            {showCodexReasoningSignals ? (
              <>
                <MetricCard
                  label="规则模式"
                  value={formatCodexReasoningRuleMode(codexReasoningGuard?.latestRuleMode)}
                />
                <MetricCard
                  label="命中来源"
                  value={formatCodexReasoningHitSource(codexReasoningGuard?.latestHitSource)}
                />
                <MetricCard label="命中次数" value={codexReasoningGuard?.count ?? 0} />
                <MetricCard
                  label="命中后策略"
                  value={formatCodexReasoningPostMatchStrategy(
                    codexReasoningGuard?.latestPostMatchStrategy
                  )}
                />
                <MetricCard
                  label="策略结果"
                  value={formatCodexReasoningContinuationStatus(
                    codexReasoningGuard?.latestStrategyOutcome
                  )}
                />
                <MetricCard
                  label="续写轮数"
                  value={codexReasoningGuard?.latestContinuationSentRounds}
                />
              </>
            ) : null}
            <MetricCard label="缓存创建" value={resolveCacheWriteValue(selectedLog)} />
            <MetricCard label="缓存读取" value={selectedLog.cache_read_input_tokens} />
            <MetricCard label="总耗时" value={formatDurationMs(displayDurationMs)} />
            <MetricCard
              label="TTFB"
              value={
                ttfbMetrics.providerTtfbMs != null
                  ? formatDurationMs(ttfbMetrics.providerTtfbMs)
                  : "—"
              }
            />
            {ttfbMetrics.showVisibleTtfb ? (
              <MetricCard
                label="可见首字"
                value={
                  ttfbMetrics.visibleTtfbMs != null
                    ? formatDurationMs(ttfbMetrics.visibleTtfbMs)
                    : "—"
                }
              />
            ) : null}
            <MetricCard
              label="速率"
              value={(() => {
                const rate = computeOutputTokensPerSecond(
                  selectedLog.output_tokens,
                  displayDurationMs,
                  ttfbMetrics.providerTtfbMs
                );
                return rate != null ? formatTokensPerSecond(rate) : "—";
              })()}
            />
            <MetricCard label="花费" value={formatUsd(selectedLog.cost_usd)} />
            <MetricCard
              label="费用系数"
              value={formatCostMultiplier(selectedLog.cost_multiplier)}
            />
          </div>
        </Card>
      ) : null}
    </div>
  );
}

function MetricCard({
  label,
  value,
}: {
  label: string;
  value: string | number | null | undefined;
}) {
  return (
    <div className="rounded-xl border border-border/80 bg-secondary/80 px-3 py-3 dark:border-border dark:bg-secondary/70">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 break-words text-base font-semibold leading-snug text-foreground">
        {value == null || value === "" ? "—" : value}
      </div>
    </div>
  );
}

function formatCodexReasoningRuleMode(value: string | null | undefined) {
  if (value === "reasoning_tokens") return "reasoning tokens";
  if (value === "final_answer_only_high_xhigh") return "final-only";
  return value || "—";
}

function formatCodexReasoningHitSource(value: string | null | undefined) {
  if (value === "reasoning_tokens") return "token";
  if (value === "final_answer_only_high_xhigh") return "final-only";
  return value || "—";
}

function formatCodexReasoningPostMatchStrategy(value: string | null | undefined) {
  if (value === "continuation_repair") return "思考续写";
  if (value === "continuation_repair_experimental") return "思考续写（实验）";
  if (value === "retry_same_provider") return "自动重试";
  return value || "—";
}

function formatCostMultiplier(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return "—";
  return value === 0 ? "免费" : `x${value.toFixed(2)}`;
}

function resolveCacheWriteValue(selectedLog: RequestLogDetail) {
  if (
    selectedLog.cache_creation_5m_input_tokens != null &&
    selectedLog.cache_creation_5m_input_tokens > 0
  ) {
    return `${selectedLog.cache_creation_5m_input_tokens} (5m)`;
  }
  if (
    selectedLog.cache_creation_1h_input_tokens != null &&
    selectedLog.cache_creation_1h_input_tokens > 0
  ) {
    return `${selectedLog.cache_creation_1h_input_tokens} (1h)`;
  }
  if (selectedLog.cache_creation_input_tokens != null) {
    return selectedLog.cache_creation_input_tokens;
  }
  if (selectedLog.cache_creation_5m_input_tokens != null) {
    return `${selectedLog.cache_creation_5m_input_tokens} (5m)`;
  }
  if (selectedLog.cache_creation_1h_input_tokens != null) {
    return `${selectedLog.cache_creation_1h_input_tokens} (1h)`;
  }
  return "—";
}
