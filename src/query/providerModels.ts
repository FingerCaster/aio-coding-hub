import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient, QueryFunctionContext } from "@tanstack/react-query";
import {
  codexManagedProfileCreate,
  codexManagedProfileDelete,
  codexManagedProfilesList,
  type CodexManagedProfile,
} from "../services/providers/codexManagedProfiles";
import {
  providerModelCapabilitiesUpdate,
  providerModelManualDelete,
  providerModelManualUpsert,
  providerModelsGet,
  providerModelsRefresh,
  type ProviderModelCatalog,
  type ProviderModelCapabilitiesInput,
  validateProviderUuid,
} from "../services/providers/providerModels";
import { validateProviderId } from "../services/providers/providers";
import { codexManagedProfilesKeys, providerModelsKeys } from "./keys";

type ProviderModelsQueryKey = ReturnType<typeof providerModelsKeys.catalog>;
type ProviderModelsMutationContext = {
  globalGeneration: number;
  identityGeneration: number;
};

type CodexManagedProfileCreateInput = {
  profileName: string;
  modelUuid: string;
  providerId: number;
  providerUuid: string;
};

type CodexManagedProfileDeleteInput = {
  profileUuid: string;
  providerId: number;
  providerUuid: string;
};

type ProviderModelCapabilitiesUpdateInput = ProviderModelCapabilitiesInput & {
  providerId: number;
  providerUuid: string;
  modelUuid: string;
};

const providerModelsGlobalGenerations = new WeakMap<QueryClient, number>();
const providerModelIdentityGenerations = new WeakMap<QueryClient, Map<string, number>>();

function getProviderModelsGlobalGeneration(queryClient: QueryClient) {
  return providerModelsGlobalGenerations.get(queryClient) ?? 0;
}

export function advanceProviderModelsGlobalGeneration(queryClient: QueryClient) {
  providerModelsGlobalGenerations.set(
    queryClient,
    getProviderModelsGlobalGeneration(queryClient) + 1
  );
}

function providerModelIdentityKey(providerId: number, providerUuid: string) {
  return `${providerId}:${providerUuid}`;
}

function getProviderModelIdentityGeneration(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string
) {
  return (
    providerModelIdentityGenerations
      .get(queryClient)
      ?.get(providerModelIdentityKey(providerId, providerUuid)) ?? 0
  );
}

function ensureProviderModelIdentityGeneration(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string
) {
  let generations = providerModelIdentityGenerations.get(queryClient);
  if (!generations) {
    generations = new Map();
    providerModelIdentityGenerations.set(queryClient, generations);
  }
  const key = providerModelIdentityKey(providerId, providerUuid);
  if (!generations.has(key)) generations.set(key, 0);
  return generations.get(key) ?? 0;
}

function advanceProviderModelIdentityGeneration(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string
) {
  let generations = providerModelIdentityGenerations.get(queryClient);
  if (!generations) {
    generations = new Map();
    providerModelIdentityGenerations.set(queryClient, generations);
  }
  const key = providerModelIdentityKey(providerId, providerUuid);
  generations.set(key, (generations.get(key) ?? 0) + 1);
}

/**
 * Invalidate every UUID that has been observed for a provider ID before its
 * row is removed. A later provider may reuse the numeric ID with a new UUID;
 * the UUID-qualified cache keys remain isolated.
 */
export function advanceProviderModelIdentityGenerationsForProviderId(
  queryClient: QueryClient,
  providerId: number
) {
  const normalizedProviderId = validateProviderId(providerId);
  const generations = providerModelIdentityGenerations.get(queryClient);
  if (!generations) return;
  const prefix = `${normalizedProviderId}:`;
  for (const [key, generation] of generations) {
    if (key.startsWith(prefix)) generations.set(key, generation + 1);
  }
}

function captureProviderModelsMutationContext(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string
): ProviderModelsMutationContext {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  return {
    globalGeneration: getProviderModelsGlobalGeneration(queryClient),
    identityGeneration: ensureProviderModelIdentityGeneration(
      queryClient,
      normalizedProviderId,
      normalizedProviderUuid
    ),
  };
}

function providerModelsMutationContextIsCurrent(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string,
  expectedContext: ProviderModelsMutationContext
) {
  return (
    getProviderModelsGlobalGeneration(queryClient) === expectedContext.globalGeneration &&
    getProviderModelIdentityGeneration(queryClient, providerId, providerUuid) ===
      expectedContext.identityGeneration
  );
}

function fetchProviderModelCatalog({ queryKey }: QueryFunctionContext<ProviderModelsQueryKey>) {
  const providerId = queryKey[2];
  const providerUuid = queryKey[3];
  if (providerId == null || providerUuid == null) {
    throw new Error("SEC_INVALID_INPUT: missing provider identity");
  }
  return providerModelsGet(validateProviderId(providerId), validateProviderUuid(providerUuid));
}

