// Usage: Frontend IPC wrappers for community plugin management.

import {
  commands,
  type JsonValue,
  type PluginAuditLog,
  type PluginDetail,
  type PluginExtensionExecutionReport,
  type PluginHookExecutionReport,
  type PluginInstallPreview,
  type PluginInstallSource,
  type PluginManifest,
  type PluginMarketListing,
  type PluginPermissionRisk,
  type PluginReplayFixture,
  type PluginRuntime,
  type PluginStatus,
  type PluginSummary,
  type PluginUpdateDiff,
} from "../generated/bindings";
import { invokeGeneratedIpc } from "./generatedIpc";

export type {
  JsonValue,
  PluginAuditLog,
  PluginDetail,
  PluginExtensionExecutionReport,
  PluginHookExecutionReport,
  PluginInstallPreview,
  PluginInstallSource,
  PluginManifest,
  PluginMarketListing,
  PluginPermissionRisk,
  PluginReplayFixture,
  PluginRuntime,
  PluginStatus,
  PluginSummary,
  PluginUpdateDiff,
};

const PLUGIN_AUDIT_LOG_DEFAULT_LIMIT = 50;
const PLUGIN_AUDIT_LOG_MAX_LIMIT = 500;
const PLUGIN_RUNTIME_REPORT_DEFAULT_LIMIT = 50;
const PLUGIN_RUNTIME_REPORT_MAX_LIMIT = 500;

function normalizeRequiredText(label: string, value: string): string {
  const normalized = value.trim();
  if (!normalized) {
    throw new Error(`SEC_INVALID_INPUT: ${label} is required`);
  }
  return normalized;
}

export function normalizePluginId(pluginId: string): string {
  return normalizeRequiredText("pluginId", pluginId);
}

export function normalizePluginFilePath(filePath: string): string {
  return normalizeRequiredText("filePath", filePath);
}

export type PluginRemotePackageInput = {
  pluginId: string;
  downloadUrl: string;
  checksum: string;
  signature?: string | null;
  publicKey?: string | null;
  marketSourceUrl?: string | null;
  source?: "market" | "github_release" | null;
};

function normalizeRemotePackageInput(input: PluginRemotePackageInput) {
  const pluginId = normalizePluginId(input.pluginId);
  const downloadUrl = normalizeRequiredText("downloadUrl", input.downloadUrl);
  const checksum = normalizeRequiredText("checksum", input.checksum);
  const signature =
    input.signature == null ? null : normalizeRequiredText("signature", input.signature);
  const publicKey =
    input.publicKey == null ? null : normalizeRequiredText("publicKey", input.publicKey);
  const marketSourceUrl =
    input.marketSourceUrl == null
      ? null
      : normalizeRequiredText("marketSourceUrl", input.marketSourceUrl);
  const source = input.source ?? null;

  return { pluginId, downloadUrl, checksum, signature, publicKey, marketSourceUrl, source };
}

function clampAuditLimit(limit: number | null | undefined): number {
  if (limit == null || !Number.isFinite(limit)) return PLUGIN_AUDIT_LOG_DEFAULT_LIMIT;
  return Math.min(PLUGIN_AUDIT_LOG_MAX_LIMIT, Math.max(1, Math.trunc(limit)));
}

function clampRuntimeReportLimit(limit: number | null | undefined): number {
  if (limit == null || !Number.isFinite(limit)) return PLUGIN_RUNTIME_REPORT_DEFAULT_LIMIT;
  return Math.min(PLUGIN_RUNTIME_REPORT_MAX_LIMIT, Math.max(1, Math.trunc(limit)));
}

export async function pluginList() {
  return invokeGeneratedIpc<PluginSummary[]>({
    title: "读取插件列表失败",
    cmd: "plugin_list",
    invoke: async () => commands.pluginList(),
  });
}

export async function pluginGet(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "读取插件详情失败",
    cmd: "plugin_get",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginGet({ pluginId: normalizedPluginId }),
  });
}

export async function pluginPreviewFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginInstallPreview>({
    title: "预览插件失败",
    cmd: "plugin_preview_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginPreviewFromFile({ filePath: normalizedFilePath }),
  });
}

