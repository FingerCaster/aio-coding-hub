import { RefreshCw } from "lucide-react";
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { refreshProviderAccountUsage, useProviderAccountUsageQuery } from "../../query/providers";
import type {
  ProviderAccountUsageResult,
  ProviderSummary,
} from "../../services/providers/providers";
import { isProviderAccountUsageConfigured } from "../../services/providers/providerAccountUsageConfig";
import { cn } from "../../utils/cn";
import { formatUnknownError } from "../../utils/errors";

function formatNumberAmount(value: number | null | undefined) {
  if (value == null || !Number.isFinite(value)) return null;
  const formatted =
    Math.abs(value) >= 100
      ? value.toFixed(0)
      : Math.abs(value) >= 10
        ? value.toFixed(1)
        : value.toFixed(2);
  return formatted;
}

function formatAmount(value: number | null | undefined, unit: string | null | undefined) {
  const formatted = formatNumberAmount(value);
  if (!formatted) return null;
  return unit ? `${formatted} ${unit}` : formatted;
}

function formatAmountRange(
  usedValue: number | null | undefined,
  totalValue: number | null | undefined,
  unit: string | null | undefined
) {
  const used = formatNumberAmount(usedValue);
  const total = formatAmount(totalValue, unit);
  if (!used || !total) return null;
  return `${used}/${total}`;
}

function resultTone(status: ProviderAccountUsageResult["status"]) {
  switch (status) {
    case "available":
      return "text-emerald-700 dark:text-emerald-400";
    case "zero_balance":
    case "expired":
    case "auth_failed":
      return "text-rose-700 dark:text-rose-400";
    case "configuration_required":
    case "query_failed":
      return "text-amber-700 dark:text-amber-400";
    default:
      return "text-muted-foreground";
  }
}

function statusLabel(status: ProviderAccountUsageResult["status"]) {
  switch (status) {
    case "available":
      return "可用";
    case "zero_balance":
      return "无可用额度";
    case "expired":
      return "已过期";
    case "auth_failed":
      return "认证失败";
    case "configuration_required":
      return "需配置";
    case "query_failed":
      return "查询失败";
    default:
      return "未支持";
  }
}

function hasPositiveAmount(value: number | null | undefined) {
  return value != null && Number.isFinite(value) && value > 0;
}

function buildUsageMetric(
  label: string,
  usedValue: number | null | undefined,
  totalValue: number | null | undefined,
  unit: string | null | undefined,
  options: { usedOnlyLabel?: string; totalOnlyLabel?: string } = {}
) {
  const range = formatAmountRange(usedValue, totalValue, unit);
  const used = formatAmount(usedValue, unit);
  const total = formatAmount(totalValue, unit);
  const usedOnlyLabel = options.usedOnlyLabel ?? `${label}已用`;
  const totalOnlyLabel = options.totalOnlyLabel ?? `${label}额度`;

  if (range && hasPositiveAmount(totalValue)) return `${label} ${range}`;
  if (used) return `${usedOnlyLabel} ${used}`;
  if (total && hasPositiveAmount(totalValue)) return `${totalOnlyLabel} ${total}`;
  return null;
}

function buildUsageDisplay(result: ProviderAccountUsageResult | null) {
  if (!result) {
    return { summary: "账户: 未刷新", metrics: [] as string[], title: "刷新账户用量" };
  }

  const unit = result.unit;
  const parts = [statusLabel(result.status)];
  const balance = formatAmount(result.balance, unit);
  const planRemaining = formatAmount(result.plan_remaining, unit);
  const metrics = [
    buildUsageMetric("已用", result.used, result.total, unit, {
      usedOnlyLabel: "已用",
      totalOnlyLabel: "总额",
    }),
    buildUsageMetric("日", result.daily_used, result.daily_total, unit),
    buildUsageMetric("周", result.weekly_used, result.weekly_total, unit),
    buildUsageMetric("月", result.monthly_used, result.monthly_total, unit),
  ].filter((metric): metric is string => Boolean(metric));

  if (result.plan_name) parts.push(result.plan_name);
  if (planRemaining) parts.push(`套餐剩余 ${planRemaining}`);
  if (balance) parts.push(`余额 ${balance}`);
  if (result.message && parts.length === 1) parts.push(result.message);

  const summary = `账户: ${parts.join(" · ")}`;

  return {
    summary,
    metrics,
    title: [summary, result.message, result.unit_note, ...metrics].filter(Boolean).join("\n"),
  };
}

export function ProviderAccountUsageInline({
  provider,
  className,
  segmentClassName,
}: {
  provider: ProviderSummary;
  className?: string;
  segmentClassName?: string;
}) {
  const configured = isProviderAccountUsageConfigured(provider);
  const queryClient = useQueryClient();
  const { data = null } = useProviderAccountUsageQuery(provider, configured);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState<string | null>(null);

  if (!configured) return null;

  const display = buildUsageDisplay(data);
  const text = refreshError ?? display.summary;
  const metrics = refreshError || refreshing ? [] : display.metrics;
  const tone = refreshError
    ? "text-amber-700 dark:text-amber-400"
    : resultTone(data?.status ?? "unsupported");

  return (
    <span className={cn("inline-flex min-w-0 flex-wrap items-center gap-2", className)}>
      <button
        type="button"
        onClick={(event) => {
          event.stopPropagation();
          if (refreshing) return;
          setRefreshing(true);
          setRefreshError(null);
          void refreshProviderAccountUsage(queryClient, provider.id)
            .catch((error) => setRefreshError(formatUnknownError(error)))
            .finally(() => setRefreshing(false));
        }}
        disabled={refreshing}
        aria-label={`刷新账户用量，${refreshing ? "账户: 刷新中" : [text, ...metrics].join("，")}`}
        className={cn(
          "inline-flex min-w-0 max-w-full shrink items-start gap-1 rounded-sm font-mono text-xs text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60",
          tone,
          segmentClassName
        )}
        title={refreshError ?? display.title}
      >
        <RefreshCw
          className={cn("mt-0.5 h-3 w-3 shrink-0", refreshing && "animate-spin")}
          aria-hidden="true"
        />
        <span className="flex min-w-0 max-w-full flex-col gap-1">
          <span className="min-w-0 max-w-full truncate">{refreshing ? "账户: 刷新中" : text}</span>
          {metrics.length ? (
            <span className="flex max-w-full flex-nowrap gap-1.5 overflow-hidden">
              {metrics.map((metric) => (
                <span
                  key={metric}
                  className="shrink-0 rounded-sm bg-muted px-1.5 py-0.5 text-[10px] leading-none text-muted-foreground"
                >
                  {metric}
                </span>
              ))}
            </span>
          ) : null}
        </span>
      </button>
    </span>
  );
}
