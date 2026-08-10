import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { MemoryRouter } from "react-router-dom";
import type { ReactElement } from "react";
import { toast } from "sonner";
import { CACHE_ANOMALY_MONITOR_GUIDE_COPY } from "../../../../services/gateway/cacheAnomalyMonitorConfig";
import { createUpstreamErrorResponseRule } from "../../../../services/gateway/upstreamErrorResponseRules";
import { DEFAULT_UPSTREAM_RETRY_POLICY } from "../../../../services/gateway/upstreamRetryPolicy";
import { DEFAULT_MODEL_ROUTING_POLICY } from "../../../../services/gateway/modelRoutingPolicy";
import type { GatewayRectifierSettingsPatch } from "../../../../services/settings/settingsGatewayRectifier";
import { createTestAppSettings } from "../../../../test/fixtures/settings";
import { CliManagerGeneralTab, type CliManagerGeneralTabProps } from "../GeneralTab";

const navigateMock = vi.fn();

const mockGatewayUpstreamProxyTest = vi.fn();
const mockGatewayUpstreamProxyDetectIp = vi.fn();
const mockProvidersList = vi.fn();

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }) }));

vi.mock("../../../../services/gateway/gateway", () => ({
  gatewayUpstreamProxyDetectIp: (...args: unknown[]) => mockGatewayUpstreamProxyDetectIp(...args),
  gatewayUpstreamProxyTest: (...args: unknown[]) => mockGatewayUpstreamProxyTest(...args),
}));

vi.mock("../../../../services/consoleLog", () => ({ logToConsole: vi.fn() }));
vi.mock("../../../../services/providers/providers", () => ({
  providersList: (...args: unknown[]) => mockProvidersList(...args),
}));

vi.mock("../../NetworkSettingsCard", () => ({
  NetworkSettingsCard: () => <div>network-card</div>,
}));
vi.mock("../../WslSettingsCard", () => ({ WslSettingsCard: () => <div>wsl-card</div> }));

vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
  return { ...actual, useNavigate: () => navigateMock };
});

beforeEach(() => {
  vi.clearAllMocks();
  mockGatewayUpstreamProxyTest.mockResolvedValue(undefined);
  mockGatewayUpstreamProxyDetectIp.mockResolvedValue("203.0.113.42");
  mockProvidersList.mockResolvedValue([]);
});

function renderTab(element: ReactElement) {
  return render(<MemoryRouter>{element}</MemoryRouter>);
}

function createRectifierPatch(): GatewayRectifierSettingsPatch {
  return {
    verbose_provider_error: true,
    intercept_anthropic_warmup_requests: false,
    enable_thinking_signature_rectifier: true,
    enable_thinking_budget_rectifier: true,
    enable_billing_header_rectifier: true,
    enable_claude_metadata_user_id_injection: true,
    enable_response_fixer: true,
    response_fixer_fix_encoding: true,
    response_fixer_fix_sse_format: true,
    response_fixer_fix_truncated_json: true,
    response_fixer_max_json_depth: 200,
    response_fixer_max_fix_size: 1024,
  };
}

type DefaultPropsOverrides = {
  appSettings?: ReturnType<typeof createTestAppSettings>;
  onPersistCommonSettings?: CliManagerGeneralTabProps["onPersistCommonSettings"];
};

function createDefaultTabProps(overrides: DefaultPropsOverrides = {}) {
  return {
    rectifierAvailable: "available" as const,
    settingsReadErrorMessage: null,
    settingsWriteBlocked: false,
    rectifierSaving: false,
    rectifier: createRectifierPatch(),
    onPersistRectifier: vi.fn(),
    circuitBreakerNoticeEnabled: false,
    circuitBreakerNoticeSaving: false,
    onPersistCircuitBreakerNotice: vi.fn(),
    codexSessionIdCompletionEnabled: true,
    codexSessionIdCompletionSaving: false,
    onPersistCodexSessionIdCompletion: vi.fn(),
    cacheAnomalyMonitorEnabled: false,
    cacheAnomalyMonitorSaving: false,
    onPersistCacheAnomalyMonitor: vi.fn(),
    taskCompleteNotifyEnabled: true,
    taskCompleteNotifySaving: false,
    onPersistTaskCompleteNotify: vi.fn(),
    notificationSoundEnabled: true,
    notificationSoundSaving: false,
    onPersistNotificationSound: vi.fn(),
    appSettings:
      overrides.appSettings ??
      createTestAppSettings({ upstream_proxy_enabled: false, upstream_proxy_url: "" }),
    commonSettingsSaving: false,
    onPersistCommonSettings: overrides.onPersistCommonSettings ?? vi.fn(),
    upstreamFirstByteTimeoutSeconds: 0,
    setUpstreamFirstByteTimeoutSeconds: vi.fn(),
    upstreamStreamIdleTimeoutSeconds: 0,
    setUpstreamStreamIdleTimeoutSeconds: vi.fn(),
    streamInternalErrorGuardMs: 500,
    setStreamInternalErrorGuardMs: vi.fn(),
    upstreamRequestTimeoutNonStreamingSeconds: 0,
    setUpstreamRequestTimeoutNonStreamingSeconds: vi.fn(),
    providerCooldownSeconds: 30,
    setProviderCooldownSeconds: vi.fn(),
    providerFailbackStrategy: "natural" as const,
    setProviderFailbackStrategy: vi.fn(),
    naturalProbeMaxWaitSeconds: 300,
    setNaturalProbeMaxWaitSeconds: vi.fn(),
    providerBaseUrlPingCacheTtlSeconds: 60,
    setProviderBaseUrlPingCacheTtlSeconds: vi.fn(),
    circuitBreakerFailureThreshold: 5,
    setCircuitBreakerFailureThreshold: vi.fn(),
    circuitBreakerOpenDurationMinutes: 30,
    setCircuitBreakerOpenDurationMinutes: vi.fn(),
    upstreamRetryPolicy: DEFAULT_UPSTREAM_RETRY_POLICY,
    setUpstreamRetryPolicy: vi.fn(),
    modelRoutingPolicy: DEFAULT_MODEL_ROUTING_POLICY,
    setModelRoutingPolicy: vi.fn(),
    blurOnEnter: vi.fn(),
  };
}

