import {
  commands,
  type GatewayCatalogPreview,
  type GatewayLifecycleInput,
  type GatewayMutationResult,
  type GatewayImportPreview,
  type GatewayImportConfirmInput,
  type NativeModelSpec,
  type GatewayModelsSnapshot,
} from "../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "./generatedIpc";
import { toProviderSummary, validateProviderId } from "./providers/providers";
export type {
  GatewayCatalogPreview,
  GatewayLifecycleInput,
  GatewayMutationResult,
  GatewayImportPreview,
  GatewayImportConfirmInput,
  NativeModelSpec,
  GatewayModelsSnapshot,
};

function checkTarget<T extends { targetId: string }>(result: T, targetId: string): T {
  if (result.targetId !== targetId) throw new Error("IPC_NATIVE_TARGET_MISMATCH");
  return result;
}

function checkProvider(result: GatewayModelsSnapshot, providerId: number, providerUuid: string) {
  if (result.providerId !== providerId || result.providerUuid !== providerUuid || !result.revision)
    throw new Error("IPC_NATIVE_PROVIDER_MISMATCH");
  return result;
}

export function nativeGatewayModelsGet(providerId: number, providerUuid: string) {
  validateProviderId(providerId);
  return invokeGeneratedIpc<GatewayModelsSnapshot>({
    title: "读取网关模型声明失败",
    cmd: "native_gateway_models_get",
    args: { providerId },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeGatewayModelsGet(providerId, providerUuid),
        (result) => checkProvider(result, providerId, providerUuid)
      ),
  });
}

export function nativeGatewayModelsSet(
  providerId: number,
  providerUuid: string,
  expectedRevision: string,
  models: NativeModelSpec[]
) {
  validateProviderId(providerId);
  return invokeGeneratedIpc<GatewayModelsSnapshot>({
    title: "保存网关模型声明失败",
    cmd: "native_gateway_models_set",
    args: { providerId },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeGatewayModelsSet(providerId, providerUuid, expectedRevision, models),
        (result) => checkProvider(result, providerId, providerUuid)
      ),
  });
}

export function nativeGatewayCatalogPreview(targetId: string) {
  return invokeGeneratedIpc<GatewayCatalogPreview>({
    title: "读取 AIO 入口预览失败",
    cmd: "native_gateway_catalog_preview",
    args: { targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeGatewayCatalogPreview(targetId), (result) =>
        checkTarget(result, targetId)
      ),
  });
}

export function nativeGatewayApply(input: GatewayLifecycleInput) {
  return invokeGeneratedIpc<GatewayMutationResult>({
    title: "应用 AIO 入口失败",
    cmd: "native_gateway_apply",
    args: { targetId: input.targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeGatewayApply(input), (result) =>
        checkTarget(result, input.targetId)
      ),
  });
}

export function nativeGatewayRemove(input: GatewayLifecycleInput) {
  return invokeGeneratedIpc<GatewayMutationResult>({
    title: "移除 AIO 入口失败",
    cmd: "native_gateway_remove",
    args: { targetId: input.targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeGatewayRemove(input), (result) =>
        checkTarget(result, input.targetId)
      ),
  });
}

export function nativeGatewayImportPreview(targetId: string, nativeKey: string) {
  return invokeGeneratedIpc<GatewayImportPreview>({
    title: "预览原生网关导入失败",
    cmd: "native_gateway_import_preview",
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeGatewayImportPreview(targetId, nativeKey),
        (result) => {
          checkTarget(result, targetId);
          if (result.nativeKey !== nativeKey) throw new Error("IPC_NATIVE_PROVIDER_MISMATCH");
          return result;
        }
      ),
  });
}

export function nativeGatewayImportConfirm(input: GatewayImportConfirmInput) {
  // API keys and native source content must never enter IPC diagnostics.
  return invokeGeneratedIpc({
    title: "导入网关上游失败",
    cmd: "native_gateway_import_confirm",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeGatewayImportConfirm(input), (rows) =>
        rows.map(toProviderSummary)
      ),
  });
}
