import { commands } from "../generated/bindings";
import type {
  NativeClient,
  NativeTargetSelection,
  NativeTarget,
  NativeProvidersList,
  NativeProviderEdit,
  NativeProviderSaveInput,
  NativeProviderActionInput,
  NativeProviderDeleteInput,
  NativeMutationResult,
  NativeProviderSummary,
} from "../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "./generatedIpc";

export type {
  NativeClient,
  NativeTargetSelection,
  NativeTarget,
  NativeProvidersList,
  NativeProviderEdit,
  NativeProviderSaveInput,
  NativeProviderActionInput,
  NativeProviderDeleteInput,
  NativeMutationResult,
  NativeProviderSummary,
};

function assertTarget(actual: string, expected: string) {
  if (!actual || actual !== expected) throw new Error("IPC_NATIVE_TARGET_MISMATCH");
}

export function nativeCliTargetsList(client: NativeClient) {
  return invokeGeneratedIpc<NativeTarget[]>({
    title: "读取原生配置目标失败",
    cmd: "native_cli_targets_list",
    args: { client },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliTargetsList(client), (targets) => {
        if (targets.some((target) => target.client !== client || !target.targetId))
          throw new Error("IPC_NATIVE_TARGET_MISMATCH");
        return targets;
      }),
  });
}

export function nativeCliTargetValidate(selection: NativeTargetSelection) {
  return invokeGeneratedIpc<NativeTarget>({
    title: "验证原生配置目标失败",
    cmd: "native_cli_target_validate",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliTargetValidate(selection), (target) => {
        if (target.client !== selection.client || !target.targetId)
          throw new Error("IPC_NATIVE_TARGET_MISMATCH");
        return target;
      }),
  });
}

export function nativeCliTargetSelect(selection: NativeTargetSelection) {
  return invokeGeneratedIpc<NativeTarget>({
    title: "选择原生配置目标失败",
    cmd: "native_cli_target_select",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliTargetSelect(selection), (target) => {
        if (target.client !== selection.client || !target.targetId)
          throw new Error("IPC_NATIVE_TARGET_MISMATCH");
        return target;
      }),
  });
}

export function nativeCliProvidersList(targetId: string) {
  return invokeGeneratedIpc<NativeProvidersList>({
    title: "读取原生供应商失败",
    cmd: "native_cli_providers_list",
    args: { targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliProvidersList(targetId), (result) => {
        assertTarget(result.target.targetId, targetId);
        return result;
      }),
  });
}

/** Secret-bearing edits are never cached or included in diagnostic arguments. */
export function nativeCliProviderReadForEdit(targetId: string, nativeKey: string) {
  return invokeGeneratedIpc<NativeProviderEdit>({
    title: "读取原生供应商编辑内容失败",
    cmd: "native_cli_provider_read_for_edit",
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeCliProviderReadForEdit(targetId, nativeKey),
        (result) => {
          assertTarget(result.target.targetId, targetId);
          if (result.provider.nativeKey !== nativeKey)
            throw new Error("IPC_NATIVE_PROVIDER_MISMATCH");
          return result;
        }
      ),
  });
}

export function nativeCliProviderSave(input: NativeProviderSaveInput) {
  return invokeGeneratedIpc<NativeMutationResult>({
    title: "保存原生供应商失败",
    cmd: "native_cli_provider_save",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliProviderSave(input), (result) => {
        assertTarget(result.targetId, input.targetId);
        return result;
      }),
  });
}

export function nativeCliProviderApply(input: NativeProviderActionInput) {
  return invokeGeneratedIpc<NativeMutationResult>({
    title: "应用原生供应商失败",
    cmd: "native_cli_provider_apply",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliProviderApply(input), (result) => {
        assertTarget(result.targetId, input.targetId);
        return result;
      }),
  });
}

export function nativeCliProviderRemove(input: NativeProviderActionInput) {
  return invokeGeneratedIpc<NativeMutationResult>({
    title: "移除原生供应商失败",
    cmd: "native_cli_provider_remove",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliProviderRemove(input), (result) => {
        assertTarget(result.targetId, input.targetId);
        return result;
      }),
  });
}

export function nativeCliProviderDelete(input: NativeProviderDeleteInput) {
  return invokeGeneratedIpc<NativeMutationResult>({
    title: "删除原生供应商档案失败",
    cmd: "native_cli_provider_delete",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeCliProviderDelete(input), (result) => {
        assertTarget(result.targetId, input.targetId);
        return result;
      }),
  });
}
