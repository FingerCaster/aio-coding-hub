// Usage: React Query adapters for community plugin management.

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  normalizePluginId,
  pluginDisable,
  pluginEnable,
  pluginGet,
  pluginGrantPermissions,
  pluginInstallFromFile,
  pluginInstallRemote,
  pluginInstallOfficial,
  pluginList,
  pluginListAuditLogs,
  pluginListRuntimeReports,
  pluginPreviewFromFile,
  pluginPreviewUpdateFromFile,
  pluginExportReplayFixture,
  pluginQuarantineRevoked,
  pluginRevokePermission,
  pluginRollback,
  pluginSaveConfig,
  pluginUninstall,
  pluginUpdateFromFile,
  type JsonValue,
  type PluginDetail,
  type PluginSummary,
} from "../services/plugins";
import { pluginKeys } from "./keys";

type QueryClientLike = ReturnType<typeof useQueryClient>;

function refreshPluginQueries(queryClient: QueryClientLike, pluginId: string) {
  queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
  queryClient.invalidateQueries({ queryKey: pluginKeys.detail(pluginId) });
}

function upsertPluginSummary(
  current: PluginSummary[] | undefined,
  detail: PluginDetail
): PluginSummary[] {
  const previous = current ?? [];
  const nextSummary = detail.summary;
  const exists = previous.some((item) => item.plugin_id === nextSummary.plugin_id);
  if (exists) {
    return previous.map((item) => (item.plugin_id === nextSummary.plugin_id ? nextSummary : item));
  }
  return [nextSummary, ...previous];
}

export function usePluginsListQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: pluginKeys.list(),
    queryFn: () => pluginList(),
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
  });
}

export function usePluginQuery(pluginId: string | null, options?: { enabled?: boolean }) {
  const normalizedPluginId = pluginId == null ? null : normalizePluginId(pluginId);

  return useQuery({
    queryKey: pluginKeys.detail(normalizedPluginId),
    queryFn: () => {
      if (normalizedPluginId == null) return null;
      return pluginGet(normalizedPluginId);
    },
    enabled: normalizedPluginId != null && (options?.enabled ?? true),
    placeholderData: keepPreviousData,
  });
}

export function usePluginAuditLogsQuery(
  pluginId: string | null,
  limit = 50,
  options?: { enabled?: boolean }
) {
  const normalizedPluginId = pluginId == null ? null : normalizePluginId(pluginId);

  return useQuery({
    queryKey: pluginKeys.auditLogs(normalizedPluginId, limit),
    queryFn: () => pluginListAuditLogs({ pluginId: normalizedPluginId, limit }),
    enabled: (options?.enabled ?? true) && normalizedPluginId != null,
    placeholderData: keepPreviousData,
  });
}

export function usePluginRuntimeReportsQuery(
  input: {
    pluginId: string | null;
    hookName?: string | null;
    traceId?: string | null;
    limit?: number | null;
  },
  options?: { enabled?: boolean }
) {
  const normalizedPluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
  const hookName = input.hookName ?? null;
  const traceId = input.traceId ?? null;
  const limit = input.limit ?? 50;

  return useQuery({
    queryKey: pluginKeys.runtimeReports(normalizedPluginId, hookName, traceId, limit),
    queryFn: () =>
      pluginListRuntimeReports({
        pluginId: normalizedPluginId,
        hookName,
        traceId,
        limit,
      }),
    enabled: (options?.enabled ?? true) && normalizedPluginId != null,
    placeholderData: keepPreviousData,
  });
}

export function usePluginExportReplayFixtureMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { traceId: string; hookName: string; pluginId?: string | null }) =>
      pluginExportReplayFixture(input),
    onSuccess: (fixture, input) => {
      const pluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
      queryClient.setQueryData(
        pluginKeys.replayFixture(fixture.traceId, fixture.hookName, pluginId),
        fixture
      );
    },
  });
}

