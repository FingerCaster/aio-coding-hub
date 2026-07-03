import { useCallback, useEffect, useMemo, useRef } from "react";
import { gatewayEventNames } from "../constants/gatewayEvents";
import {
  useActiveRequestLogsSnapshotQuery,
  useRequestLogsIncrementalRefreshMutation,
  useRequestLogsListAllQuery,
} from "../query/requestLogs";
import { logToConsole } from "../services/consoleLog";
import { subscribeGatewayEvent } from "../services/gateway/gatewayEventBus";
import { normalizeGatewayRequestSignalEvent } from "../services/gateway/gatewayEvents";
import { isRequestSignalComplete } from "../services/gateway/requestLogState";
import { requestLogGetByTraceId } from "../services/gateway/requestLogs";
import { reconcileTraceFromRequestLog } from "../services/gateway/traceStore";
import { useCoalescedAsyncRefresh } from "./useCoalescedAsyncRefresh";
import { useDocumentVisibility } from "./useDocumentVisibility";
import { useWindowForeground } from "./useWindowForeground";

const TRACE_RECONCILIATION_MAX_DELAY_MS = 30_000;

type UseRequestLogsFeedOptions = {
  limit: number;
  enabled?: boolean;
  liveUpdatesEnabled?: boolean;
  liveUpdateIntervalMs?: number | false;
  refreshOnForeground?: boolean;
  foregroundThrottleMs?: number;
};

function resolveSignalRefreshWindowMs(input: number | false | undefined) {
  if (input === false) return 400;
  if (!Number.isFinite(input) || input == null) return 400;
  return Math.max(200, Math.min(2_000, Math.trunc(input)));
}

function resolveTraceReconciliationRetryDelayMs(baseDelayMs: number, attempt: number) {
  const multiplier = 2 ** Math.min(Math.max(0, attempt), 6);
  return Math.min(TRACE_RECONCILIATION_MAX_DELAY_MS, baseDelayMs * multiplier);
}

