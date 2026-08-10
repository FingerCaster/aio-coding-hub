import { beforeEach, describe, expect, it, vi } from "vitest";
import { tauriInvoke } from "../../../test/mocks/tauri";
import { createTestAppSettings } from "../../../test/fixtures/settings";
import { setTauriRuntime } from "../../../test/utils/tauriRuntime";

describe("services/settings/settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("passes update as a single named parameter", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { settingsSet } = await import("../settings");

    await settingsSet({
      preferredPort: 37123,
      showHomeHeatmap: false,
      showHomeUsage: true,
      homeUsagePeriod: "last7",
      cliPriorityOrder: ["codex", "claude", "gemini"],
      autoStart: false,
      trayEnabled: true,
      logRetentionDays: 30,
      providerCooldownSeconds: 30,
      providerFailbackStrategy: "natural",
      naturalProbeMaxWaitSeconds: 300,
      providerBaseUrlPingCacheTtlSeconds: 60,
      upstreamFirstByteTimeoutSeconds: 0,
      upstreamStreamIdleTimeoutSeconds: 0,
      upstreamRequestTimeoutNonStreamingSeconds: 0,
      failoverMaxAttemptsPerProvider: 5,
      failoverMaxProvidersToTry: 5,
      circuitBreakerFailureThreshold: 5,
      circuitBreakerOpenDurationMinutes: 30,
    });

    expect(tauriInvoke).toHaveBeenCalledWith(
      "settings_set",
      expect.objectContaining({
        update: expect.objectContaining({
          preferredPort: 37123,
          showHomeHeatmap: false,
          showHomeUsage: true,
          homeUsagePeriod: "last7",
          cliPriorityOrder: ["codex", "claude", "gemini"],
          autoStart: false,
        }),
      })
    );

    vi.mocked(tauriInvoke).mockClear();

    await settingsSet({
      preferredPort: 37123,
      gatewayListenMode: "custom",
      gatewayCustomListenAddress: "0.0.0.0:37123",
      autoStart: false,
      trayEnabled: true,
      logRetentionDays: 30,
      providerCooldownSeconds: 30,
      providerFailbackStrategy: "aggressive",
      naturalProbeMaxWaitSeconds: 600,
      providerBaseUrlPingCacheTtlSeconds: 60,
      upstreamFirstByteTimeoutSeconds: 0,
      upstreamStreamIdleTimeoutSeconds: 0,
      upstreamRequestTimeoutNonStreamingSeconds: 0,
      cliPriorityOrder: ["gemini", "claude", "codex"],
      enableCacheAnomalyMonitor: true,
      updateReleasesUrl: "https://example.invalid/releases.json",
      failoverMaxAttemptsPerProvider: 5,
      failoverMaxProvidersToTry: 5,
      circuitBreakerFailureThreshold: 5,
      circuitBreakerOpenDurationMinutes: 30,
      wslAutoConfig: true,
      wslTargetCli: { claude: true, codex: false, gemini: true },
      codexHomeOverride: "D:\\CodexHome",
    } as any);

    expect(tauriInvoke).toHaveBeenCalledWith(
      "settings_set",
      expect.objectContaining({
        update: expect.objectContaining({
          gatewayListenMode: "custom",
          gatewayCustomListenAddress: "0.0.0.0:37123",
          enableCacheAnomalyMonitor: true,
          providerFailbackStrategy: "aggressive",
          naturalProbeMaxWaitSeconds: 600,
          cliPriorityOrder: ["gemini", "claude", "codex"],
          updateReleasesUrl: "https://example.invalid/releases.json",
          wslAutoConfig: true,
          wslTargetCli: { claude: true, codex: false, gemini: true },
          codexHomeOverride: "D:\\CodexHome",
        }),
      })
    );
  });

  it("maps cached settings back into the generated update contract", async () => {
    const { createSettingsSetInput } = await import("../settings");

    const input = createSettingsSetInput(createTestAppSettings(), {
      codex_oauth_compatible_proxy_mode: true,
      upstream_error_response_rules: [
        {
          id: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51",
          name: "限额响应",
          description: "",
          enabled: true,
          priority: 10,
          status_codes: [429],
          keywords: [],
          match_mode: "any",
          cli_keys: ["codex"],
          provider_ids: [],
          status_behavior: { mode: "passthrough" },
          message_behavior: { mode: "passthrough" },
        },
      ],
      upstream_proxy_password: { mode: "clear" },
    });

    expect(input).toMatchObject({
      preferredPort: 37123,
      gatewayListenMode: "localhost",
      wslTargetCli: { claude: true, codex: true, gemini: true },
      codexOauthCompatibleProxyMode: true,
      providerFailbackStrategy: "natural",
      naturalProbeMaxWaitSeconds: 300,
      cx2CcFallbackModelMain: "gpt-5.4",
      upstreamProxyPassword: { mode: "clear" },
      upstreamErrorResponseRules: [expect.objectContaining({ name: "限额响应" })],
    });
    expect(input).not.toHaveProperty("autoStart");
    expect(Object.keys(input).filter((key) => key.includes("ReasoningGuard"))).toEqual([]);
    expect(input).not.toHaveProperty("cx2ccFallbackModelMain");
    expect(input).not.toHaveProperty("codex_oauth_compatible_proxy_mode");
    expect(input).not.toHaveProperty("updateChannel");
  });

  it("uses the dedicated generated command for update-channel ownership", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue(createTestAppSettings() as any);

    const { settingsUpdateChannelSet } = await import("../settings");

    await settingsUpdateChannelSet("beta");
    expect(tauriInvoke).toHaveBeenLastCalledWith("settings_update_channel_set", {
      channel: "beta",
      confirm: {
        confirm: {
          action: "settings_update_channel_set",
          resource: "update_channel:beta",
          nonce: expect.any(String),
          issuedAtMs: expect.any(Number),
          ttlMs: 60_000,
        },
      },
    });

    await settingsUpdateChannelSet("stable");
    expect(tauriInvoke).toHaveBeenLastCalledWith("settings_update_channel_set", {
      channel: "stable",
      confirm: null,
    });
  });

  it("sends only caller-owned fields through the partial settings command", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { settingsPatch } = await import("../settings");
    const current = createTestAppSettings({
      preferred_port: 37123,
      provider_cooldown_seconds: 30,
    });

    await settingsPatch(current, { provider_cooldown_seconds: 99 });

    expect(tauriInvoke).toHaveBeenCalledWith(
      "settings_patch",
      expect.objectContaining({
        patch: expect.objectContaining({
          preferredPort: null,
          logRetentionDays: null,
          failoverMaxAttemptsPerProvider: null,
          failoverMaxProvidersToTry: null,
          autoStart: null,
          providerCooldownSeconds: 99,
        }),
      })
    );
    const calls = vi.mocked(tauriInvoke).mock.calls;
    const sentPatch = calls[calls.length - 1]?.[1]?.patch as Record<string, unknown>;
    expect(sentPatch).not.toHaveProperty("updateChannel");
    expect(
      Object.fromEntries(Object.entries(sentPatch).filter(([, value]) => value !== null))
    ).toEqual({ providerCooldownSeconds: 99 });

    vi.mocked(tauriInvoke).mockClear();
    await settingsPatch(current, { provider_failback_strategy: "disabled" });
    const strategyCalls = vi.mocked(tauriInvoke).mock.calls;
    const strategyPatch = strategyCalls[strategyCalls.length - 1]?.[1]?.patch as Record<
      string,
      unknown
    >;
    expect(
      Object.fromEntries(Object.entries(strategyPatch).filter(([, value]) => value !== null))
    ).toEqual({ providerFailbackStrategy: "disabled" });
  });

  it("never forwards update_channel through the ordinary settings writer", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { settingsPatch } = await import("../settings");
    await settingsPatch(createTestAppSettings(), { update_channel: "beta" } as any);

    const calls = vi.mocked(tauriInvoke).mock.calls;
    const sentPatch = calls[calls.length - 1]?.[1]?.patch as Record<string, unknown>;
    expect(sentPatch).not.toHaveProperty("updateChannel");
    expect(sentPatch).not.toHaveProperty("update_channel");
    expect(Object.values(sentPatch).every((value) => value === null)).toBe(true);
  });

  it("only sends auto-start intent when the patch owns auto_start", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { createSettingsSetInput, settingsSet } = await import("../settings");
    const current = createTestAppSettings({ auto_start: false });

    const unrelated = createSettingsSetInput(current, { provider_cooldown_seconds: 99 });
    expect(unrelated).not.toHaveProperty("autoStart");
    await settingsSet(unrelated);
    expect(tauriInvoke).toHaveBeenLastCalledWith(
      "settings_set",
      expect.objectContaining({
        update: expect.objectContaining({ autoStart: null, providerCooldownSeconds: 99 }),
      })
    );

    const explicit = createSettingsSetInput(current, { auto_start: false });
    expect(explicit.autoStart).toBe(false);
    await settingsSet(explicit);
    expect(tauriInvoke).toHaveBeenLastCalledWith(
      "settings_set",
      expect.objectContaining({ update: expect.objectContaining({ autoStart: false }) })
    );
  });

  it("rejects invalid settings at the frontend boundary before IPC", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { settingsSet } = await import("../settings");
    const required = {
      preferredPort: 37123,
      autoStart: false,
      logRetentionDays: 30,
      failoverMaxAttemptsPerProvider: 5,
      failoverMaxProvidersToTry: 5,
    };

    await expect(
      settingsSet({
        ...required,
        gatewayListenMode: "custom",
        gatewayCustomListenAddress: "http://127.0.0.1:37123",
      } as any)
    ).rejects.toThrow("自定义地址仅支持 host 或 host:port");

    await expect(
      settingsSet({
        ...required,
        upstreamProxyUsername: "x".repeat(257),
      } as any)
    ).rejects.toThrow("代理用户名必须 <= 256 字符");

    await expect(
      settingsSet({
        ...required,
        cx2CcFallbackModelMain: "x".repeat(129),
      } as any)
    ).rejects.toThrow("CX2CC 主模型默认必须 <= 128 字符");

    await expect(
      settingsSet({
        ...required,
        preferredPort: 80,
      } as any)
    ).rejects.toThrow("首选端口必须 >= 1024");

    await expect(
      settingsSet({
        ...required,
        upstreamStreamIdleTimeoutSeconds: 30,
      } as any)
    ).rejects.toThrow("流式空闲超时必须为 0");

    await expect(
      settingsSet({
        ...required,
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 20,
      } as any)
    ).rejects.toThrow("Failover 总尝试次数必须 <= 100");

    await expect(
      settingsSet({
        ...required,
        circuitBreakerOpenDurationMinutes: 1441,
      } as any)
    ).rejects.toThrow("熔断打开时长必须 <= 1440");

    await expect(
      settingsSet({
        ...required,
        providerFailbackStrategy: "future",
      } as any)
    ).rejects.toThrow("回切策略仅支持 natural、aggressive 或 disabled");

    await expect(
      settingsSet({
        ...required,
        naturalProbeMaxWaitSeconds: 86401,
      } as any)
    ).rejects.toThrow("自然模式最长试探等待必须 <= 86400");

    expect(tauriInvoke).not.toHaveBeenCalled();
  });

  it("rejects missing required settings before generated IPC", async () => {
    setTauriRuntime();
    vi.resetModules();
    vi.mocked(tauriInvoke).mockResolvedValue({ schema_version: 1 } as any);

    const { settingsSet } = await import("../settings");

    await expect(
      settingsSet({
        autoStart: false,
        logRetentionDays: 30,
        failoverMaxAttemptsPerProvider: 5,
        failoverMaxProvidersToTry: 5,
      } as any)
    ).rejects.toThrow("SEC_INVALID_INPUT: preferredPort is required");

    expect(tauriInvoke).not.toHaveBeenCalled();
  });
});
