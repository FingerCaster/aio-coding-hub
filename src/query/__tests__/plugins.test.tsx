import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ActiveContributionSnapshot } from "../../services/pluginContributions";
import { pluginActiveContributions } from "../../services/pluginContributions";
import type { PluginDetail, PluginSummary } from "../../services/plugins";
import {
  pluginDisable,
  pluginEnable,
  pluginExecuteCommand,
  pluginGet,
  pluginInstallFromFile,
  pluginInstallRemote,
  pluginInstallOfficial,
  pluginListAuditLogs,
  pluginListExtensionRuntimeReports,
  pluginList,
  pluginListRuntimeReports,
  pluginPreviewFromFile,
  pluginPreviewRemoteUpdate,
  pluginPreviewUpdateFromFile,
  pluginExportReplayFixture,
  pluginQuarantineRevoked,
  pluginRollback,
  pluginSaveConfig,
  pluginUninstall,
  pluginUpdateFromFile,
} from "../../services/plugins";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { pluginContributionKeys, pluginKeys } from "../keys";
import * as pluginQueries from "../plugins";
import {
  usePluginActiveContributionsQuery,
  usePluginDisableMutation,
  usePluginEnableMutation,
  usePluginExecuteCommandMutation,
  usePluginExtensionRuntimeReportsQuery,
  usePluginInstallFromFileMutation,
  usePluginInstallOfficialMutation,
  usePluginInstallRemoteMutation,
  usePluginAuditLogsQuery,
  usePluginPreviewFromFileMutation,
  usePluginPreviewRemoteUpdateMutation,
  usePluginPreviewUpdateFromFileMutation,
  usePluginQuery,
  usePluginQuarantineRevokedMutation,
  usePluginExportReplayFixtureMutation,
  usePluginRollbackMutation,
  usePluginRuntimeReportsQuery,
  usePluginsListQuery,
  usePluginSaveConfigMutation,
  usePluginUninstallMutation,
  usePluginUpdateFromFileMutation,
} from "../plugins";

vi.mock("../../services/plugins", async () => {
  const actual =
    await vi.importActual<typeof import("../../services/plugins")>("../../services/plugins");
  return {
    ...actual,
    pluginList: vi.fn(),
    pluginGet: vi.fn(),
    pluginEnable: vi.fn(),
    pluginExecuteCommand: vi.fn(),
    pluginInstallFromFile: vi.fn(),
    pluginInstallRemote: vi.fn(),
    pluginInstallOfficial: vi.fn(),
    pluginListAuditLogs: vi.fn(),
    pluginListExtensionRuntimeReports: vi.fn(),
    pluginListRuntimeReports: vi.fn(),
    pluginPreviewFromFile: vi.fn(),
    pluginPreviewRemoteUpdate: vi.fn(),
    pluginPreviewUpdateFromFile: vi.fn(),
    pluginExportReplayFixture: vi.fn(),
    pluginQuarantineRevoked: vi.fn(),
    pluginUpdateFromFile: vi.fn(),
    pluginRollback: vi.fn(),
    pluginDisable: vi.fn(),
    pluginUninstall: vi.fn(),
    pluginSaveConfig: vi.fn(),
  };
});

vi.mock("../../services/pluginContributions", () => ({
  pluginActiveContributions: vi.fn(),
}));

function summary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: 1,
    plugin_id: "community.prompt-helper",
    name: "Community Prompt Helper",
    current_version: "1.0.0",
    status: "disabled",
    runtime: "extensionHost",
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
      runtime: { kind: "extensionHost", language: "typescript" },
      main: "dist/extension.js",
      hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
      permissions: ["request.body.read"],
      hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    },
    install_source: "local",
    installed_dir: null,
    config: {},
    granted_permissions: [],
    pending_permissions: [],
    audit_logs: [],
    runtime_failures: [],
    rollback_versions: [],
    ...overrides,
  };
}

