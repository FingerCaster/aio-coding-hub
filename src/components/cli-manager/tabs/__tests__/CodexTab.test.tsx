import type { ComponentProps } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { confirm } from "@tauri-apps/plugin-dialog";
import { cliManagerCodexConfigTomlValidate } from "../../../../services/cli/cliManager";
import { openDesktopUrl } from "../../../../services/desktop/opener";
import { CliManagerCodexTab } from "../CodexTab";
import { createTestAppSettings } from "../../../../test/fixtures/settings";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn(),
}));

vi.mock("../../../../utils/platform", () => ({
  isWindowsRuntime: () => true,
}));

vi.mock("../../../../ui/CodeEditor", () => ({
  CodeEditor: ({ value, onChange, readOnly }: any) => (
    <textarea
      aria-label="mock-code-editor"
      value={value}
      readOnly={readOnly}
      onChange={(event) => onChange?.(event.currentTarget.value)}
    />
  ),
}));

vi.mock("../../../../services/desktop/opener", () => ({
  openDesktopUrl: vi.fn().mockResolvedValue(true),
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
    approvals_reviewer: null,
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

function createCodexModel(overrides: Partial<any> = {}) {
  return {
    id: "gpt-5.6-sol-id",
    model: "gpt-5.6-sol",
    display_name: "GPT-5.6 Sol",
    hidden: false,
    is_default: false,
    supported_reasoning_efforts: [
      { reasoning_effort: "low", description: null },
      { reasoning_effort: "medium", description: null },
      { reasoning_effort: "high", description: null },
      { reasoning_effort: "xhigh", description: null },
      { reasoning_effort: "max", description: "Maximum reasoning depth" },
      { reasoning_effort: "ultra", description: "Automatic task delegation" },
    ],
    default_reasoning_effort: "medium",
    ...overrides,
  };
}

function createCodexModelCatalog(models = [createCodexModel()]) {
  return {
    status: "ready" as const,
    issue: null,
    snapshot: {
      config_path: "/home/user/.codex/config.toml",
      executable_path: "/bin/codex",
      cli_version: "0.0.0",
    },
    models,
  };
}

function renderTab(overrides: Partial<ComponentProps<typeof CliManagerCodexTab>> = {}) {
  return render(
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
      refreshCodex={vi.fn()}
      openCodexConfigDir={vi.fn()}
      persistCodexConfig={vi.fn()}
      persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      {...overrides}
    />
  );
}

function renderApprovalReviewerSettings({
  reviewer = null,
  policy = "on-request",
  saving = false,
  persistCodexConfig = vi.fn().mockResolvedValue(null),
}: {
  reviewer?: string | null;
  policy?: string | null;
  saving?: boolean;
  persistCodexConfig?: ReturnType<typeof vi.fn>;
} = {}) {
  renderTab({
    codexConfigSaving: saving,
    persistCodexConfig,
    codexConfig: createCodexConfig({
      approvals_reviewer: reviewer,
      approval_policy: policy,
    }),
  });

  return persistCodexConfig;
}

describe("components/cli-manager/tabs/CodexTab", () => {
  beforeEach(() => {
    vi.mocked(confirm).mockReset();
    vi.mocked(openDesktopUrl).mockReset();
    vi.mocked(openDesktopUrl).mockResolvedValue(true);
    vi.mocked(cliManagerCodexConfigTomlValidate).mockResolvedValue({
      ok: true,
      error: null,
    });
  });

  it("renders only supported Codex model reasoning effort options", () => {
    renderTab({
      codexConfig: createCodexConfig({ model_reasoning_effort: "ultra" }),
    });

    expect(screen.queryByRole("radio", { name: "最低 (minimal)" })).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "最大深度 (max)" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "自动委派 (ultra)" })).toBeChecked();
  });

  it("handles sandbox confirm flow and persists related config controls", async () => {
    const persistCodexConfig = vi.fn();
    const refreshCodex = vi.fn();

    vi.mocked(confirm).mockResolvedValueOnce(false).mockResolvedValueOnce(true);

    renderTab({
      refreshCodex,
      persistCodexConfig,
      codexConfigToml: {
        config_path: "/home/user/.codex/config.toml",
        exists: true,
        toml: 'approval_policy = "on-request"\n',
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    expect(refreshCodex).toHaveBeenCalled();

    const sandboxItem = screen.getByText("沙箱模式 (sandbox_mode)").parentElement?.parentElement;
    expect(sandboxItem).toBeTruthy();
    const sandboxSelect = within(sandboxItem as HTMLElement).getByRole("combobox");

    fireEvent.change(sandboxSelect, { target: { value: "danger-full-access" } });
    await waitFor(() => {
      expect(confirm).toHaveBeenCalledTimes(1);
      expect((sandboxSelect as HTMLSelectElement).value).toBe("workspace-write");
    });
    expect(persistCodexConfig).not.toHaveBeenCalledWith(
      expect.objectContaining({ sandbox_mode: "danger-full-access" })
    );

    fireEvent.change(sandboxSelect, { target: { value: "danger-full-access" } });
    await waitFor(() => {
      expect(confirm).toHaveBeenCalledTimes(2);
      expect(persistCodexConfig).toHaveBeenCalledWith({ sandbox_mode: "danger-full-access" });
    });

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

    fireEvent.click(screen.getByRole("radio", { name: "禁用 (disabled)" }));
    expect(persistCodexConfig).toHaveBeenCalledWith({ web_search: "disabled" });
  });

  it("renders and persists approvals reviewer choices without confirmation", () => {
    const persistCodexConfig = renderApprovalReviewerSettings();
    const reviewerSelect = screen.getByRole("combobox", {
      name: "审批者 (approvals_reviewer)",
    });

    expect(
      within(reviewerSelect)
        .getAllByRole("option")
        .map((option) => option.textContent)
    ).toEqual(["默认（不设置）", "由我审批（user）", "替我审批（auto_review）"]);

    fireEvent.change(reviewerSelect, { target: { value: "auto_review" } });
    fireEvent.change(reviewerSelect, { target: { value: "user" } });
    fireEvent.change(reviewerSelect, { target: { value: "" } });

    expect(persistCodexConfig).toHaveBeenNthCalledWith(1, {
      approvals_reviewer: "auto_review",
    });
    expect(persistCodexConfig).toHaveBeenNthCalledWith(2, { approvals_reviewer: "user" });
    expect(persistCodexConfig).toHaveBeenNthCalledWith(3, { approvals_reviewer: "" });
    expect(confirm).not.toHaveBeenCalled();
  });

  it("renders an unknown reviewer verbatim and keeps it out of policy patches", () => {
    const persistCodexConfig = renderApprovalReviewerSettings({
      reviewer: "future_reviewer",
      policy: "on-request",
    });

    const reviewerSelect = screen.getByRole("combobox", {
      name: "审批者 (approvals_reviewer)",
    });

    expect(reviewerSelect).toHaveValue("future_reviewer");
    expect(
      within(reviewerSelect).getByRole("option", {
        name: "不支持的当前值（future_reviewer）",
      })
    ).toBeInTheDocument();

    const approvalItem =
      screen.getByText("审批策略 (approval_policy)").parentElement?.parentElement;
    const approvalSelect = within(approvalItem as HTMLElement).getByRole("combobox");
    fireEvent.change(approvalSelect, { target: { value: "never" } });

    expect(persistCodexConfig).toHaveBeenCalledWith({ approval_policy: "never" });
    expect(persistCodexConfig).not.toHaveBeenCalledWith(
      expect.objectContaining({ approvals_reviewer: expect.anything() })
    );
  });

  it.each([
    ["auto_review", "never", "替我审批不会生效"],
    ["auto_review", "untrusted", "不支持 auto-review"],
    ["auto_review", "on-failure", "不支持 auto-review"],
    ["user", "never", "不会产生需要你处理的审批请求"],
  ])("offers an explicit policy switch for reviewer=%s policy=%s", (reviewer, policy, copy) => {
    const persistCodexConfig = renderApprovalReviewerSettings({ reviewer, policy });

    expect(screen.getByText(new RegExp(copy))).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "切换为 on-request" }));
    expect(persistCodexConfig).toHaveBeenCalledWith({ approval_policy: "on-request" });
  });

  it("uses neutral inherited-policy copy and disables the policy switch while saving", () => {
    renderApprovalReviewerSettings({
      reviewer: "auto_review",
      policy: "never",
      saving: true,
    });

    expect(screen.getByRole("button", { name: "切换为 on-request" })).toBeDisabled();
  });

  it("toggles Codex OAuth compatible proxy mode from app settings", () => {
    const persistCodexOauthCompatibleProxyMode = vi.fn().mockResolvedValue(true);

    renderTab({
      codexConfigToml: {
        config_path: "/home/user/.codex/config.toml",
        exists: true,
        toml: 'approval_policy = "on-request"\n',
      },
      appSettings: createAppSettings({ codex_oauth_compatible_proxy_mode: false }),
      persistCodexOauthCompatibleProxyMode,
    });

    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex OAuth 兼容代理模式" }));
    expect(persistCodexOauthCompatibleProxyMode).toHaveBeenCalledWith(true);
  });

  it("persists the global provider test model and supports manual Provider Sync", async () => {
    const persistCommonSettings = vi
      .fn()
      .mockResolvedValueOnce(createAppSettings({ codex_provider_test_model: "gpt-5.4" }))
      .mockResolvedValueOnce(createAppSettings({ codex_provider_test_model: "gpt-5.4-mini" }));
    const syncCodexProvider = vi.fn().mockResolvedValue(undefined);

    renderTab({
      codexConfigToml: {
        config_path: "/home/user/.codex/config.toml",
        exists: true,
        toml: 'approval_policy = "on-request"\n',
      },
      appSettings: createAppSettings({ codex_provider_test_model: "gpt-5-codex" }),
      persistCommonSettings,
      syncCodexProvider,
    });

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

    fireEvent.click(screen.getByRole("button", { name: "手动 Provider Sync" }));
    expect(syncCodexProvider).toHaveBeenCalledTimes(1);
  });

  it("disables provider sync while codex saving or syncing", () => {
    renderTab({
      codexConfigSaving: true,
      codexProviderSyncing: true,
      syncCodexProvider: vi.fn(),
    });

    expect(screen.getByRole("button", { name: "同步中…" })).toBeDisabled();
  });

  it("does not render legacy Codex reasoning guard controls or statistics", () => {
    renderTab();

    expect(screen.queryByRole("switch", { name: "切换 Codex 降智拦截" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "查看降智拦截详情" })).not.toBeInTheDocument();
    expect(screen.queryByText("命中请求数")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("降智拦截统计时间范围")).not.toBeInTheDocument();
  });

  it("renders the retry gateway recommendation and opens the official repository", () => {
    renderTab();

    expect(screen.getByText("降智拦截网关推荐")).toBeInTheDocument();
    expect(screen.getByText("nonononull/codex-retry-gateway")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "查看仓库" }));
    expect(openDesktopUrl).toHaveBeenCalledWith(
      "https://github.com/nonononull/codex-retry-gateway"
    );
  });

  it("renders unavailable state and keeps a loaded config editable when CLI is unavailable", () => {
    renderTab({
      codexAvailable: "unavailable",
      codexConfig: null,
      codexInfo: createCodexInfo({ found: false, executable_path: null, version: null }),
    });
    expect(screen.getByText("数据不可用")).toBeInTheDocument();

    const persistCodexConfig = vi.fn();
    renderTab({
      codexAvailable: "unavailable",
      persistCodexConfig,
      codexInfo: createCodexInfo({
        found: false,
        executable_path: null,
        version: null,
      }),
    });

    const contextItem = screen.getByText("model_context_window").parentElement?.parentElement;
    expect(contextItem).toBeTruthy();
    const contextInput = within(contextItem as HTMLElement).getByRole("spinbutton");
    expect(contextInput).toBeEnabled();
    fireEvent.change(contextInput, { target: { value: "1000000" } });
    fireEvent.blur(contextInput);
    expect(persistCodexConfig).toHaveBeenCalledWith({ model_context_window: 1_000_000 });
  });

  it("disables open config dir and shows the correct hint when opening is unavailable", () => {
    renderTab({
      codexConfig: createCodexConfig({
        config_dir: "/custom/codex",
        config_path: "/custom/codex/config.toml",
        can_open_config_dir: false,
      }),
    });

    expect(screen.getByTitle("受权限限制，无法自动打开该目录")).toBeDisabled();
  });

  it("persists custom Codex home settings, validates input, and supports the picker", async () => {
    const persistCodexHomeSettings = vi.fn().mockResolvedValue(true);
    const pickCodexHomeDirectory = vi.fn().mockResolvedValue("D:\\Users\\MyPC\\.codex");

    renderTab({
      appSettings: createAppSettings(),
      persistCodexHomeSettings,
      pickCodexHomeDirectory,
    });

    fireEvent.click(screen.getByRole("radio", { name: "手动指定目录" }));
    const customCard = (await screen.findByText("自定义 .codex 目录")).closest("div");
    expect(customCard).toBeTruthy();
    const input = within(customCard as HTMLElement).getByRole("textbox");

    fireEvent.change(input, { target: { value: "D:\\Work\\Codex\\config.toml" } });
    fireEvent.blur(input);
    expect(persistCodexHomeSettings).toHaveBeenCalledWith("custom", "D:\\Work\\Codex");

    fireEvent.change(input, { target: { value: "https://example.com/config.toml" } });
    fireEvent.blur(input);
    expect(screen.getByText("这里填写的是本地目录路径，不要包含协议头。")).toBeInTheDocument();

    fireEvent.click(await screen.findByRole("button", { name: "选择目录" }));
    expect(pickCodexHomeDirectory).toHaveBeenCalled();
    await waitFor(() =>
      expect(persistCodexHomeSettings).toHaveBeenCalledWith("custom", "D:\\Users\\MyPC\\.codex")
    );
  });

  it("switches to follow CODEX_HOME mode", () => {
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
    expect(persistCodexHomeSettings).toHaveBeenNthCalledWith(1, "follow_codex_home", "");
    expect(
      screen.getByText("当前为跟随模式，手动目录选择器已收起；现在会使用 D:\\Workspace\\.codex。")
    ).toBeInTheDocument();
  });

  it("renders directory state copy for default and same-as-default follow modes", () => {
    renderTab({
      codexConfig: createCodexConfig({
        config_dir: "C:\\Users\\MyPC\\.codex",
        config_path: "C:\\Users\\MyPC\\.codex\\config.toml",
        follow_codex_home_dir: "D:\\Workspace\\.codex",
      }),
      appSettings: createAppSettings({ codex_home_mode: "user_home_default" }),
    });

    expect(screen.getByText("当前 .codex 目录")).toBeInTheDocument();
    expect(
      screen.getByText("当前为默认模式，手动目录选择器已收起；固定使用 C:\\Users\\MyPC\\.codex。")
    ).toBeInTheDocument();

    renderTab({
      codexConfig: createCodexConfig({
        user_home_default_dir: "C:\\Users\\MyPC\\.codex",
        follow_codex_home_dir: "C:\\Users\\MyPC\\.codex",
      }),
      appSettings: createAppSettings({ codex_home_mode: "user_home_default" }),
    });

    expect(
      screen.getByRole("radio", {
        name: "跟随环境变量 $CODEX_HOME（当前路径与固定目录一致）",
      })
    ).toBeInTheDocument();
    expect(screen.getByText("当前路径相同，但后续会随 $CODEX_HOME 变化。")).toBeInTheDocument();
  });

  it("treats service_tier=fast as enabled fast mode and defaults personality to none", () => {
    renderTab({
      codexConfig: createCodexConfig({ service_tier: "fast", features_fast_mode: false }),
    });

    const fastModeItem = screen.getByText("fast_mode").parentElement?.parentElement;
    expect(fastModeItem).toBeTruthy();
    expect(within(fastModeItem as HTMLElement).getByRole("switch")).toHaveAttribute(
      "data-state",
      "checked"
    );

    const personalityItem = screen.getByText("输出风格 (personality)").parentElement?.parentElement;
    expect(personalityItem).toBeTruthy();
    expect(
      within(personalityItem as HTMLElement).getByRole("radio", {
        name: "默认 / 删除配置 (none)",
      })
    ).toBeChecked();
  });

  it("shows model token override inputs and persists null when zero or cleared", () => {
    const persistCodexConfig = vi.fn();

    renderTab({
      persistCodexConfig,
      codexConfig: createCodexConfig({
        model: "gpt-5.6-sol",
        model_context_window: 1_000_000,
        model_auto_compact_token_limit: 900_000,
      }),
    });

    expect(screen.getByText("model_context_window")).toBeInTheDocument();
    expect(screen.getByText("model_auto_compact_token_limit")).toBeInTheDocument();

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

  it("uses catalog efforts for normal mode and keeps max/ultra out of plan mode", () => {
    const persistCodexConfig = vi.fn();

    renderTab({
      persistCodexConfig,
      codexConfig: createCodexConfig({ model: "gpt-5.6-sol", features_multi_agent: null }),
      codexModelCatalog: createCodexModelCatalog(),
    });

    const reasoningGroup = screen.getByRole("radiogroup", {
      name: "推理强度 (model_reasoning_effort)",
    });
    expect(reasoningGroup).toHaveAccessibleDescription(
      "调整推理强度（仅对支持的模型/Responses API 生效）。值越高通常越稳健但更慢。"
    );

    const ultraOption = within(reasoningGroup).getByRole("radio", {
      name: "自动委派 (ultra)",
    });
    fireEvent.click(ultraOption);
    expect(persistCodexConfig).toHaveBeenCalledWith({ model_reasoning_effort: "ultra" });

    const planItem = screen.getByText("计划模式推理强度 (plan_mode_reasoning_effort)").parentElement
      ?.parentElement;
    expect(planItem).toBeTruthy();
    expect(
      within(planItem as HTMLElement).queryByRole("radio", { name: /最大深度 \(max\)/ })
    ).toBeNull();
    expect(
      within(planItem as HTMLElement).queryByRole("radio", { name: /自动委派 \(ultra\)/ })
    ).toBeNull();
  });

  it("keeps the reasoning control editable when the catalog query fails", () => {
    const refreshCodex = vi.fn().mockResolvedValue(undefined);

    renderTab({
      codexModelCatalogError: true,
      refreshCodex,
    });

    expect(screen.getByText("读取模型能力失败，当前推理选项仅供编辑。")).toBeInTheDocument();
    const reasoningItem = screen.getByText("推理强度 (model_reasoning_effort)").parentElement
      ?.parentElement;
    expect(reasoningItem).toBeTruthy();
    expect(
      within(reasoningItem as HTMLElement).getByRole("radio", { name: "低 (low)" })
    ).toBeEnabled();
    fireEvent.click(
      within(reasoningItem as HTMLElement).getByRole("button", { name: "重试能力目录" })
    );
    expect(refreshCodex).toHaveBeenCalledTimes(1);
  });

  it("resets the TOML draft on config path changes and surfaces validation errors", async () => {
    const persistCodexConfigToml = vi.fn();

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
        persistCodexConfigToml={persistCodexConfigToml}
      />
    );

    fireEvent.click(screen.getByText("高级配置（config.toml）"));
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.change(await screen.findByLabelText("mock-code-editor"), {
      target: { value: 'model = "dirty-old"\n' },
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
        persistCodexConfigToml={persistCodexConfigToml}
      />
    );

    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "gpt-5.4"\n');
    expect(screen.getByRole("button", { name: "编辑" })).toBeInTheDocument();

    vi.mocked(cliManagerCodexConfigTomlValidate)
      .mockResolvedValueOnce({
        ok: false,
        error: { message: "invalid toml", line: 2, column: 3 },
      })
      .mockResolvedValue({ ok: true, error: null });

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByLabelText("mock-code-editor"), {
      target: { value: "bad = [" },
    });
    const details = screen.getByText("高级配置（config.toml）").closest("details");
    expect(details).toBeTruthy();
    const saveButton = () => within(details as HTMLElement).getAllByRole("button")[2];
    fireEvent.click(saveButton());

    expect(await screen.findByText("TOML 校验失败")).toBeInTheDocument();
    expect(screen.getByText("invalid toml")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "gpt-5.4"\n');
  });

  it("renders loading, missing config, fallback info, and detection error states", async () => {
    const refreshCodex = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <CliManagerCodexTab
        codexAvailable="checking"
        codexLoading={true}
        codexConfigLoading={true}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={null}
        codexConfig={null}
        codexConfigToml={null}
        refreshCodex={refreshCodex}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(screen.getByText("加载中...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新" })).toBeDisabled();
    expect(screen.getByText("暂无配置，请尝试刷新")).toBeInTheDocument();

    rerender(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo({
          found: false,
          version: null,
          executable_path: null,
          resolved_via: null,
          shell: null,
          error: "codex boom",
        })}
        codexConfig={createCodexConfig({
          exists: false,
          executable_path: undefined,
          resolved_via: undefined,
          config_dir: "",
          config_path: "",
          user_home_default_dir: "",
          follow_codex_home_dir: "",
          approval_policy: null,
          sandbox_mode: null,
          model: null,
          model_reasoning_effort: null,
          plan_mode_reasoning_effort: null,
          web_search: null,
          personality: "  ",
        })}
        codexConfigToml={null}
        refreshCodex={refreshCodex}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
      />
    );

    expect(screen.getByText("未检测到")).toBeInTheDocument();
    expect(screen.getByText("不存在（将自动创建）")).toBeInTheDocument();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    expect(screen.getByText("检测失败：")).toBeInTheDocument();
    expect(screen.getByText("codex boom")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    await waitFor(() => expect(refreshCodex).toHaveBeenCalled());
  });
});
