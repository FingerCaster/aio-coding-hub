import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useCodexRetryGatewayApplyCommitMutation,
  useCodexRetryGatewayCheckUpdateMutation,
  useCodexRetryGatewayCreateDetailsSessionMutation,
  useCodexRetryGatewayEnablePlanMutation,
  useCodexRetryGatewayRetryMutation,
  useCodexRetryGatewayRevokeDetailsSessionMutation,
  useCodexRetryGatewaySetEnabledMutation,
  useCodexRetryGatewaySetNodeOverrideMutation,
  useCodexRetryGatewayStatusQuery,
  useCodexRetryGatewayUninstallMutation,
  useCodexRetryGatewayValidateCommitMutation,
} from "../../../../query/codexRetryGateway";
import { openDesktopSinglePath } from "../../../../services/desktop/dialog";
import { openDesktopUrl } from "../../../../services/desktop/opener";
import {
  CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
  CODEX_RETRY_GATEWAY_PREVIOUS_COMMIT,
  createCodexRetryGatewayDetailsSession,
  createCodexRetryGatewayEnablePlan,
  createCodexRetryGatewayStatus,
} from "../../../../test/fixtures/codexRetryGateway";
import { CodexGatewayPage } from "../../../../pages/CodexGatewayPage";
import { toast } from "sonner";
import { CodexRetryGatewayManager } from "../CodexRetryGatewayManager";

vi.mock("../../../../query/codexRetryGateway", () => ({
  useCodexRetryGatewayStatusQuery: vi.fn(),
  useCodexRetryGatewayEnablePlanMutation: vi.fn(),
  useCodexRetryGatewaySetEnabledMutation: vi.fn(),
  useCodexRetryGatewayCheckUpdateMutation: vi.fn(),
  useCodexRetryGatewayValidateCommitMutation: vi.fn(),
  useCodexRetryGatewayApplyCommitMutation: vi.fn(),
  useCodexRetryGatewaySetNodeOverrideMutation: vi.fn(),
  useCodexRetryGatewayRetryMutation: vi.fn(),
  useCodexRetryGatewayUninstallMutation: vi.fn(),
  useCodexRetryGatewayCreateDetailsSessionMutation: vi.fn(),
  useCodexRetryGatewayRevokeDetailsSessionMutation: vi.fn(),
}));

vi.mock("../../../../services/desktop/dialog", () => ({
  openDesktopSinglePath: vi.fn(),
}));

vi.mock("../../../../services/desktop/opener", () => ({
  openDesktopUrl: vi.fn(),
}));

vi.mock("../../../../services/consoleLog", () => ({
  logToConsole: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: vi.fn(),
}));

type MutationMock = {
  isPending: boolean;
  mutateAsync: ReturnType<typeof vi.fn>;
};

function createMutation(mutateAsync = vi.fn()): MutationMock {
  return { isPending: false, mutateAsync };
}

let statusQuery: {
  data: ReturnType<typeof createCodexRetryGatewayStatus>;
  isLoading: boolean;
  isError: boolean;
  isFetching: boolean;
  error: null;
  refetch: ReturnType<typeof vi.fn>;
};

let mutations: {
  enablePlan: MutationMock;
  setEnabled: MutationMock;
  checkUpdate: MutationMock;
  validateCommit: MutationMock;
  applyCommit: MutationMock;
  nodeOverride: MutationMock;
  retry: MutationMock;
  uninstall: MutationMock;
  detailsSession: MutationMock;
  revokeDetailsSession: MutationMock;
};