function officialPrivacyFilterDetail(overrides: Partial<PluginDetail> = {}): PluginDetail {
  return detail({
    summary: summary({
      plugin_id: "official.privacy-filter",
      name: "Privacy Filter",
      runtime: "extensionHost",
    }),
    manifest: {
      id: "official.privacy-filter",
      name: "Privacy Filter",
      version: "1.0.0",
      apiVersion: "1.0.0",
      runtime: { kind: "extensionHost", language: "typescript" },
      main: "dist/extension.js",
      activationEvents: [
        "onGatewayHook:gateway.request.afterBodyRead",
        "onGatewayHook:gateway.request.beforeSend",
        "onGatewayHook:log.beforePersist",
      ],
      capabilities: ["gateway.hooks", "privacy.redact"],
      contributes: {
        gatewayHooks: [
          {
            name: "gateway.request.afterBodyRead",
            priority: 5,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
          {
            name: "gateway.request.beforeSend",
            priority: 5,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
          {
            name: "log.beforePersist",
            priority: 1,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
        ],
      },
      hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    },
    install_source: "official",
    granted_permissions: ["request.body.read", "request.body.write", "log.redact"],
    ...overrides,
  });
}

function activeContributions(
  overrides: Partial<ActiveContributionSnapshot> = {}
): ActiveContributionSnapshot {
  return {
    ui: [],
    providers: [],
    protocols: [],
    protocolBridges: [],
    commands: [],
    gatewayHooks: [],
    ...overrides,
  };
}

describe("query/plugins", () => {
  it("does not expose manual permission mutation hooks", () => {
    expect("usePluginGrantPermissionsMutation" in pluginQueries).toBe(false);
    expect("usePluginRevokePermissionMutation" in pluginQueries).toBe(false);
  });

  it("uses stable list and detail query keys", async () => {
    vi.mocked(pluginList).mockResolvedValue([summary()]);
    vi.mocked(pluginGet).mockResolvedValue(detail());
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => usePluginsListQuery(), { wrapper });
    renderHook(() => usePluginQuery(" community.prompt-helper "), { wrapper });

    await waitFor(() => {
      expect(pluginList).toHaveBeenCalled();
      expect(pluginGet).toHaveBeenCalledWith("community.prompt-helper");
    });

    expect(client.getQueryState(pluginKeys.list())).toBeTruthy();
    expect(client.getQueryState(pluginKeys.detail("community.prompt-helper"))).toBeTruthy();
  });

  it("uses disabled query guards and default plugin report filters", async () => {
    vi.mocked(pluginListRuntimeReports).mockResolvedValue([]);
    vi.mocked(pluginListExtensionRuntimeReports).mockResolvedValue([]);
    vi.mocked(pluginListAuditLogs).mockResolvedValue([]);
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => usePluginsListQuery({ enabled: false }), { wrapper });
    renderHook(() => usePluginQuery(null), { wrapper });
    renderHook(() => usePluginQuery("community.prompt-helper", { enabled: false }), { wrapper });
    renderHook(() => usePluginRuntimeReportsQuery({ pluginId: null }), { wrapper });
    renderHook(
      () =>
        usePluginExtensionRuntimeReportsQuery({
          pluginId: " community.prompt-helper ",
          contributionType: null,
          contributionId: null,
          traceId: null,
          limit: null,
        }),
      { wrapper }
    );
    renderHook(() => usePluginAuditLogsQuery(" community.prompt-helper "), { wrapper });

    await waitFor(() => {
      expect(pluginListExtensionRuntimeReports).toHaveBeenCalledWith({
        pluginId: "community.prompt-helper",
        contributionType: null,
        contributionId: null,
        traceId: null,
        limit: 50,
      });
      expect(pluginListAuditLogs).toHaveBeenCalledWith({
        pluginId: "community.prompt-helper",
        limit: 50,
      });
    });

    expect(pluginList).not.toHaveBeenCalled();
    expect(pluginGet).not.toHaveBeenCalled();
    expect(pluginListRuntimeReports).not.toHaveBeenCalled();
    expect(client.getQueryState(pluginKeys.detail(null))).toBeTruthy();
    expect(client.getQueryState(pluginKeys.runtimeReports(null, null, null, 50))).toBeTruthy();
  });

  it("queries runtime reports and caches previews and exported replay fixtures", async () => {
    vi.mocked(pluginListRuntimeReports).mockResolvedValue([
      {
        id: 1,
        plugin_id: "community.prompt-helper",
        trace_id: "trace-replay-1",
        hook_name: "gateway.request.afterBodyRead",
        runtime_kind: "extensionHost",
        status: "completed",
        started_at_ms: 1000,
        duration_ms: 7,
        failure_kind: null,
        error_code: null,
        failure_policy: "fail-open",
        circuit_state: "closed",
        context_budget: {},
        output_budget: {},
        mutation_summary: { changed: true },
        replayable: true,
        replay_export_reason: null,
        created_at: 10,
      },
    ]);
    vi.mocked(pluginListExtensionRuntimeReports).mockResolvedValue([
      {
        id: 1,
        pluginId: "community.prompt-helper",
        traceId: "trace-replay-1",
        contributionType: "hook",
        contributionId: "gateway.request.afterBodyRead",
        commandOrHook: "gateway.request.afterBodyRead",
        status: "completed",
        startedAtMs: 1000,
        durationMs: 7,
        failureKind: null,
        errorCode: null,
        inputBudget: {},
        outputBudget: {},
        mutationSummary: { changed: true },
        replayable: true,
        createdAt: 10,
      },
    ]);
    vi.mocked(pluginExecuteCommand).mockResolvedValue({ ok: true });
    vi.mocked(pluginPreviewFromFile).mockResolvedValue({ pluginId: "from-file" } as any);
    vi.mocked(pluginPreviewUpdateFromFile).mockResolvedValue({ pluginId: "update-file" } as any);
    vi.mocked(pluginPreviewRemoteUpdate).mockResolvedValue({ pluginId: "remote-update" } as any);
    vi.mocked(pluginExportReplayFixture).mockResolvedValue({
      schemaVersion: 1,
      traceId: "trace-replay-1",
      source: {
        appVersion: "0.62.3",
        traceId: "trace-replay-1",
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
      notes: [],
    });
    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    renderHook(
      () =>
        usePluginRuntimeReportsQuery({
          pluginId: " community.prompt-helper ",
          hookName: "gateway.request.afterBodyRead",
          traceId: "trace-replay-1",
          limit: 25,
        }),
      { wrapper }
    );

    await waitFor(() => {
      expect(pluginListRuntimeReports).toHaveBeenCalledWith({
        pluginId: "community.prompt-helper",
        hookName: "gateway.request.afterBodyRead",
        traceId: "trace-replay-1",
        limit: 25,
      });
    });
    expect(
      client.getQueryState(
        pluginKeys.runtimeReports(
          "community.prompt-helper",
          "gateway.request.afterBodyRead",
          "trace-replay-1",
          25
        )
      )
    ).toBeTruthy();

    renderHook(
      () =>
        usePluginExtensionRuntimeReportsQuery({
          pluginId: " community.prompt-helper ",
          contributionType: "hook",
          contributionId: "gateway.request.afterBodyRead",
          traceId: "trace-replay-1",
          limit: 25,
        }),
      { wrapper }
    );

    await waitFor(() => {
      expect(pluginListExtensionRuntimeReports).toHaveBeenCalledWith({
        pluginId: "community.prompt-helper",
        contributionType: "hook",
        contributionId: "gateway.request.afterBodyRead",
        traceId: "trace-replay-1",
        limit: 25,
      });
    });
    expect(
      client.getQueryState(
        pluginKeys.extensionRuntimeReports(
          "community.prompt-helper",
          "hook",
          "gateway.request.afterBodyRead",
          "trace-replay-1",
          25
        )
      )
    ).toBeTruthy();

    const { result: commandResult } = renderHook(() => usePluginExecuteCommandMutation(), {
      wrapper,
    });
    await act(async () => {
      await commandResult.current.mutateAsync({
        command: "community.prompt-helper.open",
        args: { pluginId: "community.prompt-helper" },
      });
    });

    expect(pluginExecuteCommand).toHaveBeenCalledWith("community.prompt-helper.open", {
      pluginId: "community.prompt-helper",
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.extensionRuntimeReportsRoot(),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.detail("community.prompt-helper"),
    });

    await act(async () => {
      await commandResult.current.mutateAsync({
        command: "community.prompt-helper.noop",
        args: ["not-object"] as any,
      });
      await commandResult.current.mutateAsync({
        command: "community.prompt-helper.empty",
        args: { pluginId: "   " } as any,
      });
      await commandResult.current.mutateAsync({
        command: "community.prompt-helper.null",
      });
    });

    expect(invalidateSpy).toHaveBeenCalledTimes(5);

    const { result: previewFromFileResult } = renderHook(() => usePluginPreviewFromFileMutation(), {
      wrapper,
    });
    const { result: previewUpdateFromFileResult } = renderHook(
      () => usePluginPreviewUpdateFromFileMutation(),
      { wrapper }
    );
    const { result: previewRemoteUpdateResult } = renderHook(
      () => usePluginPreviewRemoteUpdateMutation(),
      { wrapper }
    );
    await act(async () => {
      await previewFromFileResult.current.mutateAsync("/tmp/plugin.aio-plugin");
      await previewUpdateFromFileResult.current.mutateAsync("/tmp/plugin-update.aio-plugin");
      await previewRemoteUpdateResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        downloadUrl: "https://github.com/acme/plugin/releases/download/v2/plugin.aio-plugin",
        checksum: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      });
    });

    expect(client.getQueryData(pluginKeys.installPreview("/tmp/plugin.aio-plugin"))).toEqual({
      pluginId: "from-file",
    });
    expect(client.getQueryData(pluginKeys.updatePreview("/tmp/plugin-update.aio-plugin"))).toEqual({
      pluginId: "update-file",
    });
    expect(
      client.getQueryData(
        pluginKeys.updatePreview(
          "https://github.com/acme/plugin/releases/download/v2/plugin.aio-plugin"
        )
      )
    ).toEqual({ pluginId: "remote-update" });

    const { result } = renderHook(() => usePluginExportReplayFixtureMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        pluginId: " community.prompt-helper ",
        hookName: "gateway.request.afterBodyRead",
        traceId: "trace-replay-1",
      });
    });

    expect(
      client.getQueryData(
        pluginKeys.replayFixture(
          "trace-replay-1",
          "gateway.request.afterBodyRead",
          "community.prompt-helper"
        )
      )
    ).toMatchObject({ traceId: "trace-replay-1" });
  });

  it("queries active plugin contributions with a stable key", async () => {
    vi.mocked(pluginActiveContributions).mockResolvedValue(
      activeContributions({
        commands: [
          {
            pluginId: "community.prompt-helper",
            command: "community.prompt-helper.open",
            title: "Open",
            category: null,
          },
        ],
      })
    );
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => usePluginActiveContributionsQuery(), { wrapper });

    await waitFor(() => {
      expect(pluginActiveContributions).toHaveBeenCalled();
    });

    expect(client.getQueryState(pluginContributionKeys.active())).toBeTruthy();
  });

  it("invalidates list and detail queries after mutations", async () => {
    const next = detail({ summary: summary({ status: "enabled" }) });
    vi.mocked(pluginEnable).mockResolvedValue(next);
    vi.mocked(pluginInstallFromFile).mockResolvedValue(next);
    vi.mocked(pluginInstallRemote).mockResolvedValue(next);
    vi.mocked(pluginInstallOfficial).mockResolvedValue(officialPrivacyFilterDetail());
    vi.mocked(pluginQuarantineRevoked).mockResolvedValue(next);
    vi.mocked(pluginUpdateFromFile).mockResolvedValue(next);
    vi.mocked(pluginRollback).mockResolvedValue(next);
    vi.mocked(pluginDisable).mockResolvedValue(next);
    vi.mocked(pluginUninstall).mockResolvedValue(next);
    vi.mocked(pluginSaveConfig).mockResolvedValue(next);

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result: enableResult } = renderHook(() => usePluginEnableMutation(), { wrapper });
    const { result: installFromFileResult } = renderHook(() => usePluginInstallFromFileMutation(), {
      wrapper,
    });
    const { result: installOfficialResult } = renderHook(() => usePluginInstallOfficialMutation(), {
      wrapper,
    });
    const { result: installRemoteResult } = renderHook(() => usePluginInstallRemoteMutation(), {
      wrapper,
    });
    const { result: quarantineRevokedResult } = renderHook(
      () => usePluginQuarantineRevokedMutation(),
      {
        wrapper,
      }
    );
    const { result: disableResult } = renderHook(() => usePluginDisableMutation(), { wrapper });
    const { result: uninstallResult } = renderHook(() => usePluginUninstallMutation(), { wrapper });
    const { result: updateResult } = renderHook(() => usePluginUpdateFromFileMutation(), {
      wrapper,
    });
    const { result: rollbackResult } = renderHook(() => usePluginRollbackMutation(), { wrapper });
    const { result: saveConfigResult } = renderHook(() => usePluginSaveConfigMutation(), {
      wrapper,
    });
    await act(async () => {
      await enableResult.current.mutateAsync("community.prompt-helper");
      await installFromFileResult.current.mutateAsync("/tmp/plugin.aio-plugin");
      await installRemoteResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        downloadUrl: "https://github.com/acme/plugin/releases/download/v1/plugin.aio-plugin",
        checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      });
      await installOfficialResult.current.mutateAsync("official.privacy-filter");
      await quarantineRevokedResult.current.mutateAsync("community.prompt-helper");
      await disableResult.current.mutateAsync("community.prompt-helper");
      await uninstallResult.current.mutateAsync("community.prompt-helper");
      await updateResult.current.mutateAsync("/tmp/plugin-update.aio-plugin");
      await rollbackResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        version: "1.0.0",
      });
      await saveConfigResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        config: { mode: "append_instruction" },
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: pluginKeys.list() });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.detail("community.prompt-helper"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.detail("official.privacy-filter"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginContributionKeys.active(),
    });
  });

  it("invalidates broad plugin state when install or update returns no detail", async () => {
    vi.mocked(pluginInstallFromFile).mockResolvedValue(null as any);
    vi.mocked(pluginInstallRemote).mockResolvedValue(null as any);
    vi.mocked(pluginUpdateFromFile).mockResolvedValue(null as any);
    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);
    const { result: installFromFileResult } = renderHook(() => usePluginInstallFromFileMutation(), {
      wrapper,
    });
    const { result: installRemoteResult } = renderHook(() => usePluginInstallRemoteMutation(), {
      wrapper,
    });
    const { result: updateFromFileResult } = renderHook(() => usePluginUpdateFromFileMutation(), {
      wrapper,
    });

    await act(async () => {
      await installFromFileResult.current.mutateAsync("/tmp/missing.aio-plugin");
      await installRemoteResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        downloadUrl: "https://github.com/acme/plugin/releases/download/v3/plugin.aio-plugin",
        checksum: "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      });
      await updateFromFileResult.current.mutateAsync("/tmp/no-update.aio-plugin");
    });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: pluginKeys.list() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: pluginContributionKeys.active() });
    expect(client.getQueryData(pluginKeys.detail("community.prompt-helper"))).toBeUndefined();
  });

  it("updates the list cache with the returned summary after enabling a plugin", async () => {
    const client = createTestQueryClient();
    client.setQueryData<PluginSummary[]>(pluginKeys.list(), [
      summary({
        plugin_id: "official.privacy-filter",
        name: "Privacy Filter",
        status: "installed",
      }),
    ]);
    vi.mocked(pluginEnable).mockResolvedValue(
      officialPrivacyFilterDetail({
        summary: summary({
          plugin_id: "official.privacy-filter",
          name: "Privacy Filter",
          status: "enabled",
        }),
      })
    );
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => usePluginEnableMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("official.privacy-filter");
    });

    expect(client.getQueryData<PluginSummary[]>(pluginKeys.list())).toEqual([
      expect.objectContaining({
        plugin_id: "official.privacy-filter",
        status: "enabled",
      }),
    ]);
  });
});
