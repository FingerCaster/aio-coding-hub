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

function formatAmount(value: number | null | undefined, unit: string | null | undefined) {
  if (value == null || !Number.isFinite(value)) return null;
  const formatted =
    Math.abs(value) >= 100
      ? value.toFixed(0)
      : Math.abs(value) >= 10
        ? value.toFixed(1)
        : value.toFixed(2);
  return unit ? `${formatted} ${unit}` : formatted;
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
      return "余额 0";
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

function buildUsageText(result: ProviderAccountUsageResult | null) {
  if (!result) return "账户: 未刷新";

  const unit = result.unit;
  const parts = [statusLabel(result.status)];
  const balance = formatAmount(result.balance, unit);
  const used = formatAmount(result.used, unit);
  const total = formatAmount(result.total, unit);
  const monthlyUsed = formatAmount(result.monthly_used, unit);
  const monthlyTotal = formatAmount(result.monthly_total, unit);
  const dailyUsed = formatAmount(result.daily_used, unit);
  const dailyTotal = formatAmount(result.daily_total, unit);

  if (result.plan_name) parts.push(result.plan_name);
  if (balance) parts.push(`余额 ${balance}`);
  if (used && total) parts.push(`已用 ${used}/${total}`);
  if (monthlyUsed && monthlyTotal) parts.push(`月 ${monthlyUsed}/${monthlyTotal}`);
  if (dailyUsed && dailyTotal) parts.push(`日 ${dailyUsed}/${dailyTotal}`);
  if (result.message && parts.length === 1) parts.push(result.message);

  return `账户: ${parts.join(" · ")}`;
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

  const text = refreshError ?? buildUsageText(data);
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
        className={cn(
          "inline-flex shrink-0 items-center gap-1 rounded-sm font-mono text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-60",
          tone,
          segmentClassName
        )}
        title={data?.message ?? data?.unit_note ?? "刷新账户用量"}
      >
        <RefreshCw className={cn("h-3 w-3", refreshing && "animate-spin")} aria-hidden="true" />
        <span>{refreshing ? "账户: 刷新中" : text}</span>
      </button>
    </span>
  );
}