function installHookMocks() {
  vi.mocked(useCodexRetryGatewayStatusQuery).mockImplementation(() => statusQuery as never);
  vi.mocked(useCodexRetryGatewayEnablePlanMutation).mockImplementation(
    () => mutations.enablePlan as never
  );
  vi.mocked(useCodexRetryGatewaySetEnabledMutation).mockImplementation(
    () => mutations.setEnabled as never
  );
  vi.mocked(useCodexRetryGatewayCheckUpdateMutation).mockImplementation(
    () => mutations.checkUpdate as never
  );
  vi.mocked(useCodexRetryGatewayValidateCommitMutation).mockImplementation(
    () => mutations.validateCommit as never
  );
  vi.mocked(useCodexRetryGatewayApplyCommitMutation).mockImplementation(
    () => mutations.applyCommit as never
  );
  vi.mocked(useCodexRetryGatewaySetNodeOverrideMutation).mockImplementation(
    () => mutations.nodeOverride as never
  );
  vi.mocked(useCodexRetryGatewayRetryMutation).mockImplementation(() => mutations.retry as never);
  vi.mocked(useCodexRetryGatewayUninstallMutation).mockImplementation(
    () => mutations.uninstall as never
  );
  vi.mocked(useCodexRetryGatewayCreateDetailsSessionMutation).mockImplementation(
    () => mutations.detailsSession as never
  );
  vi.mocked(useCodexRetryGatewayRevokeDetailsSessionMutation).mockImplementation(
    () => mutations.revokeDetailsSession as never
  );
}

function renderManager(
  props: { showDetailsFrame?: boolean; onOpenDetailsRoute?: () => void } = {}
) {
  return render(<CodexRetryGatewayManager {...props} />);
}

function expectNoLifecycleMutation() {
  expect(mutations.setEnabled.mutateAsync).not.toHaveBeenCalled();
  expect(mutations.retry.mutateAsync).not.toHaveBeenCalled();
  expect(mutations.uninstall.mutateAsync).not.toHaveBeenCalled();
}

