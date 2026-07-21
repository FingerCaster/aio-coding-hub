import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import type { QueryClient, QueryFunctionContext } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import {
  defaultRouteProvidersList,
  defaultRouteProvidersSetOrder,
  providerClaudeTerminalLaunchCommand,
  providerUpsert,
  providerDuplicate,
  providerDelete,
  providerOAuthStatus,
  providerAccountUsageFetch,
  providerOAuthFetchLimits,
  providerOAuthResetCodexQuota,
  providerSetEnabled,
  providersList,
  providersReorder,
  providerTestAvailability,
  type CliKey,
  type OAuthLimitsResult,
  type ProviderAccountUsageResult,
  type ProviderOAuthResetCodexQuotaResult,
  type ProviderAvailabilityResult,
  type ProviderRouteRow,
  type ProviderUpsertInput,
  type ProviderSummary,
  validateProviderCliKey,
  validateProviderId,
} from "../services/providers/providers";
import {
  isProviderAccountUsageConfigured,
  readProviderAccountUsageConfig,
} from "../services/providers/providerAccountUsageConfig";
import { logToConsole } from "../services/consoleLog";
import { gatewayCircuitResetProvider } from "../services/gateway/gateway";
import { formatUnknownError } from "../utils/errors";
import {
  advanceProviderModelIdentityGenerationsForProviderId,
  invalidateProviderModelCatalog,
} from "./providerModels";
import {
  gatewayKeys,
  oauthLimitsKeys,
  providerAccountUsageKeys,
  providerModelsKeys,
  providersKeys,
} from "./keys";

export function useProvidersListQuery(cliKey: CliKey, options?: { enabled?: boolean }) {
  const normalizedCliKey = validateProviderCliKey(cliKey);

  return useQuery({
    queryKey: providersKeys.list(normalizedCliKey),
    queryFn: () => providersList(normalizedCliKey),
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
  });
}

