import { useGatewayMeta } from "./useGatewayMeta";
import { useUpdateMeta } from "./useUpdateMeta";

/**
 * Derived gateway status for Sidebar display.
 */
export function useGatewayStatus() {
  const { gatewayAvailable, gateway, preferredPort } = useGatewayMeta();
  const updateMeta = useUpdateMeta();
  const hasUpdate = !!updateMeta.updateCandidate;
  const isPortable = updateMeta.about?.run_mode === "portable";

  const statusText =
    gatewayAvailable === "checking"
      ? "检查中"
      : gatewayAvailable === "unavailable"
        ? "不可用"
        : gateway == null
          ? "未知"
          : gateway.running
            ? "运行中"
            : "已停止";

  const statusTone =
    gatewayAvailable === "available" && gateway?.running
      ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
      : "bg-secondary text-muted-foreground";

  const portTone =
    gatewayAvailable === "available" && gateway?.running
      ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300"
      : "bg-secondary text-muted-foreground";

  const portText = gatewayAvailable === "available" ? String(gateway?.port ?? preferredPort) : "—";

  const isGatewayRunning = gatewayAvailable === "available" && gateway?.running === true;
  const isGatewayStopped = gatewayAvailable === "available" && gateway != null && !gateway.running;

  return {
    gatewayAvailable,
    statusText,
    statusTone,
    portTone,
    portText,
    isGatewayRunning,
    isGatewayStopped,
    hasUpdate,
    isPortable,
    updateCandidate: updateMeta.updateCandidate,
    updateMeta,
  };
}
