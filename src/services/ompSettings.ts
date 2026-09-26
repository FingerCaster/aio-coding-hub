import { commands } from "../generated/bindings";
import type { OmpSettingsSaveInput, OmpAgentSaveInput } from "../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "./generatedIpc";

export type {
  OmpSettingsSnapshot,
  OmpSettingField,
  OmpSettingPatch,
  OmpModelOption,
  OmpAgentSummary,
  OmpAgentDocument,
} from "../generated/bindings";

function checkTarget<T extends { targetId: string }>(value: T, targetId: string): T {
  if (!targetId || value.targetId !== targetId) throw new Error("IPC_NATIVE_TARGET_MISMATCH");
  return value;
}
export function ompSettingsRead(targetId: string) {
  return invokeGeneratedIpc({
    title: "读取 OMP 设置失败",
    cmd: "omp_settings_read",
    args: { targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.ompSettingsRead(targetId), (value) =>
        checkTarget(value, targetId)
      ),
  });
}
export function ompSettingsSave(input: OmpSettingsSaveInput) {
  return invokeGeneratedIpc({
    title: "保存 OMP 设置失败",
    cmd: "omp_settings_save",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.ompSettingsSave(input), (value) =>
        checkTarget(value, input.targetId)
      ),
  });
}
/** Agent prompts/frontmatter are explicitly read for editing, never query-cached/logged. */
export function ompAgentRead(targetId: string, fileName: string) {
  return invokeGeneratedIpc({
    title: "读取 OMP Agent 失败",
    cmd: "omp_agent_read",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.ompAgentRead(targetId, fileName), (value) => {
        checkTarget(value, targetId);
        if (value.fileName !== fileName) throw new Error("IPC_OMP_AGENT_MISMATCH");
        return value;
      }),
  });
}
export function ompAgentSave(input: OmpAgentSaveInput) {
  return invokeGeneratedIpc({
    title: "保存 OMP Agent 失败",
    cmd: "omp_agent_save",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.ompAgentSave(input), (value) =>
        checkTarget(value, input.targetId)
      ),
  });
}
