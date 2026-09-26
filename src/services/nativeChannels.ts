import {
  commands,
  type ChannelModelDiscovery,
  type ModelCapabilitySuggestion,
  type ChannelBindingSummary,
  type ChannelCatalog,
  type ChannelLifecycleInput,
  type ChannelMutationResult,
  type ChannelPreview,
  type ChannelProvider,
  type ChannelSelection,
  type ChannelSource,
  type GatewayModelsSnapshot,
  type GatewayProtocol,
  type GeneratedEntry,
  type NativeModelSpec,
  type SourceChannel,
  type ThinkingSpec,
} from "../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "./generatedIpc";
import { validateProviderId } from "./providers/providers";

export type {
  ChannelModelDiscovery,
  ModelCapabilitySuggestion,
  ChannelBindingSummary,
  ChannelCatalog,
  ChannelLifecycleInput,
  ChannelMutationResult,
  ChannelPreview,
  ChannelProvider,
  ChannelSelection,
  ChannelSource,
  GatewayModelsSnapshot,
  GatewayProtocol,
  GeneratedEntry,
  NativeModelSpec,
  SourceChannel,
  ThinkingSpec,
};

export const SOURCE_CHANNELS: readonly SourceChannel[] = [
  "claude",
  "codex",
  "grok",
  "gemini",
] as const;

export const SOURCE_CHANNEL_LABELS: Record<SourceChannel, string> = {
  claude: "Claude Code",
  codex: "Codex",
  grok: "Grok",
  gemini: "Gemini",
};

export const SOURCE_CHANNEL_PROTOCOLS: Record<SourceChannel, readonly GatewayProtocol[]> = {
  claude: ["anthropic-messages"],
  codex: ["openai-responses"],
  grok: ["openai-completions", "openai-responses"],
  gemini: ["google-generative-ai"],
};

function checkTarget<T extends { targetId: string }>(result: T, targetId: string): T {
  if (result.targetId !== targetId) throw new Error("IPC_NATIVE_TARGET_MISMATCH");
  return result;
}

function checkProvider(
  result: GatewayModelsSnapshot,
  providerId: number,
  providerUuid: string
): GatewayModelsSnapshot {
  if (
    result.providerId !== providerId ||
    result.providerUuid !== providerUuid ||
    !result.revision
  ) {
    throw new Error("IPC_NATIVE_PROVIDER_MISMATCH");
  }
  return result;
}

export function nativeChannelCatalogPreview(targetId: string) {
  return invokeGeneratedIpc<ChannelCatalog>({
    title: "读取统一渠道目录失败",
    cmd: "native_channel_catalog_preview",
    args: { targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeChannelCatalogPreview(targetId), (result) =>
        checkTarget(result, targetId)
      ),
  });
}

export function nativeChannelModelsGet(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol
) {
  validateProviderId(providerId);
  return invokeGeneratedIpc<GatewayModelsSnapshot>({
    title: "读取渠道上游模型声明失败",
    cmd: "native_channel_models_get",
    args: { targetId, providerId, protocol },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeChannelModelsGet(targetId, providerId, providerUuid, protocol),
        (result) => checkProvider(result, providerId, providerUuid)
      ),
  });
}

export function nativeChannelModelsDiscover(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol
) {
  validateProviderId(providerId);
  return invokeGeneratedIpc<ChannelModelDiscovery>({
    title: "自动获取渠道模型能力失败",
    cmd: "native_channel_models_discover",
    args: { targetId, providerId, providerUuid, protocol },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeChannelModelsDiscover(targetId, providerId, providerUuid, protocol),
        (result) => {
          checkTarget(result, targetId);
          if (
            result.providerId !== providerId ||
            result.providerUuid !== providerUuid ||
            result.protocol !== protocol ||
            !result.revision
          ) {
            throw new Error("IPC_NATIVE_PROVIDER_MISMATCH");
          }
          return result;
        }
      ),
  });
}

export function nativeChannelModelsSet(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol,
  expectedRevision: string,
  models: NativeModelSpec[]
) {
  validateProviderId(providerId);
  return invokeGeneratedIpc<GatewayModelsSnapshot>({
    title: "保存渠道上游模型声明失败",
    cmd: "native_channel_models_set",
    args: { targetId, providerId, protocol },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.nativeChannelModelsSet(
          targetId,
          providerId,
          providerUuid,
          protocol,
          expectedRevision,
          models
        ),
        (result) => checkProvider(result, providerId, providerUuid)
      ),
  });
}

export function nativeChannelPreview(input: ChannelLifecycleInput) {
  return invokeGeneratedIpc<ChannelPreview>({
    title: "预览统一渠道接入失败",
    cmd: "native_channel_preview",
    args: { targetId: input.targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeChannelPreview(input), (result) =>
        checkTarget(result, input.targetId)
      ),
  });
}

export function nativeChannelApply(input: ChannelLifecycleInput) {
  return invokeGeneratedIpc<ChannelMutationResult>({
    title: "应用统一渠道接入失败",
    cmd: "native_channel_apply",
    args: { targetId: input.targetId },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.nativeChannelApply(input), (result) =>
        checkTarget(result, input.targetId)
      ),
  });
}