export function useDefaultRouteProvidersQuery(cliKey: CliKey, options?: { enabled?: boolean }) {
  const normalizedCliKey = validateProviderCliKey(cliKey);

  return useQuery({
    queryKey: providersKeys.defaultRoute(normalizedCliKey),
    queryFn: () => defaultRouteProvidersList(normalizedCliKey),
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

export function useProviderOAuthStatusQuery(
  providerId: number | null,
  options?: { enabled?: boolean }
) {
  const normalizedProviderId = providerId == null ? null : validateProviderId(providerId);

  return useQuery({
    queryKey: providersKeys.oauthStatus(normalizedProviderId),
    queryFn: () => {
      if (normalizedProviderId == null) return null;
      return providerOAuthStatus(normalizedProviderId);
    },
    enabled: (options?.enabled ?? true) && normalizedProviderId != null,
    placeholderData: keepPreviousData,
  });
}

export async function fetchProviderOAuthStatus(
  queryClient: ReturnType<typeof useQueryClient>,
  providerId: number | null
) {
  if (providerId == null) return null;
  const normalizedProviderId = validateProviderId(providerId);
  return queryClient.fetchQuery({
    queryKey: providersKeys.oauthStatus(normalizedProviderId),
    queryFn: () => providerOAuthStatus(normalizedProviderId),
  });
}

const EMPTY_OAUTH_LIMITS_RESULT: OAuthLimitsResult = {
  limit_short_label: null,
  limit_5h_text: null,
  limit_weekly_text: null,
  limit_5h_reset_at: null,
  limit_weekly_reset_at: null,
  reset_credit_available_count: null,
};

export function normalizeProviderOAuthLimitsResult(
  result: OAuthLimitsResult | null | undefined
): OAuthLimitsResult {
  if (!result) return EMPTY_OAUTH_LIMITS_RESULT;
  return result;
}

export function readProviderOAuthLimitsCache(
  queryClient: QueryClient,
  providerId: number
): OAuthLimitsResult | null {
  const normalizedProviderId = validateProviderId(providerId);
  const state = queryClient.getQueryState<OAuthLimitsResult>(
    oauthLimitsKeys.detail(normalizedProviderId)
  );
  return state?.data ?? null;
}

export async function refreshProviderOAuthLimits(
  queryClient: QueryClient,
  providerId: number,
  options?: { resetCircuitAfterRefresh?: boolean }
): Promise<OAuthLimitsResult> {
  const normalizedProviderId = validateProviderId(providerId);
  const next = normalizeProviderOAuthLimitsResult(
    await providerOAuthFetchLimits(normalizedProviderId)
  );
  queryClient.setQueryData(oauthLimitsKeys.detail(normalizedProviderId), next);
  try {
    if (options?.resetCircuitAfterRefresh) {
      try {
        await gatewayCircuitResetProvider(normalizedProviderId);
      } catch (error) {
        logToConsole("warn", "OAuth 配额刷新成功，但重置熔断器失败", {
          provider_id: normalizedProviderId,
          error: formatUnknownError(error),
        });
      }
    }
  } finally {
    void queryClient.invalidateQueries({ queryKey: gatewayKeys.circuits() });
  }
  return next;
}

export function readProviderAccountUsageCache(
  queryClient: QueryClient,
  providerId: number
): ProviderAccountUsageResult | null {
  const normalizedProviderId = validateProviderId(providerId);
  const state = queryClient.getQueryState<ProviderAccountUsageResult>(
    providerAccountUsageKeys.detail(normalizedProviderId)
  );
  return state?.data ?? null;
}

type ProviderAccountUsageQueryKey = ReturnType<typeof providerAccountUsageKeys.detail>;

function fetchProviderAccountUsageQuery({
  queryKey,
}: QueryFunctionContext<ProviderAccountUsageQueryKey>) {
  return providerAccountUsageFetch(validateProviderId(queryKey[1]));
}

export function providerAccountUsageQueryOptions(providerId: number) {
  const normalizedProviderId = validateProviderId(providerId);

  return queryOptions({
    queryKey: providerAccountUsageKeys.detail(normalizedProviderId),
    queryFn: fetchProviderAccountUsageQuery,
    staleTime: Infinity,
    retry: false,
  });
}

export async function refreshProviderAccountUsage(
  queryClient: QueryClient,
  providerId: number
): Promise<ProviderAccountUsageResult | null> {
  const options = providerAccountUsageQueryOptions(providerId);
  await queryClient.cancelQueries({ queryKey: options.queryKey, exact: true });
  return queryClient.fetchQuery({ ...options, staleTime: 0 });
}

export async function resetProviderOAuthCodexQuota(
  queryClient: QueryClient,
  providerId: number,
  options?: { resetCircuitAfterRefresh?: boolean }
): Promise<ProviderOAuthResetCodexQuotaResult> {
  const normalizedProviderId = validateProviderId(providerId);
  const result = await providerOAuthResetCodexQuota(normalizedProviderId);
  const refreshedLimits = result.refreshed_limits;

  if (refreshedLimits) {
    const next = normalizeProviderOAuthLimitsResult(refreshedLimits);
    queryClient.setQueryData(oauthLimitsKeys.detail(normalizedProviderId), next);
    try {
      if (options?.resetCircuitAfterRefresh) {
        try {
          await gatewayCircuitResetProvider(normalizedProviderId);
        } catch (error) {
          logToConsole("warn", "Codex 重置成功，但重置熔断器失败", {
            provider_id: normalizedProviderId,
            error: formatUnknownError(error),
          });
        }
      }
    } finally {
      void queryClient.invalidateQueries({ queryKey: gatewayKeys.circuits() });
    }
  }

  return result;
}

export function useProviderSetEnabledMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { providerId: number; enabled: boolean }) =>
      providerSetEnabled(input.providerId, input.enabled),
    onSuccess: (updated) => {
      if (!updated) return;

      queryClient.setQueryData<ProviderSummary[] | null>(
        providersKeys.list(updated.cli_key),
        (prev) => {
          if (!prev) return prev;
          return prev.map((row) => (row.id === updated.id ? updated : row));
        }
      );
    },
  });
}

function providerModelConnectionChanged(
  previous: ProviderSummary | undefined,
  saved: ProviderSummary,
  input: ProviderUpsertInput
) {
  // A name, note, pricing, or display-only edit must not invalidate an in-flight
  // catalog refresh. Connection identity changes still advance the generation so
  // an older response cannot replace the backend-marked-stale catalog.
  if (!previous || previous.provider_uuid !== saved.provider_uuid) return true;
  if (
    previous.base_url_mode !== saved.base_url_mode ||
    previous.auth_mode !== saved.auth_mode ||
    previous.source_provider_id !== saved.source_provider_id ||
    previous.bridge_type !== saved.bridge_type ||
    previous.base_urls.length !== saved.base_urls.length ||
    previous.base_urls.some((url, index) => url !== saved.base_urls[index])
  ) {
    return true;
  }

  // The API key is intentionally write-only in the summary. A non-empty value
  // in the submitted payload is therefore the only frontend-side signal that
  // its connection credential changed.
  return input.apiKey != null && input.apiKey.trim().length > 0;
}