export function providerModelCatalogQueryOptions(providerId: number, providerUuid: string) {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  return queryOptions({
    queryKey: providerModelsKeys.catalog(normalizedProviderId, normalizedProviderUuid),
    queryFn: fetchProviderModelCatalog,
    staleTime: Infinity,
    retry: false,
  });
}

export function useProviderModelCatalogQuery(
  providerId: number | null,
  providerUuid: string | null,
  options?: { enabled?: boolean }
) {
  const normalizedProviderId = providerId == null ? null : validateProviderId(providerId);
  const normalizedProviderUuid = providerUuid == null ? null : validateProviderUuid(providerUuid);
  return useQuery({
    queryKey: providerModelsKeys.catalog(normalizedProviderId, normalizedProviderUuid),
    queryFn: fetchProviderModelCatalog,
    enabled:
      (options?.enabled ?? true) && normalizedProviderId != null && normalizedProviderUuid != null,
    staleTime: Infinity,
    retry: false,
  });
}

export function useCodexManagedProfilesQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: codexManagedProfilesKeys.list(),
    queryFn: codexManagedProfilesList,
    enabled: options?.enabled ?? true,
    staleTime: Infinity,
    retry: false,
  });
}

export async function invalidateProviderModelCatalog(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string,
  options: { advanceGeneration?: boolean } = {}
) {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  if (options.advanceGeneration !== false) {
    advanceProviderModelIdentityGeneration(
      queryClient,
      normalizedProviderId,
      normalizedProviderUuid
    );
  }
  const key = providerModelsKeys.catalog(normalizedProviderId, normalizedProviderUuid);
  await queryClient.cancelQueries({ queryKey: key, exact: true });
  await queryClient.invalidateQueries({ queryKey: key, exact: true });
}

async function commitCatalogResult(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string,
  result: ProviderModelCatalog,
  expectedContext: ProviderModelsMutationContext
) {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  if (
    result.providerId !== normalizedProviderId ||
    result.providerUuid !== normalizedProviderUuid
  ) {
    throw new Error("IPC_PROVIDER_MODEL_SCOPE_MISMATCH");
  }
  const contextStillCurrent = () =>
    providerModelsMutationContextIsCurrent(
      queryClient,
      normalizedProviderId,
      normalizedProviderUuid,
      expectedContext
    );
  if (!contextStillCurrent()) return;
  const key = providerModelsKeys.catalog(normalizedProviderId, normalizedProviderUuid);
  await queryClient.cancelQueries({ queryKey: key, exact: true });
  if (!contextStillCurrent()) return;
  queryClient.setQueryData(key, result);
  await queryClient.invalidateQueries({ queryKey: codexManagedProfilesKeys.list(), exact: true });
}

async function reconcileStaleManagedProfileMutation(queryClient: QueryClient) {
  const profilesKey = codexManagedProfilesKeys.list();
  const queryState = queryClient.getQueryState(profilesKey);
  if (!queryState) return;

  // Keep an import-triggered refetch alive, then run one more pass if it was
  // already in flight when the backend profile operation completed.
  const wasFetching = queryState.fetchStatus !== "idle";
  await queryClient.invalidateQueries(
    { queryKey: profilesKey, exact: true, refetchType: "none" },
    { cancelRefetch: false }
  );
  if (!queryClient.getQueryState(profilesKey)) return;
  await queryClient.refetchQueries(
    { queryKey: profilesKey, exact: true, type: "all" },
    { cancelRefetch: false }
  );
  if (!wasFetching || !queryClient.getQueryState(profilesKey)) return;
  await queryClient.refetchQueries(
    { queryKey: profilesKey, exact: true, type: "all" },
    { cancelRefetch: false }
  );
}

async function invalidateManagedProfileCatalogIfPresent(
  queryClient: QueryClient,
  providerId: number,
  providerUuid: string
) {
  const key = providerModelsKeys.catalog(providerId, providerUuid);
  if (!queryClient.getQueryState(key)) return;
  await queryClient.invalidateQueries({ queryKey: key, exact: true }, { cancelRefetch: false });
}

