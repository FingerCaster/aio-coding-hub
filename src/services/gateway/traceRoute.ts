// Mirrors src-tauri/src/infra/request_logs/queries.rs `route_from_attempts` +
// `has_failover`（row_to_summary: has_failover = route.len() > 1）; keep in sync.
//
// 规则：连续同 provider 的段折叠为一个 hop（同 provider 重试不算切换），
// 真正切换过 provider（hop 数 > 1）才算 failover。判定只看 provider 序列，
// 不看 status——与 Rust 一致（普通 provider_id>0 的 skipped attempt 也计入
// hop）。probe_result=not_triggered 只是规划观测，不代表 provider attempt，
// 因而不计入 hop、failover 或 attempt 数。
//
// 注意：单 provider 失败（含重试后成功）不算 failover——这是与旧前端内联
// 判定（`segments.some(failed)`）的行为差异点，目的是与落库后的徽章一致。

export type TraceRouteSegment = {
  provider: string;
  status: string;
  probe_result?: string | null;
};

export type TraceAttemptIndex = {
  attempt_index: number;
  probe_result?: string | null;
};

export function isNotTriggeredProbeObservation(attempt: { probe_result?: string | null }): boolean {
  return attempt.probe_result === "not_triggered";
}

export function countTraceProviderAttempts(attempts: ReadonlyArray<TraceAttemptIndex>): number {
  const providerAttempts = attempts.filter((attempt) => !isNotTriggeredProbeObservation(attempt));
  if (providerAttempts.length === 0) return 0;

  const latestProviderAttemptIndex = Math.max(
    ...providerAttempts.map((attempt) => attempt.attempt_index)
  );
  const precedingObservations = attempts.filter(
    (attempt) =>
      isNotTriggeredProbeObservation(attempt) && attempt.attempt_index <= latestProviderAttemptIndex
  ).length;

  // Preserve the existing sparse-event inference while removing synthetic
  // observation slots from the displayed provider-attempt count.
  return Math.max(providerAttempts.length, latestProviderAttemptIndex - precedingObservations);
}

export function hasFailoverFromSegments(segments: ReadonlyArray<TraceRouteSegment>): boolean {
  let hopCount = 0;
  let lastProvider: string | null = null;
  for (const seg of segments) {
    if (isNotTriggeredProbeObservation(seg)) continue;
    if (seg.provider === lastProvider) continue;
    lastProvider = seg.provider;
    hopCount += 1;
    if (hopCount > 1) return true;
  }
  return false;
}
