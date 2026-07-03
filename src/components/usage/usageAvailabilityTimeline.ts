import type { GatewayProviderCircuitStatus } from "../../services/gateway/gateway";
import type { RequestLogSummary } from "../../services/gateway/requestLogs";
import type { CliKey } from "../../services/providers/providers";

function isSuccess(status: number | null) {
  return status != null && status >= 200 && status < 400;
}

export type TimeBucket = {
  startMs: number;
  endMs: number;
  totalRequests: number;
  successCount: number;
  availabilityRate: number;
};

export type ProviderTimeline = {
  providerId: number;
  providerName: string;
  cliKey: CliKey;
  totalRequests: number;
  successCount: number;
  availabilityRate: number;
  avgDurationMs: number;
  circuitState: string | null;
  buckets: TimeBucket[];
  density: "dense" | "sparse" | "none";
};

export type AvailabilityTimelineData = {
  providers: ProviderTimeline[];
  bucketStartMs: number;
  bucketEndMs: number;
  bucketCount: number;
  bucketSizeMs: number;
  bucketSizeLabel: string;
};

function computeBucketSizeMs(rangeMs: number): { sizeMs: number; label: string } {
  const TARGET_BUCKETS = 60;
  const raw = rangeMs / TARGET_BUCKETS;

  const candidates = [
    { sizeMs: 5 * 60_000, label: "5 分钟" },
    { sizeMs: 10 * 60_000, label: "10 分钟" },
    { sizeMs: 15 * 60_000, label: "15 分钟" },
    { sizeMs: 24 * 60_000, label: "24 分钟" },
    { sizeMs: 30 * 60_000, label: "30 分钟" },
    { sizeMs: 60 * 60_000, label: "1 小时" },
    { sizeMs: 2 * 60 * 60_000, label: "2 小时" },
    { sizeMs: 4 * 60 * 60_000, label: "4 小时" },
    { sizeMs: 6 * 60 * 60_000, label: "6 小时" },
    { sizeMs: 12 * 60 * 60_000, label: "12 小时" },
    { sizeMs: 24 * 60 * 60_000, label: "1 天" },
  ];

  for (const c of candidates) {
    if (c.sizeMs >= raw) return c;
  }
  return candidates[candidates.length - 1]!;
}

function classifyDensity(totalRequests: number, bucketCount: number): "dense" | "sparse" | "none" {
  if (totalRequests === 0) return "none";
  const avgPerBucket = totalRequests / bucketCount;
  return avgPerBucket >= 2 ? "dense" : "sparse";
}

export function buildAvailabilityTimeline(
  logs: RequestLogSummary[],
  circuitMap: Record<number, GatewayProviderCircuitStatus> | null,
  rangeStartMs: number,
  rangeEndMs: number
): AvailabilityTimelineData {
  const rangeMs = rangeEndMs - rangeStartMs;
  const { sizeMs, label } = computeBucketSizeMs(rangeMs);
  const bucketCount = Math.max(1, Math.ceil(rangeMs / sizeMs));

  const providerMap = new Map<
    number,
    {
      providerId: number;
      providerName: string;
      cliKey: CliKey;
      totalRequests: number;
      successCount: number;
      totalDurationMs: number;
      bucketData: Map<number, { total: number; success: number }>;
    }
  >();

  for (const log of logs) {
    const pid = log.final_provider_id;
    let entry = providerMap.get(pid);
    if (!entry) {
      entry = {
        providerId: pid,
        providerName: log.final_provider_name,
        cliKey: log.cli_key,
        totalRequests: 0,
        successCount: 0,
        totalDurationMs: 0,
        bucketData: new Map(),
      };
      providerMap.set(pid, entry);
    }

    entry.totalRequests++;
    const success = isSuccess(log.status);
    if (success) entry.successCount++;
    entry.totalDurationMs += log.duration_ms;

    const bucketIdx = Math.min(
      bucketCount - 1,
      Math.max(0, Math.floor((log.created_at_ms - rangeStartMs) / sizeMs))
    );
    let bucket = entry.bucketData.get(bucketIdx);
    if (!bucket) {
      bucket = { total: 0, success: 0 };
      entry.bucketData.set(bucketIdx, bucket);
    }
    bucket.total++;
    if (success) bucket.success++;
  }

  const providers: ProviderTimeline[] = [];
  for (const entry of providerMap.values()) {
    const buckets: TimeBucket[] = [];
    for (let i = 0; i < bucketCount; i++) {
      const startMs = rangeStartMs + i * sizeMs;
      const endMs = startMs + sizeMs;
      const data = entry.bucketData.get(i);
      if (data) {
        buckets.push({
          startMs,
          endMs,
          totalRequests: data.total,
          successCount: data.success,
          availabilityRate: data.total > 0 ? data.success / data.total : 0,
        });
      } else {
        buckets.push({
          startMs,
          endMs,
          totalRequests: 0,
          successCount: 0,
          availabilityRate: 0,
        });
      }
    }

    const rate = entry.totalRequests > 0 ? entry.successCount / entry.totalRequests : 0;
    const avgMs = entry.totalRequests > 0 ? entry.totalDurationMs / entry.totalRequests : 0;

    providers.push({
      providerId: entry.providerId,
      providerName: entry.providerName,
      cliKey: entry.cliKey,
      totalRequests: entry.totalRequests,
      successCount: entry.successCount,
      availabilityRate: rate,
      avgDurationMs: avgMs,
      circuitState: circuitMap?.[entry.providerId]?.state ?? null,
      buckets,
      density: classifyDensity(entry.totalRequests, bucketCount),
    });
  }

  providers.sort((a, b) => b.totalRequests - a.totalRequests);

  return {
    providers,
    bucketStartMs: rangeStartMs,
    bucketEndMs: rangeEndMs,
    bucketCount,
    bucketSizeMs: sizeMs,
    bucketSizeLabel: label,
  };
}
