import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { cliManagerCodexConfigTomlValidate } from "../../../../services/cli/cliManager";
import { CliManagerCodexTab } from "../CodexTab";
import { createTestAppSettings } from "../../../../test/fixtures/settings";

vi.mock("../../../../utils/platform", () => ({
  isWindowsRuntime: () => true,
}));

vi.mock("../../../../ui/CodeEditor", () => ({
  CodeEditor: ({ value, onChange, readOnly }: any) => (
    <textarea
      aria-label="mock-code-editor"
      value={value}
      readOnly={readOnly}
      onChange={(e) => onChange?.(e.currentTarget.value)}
    />
  ),
}));

vi.mock("../../../../services/cli/cliManager", async () => {
  const actual = await vi.importActual<typeof import("../../../../services/cli/cliManager")>(
    "../../../../services/cli/cliManager"
  );
  return {
    ...actual,
    cliManagerCodexConfigTomlValidate: vi.fn().mockResolvedValue({
      ok: true,
      error: null,
    }),
  };
});
function createCodexInfo(overrides: Partial<any> = {}) {
  return {
    found: true,
    version: "0.0.0",
    executable_path: "/bin/codex",
    resolved_via: "PATH",
    shell: "/bin/zsh",
    error: null,
    ...overrides,
  };
}

function createCodexConfig(overrides: Partial<any> = {}) {
  return {
    config_dir: "/home/user/.codex",
    config_path: "/home/user/.codex/config.toml",
    user_home_default_dir: "C:\\Users\\MyPC\\.codex",
    user_home_default_path: "C:\\Users\\MyPC\\.codex\\config.toml",
    follow_codex_home_dir: "C:\\Users\\MyPC\\.codex",
    follow_codex_home_path: "C:\\Users\\MyPC\\.codex\\config.toml",
    can_open_config_dir: true,
    exists: true,
    model: "gpt-5-codex",
    approval_policy: "on-request",
    sandbox_mode: "workspace-write",
    sandbox_workspace_write_network_access: null,
    model_reasoning_effort: "medium",
    plan_mode_reasoning_effort: null,
    web_search: "cached",
    personality: null,
    model_context_window: null,
    model_auto_compact_token_limit: null,
    service_tier: null,
    features_shell_snapshot: false,
    features_unified_exec: false,
    features_shell_tool: false,
    features_exec_policy: false,
    features_apply_patch_freeform: false,
    features_remote_compaction: false,
    features_fast_mode: false,
    features_responses_websockets_v2: false,
    features_multi_agent: false,
    ...overrides,
  };
}

function createAppSettings(overrides: Parameters<typeof createTestAppSettings>[0] = {}) {
  return createTestAppSettings({
    codex_home_mode: "user_home_default",
    codex_home_override: "",
    ...overrides,
  });
}

function createReasoningGuardStats(overrides: Partial<any> = {}) {
  return {
    hit_request_count: 4,
    hit_attempt_count: 9,
    normal_request_count: 28,
    total_request_count: 32,
    hit_rate: 0.125,
    by_model: [
      {
        requested_model: "gpt-5-codex",
        total_request_count: 20,
        hit_request_count: 4,
        normal_request_count: 16,
        hit_attempt_count: 9,
        hit_rate: 0.2,
      },
    ],
    ...overrides,
  };
}

