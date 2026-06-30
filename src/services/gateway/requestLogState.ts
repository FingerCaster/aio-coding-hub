export type RequestLogProgressInput = {
  status: number | null;
  error_code?: string | null;
  created_at?: number;
  created_at_ms?: number | null;
};

export const PENDING_IDLE_NOTICE_MS = 10 * 60 * 1000;

export type RequestLogActivityState = "completed" | "in_progress_active" | "in_progress_idle";

export type RequestSignalLike = {
  phase?: string | null;
};

export function requestLogCreatedAtMs(
  log: Pick<RequestLogProgressInput, "created_at" | "created_at_ms">
) {
  const ms = log.created_at_ms ?? 0;
  if (Number.isFinite(ms) && ms > 0) return ms;
  return (log.created_at ?? 0) * 1000;
}

export function isPersistedRequestLogInProgress(log: RequestLogProgressInput) {
  if (log.status != null || (log.error_code ?? null) != null) return false;
  return true;
}

export function requestLogLastActivityMs(
  log: Pick<RequestLogProgressInput, "created_at" | "created_at_ms"> & {
    last_activity_ms?: number | null;
  }
) {
  const ms = log.last_activity_ms ?? 0;
  if (Number.isFinite(ms) && ms > 0) return ms;
  return requestLogCreatedAtMs(log);
}

export function requestLogActivityState(
  log: RequestLogProgressInput & { last_activity_ms?: number | null },
  nowMs: number
): RequestLogActivityState {
  if (!isPersistedRequestLogInProgress(log)) return "completed";
  const idleForMs = Math.max(0, nowMs - requestLogLastActivityMs(log));
  return idleForMs >= PENDING_IDLE_NOTICE_MS ? "in_progress_idle" : "in_progress_active";
}

export function isRequestSignalComplete(signal: RequestSignalLike | null | undefined) {
  return signal?.phase === "complete";
}