export async function pluginPreviewUpdateFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginUpdateDiff>({
    title: "预览插件更新失败",
    cmd: "plugin_preview_update_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginPreviewUpdateFromFile({ filePath: normalizedFilePath }),
  });
}

export async function pluginInstallFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginDetail>({
    title: "导入插件失败",
    cmd: "plugin_install_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginInstallFromFile({ filePath: normalizedFilePath }),
  });
}

export async function pluginUpdateFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginDetail>({
    title: "更新插件失败",
    cmd: "plugin_update_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginUpdateFromFile({ filePath: normalizedFilePath }),
  });
}

export async function pluginPreviewRemoteUpdate(input: PluginRemotePackageInput) {
  const remoteInput = normalizeRemotePackageInput(input);

  return invokeGeneratedIpc<PluginUpdateDiff>({
    title: "预览远程插件更新失败",
    cmd: "plugin_preview_remote_update",
    args: remoteInput,
    invoke: async () => commands.pluginPreviewRemoteUpdate(remoteInput),
  });
}

export async function pluginInstallRemote(input: PluginRemotePackageInput) {
  const remoteInput = normalizeRemotePackageInput(input);

  return invokeGeneratedIpc<PluginDetail>({
    title: "远程安装插件失败",
    cmd: "plugin_install_remote",
    args: remoteInput,
    invoke: async () => commands.pluginInstallRemote(remoteInput),
  });
}

export async function pluginUpdateRemote(input: PluginRemotePackageInput) {
  const remoteInput = normalizeRemotePackageInput(input);

  return invokeGeneratedIpc<PluginDetail>({
    title: "远程更新插件失败",
    cmd: "plugin_update_remote",
    args: remoteInput,
    invoke: async () => commands.pluginUpdateRemote(remoteInput),
  });
}

export async function pluginRollback(pluginId: string, version: string) {
  const normalizedPluginId = normalizePluginId(pluginId);
  const normalizedVersion = normalizeRequiredText("version", version);

  return invokeGeneratedIpc<PluginDetail>({
    title: "回滚插件失败",
    cmd: "plugin_rollback",
    args: { pluginId: normalizedPluginId, version: normalizedVersion },
    invoke: async () =>
      commands.pluginRollback({ pluginId: normalizedPluginId, version: normalizedVersion }),
  });
}

export async function pluginParseMarketIndex(
  indexJson: string,
  indexUrl?: string | null,
  signature?: string | null
) {
  const normalizedIndexJson = normalizeRequiredText("indexJson", indexJson);
  const normalizedIndexUrl = indexUrl == null ? null : normalizeRequiredText("indexUrl", indexUrl);
  const normalizedSignature =
    signature == null ? null : normalizeRequiredText("signature", signature);

  return invokeGeneratedIpc<PluginMarketListing[]>({
    title: "解析插件市场索引失败",
    cmd: "plugin_parse_market_index",
    args: {
      indexJson: normalizedIndexJson,
      indexUrl: normalizedIndexUrl,
      signature: normalizedSignature,
    },
    invoke: async () =>
      commands.pluginParseMarketIndex({
        indexJson: normalizedIndexJson,
        indexUrl: normalizedIndexUrl,
        signature: normalizedSignature,
      }),
  });
}

export async function pluginInstallOfficial(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "安装官方插件失败",
    cmd: "plugin_install_official",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginInstallOfficial({ pluginId: normalizedPluginId }),
  });
}

export async function pluginQuarantineRevoked(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "隔离撤销插件失败",
    cmd: "plugin_quarantine_revoked",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginQuarantineRevoked({ pluginId: normalizedPluginId }),
  });
}

export async function pluginEnable(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "启用插件失败",
    cmd: "plugin_enable",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginEnable({ pluginId: normalizedPluginId }),
  });
}

export async function pluginDisable(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "禁用插件失败",
    cmd: "plugin_disable",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginDisable({ pluginId: normalizedPluginId }),
  });
}