export function useRequestLogsFeed({
  limit,
  enabled = true,
  liveUpdatesEnabled = false,
  liveUpdateIntervalMs = false,
  refreshOnForeground = false,
  foregroundThrottleMs = 1000,
}: UseRequestLogsFeedOptions) {
  const foregroundActive = useDocumentVisibility();
  const requestLogsQuery = useRequestLogsListAllQuery(limit, { enabled });
  const activeRequestsQuery = useActiveRequestLogsSnapshotQuery({ enabled });
  const incrementalRefreshMutation = useRequestLogsIncrementalRefreshMutation(limit);
  const liveRefreshEnabled = enabled && liveUpdatesEnabled && foregroundActive;
  const signalSubscriptionEnabled = enabled && liveUpdatesEnabled;
  const liveRefreshWindowMs = resolveSignalRefreshWindowMs(liveUpdateIntervalMs);
  const pendingTraceReconciliationIdsRef = useRef(new Set<string>());
  const traceReconciliationAttemptsRef = useRef(new Map<string, number>());
  const traceReconciliationTimerRef = useRef<number | null>(null);
  const traceReconciliationInFlightRef = useRef(false);
  const scheduleTraceReconciliationRef = useRef<(delayMs?: number) => void>(() => {});
  const { schedule: scheduleLiveRefresh } = useCoalescedAsyncRefresh<void, unknown>({
    enabled: liveRefreshEnabled,
    delayMs: liveRefreshWindowMs,
    task: async () => {
      await Promise.all([incrementalRefreshMutation.mutateAsync(), activeRequestsQuery.refetch()]);
    },
    onError: (error) => {
      logToConsole("warn", "增量刷新请求记录失败", { limit, error: String(error) });
      return null;
    },
  });
  const clearTraceReconciliationTimer = useCallback(() => {
    if (traceReconciliationTimerRef.current == null) return;
    window.clearTimeout(traceReconciliationTimerRef.current);
    traceReconciliationTimerRef.current = null;
  }, []);

  const runTraceReconciliation = useCallback(async () => {
    if (!signalSubscriptionEnabled || traceReconciliationInFlightRef.current) {
      return;
    }

    const traceIds = Array.from(pendingTraceReconciliationIdsRef.current);
    if (traceIds.length === 0) {
      return;
    }

    pendingTraceReconciliationIdsRef.current.clear();
    traceReconciliationInFlightRef.current = true;
    let nextDelayMs = liveRefreshWindowMs;

    await Promise.all(
      traceIds.map(async (traceId) => {
        try {
          const requestLog = await requestLogGetByTraceId(traceId);
          const reconciled = reconcileTraceFromRequestLog(requestLog);
          if (reconciled) {
            traceReconciliationAttemptsRef.current.delete(traceId);
            return;
          }

          const nextAttempt = (traceReconciliationAttemptsRef.current.get(traceId) ?? 0) + 1;
          traceReconciliationAttemptsRef.current.set(traceId, nextAttempt);
          pendingTraceReconciliationIdsRef.current.add(traceId);
          nextDelayMs = Math.max(
            nextDelayMs,
            resolveTraceReconciliationRetryDelayMs(liveRefreshWindowMs, nextAttempt)
          );
        } catch (error) {
          const nextAttempt = (traceReconciliationAttemptsRef.current.get(traceId) ?? 0) + 1;
          traceReconciliationAttemptsRef.current.set(traceId, nextAttempt);
          pendingTraceReconciliationIdsRef.current.add(traceId);
          nextDelayMs = Math.max(
            nextDelayMs,
            resolveTraceReconciliationRetryDelayMs(liveRefreshWindowMs, nextAttempt)
          );
          logToConsole("warn", "按追踪 ID 校准实时请求状态失败", {
            trace_id: traceId,
            error: String(error),
          });
        }
      })
    );

    traceReconciliationInFlightRef.current = false;
    if (pendingTraceReconciliationIdsRef.current.size > 0) {
      scheduleTraceReconciliationRef.current(nextDelayMs);
    }
  }, [liveRefreshWindowMs, signalSubscriptionEnabled]);

  const scheduleTraceReconciliation = useCallback(
    (delayMs = liveRefreshWindowMs) => {
      if (!signalSubscriptionEnabled || traceReconciliationTimerRef.current != null) {
        return;
      }

      traceReconciliationTimerRef.current = window.setTimeout(() => {
        traceReconciliationTimerRef.current = null;
        void runTraceReconciliation();
      }, delayMs);
    },
    [liveRefreshWindowMs, runTraceReconciliation, signalSubscriptionEnabled]
  );
  scheduleTraceReconciliationRef.current = scheduleTraceReconciliation;

  const refreshRequestLogs = useCallback(() => {
    return Promise.all([requestLogsQuery.refetch(), activeRequestsQuery.refetch()]).then(
      ([requestLogsResult]) => requestLogsResult
    );
  }, [activeRequestsQuery, requestLogsQuery]);

  const refreshForForeground = useCallback(() => {
    if (!enabled) {
      return;
    }

    if (liveUpdatesEnabled) {
      scheduleLiveRefresh();
      return;
    }

    void refreshRequestLogs();
  }, [enabled, liveUpdatesEnabled, refreshRequestLogs, scheduleLiveRefresh]);

  useWindowForeground({
    enabled: enabled && refreshOnForeground,
    throttleMs: foregroundThrottleMs,
    onForeground: refreshForForeground,
  });

  useEffect(() => {
    if (!signalSubscriptionEnabled) {
      return;
    }

    let cancelled = false;
    const requestSignalSub = subscribeGatewayEvent(gatewayEventNames.requestSignal, (payload) => {
      const requestSignal = normalizeGatewayRequestSignalEvent(payload);
      if (cancelled || !requestSignal) {
        return;
      }

      if (!isRequestSignalComplete(requestSignal)) {
        return;
      }

      if (foregroundActive) {
        scheduleLiveRefresh();
      }
      pendingTraceReconciliationIdsRef.current.add(requestSignal.trace_id);
      scheduleTraceReconciliation();
    });

    void Promise.allSettled([requestSignalSub.ready]).then((results) => {
      if (cancelled) {
        return;
      }

      const subscribeFailed = results.some((result) => result.status === "rejected");
      if (!subscribeFailed) {
        return;
      }

      requestSignalSub.unsubscribe();
      const failedResult = results.find((result) => result.status === "rejected");
      logToConsole("warn", "请求记录实时监听初始化失败", {
        stage: "useRequestLogsFeed",
        error: String(failedResult?.status === "rejected" ? failedResult.reason : "unknown"),
      });
    });

    return () => {
      cancelled = true;
      requestSignalSub.unsubscribe();
    };
  }, [
    foregroundActive,
    scheduleLiveRefresh,
    scheduleTraceReconciliation,
    signalSubscriptionEnabled,
  ]);

  useEffect(() => {
    if (signalSubscriptionEnabled) {
      return;
    }

    clearTraceReconciliationTimer();
    pendingTraceReconciliationIdsRef.current.clear();
    traceReconciliationAttemptsRef.current.clear();
    traceReconciliationInFlightRef.current = false;
  }, [clearTraceReconciliationTimer, signalSubscriptionEnabled]);

  useEffect(() => {
    return () => {
      clearTraceReconciliationTimer();
      traceReconciliationInFlightRef.current = false;
    };
  }, [clearTraceReconciliationTimer]);

  const requestLogs = useMemo(() => requestLogsQuery.data ?? [], [requestLogsQuery.data]);
  const activeRequests = useMemo(() => activeRequestsQuery.data ?? [], [activeRequestsQuery.data]);
  const requestLogsLoading = requestLogsQuery.isLoading;
  const requestLogsRefreshing =
    (requestLogsQuery.isFetching && !requestLogsQuery.isLoading) ||
    incrementalRefreshMutation.isPending ||
    (activeRequestsQuery.isFetching && !activeRequestsQuery.isLoading);
  const requestLogsAvailable: boolean | null = requestLogsQuery.isLoading
    ? null
    : requestLogsQuery.data != null;

  return {
    requestLogs,
    activeRequests,
    requestLogsLoading,
    requestLogsRefreshing,
    requestLogsAvailable,
    refreshRequestLogs,
  };
}
