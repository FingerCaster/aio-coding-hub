import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  nativeCliTargetsList,
  nativeCliTargetSelect,
  nativeCliTargetValidate,
  nativeCliProvidersList,
  nativeCliProviderSave,
  nativeCliProviderApply,
  nativeCliProviderRemove,
  nativeCliProviderDelete,
  type NativeClient,
  type NativeTargetSelection,
  type NativeProviderActionInput,
  type NativeProviderDeleteInput,
  type NativeProviderSaveInput,
} from "../services/nativeCli";

export const nativeCliKeys = {
  all: ["native-cli"] as const,
  targets: (client: NativeClient) => ["native-cli", "targets", client] as const,
  providers: (targetId: string) => ["native-cli", "providers", targetId] as const,
  gateway: (targetId: string) => ["native-cli", "gateway", targetId] as const,
};

export function useNativeCliTargetsQuery(client: NativeClient) {
  return useQuery({
    queryKey: nativeCliKeys.targets(client),
    queryFn: () => nativeCliTargetsList(client),
    staleTime: 30_000,
    retry: false,
  });
}

export function useNativeCliProvidersQuery(targetId: string) {
  return useQuery({
    queryKey: nativeCliKeys.providers(targetId),
    queryFn: () => nativeCliProvidersList(targetId),
    staleTime: 10_000,
    retry: false,
  });
}

export async function invalidateNativeTarget(queryClient: QueryClient, targetId: string) {
  // IPC cannot be aborted, but cancellation fences off late query completions.
  await Promise.all([
    queryClient.cancelQueries({ queryKey: nativeCliKeys.providers(targetId) }),
    queryClient.cancelQueries({ queryKey: nativeCliKeys.gateway(targetId) }),
  ]);
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: nativeCliKeys.providers(targetId) }),
    queryClient.invalidateQueries({ queryKey: nativeCliKeys.gateway(targetId) }),
  ]);
}

export async function invalidateNativeGatewayPreviews(queryClient: QueryClient) {
  const filters = {
    queryKey: nativeCliKeys.all,
    predicate: (query: { queryKey: readonly unknown[] }) => query.queryKey[1] === "gateway",
  };
  await queryClient.cancelQueries(filters);
  await queryClient.invalidateQueries(filters);
}

export function useNativeCliTargetValidateMutation() {
  return useMutation({ mutationFn: nativeCliTargetValidate });
}

export function useNativeCliTargetSelectMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: nativeCliTargetSelect,
    onSuccess: async (_target, input: NativeTargetSelection) => {
      await client.cancelQueries({ queryKey: nativeCliKeys.targets(input.client) });
      await client.invalidateQueries({ queryKey: nativeCliKeys.targets(input.client) });
    },
  });
}

type NativeProviderMutation =
  | { action: "save"; input: NativeProviderSaveInput }
  | { action: "apply" | "remove"; input: NativeProviderActionInput }
  | { action: "delete"; input: NativeProviderDeleteInput };

export function useNativeCliProviderMutation() {
  const client = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (mutation: NativeProviderMutation) => {
      switch (mutation.action) {
        case "save":
          return nativeCliProviderSave(mutation.input);
        case "apply":
          return nativeCliProviderApply(mutation.input);
        case "remove":
          return nativeCliProviderRemove(mutation.input);
        case "delete":
          return nativeCliProviderDelete(mutation.input);
      }
    },
    onSettled: (_data, _error, variables) =>
      invalidateNativeTarget(client, variables.input.targetId),
  });
}