export function usePluginInstallFromFileMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (filePath: string) => pluginInstallFromFile(filePath),
    onSuccess: (next) => {
      if (!next) return;
      queryClient.setQueryData<PluginSummary[]>(pluginKeys.list(), (current) =>
        upsertPluginSummary(current, next)
      );
      queryClient.setQueryData(pluginKeys.detail(next.summary.plugin_id), next);
      refreshPluginQueries(queryClient, next.summary.plugin_id);
    },
  });
}

export function usePluginPreviewFromFileMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (filePath: string) => pluginPreviewFromFile(filePath),
    onSuccess: (next, filePath) => {
      queryClient.setQueryData(pluginKeys.installPreview(filePath), next);
    },
  });
}

export function usePluginPreviewUpdateFromFileMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (filePath: string) => pluginPreviewUpdateFromFile(filePath),
    onSuccess: (next, filePath) => {
      queryClient.setQueryData(pluginKeys.updatePreview(filePath), next);
    },
  });
}

export function usePluginUpdateFromFileMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (filePath: string) => pluginUpdateFromFile(filePath),
    onSuccess: (next) => {
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(next.summary.plugin_id), next);
        refreshPluginQueries(queryClient, next.summary.plugin_id);
      } else {
        queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      }
    },
  });
}

export function usePluginInstallRemoteMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: Parameters<typeof pluginInstallRemote>[0]) => pluginInstallRemote(input),
    onSuccess: (next) => {
      if (!next) return;
      queryClient.setQueryData<PluginSummary[]>(pluginKeys.list(), (current) =>
        upsertPluginSummary(current, next)
      );
      queryClient.setQueryData(pluginKeys.detail(next.summary.plugin_id), next);
      refreshPluginQueries(queryClient, next.summary.plugin_id);
    },
  });
}

export function usePluginRollbackMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { pluginId: string; version: string }) =>
      pluginRollback(input.pluginId, input.version),
    onSuccess: (next, input) => {
      const normalizedPluginId = normalizePluginId(input.pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginQuarantineRevokedMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pluginId: string) => pluginQuarantineRevoked(pluginId),
    onSuccess: (next, pluginId) => {
      const normalizedPluginId = normalizePluginId(pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginInstallOfficialMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pluginId: string) => pluginInstallOfficial(pluginId),
    onSuccess: (next, pluginId) => {
      const normalizedPluginId = normalizePluginId(pluginId);
      if (next) {
        queryClient.setQueryData<PluginSummary[]>(pluginKeys.list(), (current) =>
          upsertPluginSummary(current, next)
        );
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginEnableMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pluginId: string) => pluginEnable(pluginId),
    onSuccess: (next, pluginId) => {
      const normalizedPluginId = normalizePluginId(pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginDisableMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pluginId: string) => pluginDisable(pluginId),
    onSuccess: (next, pluginId) => {
      const normalizedPluginId = normalizePluginId(pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginUninstallMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (pluginId: string) => pluginUninstall(pluginId),
    onSuccess: (next, pluginId) => {
      const normalizedPluginId = normalizePluginId(pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginSaveConfigMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { pluginId: string; config: JsonValue }) =>
      pluginSaveConfig(input.pluginId, input.config),
    onSuccess: (next, input) => {
      const normalizedPluginId = normalizePluginId(input.pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginGrantPermissionsMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { pluginId: string; permissions: readonly string[] }) =>
      pluginGrantPermissions(input.pluginId, input.permissions),
    onSuccess: (next, input) => {
      const normalizedPluginId = normalizePluginId(input.pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}

export function usePluginRevokePermissionMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { pluginId: string; permission: string }) =>
      pluginRevokePermission(input.pluginId, input.permission),
    onSuccess: (next, input) => {
      const normalizedPluginId = normalizePluginId(input.pluginId);
      if (next) {
        queryClient.setQueryData(pluginKeys.detail(normalizedPluginId), next);
      }
      refreshPluginQueries(queryClient, normalizedPluginId);
    },
  });
}
