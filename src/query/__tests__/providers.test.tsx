import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
  ProviderAccountUsageResult,
  ProviderSummary,
} from "../../services/providers/providers";
import type { ProviderModelCatalog } from "../../services/providers/providerModels";
import {
  providerOAuthFetchLimits,
  providerOAuthResetCodexQuota,
  providerOAuthStatus,
  providerAccountUsageFetch,
  providerClaudeTerminalLaunchCommand,
  providerDelete,
  providerDuplicate,
  providerTestAvailability,
  providerUpsert,
  providerSetEnabled,
  providersList,
  providersReorder,
} from "../../services/providers/providers";
import { gatewayCircuitResetProvider } from "../../services/gateway/gateway";
import { providerModelsRefresh } from "../../services/providers/providerModels";
import {
  fetchProviderOAuthStatus,
  providerAccountUsageQueryOptions,
  readProviderOAuthLimitsCache,
  readProviderAccountUsageCache,
  refreshProviderAccountUsage,
  refreshProviderOAuthLimits,
  resetProviderOAuthCodexQuota,
  useOAuthLimitsQuery,
  useProviderAccountUsageQuery,
  useProviderClaudeTerminalLaunchCommandMutation,
  useProviderDeleteMutation,
  useProviderDuplicateMutation,
  useProviderOAuthStatusQuery,
  useProviderSetEnabledMutation,
  useProviderTestAvailabilityMutation,
  useProviderUpsertMutation,
  useProvidersListQuery,
  useProvidersReorderMutation,
} from "../providers";
import { useProviderModelsRefreshMutation } from "../providerModels";
import {
  gatewayKeys,
  oauthLimitsKeys,
  providerAccountUsageKeys,
  providerModelsKeys,
  providersKeys,
} from "../keys";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";

vi.mock("../../services/providers/providers", async () => {
  const actual = await vi.importActual<typeof import("../../services/providers/providers")>(
    "../../services/providers/providers"
  );
  return {
    ...actual,
    providersList: vi.fn(),
    providerOAuthStatus: vi.fn(),
    providerAccountUsageFetch: vi.fn(),
    providerOAuthFetchLimits: vi.fn(),
    providerOAuthResetCodexQuota: vi.fn(),
    providerUpsert: vi.fn(),
    providerSetEnabled: vi.fn(),
    providerDelete: vi.fn(),
    providerDuplicate: vi.fn(),
    providerTestAvailability: vi.fn(),
    providersReorder: vi.fn(),
    providerClaudeTerminalLaunchCommand: vi.fn(),
  };
});

vi.mock("../../services/gateway/gateway", async () => {
  const actual = await vi.importActual<typeof import("../../services/gateway/gateway")>(
    "../../services/gateway/gateway"
  );
  return {
    ...actual,
    gatewayCircuitResetProvider: vi.fn(),
  };
});

vi.mock("../../services/providers/providerModels", async () => {
  const actual = await vi.importActual<typeof import("../../services/providers/providerModels")>(
    "../../services/providers/providerModels"
  );
  return {
    ...actual,
    providerModelsRefresh: vi.fn(),
  };
});

function makeProvider(
  partial: Partial<ProviderSummary> & Pick<ProviderSummary, "id" | "cli_key" | "name">
): ProviderSummary {
  return {
    id: partial.id,
    provider_uuid: partial.provider_uuid ?? "11111111-1111-4111-8111-111111111111",
    cli_key: partial.cli_key,
    name: partial.name,
    base_urls: partial.base_urls ?? [],
    base_url_mode: partial.base_url_mode ?? "order",
    claude_models: partial.claude_models ?? {},
    enabled: partial.enabled ?? true,
    priority: partial.priority ?? 0,
    cost_multiplier: partial.cost_multiplier ?? 1,
    limit_5h_usd: partial.limit_5h_usd ?? null,
    limit_daily_usd: partial.limit_daily_usd ?? null,
    daily_reset_mode: partial.daily_reset_mode ?? "fixed",
    daily_reset_time: partial.daily_reset_time ?? "00:00:00",
    limit_weekly_usd: partial.limit_weekly_usd ?? null,
    limit_monthly_usd: partial.limit_monthly_usd ?? null,
    limit_total_usd: partial.limit_total_usd ?? null,
    tags: partial.tags ?? [],
    note: partial.note ?? "",
    created_at: partial.created_at ?? 0,
    updated_at: partial.updated_at ?? 0,
    auth_mode: partial.auth_mode ?? "api_key",
    oauth_provider_type: partial.oauth_provider_type ?? null,
    oauth_email: partial.oauth_email ?? null,
    oauth_expires_at: partial.oauth_expires_at ?? null,
    oauth_last_error: partial.oauth_last_error ?? null,
    source_provider_id: partial.source_provider_id ?? null,
    bridge_type: partial.bridge_type ?? null,
    availability_test_model: partial.availability_test_model ?? null,
    stream_idle_timeout_seconds: partial.stream_idle_timeout_seconds ?? null,
    upstream_retry_policy_override: partial.upstream_retry_policy_override ?? null,
    model_mapping: partial.model_mapping ?? { default_model: null, exact: {} },
    extension_values: partial.extension_values ?? [],
    api_key_configured: partial.api_key_configured ?? false,
    newapi_account_user_id: partial.newapi_account_user_id ?? null,
    newapi_account_access_token_configured: partial.newapi_account_access_token_configured ?? false,
  };
}

