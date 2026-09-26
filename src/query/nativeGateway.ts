import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  nativeGatewayCatalogPreview,
  nativeGatewayModelsGet,
  nativeGatewayModelsSet,
  nativeGatewayApply,
  nativeGatewayRemove,
  nativeGatewayImportConfirm,
  type GatewayImportConfirmInput,
  type GatewayLifecycleInput,
  type NativeModelSpec,
} from "../services/nativeGateway";
import { providersKeys } from "./keys";
import {
  invalidateNativeTarget,
  invalidateNativeGatewayPreviews,
  nativeCliKeys,
} from "./nativeCli";

export const nativeGatewayModelsKey = (providerId: number, providerUuid: string) =>
  ["native-gateway-models", providerId, providerUuid] as const;

export function useNativeGatewayCatalogQuery(targetId: string) {
  return useQuery({
    queryKey: nativeCliKeys.gateway(targetId),
    queryFn: () => nativeGatewayCatalogPreview(targetId),
    retry: false,
    staleTime: 0,
  });
}

export function useNativeGatewayModelsQuery(providerId: number, providerUuid: string) {
  return useQuery({
    queryKey: nativeGatewayModelsKey(providerId, providerUuid),
    queryFn: () => nativeGatewayModelsGet(providerId, providerUuid),
    retry: false,
    staleTime: 0,
  });
}

export function useNativeGatewayModelsMutation(providerId: number, providerUuid: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: { expectedRevision: string; models: NativeModelSpec[] }) =>
      nativeGatewayModelsSet(providerId, providerUuid, input.expectedRevision, input.models),
    onSuccess: async () => {
      await client.cancelQueries({ queryKey: nativeGatewayModelsKey(providerId, providerUuid) });
      await Promise.all([
        client.invalidateQueries({ queryKey: nativeGatewayModelsKey(providerId, providerUuid) }),
        invalidateNativeGatewayPreviews(client),
      ]);
    },
  });
}

export function useNativeGatewayMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({
      action,
      input,
    }: {
      action: "apply" | "remove";
      input: GatewayLifecycleInput;
    }) => (action === "apply" ? nativeGatewayApply(input) : nativeGatewayRemove(input)),
    onSettled: (_result, _error, variables) =>
      invalidateNativeTarget(client, variables.input.targetId),
  });
}

export function useNativeGatewayImportMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: GatewayImportConfirmInput) => nativeGatewayImportConfirm(input),
    gcTime: 0,
    onSuccess: async (providers, input) => {
      await client.cancelQueries({ queryKey: providersKeys.lists() });
      await Promise.all([
        client.invalidateQueries({ queryKey: providersKeys.lists() }),
        client.invalidateQueries({ queryKey: nativeCliKeys.gateway(input.targetId) }),
      ]);
      return providers;
    },
  });
}
