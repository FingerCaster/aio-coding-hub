import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cliManagerCodexConfigTomlValidate } from "../../../../services/cli/cliManager";
import { useCliManagerCodexReasoningGuardStatsQuery } from "../../../../query/cliManager";
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
vi.mock("../../../../query/cliManager", async () => {
  const actual = await vi.importActual<typeof import("../../../../query/cliManager")>(
    "../../../../query/cliManager"
  );
  return {
    ...actual,
    useCliManagerCodexReasoningGuardStatsQuery: vi.fn(),
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
    token_hit_attempt_count: 7,
    feature_hit_attempt_count: 2,
    reasoning_token_hit_request_count: 3,
    final_answer_only_high_xhigh_hit_request_count: 1,
    normal_request_count: 28,
    total_request_count: 32,
    hit_rate: 0.125,
    feature_sample_request_count: 6,
    feature_sample_count: 8,
    final_answer_only_sample_count: 5,
    high_xhigh_final_answer_only_sample_count: 3,
    reasoning_516_final_answer_only_no_commentary_count: 2,
    compaction_exempt_sample_count: 1,
    reasoning_tokens_coverage_count: 7,
    final_answer_only_coverage_count: 6,
    commentary_observed_coverage_count: 6,
    reasoning_effort_coverage_count: 8,
    duration_ms_coverage_count: 8,
    output_tokens_coverage_count: 4,
    continuation_triggered_request_count: 5,
    continuation_triggered_attempt_count: 6,
    continuation_repaired_request_count: 3,
    continuation_repaired_attempt_count: 3,
    continuation_non_repaired_attempt_count: 3,
    continuation_repair_rate: 0.6,
    continuation_average_sent_rounds: 1.5,
    continuation_by_status: [
      {
        status: "repaired",
        request_count: 3,
        attempt_count: 3,
        average_sent_rounds: 1,
      },
      {
        status: "still_matched",
        request_count: 2,
        attempt_count: 2,
        average_sent_rounds: 3,
      },
    ],
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
    by_model_and_effort: [
      {
        requested_model: "gpt-5-codex",
        reasoning_effort: "high",
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
  const defaultCodexReasoningGuardRetrySettings = {
    codex_reasoning_guard_rule_mode: "reasoning_tokens",
    codex_reasoning_guard_active_template_id: "builtin-legacy-reasoning-tokens",
    codex_reasoning_guard_custom_templates: [],
    codex_reasoning_guard_retry_policy: "single",
    codex_reasoning_guard_concurrent_max: 5,
    codex_reasoning_guard_concurrent_interval_ms: 1000,
    codex_reasoning_guard_concurrent_max_attempts: 10,
    codex_reasoning_guard_model_fallbacks: [],
  } as const;

  const mockReasoningGuardStatsQuery = vi.mocked(useCliManagerCodexReasoningGuardStatsQuery);
  let reasoningGuardStatsRefetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    reasoningGuardStatsRefetch = vi.fn();
    mockReasoningGuardStatsQuery.mockReturnValue({
      data: createReasoningGuardStats(),
      isFetching: false,
      refetch: reasoningGuardStatsRefetch,
    } as any);
  });

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
    const syncCodexProvider = vi.fn().mockResolvedValue(undefined);

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
        syncCodexProvider={syncCodexProvider}
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

    fireEvent.click(screen.getByRole("button", { name: "手动 Provider Sync" }));
    expect(syncCodexProvider).toHaveBeenCalledTimes(1);
  });

  it("disables provider sync while codex saving or syncing", () => {
    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={true}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexProviderSyncing={true}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig()}
        codexConfigToml={null}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(false)}
        syncCodexProvider={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "同步中…" })).toBeDisabled();
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
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    expect(screen.getByText("命中请求数")).toBeInTheDocument();
    expect(screen.getByText("时间范围:")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "本次应用打开后" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "全部统计" })).not.toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("12.5%")).toBeInTheDocument();
    expect(screen.getByText("补救触发数")).toBeInTheDocument();
    expect(screen.getByText("平均续写轮数")).toBeInTheDocument();
    expect(screen.getByText("60.0%")).toBeInTheDocument();
    expect(screen.getByText(/优先级：续写补救会先尝试修复/)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "切换 Codex 继续思考补救" })).not.toBeDisabled();

    fireEvent.click(
      within(screen.getByLabelText("降智拦截统计时间范围")).getByRole("button", { name: "刷新" })
    );
    expect(reasoningGuardStatsRefetch).toHaveBeenCalled();

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

  it("applies custom date range for Codex reasoning guard stats", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(
      within(screen.getByLabelText("降智拦截统计时间范围")).getByRole("button", { name: /当天/ })
    );
    expect(screen.getByRole("button", { name: "今天" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "昨天" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "近24小时" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "近7天" })).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("降智拦截统计开始日期"), {
      target: { value: "2026-06-28" },
    });
    fireEvent.change(screen.getByLabelText("降智拦截统计结束日期"), {
      target: { value: "2026-06-30" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));

    expect(mockReasoningGuardStatsQuery).toHaveBeenCalledWith(
      {
        startCreatedAtMs: new Date(2026, 5, 28, 0, 0, 0, 0).getTime(),
        endCreatedAtMs: new Date(2026, 6, 1, 0, 0, 0, 0).getTime(),
      },
      { enabled: true }
    );
    expect(
      within(screen.getByLabelText("降智拦截统计时间范围")).getByRole("button", {
        name: /2026-06-28 至 2026-06-30/,
      })
    ).toBeInTheDocument();
  });

  it("keeps Codex reasoning guard and continuation stats date ranges independent", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    const guardRangeControls = screen.getByLabelText("降智拦截统计时间范围");
    const continuationRangeControls = screen.getByLabelText("继续思考补救统计时间范围");

    fireEvent.click(within(guardRangeControls).getByRole("button", { name: /当天/ }));
    fireEvent.change(screen.getByLabelText("降智拦截统计开始日期"), {
      target: { value: "2026-06-28" },
    });
    fireEvent.change(screen.getByLabelText("降智拦截统计结束日期"), {
      target: { value: "2026-06-30" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));

    expect(
      within(guardRangeControls).getByRole("button", { name: /2026-06-28 至 2026-06-30/ })
    ).toBeInTheDocument();
    expect(
      within(continuationRangeControls).getByRole("button", { name: /当天/ })
    ).toBeInTheDocument();

    fireEvent.click(within(continuationRangeControls).getByRole("button", { name: /当天/ }));
    fireEvent.change(screen.getByLabelText("继续思考补救统计开始日期"), {
      target: { value: "2026-07-01" },
    });
    fireEvent.change(screen.getByLabelText("继续思考补救统计结束日期"), {
      target: { value: "2026-07-02" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));

    expect(
      within(guardRangeControls).getByRole("button", { name: /2026-06-28 至 2026-06-30/ })
    ).toBeInTheDocument();
    expect(
      within(continuationRangeControls).getByRole("button", {
        name: /2026-07-01 至 2026-07-02/,
      })
    ).toBeInTheDocument();
  });

  it("allows changing Codex reasoning guard stats date range inside detail dialog", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: /当天/ }));

    fireEvent.change(within(dialog).getByLabelText("降智拦截统计开始日期"), {
      target: { value: "2026-06-20" },
    });
    fireEvent.change(within(dialog).getByLabelText("降智拦截统计结束日期"), {
      target: { value: "2026-06-22" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "应用" }));

    expect(mockReasoningGuardStatsQuery).toHaveBeenCalledWith(
      {
        startCreatedAtMs: new Date(2026, 5, 20, 0, 0, 0, 0).getTime(),
        endCreatedAtMs: new Date(2026, 5, 23, 0, 0, 0, 0).getTime(),
      },
      { enabled: true }
    );
    expect(
      within(dialog).getByRole("button", { name: /2026-06-20 至 2026-06-22/ })
    ).toBeInTheDocument();
  });

  it("allows changing Codex continuation repair stats date range inside detail dialog", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看继续思考补救详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: /当天/ }));

    fireEvent.change(within(dialog).getByLabelText("继续思考补救统计开始日期"), {
      target: { value: "2026-06-23" },
    });
    fireEvent.change(within(dialog).getByLabelText("继续思考补救统计结束日期"), {
      target: { value: "2026-06-25" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "应用" }));

    expect(mockReasoningGuardStatsQuery).toHaveBeenCalledWith(
      {
        startCreatedAtMs: new Date(2026, 5, 23, 0, 0, 0, 0).getTime(),
        endCreatedAtMs: new Date(2026, 5, 26, 0, 0, 0, 0).getTime(),
      },
      { enabled: true }
    );
    expect(
      within(dialog).getByRole("button", { name: /2026-06-23 至 2026-06-25/ })
    ).toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("降智命中标签"), {
      target: { value: "守卫命中" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_hit_label: "守卫命中",
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      ...defaultCodexReasoningGuardRetrySettings,
    });
  });

  it("saves Codex reasoning guard final-answer-only rule mode", () => {
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
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("规则模板"), {
      target: { value: "builtin-final-answer-only-high-xhigh" },
    });
    expect(
      within(dialog).getByText(
        "请求 reasoning effort 为 high/xhigh 且响应只有 final answer 时命中；仅 reasoning_tokens 为 0 的 context_compaction 豁免。"
      )
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        codex_reasoning_guard_rule_mode: "final_answer_only_high_xhigh",
        codex_reasoning_guard_active_template_id: "builtin-final-answer-only-high-xhigh",
      })
    );
  });

  it("copies and saves a custom Codex reasoning guard rule template", () => {
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
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "复制为自定义" }));

    fireEvent.change(within(dialog).getByLabelText("模板名称"), {
      target: { value: "Custom token template" },
    });
    fireEvent.change(within(dialog).getAllByLabelText("规则 ID")[0], {
      target: { value: "token-777" },
    });
    fireEvent.change(within(dialog).getAllByLabelText("规则名称")[0], {
      target: { value: "reasoning_tokens == 777" },
    });
    fireEvent.change(within(dialog).getAllByLabelText("token 匹配")[0], {
      target: { value: "777" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        codex_reasoning_guard_active_template_id: "custom-builtin-legacy-reasoning-tokens",
        codex_reasoning_guard_custom_templates: [
          expect.objectContaining({
            id: "custom-builtin-legacy-reasoning-tokens",
            name: "Custom token template",
            rules: expect.arrayContaining([
              expect.objectContaining({
                id: "token-777",
                reasoning_tokens: 777,
                action: "intercept",
              }),
            ]),
          }),
        ],
      })
    );
  });

  it("rejects invalid Codex reasoning guard template form before saving", () => {
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
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "复制为自定义" }));
    fireEvent.change(within(dialog).getByLabelText("模板名称"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText("Codex 降智拦截模板 1名称不能为空")).toBeInTheDocument();
  });

  it("rejects nonnumeric Codex reasoning guard token input before saving", () => {
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
        codexConfigToml={null}
        appSettings={createAppSettings()}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "复制为自定义" }));
    fireEvent.change(within(dialog).getAllByLabelText("token 匹配")[0], {
      target: { value: "abc" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText(/token 匹配必须是 0 到 .*之间的整数/)).toBeInTheDocument();
  });

  it("copies final-answer-only template with the zero reasoning token exemption", () => {
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
          codex_reasoning_guard_rule_mode: "final_answer_only_high_xhigh",
          codex_reasoning_guard_active_template_id: "builtin-final-answer-only-high-xhigh",
        })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={vi.fn().mockResolvedValue(true)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "复制为自定义" }));
    const tokenInputs = within(dialog).getAllByLabelText("token 匹配") as HTMLInputElement[];
    const actionSelects = within(dialog).getAllByLabelText("动作") as HTMLSelectElement[];

    expect(tokenInputs[0]).toHaveValue("0");
    expect(actionSelects[0].value).toBe("no_intercept");
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
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
      codex_reasoning_guard_hit_label: "降智命中",
      codex_reasoning_guard_immediate_retry_budget: 4,
      codex_reasoning_guard_delayed_retry_budget: 3,
      codex_reasoning_guard_delayed_retry_ms: 1500,
      codex_reasoning_guard_exhausted_action: "switch_provider",
      ...defaultCodexReasoningGuardRetrySettings,
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("立即重试次数"), {
      target: { value: "101" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText("立即重试预算必须在 0 到 100 之间。")).toBeInTheDocument();
  });

  it("saves Codex reasoning guard continuation repair settings", () => {
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

    expect(screen.getByText("继续思考补救")).toBeInTheDocument();
    expect(screen.getByText(/独立于降智拦截开关生效/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex 继续思考补救" }));
    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_continuation_repair_enabled: true,
    });

    fireEvent.change(screen.getByLabelText("最大续写轮数"), {
      target: { value: "4" },
    });
    fireEvent.change(screen.getByLabelText("最大 output tokens"), {
      target: { value: "12000" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存补救" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_continuation_repair_enabled: true,
      codex_reasoning_guard_continuation_max_rounds: 4,
      codex_reasoning_guard_continuation_max_output_tokens: 12000,
    });
  });

  it("renders saved Codex continuation repair state without using unsaved cap drafts", () => {
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
        appSettings={createAppSettings({
          codex_reasoning_guard_continuation_repair_enabled: true,
          codex_reasoning_guard_continuation_max_rounds: 4,
          codex_reasoning_guard_continuation_max_output_tokens: 12000,
        })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    expect(screen.getByRole("switch", { name: "切换 Codex 继续思考补救" })).toBeChecked();
    expect(screen.getByText("on")).toBeInTheDocument();
    expect(screen.getByText("rounds=4 / output=12000")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("最大续写轮数"), {
      target: { value: "8" },
    });
    fireEvent.change(screen.getByLabelText("最大 output tokens"), {
      target: { value: "24000" },
    });

    expect(screen.getByText("rounds=4 / output=12000")).toBeInTheDocument();
  });

  it("keeps unsaved continuation caps when the continuation toggle save fails", async () => {
    const persistCodexReasoningGuardSettings = vi.fn().mockResolvedValue(false);

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

    fireEvent.change(screen.getByLabelText("最大续写轮数"), {
      target: { value: "4" },
    });
    fireEvent.change(screen.getByLabelText("最大 output tokens"), {
      target: { value: "12000" },
    });
    fireEvent.click(screen.getByRole("switch", { name: "切换 Codex 继续思考补救" }));

    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "切换 Codex 继续思考补救" })).not.toBeChecked()
    );
    expect(screen.getByLabelText("最大续写轮数")).toHaveValue(4);
    expect(screen.getByLabelText("最大 output tokens")).toHaveValue(12000);
  });

  it("renders unknown Codex continuation status as localized text", () => {
    mockReasoningGuardStatsQuery.mockReturnValue({
      data: createReasoningGuardStats({
        continuation_by_status: [
          {
            status: "unknown",
            request_count: 2,
            attempt_count: 9,
            average_sent_rounds: 0,
          },
        ],
      }),
      isFetching: false,
      refetch: reasoningGuardStatsRefetch,
    } as any);

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
        persistCodexReasoningGuardSettings={vi.fn().mockResolvedValue(true)}
      />
    );

    expect(screen.getByText("未知状态 · 9")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "查看继续思考补救详情" }));
    expect(within(screen.getByRole("dialog")).getByText("未知状态")).toBeInTheDocument();
  });

  it("rejects invalid Codex reasoning guard continuation caps before saving", () => {
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

    fireEvent.change(screen.getByLabelText("最大续写轮数"), {
      target: { value: "0" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存补救" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText("继续思考最大轮数必须在 1 到 10 之间。")).toBeInTheDocument();
  });

  it("does not validate continuation caps when saving Codex reasoning guard rules", () => {
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

    fireEvent.change(screen.getByLabelText("最大续写轮数"), {
      target: { value: "0" },
    });
    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("降智命中标签"), {
      target: { value: "规则命中" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        codex_reasoning_guard_hit_label: "规则命中",
      })
    );
    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalledWith(
      expect.objectContaining({
        codex_reasoning_guard_continuation_max_rounds: expect.any(Number),
      })
    );
  });

  it("does not render removed Codex reasoning guard legacy controls or sample diagnostics", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={vi.fn().mockResolvedValue(true)}
      />
    );

    expect(screen.queryByText("候选样本")).not.toBeInTheDocument();
    expect(screen.queryByText("被动样本")).not.toBeInTheDocument();
    expect(screen.queryByText("全局回退规则")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");

    expect(within(dialog).queryByLabelText("规则模板 JSON")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("全局回退规则")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("模型规则")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("候选样本")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("被动样本")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("继续思考补救")).not.toBeInTheDocument();
  });

  it("saves Codex reasoning guard concurrent retry settings", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("重试策略"), {
      target: { value: "concurrent" },
    });
    fireEvent.change(within(dialog).getByLabelText("最大并发数"), {
      target: { value: "3" },
    });
    fireEvent.change(within(dialog).getByLabelText("并发启动间隔 ms"), {
      target: { value: "1200" },
    });
    fireEvent.change(within(dialog).getByLabelText("最大尝试次数"), {
      target: { value: "7" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_hit_label: "降智命中",
      codex_reasoning_guard_rule_mode: "reasoning_tokens",
      codex_reasoning_guard_active_template_id: "builtin-legacy-reasoning-tokens",
      codex_reasoning_guard_custom_templates: [],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      codex_reasoning_guard_retry_policy: "concurrent",
      codex_reasoning_guard_concurrent_max: 3,
      codex_reasoning_guard_concurrent_interval_ms: 1200,
      codex_reasoning_guard_concurrent_max_attempts: 7,
      codex_reasoning_guard_model_fallbacks: [],
    });
  });

  it("saves Codex reasoning guard model fallback priority when switching models", async () => {
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("预算耗尽后"), {
      target: { value: "switch_model" },
    });
    const fallbackTextarea = await within(dialog).findByLabelText("模型回退优先级");
    fireEvent.change(fallbackTextarea, {
      target: { value: "gpt-5.4\ngpt-5.3-codex\n gpt-5.4 " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_hit_label: "降智命中",
      codex_reasoning_guard_rule_mode: "reasoning_tokens",
      codex_reasoning_guard_active_template_id: "builtin-legacy-reasoning-tokens",
      codex_reasoning_guard_custom_templates: [],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "switch_model",
      codex_reasoning_guard_retry_policy: "single",
      codex_reasoning_guard_concurrent_max: 5,
      codex_reasoning_guard_concurrent_interval_ms: 1000,
      codex_reasoning_guard_concurrent_max_attempts: 10,
      codex_reasoning_guard_model_fallbacks: ["gpt-5.4", "gpt-5.3-codex"],
    });
  });

  it("saves model-specific Codex reasoning guard matching through template filters", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "新建模板" }));
    fireEvent.change(within(dialog).getByLabelText("模板名称"), {
      target: { value: "Mini model template" },
    });
    fireEvent.change(within(dialog).getByLabelText("token 匹配"), {
      target: { value: "256" },
    });
    fireEvent.click(screen.getByRole("button", { name: "新增条件" }));
    fireEvent.change(within(dialog).getByLabelText("条件字段 1"), {
      target: { value: "requested_model" },
    });
    fireEvent.change(within(dialog).getByLabelText("条件值 1"), {
      target: { value: "gpt-5-mini-codex" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_hit_label: "降智命中",
      codex_reasoning_guard_rule_mode: "reasoning_tokens",
      codex_reasoning_guard_active_template_id: "custom-reasoning-guard",
      codex_reasoning_guard_custom_templates: [
        expect.objectContaining({
          id: "custom-reasoning-guard",
          name: "Mini model template",
          rules: [
            expect.objectContaining({
              reasoning_tokens: 256,
              filters: [
                expect.objectContaining({
                  field: "requested_model",
                  operator: "equals",
                  string_value: "gpt-5-mini-codex",
                }),
              ],
            }),
          ],
        }),
      ],
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      codex_reasoning_guard_retry_policy: "single",
      codex_reasoning_guard_concurrent_max: 5,
      codex_reasoning_guard_concurrent_interval_ms: 1000,
      codex_reasoning_guard_concurrent_max_attempts: 10,
      codex_reasoning_guard_model_fallbacks: [],
    });
  });

  it("does not surface or resave legacy Codex model rules", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");

    expect(within(dialog).queryByDisplayValue("gpt-5-mini-codex")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("模型规则")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith({
      codex_reasoning_guard_hit_label: "降智命中",
      codex_reasoning_guard_immediate_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_budget: 5,
      codex_reasoning_guard_delayed_retry_ms: 1000,
      codex_reasoning_guard_exhausted_action: "return_error",
      ...defaultCodexReasoningGuardRetrySettings,
    });
  });

  it("shows validation for invalid Codex reasoning guard template values", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(screen.getByRole("button", { name: "新建模板" }));
    fireEvent.change(within(dialog).getByLabelText("token 匹配"), {
      target: { value: "-1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    expect(persistCodexReasoningGuardSettings).not.toHaveBeenCalled();
    expect(screen.getByText(/token 匹配必须是 0 到 .*之间的整数/)).toBeInTheDocument();
  });

  it("falls back to default hit label when the field is blank", async () => {
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
        codexConfigToml={null}
        appSettings={createAppSettings({ codex_reasoning_guard_hit_label: "自定义命中" })}
        refreshCodex={vi.fn()}
        openCodexConfigDir={vi.fn()}
        persistCodexConfig={vi.fn()}
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={persistCodexReasoningGuardSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("降智命中标签"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "保存规则" }));

    await waitFor(() =>
      expect(persistCodexReasoningGuardSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          codex_reasoning_guard_hit_label: "降智命中",
        })
      )
    );
  });

  it("keeps the Codex reasoning guard hit label inside the guard details dialog", () => {
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
        persistCodexConfigToml={vi.fn().mockResolvedValue(true)}
        persistCodexReasoningGuardSettings={vi.fn().mockResolvedValue(true)}
      />
    );

    expect(screen.queryByLabelText("降智命中标签")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看降智拦截详情" }));
    expect(within(screen.getByRole("dialog")).getByLabelText("降智命中标签")).toBeInTheDocument();
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

  it("validates, cancels, reloads, and saves raw config.toml edits", async () => {
    const persistCodexConfigToml = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    vi.mocked(cliManagerCodexConfigTomlValidate)
      .mockResolvedValueOnce({ ok: true, error: null })
      .mockResolvedValueOnce({
        ok: false,
        error: { message: "invalid toml", line: 2, column: 3 },
      })
      .mockResolvedValueOnce({ ok: true, error: null })
      .mockResolvedValueOnce({ ok: true, error: null })
      .mockResolvedValueOnce({ ok: true, error: null });

    render(
      <CliManagerCodexTab
        codexAvailable="available"
        codexLoading={false}
        codexConfigLoading={false}
        codexConfigSaving={false}
        codexConfigTomlLoading={false}
        codexConfigTomlSaving={false}
        codexInfo={createCodexInfo()}
        codexConfig={createCodexConfig({ config_path: null })}
        codexConfigToml={{
          config_path: "/home/user/.codex/config.toml",
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
    const reloadButton = await screen.findByRole("button", { name: "重新加载" });
    expect(
      screen.getByText((_, element) => element?.textContent === "/home/user/.codex/config.toml")
    ).toBeInTheDocument();

    fireEvent.click(reloadButton);
    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "gpt-5"\n');

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    await waitFor(() => expect(cliManagerCodexConfigTomlValidate).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("mock-code-editor"), {
      target: { value: "bad = [" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(await screen.findByText("TOML 校验失败")).toBeInTheDocument();
    expect(screen.getByText("invalid toml")).toBeInTheDocument();
    expect(screen.getByText("(line 2, column 3)")).toBeInTheDocument();
    expect(persistCodexConfigToml).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("mock-code-editor"), {
      target: { value: 'model = "gpt-5.4"\n' },
    });
    await waitFor(
      () => {
        expect(cliManagerCodexConfigTomlValidate).toHaveBeenCalledWith('model = "gpt-5.4"\n');
      },
      { timeout: 1200 }
    );
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => {
      expect(persistCodexConfigToml).toHaveBeenCalledWith('model = "gpt-5.4"\n');
    });
    expect(screen.getByRole("button", { name: "取消" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(persistCodexConfigToml).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("button", { name: "编辑" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByLabelText("mock-code-editor"), {
      target: { value: 'model = "discarded"\n' },
    });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.getByLabelText("mock-code-editor")).toHaveValue('model = "gpt-5"\n');
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
