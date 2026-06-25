import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { PluginsPage } from "../PluginsPage";
import type {
  PluginDetail,
  PluginHookExecutionReport,
  PluginInstallPreview,
  PluginReplayFixture,
  PluginSummary,
  PluginUpdateDiff,
} from "../../services/plugins";
import { openDesktopSinglePath } from "../../services/desktop/dialog";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import {
  usePluginDisableMutation,
  usePluginEnableMutation,
  usePluginGrantPermissionsMutation,
  usePluginInstallFromFileMutation,
  usePluginInstallOfficialMutation,
  usePluginPreviewFromFileMutation,
  usePluginPreviewUpdateFromFileMutation,
  usePluginExportReplayFixtureMutation,
  usePluginQuery,
  usePluginRollbackMutation,
  usePluginSaveConfigMutation,
  usePluginRuntimeReportsQuery,
  usePluginUpdateFromFileMutation,
  usePluginsListQuery,
  usePluginUninstallMutation,
} from "../../query/plugins";

vi.mock("sonner", () => {
  const toast = Object.assign(vi.fn(), {
    loading: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
  });
  return { toast };
});

vi.mock("../../services/desktop/dialog", async () => {
  const actual = await vi.importActual<typeof import("../../services/desktop/dialog")>(
    "../../services/desktop/dialog"
  );
  return { ...actual, openDesktopSinglePath: vi.fn() };
});

vi.mock("../../services/clipboard", () => ({ copyText: vi.fn().mockResolvedValue(undefined) }));

vi.mock("../../query/plugins", async () => {
  const actual = await vi.importActual<typeof import("../../query/plugins")>("../../query/plugins");
  return {
    ...actual,
    usePluginsListQuery: vi.fn(),
    usePluginQuery: vi.fn(),
    usePluginInstallFromFileMutation: vi.fn(),
    usePluginInstallOfficialMutation: vi.fn(),
    usePluginPreviewFromFileMutation: vi.fn(),
    usePluginPreviewUpdateFromFileMutation: vi.fn(),
    usePluginExportReplayFixtureMutation: vi.fn(),
    usePluginUpdateFromFileMutation: vi.fn(),
    usePluginRollbackMutation: vi.fn(),
    usePluginEnableMutation: vi.fn(),
    usePluginGrantPermissionsMutation: vi.fn(),
    usePluginDisableMutation: vi.fn(),
    usePluginUninstallMutation: vi.fn(),
    usePluginSaveConfigMutation: vi.fn(),
    usePluginRuntimeReportsQuery: vi.fn(),
  };
});

function summary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: 1,
    plugin_id: "community.prompt-helper",
    name: "Community Prompt Helper",
    current_version: "1.0.0",
    status: "disabled",
    runtime: "declarativeRules",
    permission_risk: "high",
    update_available: false,
    last_error: null,
    created_at: 10,
    updated_at: 20,
    ...overrides,
  };
}

function detail(overrides: Partial<PluginDetail> = {}): PluginDetail {
  const baseSummary = summary();
  return {
    summary: baseSummary,
    manifest: {
      id: baseSummary.plugin_id,
      name: baseSummary.name,
      version: "1.0.0",
      apiVersion: "1.0.0",
      runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
      hooks: [{ name: "gateway.request.afterBodyRead", priority: 100, failurePolicy: "fail-open" }],
      permissions: ["request.body.read", "request.body.write"],
      hostCompatibility: {
        app: ">=0.56.0 <1.0.0",
        pluginApi: "^1.0.0",
        platforms: ["macos", "windows", "linux"],
      },
      configSchema: {
        type: "object",
        required: ["mode"],
        properties: {
          mode: { type: "string", enum: ["append_instruction", "rewrite_system_message"] },
        },
      },
    },
    install_source: "local",
    installed_dir: null,
    config: { mode: "append_instruction" },
    granted_permissions: ["request.body.read"],
    pending_permissions: ["request.body.write"],
    audit_logs: [
      {
        id: 1,
        plugin_id: baseSummary.plugin_id,
        trace_id: "trace-1",
        event_type: "plugin.installed",
        risk_level: "low",
        message: "Plugin installed",
        details: {},
        created_at: 30,
      },
    ],
    runtime_failures: [],
    rollback_versions: [],
    ...overrides,
  };
}