export async function pluginUninstall(pluginId: string) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "卸载插件失败",
    cmd: "plugin_uninstall",
    args: { pluginId: normalizedPluginId },
    invoke: async () => commands.pluginUninstall({ pluginId: normalizedPluginId }),
  });
}

export async function pluginSaveConfig(pluginId: string, config: JsonValue) {
  const normalizedPluginId = normalizePluginId(pluginId);

  return invokeGeneratedIpc<PluginDetail>({
    title: "保存插件配置失败",
    cmd: "plugin_save_config",
    args: { pluginId: normalizedPluginId, config },
    invoke: async () => commands.pluginSaveConfig({ pluginId: normalizedPluginId, config }),
  });
}

export async function pluginListAuditLogs(input: {
  pluginId?: string | null;
  limit?: number | null;
}) {
  const pluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
  const limit = clampAuditLimit(input.limit);

  return invokeGeneratedIpc<PluginAuditLog[]>({
    title: "读取插件审计日志失败",
    cmd: "plugin_list_audit_logs",
    args: { pluginId, limit },
    invoke: async () => commands.pluginListAuditLogs({ pluginId, limit }),
  });
}

export async function pluginListRuntimeReports(input: {
  pluginId?: string | null;
  hookName?: string | null;
  traceId?: string | null;
  limit?: number | null;
}) {
  const pluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
  const hookName =
    input.hookName == null ? null : normalizeRequiredText("hookName", input.hookName);
  const traceId = input.traceId == null ? null : normalizeRequiredText("traceId", input.traceId);
  const limit = clampRuntimeReportLimit(input.limit);

  return invokeGeneratedIpc<PluginHookExecutionReport[]>({
    title: "读取插件运行报告失败",
    cmd: "plugin_list_runtime_reports",
    args: { pluginId, hookName, traceId, limit },
    invoke: async () => commands.pluginListRuntimeReports({ pluginId, hookName, traceId, limit }),
  });
}

export async function pluginListExtensionRuntimeReports(input: {
  pluginId?: string | null;
  contributionType?: "command" | "hook" | null;
  contributionId?: string | null;
  traceId?: string | null;
  limit?: number | null;
}) {
  const pluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);
  const contributionType =
    input.contributionType == null
      ? null
      : normalizeRequiredText("contributionType", input.contributionType);
  const contributionId =
    input.contributionId == null
      ? null
      : normalizeRequiredText("contributionId", input.contributionId);
  const traceId = input.traceId == null ? null : normalizeRequiredText("traceId", input.traceId);
  const limit = clampRuntimeReportLimit(input.limit);

  return invokeGeneratedIpc<PluginExtensionExecutionReport[]>({
    title: "读取插件运行报告失败",
    cmd: "plugin_list_extension_runtime_reports",
    args: { pluginId, contributionType, contributionId, traceId, limit },
    invoke: async () =>
      commands.pluginListExtensionRuntimeReports({
        pluginId,
        contributionType,
        contributionId,
        traceId,
        limit,
      }),
  });
}

export async function pluginExecuteCommand(command: string, args: JsonValue = null) {
  const normalizedCommand = normalizeRequiredText("command", command);

  return invokeGeneratedIpc<JsonValue>({
    title: "执行插件命令失败",
    cmd: "plugin_execute_command",
    args: { command: normalizedCommand, args },
    invoke: async () => commands.pluginExecuteCommand({ command: normalizedCommand, args }),
  });
}

export async function pluginExportReplayFixture(input: {
  traceId: string;
  hookName: string;
  pluginId?: string | null;
}) {
  const traceId = normalizeRequiredText("traceId", input.traceId);
  const hookName = normalizeRequiredText("hookName", input.hookName);
  const pluginId = input.pluginId == null ? null : normalizePluginId(input.pluginId);

  return invokeGeneratedIpc<PluginReplayFixture>({
    title: "导出插件 replay fixture 失败",
    cmd: "plugin_export_replay_fixture",
    args: { traceId, hookName, pluginId },
    invoke: async () => commands.pluginExportReplayFixture({ traceId, hookName, pluginId }),
  });
}