describe("components/cli-manager/tabs/CodexRetryGatewayManager", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    statusQuery = {
      data: createCodexRetryGatewayStatus(),
      isLoading: false,
      isError: false,
      isFetching: false,
      error: null,
      refetch: vi.fn().mockResolvedValue(undefined),
    };
    mutations = {
      enablePlan: createMutation(),
      setEnabled: createMutation(
        vi.fn().mockResolvedValue({
          status: createCodexRetryGatewayStatus(),
          provider_sync: null,
        })
      ),
      checkUpdate: createMutation(),
      validateCommit: createMutation(),
      applyCommit: createMutation(vi.fn().mockResolvedValue(createCodexRetryGatewayStatus())),
      nodeOverride: createMutation(vi.fn().mockResolvedValue(statusQuery.data.node_status)),
      retry: createMutation(vi.fn().mockResolvedValue(createCodexRetryGatewayStatus())),
      uninstall: createMutation(vi.fn().mockResolvedValue(createCodexRetryGatewayStatus())),
      detailsSession: createMutation(
        vi.fn().mockResolvedValue(createCodexRetryGatewayDetailsSession())
      ),
      revokeDetailsSession: createMutation(vi.fn().mockResolvedValue(null)),
    };
    installHookMocks();
    vi.mocked(openDesktopSinglePath).mockResolvedValue(null);
    vi.mocked(openDesktopUrl).mockResolvedValue(true);
  });

  it("renders official source metadata, wrapping-safe values, and the persistent WSL warning", () => {
    const longError =
      "Gateway recovery failed at C:\\Users\\Example User\\AppData\\Local\\aio\\codex-retry-gateway\\sources\\abcdef1234567890abcdef1234567890abcdef12";
    statusQuery.data = createCodexRetryGatewayStatus({
      last_error: {
        code: "ROUTE_VERIFY_FAILED",
        category: "route_verify",
        message: longError,
        retryable: true,
      },
    });

    renderManager();

    expect(screen.getByText("nonononull/codex-retry-gateway")).toHaveClass("break-all");
    expect(screen.getByText("未声明")).toBeInTheDocument();
    expect(screen.getAllByText(statusQuery.data.selected_commit)).not.toHaveLength(0);
    for (const commit of screen.getAllByText(statusQuery.data.selected_commit)) {
      expect(commit).toHaveClass("break-all");
    }
    expect(screen.getByText("C:\\Program Files\\nodejs\\node.exe")).toHaveClass("break-all");
    expect(screen.getByText(/WSL 中的 Codex 仍然直连 AIO/)).toBeInTheDocument();
    expect(
      screen.getByText(new RegExp(longError.replace(/[\\.*+?()[\]{}|]/gu, "\\$&")))
    ).toHaveClass("break-words");
  });

  it("requests a read-only enable plan and submits every required acknowledgement together", async () => {
    const user = userEvent.setup();
    const plan = createCodexRetryGatewayEnablePlan();
    statusQuery.data = createCodexRetryGatewayStatus({
      desired_enabled: false,
      runtime_phase: "disabled",
      route_mode: "direct_aio",
      details_available: false,
    });
    mutations.enablePlan.mutateAsync.mockResolvedValue(plan);
    mutations.setEnabled.mutateAsync.mockResolvedValue({
      status: createCodexRetryGatewayStatus(),
      provider_sync: {
        status: "ok",
        target_provider: "aio",
        trigger: "external_gateway_enable",
        backup_dir: "C:\\Users\\test\\.codex\\backups_state\\provider-sync\\1",
        changed_session_files: ["session-1.jsonl", "session-2.jsonl"],
        sqlite_provider_rows_updated: 3,
        sqlite_user_event_rows_updated: 4,
        sqlite_cwd_rows_updated: 5,
        updated_workspace_roots: ["workspace-1"],
        warning: null,
      },
    });
    renderManager();

    await user.click(screen.getByRole("switch", { name: "切换 Codex 外部网关" }));

    await waitFor(() => expect(mutations.enablePlan.mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutations.setEnabled.mutateAsync).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog", { name: "启用 Codex 外部网关" });
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(within(dialog).getByText(/首次启用会下载并准备外部网关源码/)).toBeInTheDocument();
    expect(within(dialog).getAllByText(new RegExp(plan.selected_commit))).toHaveLength(2);
    expect(within(dialog).getByText(/同时启用 Codex CLI 代理/)).toBeInTheDocument();
    expect(within(dialog).getByText(/Provider Sync：OpenAI -> aio/)).toHaveTextContent(
      "同步会话与 Provider 状态、写入备份，需要先关闭 Codex App"
    );
    expect(within(dialog).getByText(/WSL 中的 Codex 仍然直连 AIO/)).toBeInTheDocument();

    const confirm = within(dialog).getByRole("button", { name: "确认启用" });
    const acknowledgements = within(dialog).getAllByRole("checkbox");
    expect(acknowledgements).toHaveLength(5);
    expect(confirm).toBeDisabled();

    for (const acknowledgement of acknowledgements.slice(0, -1)) {
      await user.click(acknowledgement);
      expect(confirm).toBeDisabled();
    }
    await user.click(acknowledgements[acknowledgements.length - 1]!);
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    await waitFor(() => {
      expect(mutations.setEnabled.mutateAsync).toHaveBeenCalledWith({
        enabled: true,
        planGeneration: 41,
        confirmation: {
          acceptedFirstDownload: true,
          acceptedUnreviewedCommit: true,
          acceptedCliProxyEnable: true,
          acceptedProviderSync: true,
          acceptedWslUnprotected: true,
        },
      });
    });
    expect(toast).toHaveBeenCalledWith(
      "Provider Sync 已完成：aio；会话文件 2；SQLite Provider 3；用户事件 4；工作目录 5；工作区 1；备份已创建"
    );
  });

  it("omits non-required enable rows and cancel performs no mutation", async () => {
    const user = userEvent.setup();
    statusQuery.data = createCodexRetryGatewayStatus({ desired_enabled: false });
    mutations.enablePlan.mutateAsync.mockResolvedValue(
      createCodexRetryGatewayEnablePlan({
        first_download_required: false,
        unreviewed_commit: false,
        cli_proxy_enable_required: false,
        provider_sync: {
          current_provider: "aio",
          target_provider: "aio",
          change_required: false,
          codex_must_be_closed: false,
        },
        wsl_codex_unprotected: false,
      })
    );
    renderManager();

    await user.click(screen.getByRole("switch", { name: "切换 Codex 外部网关" }));
    const dialog = await screen.findByRole("dialog", { name: "启用 Codex 外部网关" });
    expect(within(dialog).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "确认启用" })).toBeEnabled();
    await user.click(within(dialog).getByRole("button", { name: "取消" }));

    expect(mutations.setEnabled.mutateAsync).not.toHaveBeenCalled();
  });

  it("switch-off sends a gateway-only disable request", async () => {
    const user = userEvent.setup();
    renderManager();

    await user.click(screen.getByRole("switch", { name: "切换 Codex 外部网关" }));

    await waitFor(() => {
      expect(mutations.setEnabled.mutateAsync).toHaveBeenCalledWith({
        enabled: false,
        planGeneration: 7,
        confirmation: {
          acceptedFirstDownload: false,
          acceptedUnreviewedCommit: false,
          acceptedCliProxyEnable: false,
          acceptedProviderSync: false,
          acceptedWslUnprotected: false,
        },
      });
    });
  });

  it.each([
    [
      "CODEX_PROVIDER_SYNC_PROCESS_RUNNING: Codex is running",
      "Codex App 正在运行，请先关闭 Codex App 后重试",
    ],
    [
      "CODEX_PROVIDER_SYNC_PROCESS_CHECK_FAILED: process check failed",
      "无法确认 Codex App 是否已完全关闭，请先手动确认已退出后重试；详情见 Console 日志",
    ],
  ])("reuses safe Provider Sync guidance for %s", async (error, expectedToast) => {
    const user = userEvent.setup();
    statusQuery.data = createCodexRetryGatewayStatus({ desired_enabled: false });
    mutations.enablePlan.mutateAsync.mockResolvedValue(
      createCodexRetryGatewayEnablePlan({
        first_download_required: false,
        unreviewed_commit: false,
        cli_proxy_enable_required: false,
        wsl_codex_unprotected: false,
      })
    );
    mutations.setEnabled.mutateAsync.mockRejectedValue(new Error(error));
    renderManager();

    await user.click(screen.getByRole("switch", { name: "切换 Codex 外部网关" }));
    const dialog = await screen.findByRole("dialog", { name: "启用 Codex 外部网关" });
    await user.click(within(dialog).getByRole("checkbox", { name: /Provider Sync/ }));
    await user.click(within(dialog).getByRole("button", { name: "确认启用" }));

    await waitFor(() => expect(toast).toHaveBeenCalledWith(expectedToast));
  });

  it("checks for an update and applies the exact confirmed candidate", async () => {
    const user = userEvent.setup();
    const candidate = {
      commit: CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
      current_commit: statusQuery.data.active_commit,
      previous_commit: CODEX_RETRY_GATEWAY_PREVIOUS_COMMIT,
      rollback_commit: statusQuery.data.active_commit,
      official_main_commit: "fedcba0987654321fedcba0987654321fedcba09",
      commits_ahead: 3,
      summary: "Health verification and bridge-session hardening.",
      trust_state: "official_main_unreviewed" as const,
    };
    mutations.checkUpdate.mutateAsync.mockResolvedValue(candidate);
    renderManager();

    await user.click(screen.getByRole("button", { name: "检查更新" }));
    const dialog = await screen.findByRole("dialog", { name: "应用外部网关更新" });
    expect(within(dialog).getByText(candidate.summary)).toBeInTheDocument();
    expect(within(dialog).getByText(/回滚目标/)).toHaveTextContent(statusQuery.data.active_commit!);
    expect(within(dialog).getByText(/回滚目标/)).not.toHaveTextContent(
      CODEX_RETRY_GATEWAY_PREVIOUS_COMMIT
    );
    const confirm = within(dialog).getByRole("button", { name: "确认更新" });
    expect(confirm).toBeDisabled();
    await user.click(
      within(dialog).getByRole("checkbox", { name: "我确认要运行官方主线未审阅提交。" })
    );
    await user.click(confirm);

    await waitFor(() => {
      expect(mutations.applyCommit.mutateAsync).toHaveBeenCalledWith({
        planGeneration: 7,
        commit: CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
        acceptedUpdate: true,
        acceptedUnreviewedCommit: true,
      });
    });
  });

  it("validates a manual official SHA and applies it as an explicitly accepted update", async () => {
    const user = userEvent.setup();
    const typedCommit = "abcdef1";
    mutations.validateCommit.mutateAsync.mockResolvedValue({
      requested_commit: typedCommit,
      canonical_commit: CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
      official_main_commit: "fedcba0987654321fedcba0987654321fedcba09",
      official_main_ancestor: true,
      trust_state: "official_main_unreviewed",
      summary: "Validated as an ancestor of official main.",
      error: null,
    });
    renderManager();

    await user.type(screen.getByPlaceholderText("输入官方仓库完整提交 SHA"), typedCommit);
    await user.click(screen.getByRole("button", { name: "校验并选择" }));
    expect(mutations.validateCommit.mutateAsync).toHaveBeenCalledWith(typedCommit);

    const dialog = await screen.findByRole("dialog", { name: "应用指定提交" });
    await user.click(
      within(dialog).getByRole("checkbox", { name: "我确认要运行官方主线未审阅提交。" })
    );
    await user.click(within(dialog).getByRole("button", { name: "应用此提交" }));

    await waitFor(() => {
      expect(mutations.applyCommit.mutateAsync).toHaveBeenCalledWith({
        planGeneration: 7,
        commit: CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
        acceptedUpdate: true,
        acceptedUnreviewedCommit: true,
      });
    });
  });

  it("selects and resets Node with the current generation", async () => {
    const user = userEvent.setup();
    vi.mocked(openDesktopSinglePath).mockResolvedValueOnce("C:\\Tools\\node-v22\\node.exe");
    renderManager();

    await user.click(screen.getByRole("button", { name: "选择 Node" }));
    await waitFor(() => {
      expect(mutations.nodeOverride.mutateAsync).toHaveBeenNthCalledWith(1, {
        generation: 7,
        executable: "C:\\Tools\\node-v22\\node.exe",
      });
    });
    await user.click(screen.getByRole("button", { name: "恢复自动" }));
    await waitFor(() => {
      expect(mutations.nodeOverride.mutateAsync).toHaveBeenNthCalledWith(2, {
        generation: 7,
        executable: null,
      });
    });
  });

  it("retries recovery and only uninstalls after explicit data-removal confirmation", async () => {
    const user = userEvent.setup();
    statusQuery.data = createCodexRetryGatewayStatus({
      desired_enabled: false,
      runtime_phase: "disabled",
      route_mode: "direct_aio",
      process_status: {
        phase: "stopped",
        owned: false,
        healthy: false,
        process_id: null,
        listener: null,
      },
      details_available: false,
    });
    renderManager();

    await user.click(screen.getByRole("button", { name: "重试恢复" }));
    expect(mutations.retry.mutateAsync).toHaveBeenCalledWith(7);
    await user.click(screen.getByRole("button", { name: "卸载并清理" }));
    expect(mutations.uninstall.mutateAsync).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("dialog", { name: "卸载 Codex 外部网关" });
    await user.click(within(dialog).getByRole("button", { name: "确认卸载" }));
    await waitFor(() => {
      expect(mutations.uninstall.mutateAsync).toHaveBeenCalledWith({
        generation: 7,
        confirmedDataRemoval: true,
      });
    });
  });

  it("requires the gateway to be disabled and stopped before offering uninstall", async () => {
    const user = userEvent.setup();
    renderManager();

    const uninstallButton = screen.getByRole("button", { name: "卸载并清理" });
    expect(uninstallButton).toBeDisabled();
    expect(screen.getByText(/请先停用拦截网关/)).toBeInTheDocument();
    await user.click(uninstallButton);
    expect(screen.queryByRole("dialog", { name: "卸载 Codex 外部网关" })).not.toBeInTheDocument();
    expect(mutations.uninstall.mutateAsync).not.toHaveBeenCalled();
  });

  it("creates bridge sessions on entry and refresh, replacing a same-generation URL", async () => {
    const user = userEvent.setup();
    const firstSession = createCodexRetryGatewayDetailsSession("first", "a".repeat(32));
    const refreshedSession = createCodexRetryGatewayDetailsSession("refreshed", "b".repeat(32));
    const browserSession = createCodexRetryGatewayDetailsSession("browser", "c".repeat(32));
    mutations.detailsSession.mutateAsync
      .mockResolvedValueOnce(firstSession)
      .mockResolvedValueOnce(refreshedSession)
      .mockResolvedValueOnce(browserSession);

    const { unmount } = renderManager({ showDetailsFrame: true });

    const firstFrame = await screen.findByTitle("Codex 外部网关管理页");
    expect(firstFrame).toHaveAttribute("src", firstSession.iframe_url);
    expect(screen.getByText("仅限 127.0.0.1 的临时桥接会话")).toBeInTheDocument();
    expect(firstFrame).toHaveAttribute(
      "sandbox",
      "allow-scripts allow-same-origin allow-forms allow-modals allow-downloads"
    );
    expect(firstFrame.getAttribute("sandbox")).not.toMatch(
      /allow-top-navigation|allow-popups|allow-popups-to-escape-sandbox|ipc/iu
    );
    fireEvent.load(firstFrame);
    expect(refreshedSession.generation).toBe(firstSession.generation);
    expect(refreshedSession.iframe_url).not.toBe(firstSession.iframe_url);

    await user.click(screen.getByRole("button", { name: "刷新嵌入" }));
    await waitFor(() => {
      expect(screen.getByTitle("Codex 外部网关管理页")).toHaveAttribute(
        "src",
        refreshedSession.iframe_url
      );
    });
    expect(mutations.detailsSession.mutateAsync).toHaveBeenCalledTimes(2);

    const browserButtons = screen.getAllByRole("button", { name: "浏览器打开" });
    await user.click(browserButtons[browserButtons.length - 1]!);
    expect(openDesktopUrl).toHaveBeenCalledWith(browserSession.browser_url);
    expect(mutations.detailsSession.mutateAsync).toHaveBeenCalledTimes(3);
    expect(mutations.revokeDetailsSession.mutateAsync).toHaveBeenNthCalledWith(
      1,
      firstSession.iframe_view_id
    );
    expect(mutations.revokeDetailsSession.mutateAsync).toHaveBeenNthCalledWith(
      2,
      browserSession.iframe_view_id
    );

    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expectNoLifecycleMutation();
    unmount();
    expectNoLifecycleMutation();
    await waitFor(() => {
      expect(mutations.revokeDetailsSession.mutateAsync).toHaveBeenNthCalledWith(
        3,
        refreshedSession.iframe_view_id
      );
    });
  });

  it("replaces the bridge session when the managed runtime generation changes", async () => {
    const firstSession = createCodexRetryGatewayDetailsSession("generation-7");
    const nextSession = {
      ...createCodexRetryGatewayDetailsSession("generation-8"),
      generation: 8,
    };
    mutations.detailsSession.mutateAsync
      .mockResolvedValueOnce(firstSession)
      .mockResolvedValueOnce(nextSession);

    const view = renderManager({ showDetailsFrame: true });
    expect(await screen.findByTitle("Codex 外部网关管理页")).toHaveAttribute(
      "src",
      firstSession.iframe_url
    );

    statusQuery.data = createCodexRetryGatewayStatus({ generation: 8 });
    view.rerender(<CodexRetryGatewayManager showDetailsFrame />);

    await waitFor(() => {
      expect(mutations.detailsSession.mutateAsync).toHaveBeenCalledTimes(2);
      expect(screen.getByTitle("Codex 外部网关管理页")).toHaveAttribute(
        "src",
        nextSession.iframe_url
      );
    });
  });

  it("does not mint or advertise a browser session when details are unavailable", () => {
    statusQuery.data = createCodexRetryGatewayStatus({ details_available: false });
    renderManager({ showDetailsFrame: true });

    expect(mutations.detailsSession.mutateAsync).not.toHaveBeenCalled();
    expect(screen.getByText(/管理桥接暂不可用/)).toHaveTextContent("请刷新状态");
    expect(screen.queryByText(/直接在浏览器中打开管理页/)).not.toBeInTheDocument();
    for (const button of screen.getAllByRole("button", { name: "浏览器打开" })) {
      expect(button).toBeDisabled();
    }
  });

  it.each(["返回", "退出"])(
    "%s only navigates and never changes gateway lifecycle",
    async (action) => {
      const user = userEvent.setup();
      render(
        <MemoryRouter
          initialEntries={["/cli-manager", "/cli-manager/codex-gateway"]}
          initialIndex={1}
        >
          <Routes>
            <Route path="/cli-manager" element={<div>CLI manager route</div>} />
            <Route path="/cli-manager/codex-gateway" element={<CodexGatewayPage />} />
          </Routes>
        </MemoryRouter>
      );

      await screen.findByTitle("Codex 外部网关管理页");
      expect(screen.getByText("受管实例状态与本地桥接安全边界")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: action }));
      expect(await screen.findByText("CLI manager route")).toBeInTheDocument();
      expectNoLifecycleMutation();
    }
  );
});
