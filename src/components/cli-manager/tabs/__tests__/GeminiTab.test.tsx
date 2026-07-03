import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { CliManagerGeminiTab } from "../GeminiTab";

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn() }),
}));

vi.mock("../../CliVersionBadge", () => ({
  CliVersionBadge: ({ cliKey }: { cliKey: string }) => <div>version-badge-{cliKey}</div>,
}));

function createGeminiInfo(overrides: Partial<any> = {}) {
  return {
    found: true,
    version: "1.2.3",
    executable_path: "/bin/gemini",
    resolved_via: "PATH",
    shell: "/bin/zsh",
    error: null,
    ...overrides,
  };
}

function createGeminiConfig(overrides: Partial<any> = {}) {
  return {
    configDir: "/home/user/.gemini",
    configPath: "/home/user/.gemini/settings.json",
    exists: true,
    modelName: "gemini-2.5-pro",
    modelMaxSessionTurns: -1,
    modelCompressionThreshold: 0.7,
    defaultApprovalMode: "plan",
    enableAutoUpdate: true,
    enableNotifications: false,
    vimMode: true,
    retryFetchErrors: true,
    maxAttempts: 5,
    uiTheme: "dark",
    uiHideBanner: true,
    uiHideTips: false,
    uiShowLineNumbers: true,
    uiInlineThinkingMode: "full",
    usageStatisticsEnabled: false,
    sessionRetentionEnabled: true,
    sessionRetentionMaxAge: "30d",
    planModelRouting: false,
    securityAuthSelectedType: "gemini-api-key",
    ...overrides,
  };
}