export function useProviderUpsertMutation() {
  const queryClient = useQueryClient();

  return useMutation<ProviderSummary, Error, { input: ProviderUpsertInput }>({
    mutationFn: (input: { input: ProviderUpsertInput }) => providerUpsert(input.input),
    onSuccess: async (saved, variables) => {
      const previous = queryClient
        .getQueryData<ProviderSummary[] | null>(providersKeys.list(saved.cli_key))
        ?.find((row) => row.id === saved.id);
      queryClient.setQueryData<ProviderSummary[] | null>(
        providersKeys.list(saved.cli_key),
        (prev) => {
          if (!prev) {
            return [saved];
          }

          const existingIndex = prev.findIndex((row) => row.id === saved.id);
          if (existingIndex === -1) {
            return [...prev, saved];
          }

          return prev.map((row) => (row.id === saved.id ? saved : row));
        }
      );

      queryClient.removeQueries({ queryKey: providerAccountUsageKeys.detail(saved.id) });
      await invalidateProviderModelCatalog(queryClient, saved.id, saved.provider_uuid, {
        advanceGeneration: providerModelConnectionChanged(previous, saved, variables.input),
      });
      void queryClient.invalidateQueries({ queryKey: providersKeys.list(saved.cli_key) });
      void queryClient.invalidateQueries({ queryKey: gatewayKeys.circuitStatus(saved.cli_key) });
    },
  });
}

export function useProviderDeleteMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { cliKey: CliKey; providerId: number; clearUsageStats?: boolean }) =>
      providerDelete(input.providerId, { clearUsageStats: input.clearUsageStats === true }),
    onSuccess: async (ok, input) => {
      if (!ok) return;
      const cliKey = validateProviderCliKey(input.cliKey);

      queryClient.setQueryData<ProviderSummary[] | null>(providersKeys.list(cliKey), (prev) => {
        if (!prev) return prev;
        return prev.filter((row) => row.id !== input.providerId);
      });
      queryClient.removeQueries({ queryKey: providerAccountUsageKeys.detail(input.providerId) });
      advanceProviderModelIdentityGenerationsForProviderId(queryClient, input.providerId);
      await queryClient.cancelQueries({
        queryKey: providerModelsKeys.catalogsByProvider(input.providerId),
      });
      queryClient.removeQueries({
        queryKey: providerModelsKeys.catalogsByProvider(input.providerId),
      });
    },
  });
}

export function useProvidersReorderMutation() {
  const queryClient = useQueryClient();

  return useMutation<
    ProviderSummary[] | null,
    Error,
    {
      cliKey: CliKey;
      orderedProviderIds: number[];
      optimisticProviders?: ProviderSummary[];
    },
    { previousProviders: ProviderSummary[] | null | undefined }
  >({
    mutationFn: (input: {
      cliKey: CliKey;
      orderedProviderIds: number[];
      optimisticProviders?: ProviderSummary[];
    }) => providersReorder(validateProviderCliKey(input.cliKey), input.orderedProviderIds),
    onMutate: async (input) => {
      const cliKey = validateProviderCliKey(input.cliKey);
      await queryClient.cancelQueries({ queryKey: providersKeys.list(cliKey) });
      const previousProviders = queryClient.getQueryData<ProviderSummary[] | null>(
        providersKeys.list(cliKey)
      );
      if (input.optimisticProviders) {
        queryClient.setQueryData(providersKeys.list(cliKey), input.optimisticProviders);
      }
      return { previousProviders };
    },
    onError: (_error, input, context) => {
      if (context?.previousProviders !== undefined) {
        const cliKey = validateProviderCliKey(input.cliKey);
        queryClient.setQueryData(providersKeys.list(cliKey), context.previousProviders);
      }
    },
    onSuccess: (next, input) => {
      if (!next) return;
      const cliKey = validateProviderCliKey(input.cliKey);
      queryClient.setQueryData(providersKeys.list(cliKey), next);
    },
  });
}

export function useDefaultRouteProvidersSetOrderMutation() {
  const queryClient = useQueryClient();

  return useMutation<
    ProviderRouteRow[] | null,
    Error,
    {
      cliKey: CliKey;
      orderedProviderIds: number[];
      optimisticRows?: ProviderRouteRow[];
    },
    { previousRows: ProviderRouteRow[] | null | undefined }
  >({
    mutationFn: (input) =>
      defaultRouteProvidersSetOrder(validateProviderCliKey(input.cliKey), input.orderedProviderIds),
    onMutate: async (input) => {
      const cliKey = validateProviderCliKey(input.cliKey);
      await queryClient.cancelQueries({ queryKey: providersKeys.defaultRoute(cliKey) });
      const previousRows = queryClient.getQueryData<ProviderRouteRow[] | null>(
        providersKeys.defaultRoute(cliKey)
      );
      if (input.optimisticRows) {
        queryClient.setQueryData(providersKeys.defaultRoute(cliKey), input.optimisticRows);
      }
      return { previousRows };
    },
    onError: (_error, input, context) => {
      if (context?.previousRows !== undefined) {
        const cliKey = validateProviderCliKey(input.cliKey);
        queryClient.setQueryData(providersKeys.defaultRoute(cliKey), context.previousRows);
      }
    },
    onSuccess: (next, input) => {
      if (!next) return;
      const cliKey = validateProviderCliKey(input.cliKey);
      queryClient.setQueryData(providersKeys.defaultRoute(cliKey), next);
    },
    onSettled: (_data, _error, input) => {
      const cliKey = validateProviderCliKey(input.cliKey);
      void queryClient.invalidateQueries({ queryKey: providersKeys.defaultRoute(cliKey) });
    },
  });
}

