// Usage: Read-only model suggestions for a Provider draft or probe.
import {
  commands,
  type ProviderModelDiscoveryInput,
  type ProviderModelDiscoveryResult,
} from "../../generated/bindings";
import { invokeGeneratedIpc } from "../generatedIpc";

export type { ProviderModelDiscoveryInput, ProviderModelDiscoveryResult };

export function providerModelsDiscover(input: ProviderModelDiscoveryInput) {
  return invokeGeneratedIpc<ProviderModelDiscoveryResult>({
    title: "获取供应商模型失败",
    cmd: "provider_models_discover",
    args: { input },
    invoke: () => commands.providerModelsDiscover(input),
  });
}