describe("cli-manager/GeneralTab", () => {
  it("creates a response rule from the unified upstream error handling entry", async () => {
    const appSettings = createTestAppSettings();
    const onPersistCommonSettings = vi.fn().mockImplementation(async (patch) =>
      createTestAppSettings({
        ...patch,
        upstream_error_response_rules: patch.upstream_error_response_rules ?? [],
      })
    );
    renderTab(
      <CliManagerGeneralTab {...createDefaultTabProps({ appSettings, onPersistCommonSettings })} />
    );

    expect(screen.getByRole("heading", { name: "上游错误处理" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /上游错误处理/ }));
    fireEvent.click(screen.getByRole("tab", { name: "最终 HTTP 错误改写" }));
    expect(screen.getByText("最终 HTTP 错误改写规则")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "新建上游错误响应规则" }));
    expect(screen.getByRole("button", { name: "状态码或关键词" })).toBeInTheDocument();
    expect(screen.getByText(/同一组内的多个状态码/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("规则名称"), { target: { value: "限额响应" } });
    fireEvent.change(screen.getByLabelText("匹配上游状态码"), {
      target: { value: "429" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() => {
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_error_response_rules: [
          expect.objectContaining({
            name: "限额响应",
            status_codes: [429],
            enabled: true,
          }),
        ],
      });
    });
  });

  it("filters response-rule providers by CLI and clears incompatible known selections", async () => {
    mockProvidersList.mockImplementation(async (...args: unknown[]) => {
      const cliKey = args[0];
      if (cliKey === "codex") return [{ id: 11, name: "Codex Provider", cli_key: "codex" }];
      if (cliKey === "claude") return [{ id: 22, name: "Claude Provider", cli_key: "claude" }];
      return [];
    });
    const onPersistCommonSettings = vi
      .fn()
      .mockImplementation(async (patch) => createTestAppSettings({ ...patch }));
    renderTab(<CliManagerGeneralTab {...createDefaultTabProps({ onPersistCommonSettings })} />);

    fireEvent.click(screen.getByRole("button", { name: /上游错误处理/ }));
    fireEvent.click(screen.getByRole("tab", { name: "最终 HTTP 错误改写" }));
    fireEvent.click(screen.getByRole("button", { name: "新建上游错误响应规则" }));
    await waitFor(() => {
      expect(mockProvidersList).toHaveBeenCalledWith("codex");
      expect(mockProvidersList).toHaveBeenCalledWith("claude");
      expect(screen.getByText("Codex · Codex Provider")).toBeInTheDocument();
      expect(screen.getByText("Claude · Claude Provider")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByLabelText("Codex"));
    expect(screen.getByText("Codex · Codex Provider")).toBeInTheDocument();
    expect(screen.queryByText("Claude · Claude Provider")).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Codex · Codex Provider"));

    fireEvent.click(screen.getByLabelText("Codex"));
    fireEvent.click(screen.getByLabelText("Claude"));
    expect(screen.queryByText("Codex · Codex Provider")).not.toBeInTheDocument();
    expect(screen.getByText("Claude · Claude Provider")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("规则名称"), { target: { value: "Claude 终态" } });
    fireEvent.change(screen.getByLabelText("匹配上游状态码"), { target: { value: "503" } });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() =>
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_error_response_rules: [
          expect.objectContaining({
            cli_keys: ["claude"],
            provider_ids: [],
          }),
        ],
      })
    );
  });

  it("renders unavailable state", () => {
    renderTab(
      <CliManagerGeneralTab
        rectifierAvailable="unavailable"
        settingsReadErrorMessage={null}
        settingsWriteBlocked={false}
        rectifierSaving={false}
        rectifier={createRectifierPatch()}
        onPersistRectifier={vi.fn()}
        circuitBreakerNoticeEnabled={false}
        circuitBreakerNoticeSaving={false}
        onPersistCircuitBreakerNotice={vi.fn()}
        codexSessionIdCompletionEnabled={true}
        codexSessionIdCompletionSaving={false}
        onPersistCodexSessionIdCompletion={vi.fn()}
        cacheAnomalyMonitorEnabled={false}
        cacheAnomalyMonitorSaving={false}
        onPersistCacheAnomalyMonitor={vi.fn()}
        taskCompleteNotifyEnabled={true}
        taskCompleteNotifySaving={false}
        onPersistTaskCompleteNotify={vi.fn()}
        notificationSoundEnabled={true}
        notificationSoundSaving={false}
        onPersistNotificationSound={vi.fn()}
        appSettings={null}
        commonSettingsSaving={false}
        onPersistCommonSettings={vi.fn()}
        upstreamFirstByteTimeoutSeconds={0}
        setUpstreamFirstByteTimeoutSeconds={vi.fn()}
        upstreamStreamIdleTimeoutSeconds={0}
        setUpstreamStreamIdleTimeoutSeconds={vi.fn()}
        streamInternalErrorGuardMs={500}
        setStreamInternalErrorGuardMs={vi.fn()}
        upstreamRequestTimeoutNonStreamingSeconds={0}
        setUpstreamRequestTimeoutNonStreamingSeconds={vi.fn()}
        providerCooldownSeconds={30}
        setProviderCooldownSeconds={vi.fn()}
        providerFailbackStrategy="natural"
        setProviderFailbackStrategy={vi.fn()}
        naturalProbeMaxWaitSeconds={300}
        setNaturalProbeMaxWaitSeconds={vi.fn()}
        providerBaseUrlPingCacheTtlSeconds={60}
        setProviderBaseUrlPingCacheTtlSeconds={vi.fn()}
        circuitBreakerFailureThreshold={5}
        setCircuitBreakerFailureThreshold={vi.fn()}
        circuitBreakerOpenDurationMinutes={30}
        setCircuitBreakerOpenDurationMinutes={vi.fn()}
        upstreamRetryPolicy={DEFAULT_UPSTREAM_RETRY_POLICY}
        setUpstreamRetryPolicy={vi.fn()}
        modelRoutingPolicy={DEFAULT_MODEL_ROUTING_POLICY}
        setModelRoutingPolicy={vi.fn()}
        blurOnEnter={vi.fn()}
      />
    );

    expect(screen.getAllByText("数据不可用").length).toBeGreaterThan(0);
  });

  it("wires switches, inputs and navigation actions when available", () => {
    navigateMock.mockClear();

    const rectifier = createRectifierPatch();
    const onPersistRectifier = vi.fn();
    const onPersistCircuitBreakerNotice = vi.fn();
    const onPersistCodexSessionIdCompletion = vi.fn();
    const onPersistCacheAnomalyMonitor = vi.fn();
    const onPersistCommonSettings = vi
      .fn()
      .mockResolvedValue(
        createTestAppSettings({ wsl_target_cli: { claude: true, codex: false, gemini: false } })
      );

    const setUpstreamFirstByteTimeoutSeconds = vi.fn();
    const setUpstreamStreamIdleTimeoutSeconds = vi.fn();
    const setStreamInternalErrorGuardMs = vi.fn();
    const setUpstreamRequestTimeoutNonStreamingSeconds = vi.fn();
    const setProviderCooldownSeconds = vi.fn();
    const setProviderFailbackStrategy = vi.fn();
    const setNaturalProbeMaxWaitSeconds = vi.fn();
    const setProviderBaseUrlPingCacheTtlSeconds = vi.fn();
    const setCircuitBreakerFailureThreshold = vi.fn();
    const setCircuitBreakerOpenDurationMinutes = vi.fn();
    const blurOnEnter = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        rectifierAvailable="available"
        settingsReadErrorMessage={null}
        settingsWriteBlocked={false}
        rectifierSaving={false}
        rectifier={rectifier}
        onPersistRectifier={onPersistRectifier}
        circuitBreakerNoticeEnabled={false}
        circuitBreakerNoticeSaving={false}
        onPersistCircuitBreakerNotice={onPersistCircuitBreakerNotice}
        codexSessionIdCompletionEnabled={true}
        codexSessionIdCompletionSaving={false}
        onPersistCodexSessionIdCompletion={onPersistCodexSessionIdCompletion}
        cacheAnomalyMonitorEnabled={false}
        cacheAnomalyMonitorSaving={false}
        onPersistCacheAnomalyMonitor={onPersistCacheAnomalyMonitor}
        taskCompleteNotifyEnabled={true}
        taskCompleteNotifySaving={false}
        onPersistTaskCompleteNotify={vi.fn()}
        notificationSoundEnabled={true}
        notificationSoundSaving={false}
        onPersistNotificationSound={vi.fn()}
        appSettings={createTestAppSettings({
          wsl_target_cli: { claude: true, codex: false, gemini: false },
        })}
        commonSettingsSaving={false}
        onPersistCommonSettings={onPersistCommonSettings}
        upstreamFirstByteTimeoutSeconds={0}
        setUpstreamFirstByteTimeoutSeconds={setUpstreamFirstByteTimeoutSeconds}
        upstreamStreamIdleTimeoutSeconds={0}
        setUpstreamStreamIdleTimeoutSeconds={setUpstreamStreamIdleTimeoutSeconds}
        streamInternalErrorGuardMs={500}
        setStreamInternalErrorGuardMs={setStreamInternalErrorGuardMs}
        upstreamRequestTimeoutNonStreamingSeconds={0}
        setUpstreamRequestTimeoutNonStreamingSeconds={setUpstreamRequestTimeoutNonStreamingSeconds}
        providerCooldownSeconds={30}
        setProviderCooldownSeconds={setProviderCooldownSeconds}
        providerFailbackStrategy="natural"
        setProviderFailbackStrategy={setProviderFailbackStrategy}
        naturalProbeMaxWaitSeconds={300}
        setNaturalProbeMaxWaitSeconds={setNaturalProbeMaxWaitSeconds}
        providerBaseUrlPingCacheTtlSeconds={60}
        setProviderBaseUrlPingCacheTtlSeconds={setProviderBaseUrlPingCacheTtlSeconds}
        circuitBreakerFailureThreshold={5}
        setCircuitBreakerFailureThreshold={setCircuitBreakerFailureThreshold}
        circuitBreakerOpenDurationMinutes={30}
        setCircuitBreakerOpenDurationMinutes={setCircuitBreakerOpenDurationMinutes}
        upstreamRetryPolicy={DEFAULT_UPSTREAM_RETRY_POLICY}
        setUpstreamRetryPolicy={vi.fn()}
        modelRoutingPolicy={DEFAULT_MODEL_ROUTING_POLICY}
        setModelRoutingPolicy={vi.fn()}
        blurOnEnter={blurOnEnter}
      />
    );

    // Navigation
    fireEvent.click(screen.getByRole("button", { name: "打开控制台" }));
    expect(navigateMock).toHaveBeenCalledWith("/console");

    // Toggle a few switches to execute handler paths.
    const switches = screen.getAllByRole("switch");
    expect(switches.length).toBeGreaterThan(5);
    for (const el of switches) {
      fireEvent.click(el);
    }
    expect(onPersistRectifier).toHaveBeenCalled();
    expect(onPersistCircuitBreakerNotice).toHaveBeenCalled();
    expect(onPersistCodexSessionIdCompletion).toHaveBeenCalled();
    expect(onPersistCacheAnomalyMonitor).toHaveBeenCalled();

    // Copy is sourced from central config.
    expect(screen.getByText(CACHE_ANOMALY_MONITOR_GUIDE_COPY.overview)).toBeInTheDocument();
    expect(screen.getByText(CACHE_ANOMALY_MONITOR_GUIDE_COPY.thresholds)).toBeInTheDocument();

    // Inputs: change + blur should validate and persist (or toast on invalid)
    fireEvent.click(screen.getByRole("button", { name: /熔断与回切/ }));
    expect(screen.getByText("最长熔断等待（可提前试探）")).toBeInTheDocument();
    expect(screen.getByText(/策略触发不会绕过它/)).toBeInTheDocument();
    const firstByteInput = screen.getByRole("spinbutton", { name: "首字节超时" });
    fireEvent.keyDown(firstByteInput, { key: "Enter" });
    expect(blurOnEnter).toHaveBeenCalled();

    fireEvent.change(firstByteInput, { target: { value: "5" } });
    fireEvent.blur(firstByteInput, { target: { value: "5" } });
    expect(setUpstreamFirstByteTimeoutSeconds).toHaveBeenCalled();
    expect(onPersistCommonSettings).toHaveBeenCalledWith({
      upstream_first_byte_timeout_seconds: 5,
    });

    fireEvent.click(screen.getByRole("button", { name: /上游错误处理/ }));
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));
    const guardInput = screen.getByRole("spinbutton", { name: "流内部错误观察窗口" });
    fireEvent.change(guardInput, { target: { value: "750" } });
    expect(setStreamInternalErrorGuardMs).toHaveBeenCalledWith(750);

    const streamIdleInput = screen.getByRole("spinbutton", { name: "流式空闲超时" });
    fireEvent.change(streamIdleInput, { target: { value: "-1" } });
    fireEvent.blur(streamIdleInput, { target: { value: "-1" } });
    expect(toast).toHaveBeenCalledWith("上游流式空闲超时必须为 0（禁用）或 60-3600 秒");
    expect(setUpstreamStreamIdleTimeoutSeconds).toHaveBeenCalled();

    const nonStreamingInput = screen.getByRole("spinbutton", { name: "非流式总超时" });
    fireEvent.change(nonStreamingInput, { target: { value: "10" } });
    fireEvent.blur(nonStreamingInput, { target: { value: "10" } });
    expect(setUpstreamRequestTimeoutNonStreamingSeconds).toHaveBeenCalled();
    expect(onPersistCommonSettings).toHaveBeenCalledWith({
      upstream_request_timeout_non_streaming_seconds: 10,
    });

    const naturalMaxWaitInput = screen.getByRole("spinbutton", {
      name: "自然模式最长回切等待",
    });
    expect(
      screen.getByText(/即使仍显示 CLOSED.*OPEN\/HALF_OPEN 则申请单飞探测/)
    ).toBeInTheDocument();
    fireEvent.change(naturalMaxWaitInput, { target: { value: "301" } });
    fireEvent.blur(naturalMaxWaitInput, { target: { value: "301" } });
    expect(setNaturalProbeMaxWaitSeconds).toHaveBeenCalled();
    expect(onPersistCommonSettings).toHaveBeenCalledWith({
      natural_probe_max_wait_seconds: 301,
    });
    const cooldownInput = screen.getByRole("spinbutton", {
      name: "Provider 冷却 / 试探最短间隔",
    });
    fireEvent.change(cooldownInput, { target: { value: "12" } });
    fireEvent.blur(cooldownInput, { target: { value: "12" } });
    expect(setProviderCooldownSeconds).toHaveBeenCalled();
    expect(onPersistCommonSettings).toHaveBeenCalledWith({ provider_cooldown_seconds: 12 });

    const pingTtlInput = screen.getByRole("spinbutton", { name: "Ping 选择缓存 TTL" });
    fireEvent.change(pingTtlInput, { target: { value: "120" } });
    fireEvent.blur(pingTtlInput, { target: { value: "120" } });
    expect(setProviderBaseUrlPingCacheTtlSeconds).toHaveBeenCalled();

    const thresholdInput = screen.getByRole("spinbutton", { name: "熔断阈值" });
    fireEvent.change(thresholdInput, { target: { value: "6" } });
    fireEvent.blur(thresholdInput, { target: { value: "6" } });
    expect(setCircuitBreakerFailureThreshold).toHaveBeenCalled();

    const openDurationInput = screen.getByRole("spinbutton", { name: "最长熔断等待" });
    fireEvent.change(openDurationInput, { target: { value: "31" } });
    fireEvent.blur(openDurationInput, { target: { value: "31" } });
    expect(setCircuitBreakerOpenDurationMinutes).toHaveBeenCalled();

    fireEvent.click(screen.getByRole("radio", { name: "积极回切" }));
    expect(setProviderFailbackStrategy).toHaveBeenCalledWith("aggressive");
    expect(onPersistCommonSettings).toHaveBeenCalledWith({
      provider_failback_strategy: "aggressive",
    });
  });

  it("rejects an invalid natural probe max wait before persisting", () => {
    const onPersistCommonSettings = vi.fn();
    const setNaturalProbeMaxWaitSeconds = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({ onPersistCommonSettings })}
        setNaturalProbeMaxWaitSeconds={setNaturalProbeMaxWaitSeconds}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /熔断与回切/ }));
    const input = screen.getByRole("spinbutton", { name: "自然模式最长回切等待" });
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.blur(input, { target: { value: "0" } });

    expect(toast).toHaveBeenCalledWith("自然模式最长回切等待必须为 1-86400 秒");
    expect(setNaturalProbeMaxWaitSeconds).toHaveBeenLastCalledWith(300);
    expect(onPersistCommonSettings).not.toHaveBeenCalled();
  });

  it("hides the natural-only max wait input in aggressive mode", () => {
    renderTab(
      <CliManagerGeneralTab {...createDefaultTabProps()} providerFailbackStrategy="aggressive" />
    );

    fireEvent.click(screen.getByRole("button", { name: /熔断与回切/ }));
    expect(
      screen.queryByRole("spinbutton", { name: "自然模式最长回切等待" })
    ).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "积极回切" })).toBeChecked();
  });

  it("switches modes and keeps retry and rewrite persistence isolated", async () => {
    const upstreamRetryPolicy = {
      ...DEFAULT_UPSTREAM_RETRY_POLICY,
      max_retries: 2,
      backoff_ms: 250,
    };
    const responseRule = {
      ...createUpstreamErrorResponseRule([]),
      name: "容量终态",
      status_codes: [429],
    };
    const appSettings = createTestAppSettings({
      upstream_retry_policy: upstreamRetryPolicy,
      upstream_error_response_rules: [responseRule],
    });
    const onPersistCommonSettings = vi.fn().mockImplementation(async (patch) =>
      createTestAppSettings({
        ...appSettings,
        ...patch,
      })
    );
    const setUpstreamRetryPolicy = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings,
          onPersistCommonSettings,
        })}
        upstreamRetryPolicy={upstreamRetryPolicy}
        setUpstreamRetryPolicy={setUpstreamRetryPolicy}
      />
    );

    const upstreamErrorSection = screen.getByRole("button", { name: /上游错误处理/ });
    expect(upstreamErrorSection).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("tab", { name: "重试规则" })).not.toBeInTheDocument();
    fireEvent.click(upstreamErrorSection);
    expect(upstreamErrorSection).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("tab", { name: "重试规则" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("HTTP 规则")).toBeInTheDocument();
    expect(screen.queryByText("最终 HTTP 错误改写规则")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存重试策略" }));

    await waitFor(() =>
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_retry_policy: upstreamRetryPolicy,
        stream_internal_error_guard_ms: 500,
      })
    );
    expect(setUpstreamRetryPolicy).toHaveBeenCalledWith(upstreamRetryPolicy);

    onPersistCommonSettings.mockClear();
    fireEvent.click(screen.getByRole("tab", { name: "最终 HTTP 错误改写" }));
    expect(screen.getByRole("tab", { name: "最终 HTTP 错误改写" })).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(screen.queryByText("HTTP 规则")).not.toBeInTheDocument();
    expect(screen.getByText("最终 HTTP 错误改写规则")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: "停用规则 容量终态" }));
    await waitFor(() =>
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_error_response_rules: [
          expect.objectContaining({ name: "容量终态", enabled: false }),
        ],
      })
    );
  });

  it("normalizes and saves the global model routing policy", async () => {
    const modelRoutingPolicy = {
      enabled: true,
      rules: [
        {
          source_model: " source ",
          target_model: " target ",
          reasoning_effort: " low ",
        },
      ],
    };
    const normalized = {
      enabled: true,
      rules: [{ source_model: "source", target_model: "target", reasoning_effort: "low" }],
    };
    const onPersistCommonSettings = vi
      .fn()
      .mockResolvedValue(createTestAppSettings({ model_routing_policy: normalized }));
    const setModelRoutingPolicy = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({ onPersistCommonSettings })}
        modelRoutingPolicy={modelRoutingPolicy}
        setModelRoutingPolicy={setModelRoutingPolicy}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /模型路由/ }));
    fireEvent.click(screen.getByRole("button", { name: "保存模型路由" }));

    await waitFor(() =>
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        model_routing_policy: normalized,
      })
    );
    expect(setModelRoutingPolicy).toHaveBeenCalledWith(normalized);
  });

  it("rejects an invalid Codex stream observation window when saving retry settings", () => {
    const onPersistCommonSettings = vi.fn();
    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({ onPersistCommonSettings })}
        streamInternalErrorGuardMs={5001}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /上游错误处理/ }));
    fireEvent.click(screen.getByRole("button", { name: "保存重试策略" }));

    expect(toast).toHaveBeenCalledWith("流内部错误观察窗口必须为 0-5000 毫秒");
    expect(onPersistCommonSettings).not.toHaveBeenCalled();
  });

  it("keeps segmented modes and longest rule text bounded for narrow layouts", () => {
    const longName = Array.from("超长上游错误响应规则名称".repeat(16)).slice(0, 100).join("");
    const responseRule = {
      ...createUpstreamErrorResponseRule([]),
      name: longName,
      status_codes: [429, 500, 502, 503, 504],
    };
    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_error_response_rules: [responseRule],
          }),
        })}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /上游错误处理/ }));
    const modeTabs = screen.getByRole("tablist", { name: "上游错误处理模式" });
    expect(modeTabs).toHaveClass("w-full");
    for (const tab of screen.getAllByRole("tab")) expect(tab).toHaveClass("min-w-0");

    fireEvent.click(screen.getByRole("tab", { name: "最终 HTTP 错误改写" }));
    const ruleName = screen.getByText(longName);
    expect(ruleName).toHaveClass("truncate");
    expect(ruleName.closest(".min-w-0")).not.toBeNull();

    const detailButton = screen.getByRole("button", { name: `查看规则详情 ${longName}` });
    expect(detailButton).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(detailButton);
    expect(screen.getByRole("button", { name: `收起规则详情 ${longName}` })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    expect(screen.getByText("匹配条件")).toBeInTheDocument();
    expect(screen.getByText(/状态码 429、500、502、503、504/)).toBeInTheDocument();
  });

  it("shows readonly banner and disables settings controls", () => {
    renderTab(
      <CliManagerGeneralTab
        rectifierAvailable="available"
        settingsReadErrorMessage="设置文件读取失败"
        settingsWriteBlocked={true}
        rectifierSaving={false}
        rectifier={createRectifierPatch()}
        onPersistRectifier={vi.fn()}
        circuitBreakerNoticeEnabled={false}
        circuitBreakerNoticeSaving={false}
        onPersistCircuitBreakerNotice={vi.fn()}
        codexSessionIdCompletionEnabled={true}
        codexSessionIdCompletionSaving={false}
        onPersistCodexSessionIdCompletion={vi.fn()}
        cacheAnomalyMonitorEnabled={false}
        cacheAnomalyMonitorSaving={false}
        onPersistCacheAnomalyMonitor={vi.fn()}
        taskCompleteNotifyEnabled={true}
        taskCompleteNotifySaving={false}
        onPersistTaskCompleteNotify={vi.fn()}
        notificationSoundEnabled={true}
        notificationSoundSaving={false}
        onPersistNotificationSound={vi.fn()}
        appSettings={createTestAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={vi.fn()}
        upstreamFirstByteTimeoutSeconds={0}
        setUpstreamFirstByteTimeoutSeconds={vi.fn()}
        upstreamStreamIdleTimeoutSeconds={0}
        setUpstreamStreamIdleTimeoutSeconds={vi.fn()}
        streamInternalErrorGuardMs={500}
        setStreamInternalErrorGuardMs={vi.fn()}
        upstreamRequestTimeoutNonStreamingSeconds={0}
        setUpstreamRequestTimeoutNonStreamingSeconds={vi.fn()}
        providerCooldownSeconds={30}
        setProviderCooldownSeconds={vi.fn()}
        providerFailbackStrategy="natural"
        setProviderFailbackStrategy={vi.fn()}
        naturalProbeMaxWaitSeconds={300}
        setNaturalProbeMaxWaitSeconds={vi.fn()}
        providerBaseUrlPingCacheTtlSeconds={60}
        setProviderBaseUrlPingCacheTtlSeconds={vi.fn()}
        circuitBreakerFailureThreshold={5}
        setCircuitBreakerFailureThreshold={vi.fn()}
        circuitBreakerOpenDurationMinutes={30}
        setCircuitBreakerOpenDurationMinutes={vi.fn()}
        upstreamRetryPolicy={DEFAULT_UPSTREAM_RETRY_POLICY}
        setUpstreamRetryPolicy={vi.fn()}
        modelRoutingPolicy={DEFAULT_MODEL_ROUTING_POLICY}
        setModelRoutingPolicy={vi.fn()}
        blurOnEnter={vi.fn()}
      />
    );

    expect(screen.getByText("设置文件读取失败")).toBeInTheDocument();
    expect(screen.getAllByRole("switch")[0]).toBeDisabled();
    expect(screen.getAllByRole("spinbutton")[0]).toBeDisabled();
  });

  it("tests proxy connectivity with the backend command", async () => {
    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    expect(screen.getByText("上游代理")).toBeInTheDocument();

    const proxyUrlInput = screen.getByPlaceholderText("http://127.0.0.1:7890");
    expect(proxyUrlInput).toBeInTheDocument();

    fireEvent.change(proxyUrlInput, { target: { value: "http://127.0.0.1:7890" } });
    fireEvent.change(screen.getByPlaceholderText("proxy-user"), {
      target: { value: "proxy-user" },
    });
    fireEvent.change(screen.getByPlaceholderText("proxy-password"), {
      target: { value: "secret" },
    });

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));

    await waitFor(() => {
      expect(mockGatewayUpstreamProxyTest).toHaveBeenCalledWith({
        proxyUrl: "http://127.0.0.1:7890",
        proxyUsername: "proxy-user",
        proxyPassword: "secret",
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理连接测试成功");
  });

  it("detects proxy exit ip with the backend command", async () => {
    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    fireEvent.change(screen.getByPlaceholderText("http://127.0.0.1:7890"), {
      target: { value: "socks5://192.168.31.41:6153" },
    });
    fireEvent.change(screen.getByPlaceholderText("proxy-user"), {
      target: { value: "proxy-user" },
    });
    fireEvent.change(screen.getByPlaceholderText("proxy-password"), {
      target: { value: "secret" },
    });

    fireEvent.click(screen.getByRole("button", { name: "检测出口 IP" }));

    await waitFor(() => {
      expect(mockGatewayUpstreamProxyDetectIp).toHaveBeenCalledWith({
        proxyUrl: "socks5://192.168.31.41:6153",
        proxyUsername: "proxy-user",
        proxyPassword: "secret",
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理出口 IP: 203.0.113.42");
  });

  it("shows error when enabling proxy without URL", async () => {
    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    fireEvent.click(screen.getByRole("switch", { name: "启用上游代理" }));

    await waitFor(() => {
      expect(toast).toHaveBeenCalledWith("请先输入代理地址");
    });
  });

  it("rejects invalid proxy protocol before enabling", async () => {
    const onPersistCommonSettings = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: false,
            upstream_proxy_url: "ftp://proxy.example.com:21",
          }),
          onPersistCommonSettings,
        })}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用上游代理" }));

    await waitFor(() => {
      expect(toast).toHaveBeenCalledWith("代理地址协议仅支持 http/https/socks5/socks5h");
    });
    expect(onPersistCommonSettings).not.toHaveBeenCalled();
  });

  it("rejects invalid proxy protocol before running test command", async () => {
    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    fireEvent.change(screen.getByPlaceholderText("http://127.0.0.1:7890"), {
      target: { value: "ftp://proxy.example.com:21" },
    });

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));

    await waitFor(() => {
      expect(toast).toHaveBeenCalledWith("代理地址协议仅支持 http/https/socks5/socks5h");
    });
    expect(mockGatewayUpstreamProxyTest).not.toHaveBeenCalled();
  });

  it("handles proxy connectivity failure on test", async () => {
    mockGatewayUpstreamProxyTest.mockRejectedValue(new Error("connection refused"));

    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    fireEvent.change(screen.getByPlaceholderText("http://127.0.0.1:7890"), {
      target: { value: "socks5://bad-proxy:1080" },
    });

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("代理连接测试失败"));
    });
  });

  it("handles proxy exit ip detection failure", async () => {
    mockGatewayUpstreamProxyDetectIp.mockRejectedValue(new Error("request timed out"));

    renderTab(<CliManagerGeneralTab {...createDefaultTabProps()} />);

    fireEvent.change(screen.getByPlaceholderText("http://127.0.0.1:7890"), {
      target: { value: "socks5://bad-proxy:1080" },
    });

    fireEvent.click(screen.getByRole("button", { name: "检测出口 IP" }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("代理出口 IP 检测失败"));
    });
  });

  it("handles proxy URL blur update through persisted settings", async () => {
    const onPersistCommonSettings = vi.fn().mockResolvedValue(
      createTestAppSettings({
        upstream_proxy_enabled: true,
        upstream_proxy_url: "http://127.0.0.1:7890",
      })
    );

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: true,
            upstream_proxy_url: "socks5://old-proxy:1080",
          }),
          onPersistCommonSettings,
        })}
      />
    );

    const proxyUrlInput = screen.getByDisplayValue("socks5://old-proxy:1080");
    fireEvent.change(proxyUrlInput, { target: { value: "http://127.0.0.1:7890" } });
    fireEvent.blur(proxyUrlInput);

    await waitFor(() => {
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_proxy_url: "http://127.0.0.1:7890",
        upstream_proxy_username: "",
        upstream_proxy_password: { mode: "preserve" },
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理地址已更新");
  });

  it("persists proxy credentials through common settings", async () => {
    const onPersistCommonSettings = vi.fn().mockResolvedValue(
      createTestAppSettings({
        upstream_proxy_enabled: true,
        upstream_proxy_url: "http://127.0.0.1:7890",
        upstream_proxy_username: "proxy-user",
        upstream_proxy_password_configured: true,
      })
    );

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: true,
            upstream_proxy_url: "http://127.0.0.1:7890",
            upstream_proxy_username: "",
            upstream_proxy_password_configured: true,
          }),
          onPersistCommonSettings,
        })}
      />
    );

    fireEvent.change(screen.getByPlaceholderText("proxy-user"), {
      target: { value: " proxy-user " },
    });
    fireEvent.change(screen.getByPlaceholderText("留空表示保留已保存密码"), {
      target: { value: "secret" },
    });
    fireEvent.blur(screen.getByPlaceholderText("留空表示保留已保存密码"));

    await waitFor(() => {
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_proxy_url: "http://127.0.0.1:7890",
        upstream_proxy_username: "proxy-user",
        upstream_proxy_password: { mode: "replace", value: "secret" },
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理认证信息已更新");
  });

  it("rejects oversized proxy username before persisting", async () => {
    const onPersistCommonSettings = vi.fn();

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: true,
            upstream_proxy_url: "http://127.0.0.1:7890",
          }),
          onPersistCommonSettings,
        })}
      />
    );

    const usernameInput = screen.getByPlaceholderText("proxy-user");
    fireEvent.change(usernameInput, { target: { value: "x".repeat(257) } });
    fireEvent.blur(usernameInput);

    await waitFor(() => {
      expect(toast).toHaveBeenCalledWith("代理用户名必须 <= 256 字符");
    });
    expect(onPersistCommonSettings).not.toHaveBeenCalled();
  });

  it("prevents clearing proxy URL when proxy is enabled", async () => {
    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: true,
            upstream_proxy_url: "http://127.0.0.1:7890",
          }),
        })}
      />
    );

    const proxyUrlInput = screen.getByDisplayValue("http://127.0.0.1:7890");
    fireEvent.change(proxyUrlInput, { target: { value: "" } });
    fireEvent.blur(proxyUrlInput);

    await waitFor(() => {
      expect(toast).toHaveBeenCalledWith("代理已启用时地址不能为空");
    });
  });

  it("successfully enables proxy with valid URL", async () => {
    const onPersistCommonSettings = vi.fn().mockResolvedValue(
      createTestAppSettings({
        upstream_proxy_enabled: true,
        upstream_proxy_url: "http://127.0.0.1:7890",
      })
    );

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: false,
            upstream_proxy_url: "http://127.0.0.1:7890",
          }),
          onPersistCommonSettings,
        })}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用上游代理" }));

    await waitFor(() => {
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_proxy_enabled: true,
        upstream_proxy_url: "http://127.0.0.1:7890",
        upstream_proxy_username: "",
        upstream_proxy_password: { mode: "preserve" },
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理已启用");
  });

  it("disables proxy successfully", async () => {
    const onPersistCommonSettings = vi.fn().mockResolvedValue(
      createTestAppSettings({
        upstream_proxy_enabled: false,
        upstream_proxy_url: "http://127.0.0.1:7890",
      })
    );

    renderTab(
      <CliManagerGeneralTab
        {...createDefaultTabProps({
          appSettings: createTestAppSettings({
            upstream_proxy_enabled: true,
            upstream_proxy_url: "http://127.0.0.1:7890",
          }),
          onPersistCommonSettings,
        })}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用上游代理" }));

    await waitFor(() => {
      expect(onPersistCommonSettings).toHaveBeenCalledWith({
        upstream_proxy_enabled: false,
        upstream_proxy_url: "http://127.0.0.1:7890",
        upstream_proxy_username: "",
        upstream_proxy_password: { mode: "preserve" },
      });
    });
    expect(toast.success).toHaveBeenCalledWith("代理已禁用");
  });
});
