import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { GatewayStatus } from "../../services/gateway/gateway";
import {
  settingsGet,
  settingsPatch,
  settingsSet,
  type AppSettings,
  type SettingsMutationResult,
} from "../../services/settings/settings";
import { settingsCircuitBreakerNoticeSet } from "../../services/settings/settingsCircuitBreakerNotice";
import { settingsCodexSessionIdCompletionSet } from "../../services/settings/settingsCodexSessionIdCompletion";
import { settingsGatewayRectifierSet } from "../../services/settings/settingsGatewayRectifier";
import { createTestAppSettings } from "../../test/fixtures/settings";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import { cliManagerKeys, cliProxyKeys, settingsKeys } from "../keys";
import {
  getSettingsReadProtection,
  SETTINGS_READONLY_MESSAGE,
  useSettingsCircuitBreakerNoticeSetMutation,
  useSettingsCodexSessionIdCompletionSetMutation,
  useSettingsGatewayRectifierSetMutation,
  useSettingsQuery,
  useSettingsPatchMutation,
  useSettingsSetMutation,
} from "../settings";

vi.mock("../../services/settings/settings", async () => {
  const actual = await vi.importActual<typeof import("../../services/settings/settings")>(
    "../../services/settings/settings"
  );
  return { ...actual, settingsGet: vi.fn(), settingsPatch: vi.fn(), settingsSet: vi.fn() };
});
vi.mock("../../services/settings/settingsGatewayRectifier", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/settings/settingsGatewayRectifier")
  >("../../services/settings/settingsGatewayRectifier");
  return { ...actual, settingsGatewayRectifierSet: vi.fn() };
});
vi.mock("../../services/settings/settingsCircuitBreakerNotice", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/settings/settingsCircuitBreakerNotice")
  >("../../services/settings/settingsCircuitBreakerNotice");
  return { ...actual, settingsCircuitBreakerNoticeSet: vi.fn() };
});
vi.mock("../../services/settings/settingsCodexSessionIdCompletion", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/settings/settingsCodexSessionIdCompletion")
  >("../../services/settings/settingsCodexSessionIdCompletion");
  return { ...actual, settingsCodexSessionIdCompletionSet: vi.fn() };
});

function createSettingsMutationResult(settings: AppSettings): SettingsMutationResult {
  return {
    settings,
    runtime: {
      gateway_rebound: false,
      cli_proxy_synced: false,
      wsl_auto_sync_triggered: false,
      gateway_status: {
        running: false,
        port: settings.preferred_port,
        base_url: `http://127.0.0.1:${settings.preferred_port}`,
        listen_addr: `127.0.0.1:${settings.preferred_port}`,
      },
    },
  };
}

