import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PluginDetail, PluginSummary } from "../../services/plugins";
import {
  pluginDisable,
  pluginEnable,
  pluginGet,
  pluginInstallRemote,
  pluginInstallOfficial,
  pluginList,
  pluginListRuntimeReports,
  pluginExportReplayFixture,
  pluginQuarantineRevoked,
  pluginRevokePermission,
  pluginRollback,
  pluginSaveConfig,
  pluginUninstall,
  pluginUpdateFromFile,
} from "../../services/plugins";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { pluginKeys } from "../keys";
import {
  usePluginDisableMutation,
  usePluginEnableMutation,
  usePluginInstallOfficialMutation,
  usePluginInstallRemoteMutation,
  usePluginQuery,
  usePluginQuarantineRevokedMutation,
  usePluginExportReplayFixtureMutation,
  usePluginRevokePermissionMutation,
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
    pluginInstallRemote: vi.fn(),
    pluginInstallOfficial: vi.fn(),
    pluginListRuntimeReports: vi.fn(),
    pluginExportReplayFixture: vi.fn(),
    pluginQuarantineRevoked: vi.fn(),
    pluginUpdateFromFile: vi.fn(),
    pluginRollback: vi.fn(),
    pluginDisable: vi.fn(),
    pluginUninstall: vi.fn(),
    pluginSaveConfig: vi.fn(),
    pluginRevokePermission: vi.fn(),
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

function officialPrivacyFilterDetail(): PluginDetail {
  return detail({
    summary: summary({
      plugin_id: "official.privacy-filter",
      name: "Privacy Filter",
      runtime: "native:privacyFilter",
    }),
    manifest: {
      id: "official.privacy-filter",
      name: "Privacy Filter",
      version: "1.0.0",
      apiVersion: "1.0.0",
      runtime: { kind: "native", engine: "privacyFilter" },
      hooks: [
        { name: "gateway.request.afterBodyRead", priority: 10 },
        { name: "log.beforePersist", priority: 10 },
      ],
      permissions: ["request.body.read", "request.body.write", "log.redact"],
      hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    },
    install_source: "official",
    granted_permissions: ["request.body.read", "request.body.write", "log.redact"],
  });
}

describe("query/plugins", () => {
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

  it("queries runtime reports and caches exported replay fixtures", async () => {
    vi.mocked(pluginListRuntimeReports).mockResolvedValue([
      {
        id: 1,
        plugin_id: "community.prompt-helper",
        trace_id: "trace-replay-1",
        hook_name: "gateway.request.afterBodyRead",
        runtime_kind: "declarativeRules",
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

  it("invalidates list and detail queries after mutations", async () => {
    const next = detail({ summary: summary({ status: "enabled" }) });
    vi.mocked(pluginEnable).mockResolvedValue(next);
    vi.mocked(pluginInstallRemote).mockResolvedValue(next);
    vi.mocked(pluginInstallOfficial).mockResolvedValue(officialPrivacyFilterDetail());
    vi.mocked(pluginQuarantineRevoked).mockResolvedValue(next);
    vi.mocked(pluginUpdateFromFile).mockResolvedValue(next);
    vi.mocked(pluginRollback).mockResolvedValue(next);
    vi.mocked(pluginDisable).mockResolvedValue(next);
    vi.mocked(pluginUninstall).mockResolvedValue(next);
    vi.mocked(pluginSaveConfig).mockResolvedValue(next);
    vi.mocked(pluginRevokePermission).mockResolvedValue(next);

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result: enableResult } = renderHook(() => usePluginEnableMutation(), { wrapper });
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
    const { result: revokePermissionResult } = renderHook(
      () => usePluginRevokePermissionMutation(),
      {
        wrapper,
      }
    );

    await act(async () => {
      await enableResult.current.mutateAsync("community.prompt-helper");
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
      await revokePermissionResult.current.mutateAsync({
        pluginId: "community.prompt-helper",
        permission: "request.body.write",
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: pluginKeys.list() });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.detail("community.prompt-helper"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: pluginKeys.detail("official.privacy-filter"),
    });
  });
});
