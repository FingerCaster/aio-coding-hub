import { useEffect, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { gatewayEventNames } from "../constants/gatewayEvents";
import { logToConsole } from "../services/consoleLog";
import { subscribeGatewayEvent } from "../services/gateway/gatewayEventBus";
import { type CodexRetryGatewayStatus } from "../services/cli/codexRetryGateway";
import { useCoalescedAsyncRefresh } from "./useCoalescedAsyncRefresh";
import { codexRetryGatewayKeys, cliProxyKeys } from "../query/keys";

const STATUS_INVALIDATE_THROTTLE_MS = 250;

function isRetryGatewayStatusPayload(value: unknown): value is CodexRetryGatewayStatus {
  if (!value || typeof value !== "object") return false;
  const row = value as Partial<CodexRetryGatewayStatus>;
  return (
    typeof row.generation === "number" &&
    typeof row.desired_enabled === "boolean" &&
    typeof row.runtime_phase === "string" &&
    typeof row.route_mode === "string"
  );
}

export function useCodexRetryGatewayQuerySync() {
  const queryClient = useQueryClient();

  const invalidateStatus = useMemo(
    () => () => queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() }),
    [queryClient]
  );
  const invalidateCliProxy = useMemo(
    () => () => queryClient.invalidateQueries({ queryKey: cliProxyKeys.statusAll() }),
    [queryClient]
  );

  const { schedule: scheduleInvalidateStatus } = useCoalescedAsyncRefresh<void, unknown>({
    enabled: true,
    delayMs: STATUS_INVALIDATE_THROTTLE_MS,
    task: async () => {
      await Promise.all([invalidateStatus(), invalidateCliProxy()]);
    },
    onError: (error) => {
      logToConsole("warn", "Codex 外部网关查询缓存失效失败", {
        stage: "useCodexRetryGatewayQuerySync",
        error: String(error),
      });
      return null;
    },
  });

  useEffect(() => {
    let cancelled = false;

    const statusSub = subscribeGatewayEvent(gatewayEventNames.status, (payload) => {
      if (cancelled) return;

      if (isRetryGatewayStatusPayload(payload)) {
        queryClient.setQueryData<CodexRetryGatewayStatus | null>(
          codexRetryGatewayKeys.status(),
          (current) => {
            if (!current || current.generation <= payload.generation) {
              return payload;
            }
            return current;
          }
        );
        return;
      }

      scheduleInvalidateStatus();
    });

    void statusSub.ready.catch((error) => {
      if (cancelled) return;
      statusSub.unsubscribe();
      logToConsole("warn", "Codex 外部网关状态监听初始化失败", {
        stage: "useCodexRetryGatewayQuerySync",
        error: String(error),
      });
    });

    return () => {
      cancelled = true;
      statusSub.unsubscribe();
    };
  }, [queryClient, scheduleInvalidateStatus]);
}