describe("components/cli-manager/tabs/CodexTab", () => {
  it("handles sandbox confirm flow and toggles", () => {
    const persistCodexConfig = vi.fn();
    const refreshCodex = vi.fn();
    const openCodexConfigDir = vi.fn();

    const confirmSpy = vi
      .spyOn(window, "confirm")
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        refreshCodex={refreshCodex}
        openCodexConfigDir={openCodexConfigDir}
        persistCodexConfig={persistCodexConfig}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    expect(refreshCodex).toHaveBeenCalled();

    // Select danger-full-access but cancel.
    const sandboxItem = screen.getByText("沙箱模式 (sandbox_mode)").parentElement?.parentElement;
    expect(sandboxItem).toBeTruthy();
    const sandboxSelect = within(sandboxItem as HTMLElement).getByRole("combobox");
    fireEvent.change(sandboxSelect, { target: { value: "danger-full-access" } });
    expect(confirmSpy).toHaveBeenCalled();
    expect(persistCodexConfig).not.toHaveBeenCalledWith(
      expect.objectContaining({ sandbox_mode: "danger-full-access" })
    );

    // Confirm selection.
    fireEvent.change(sandboxSelect, { target: { value: "danger-full-access" } });
    expect(persistCodexConfig).toHaveBeenCalledWith({ sandbox_mode: "danger-full-access" });

    // Toggle the linked fast mode switch.
    const fastModeItem = screen.getByText("fast_mode").parentElement?.parentElement;
    expect(fastModeItem).toBeTruthy();
    fireEvent.click(within(fastModeItem as HTMLElement).getByRole("switch"));
    expect(persistCodexConfig).toHaveBeenCalledWith({
      features_fast_mode: true,
      service_tier: "fast",
    });

    const websocketItem = screen.getByText("responses_websockets_v2").parentElement?.parentElement;
    expect(websocketItem).toBeTruthy();
    fireEvent.click(within(websocketItem as HTMLElement).getByRole("switch"));
    expect(persistCodexConfig).toHaveBeenCalledWith({
      features_responses_websockets_v2: true,
    });

    // Radio group
    fireEvent.click(screen.getByRole("radio", { name: "禁用 (disabled)" }));
    expect(persistCodexConfig).toHaveBeenCalledWith({ web_search: "disabled" });

    const personalityItem = screen.getByText("输出风格 (personality)").parentElement?.parentElement;
    expect(personalityItem).toBeTruthy();
    fireEvent.click(
      within(personalityItem as HTMLElement).getByRole("radio", { name: "友好 (friendly)" })
    );
    expect(persistCodexConfig).toHaveBeenCalledWith({ personality: "friendly" });

    fireEvent.click(
      within(personalityItem as HTMLElement).getByRole("radio", {
        name: "默认 / 删除配置 (none)",
      })
    );
    expect(persistCodexConfig).toHaveBeenCalledWith({ personality: "" });

    // Model input blur persists trimmed value and clears gpt-5.4-only linked keys.
    const modelItem = screen.getByText("默认模型 (model)").parentElement?.parentElement;
    expect(modelItem).toBeTruthy();
    const modelInput = within(modelItem as HTMLElement).getByRole("textbox");
    fireEvent.change(modelInput, { target: { value: "  gpt-5-codex  " } });
    fireEvent.blur(modelInput);
    expect(persistCodexConfig).toHaveBeenCalledWith({
      model: "gpt-5-codex",
      model_context_window: null,
      model_auto_compact_token_limit: null,
    });

    // Approval policy select persists.
    const approvalItem =
      screen.getByText("审批策略 (approval_policy)").parentElement?.parentElement;
    expect(approvalItem).toBeTruthy();
    const approvalSelect = within(approvalItem as HTMLElement).getByRole("combobox");
    fireEvent.change(approvalSelect, { target: { value: "never" } });
    expect(persistCodexConfig).toHaveBeenCalledWith({ approval_policy: "never" });

    // Exercise remaining toggle handlers for function/branch coverage.
    for (const sw of screen.getAllByRole("switch")) fireEvent.click(sw);

    confirmSpy.mockRestore();
  });

  it("toggles Codex OAuth compatible proxy mode from app settings", () => {
    const persistCodexOauthCompatibleProxyMode = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings({ codex_oauth_compatible_proxy_mode: false })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexOauthCompatibleProxyMode={persistCodexOauthCompatibleProxyMode}
      />
    );

    expect(screen.getByText("OAuth 兼容代理模式")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex OAuth 兼容代理模式" }));

    expect(persistCodexOauthCompatibleProxyMode).toHaveBeenCalledWith(true);
  });

  it("persists the global provider test model and falls back to the default when blank", async () => {
    const persistCommonSettings = vi
      .fn()
      .mockResolvedValueOnce(createAppSettings({ codex_provider_test_model: "gpt-5.4" }))
      .mockResolvedValueOnce(createAppSettings({ codex_provider_test_model: "gpt-5.4-mini" }));

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings({ codex_provider_test_model: "gpt-5-codex" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCommonSettings={persistCommonSettings}
      />
    );

    const field = screen.getByText("供应商测试默认模型").parentElement?.parentElement;
    expect(field).toBeTruthy();
    const input = within(field as HTMLElement).getByRole("textbox");

    fireEvent.change(input, { target: { value: "  gpt-5.4  " } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(persistCommonSettings).toHaveBeenNthCalledWith(1, {
        codex_provider_test_model: "gpt-5.4",
      })
    );

    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(persistCommonSettings).toHaveBeenNthCalledWith(2, {
        codex_provider_test_model: "gpt-5.4-mini",
      })
    );
  });

  it("persists Codex reasoning guard toggle and renders hit stats", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);
    const { rerender } = render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings({ codex_reasoning_guard_enabled: false })}
        codexReasoningGuardSessionStats={createReasoningGuardStats()}
        codexReasoningGuardAllStats={createReasoningGuardStats({
          hit_request_count: 12,
          hit_attempt_count: 18,
          normal_request_count: 68,
          total_request_count: 80,
          hit_rate: 0.15,
        })}
        appSessionStartedAtMs={1_770_000_000_000}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    expect(screen.getByText("命中请求数")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "本次应用打开后" })[0]).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("12.5%")).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "全部统计" })[0]);
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("18")).toBeInTheDocument();
    expect(screen.getByText("15.0%")).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "本次应用打开后" })[0]);
    expect(screen.getByText("4")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex 降智拦截" }));
    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_enabled: true,
    });

    rerender(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings({ codex_reasoning_guard_enabled: true })}
        codexReasoningGuardSessionStats={createReasoningGuardStats()}
        codexReasoningGuardAllStats={createReasoningGuardStats({
          hit_request_count: 12,
          hit_attempt_count: 18,
          normal_request_count: 68,
          total_request_count: 80,
          hit_rate: 0.15,
        })}
        appSessionStartedAtMs={1_770_000_000_000}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex 降智拦截" }));
    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_enabled: false,
    });
  });

  it("saves Codex reasoning guard rules from detail dialog", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");
    const input = within(dialog).getByDisplayValue("516, 1034, 1552") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "516, 1024" } });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_compare_mode: "equals",
      codex_reasoning_guard_reasoning_equals: [516, 1024],
      codex_reasoning_guard_model_rules: [],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
    });
  });

  it("saves Codex reasoning guard budget settings from detail dialog", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("立即重试次数"), {
      target: { value: "4" },
    });
    fireEvent.change(within(dialog).getByLabelText("等待重试次数"), {
      target: { value: "3" },
    });
    fireEvent.change(within(dialog).getByLabelText("等待毫秒数"), {
      target: { value: "1500" },
    });
    fireEvent.change(within(dialog).getByLabelText("预算耗尽后"), {
      target: { value: "switch_provider" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_compare_mode: "equals",
      codex_reasoning_guard_reasoning_equals: [516, 1034, 1552],
      codex_reasoning_guard_model_rules: [],
      codex_reasoning_guard_immediate_retry_budget: 4,
      codex_reasoning_guard_delayed_retry_budget: 3,
      codex_reasoning_guard_delayed_retry_ms: 1500,
      codex_reasoning_guard_exhausted_action: "switch_provider",
    });
  });

  it("shows validation for invalid Codex reasoning guard budget settings", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("立即重试次数"), {
      target: { value: "101" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText("立即重试预算必须在 0 到 100 之间。")).toBeInTheDocument();
  });

  it("saves Codex reasoning guard compare mode and model rules", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings({ codex_reasoning_guard_compare_mode: "equals" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");
    const compareSelect = within(dialog).getAllByDisplayValue("等于 (==)")[0] as HTMLSelectElement;
    fireEvent.change(compareSelect, { target: { value: "less_than_or_equal" } });
    fireEvent.click(screen.getByRole("button", { name: "新增模型规则" }));
    fireEvent.change(within(dialog).getByPlaceholderText("例如：gpt-5-codex"), {
      target: { value: "gpt-5-mini-codex" },
    });
    fireEvent.change(within(dialog).getByDisplayValue("516") as HTMLInputElement, {
      target: { value: "256" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_compare_mode: "less_than_or_equal",
      codex_reasoning_guard_reasoning_equals: [516, 1034, 1552],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      codex_reasoning_guard_model_rules: [
        {
          requested_model: "gpt-5-mini-codex",
          compare_mode: "equals",
          reasoning_equals: [256],
        },
      ],
    });
    expect(
      screen.getByText(
        "多个值请用英文逗号分隔。命中条件为 reasoning_tokens 小于等于任一规则值；若有多个阈值，会优先匹配更贴近的较小阈值。"
      )
    ).toBeInTheDocument();
  });

  it("defaults missing Codex model rule compare mode to equals", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\n',
        }}
        appSettings={createAppSettings({
          codex_reasoning_guard_model_rules: [
            {
              requested_model: "gpt-5-mini-codex",
              reasoning_equals: [256],
            },
          ],
        })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");

    expect(within(dialog).getByDisplayValue("gpt-5-mini-codex")).toBeInTheDocument();
    expect(within(dialog).getAllByDisplayValue("等于 (==)")[1]).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_compare_mode: "equals",
      codex_reasoning_guard_reasoning_equals: [516, 1034, 1552],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      codex_reasoning_guard_model_rules: [
        {
          requested_model: "gpt-5-mini-codex",
          compare_mode: "equals",
          reasoning_equals: [256],
        },
      ],
    });
  });

  it("shows validation for invalid Codex reasoning guard values", () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
          exists: true,
          toml: 'approval_policy = "on-request"\\n',
        }}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "详情" }));
    const dialog = screen.getByRole("dialog");
    const input = within(dialog).getByDisplayValue("516, 1034, 1552") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "516, nope" } });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText("只支持非负整数，多个值请用逗号分隔。")).toBeInTheDocument();
  });

  it("renders unavailable state", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="unavailable"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={null}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );
    expect(screen.getByText("数据不可用")).toBeInTheDocument();
  });

  it("disables open config dir and shows hint when CODEX_HOME is overridden", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "/custom/codex",
          config_path: "/custom/codex/config.toml",
          can_open_config_dir: false,
        })}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(
      screen.getByText("受权限限制，无法自动打开该目录；请手动打开该路径。")
    ).toBeInTheDocument();
    const openBtn = screen.getByTitle("受权限限制，无法自动打开该目录");
    expect(openBtn).toBeDisabled();
  });

  it("saves a custom codex home override and normalizes config.toml input", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "手动指定目录" }));
    const customCard = (await screen.findByText("自定义 .codex 目录")).closest("div");
    expect(customCard).toBeTruthy();
    const input = within(customCard as HTMLElement).getByRole("textbox");
    fireEvent.change(input, { target: { value: "D:\\Work\\Codex\\config.toml" } });
    fireEvent.blur(input);

    expect(persistCodexHomeSettings).toHaveBeenCalledWith("custom", "D:\\Work\\Codex");
    expect(
      screen.getByText(
        "保存后将使用 D:\\Work\\Codex\\config.toml。支持普通 Windows 路径、UNC 路径，也可以点“选择目录”。"
      )
    ).toBeInTheDocument();
  });

  it("shows validation for invalid custom codex home input", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "手动指定目录" }));
    const customCard = (await screen.findByText("自定义 .codex 目录")).closest("div");
    expect(customCard).toBeTruthy();
    const input = within(customCard as HTMLElement).getByRole("textbox");
    fireEvent.change(input, { target: { value: "https://example.com/config.toml" } });
    fireEvent.blur(input);

    expect(persistCodexHomeSettings).not.toHaveBeenCalled();
    expect(screen.getByText("这里填写的是本地目录路径，不要包含协议头。")).toBeInTheDocument();
  });

  it("uses directory picker to switch into custom mode and persist", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(true);
    const pickCodexHomeDirectory = vi.fn().mockResolvedValue("D:\\Users\\MyPC\\.codex");

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "C:\\Users\\MyPC\\.codex",
          config_path: "C:\\Users\\MyPC\\.codex\\config.toml",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={pickCodexHomeDirectory}
      />
    );

    expect(screen.queryByRole("button", { name: "选择目录" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("手动指定目录"));
    fireEvent.click(await screen.findByRole("button", { name: "选择目录" }));

    expect(pickCodexHomeDirectory).toHaveBeenCalledWith("C:\\Users\\MyPC\\.codex");
    expect(await screen.findByDisplayValue("D:\\Users\\MyPC\\.codex")).toBeInTheDocument();
    expect(persistCodexHomeSettings).toHaveBeenCalledWith("custom", "D:\\Users\\MyPC\\.codex");
  });

  it("switches to follow CODEX_HOME mode and disables manual selection", () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(true);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          follow_codex_home_dir: "D:\\Workspace\\.codex",
          follow_codex_home_path: "D:\\Workspace\\.codex\\config.toml",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings({ codex_home_mode: "user_home_default" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "跟随环境变量 $CODEX_HOME" }));

    expect(persistCodexHomeSettings).toHaveBeenCalledWith("follow_codex_home", "");
    expect(screen.queryByRole("button", { name: "选择目录" })).not.toBeInTheDocument();
    expect(
      screen.getByText("当前为跟随模式，手动目录选择器已收起；现在会使用 D:\\Workspace\\.codex。")
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("当前路径跟随 $CODEX_HOME 解析；后续会随环境变量变化。").length
    ).toBeGreaterThan(0);
  });

  it("rolls back mode change when saving codex home settings fails", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(false);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          follow_codex_home_dir: "D:\\Workspace\\.codex",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings({ codex_home_mode: "user_home_default" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("radio", { name: "跟随环境变量 $CODEX_HOME" }));

    expect(persistCodexHomeSettings).toHaveBeenCalledWith("follow_codex_home", "");
    await screen.findByText(
      "当前为默认模式，手动目录选择器已收起；固定使用 C:\\Users\\MyPC\\.codex。"
    );
    expect(screen.getByRole("radio", { name: "固定到 Windows 用户目录" })).toBeChecked();
  });

  it("rolls back reset when saving the default codex home fails", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(false);

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "D:\\Work\\Saved\\.codex",
          config_path: "D:\\Work\\Saved\\.codex\\config.toml",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings({
          codex_home_mode: "custom",
          codex_home_override: "D:\\Work\\Saved\\.codex",
        })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "恢复默认" }));

    expect(persistCodexHomeSettings).toHaveBeenCalledWith("user_home_default", "");
    expect(await screen.findByDisplayValue("D:\\Work\\Saved\\.codex")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "手动指定目录" })).toBeChecked();
  });

  it("rolls back the picked custom codex home when saving fails", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(false);
    const pickCodexHomeDirectory = vi.fn().mockResolvedValue("D:\\Users\\MyPC\\.codex");

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={null}
        appSettings={createAppSettings({
          codex_home_mode: "custom",
          codex_home_override: "D:\\Work\\Saved\\.codex",
        })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        persistCodexHomeSettings={persistCodexHomeSettings}
        pickCodexHomeDirectory={pickCodexHomeDirectory}
      />
    );

    fireEvent.click(await screen.findByRole("button", { name: "选择目录" }));

    await waitFor(() =>
      expect(pickCodexHomeDirectory).toHaveBeenCalledWith("D:\\Work\\Saved\\.codex")
    );
    await waitFor(() =>
      expect(persistCodexHomeSettings).toHaveBeenCalledWith("custom", "D:\\Users\\MyPC\\.codex")
    );
    expect(await screen.findByDisplayValue("D:\\Work\\Saved\\.codex")).toBeInTheDocument();
  });

  it("labels the active directory card clearly in default mode", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "C:\\Users\\MyPC\\.codex",
          config_path: "C:\\Users\\MyPC\\.codex\\config.toml",
          follow_codex_home_dir: "D:\\Workspace\\.codex",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings({ codex_home_mode: "user_home_default" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(screen.getByText("当前 .codex 目录")).toBeInTheDocument();
    expect(
      screen.getAllByText("当前固定使用 Windows 用户目录下的 .codex。").length
    ).toBeGreaterThan(0);
    expect(
      screen.getByText("当前为默认模式，手动目录选择器已收起；固定使用 C:\\Users\\MyPC\\.codex。")
    ).toBeInTheDocument();
    expect(screen.queryByText("CODEX_HOME")).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText("例如：D:\\Users\\you\\.codex")).not.toBeInTheDocument();
  });

  it("shows follow mode as same-as-default when both resolve to the same path", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          user_home_default_dir: "C:\\Users\\MyPC\\.codex",
          follow_codex_home_dir: "C:\\Users\\MyPC\\.codex",
        })}
        codexConfigToml={null}
        appSettings={createAppSettings({ codex_home_mode: "user_home_default" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(
      screen.getByRole("radio", {
        name: "跟随环境变量 $CODEX_HOME（当前路径与固定目录一致）",
      })
    ).toBeInTheDocument();
    expect(screen.getByText("当前路径相同，但后续会随 $CODEX_HOME 变化。")).toBeInTheDocument();
  });

  it("treats service_tier=fast as enabled fast mode", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({ service_tier: "fast", features_fast_mode: false })}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    const fastModeItem = screen.getByText("fast_mode").parentElement?.parentElement;
    expect(fastModeItem).toBeTruthy();
    expect(within(fastModeItem as HTMLElement).getByRole("switch")).toHaveAttribute(
      "data-state",
      "checked"
    );
  });

  it("defaults personality to none when config is unset", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({ personality: null })}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    const personalityItem = screen.getByText("输出风格 (personality)").parentElement?.parentElement;
    expect(personalityItem).toBeTruthy();
    expect(
      within(personalityItem as HTMLElement).getByRole("radio", {
        name: "默认 / 删除配置 (none)",
      })
    ).toBeChecked();
  });

  it("shows gpt-5.4 linked settings and persists their defaults", () => {
    const persistCodexConfig = vi.fn();

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({ model: "gpt-5.4" })}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={persistCodexConfig}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(screen.getByText("model_context_window")).toBeInTheDocument();
    expect(screen.getByText("model_auto_compact_token_limit")).toBeInTheDocument();

    const modelItem = screen.getByText("默认模型 (model)").parentElement?.parentElement;
    expect(modelItem).toBeTruthy();
    const modelInput = within(modelItem as HTMLElement).getByRole("textbox");
    fireEvent.blur(modelInput);

    expect(persistCodexConfig).toHaveBeenCalledWith({
      model: "gpt-5.4",
      model_context_window: null,
      model_auto_compact_token_limit: null,
    });
  });

  it("persists null for gpt-5.4 linked settings when input is zero or cleared", () => {
    const persistCodexConfig = vi.fn();

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          model: "gpt-5.4",
          model_context_window: 1_000_000,
          model_auto_compact_token_limit: 900_000,
        })}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={persistCodexConfig}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    const contextItem = screen.getByText("model_context_window").parentElement?.parentElement;
    expect(contextItem).toBeTruthy();
    const contextInput = within(contextItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(contextInput, { target: { value: "0" } });
    fireEvent.blur(contextInput);
    expect(persistCodexConfig).toHaveBeenCalledWith({ model_context_window: null });

    const compactItem = screen.getByText("model_auto_compact_token_limit").parentElement
      ?.parentElement;
    expect(compactItem).toBeTruthy();
    const compactInput = within(compactItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(compactInput, { target: { value: "" } });
    fireEvent.blur(compactInput);
    expect(persistCodexConfig).toHaveBeenCalledWith({
      model_auto_compact_token_limit: null,
    });
  });

  it("resets toml draft when codex config path changes", async () => {
    vi.mocked(cliManagerCodexConfigTomlValidate).mockResolvedValue({
      ok: true,
      error: null,
    });

    const { rerender } = render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "C:\\Users\\MyPC\\.codex",
          config_path: "C:\\Users\\MyPC\\.codex\\config.toml",
        })}
        codexConfigToml={{
          config_path: "C:\\Users\\MyPC\\.codex\\config.toml",
          exists: true,
          toml: 'model = "gpt-5"\n',
        }}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(screen.getByText("高级配置（config.toml）"));
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    await screen.findByRole("button", { name: "取消" });
    fireEvent.change(await screen.findByLabelText("mock-code-editor"), {
      target: { value: 'model = "dirty-old"\n' },
    });

    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "dirty-old"\n');
    expect(screen.getByRole("button", { name: "取消" })).toBeInTheDocument();

    rerender(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({
          config_dir: "D:\\Work\\.codex",
          config_path: "D:\\Work\\.codex\\config.toml",
        })}
        codexConfigToml={{
          config_path: "D:\\Work\\.codex\\config.toml",
          exists: true,
          toml: 'model = "gpt-5.4"\n',
        }}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "gpt-5.4"\n');
    expect(screen.getByRole("button", { name: "编辑" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消" })).not.toBeInTheDocument();
  });
});
