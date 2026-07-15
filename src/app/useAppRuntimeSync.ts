import { useCodexRetryGatewayQuerySync } from "../hooks/useCodexRetryGatewayQuerySync";
import { useGatewayQuerySync } from "../hooks/useGatewayQuerySync";
import { useSettingsRuntimeBridge } from "./useSettingsRuntimeBridge";

export function useAppRuntimeSync() {
  useCodexRetryGatewayQuerySync();
  useGatewayQuerySync();
  useSettingsRuntimeBridge();
}