function installPreview(overrides: Partial<PluginInstallPreview> = {}): PluginInstallPreview {
  return {
    pluginId: "community.prompt-helper",
    name: "Community Prompt Helper",
    version: "1.0.0",
    source: "local",
    description: "Helps prompt editing",
    author: null,
    homepage: null,
    repository: null,
    license: "MIT",
    category: "productivity",
    runtime: {
      kind: "declarativeRules",
      label: "规则插件",
      supported: true,
      blockingReasons: [],
    },
    hooks: [{ name: "gateway.request.afterBodyRead", priority: 100, failurePolicy: "fail-open" }],
    permissions: [{ permission: "request.body.read", risk: "high", granted: false, pending: true }],
    compatibility: {
      compatible: true,
      hostVersion: "0.62.2",
      appRange: ">=0.56.0 <1.0.0",
      pluginApiRange: "^1.0.0",
      platforms: ["macos", "windows", "linux"],
      blockingReasons: [],
    },
    trust: {
      checksum: "sha256-install",
      expectedChecksum: null,
      checksumVerified: false,
      signatureVerified: false,
      unsigned: true,
      developerMode: false,
    },
    existingStatus: null,
    existingVersion: null,
    blockingReasons: [],
    warnings: [],
    ...overrides,
  };
}

function updateDiff(overrides: Partial<PluginUpdateDiff> = {}): PluginUpdateDiff {
  return {
    pluginId: "community.prompt-helper",
    fromVersion: "1.0.0",
    toVersion: "1.1.0",
    versionDirection: "upgrade",
    runtimeChange: null,
    hookChanges: [
      {
        name: "gateway.response.beforeSend",
        change: "added",
        before: null,
        after: "priority 50",
      },
    ],
    permissionChanges: [
      {
        permission: "request.body.write",
        risk: "critical",
        change: "added_pending",
      },
    ],
    configVersionChange: "1 -> 2",
    compatibility: {
      compatible: true,
      hostVersion: "0.62.2",
      appRange: ">=0.56.0 <1.0.0",
      pluginApiRange: "^1.0.0",
      platforms: ["macos", "windows", "linux"],
      blockingReasons: [],
    },
    trust: {
      checksum: "sha256-update",
      expectedChecksum: null,
      checksumVerified: false,
      signatureVerified: false,
      unsigned: true,
      developerMode: false,
    },
    rollbackAvailable: true,
    blockingReasons: [],
    warnings: [],
    ...overrides,
  };
}

function runtimeReport(
  overrides: Partial<PluginHookExecutionReport> = {}
): PluginHookExecutionReport {
  return {
    id: 1,
    plugin_id: "community.prompt-helper",
    trace_id: "trace-report-1",
    hook_name: "gateway.request.afterBodyRead",
    runtime_kind: "declarativeRules",
    status: "completed",
    started_at_ms: 1000,
    duration_ms: 9,
    failure_kind: null,
    error_code: null,
    failure_policy: "fail-open",
    circuit_state: "closed",
    context_budget: {},
    output_budget: {},
    mutation_summary: { changed: true, field: "requestBody" },
    replayable: true,
    replay_export_reason: null,
    created_at: 10,
    ...overrides,
  };
}

function replayFixture(overrides: Partial<PluginReplayFixture> = {}): PluginReplayFixture {
  return {
    schemaVersion: 1,
    traceId: "trace-report-1",
    source: {
      appVersion: "0.62.3",
      traceId: "trace-report-1",
      exportedAtMs: 1000,
      requestLogId: 1,
      createdAtMs: 900,
    },
    hookName: "gateway.request.afterBodyRead",
    pluginId: "community.prompt-helper",
    request: {
      cliKey: "codex",
      sessionId: null,
      method: "POST",
      path: "/v1/responses",
      query: null,
      provider: "OpenAI Primary",
      providerSource: null,
      model: "gpt-5-mini",
      headers: null,
      body: null,
      normalizedMessages: [],
      meta: {},
    },
    response: {
      status: 200,
      errorCode: null,
      headers: null,
      body: null,
      chunks: [],
      meta: {},
    },
    log: { body: null, meta: {} },
    attempts: [],
    runtimeReports: [],
    notes: ["request body is not persisted"],
    ...overrides,
  };
}