function makeAccountUsage(balance: number): ProviderAccountUsageResult {
  return {
    adapter_kind: "newapi",
    status: balance > 0 ? "available" : "zero_balance",
    freshness: "fresh",
    plan_name: null,
    balance,
    plan_remaining: null,
    used: 2,
    total: 3,
    unit: "USD",
    unit_note: null,
    daily_used: null,
    daily_total: null,
    weekly_used: null,
    weekly_total: null,
    monthly_used: null,
    monthly_total: null,
    expires_at: null,
    last_fetched_at: 1_700_000_000,
    message: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("query/providers", () => {
  it("calls providersList with tauri runtime", async () => {
    setTauriRuntime();

    vi.mocked(providersList).mockResolvedValue([]);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useProvidersListQuery("claude"), { wrapper });

    await waitFor(() => {
      expect(providersList).toHaveBeenCalledWith("claude");
    });
  });

  it("useProvidersListQuery normalizes cliKey before cache key and service call", async () => {
    setTauriRuntime();

    vi.mocked(providersList).mockResolvedValue([]);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProvidersListQuery(" claude " as never), { wrapper });

    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });

    expect(providersList).toHaveBeenCalledWith("claude");
    expect(client.getQueryState(providersKeys.list("claude"))).toBeTruthy();
    expect(client.getQueryState(providersKeys.list(" claude " as never))).toBeUndefined();
  });

  it("rejects invalid provider list cliKey before creating query adapters", () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    expect(() => renderHook(() => useProvidersListQuery("opencode" as never), { wrapper })).toThrow(
      "SEC_INVALID_INPUT"
    );
    expect(providersList).not.toHaveBeenCalled();
  });

  it("useProvidersListQuery enters error state when providersList rejects", async () => {
    setTauriRuntime();

    vi.mocked(providersList).mockRejectedValue(new Error("providers query boom"));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProvidersListQuery("claude"), { wrapper });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("respects options.enabled=false", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useProvidersListQuery("claude", { enabled: false }), { wrapper });
    await Promise.resolve();

    expect(providersList).not.toHaveBeenCalled();
  });

  it("normalizes OAuth status providerId before cache key and service call", async () => {
    setTauriRuntime();

    const status = {
      connected: true,
      provider_type: "google",
      email: "test@example.com",
      expires_at: 1700000000,
      has_refresh_token: true,
    };
    vi.mocked(providerOAuthStatus).mockResolvedValue(status);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderOAuthStatusQuery(7), { wrapper });

    await waitFor(() => {
      expect(result.current.data).toEqual(status);
    });

    expect(providerOAuthStatus).toHaveBeenCalledWith(7);
    expect(client.getQueryState(providersKeys.oauthStatus(7))).toBeTruthy();
  });

  it("rejects invalid OAuth status providerId before creating query adapters", () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    expect(() => renderHook(() => useProviderOAuthStatusQuery(0), { wrapper })).toThrow(
      "SEC_INVALID_INPUT"
    );
    expect(providerOAuthStatus).not.toHaveBeenCalled();
    expect(client.getQueryState(providersKeys.oauthStatus(0))).toBeUndefined();
  });

  it("fetchProviderOAuthStatus normalizes providerId before cache key and service call", async () => {
    setTauriRuntime();

    const status = {
      connected: false,
      provider_type: null,
      email: null,
      expires_at: null,
      has_refresh_token: false,
    };
    vi.mocked(providerOAuthStatus).mockResolvedValue(status);

    const client = createTestQueryClient();

    await expect(fetchProviderOAuthStatus(client as never, null)).resolves.toBeNull();
    await expect(fetchProviderOAuthStatus(client as never, 9)).resolves.toEqual(status);
    await expect(fetchProviderOAuthStatus(client as never, Number.NaN)).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );

    expect(providerOAuthStatus).toHaveBeenCalledWith(9);
    expect(client.getQueryData(providersKeys.oauthStatus(9))).toEqual(status);
    expect(client.getQueryState(providersKeys.oauthStatus(Number.NaN))).toBeUndefined();
  });

  it("normalizes OAuth limits providerId before cache reads, refreshes, and query calls", async () => {
    setTauriRuntime();

    const limits = {
      limit_short_label: "5h",
      limit_5h_text: "100 requests",
      limit_weekly_text: null,
      limit_5h_reset_at: 1700000000,
      limit_weekly_reset_at: null,
      reset_credit_available_count: null,
    };
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue(limits);

    const client = createTestQueryClient();
    client.setQueryData(oauthLimitsKeys.detail(11), limits);

    expect(readProviderOAuthLimitsCache(client, 11)).toEqual(limits);
    expect(() => readProviderOAuthLimitsCache(client, 0)).toThrow("SEC_INVALID_INPUT");

    await expect(refreshProviderOAuthLimits(client, 11)).resolves.toEqual(limits);
    await expect(refreshProviderOAuthLimits(client, 0)).rejects.toThrow("SEC_INVALID_INPUT");
    expect(gatewayCircuitResetProvider).not.toHaveBeenCalled();

    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useOAuthLimitsQuery(11, true), { wrapper });

    await waitFor(() => {
      expect(result.current.data).toEqual(limits);
    });

    expect(providerOAuthFetchLimits).toHaveBeenCalledWith(11);
    expect(client.getQueryData(oauthLimitsKeys.detail(11))).toEqual(limits);
    expect(() => renderHook(() => useOAuthLimitsQuery(0, false), { wrapper })).toThrow(
      "SEC_INVALID_INPUT"
    );
  });

  it("shares account usage query options and forces a fresh manual IPC request", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const accountUsage = makeAccountUsage(1);
    vi.mocked(providerAccountUsageFetch).mockResolvedValue(accountUsage);

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 12,
      cli_key: "codex",
      name: "NewAPI",
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi" },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });

    await waitFor(() => {
      expect(result.current.data).toEqual(accountUsage);
    });

    expect(providerAccountUsageFetch).toHaveBeenCalledTimes(1);
    expect(providerAccountUsageFetch).toHaveBeenCalledWith(12);
    expect(client.getQueryData(providerAccountUsageKeys.detail(12))).toEqual(accountUsage);
    expect(readProviderAccountUsageCache(client, 12)).toEqual(accountUsage);

    const sharedOptions = providerAccountUsageQueryOptions(12);
    const query = client.getQueryCache().find({ queryKey: sharedOptions.queryKey, exact: true });
    expect(query?.options.queryFn).toBe(sharedOptions.queryFn);
    expect(providerAccountUsageQueryOptions(12).queryFn).toBe(sharedOptions.queryFn);

    const refreshedAccountUsage = { ...accountUsage, balance: 4 };
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce(refreshedAccountUsage);
    const cancelSpy = vi.spyOn(client, "cancelQueries");
    const fetchSpy = vi.spyOn(client, "fetchQuery");
    await expect(refreshProviderAccountUsage(client, 12)).resolves.toEqual(refreshedAccountUsage);

    expect(providerAccountUsageFetch).toHaveBeenCalledTimes(2);
    expect(cancelSpy).toHaveBeenCalledWith({ queryKey: sharedOptions.queryKey, exact: true });
    expect(fetchSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: sharedOptions.queryKey,
        queryFn: sharedOptions.queryFn,
        staleTime: 0,
        retry: false,
      })
    );
    expect(client.getQueryData(providerAccountUsageKeys.detail(12))).toEqual(refreshedAccountUsage);
    expect(readProviderAccountUsageCache(client, 12)).toEqual(refreshedAccountUsage);
    expect(gatewayCircuitResetProvider).not.toHaveBeenCalled();
  });

  it("keeps a manual result authoritative when an uncancellable initial request finishes late", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const initialOld = deferred<ProviderAccountUsageResult | null>();
    const manualNew = deferred<ProviderAccountUsageResult | null>();
    vi.mocked(providerAccountUsageFetch)
      .mockImplementationOnce(() => initialOld.promise)
      .mockImplementationOnce(() => manualNew.promise);

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 16,
      cli_key: "codex",
      name: "Initial race",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi" },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });

    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledTimes(1));
    let manualPromise!: Promise<ProviderAccountUsageResult | null>;
    act(() => {
      manualPromise = refreshProviderAccountUsage(client, provider.id);
    });
    await waitFor(() => {
      expect(providerAccountUsageFetch).toHaveBeenCalledTimes(2);
      expect(result.current.isFetching).toBe(true);
    });

    const newUsage = makeAccountUsage(9);
    await act(async () => {
      manualNew.resolve(newUsage);
      await manualPromise;
    });
    await waitFor(() => {
      expect(result.current.data).toEqual(newUsage);
      expect(result.current.isFetching).toBe(false);
    });

    await act(async () => {
      initialOld.resolve(makeAccountUsage(0));
      await initialOld.promise;
      await Promise.resolve();
    });
    expect(result.current.data).toEqual(newUsage);
    expect(client.getQueryData(providerAccountUsageKeys.detail(provider.id))).toEqual(newUsage);
  });

  it("keeps a manual result authoritative when an uncancellable timed refetch finishes late", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const initialUsage = makeAccountUsage(1);
    const timedOld = deferred<ProviderAccountUsageResult | null>();
    const manualNew = deferred<ProviderAccountUsageResult | null>();
    vi.mocked(providerAccountUsageFetch)
      .mockResolvedValueOnce(initialUsage)
      .mockImplementationOnce(() => timedOld.promise)
      .mockImplementationOnce(() => manualNew.promise);

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 17,
      cli_key: "codex",
      name: "Timed race",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "newapi",
            timedRefreshEnabled: true,
            refreshIntervalSeconds: 60,
          },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });
    await waitFor(() => expect(result.current.data).toEqual(initialUsage));

    act(() => {
      void result.current.refetch();
    });
    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledTimes(2));

    let manualPromise!: Promise<ProviderAccountUsageResult | null>;
    act(() => {
      manualPromise = refreshProviderAccountUsage(client, provider.id);
    });
    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledTimes(3));

    const newUsage = makeAccountUsage(11);
    await act(async () => {
      manualNew.resolve(newUsage);
      await manualPromise;
    });
    await waitFor(() => expect(result.current.data).toEqual(newUsage));
    await act(async () => {
      timedOld.resolve(makeAccountUsage(0));
      await timedOld.promise;
      await Promise.resolve();
    });

    expect(result.current.data).toEqual(newUsage);
    expect(client.getQueryData(providerAccountUsageKeys.detail(provider.id))).toEqual(newUsage);
  });

  it("serializes repeated manual entry points by canceling the older same-key fetch", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const first = deferred<ProviderAccountUsageResult | null>();
    const second = deferred<ProviderAccountUsageResult | null>();
    vi.mocked(providerAccountUsageFetch)
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    const client = createTestQueryClient();
    const firstRefresh = refreshProviderAccountUsage(client, 18);
    const firstRefreshCanceled = expect(firstRefresh).rejects.toThrow("CancelledError");
    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledTimes(1));
    const secondRefresh = refreshProviderAccountUsage(client, 18);
    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledTimes(2));

    const secondUsage = makeAccountUsage(12);
    second.resolve(secondUsage);
    await expect(secondRefresh).resolves.toEqual(secondUsage);
    first.resolve(makeAccountUsage(2));
    await first.promise;
    await firstRefreshCanceled;

    expect(client.getQueryData(providerAccountUsageKeys.detail(18))).toEqual(secondUsage);
  });

  it("keeps manual account refresh isolated from availability, mutations, and other caches", async () => {
    setTauriRuntime();
    vi.clearAllMocks();

    const client = createTestQueryClient();
    const otherUsage = makeAccountUsage(23);
    const providerList = [makeProvider({ id: 19, cli_key: "codex", name: "Isolated" })];
    const gatewayState = { untouched: true };
    client.setQueryData(providerAccountUsageKeys.detail(20), otherUsage);
    client.setQueryData(providersKeys.list("codex"), providerList);
    client.setQueryData(gatewayKeys.circuits(), gatewayState);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce(makeAccountUsage(7));

    await refreshProviderAccountUsage(client, 19);

    expect(providerTestAvailability).not.toHaveBeenCalled();
    expect(gatewayCircuitResetProvider).not.toHaveBeenCalled();
    expect(providerUpsert).not.toHaveBeenCalled();
    expect(providerSetEnabled).not.toHaveBeenCalled();
    expect(providerDelete).not.toHaveBeenCalled();
    expect(providerDuplicate).not.toHaveBeenCalled();
    expect(providersReorder).not.toHaveBeenCalled();
    expect(providerOAuthFetchLimits).not.toHaveBeenCalled();
    expect(providerOAuthResetCodexQuota).not.toHaveBeenCalled();
    expect(invalidateSpy).not.toHaveBeenCalled();
    expect(client.getQueryData(providerAccountUsageKeys.detail(20))).toEqual(otherUsage);
    expect(client.getQueryData(providersKeys.list("codex"))).toEqual(providerList);
    expect(client.getQueryData(gatewayKeys.circuits())).toEqual(gatewayState);
  });

  it("auto-fetches initial account usage even when timed refresh is disabled", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const accountUsage = {
      adapter_kind: "newapi" as const,
      status: "available" as const,
      freshness: "fresh" as const,
      plan_name: null,
      balance: 1,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    };
    vi.mocked(providerAccountUsageFetch).mockResolvedValue(accountUsage);

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 13,
      cli_key: "codex",
      name: "NewAPI",
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "newapi",
            timedRefreshEnabled: false,
            refreshIntervalSeconds: 60,
          },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });

    await waitFor(() => {
      expect(result.current.data).toEqual(accountUsage);
    });

    const query = client.getQueryCache().find({ queryKey: providerAccountUsageKeys.detail(13) });
    expect((query?.options as { refetchInterval?: unknown }).refetchInterval).toBe(false);
  });

  it("does not auto-fetch provider account usage for disabled providers", async () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 14,
      cli_key: "codex",
      name: "Disabled NewAPI",
      enabled: false,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi" },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });
    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.fetchStatus).toBe("idle");
    expect(providerAccountUsageFetch).not.toHaveBeenCalled();
    expect(readProviderAccountUsageCache(client, 14)).toBeNull();
  });

  it("uses configured account usage polling interval when timed refresh is enabled", () => {
    setTauriRuntime();
    vi.mocked(providerAccountUsageFetch).mockClear();
    vi.mocked(providerAccountUsageFetch).mockResolvedValue({
      adapter_kind: "newapi",
      status: "available",
      freshness: "fresh",
      plan_name: null,
      balance: 1,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    const client = createTestQueryClient();
    const provider = makeProvider({
      id: 15,
      cli_key: "codex",
      name: "Polling NewAPI",
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "newapi",
            timedRefreshEnabled: true,
            refreshIntervalSeconds: 60,
          },
          updatedAt: 1,
        },
      ],
    });
    const wrapper = createQueryWrapper(client);

    renderHook(() => useProviderAccountUsageQuery(provider), { wrapper });

    const query = client.getQueryCache().find({ queryKey: providerAccountUsageKeys.detail(15) });
    expect((query?.options as { refetchInterval?: unknown }).refetchInterval).toBe(60_000);
  });

  it("active OAuth limits refresh resets circuit after every successful refresh", async () => {
    setTauriRuntime();

    const availableLimits = {
      limit_short_label: "5h",
      limit_5h_text: "12%",
      limit_weekly_text: null,
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 1,
    };
    vi.mocked(providerOAuthFetchLimits).mockResolvedValueOnce(availableLimits);
    vi.mocked(gatewayCircuitResetProvider).mockResolvedValue(true);

    const client = createTestQueryClient();

    await expect(
      refreshProviderOAuthLimits(client, 11, { resetCircuitAfterRefresh: true })
    ).resolves.toEqual(availableLimits);

    expect(gatewayCircuitResetProvider).toHaveBeenCalledWith(11);

    vi.mocked(gatewayCircuitResetProvider).mockClear();
    vi.mocked(providerOAuthFetchLimits).mockResolvedValueOnce({
      limit_short_label: "5h",
      limit_5h_text: "0%",
      limit_weekly_text: null,
      limit_5h_reset_at: 1_700_100_000,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 0,
    });

    await expect(
      refreshProviderOAuthLimits(client, 11, { resetCircuitAfterRefresh: true })
    ).resolves.toMatchObject({ limit_5h_text: "0%" });

    expect(gatewayCircuitResetProvider).toHaveBeenCalledWith(11);
  });

  it("active OAuth limits refresh keeps refreshed limits when circuit reset fails", async () => {
    setTauriRuntime();

    const limits = {
      limit_short_label: "5h",
      limit_5h_text: "24%",
      limit_weekly_text: null,
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 2,
    };
    vi.mocked(providerOAuthFetchLimits).mockResolvedValueOnce(limits);
    vi.mocked(gatewayCircuitResetProvider).mockRejectedValueOnce(new Error("reset boom"));

    const client = createTestQueryClient();

    await expect(
      refreshProviderOAuthLimits(client, 11, { resetCircuitAfterRefresh: true })
    ).resolves.toEqual(limits);

    expect(gatewayCircuitResetProvider).toHaveBeenCalledWith(11);
    expect(client.getQueryData(oauthLimitsKeys.detail(11))).toEqual(limits);
  });

  it("resetProviderOAuthCodexQuota writes refreshed limits only for the target provider", async () => {
    setTauriRuntime();

    const oldTargetLimits = {
      limit_short_label: "5h",
      limit_5h_text: "0%",
      limit_weekly_text: "10%",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 1,
    };
    const otherLimits = {
      limit_short_label: "5h",
      limit_5h_text: "80%",
      limit_weekly_text: "90%",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 5,
    };
    const refreshedLimits = {
      limit_short_label: "5h",
      limit_5h_text: "100%",
      limit_weekly_text: "100%",
      limit_5h_reset_at: 1_700_000_000,
      limit_weekly_reset_at: 1_700_100_000,
      reset_credit_available_count: 0,
    };
    vi.mocked(providerOAuthResetCodexQuota).mockResolvedValueOnce({
      success: true,
      code: "ok",
      windows_reset: 2,
      refreshed_limits: refreshedLimits,
      refresh_error: null,
    });
    vi.mocked(gatewayCircuitResetProvider).mockResolvedValue(true);

    const client = createTestQueryClient();
    client.setQueryData(oauthLimitsKeys.detail(11), oldTargetLimits);
    client.setQueryData(oauthLimitsKeys.detail(22), otherLimits);

    await expect(
      resetProviderOAuthCodexQuota(client, 11, { resetCircuitAfterRefresh: true })
    ).resolves.toMatchObject({ success: true, refreshed_limits: refreshedLimits });

    expect(providerOAuthResetCodexQuota).toHaveBeenCalledWith(11);
    expect(gatewayCircuitResetProvider).toHaveBeenCalledWith(11);
    expect(client.getQueryData(oauthLimitsKeys.detail(11))).toEqual(refreshedLimits);
    expect(client.getQueryData(oauthLimitsKeys.detail(22))).toEqual(otherLimits);
  });

  it("resetProviderOAuthCodexQuota preserves cached limits on partial success", async () => {
    setTauriRuntime();
    vi.mocked(gatewayCircuitResetProvider).mockClear();

    const oldLimits = {
      limit_short_label: "5h",
      limit_5h_text: "0%",
      limit_weekly_text: "10%",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 1,
    };
    vi.mocked(providerOAuthResetCodexQuota).mockResolvedValueOnce({
      success: true,
      code: "ok",
      windows_reset: 2,
      refreshed_limits: null,
      refresh_error: "usage refresh failed",
    });

    const client = createTestQueryClient();
    client.setQueryData(oauthLimitsKeys.detail(11), oldLimits);

    await expect(resetProviderOAuthCodexQuota(client, 11)).resolves.toMatchObject({
      success: true,
      refreshed_limits: null,
      refresh_error: "usage refresh failed",
    });

    expect(client.getQueryData(oauthLimitsKeys.detail(11))).toEqual(oldLimits);
    expect(gatewayCircuitResetProvider).not.toHaveBeenCalled();
  });

  it("useProviderSetEnabledMutation updates cached providers list", async () => {
    setTauriRuntime();

    const provider: ProviderSummary = makeProvider({
      id: 1,
      cli_key: "claude",
      name: "P1",
      enabled: false,
    });
    const updated: ProviderSummary = { ...provider, enabled: true };

    vi.mocked(providerSetEnabled).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), [provider]);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderSetEnabledMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 1, enabled: true });
    });

    expect(providerSetEnabled).toHaveBeenCalledWith(1, true);
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual([updated]);
  });

  it("useProviderUpsertMutation updates the cached providers list and invalidates related queries", async () => {
    setTauriRuntime();

    const existing = makeProvider({
      id: 1,
      cli_key: "claude",
      name: "Existing",
      enabled: true,
    });
    const saved = { ...existing, name: "Updated" };

    vi.mocked(providerUpsert).mockResolvedValue(saved);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), [existing]);
    const otherProviderUuid = "22222222-2222-4222-8222-222222222222";
    client.setQueryData(providerModelsKeys.catalog(1, existing.provider_uuid), {
      providerId: 1,
      marker: "target",
    });
    client.setQueryData(providerModelsKeys.catalog(2, otherProviderUuid), {
      providerId: 2,
      marker: "other",
    });
    client.setQueryData(providerAccountUsageKeys.detail(1), {
      adapter_kind: "sub2api",
      status: "available",
      freshness: "fresh",
      plan_name: "old",
      balance: 9,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1,
      message: null,
    });
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderUpsertMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        input: {
          providerId: 1,
          cliKey: "claude",
          name: "Updated",
          baseUrls: [],
          baseUrlMode: "order",
          enabled: true,
          costMultiplier: 1,
          limit5hUsd: null,
          limitDailyUsd: null,
          dailyResetMode: "fixed",
          dailyResetTime: "00:00:00",
          limitWeeklyUsd: null,
          limitMonthlyUsd: null,
          limitTotalUsd: null,
        },
      });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toEqual([saved]);
    expect(client.getQueryData(providerAccountUsageKeys.detail(1))).toBeUndefined();
    expect(
      client.getQueryState(providerModelsKeys.catalog(1, existing.provider_uuid))?.isInvalidated
    ).toBe(true);
    expect(
      client.getQueryState(providerModelsKeys.catalog(2, otherProviderUuid))?.isInvalidated
    ).toBe(false);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: providerModelsKeys.catalog(1, existing.provider_uuid),
      exact: true,
    });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: providersKeys.list("claude") });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: gatewayKeys.circuitStatus("claude"),
    });
  });

  it("keeps provider B refresh cached while provider A is being updated", async () => {
    setTauriRuntime();

    const providerA = makeProvider({ id: 1, cli_key: "codex", name: "Provider A" });
    const providerB = makeProvider({
      id: 2,
      cli_key: "codex",
      name: "Provider B",
      provider_uuid: "22222222-2222-4222-8222-222222222222",
    });
    const savedProviderA = { ...providerA, name: "Provider A updated" };
    const pendingProviderBRefresh = deferred<ProviderModelCatalog>();
    const refreshedProviderB: ProviderModelCatalog = {
      providerId: providerB.id,
      providerUuid: providerB.provider_uuid,
      protocol: "openai_compatible",
      stale: false,
      lastAttemptAt: 200,
      lastSuccessAt: 200,
      lastErrorCode: null,
      models: [],
    };
    vi.mocked(providerUpsert).mockResolvedValueOnce(savedProviderA);
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingProviderBRefresh.promise);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("codex"), [providerA, providerB]);
    client.setQueryData(providerModelsKeys.catalog(1, providerA.provider_uuid), {
      providerId: 1,
      marker: "provider-a-before-edit",
    });
    const wrapper = createQueryWrapper(client);
    const refreshMutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });
    const upsertMutation = renderHook(() => useProviderUpsertMutation(), { wrapper });

    let refreshPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      refreshPromise = refreshMutation.result.current.mutateAsync({
        providerId: providerB.id,
        providerUuid: providerB.provider_uuid,
      });
    });
    await waitFor(() =>
      expect(providerModelsRefresh).toHaveBeenCalledWith(providerB.id, providerB.provider_uuid)
    );

    await act(async () => {
      await upsertMutation.result.current.mutateAsync({
        input: {
          providerId: providerA.id,
          cliKey: "codex",
          name: savedProviderA.name,
          baseUrls: [],
          baseUrlMode: "order",
          enabled: true,
          costMultiplier: 1,
          limit5hUsd: null,
          limitDailyUsd: null,
          dailyResetMode: "fixed",
          dailyResetTime: "00:00:00",
          limitWeeklyUsd: null,
          limitMonthlyUsd: null,
          limitTotalUsd: null,
        },
      });
    });

    pendingProviderBRefresh.resolve(refreshedProviderB);
    await act(async () => {
      await refreshPromise;
    });

    expect(
      client.getQueryData(providerModelsKeys.catalog(providerB.id, providerB.provider_uuid))
    ).toEqual(refreshedProviderB);
    expect(
      client.getQueryState(providerModelsKeys.catalog(providerA.id, providerA.provider_uuid))
        ?.isInvalidated
    ).toBe(true);
  });

  it("keeps the same provider refresh result when only display fields change", async () => {
    setTauriRuntime();
    vi.clearAllMocks();

    const provider = makeProvider({
      id: 31,
      cli_key: "codex",
      name: "Provider before",
      base_urls: ["https://provider.example/v1"],
      api_key_configured: true,
    });
    const saved = { ...provider, name: "Provider after" };
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerUpsert).mockResolvedValueOnce(saved);
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("codex"), [provider]);
    const wrapper = createQueryWrapper(client);
    const refreshMutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });
    const upsertMutation = renderHook(() => useProviderUpsertMutation(), { wrapper });

    let refreshPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      refreshPromise = refreshMutation.result.current.mutateAsync({
        providerId: provider.id,
        providerUuid: provider.provider_uuid,
      });
    });
    await waitFor(() =>
      expect(providerModelsRefresh).toHaveBeenCalledWith(provider.id, provider.provider_uuid)
    );

    await act(async () => {
      await upsertMutation.result.current.mutateAsync({
        input: {
          providerId: provider.id,
          cliKey: "codex",
          name: saved.name,
          baseUrls: provider.base_urls,
          baseUrlMode: provider.base_url_mode,
          authMode: provider.auth_mode,
          apiKey: null,
          enabled: provider.enabled,
          costMultiplier: provider.cost_multiplier,
          limit5hUsd: null,
          limitDailyUsd: null,
          dailyResetMode: "fixed",
          dailyResetTime: "00:00:00",
          limitWeeklyUsd: null,
          limitMonthlyUsd: null,
          limitTotalUsd: null,
        },
      });
    });

    const refreshed: ProviderModelCatalog = {
      providerId: provider.id,
      providerUuid: provider.provider_uuid,
      protocol: "openai_compatible",
      stale: false,
      lastAttemptAt: 300,
      lastSuccessAt: 300,
      lastErrorCode: null,
      models: [],
    };
    pendingRefresh.resolve(refreshed);
    await act(async () => {
      await refreshPromise;
    });

    expect(
      client.getQueryData(providerModelsKeys.catalog(provider.id, provider.provider_uuid))
    ).toEqual(refreshed);
  });

  it("drops the same provider refresh result when connection fields change", async () => {
    setTauriRuntime();
    vi.clearAllMocks();

    const provider = makeProvider({
      id: 32,
      cli_key: "codex",
      name: "Provider",
      base_urls: ["https://old.example/v1"],
      api_key_configured: true,
    });
    const saved = { ...provider, base_urls: ["https://new.example/v1"] };
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerUpsert).mockResolvedValueOnce(saved);
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);

    const client = createTestQueryClient();
    const key = providerModelsKeys.catalog(provider.id, provider.provider_uuid);
    client.setQueryData(providersKeys.list("codex"), [provider]);
    const wrapper = createQueryWrapper(client);
    const refreshMutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });
    const upsertMutation = renderHook(() => useProviderUpsertMutation(), { wrapper });

    let refreshPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      refreshPromise = refreshMutation.result.current.mutateAsync({
        providerId: provider.id,
        providerUuid: provider.provider_uuid,
      });
    });
    await waitFor(() =>
      expect(providerModelsRefresh).toHaveBeenCalledWith(provider.id, provider.provider_uuid)
    );

    await act(async () => {
      await upsertMutation.result.current.mutateAsync({
        input: {
          providerId: provider.id,
          cliKey: "codex",
          name: provider.name,
          baseUrls: saved.base_urls,
          baseUrlMode: provider.base_url_mode,
          authMode: provider.auth_mode,
          apiKey: null,
          enabled: provider.enabled,
          costMultiplier: provider.cost_multiplier,
          limit5hUsd: null,
          limitDailyUsd: null,
          dailyResetMode: "fixed",
          dailyResetTime: "00:00:00",
          limitWeeklyUsd: null,
          limitMonthlyUsd: null,
          limitTotalUsd: null,
        },
      });
    });

    const postEditCatalog = {
      providerId: provider.id,
      providerUuid: provider.provider_uuid,
      protocol: "openai_compatible" as const,
      stale: true,
      lastAttemptAt: null,
      lastSuccessAt: null,
      lastErrorCode: null,
      models: [],
    };
    client.setQueryData(key, postEditCatalog);
    pendingRefresh.resolve({ ...postEditCatalog, stale: false, lastSuccessAt: 200 });
    await act(async () => {
      await refreshPromise;
    });

    expect(client.getQueryData(key)).toEqual(postEditCatalog);
  });

  it("useProviderSetEnabledMutation is a no-op when service returns null", async () => {
    setTauriRuntime();

    const provider: ProviderSummary = makeProvider({
      id: 1,
      cli_key: "claude",
      name: "P1",
      enabled: false,
    });

    vi.mocked(providerSetEnabled).mockResolvedValue(null);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), [provider]);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderSetEnabledMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 1, enabled: true });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toEqual([provider]);
  });

  it("useProviderSetEnabledMutation does not update when list cache is missing", async () => {
    setTauriRuntime();

    const provider: ProviderSummary = makeProvider({
      id: 1,
      cli_key: "claude",
      name: "P1",
      enabled: true,
    });

    vi.mocked(providerSetEnabled).mockResolvedValue(provider);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), null);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderSetEnabledMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 1, enabled: true });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toBeNull();
  });

  it("useProviderDeleteMutation removes provider from cached list", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [
      makeProvider({ id: 1, cli_key: "claude", name: "P1" }),
      makeProvider({ id: 2, cli_key: "claude", name: "P2" }),
    ];

    vi.mocked(providerDelete).mockResolvedValue(true);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), providers);
    client.setQueryData(providerModelsKeys.catalog(1, providers[0].provider_uuid), {
      providerId: 1,
      marker: "target",
    });
    client.setQueryData(providerModelsKeys.catalog(2, providers[1].provider_uuid), {
      providerId: 2,
      marker: "other",
    });
    client.setQueryData(providerAccountUsageKeys.detail(1), {
      adapter_kind: "sub2api",
      status: "available",
      freshness: "fresh",
      plan_name: null,
      balance: 1,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1,
      message: null,
    });
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDeleteMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ cliKey: " claude " as never, providerId: 1 });
    });

    expect(providerDelete).toHaveBeenCalledWith(1, { clearUsageStats: false });
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual([providers[1]]);
    expect(client.getQueryData(providerAccountUsageKeys.detail(1))).toBeUndefined();
    expect(
      client.getQueryData(providerModelsKeys.catalog(1, providers[0].provider_uuid))
    ).toBeUndefined();
    expect(client.getQueryData(providerModelsKeys.catalog(2, providers[1].provider_uuid))).toEqual({
      providerId: 2,
      marker: "other",
    });
    expect(client.getQueryData(providersKeys.list(" claude " as never))).toBeUndefined();
  });

  it("retires in-flight model mutations for a deleted provider without affecting another provider", async () => {
    setTauriRuntime();

    const providerA = makeProvider({ id: 1, cli_key: "codex", name: "P1" });
    const providerB = makeProvider({
      id: 2,
      cli_key: "codex",
      name: "P2",
      provider_uuid: "22222222-2222-4222-8222-222222222222",
    });
    const pendingA = deferred<ProviderModelCatalog>();
    const pendingB = deferred<ProviderModelCatalog>();
    const catalogA: ProviderModelCatalog = {
      providerId: providerA.id,
      providerUuid: providerA.provider_uuid,
      protocol: "openai_compatible",
      stale: false,
      lastAttemptAt: 1,
      lastSuccessAt: 1,
      lastErrorCode: null,
      models: [],
    };
    const catalogB: ProviderModelCatalog = {
      ...catalogA,
      providerId: providerB.id,
      providerUuid: providerB.provider_uuid,
    };
    vi.mocked(providerModelsRefresh)
      .mockReturnValueOnce(pendingA.promise)
      .mockReturnValueOnce(pendingB.promise);
    vi.mocked(providerDelete).mockResolvedValueOnce(true);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("codex"), [providerA, providerB]);
    const wrapper = createQueryWrapper(client);
    const refreshA = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });
    const refreshB = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });
    const deleteProvider = renderHook(() => useProviderDeleteMutation(), { wrapper });

    let refreshAPromise!: Promise<ProviderModelCatalog>;
    let refreshBPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      refreshAPromise = refreshA.result.current.mutateAsync({
        providerId: providerA.id,
        providerUuid: providerA.provider_uuid,
      });
      refreshBPromise = refreshB.result.current.mutateAsync({
        providerId: providerB.id,
        providerUuid: providerB.provider_uuid,
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledTimes(2));

    await act(async () => {
      await deleteProvider.result.current.mutateAsync({
        cliKey: "codex",
        providerId: providerA.id,
      });
    });
    pendingA.resolve(catalogA);
    pendingB.resolve(catalogB);
    await act(async () => {
      await Promise.all([refreshAPromise, refreshBPromise]);
    });

    expect(
      client.getQueryState(providerModelsKeys.catalog(providerA.id, providerA.provider_uuid))
    ).toBeUndefined();
    expect(
      client.getQueryData(providerModelsKeys.catalog(providerB.id, providerB.provider_uuid))
    ).toEqual(catalogB);
  });

  it("useProviderDeleteMutation forwards usage stats cleanup choice", async () => {
    setTauriRuntime();

    vi.mocked(providerDelete).mockResolvedValue(true);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDeleteMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        cliKey: "claude",
        providerId: 1,
        clearUsageStats: true,
      });
    });

    expect(providerDelete).toHaveBeenCalledWith(1, { clearUsageStats: true });
  });

  it("useProviderDeleteMutation is a no-op when service returns false", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [makeProvider({ id: 1, cli_key: "claude", name: "P1" })];

    vi.mocked(providerDelete).mockResolvedValue(false);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), providers);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDeleteMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ cliKey: "claude", providerId: 1 });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toEqual(providers);
  });

  it("useProviderDeleteMutation does not update when list cache is missing", async () => {
    setTauriRuntime();

    vi.mocked(providerDelete).mockResolvedValue(true);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), null);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDeleteMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ cliKey: "claude", providerId: 1 });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toBeNull();
  });

  it("useProvidersReorderMutation sets cached list when service returns next order", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [
      makeProvider({ id: 1, cli_key: "claude", name: "P1" }),
      makeProvider({ id: 2, cli_key: "claude", name: "P2" }),
    ];
    const next: ProviderSummary[] = [providers[1], providers[0]];

    vi.mocked(providersReorder).mockResolvedValue(next);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), providers);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProvidersReorderMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ cliKey: " claude " as never, orderedProviderIds: [2, 1] });
    });

    expect(providersReorder).toHaveBeenCalledWith("claude", [2, 1]);
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual(next);
    expect(client.getQueryData(providersKeys.list(" claude " as never))).toBeUndefined();
  });

  it("useProvidersReorderMutation is a no-op when service returns null", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [makeProvider({ id: 1, cli_key: "claude", name: "P1" })];

    vi.mocked(providersReorder).mockResolvedValue(null);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), providers);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProvidersReorderMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ cliKey: "claude", orderedProviderIds: [1] });
    });

    expect(client.getQueryData(providersKeys.list("claude"))).toEqual(providers);
  });

  it("useProviderDuplicateMutation inserts duplicate after source and persists order", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [
      makeProvider({ id: 1, cli_key: "claude", name: "P1" }),
      makeProvider({ id: 2, cli_key: "claude", name: "P2" }),
    ];
    const duplicated = makeProvider({ id: 3, cli_key: "claude", name: "P1 副本" });
    const reordered = [providers[0], duplicated, providers[1]];

    vi.mocked(providerDuplicate).mockClear();
    vi.mocked(providersReorder).mockClear();
    vi.mocked(providerDuplicate).mockResolvedValue(duplicated);
    vi.mocked(providersReorder).mockResolvedValue(reordered);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), providers);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDuplicateMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 1 });
    });

    expect(providerDuplicate).toHaveBeenCalledWith(1);
    expect(providersReorder).toHaveBeenCalledWith("claude", [1, 3, 2]);
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual(reordered);
  });

  it("useProviderDuplicateMutation repositions duplicate already present in cache", async () => {
    setTauriRuntime();

    const source = makeProvider({ id: 1, cli_key: "claude", name: "P1" });
    const other = makeProvider({ id: 2, cli_key: "claude", name: "P2" });
    const duplicated = makeProvider({ id: 3, cli_key: "claude", name: "P1 副本" });
    const reordered = [source, duplicated, other];

    vi.mocked(providerDuplicate).mockClear();
    vi.mocked(providersReorder).mockClear();
    vi.mocked(providerDuplicate).mockResolvedValue(duplicated);
    vi.mocked(providersReorder).mockResolvedValue(reordered);

    const client = createTestQueryClient();
    client.setQueryData(providersKeys.list("claude"), [source, other, duplicated]);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDuplicateMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 1 });
    });

    expect(providersReorder).toHaveBeenCalledWith("claude", [1, 3, 2]);
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual(reordered);
  });

  it("useProviderDuplicateMutation propagates reorder failures after invalidating list", async () => {
    setTauriRuntime();

    const providers: ProviderSummary[] = [
      makeProvider({ id: 1, cli_key: "claude", name: "P1" }),
      makeProvider({ id: 2, cli_key: "claude", name: "P2" }),
    ];
    const duplicated = makeProvider({ id: 3, cli_key: "claude", name: "P1 副本" });

    vi.mocked(providerDuplicate).mockClear();
    vi.mocked(providersReorder).mockClear();
    vi.mocked(providerDuplicate).mockResolvedValue(duplicated);
    vi.mocked(providersReorder).mockRejectedValue(new Error("reorder failed"));

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    client.setQueryData(providersKeys.list("claude"), providers);
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderDuplicateMutation(), { wrapper });
    await expect(result.current.mutateAsync({ providerId: 1 })).rejects.toThrow("reorder failed");

    expect(providersReorder).toHaveBeenCalledWith("claude", [1, 3, 2]);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: providersKeys.list("claude") });
  });

  it("useProviderClaudeTerminalLaunchCommandMutation calls service with provider id", async () => {
    setTauriRuntime();

    vi.mocked(providerClaudeTerminalLaunchCommand).mockResolvedValue("bash '/tmp/aio.sh'");

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderClaudeTerminalLaunchCommandMutation(), {
      wrapper,
    });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 8 });
    });

    expect(providerClaudeTerminalLaunchCommand).toHaveBeenCalledWith(8);
  });

  it("useProviderTestAvailabilityMutation calls service with provider id", async () => {
    setTauriRuntime();

    vi.mocked(providerTestAvailability).mockResolvedValue({
      ok: true,
      provider_id: 8,
      provider_name: "P1",
      base_url: "https://api.example.com",
      status: 200,
      latency_ms: 42,
      error: null,
      response_preview: null,
    });

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useProviderTestAvailabilityMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ providerId: 8 });
    });

    expect(providerTestAvailability).toHaveBeenCalledWith(8);
  });
});
