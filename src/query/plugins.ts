// Usage: React Query adapters for community plugin management.

import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  normalizePluginId,
  pluginDisable,
  pluginEnable,
  pluginExecuteCommand,
  pluginGet,
  pluginInstallFromFile,
  pluginInstallRemote,
  pluginInstallOfficial,
  pluginList,
  pluginListAuditLogs,
  pluginListExtensionRuntimeReports,
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
  pluginUpdateRemote,
  type JsonValue,
  type PluginDetail,
  type PluginRemotePackageInput,
  type PluginSummary,
} from "../services/plugins";
import { pluginActiveContributions } from "../services/pluginContributions";
import { pluginContributionKeys, pluginKeys } from "./keys";

type QueryClientLike = ReturnType<typeof useQueryClient>;

function refreshPluginQueries(queryClient: QueryClientLike, pluginId: string) {
  queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
  queryClient.invalidateQueries({ queryKey: pluginKeys.detail(pluginId) });
}

function refreshPluginContributionQueries(queryClient: QueryClientLike) {
  queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
}

function refreshPluginMutationQueries(queryClient: QueryClientLike, pluginId: string) {
  refreshPluginQueries(queryClient, pluginId);
  refreshPluginContributionQueries(queryClient);
}

function pluginIdFromCommandArgs(args: JsonValue | undefined): string | null {
  if (args == null || typeof args !== "object" || Array.isArray(args)) return null;
  const pluginId = (args as Record<string, JsonValue>).pluginId;
  return typeof pluginId === "string" && pluginId.trim() ? normalizePluginId(pluginId) : null;
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

function setPluginDetailAndSummary(
  queryClient: QueryClientLike,
  pluginId: string,
  detail: PluginDetail
) {
  queryClient.setQueryData<PluginSummary[]>(pluginKeys.list(), (current) =>
    upsertPluginSummary(current, detail)
  );
  queryClient.setQueryData(pluginKeys.detail(pluginId), detail);
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

export function usePluginExtensionRuntimeReportsQuery(
  input: {
    pluginId: string | null;
    contributionType?: "command" | "hook" | null;
    contributionId?: string | null;
    traceId?: string | null;
    limit?: number | null;
  },
  options?: { enabled?: boolean }
) {
  const normalizedPluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
  const contributionType = input.contributionType ?? null;
  const contributionId = input.contributionId ?? null;
  const traceId = input.traceId ?? null;
  const limit = input.limit ?? 50;

  return useQuery({
    queryKey: pluginKeys.extensionRuntimeReports(
      normalizedPluginId,
      contributionType,
      contributionId,
      traceId,
      limit
    ),
    queryFn: () =>
      pluginListExtensionRuntimeReports({
        pluginId: normalizedPluginId,
        contributionType,
        contributionId,
        traceId,
        limit,
      }),
    enabled: (options?.enabled ?? true) && normalizedPluginId != null,
    placeholderData: keepPreviousData,
  });
}

export function usePluginActiveContributionsQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: pluginContributionKeys.active(),
    queryFn: () => pluginActiveContributions(),
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
  });
}

export function usePluginExecuteCommandMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { command: string; args?: JsonValue }) =>
      pluginExecuteCommand(input.command, input.args ?? null),
    onSettled: (_data, _error, input) => {
      queryClient.invalidateQueries({ queryKey: pluginKeys.extensionRuntimeReportsRoot() });
      const pluginId = pluginIdFromCommandArgs(input.args);
      if (pluginId) {
        queryClient.invalidateQueries({ queryKey: pluginKeys.detail(pluginId) });
      }
    },
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
      setPluginDetailAndSummary(queryClient, next.summary.plugin_id, next);
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(next.summary.plugin_id) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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

export function usePluginPreviewRemoteUpdateMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: PluginRemotePackageInput) => pluginPreviewRemoteUpdate(input),
    onSuccess: (next, input) => {
      queryClient.setQueryData(pluginKeys.updatePreview(input.downloadUrl), next);
    },
  });
}

export function usePluginUpdateFromFileMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (filePath: string) => pluginUpdateFromFile(filePath),
    onSuccess: (next) => {
      if (next) {
        setPluginDetailAndSummary(queryClient, next.summary.plugin_id, next);
        refreshPluginMutationQueries(queryClient, next.summary.plugin_id);
      } else {
        queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
        refreshPluginContributionQueries(queryClient);
      }
    },
  });
}

export function usePluginUpdateRemoteMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: PluginRemotePackageInput) => pluginUpdateRemote(input),
    onSuccess: (next, input) => {
      const normalizedPluginId = normalizePluginId(input.pluginId);
      if (next) {
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
    },
  });
}

export function usePluginInstallRemoteMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: Parameters<typeof pluginInstallRemote>[0]) => pluginInstallRemote(input),
    onSuccess: (next) => {
      if (!next) return;
      setPluginDetailAndSummary(queryClient, next.summary.plugin_id, next);
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(next.summary.plugin_id) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
      queryClient.invalidateQueries({ queryKey: pluginContributionKeys.active() });
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
        setPluginDetailAndSummary(queryClient, normalizedPluginId, next);
      }
      queryClient.invalidateQueries({ queryKey: pluginKeys.list() });
      queryClient.invalidateQueries({ queryKey: pluginKeys.detail(normalizedPluginId) });
    },
  });
}