function mutation(overrides: Record<string, unknown> = {}) {
  return {
    mutateAsync: vi.fn().mockResolvedValue(detail()),
    isPending: false,
    ...overrides,
  };
}

function renderWithProviders(element: ReactElement) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{element}</MemoryRouter>
    </QueryClientProvider>
  );
}

describe("pages/PluginsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(
      mutation({ mutateAsync: vi.fn().mockResolvedValue(installPreview()) }) as any
    );
    vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(
      mutation({ mutateAsync: vi.fn().mockResolvedValue(updateDiff()) }) as any
    );
    vi.mocked(usePluginInstallFromFileMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginInstallOfficialMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginUpdateFromFileMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginRollbackMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginEnableMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginGrantPermissionsMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginDisableMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginUninstallMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginSaveConfigMutation).mockReturnValue(mutation() as any);
    vi.mocked(usePluginExportReplayFixtureMutation).mockReturnValue(
      mutation({ mutateAsync: vi.fn().mockResolvedValue(replayFixture()) }) as any
    );
    vi.mocked(usePluginRuntimeReportsQuery).mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail(),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
  });

  it("renders list fields and plugin detail permissions", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary({ update_available: true, last_error: "Last failure" })],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getAllByText("Community Prompt Helper").length).toBeGreaterThan(0);
    expect(screen.getAllByText("community.prompt-helper").length).toBeGreaterThan(0);
    expect(screen.getAllByText("规则插件").length).toBeGreaterThan(0);
    expect(screen.getByText("高风险")).toBeInTheDocument();
    expect(screen.getByText("可更新")).toBeInTheDocument();
    expect(screen.getByText("Last failure")).toBeInTheDocument();
    expect(screen.getByText("gateway.request.afterBodyRead")).toBeInTheDocument();
    expect(screen.getByText("request.body.write")).toBeInTheDocument();
    expect(screen.getByText("待允许")).toBeInTheDocument();
    expect(screen.getByText("Plugin installed")).toBeInTheDocument();
  });

  it("presents plugin value, data access, settings, and developer metadata in that order", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("这个插件会做什么")).toBeInTheDocument();
    expect(screen.getByText("数据访问")).toBeInTheDocument();
    expect(screen.getByText("设置")).toBeInTheDocument();
    expect(screen.getByText("开发者信息")).toBeInTheDocument();
    expect(screen.getByText("读取你发送给模型的内容")).toBeInTheDocument();
  });

  it("does not present unknown audit trust as verified", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        audit_logs: [
          {
            id: 12,
            plugin_id: "community.prompt-helper",
            trace_id: null,
            event_type: "plugin.installed",
            risk_level: "low",
            message: "Plugin installed before trust audit fields",
            details: {},
            created_at: 42,
          },
        ],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    const lifecyclePanel = screen.getByText("生命周期").closest("section");
    expect(lifecyclePanel).not.toBeNull();
    expect(within(lifecyclePanel as HTMLElement).getByText("签名状态未记录")).toBeInTheDocument();
    expect(screen.queryByText("签名已验证")).not.toBeInTheDocument();
  });

  it("renders runtime reports and exports replay fixtures", async () => {
    const { copyText } = await import("../../services/clipboard");
    const exportReplay = vi.fn().mockResolvedValue(replayFixture());
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginRuntimeReportsQuery).mockReturnValue({
      data: [runtimeReport()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginExportReplayFixtureMutation).mockReturnValue(
      mutation({ mutateAsync: exportReplay }) as any
    );

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("completed")).toBeInTheDocument();
    expect(screen.getByText("declarativeRules")).toBeInTheDocument();
    expect(screen.getByText("9ms")).toBeInTheDocument();
    expect(screen.getByText("trace-report-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /导出 Replay/ }));

    await waitFor(() => {
      expect(exportReplay).toHaveBeenCalledWith({
        traceId: "trace-report-1",
        hookName: "gateway.request.afterBodyRead",
        pluginId: "community.prompt-helper",
      });
      expect(copyText).toHaveBeenCalledWith(expect.stringContaining('"traceId": "trace-report-1"'));
      expect(toast.success).toHaveBeenCalledWith("Replay fixture 已复制");
    });
  });

  it("uses only the latest lifecycle audit for trust state", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        audit_logs: [
          {
            id: 13,
            plugin_id: "community.prompt-helper",
            trace_id: null,
            event_type: "plugin.rollback",
            risk_level: "high",
            message: "Plugin rolled back",
            details: { version: "1.0.0" },
            created_at: 50,
          },
          {
            id: 12,
            plugin_id: "community.prompt-helper",
            trace_id: null,
            event_type: "plugin.updated",
            risk_level: "low",
            message: "Plugin updated from signed package",
            details: { signatureVerified: true, unsigned: false },
            created_at: 40,
          },
        ],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    const lifecyclePanel = screen.getByText("生命周期").closest("section");
    expect(lifecyclePanel).not.toBeNull();
    expect(within(lifecyclePanel as HTMLElement).getByText("签名状态未记录")).toBeInTheDocument();
    expect(screen.queryByText("签名已验证")).not.toBeInTheDocument();
    expect(screen.queryByText("未签名")).not.toBeInTheDocument();
  });

  it("renders runtime failures in the runtime observability section", async () => {
    const { copyText } = await import("../../services/clipboard");
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        audit_logs: [],
        runtime_failures: [
          {
            id: 11,
            plugin_id: "community.prompt-helper",
            hook_name: "gateway.request.afterBodyRead",
            failure_kind: "timeout",
            message: "Hook timed out after 30s",
            trace_id: "trace-runtime-1",
            created_at: 41,
          },
        ],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("运行观测")).toBeInTheDocument();
    expect(screen.getByText("Hook timed out after 30s")).toBeInTheDocument();
    expect(screen.getByText("timeout")).toBeInTheDocument();
    expect(screen.getAllByText("gateway.request.afterBodyRead").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: /trace-runtime-1/ }));

    await waitFor(() => {
      expect(copyText).toHaveBeenCalledWith("trace-runtime-1");
      expect(toast.success).toHaveBeenCalledWith("Trace ID 已复制");
    });
  });

  it("renders structured runtime reports and copies replay fixtures", async () => {
    const { copyText } = await import("../../services/clipboard");
    const mutateAsync = vi.fn().mockResolvedValue(replayFixture());
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginRuntimeReportsQuery).mockReturnValue({
      data: [
        runtimeReport({
          trace_id: "trace-report-1",
          duration_ms: 17,
          status: "completed",
        }),
      ],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginExportReplayFixtureMutation).mockReturnValue(
      mutation({ mutateAsync }) as any
    );

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("completed")).toBeInTheDocument();
    expect(screen.getByText("17ms")).toBeInTheDocument();
    expect(screen.getByText("trace-report-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /导出 Replay/ }));

    await waitFor(() => {
      expect(mutateAsync).toHaveBeenCalledWith({
        traceId: "trace-report-1",
        hookName: "gateway.request.afterBodyRead",
        pluginId: "community.prompt-helper",
      });
      expect(copyText).toHaveBeenCalledWith(expect.stringContaining('"traceId": "trace-report-1"'));
      expect(toast.success).toHaveBeenCalledWith("Replay fixture 已复制");
    });
  });

  it("renders audit logs with risk, event, trace, and detail fields", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        runtime_failures: [],
        audit_logs: [
          {
            id: 12,
            plugin_id: "community.prompt-helper",
            trace_id: "trace-audit-1",
            event_type: "plugin.hook.failed",
            risk_level: "high",
            message: "Plugin hook failed closed",
            details: { hookName: "gateway.response.beforeSend", failureKind: "exception" },
            created_at: 42,
          },
        ],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("plugin.hook.failed")).toBeInTheDocument();
    expect(screen.getByText("high")).toBeInTheDocument();
    expect(screen.getByText("trace-audit-1")).toBeInTheDocument();
    expect(screen.getByText("gateway.response.beforeSend")).toBeInTheDocument();
    expect(screen.getByText("exception")).toBeInTheDocument();
  });

  it("shows an empty runtime observability state when no events were recorded", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({ audit_logs: [], runtime_failures: [] }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("还没有记录到插件运行事件")).toBeInTheDocument();
  });

  it("disables plugin actions while config save is pending", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary({ status: "disabled", update_available: true })],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginSaveConfigMutation).mockReturnValue(mutation({ isPending: true }) as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByRole("button", { name: /启用/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /卸载/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /授权待审批权限/ })).toBeDisabled();
  });

  it("uses the generic schema form for official plugin configuration", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [
        summary({
          plugin_id: "official.privacy-filter",
          name: "Privacy Filter",
          runtime: "native:privacyFilter",
        }),
      ],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        summary: summary({
          plugin_id: "official.privacy-filter",
          name: "Privacy Filter",
          runtime: "native:privacyFilter",
        }),
        manifest: {
          ...detail().manifest,
          id: "official.privacy-filter",
          name: "Privacy Filter",
          runtime: { kind: "native", engine: "privacyFilter" },
          permissions: ["request.body.read", "request.body.write", "log.redact"],
          configSchema: {
            type: "object",
            "x-aio-ui": {
              sections: [
                {
                  id: "content",
                  title: "检测策略",
                  description:
                    "这里展示的是可配置的策略大类；密钥类检测由打包的 200+ Gitleaks 规则、上下文规则和熵检测共同支撑。",
                  order: 10,
                },
              ],
            },
            properties: {
              sensitiveTypes: {
                type: "array",
                title: "策略大类",
                description:
                  "这些不是全部底层规则。密钥相关选项会控制打包的 200+ Gitleaks 规则以及上下文/熵检测结果是否生效。",
                items: {
                  type: "string",
                  enum: ["email", "cn_phone"],
                  "x-aio-ui": {
                    enumLabels: { email: "邮箱地址", cn_phone: "中国手机号" },
                  },
                },
                "x-aio-ui": { section: "content", widget: "checkboxGroup", order: 10 },
              },
            },
          },
        },
        install_source: "official",
        config: { sensitiveTypes: ["email", "cn_phone"] },
        granted_permissions: ["request.body.read", "request.body.write", "log.redact"],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("检测策略")).toBeInTheDocument();
    expect(screen.getByText("官方来源")).toBeInTheDocument();
    expect(screen.getAllByText(/200\+ Gitleaks/).length).toBeGreaterThanOrEqual(2);
    expect(screen.getByLabelText("邮箱地址")).toBeChecked();
    expect(screen.queryByLabelText("sensitiveTypes")).not.toBeInTheDocument();
  });

  it("shows empty and error states", () => {
    vi.mocked(usePluginsListQuery).mockReturnValueOnce({
      data: [],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    const { rerender } = renderWithProviders(<PluginsPage />);
    expect(screen.getByText("还没有安装插件")).toBeInTheDocument();

    vi.mocked(usePluginsListQuery).mockReturnValueOnce({
      data: null,
      isLoading: false,
      isFetching: false,
      error: new Error("boom"),
    } as any);
    rerender(
      <QueryClientProvider client={createTestQueryClient()}>
        <MemoryRouter>
          <PluginsPage />
        </MemoryRouter>
      </QueryClientProvider>
    );
    expect(screen.getByText(/插件列表加载失败/)).toBeInTheDocument();
  });

  it("wires import and enable actions", async () => {
    const previewMutation = mutation({ mutateAsync: vi.fn().mockResolvedValue(installPreview()) });
    const importMutation = mutation();
    const installOfficialMutation = mutation();
    const enableMutation = mutation();
    vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(previewMutation as any);
    vi.mocked(usePluginInstallFromFileMutation).mockReturnValue(importMutation as any);
    vi.mocked(usePluginInstallOfficialMutation).mockReturnValue(installOfficialMutation as any);
    vi.mocked(usePluginEnableMutation).mockReturnValue(enableMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/plugin.json");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导入 .aio-plugin" }));

    await screen.findByRole("dialog", { name: "安装前预检" });
    fireEvent.click(screen.getByRole("button", { name: "确认安装" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "安装前预检" })).not.toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: /Privacy Filter/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Safety Detector/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Prompt Optimizer/ })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Sensitive Data Redactor/ })
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Privacy Filter/ }));
    fireEvent.click(screen.getByRole("button", { name: "启用" }));

    await waitFor(() => {
      expect(previewMutation.mutateAsync).toHaveBeenCalledWith("/tmp/plugin.json");
      expect(importMutation.mutateAsync).toHaveBeenCalledWith("/tmp/plugin.json");
      expect(installOfficialMutation.mutateAsync).toHaveBeenCalledWith("official.privacy-filter");
      expect(enableMutation.mutateAsync).toHaveBeenCalledWith("community.prompt-helper");
      expect(toast.success).toHaveBeenCalled();
    });
  });

  it("previews local package before install", async () => {
    const previewMutation = mutation({ mutateAsync: vi.fn().mockResolvedValue(installPreview()) });
    const importMutation = mutation();
    vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(previewMutation as any);
    vi.mocked(usePluginInstallFromFileMutation).mockReturnValue(importMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/prompt-helper.aio-plugin");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导入 .aio-plugin" }));

    const previewDialog = await screen.findByRole("dialog", { name: "安装前预检" });
    expect(within(previewDialog).getByText("Community Prompt Helper")).toBeInTheDocument();
    expect(within(previewDialog).getByText("sha256-install")).toBeInTheDocument();
    expect(within(previewDialog).getByText("gateway.request.afterBodyRead")).toBeInTheDocument();
    expect(
      within(previewDialog).getByText("预检只是解释层，最终安装仍会重新校验。")
    ).toBeInTheDocument();
    expect(importMutation.mutateAsync).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "确认安装" }));

    await waitFor(() => {
      expect(previewMutation.mutateAsync).toHaveBeenCalledWith("/tmp/prompt-helper.aio-plugin");
      expect(importMutation.mutateAsync).toHaveBeenCalledWith("/tmp/prompt-helper.aio-plugin");
      expect(toast.success).toHaveBeenCalledWith("导入插件成功");
    });
  });

  it("blocks install confirmation for destructive preview reasons", async () => {
    const previewMutation = mutation({
      mutateAsync: vi.fn().mockResolvedValue(
        installPreview({
          blockingReasons: [
            {
              severity: "error",
              code: "PLUGIN_UNSIGNED_HIGH_RISK_PERMISSION",
              message: "Unsigned plugin cannot request high-risk permission",
            },
          ],
        })
      ),
    });
    const importMutation = mutation();
    vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(previewMutation as any);
    vi.mocked(usePluginInstallFromFileMutation).mockReturnValue(importMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/risky-plugin.aio-plugin");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导入 .aio-plugin" }));

    const previewDialog = await screen.findByRole("dialog", { name: "安装前预检" });
    const reason = within(previewDialog).getByText(
      "Unsigned plugin cannot request high-risk permission"
    );
    expect(within(previewDialog).getByText("阻断项")).toBeInTheDocument();
    expect(reason.closest(".text-destructive")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认安装" })).toBeDisabled();
    expect(importMutation.mutateAsync).not.toHaveBeenCalled();
  });

  it("approves pending plugin permissions from the detail panel", async () => {
    const grantMutation = mutation();
    vi.mocked(usePluginGrantPermissionsMutation).mockReturnValue(grantMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "授权待审批权限" }));

    await waitFor(() => {
      expect(grantMutation.mutateAsync).toHaveBeenCalledWith({
        pluginId: "community.prompt-helper",
        permissions: ["request.body.write"],
      });
      expect(toast.success).toHaveBeenCalledWith("授权权限成功");
    });
  });

  it("keeps the pending permission action visible when enable fails", async () => {
    const enableMutation = mutation({
      mutateAsync: vi
        .fn()
        .mockRejectedValue(new Error("PLUGIN_PERMISSION_REQUIRED: request.body.write")),
    });
    vi.mocked(usePluginEnableMutation).mockReturnValue(enableMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary()],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "启用" }));

    await waitFor(() => {
      expect(enableMutation.mutateAsync).toHaveBeenCalledWith("community.prompt-helper");
      expect(toast.error).toHaveBeenCalledWith(
        "启用插件失败（code PLUGIN_PERMISSION_REQUIRED）：request.body.write"
      );
    });
    expect(screen.getByRole("button", { name: "授权待审批权限" })).toBeInTheDocument();
  });

  it("does not infer rollback targets from audit details", () => {
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [
        summary({
          plugin_id: "community.redactor",
          name: "Community Redactor",
          current_version: "1.1.0",
        }),
      ],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        summary: summary({
          plugin_id: "community.redactor",
          name: "Community Redactor",
          current_version: "1.1.0",
        }),
        audit_logs: [
          {
            id: 2,
            plugin_id: "community.redactor",
            trace_id: null,
            event_type: "plugin.updated",
            risk_level: "medium",
            message: "Plugin updated",
            details: { fromVersion: "1.0.0" },
            created_at: 40,
          },
        ],
        rollback_versions: [],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.queryByRole("button", { name: "回滚 1.0.0" })).not.toBeInTheDocument();
    expect(screen.getByText("暂无可回滚版本")).toBeInTheDocument();
  });

  it("shows package risk labels and wires update/rollback actions", async () => {
    const previewUpdateMutation = mutation({
      mutateAsync: vi.fn().mockResolvedValue(updateDiff()),
    });
    const updateMutation = mutation();
    const rollbackMutation = mutation();
    vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(previewUpdateMutation as any);
    vi.mocked(usePluginUpdateFromFileMutation).mockReturnValue(updateMutation as any);
    vi.mocked(usePluginRollbackMutation).mockReturnValue(rollbackMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [
        summary({
          plugin_id: "community.redactor",
          name: "Community Redactor",
          status: "update_available",
          update_available: true,
          permission_risk: "critical",
        }),
        summary({
          plugin_id: "community.revoked",
          name: "Revoked Plugin",
          status: "quarantined",
          update_available: false,
          last_error: "revoked by market",
        }),
      ],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        summary: summary({
          plugin_id: "community.redactor",
          name: "Community Redactor",
          current_version: "1.1.0",
          status: "update_available",
          permission_risk: "critical",
          update_available: true,
        }),
        install_source: "offline",
        audit_logs: [
          {
            id: 2,
            plugin_id: "community.redactor",
            trace_id: null,
            event_type: "plugin.installed",
            risk_level: "high",
            message: "Local plugin package installed",
            details: { unsigned: true, fromVersion: "1.0.0" },
            created_at: 40,
          },
        ],
        rollback_versions: ["1.0.0"],
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/community-redactor-1.1.0.aio-plugin");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getAllByText("Community Redactor")[0]);
    const lifecyclePanel = screen.getByText("生命周期").closest("section");
    expect(lifecyclePanel).not.toBeNull();
    expect(within(lifecyclePanel as HTMLElement).getByText("当前版本")).toBeInTheDocument();
    expect(within(lifecyclePanel as HTMLElement).getByText("1.1.0")).toBeInTheDocument();
    expect(within(lifecyclePanel as HTMLElement).getByText("有可用更新")).toBeInTheDocument();
    expect(within(lifecyclePanel as HTMLElement).getByText("最后更新")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "更新" }));
    await screen.findByRole("dialog", { name: "更新预检" });
    fireEvent.click(screen.getByRole("button", { name: "确认更新" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "更新预检" })).not.toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "回滚 1.0.0" }));

    await waitFor(() => {
      expect(screen.getAllByText("未签名").length).toBeGreaterThan(0);
      expect(screen.getByText("已隔离")).toBeInTheDocument();
      expect(screen.getByText("revoked by market")).toBeInTheDocument();
      expect(previewUpdateMutation.mutateAsync).toHaveBeenCalledWith(
        "/tmp/community-redactor-1.1.0.aio-plugin"
      );
      expect(updateMutation.mutateAsync).toHaveBeenCalledWith(
        "/tmp/community-redactor-1.1.0.aio-plugin"
      );
      expect(rollbackMutation.mutateAsync).toHaveBeenCalledWith({
        pluginId: "community.redactor",
        version: "1.0.0",
      });
    });
  });

  it("shows update diff before applying update", async () => {
    const previewUpdateMutation = mutation({
      mutateAsync: vi.fn().mockResolvedValue(
        updateDiff({
          warnings: [
            {
              severity: "warning",
              code: "PLUGIN_MARKET_REVOKED",
              message: "Plugin revoked by market index",
            },
          ],
        })
      ),
    });
    const updateMutation = mutation();
    vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(previewUpdateMutation as any);
    vi.mocked(usePluginUpdateFromFileMutation).mockReturnValue(updateMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary({ update_available: true })],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({
        summary: summary({ update_available: true }),
      }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/prompt-helper-1.1.0.aio-plugin");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "更新" }));

    const updateDialog = await screen.findByRole("dialog", { name: "更新预检" });
    expect(within(updateDialog).getByText("1.0.0 -> 1.1.0")).toBeInTheDocument();
    expect(within(updateDialog).getByText("gateway.response.beforeSend")).toBeInTheDocument();
    expect(within(updateDialog).getByText("新增，待授权")).toBeInTheDocument();
    expect(within(updateDialog).getByText("隔离与撤销")).toBeInTheDocument();
    expect(within(updateDialog).getByText("Plugin revoked by market index")).toBeInTheDocument();
    expect(updateMutation.mutateAsync).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "确认更新" }));

    await waitFor(() => {
      expect(previewUpdateMutation.mutateAsync).toHaveBeenCalledWith(
        "/tmp/prompt-helper-1.1.0.aio-plugin"
      );
      expect(updateMutation.mutateAsync).toHaveBeenCalledWith(
        "/tmp/prompt-helper-1.1.0.aio-plugin"
      );
      expect(toast.success).toHaveBeenCalledWith("更新插件成功");
    });
  });

  it("keeps blocking revocation notices visually distinct in update preview", async () => {
    const previewUpdateMutation = mutation({
      mutateAsync: vi.fn().mockResolvedValue(
        updateDiff({
          blockingReasons: [
            {
              severity: "error",
              code: "PLUGIN_MARKET_REVOKED",
              message: "Plugin revoked by market index",
            },
          ],
        })
      ),
    });
    const updateMutation = mutation();
    vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(previewUpdateMutation as any);
    vi.mocked(usePluginUpdateFromFileMutation).mockReturnValue(updateMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [summary({ update_available: true })],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(usePluginQuery).mockReturnValue({
      data: detail({ summary: summary({ update_available: true }) }),
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/revoked-update.aio-plugin");

    renderWithProviders(<PluginsPage />);
    fireEvent.click(screen.getByRole("button", { name: "更新" }));

    const updateDialog = await screen.findByRole("dialog", { name: "更新预检" });
    expect(within(updateDialog).getByText("隔离/撤销阻断项")).toBeInTheDocument();
    expect(within(updateDialog).getByText("Plugin revoked by market index")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认更新" })).toBeDisabled();
    expect(updateMutation.mutateAsync).not.toHaveBeenCalled();
  });

  it("does not offer enable action for quarantined or uninstalled plugins", () => {
    const enableMutation = mutation();
    vi.mocked(usePluginEnableMutation).mockReturnValue(enableMutation as any);
    vi.mocked(usePluginsListQuery).mockReturnValue({
      data: [
        summary({
          plugin_id: "community.revoked",
          name: "Revoked Plugin",
          status: "quarantined",
          last_error: "revoked by market",
        }),
        summary({
          plugin_id: "community.removed",
          name: "Removed Plugin",
          status: "uninstalled",
        }),
      ],
      isLoading: false,
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<PluginsPage />);

    expect(screen.getByText("已隔离")).toBeInTheDocument();
    expect(screen.getByText("已卸载")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "启用" })).not.toBeInTheDocument();
    expect(enableMutation.mutateAsync).not.toHaveBeenCalled();
  });
});
