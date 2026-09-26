import { commands, type NativeCliVersionCheck } from "../../generated/bindings";
import type { NativeCliKey } from "../../constants/clis";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import { redactDiagnosticText } from "../diagnosticRedaction";

export type { NativeCliVersionCheck };

export function nativeCliCheckLatestVersion(client: NativeCliKey) {
  return invokeGeneratedIpc<NativeCliVersionCheck>({
    title: "检查新版本失败",
    cmd: "native_cli_check_latest_version",
    args: { client },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliCheckLatestVersion(client), (result) => {
        if (result.client !== client) throw new Error("IPC_NATIVE_CLIENT_MISMATCH");
        return {
          ...result,
          blockedReason: result.blockedReason ? redactDiagnosticText(result.blockedReason) : null,
        };
      }),
  });
}

export function nativeCliUpdate(client: NativeCliKey, planId: string) {
  if (!/^[a-f0-9]{32}$/.test(planId)) throw new Error("安装计划无效，请重新检查版本");
  return invokeGeneratedIpc({
    title: "安装或升级失败",
    cmd: "native_cli_update",
    args: { client, planId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliUpdate(client, planId), (result) => {
        if (result.cliKey !== client) throw new Error("IPC_NATIVE_CLIENT_MISMATCH");
        return {
          ...result,
          output: redactDiagnosticText(result.output, 65536),
          error: result.error ? redactDiagnosticText(result.error) : null,
        };
      }),
  });
}