describe("components/cli-manager/tabs/GeminiTab", () => {
  it("renders installed state, badge, and refresh action", () => {
    const refresh = vi.fn();
    render(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={createGeminiConfig()}
        refreshGeminiInfo={refresh}
        persistGeminiConfig={vi.fn()}
      />
    );

    expect(screen.getByText("已安装 1.2.3")).toBeInTheDocument();
    expect(screen.getByText("version-badge-gemini")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "刷新状态" }));
    expect(refresh).toHaveBeenCalled();
  });

  it("persists config changes through inputs, selects, and switches", () => {
    const persistGeminiConfig = vi.fn();

    render(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={createGeminiConfig()}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={persistGeminiConfig}
      />
    );

    const modelItem = screen.getByText("默认模型 (model.name)").parentElement?.parentElement;
    expect(modelItem).toBeTruthy();
    const modelInput = within(modelItem as HTMLElement).getByRole("textbox");
    fireEvent.change(modelInput, { target: { value: " gemini-2.5-flash " } });
    fireEvent.blur(modelInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ modelName: "gemini-2.5-flash" });

    const approvalItem = screen.getByText("审批模式 (general.defaultApprovalMode)").parentElement
      ?.parentElement;
    expect(approvalItem).toBeTruthy();
    fireEvent.change(within(approvalItem as HTMLElement).getByRole("combobox"), {
      target: { value: "auto_edit" },
    });
    expect(persistGeminiConfig).toHaveBeenCalledWith({ defaultApprovalMode: "auto_edit" });

    const maxAttemptsItem = screen.getByText("最大尝试次数 (general.maxAttempts)").parentElement
      ?.parentElement;
    expect(maxAttemptsItem).toBeTruthy();
    const maxAttemptsInput = within(maxAttemptsItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(maxAttemptsInput, { target: { value: "9" } });
    fireEvent.blur(maxAttemptsInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ maxAttempts: 9 });

    const hideBannerItem = screen.getByText("隐藏 Banner (ui.hideBanner)").parentElement
      ?.parentElement;
    expect(hideBannerItem).toBeTruthy();
    fireEvent.click(within(hideBannerItem as HTMLElement).getByRole("switch"));
    expect(persistGeminiConfig).toHaveBeenCalledWith({ uiHideBanner: false });

    const statsItem = screen.getByText("使用统计 (privacy.usageStatisticsEnabled)").parentElement
      ?.parentElement;
    expect(statsItem).toBeTruthy();
    fireEvent.click(within(statsItem as HTMLElement).getByRole("switch"));
    expect(persistGeminiConfig).toHaveBeenCalledWith({ usageStatisticsEnabled: true });
  });

  it("validates number fields and persists session/auth fields", () => {
    const persistGeminiConfig = vi.fn();

    render(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={createGeminiConfig({
          modelMaxSessionTurns: 12,
          modelCompressionThreshold: 0.8,
          sessionRetentionMaxAge: "14d",
          securityAuthSelectedType: "oauth-personal",
        })}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={persistGeminiConfig}
      />
    );

    const maxAttemptsItem = screen.getByText("最大尝试次数 (general.maxAttempts)").parentElement
      ?.parentElement;
    const maxAttemptsInput = within(maxAttemptsItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(maxAttemptsInput, { target: { value: "1.5" } });
    fireEvent.blur(maxAttemptsInput);
    expect(toast.error).toHaveBeenCalledWith("maxAttempts 必须为整数");
    expect(maxAttemptsInput).toHaveValue(5);

    const turnsItem = screen.getByText("会话轮次上限 (model.maxSessionTurns)").parentElement
      ?.parentElement;
    const turnsInput = within(turnsItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(turnsInput, { target: { value: "18" } });
    fireEvent.blur(turnsInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ modelMaxSessionTurns: 18 });

    fireEvent.change(turnsInput, { target: { value: "" } });
    fireEvent.blur(turnsInput);
    expect(turnsInput).toHaveValue(12);

    const thresholdItem = screen.getByText("压缩阈值 (model.compressionThreshold)").parentElement
      ?.parentElement;
    const thresholdInput = within(thresholdItem as HTMLElement).getByRole("spinbutton");
    fireEvent.change(thresholdInput, { target: { value: "" } });
    fireEvent.blur(thresholdInput);
    expect(thresholdInput).toHaveValue(0.8);

    fireEvent.change(thresholdInput, { target: { value: "0.65" } });
    fireEvent.blur(thresholdInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ modelCompressionThreshold: 0.65 });

    const themeItem = screen.getByText("主题 (ui.theme)").parentElement?.parentElement;
    const themeInput = within(themeItem as HTMLElement).getByRole("textbox");
    fireEvent.change(themeInput, { target: { value: "  light  " } });
    fireEvent.blur(themeInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ uiTheme: "light" });

    const thinkingItem = screen.getByText("思考展示模式 (ui.inlineThinkingMode)").parentElement
      ?.parentElement;
    fireEvent.change(within(thinkingItem as HTMLElement).getByRole("combobox"), {
      target: { value: "off" },
    });
    expect(persistGeminiConfig).toHaveBeenCalledWith({ uiInlineThinkingMode: "off" });

    const sessionAgeItem = screen.getByText("会话保留时长 (general.sessionRetention.maxAge)")
      .parentElement?.parentElement;
    const sessionAgeInput = within(sessionAgeItem as HTMLElement).getByRole("textbox");
    fireEvent.change(sessionAgeInput, { target: { value: "  45d  " } });
    fireEvent.blur(sessionAgeInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({ sessionRetentionMaxAge: "45d" });

    const authItem = screen.getByText("认证类型 (security.auth.selectedType)").parentElement
      ?.parentElement;
    const authInput = within(authItem as HTMLElement).getByRole("textbox");
    fireEvent.change(authInput, { target: { value: "  gemini-api-key  " } });
    fireEvent.blur(authInput);
    expect(persistGeminiConfig).toHaveBeenCalledWith({
      securityAuthSelectedType: "gemini-api-key",
    });
  });

  it("persists remaining switches and disables controls while saving", () => {
    const persistGeminiConfig = vi.fn();

    render(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={true}
        geminiConfig={createGeminiConfig({
          uiHideTips: null,
          enableNotifications: null,
          sessionRetentionEnabled: null,
          planModelRouting: null,
        })}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={persistGeminiConfig}
      />
    );

    const hideTipsSwitch = within(
      screen.getByText("隐藏 Tips (ui.hideTips)").parentElement?.parentElement as HTMLElement
    ).getByRole("switch");
    expect(hideTipsSwitch).not.toBeChecked();
    expect(hideTipsSwitch).toBeDisabled();

    for (const label of [
      "显示行号 (ui.showLineNumbers)",
      "Vim 模式 (general.vimMode)",
      "自动更新 (general.enableAutoUpdate)",
      "通知 (general.enableNotifications)",
      "重试抓取错误 (general.retryFetchErrors)",
      "会话保留 (general.sessionRetention.enabled)",
      "计划模式模型路由 (general.plan.modelRouting)",
    ]) {
      const item = screen.getByText(label).parentElement?.parentElement;
      fireEvent.click(within(item as HTMLElement).getByRole("switch"));
    }

    expect(persistGeminiConfig).not.toHaveBeenCalled();
  });

  it("renders checking/no-info/null-config states and increments refresh token after refresh", async () => {
    const refreshGeminiInfo = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <CliManagerGeminiTab
        geminiAvailable="checking"
        geminiLoading={true}
        geminiInfo={null}
        geminiConfigLoading={true}
        geminiConfigSaving={false}
        geminiConfig={null}
        refreshGeminiInfo={refreshGeminiInfo}
        persistGeminiConfig={vi.fn()}
      />
    );

    expect(screen.getByText("检测中...")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新状态" })).toBeDisabled();
    expect(screen.getByText("暂无信息，请尝试刷新")).toBeInTheDocument();

    rerender(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo({ shell: null, resolved_via: null, executable_path: null })}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={null}
        refreshGeminiInfo={refreshGeminiInfo}
        persistGeminiConfig={vi.fn()}
      />
    );

    expect(screen.getByText("暂无配置，请尝试刷新")).toBeInTheDocument();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: "刷新状态" }));
    await waitFor(() => expect(refreshGeminiInfo).toHaveBeenCalledTimes(1));
  });

  it("resets draft values when a new config source is rendered", () => {
    const persistGeminiConfig = vi.fn();

    const { rerender } = render(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={createGeminiConfig({ modelName: "gemini-old", uiTheme: "dark" })}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={persistGeminiConfig}
      />
    );

    const modelItem = screen.getByText("默认模型 (model.name)").parentElement?.parentElement;
    const modelInput = within(modelItem as HTMLElement).getByRole("textbox");
    fireEvent.change(modelInput, { target: { value: "dirty-draft" } });
    expect(modelInput).toHaveValue("dirty-draft");

    rerender(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo()}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={createGeminiConfig({ modelName: "gemini-new", uiTheme: "light" })}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={persistGeminiConfig}
      />
    );

    expect(within(modelItem as HTMLElement).getByRole("textbox")).toHaveValue("gemini-new");
  });

  it("renders unavailable and error states", () => {
    const { rerender } = render(
      <CliManagerGeminiTab
        geminiAvailable="unavailable"
        geminiLoading={false}
        geminiInfo={null}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={null}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={vi.fn()}
      />
    );
    expect(screen.getByText("数据不可用")).toBeInTheDocument();

    rerender(
      <CliManagerGeminiTab
        geminiAvailable="available"
        geminiLoading={false}
        geminiInfo={createGeminiInfo({
          found: false,
          version: null,
          executable_path: null,
          error: "boom",
        })}
        geminiConfigLoading={false}
        geminiConfigSaving={false}
        geminiConfig={null}
        refreshGeminiInfo={vi.fn()}
        persistGeminiConfig={vi.fn()}
      />
    );
    expect(screen.getByText("检测失败：")).toBeInTheDocument();
    expect(screen.getByText("boom")).toBeInTheDocument();
  });
});