async function commitManagedProfileMutation(
  queryClient: QueryClient,
  input: { providerId: number; providerUuid: string },
  expectedContext: ProviderModelsMutationContext,
  updateProfiles: (current: CodexManagedProfile[] | undefined) => CodexManagedProfile[] | undefined
) {
  const providerId = validateProviderId(input.providerId);
  const providerUuid = validateProviderUuid(input.providerUuid);
  const contextStillCurrent = () =>
    providerModelsMutationContextIsCurrent(queryClient, providerId, providerUuid, expectedContext);

  if (!contextStillCurrent()) {
    await reconcileStaleManagedProfileMutation(queryClient);
    return;
  }

  const profilesKey = codexManagedProfilesKeys.list();
  if (queryClient.getQueryState(profilesKey)) {
    await queryClient.cancelQueries({ queryKey: profilesKey, exact: true });
    if (!contextStillCurrent()) {
      await reconcileStaleManagedProfileMutation(queryClient);
      return;
    }
    if (queryClient.getQueryState(profilesKey)) {
      queryClient.setQueryData<CodexManagedProfile[]>(profilesKey, updateProfiles);
    }
  }
  if (!contextStillCurrent()) {
    await reconcileStaleManagedProfileMutation(queryClient);
    return;
  }
  await invalidateManagedProfileCatalogIfPresent(queryClient, providerId, providerUuid);
}

export function useProviderModelsRefreshMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ providerId, providerUuid }: { providerId: number; providerUuid: string }) =>
      providerModelsRefresh(validateProviderId(providerId), validateProviderUuid(providerUuid)),
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: (result, input, context) =>
      commitCatalogResult(
        queryClient,
        input.providerId,
        input.providerUuid,
        result,
        context ?? { globalGeneration: -1, identityGeneration: -1 }
      ),
  });
}

export function useProviderModelManualUpsertMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { providerId: number; providerUuid: string; remoteModelId: string }) =>
      providerModelManualUpsert(input.providerId, input.providerUuid, input.remoteModelId),
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: (result, input, context) =>
      commitCatalogResult(
        queryClient,
        input.providerId,
        input.providerUuid,
        result,
        context ?? { globalGeneration: -1, identityGeneration: -1 }
      ),
  });
}

export function useProviderModelManualDeleteMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { providerId: number; providerUuid: string; modelUuid: string }) =>
      providerModelManualDelete(input.providerId, input.providerUuid, input.modelUuid),
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: (result, input, context) =>
      commitCatalogResult(
        queryClient,
        input.providerId,
        input.providerUuid,
        result,
        context ?? { globalGeneration: -1, identityGeneration: -1 }
      ),
  });
}

export function useProviderModelCapabilitiesUpdateMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ProviderModelCapabilitiesUpdateInput) =>
      providerModelCapabilitiesUpdate(input.providerId, input.providerUuid, input.modelUuid, {
        supportedReasoningEfforts: input.supportedReasoningEfforts,
        defaultReasoningEffort: input.defaultReasoningEffort,
        contextWindow: input.contextWindow,
      }),
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: (result, input, context) =>
      commitCatalogResult(
        queryClient,
        input.providerId,
        input.providerUuid,
        result,
        context ?? { globalGeneration: -1, identityGeneration: -1 }
      ),
  });
}

export function useCodexManagedProfileCreateMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CodexManagedProfileCreateInput) => {
      validateProviderId(input.providerId);
      validateProviderUuid(input.providerUuid);
      return codexManagedProfileCreate(input.profileName, input.modelUuid);
    },
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: async (created, input, context) => {
      const providerId = validateProviderId(input.providerId);
      const providerUuid = validateProviderUuid(input.providerUuid);
      if (created.providerId !== providerId || created.providerUuid !== providerUuid) {
        throw new Error("IPC_MANAGED_PROFILE_PROVIDER_SCOPE_MISMATCH");
      }
      await commitManagedProfileMutation(
        queryClient,
        { providerId, providerUuid },
        context ?? { globalGeneration: -1, identityGeneration: -1 },
        (current) => {
          if (!current) return [created];
          return [
            ...current.filter((profile) => profile.profileUuid !== created.profileUuid),
            created,
          ];
        }
      );
    },
  });
}

export function useCodexManagedProfileDeleteMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CodexManagedProfileDeleteInput) => {
      validateProviderId(input.providerId);
      validateProviderUuid(input.providerUuid);
      return codexManagedProfileDelete(input.profileUuid);
    },
    onMutate: (input): ProviderModelsMutationContext =>
      captureProviderModelsMutationContext(queryClient, input.providerId, input.providerUuid),
    onSuccess: async (result, input, context) => {
      const providerId = validateProviderId(input.providerId);
      const providerUuid = validateProviderUuid(input.providerUuid);
      await commitManagedProfileMutation(
        queryClient,
        { providerId, providerUuid },
        context ?? { globalGeneration: -1, identityGeneration: -1 },
        (current) =>
          result.deleted
            ? (current?.filter((profile) => profile.profileUuid !== input.profileUuid) ?? [])
            : current
      );
    },
  });
}