export function useProviderDuplicateMutation() {
  const queryClient = useQueryClient();

  return useMutation<
    ProviderSummary | null,
    Error,
    { providerId: number },
    { sourceProviderId: number }
  >({
    mutationFn: (input: { providerId: number }) => providerDuplicate(input.providerId),
    onMutate: (input) => {
      return { sourceProviderId: input.providerId };
    },
    onSuccess: async (duplicated, _input, context) => {
      if (!duplicated) return;

      const cliKey = duplicated.cli_key;
      const sourceId = context?.sourceProviderId;

      // Insert the duplicated provider right after the source in the cache
      queryClient.setQueryData<ProviderSummary[] | null>(providersKeys.list(cliKey), (prev) => {
        if (!prev) return [duplicated];

        const rows = prev.filter((row) => row.id !== duplicated.id);

        if (sourceId != null) {
          const sourceIndex = rows.findIndex((row) => row.id === sourceId);
          if (sourceIndex !== -1) {
            const next = [...rows];
            next.splice(sourceIndex + 1, 0, duplicated);
            return next;
          }
        }

        return [...rows, duplicated];
      });

      // Persist the new order to the backend
      const currentList = queryClient.getQueryData<ProviderSummary[] | null>(
        providersKeys.list(cliKey)
      );
      if (currentList && currentList.length > 1) {
        try {
          const reordered = await providersReorder(
            cliKey as CliKey,
            currentList.map((p) => p.id)
          );
          if (reordered) {
            queryClient.setQueryData(providersKeys.list(cliKey), reordered);
          }
        } catch (error) {
          await queryClient.invalidateQueries({ queryKey: providersKeys.list(cliKey) });
          throw error;
        }
      } else {
        await queryClient.invalidateQueries({ queryKey: providersKeys.list(cliKey) });
      }
    },
  });
}

export function useProviderClaudeTerminalLaunchCommandMutation() {
  const [isPending, setIsPending] = useState(false);
  const mutateAsync = useCallback(async (input: { providerId: number }) => {
    setIsPending(true);
    try {
      return await providerClaudeTerminalLaunchCommand(input.providerId);
    } finally {
      setIsPending(false);
    }
  }, []);

  return useMemo(() => ({ isPending, mutateAsync }), [isPending, mutateAsync]);
}

export function useOAuthLimitsQuery(providerId: number, enabled: boolean) {
  const normalizedProviderId = validateProviderId(providerId);

  return useQuery({
    queryKey: oauthLimitsKeys.detail(normalizedProviderId),
    queryFn: async (): Promise<OAuthLimitsResult> => {
      return normalizeProviderOAuthLimitsResult(
        await providerOAuthFetchLimits(normalizedProviderId)
      );
    },
    enabled,
    staleTime: 180_000,
    refetchInterval: 180_000,
  });
}

export function useProviderAccountUsageQuery(provider: ProviderSummary, enabled = true) {
  const normalizedProviderId = validateProviderId(provider.id);
  const options = providerAccountUsageQueryOptions(normalizedProviderId);
  const configured = isProviderAccountUsageConfigured(provider);
  const config = readProviderAccountUsageConfig(provider.extension_values);
  const autoFetchEnabled = enabled && provider.enabled && configured;
  const refetchInterval =
    autoFetchEnabled && config.timedRefreshEnabled ? config.refreshIntervalSeconds * 1000 : false;

  return useQuery({
    ...options,
    enabled: autoFetchEnabled,
    refetchInterval,
    meta: {
      configured: enabled && configured,
    },
  });
}

export function useProviderTestAvailabilityMutation() {
  const queryClient = useQueryClient();

  return useMutation<ProviderAvailabilityResult | null, Error, { providerId: number }>({
    mutationFn: (input) => providerTestAvailability(input.providerId),
    onSuccess: (result) => {
      if (!result) return;
      queryClient.invalidateQueries({ queryKey: gatewayKeys.circuits() });
    },
  });
}