describe("query/settings", () => {
  it("getSettingsReadProtection blocks writes when there is no data and query errored", () => {
    expect(
      getSettingsReadProtection({
        data: null,
        isError: true,
      })
    ).toEqual({
      settingsReadErrorMessage: SETTINGS_READONLY_MESSAGE,
      settingsWriteBlocked: true,
    });
  });

  it("getSettingsReadProtection blocks writes when refetch falls back to stale data", () => {
    expect(
      getSettingsReadProtection({
        data: createTestAppSettings(),
        isError: true,
      })
    ).toEqual({
      settingsReadErrorMessage: SETTINGS_READONLY_MESSAGE,
      settingsWriteBlocked: true,
    });
  });

  it("useSettingsQuery respects enabled=false", () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockClear();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useSettingsQuery({ enabled: false }), { wrapper });

    expect(settingsGet).not.toHaveBeenCalled();
  });

  it("calls settingsGet with tauri runtime", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockResolvedValue(createTestAppSettings());

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useSettingsQuery(), { wrapper });

    await waitFor(() => {
      expect(settingsGet).toHaveBeenCalled();
    });
  });

  it("useSettingsQuery enters error state when settingsGet rejects", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockRejectedValue(new Error("settings query boom"));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsQuery(), { wrapper });
    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("useSettingsSetMutation updates cache and invalidates on settle", async () => {
    setTauriRuntime();

    const updated = createTestAppSettings({ preferred_port: 40000 });
    const gatewayStatus: GatewayStatus = {
      running: false,
      port: 40000,
      base_url: "http://127.0.0.1:40000",
      listen_addr: "127.0.0.1:40000",
    };
    vi.mocked(settingsSet).mockResolvedValue({
      settings: updated,
      runtime: {
        gateway_rebound: false,
        cli_proxy_synced: false,
        wsl_auto_sync_triggered: false,
        gateway_status: gatewayStatus,
      },
    });

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), createTestAppSettings());
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        preferredPort: 40000,
        autoStart: false,
        trayEnabled: true,
        logRetentionDays: 30,
        providerCooldownSeconds: 30,
        providerBaseUrlPingCacheTtlSeconds: 60,
        upstreamFirstByteTimeoutSeconds: 0,
        upstreamStreamIdleTimeoutSeconds: 0,
        upstreamRequestTimeoutNonStreamingSeconds: 0,
        enableCacheAnomalyMonitor: false,
        failoverMaxAttemptsPerProvider: 5,
        failoverMaxProvidersToTry: 5,
        circuitBreakerFailureThreshold: 5,
        circuitBreakerOpenDurationMinutes: 30,
      });
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.get() });
  });

  it("useSettingsSetMutation keeps cache when service returns null", async () => {
    setTauriRuntime();

    const initial = createTestAppSettings();
    vi.mocked(settingsSet).mockResolvedValue(null as any);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), initial);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        preferredPort: 40000,
        autoStart: false,
        trayEnabled: true,
        logRetentionDays: 30,
        providerCooldownSeconds: 30,
        providerBaseUrlPingCacheTtlSeconds: 60,
        upstreamFirstByteTimeoutSeconds: 0,
        upstreamStreamIdleTimeoutSeconds: 0,
        upstreamRequestTimeoutNonStreamingSeconds: 0,
        enableCacheAnomalyMonitor: false,
        failoverMaxAttemptsPerProvider: 5,
        failoverMaxProvidersToTry: 5,
        circuitBreakerFailureThreshold: 5,
        circuitBreakerOpenDurationMinutes: 30,
      });
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(initial);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.get() });
  });

  it("useSettingsPatchMutation invalidates Codex projection queries for OAuth mode", async () => {
    setTauriRuntime();

    const current = createTestAppSettings();
    const updated = createTestAppSettings({ codex_oauth_compatible_proxy_mode: true });
    vi.mocked(settingsGet).mockResolvedValue(current);
    vi.mocked(settingsPatch).mockResolvedValue({
      settings: updated,
      runtime: {
        gateway_rebound: false,
        cli_proxy_synced: true,
        wsl_auto_sync_triggered: false,
        gateway_status: {
          running: false,
          port: null,
          base_url: null,
          listen_addr: null,
        },
      },
    });

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), current);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsPatchMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ codex_oauth_compatible_proxy_mode: true });
    });

    expect(settingsPatch).toHaveBeenCalledWith(current, {
      codex_oauth_compatible_proxy_mode: true,
    });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfig() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfigToml() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliProxyKeys.statusAll() });
  });

  it("serializes independent settings patches and rebuilds from authoritative settings", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockReset();
    vi.mocked(settingsPatch).mockReset();

    const initial = createTestAppSettings({
      provider_cooldown_seconds: 30,
      wsl_auto_config: false,
    });
    const firstUpdated = createTestAppSettings({
      provider_cooldown_seconds: 41,
      wsl_auto_config: false,
    });
    const secondUpdated = createTestAppSettings({
      provider_cooldown_seconds: 41,
      wsl_auto_config: true,
    });
    let resolveFirst!: (value: SettingsMutationResult) => void;
    const firstResponse = new Promise<SettingsMutationResult>((resolve) => {
      resolveFirst = resolve;
    });

    vi.mocked(settingsGet).mockResolvedValueOnce(initial).mockResolvedValueOnce(firstUpdated);
    vi.mocked(settingsPatch)
      .mockReturnValueOnce(firstResponse)
      .mockResolvedValueOnce(createSettingsMutationResult(secondUpdated));

    const client = createTestQueryClient();
    client.setQueryData(
      settingsKeys.get(),
      createTestAppSettings({ provider_cooldown_seconds: 12, wsl_auto_config: false })
    );
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(
      () => ({
        settingsPage: useSettingsPatchMutation(),
        cliManager: useSettingsPatchMutation(),
      }),
      { wrapper }
    );

    let firstMutation!: Promise<unknown>;
    let secondMutation!: Promise<unknown>;
    act(() => {
      firstMutation = result.current.settingsPage.mutateAsync({ provider_cooldown_seconds: 41 });
      secondMutation = result.current.cliManager.mutateAsync({ wsl_auto_config: true });
    });

    await waitFor(() => expect(settingsPatch).toHaveBeenCalledTimes(1));
    expect(settingsGet).toHaveBeenCalledTimes(1);
    expect(settingsPatch).toHaveBeenNthCalledWith(1, initial, {
      provider_cooldown_seconds: 41,
    });

    act(() => resolveFirst(createSettingsMutationResult(firstUpdated)));
    await act(async () => {
      await Promise.all([firstMutation, secondMutation]);
    });

    expect(settingsGet).toHaveBeenCalledTimes(2);
    expect(settingsPatch).toHaveBeenCalledTimes(2);
    expect(settingsPatch).toHaveBeenNthCalledWith(2, firstUpdated, {
      wsl_auto_config: true,
    });
    expect(client.getQueryData(settingsKeys.get())).toEqual(secondUpdated);
  });

  it("keeps same-field settings patches in FIFO order", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockReset();
    vi.mocked(settingsPatch).mockReset();

    const initial = createTestAppSettings({ provider_cooldown_seconds: 30 });
    const firstUpdated = createTestAppSettings({ provider_cooldown_seconds: 41 });
    const secondUpdated = createTestAppSettings({ provider_cooldown_seconds: 42 });
    let resolveFirst!: (value: SettingsMutationResult) => void;
    const firstResponse = new Promise<SettingsMutationResult>((resolve) => {
      resolveFirst = resolve;
    });

    vi.mocked(settingsGet).mockResolvedValueOnce(initial).mockResolvedValueOnce(firstUpdated);
    vi.mocked(settingsPatch)
      .mockReturnValueOnce(firstResponse)
      .mockResolvedValueOnce(createSettingsMutationResult(secondUpdated));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(
      () => ({ first: useSettingsPatchMutation(), second: useSettingsPatchMutation() }),
      { wrapper }
    );

    let firstMutation!: Promise<unknown>;
    let secondMutation!: Promise<unknown>;
    act(() => {
      firstMutation = result.current.first.mutateAsync({ provider_cooldown_seconds: 41 });
      secondMutation = result.current.second.mutateAsync({ provider_cooldown_seconds: 42 });
    });

    await waitFor(() => expect(settingsPatch).toHaveBeenCalledTimes(1));
    expect(settingsPatch).toHaveBeenNthCalledWith(1, initial, {
      provider_cooldown_seconds: 41,
    });

    act(() => resolveFirst(createSettingsMutationResult(firstUpdated)));
    await act(async () => {
      await Promise.all([firstMutation, secondMutation]);
    });

    expect(settingsPatch).toHaveBeenNthCalledWith(2, firstUpdated, {
      provider_cooldown_seconds: 42,
    });
    expect(client.getQueryData(settingsKeys.get())).toEqual(secondUpdated);
  });

  it("shares FIFO ordering between full set and changed-key patch mutations", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockReset();
    vi.mocked(settingsPatch).mockReset();
    vi.mocked(settingsSet).mockReset();

    const firstUpdated = createTestAppSettings({ provider_cooldown_seconds: 41 });
    const secondUpdated = createTestAppSettings({
      provider_cooldown_seconds: 41,
      wsl_auto_config: true,
    });
    let resolveFirst!: (value: SettingsMutationResult) => void;
    const firstResponse = new Promise<SettingsMutationResult>((resolve) => {
      resolveFirst = resolve;
    });

    vi.mocked(settingsGet).mockResolvedValue(firstUpdated);
    vi.mocked(settingsSet).mockReturnValueOnce(firstResponse);
    vi.mocked(settingsPatch).mockResolvedValueOnce(createSettingsMutationResult(secondUpdated));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(
      () => ({ full: useSettingsSetMutation(), patch: useSettingsPatchMutation() }),
      { wrapper }
    );

    let fullMutation!: Promise<unknown>;
    let patchMutation!: Promise<unknown>;
    act(() => {
      fullMutation = result.current.full.mutateAsync({
        preferredPort: firstUpdated.preferred_port,
        logRetentionDays: firstUpdated.log_retention_days,
        failoverMaxAttemptsPerProvider: firstUpdated.failover_max_attempts_per_provider,
        failoverMaxProvidersToTry: firstUpdated.failover_max_providers_to_try,
        providerCooldownSeconds: firstUpdated.provider_cooldown_seconds,
      });
      patchMutation = result.current.patch.mutateAsync({ wsl_auto_config: true });
    });

    await waitFor(() => expect(settingsSet).toHaveBeenCalledTimes(1));
    expect(settingsGet).not.toHaveBeenCalled();
    expect(settingsPatch).not.toHaveBeenCalled();

    act(() => resolveFirst(createSettingsMutationResult(firstUpdated)));
    await act(async () => {
      await Promise.all([fullMutation, patchMutation]);
    });

    expect(settingsGet).toHaveBeenCalledTimes(1);
    expect(settingsSet).toHaveBeenCalledTimes(1);
    expect(settingsPatch).toHaveBeenCalledWith(firstUpdated, { wsl_auto_config: true });
    expect(client.getQueryData(settingsKeys.get())).toEqual(secondUpdated);
  });

  it("continues queued settings patches after an earlier write fails", async () => {
    setTauriRuntime();
    vi.mocked(settingsGet).mockReset();
    vi.mocked(settingsPatch).mockReset();

    const initial = createTestAppSettings({
      provider_cooldown_seconds: 30,
      wsl_auto_config: false,
    });
    const secondUpdated = createTestAppSettings({
      provider_cooldown_seconds: 30,
      wsl_auto_config: true,
    });
    let rejectFirst!: (reason: Error) => void;
    const firstResponse = new Promise<never>((_resolve, reject) => {
      rejectFirst = reject;
    });

    vi.mocked(settingsGet).mockResolvedValue(initial);
    vi.mocked(settingsPatch)
      .mockReturnValueOnce(firstResponse)
      .mockResolvedValueOnce(createSettingsMutationResult(secondUpdated));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(
      () => ({ first: useSettingsPatchMutation(), second: useSettingsPatchMutation() }),
      { wrapper }
    );

    let firstMutation!: Promise<unknown>;
    let secondMutation!: Promise<unknown>;
    act(() => {
      firstMutation = result.current.first.mutateAsync({ provider_cooldown_seconds: 41 });
      void firstMutation.catch(() => undefined);
      secondMutation = result.current.second.mutateAsync({ wsl_auto_config: true });
    });

    await waitFor(() => expect(settingsPatch).toHaveBeenCalledTimes(1));
    act(() => rejectFirst(new Error("settings write failed")));
    await act(async () => {
      await expect(firstMutation).rejects.toThrow("settings write failed");
      await secondMutation;
    });

    expect(settingsPatch).toHaveBeenCalledTimes(2);
    expect(settingsPatch).toHaveBeenNthCalledWith(2, initial, { wsl_auto_config: true });
    expect(client.getQueryData(settingsKeys.get())).toEqual(secondUpdated);
  });

  it("useSettingsGatewayRectifierSetMutation updates cache", async () => {
    setTauriRuntime();

    const updated = createTestAppSettings({ enable_response_fixer: false });
    vi.mocked(settingsGatewayRectifierSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), createTestAppSettings());
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsGatewayRectifierSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ enable_response_fixer: false } as any);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(updated);
  });

  it("useSettingsGatewayRectifierSetMutation keeps cache when service returns null", async () => {
    setTauriRuntime();

    const initial = createTestAppSettings();
    vi.mocked(settingsGatewayRectifierSet).mockResolvedValue(null as any);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), initial);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsGatewayRectifierSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ enable_response_fixer: false } as any);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(initial);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.get() });
  });

  it("useSettingsCircuitBreakerNoticeSetMutation updates cache", async () => {
    setTauriRuntime();

    const updated = createTestAppSettings({ enable_circuit_breaker_notice: true });
    vi.mocked(settingsCircuitBreakerNoticeSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), createTestAppSettings());
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsCircuitBreakerNoticeSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync(true);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(updated);
  });

  it("useSettingsCircuitBreakerNoticeSetMutation keeps cache when service returns null", async () => {
    setTauriRuntime();

    const initial = createTestAppSettings();
    vi.mocked(settingsCircuitBreakerNoticeSet).mockResolvedValue(null as any);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), initial);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsCircuitBreakerNoticeSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync(true);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(initial);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.get() });
  });

  it("useSettingsCodexSessionIdCompletionSetMutation updates cache", async () => {
    setTauriRuntime();

    const updated = createTestAppSettings({ enable_codex_session_id_completion: false });
    vi.mocked(settingsCodexSessionIdCompletionSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), createTestAppSettings());
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsCodexSessionIdCompletionSetMutation(), {
      wrapper,
    });
    await act(async () => {
      await result.current.mutateAsync(false);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(updated);
  });

  it("useSettingsCodexSessionIdCompletionSetMutation keeps cache when service returns null", async () => {
    setTauriRuntime();

    const initial = createTestAppSettings();
    vi.mocked(settingsCodexSessionIdCompletionSet).mockResolvedValue(null as any);

    const client = createTestQueryClient();
    client.setQueryData(settingsKeys.get(), initial);
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useSettingsCodexSessionIdCompletionSetMutation(), {
      wrapper,
    });
    await act(async () => {
      await result.current.mutateAsync(false);
    });

    expect(client.getQueryData(settingsKeys.get())).toEqual(initial);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.get() });
  });
});
