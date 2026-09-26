import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  nativeChannelCatalogPreview,
  nativeChannelModelsGet,
  nativeChannelModelsDiscover,
  nativeChannelModelsSet,
  nativeChannelApply,
  type ChannelLifecycleInput,
  type GatewayProtocol,
  type NativeModelSpec,
} from "../services/nativeChannels";
import { invalidateNativeGatewayPreviews, invalidateNativeTarget } from "./nativeCli";

export const nativeChannelsKeys = {
  all: ["native-channels"] as const,
  catalog: (targetId: string) => ["native-channels", "catalog", targetId] as const,
  models: (targetId: string, providerId: number, providerUuid: string, protocol: GatewayProtocol) =>
    ["native-channels", "models", targetId, providerId, providerUuid, protocol] as const,
  discovery: (
    targetId: string,
    providerId: number,
    providerUuid: string,
    protocol: GatewayProtocol
  ) => ["native-channels", "discovery", targetId, providerId, providerUuid, protocol] as const,
};

export function useNativeChannelCatalogQuery(targetId: string, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: nativeChannelsKeys.catalog(targetId),
    queryFn: () => nativeChannelCatalogPreview(targetId),
    retry: false,
    staleTime: 0,
    enabled: options?.enabled,
  });
}

export function useNativeChannelModelsQuery(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol,
  options?: { enabled?: boolean }
) {
  return useQuery({
    queryKey: nativeChannelsKeys.models(targetId, providerId, providerUuid, protocol),
    queryFn: () => nativeChannelModelsGet(targetId, providerId, providerUuid, protocol),
    retry: false,
    staleTime: 0,
    enabled: options?.enabled,
  });
}

export function useNativeChannelModelsDiscoverQuery(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol,
  enabled: boolean
) {
  return useQuery({
    queryKey: nativeChannelsKeys.discovery(targetId, providerId, providerUuid, protocol),
    queryFn: () => nativeChannelModelsDiscover(targetId, providerId, providerUuid, protocol),
    enabled,
    retry: false,
    staleTime: 0,
    gcTime: 0,
    refetchOnWindowFocus: false,
  });
}

export function useNativeChannelModelsMutation(
  targetId: string,
  providerId: number,
  providerUuid: string,
  protocol: GatewayProtocol
) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: { expectedRevision: string; models: NativeModelSpec[] }) =>
      nativeChannelModelsSet(
        targetId,
        providerId,
        providerUuid,
        protocol,
        input.expectedRevision,
        input.models
      ),
    onSuccess: async () => {
      await client.cancelQueries({
        queryKey: nativeChannelsKeys.models(targetId, providerId, providerUuid, protocol),
      });
      await Promise.all([
        client.invalidateQueries({
          queryKey: nativeChannelsKeys.models(targetId, providerId, providerUuid, protocol),
        }),
        client.invalidateQueries({
          queryKey: nativeChannelsKeys.catalog(targetId),
        }),
        invalidateNativeGatewayPreviews(client),
      ]);
    },
  });
}

export function useNativeChannelMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: ChannelLifecycleInput) => nativeChannelApply(input),
    onSettled: (_result, _error, variables) => {
      if (variables?.targetId) {
        void invalidateNativeTarget(client, variables.targetId);
        void client.invalidateQueries({
          queryKey: nativeChannelsKeys.catalog(variables.targetId),
        });
      }
    },
  });
}

export async function invalidateNativeChannels(queryClient: QueryClient, targetId: string) {
  await queryClient.cancelQueries({ queryKey: nativeChannelsKeys.catalog(targetId) });
  await queryClient.invalidateQueries({ queryKey: nativeChannelsKeys.catalog(targetId) });
}
