// Usage: Manage installed community plugins and inspect manifest/permission state.

import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  Copy,
  Download,
  Upload,
  Power,
  PowerOff,
  RefreshCw,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { copyText } from "../services/clipboard";
import { openDesktopSinglePath } from "../services/desktop/dialog";
import type {
  JsonValue,
  PluginDetail,
  PluginHookExecutionReport,
  PluginInstallPreview,
  PluginPermissionRisk,
  PluginStatus,
  PluginSummary,
  PluginUpdateDiff,
} from "../services/plugins";
import { formatActionFailureToast, formatUnknownError } from "../utils/errors";
import { Button } from "../ui/Button";
import { PageHeader } from "../ui/PageHeader";
import { Spinner } from "../ui/Spinner";
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
  usePluginUninstallMutation,
  usePluginUpdateFromFileMutation,
  usePluginsListQuery,
} from "../query/plugins";
import { PluginConfigSchemaForm } from "./plugins/PluginConfigSchemaForm";
import { PluginInstallPreviewDialog } from "./plugins/PluginInstallPreviewDialog";
import { PluginLifecyclePanel } from "./plugins/PluginLifecyclePanel";
import { PluginUpdatePreviewDialog } from "./plugins/PluginUpdatePreviewDialog";
import {
  describePluginPermission,
  describePluginRuntime,
  pluginRiskLabel,
  pluginStatusLabel,
} from "./plugins/pluginProductCopy";

const OFFICIAL_PLUGINS = [{ id: "official.privacy-filter", name: "Privacy Filter" }];

const INSTALL_SOURCE_LABELS: Record<string, string> = {
  local: "本地",
  market: "市场",
  github_release: "GitHub Release",
  offline: "离线",
  official: "官方",
};

const RISK_CLASS: Record<PluginPermissionRisk, string> = {
  low: "border-success/30 bg-success/10 text-success",
  medium: "border-warning/30 bg-warning/10 text-warning",
  high: "border-destructive/30 bg-destructive/10 text-destructive",
  critical: "border-destructive bg-destructive text-destructive-foreground",
};

function canEnableStatus(status: PluginStatus) {
  return status === "disabled" || status === "installed";
}

function riskPill(risk: PluginPermissionRisk) {
  return (
    <span
      className={`inline-flex rounded-md border px-2 py-0.5 text-xs font-semibold ${RISK_CLASS[risk]}`}
    >
      {pluginRiskLabel(risk)}
    </span>
  );
}

function jsonRecord(value: JsonValue): Record<string, JsonValue> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, JsonValue>;
  }
  return null;
}

function detailValue(details: JsonValue, key: string) {
  const value = jsonRecord(details)?.[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

const TRUST_EVENT_TYPES = new Set([
  "plugin.installed",
  "plugin.updated",
  "plugin.rollback",
  "plugin.official.installed",
]);

function latestTrustAudit(detail: PluginDetail) {
  return detail.audit_logs
    .filter((log) => TRUST_EVENT_TYPES.has(log.event_type))
    .sort((a, b) => b.created_at - a.created_at || b.id - a.id)[0];
}

function isUnsigned(detail: PluginDetail) {
  return jsonRecord(latestTrustAudit(detail)?.details)?.unsigned === true;
}

function previousVersion(detail: PluginDetail) {
  return detail.rollback_versions[0] ?? null;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-2">
      <h2 className="text-sm font-semibold text-foreground">{title}</h2>
      {children}
    </section>
  );
}

function detailRows(detail: PluginDetail) {
  return [
    ["插件 ID", detail.manifest.id],
    ["版本", detail.manifest.version],
    ["API", detail.manifest.apiVersion],
    ["安装来源", INSTALL_SOURCE_LABELS[detail.install_source] ?? detail.install_source],
    ["安装目录", detail.installed_dir ?? "-"],
    ["宿主版本", detail.manifest.hostCompatibility.app],
    ["插件 API", detail.manifest.hostCompatibility.pluginApi],
  ];
}

function TraceIdButton({ traceId }: { traceId: string | null | undefined }) {
  if (!traceId) return <span className="font-mono text-xs text-muted-foreground">-</span>;
  const value = traceId;

  async function handleCopy() {
    try {
      await copyText(value);
      toast.success("Trace ID 已复制");
    } catch (error) {
      toast.error(formatActionFailureToast("复制 Trace ID", error).toast);
    }
  }

  return (
    <Button
      size="sm"
      variant="ghost"
      className="h-auto min-h-0 px-1.5 py-0.5 font-mono text-xs"
      onClick={handleCopy}
      title="复制 Trace ID"
    >
      <Copy className="h-3 w-3" />
      {value}
    </Button>
  );
}

function reportFailureLabel(report: PluginHookExecutionReport) {
  return report.failure_kind ?? report.error_code ?? "-";
}

function RuntimeObservabilitySection({ detail }: { detail: PluginDetail }) {
  const reportsQuery = usePluginRuntimeReportsQuery({
    pluginId: detail.summary.plugin_id,
    limit: 8,
  });
  const replayExportMutation = usePluginExportReplayFixtureMutation();
  const reports = reportsQuery.data?.slice(0, 8) ?? [];
  const failures = detail.runtime_failures.slice(0, 5);
  const auditLogs = detail.audit_logs.slice(0, 8);
  const empty =
    reports.length === 0 &&
    failures.length === 0 &&
    auditLogs.length === 0 &&
    !reportsQuery.isLoading;

  async function handleExportReplayFixture(report: PluginHookExecutionReport) {
    if (!report.trace_id) return;
    try {
      const fixture = await replayExportMutation.mutateAsync({
        traceId: report.trace_id,
        hookName: report.hook_name,
        pluginId: report.plugin_id,
      });
      await copyText(JSON.stringify(fixture, null, 2));
      toast.success("Replay fixture 已复制");
    } catch (error) {
      toast.error(formatActionFailureToast("导出 replay fixture", error).toast);
    }
  }

  return (
    <Section title="运行观测">
      {empty ? (
        <div className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
          还没有记录到插件运行事件
        </div>
      ) : (
        <div className="grid gap-2">
          {reportsQuery.isLoading ? (
            <div className="rounded-md border border-border px-3 py-3 text-sm text-muted-foreground">
              正在读取插件运行报告
            </div>
          ) : null}

          {reports.map((report) => (
            <div key={`report-${report.id}`} className="rounded-md border border-border px-3 py-2">
              <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                <span className="font-medium text-foreground">{report.status}</span>
                <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
                  {report.runtime_kind}
                </span>
              </div>
              <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-4">
                <div>
                  <div>Hook</div>
                  <div className="break-words font-mono text-foreground">{report.hook_name}</div>
                </div>
                <div>
                  <div>耗时</div>
                  <div className="break-words font-mono text-foreground">
                    {report.duration_ms}ms
                  </div>
                </div>
                <div>
                  <div>Failure</div>
                  <div className="break-words font-mono text-foreground">
                    {reportFailureLabel(report)}
                  </div>
                </div>
                <div>
                  <div>Trace ID</div>
                  <TraceIdButton traceId={report.trace_id} />
                </div>
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={!report.trace_id || replayExportMutation.isPending}
                  onClick={() => void handleExportReplayFixture(report)}
                  title={report.replay_export_reason ?? "导出 replay fixture"}
                >
                  <Download className="h-3.5 w-3.5" />
                  导出 Replay
                </Button>
                {report.replay_export_reason ? (
                  <span className="text-xs text-muted-foreground">
                    {report.replay_export_reason}
                  </span>
                ) : null}
              </div>
            </div>
          ))}

          {failures.map((failure) => (
            <div
              key={`failure-${failure.id}`}
              className="rounded-md border border-border px-3 py-2"
            >
              <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                <span className="font-medium text-foreground">{failure.message}</span>
                <span className="rounded-md bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
                  {failure.failure_kind}
                </span>
              </div>
              <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-2">
                <div>
                  <div>Hook</div>
                  <div className="break-words font-mono text-foreground">
                    {failure.hook_name ?? "-"}
                  </div>
                </div>
                <div>
                  <div>Trace ID</div>
                  <TraceIdButton traceId={failure.trace_id} />
                </div>
              </div>
            </div>
          ))}

          {auditLogs.map((log) => {
            const hookName = detailValue(log.details, "hookName");
            const failureKind = detailValue(log.details, "failureKind");

            return (
              <div key={`audit-${log.id}`} className="rounded-md border border-border px-3 py-2">
                <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                  <span className="font-medium text-foreground">{log.message}</span>
                  <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
                    {log.risk_level}
                  </span>
                </div>
                <div className="mt-1 break-words font-mono text-xs text-muted-foreground">
                  {log.event_type}
                </div>
                <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-3">
                  <div>
                    <div>Hook</div>
                    <div className="break-words font-mono text-foreground">{hookName ?? "-"}</div>
                  </div>
                  <div>
                    <div>Failure</div>
                    <div className="break-words font-mono text-foreground">
                      {failureKind ?? "-"}
                    </div>
                  </div>
                  <div>
                    <div>Trace ID</div>
                    <TraceIdButton traceId={log.trace_id} />
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </Section>
  );
}

function PluginListRow({
  plugin,
  active,
  onSelect,
  onEnable,
  onDisable,
  onUninstall,
  busy,
}: {
  plugin: PluginSummary;
  active: boolean;
  onSelect: () => void;
  onEnable: () => void;
  onDisable: () => void;
  onUninstall: () => void;
  busy: boolean;
}) {
  const enabled = plugin.status === "enabled";
  const canEnable = canEnableStatus(plugin.status);
  const runtime = describePluginRuntime(plugin.runtime);
  return (
    <article
      className={`rounded-lg border p-3 ${
        active ? "border-primary bg-primary/5" : "border-border bg-card"
      }`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <button className="min-w-0 text-left" onClick={onSelect} type="button">
          <div className="truncate text-sm font-semibold text-foreground">{plugin.name}</div>
          <div className="truncate font-mono text-xs text-muted-foreground">{plugin.plugin_id}</div>
        </button>
        <div className="flex items-center gap-2">
          <span className="rounded-md border border-border px-2 py-0.5 text-xs">
            {pluginStatusLabel(plugin.status)}
          </span>
          {riskPill(plugin.permission_risk)}
        </div>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-2 text-xs text-muted-foreground sm:grid-cols-4">
        <div>
          <div className="text-[11px] uppercase">版本</div>
          <div className="text-foreground">{plugin.current_version ?? "-"}</div>
        </div>
        <div>
          <div className="text-[11px] uppercase">运行方式</div>
          <div className="text-foreground">{runtime.label}</div>
        </div>
        <div>
          <div className="text-[11px] uppercase">更新</div>
          <div className="text-foreground">{plugin.update_available ? "可更新" : "-"}</div>
        </div>
        <div>
          <div className="text-[11px] uppercase">错误</div>
          <div className="truncate text-foreground">{plugin.last_error ?? "-"}</div>
        </div>
      </div>

      <div className="mt-3 flex flex-wrap gap-2">
        <Button size="sm" variant="secondary" onClick={onSelect}>
          查看详情
        </Button>
        {enabled ? (
          <Button size="sm" variant="secondary" onClick={onDisable} disabled={busy}>
            <PowerOff className="h-3.5 w-3.5" />
            禁用
          </Button>
        ) : canEnable ? (
          <Button size="sm" onClick={onEnable} disabled={busy}>
            <Power className="h-3.5 w-3.5" />
            启用
          </Button>
        ) : null}
        <Button size="sm" variant="secondary" onClick={onUninstall} disabled={busy}>
          <Trash2 className="h-3.5 w-3.5" />
          卸载
        </Button>
      </div>
    </article>
  );
}

function PermissionList({ detail }: { detail: PluginDetail }) {
  const granted = new Set(detail.granted_permissions);
  return (
    <div className="grid gap-2">
      {detail.manifest.permissions.map((permission) => {
        const ok = granted.has(permission);
        const copy = describePluginPermission(permission);
        return (
          <div
            key={permission}
            className="flex flex-wrap items-start justify-between gap-2 rounded-md border border-border px-3 py-2 text-sm"
          >
            <div>
              <div className="font-medium text-foreground">{copy.label}</div>
              <div className="mt-0.5 text-xs text-muted-foreground">{copy.detail}</div>
              <div className="mt-1 font-mono text-[11px] text-muted-foreground">{permission}</div>
            </div>
            <span
              className={`rounded-md px-2 py-0.5 text-xs font-semibold ${
                ok ? "bg-success/10 text-success" : "bg-warning/10 text-warning"
              }`}
            >
              {ok ? "已允许" : "待允许"}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function PluginDetailPanel({
  detail,
  loading,
  onSaveConfig,
  onUpdate,
  onRollback,
  onGrantPendingPermissions,
  savingConfig,
  busy,
}: {
  detail: PluginDetail | null | undefined;
  loading: boolean;
  onSaveConfig: (config: JsonValue) => void;
  onUpdate: () => void;
  onRollback: (version: string) => void;
  onGrantPendingPermissions: (pluginId: string, permissions: readonly string[]) => void;
  savingConfig: boolean;
  busy: boolean;
}) {
  if (loading) {
    return (
      <div className="flex min-h-72 items-center justify-center">
        <Spinner />
      </div>
    );
  }

  if (!detail) {
    return <div className="text-sm text-muted-foreground">选择一个插件查看详情。</div>;
  }

  const runtime =
    detail.manifest.runtime.kind === "declarativeRules"
      ? `declarativeRules: ${detail.manifest.runtime.rules.join(", ")}`
      : detail.manifest.runtime.kind === "native"
        ? `native: ${detail.manifest.runtime.engine}`
        : `wasm: ${detail.manifest.runtime.abiVersion}`;
  const runtimeCopy = describePluginRuntime(detail.summary.runtime);
  const unsigned = isUnsigned(detail);
  const rollbackVersion = previousVersion(detail);

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          {detail.summary.update_available ? (
            <span className="rounded-md border border-warning/30 bg-warning/10 px-2 py-0.5 text-xs font-semibold text-warning">
              可更新
            </span>
          ) : null}
          {unsigned ? (
            <span className="rounded-md border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
              未签名
            </span>
          ) : null}
          {detail.pending_permissions.length > 0 ? (
            <span className="rounded-md border border-warning/30 bg-warning/10 px-2 py-0.5 text-xs font-semibold text-warning">
              新权限待授权
            </span>
          ) : null}
        </div>
        <div className="flex flex-wrap gap-2">
          {detail.pending_permissions.length > 0 ? (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() =>
                onGrantPendingPermissions(detail.summary.plugin_id, detail.pending_permissions)
              }
            >
              <ShieldAlert className="h-3.5 w-3.5" />
              授权待审批权限
            </Button>
          ) : null}
          {detail.summary.update_available ? (
            <Button size="sm" variant="secondary" disabled={busy} onClick={onUpdate}>
              <Upload className="h-3.5 w-3.5" />
              更新
            </Button>
          ) : null}
        </div>
      </div>

      <PluginLifecyclePanel
        detail={detail}
        rollbackVersion={rollbackVersion}
        busy={busy}
        onRollback={onRollback}
      />

      <Section title="这个插件会做什么">
        <div className="rounded-md border border-border px-3 py-2 text-sm">
          <div className="font-medium text-foreground">
            {detail.manifest.description ?? detail.summary.name}
          </div>
          <div className="mt-1 text-xs text-muted-foreground">{runtimeCopy.detail}</div>
        </div>
      </Section>

      <Section title="数据访问">
        {detail.summary.permission_risk === "high" ||
        detail.summary.permission_risk === "critical" ? (
          <div className="flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            <ShieldAlert className="h-4 w-4" />
            高危权限需要确认插件来源和用途。
          </div>
        ) : null}
        <PermissionList detail={detail} />
      </Section>

      <Section title="设置">
        <PluginConfigSchemaForm
          identity={`${detail.summary.plugin_id}:${detail.manifest.configVersion ?? 1}:${detail.summary.updated_at}`}
          schema={detail.manifest.configSchema}
          value={detail.config}
          pending={savingConfig}
          onSubmit={onSaveConfig}
        />
      </Section>

      <RuntimeObservabilitySection detail={detail} />

      <Section title="开发者信息">
        <div className="grid gap-2 text-sm sm:grid-cols-2">
          {detailRows(detail).map(([label, value]) => (
            <div key={label} className="rounded-md border border-border px-3 py-2">
              <div className="text-xs text-muted-foreground">{label}</div>
              <div className="break-words text-foreground">{value}</div>
            </div>
          ))}
          <div className="rounded-md border border-border px-3 py-2 sm:col-span-2">
            <div className="text-xs text-muted-foreground">运行方式</div>
            <div className="break-words text-foreground">{runtimeCopy.label}</div>
            <div className="mt-1 break-words font-mono text-xs text-muted-foreground">
              {runtime}
            </div>
          </div>
        </div>

        <div className="mt-3 grid gap-2">
          {detail.manifest.hooks.map((hook) => (
            <div key={hook.name} className="rounded-md border border-border px-3 py-2 text-sm">
              <div className="font-mono text-xs">{hook.name}</div>
              <div className="mt-1 text-xs text-muted-foreground">
                priority {hook.priority ?? 0}
                {hook.failurePolicy ? ` · ${hook.failurePolicy}` : ""}
              </div>
            </div>
          ))}
        </div>
      </Section>
    </div>
  );
}

export function PluginsPage() {
  const listQuery = usePluginsListQuery();
  const plugins = useMemo(() => listQuery.data ?? [], [listQuery.data]);
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [installPreviewState, setInstallPreviewState] = useState<{
    filePath: string;
    preview: PluginInstallPreview;
  } | null>(null);
  const [updatePreviewState, setUpdatePreviewState] = useState<{
    filePath: string;
    diff: PluginUpdateDiff;
  } | null>(null);
  const previewInstallMutation = usePluginPreviewFromFileMutation();
  const previewUpdateMutation = usePluginPreviewUpdateFromFileMutation();
  const installMutation = usePluginInstallFromFileMutation();
  const installOfficialMutation = usePluginInstallOfficialMutation();
  const updateMutation = usePluginUpdateFromFileMutation();
  const rollbackMutation = usePluginRollbackMutation();
  const enableMutation = usePluginEnableMutation();
  const grantPermissionsMutation = usePluginGrantPermissionsMutation();
  const disableMutation = usePluginDisableMutation();
  const uninstallMutation = usePluginUninstallMutation();
  const saveConfigMutation = usePluginSaveConfigMutation();

  useEffect(() => {
    if (!selectedPluginId && plugins.length > 0) {
      setSelectedPluginId(plugins[0].plugin_id);
    }
  }, [plugins, selectedPluginId]);

  const selectedSummary = useMemo(
    () => plugins.find((plugin) => plugin.plugin_id === selectedPluginId) ?? null,
    [plugins, selectedPluginId]
  );
  const detailQuery = usePluginQuery(selectedPluginId, { enabled: Boolean(selectedPluginId) });
  const busy =
    previewInstallMutation.isPending ||
    previewUpdateMutation.isPending ||
    installMutation.isPending ||
    installOfficialMutation.isPending ||
    updateMutation.isPending ||
    rollbackMutation.isPending ||
    enableMutation.isPending ||
    grantPermissionsMutation.isPending ||
    disableMutation.isPending ||
    uninstallMutation.isPending ||
    saveConfigMutation.isPending;

  async function runAction(action: string, task: () => Promise<unknown>) {
    try {
      await task();
      toast.success(`${action}成功`);
      return true;
    } catch (error) {
      toast.error(formatActionFailureToast(action, error).toast);
      return false;
    }
  }

  async function handleImport() {
    const filePath = await openDesktopSinglePath({
      title: "选择 .aio-plugin 插件包",
      filters: [{ name: "AIO plugin package", extensions: ["aio-plugin"] }],
    });
    if (!filePath) return;
    try {
      const preview = await previewInstallMutation.mutateAsync(filePath);
      setInstallPreviewState({ filePath, preview });
    } catch (error) {
      toast.error(formatActionFailureToast("预览插件", error).toast);
    }
  }

  async function handleUpdate() {
    const filePath = await openDesktopSinglePath({
      title: "选择更新插件包",
      filters: [{ name: "AIO plugin package", extensions: ["aio-plugin"] }],
    });
    if (!filePath) return;
    try {
      const diff = await previewUpdateMutation.mutateAsync(filePath);
      setUpdatePreviewState({ filePath, diff });
    } catch (error) {
      toast.error(formatActionFailureToast("预览更新", error).toast);
    }
  }

  async function confirmInstallPreview() {
    if (!installPreviewState) return;
    const done = await runAction("导入插件", () =>
      installMutation.mutateAsync(installPreviewState.filePath)
    );
    if (done) setInstallPreviewState(null);
  }

  async function confirmUpdatePreview() {
    if (!updatePreviewState) return;
    const done = await runAction("更新插件", () =>
      updateMutation.mutateAsync(updatePreviewState.filePath)
    );
    if (done) setUpdatePreviewState(null);
  }

  if (listQuery.isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }

  return (
    <>
      <div className="flex h-full flex-col gap-5 overflow-hidden">
        <PageHeader
          title="插件"
          subtitle="为 AIO Coding Hub 增加本地能力。插件可以在请求发送前、响应返回后或日志保存前处理内容。"
          actions={
            <Button onClick={handleImport} disabled={busy}>
              <Download className="h-4 w-4" />
              导入 .aio-plugin
            </Button>
          }
        />

        {listQuery.error ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
            插件列表加载失败：{formatUnknownError(listQuery.error)}
          </div>
        ) : null}

        <div className="flex flex-wrap items-center gap-2 border-b border-border pb-3">
          <span className="text-xs font-semibold text-muted-foreground">推荐插件</span>
          {OFFICIAL_PLUGINS.map((plugin) => {
            const installed = plugins.some((item) => item.plugin_id === plugin.id);
            return (
              <Button
                key={plugin.id}
                size="sm"
                variant="secondary"
                disabled={busy || installed}
                onClick={() =>
                  runAction("安装官方插件", () => installOfficialMutation.mutateAsync(plugin.id))
                }
              >
                <Download className="h-3.5 w-3.5" />
                {installed ? "已安装" : plugin.name}
              </Button>
            );
          })}
        </div>

        {plugins.length === 0 && !listQuery.error ? (
          <div className="rounded-lg border border-dashed border-border px-4 py-10 text-center">
            <div className="text-sm font-semibold text-foreground">还没有安装插件</div>
            <div className="mt-1 text-sm text-muted-foreground">
              可以安装官方 Privacy Filter，或导入 .aio-plugin 插件包。
            </div>
          </div>
        ) : (
          <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(360px,0.9fr)_minmax(0,1.1fr)]">
            <div className="min-h-0 overflow-y-auto pr-1 scrollbar-overlay">
              <div className="space-y-3">
                {plugins.map((plugin) => (
                  <PluginListRow
                    key={plugin.plugin_id}
                    plugin={plugin}
                    active={plugin.plugin_id === selectedPluginId}
                    busy={busy}
                    onSelect={() => setSelectedPluginId(plugin.plugin_id)}
                    onEnable={() =>
                      runAction("启用插件", () => enableMutation.mutateAsync(plugin.plugin_id))
                    }
                    onDisable={() =>
                      runAction("禁用插件", () => disableMutation.mutateAsync(plugin.plugin_id))
                    }
                    onUninstall={() =>
                      runAction("卸载插件", () => uninstallMutation.mutateAsync(plugin.plugin_id))
                    }
                  />
                ))}
              </div>
            </div>

            <div className="min-h-0 overflow-y-auto rounded-lg border border-border bg-card p-4 scrollbar-overlay">
              <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
                <div>
                  <div className="text-sm font-semibold text-foreground">
                    {selectedSummary?.name ?? "插件详情"}
                  </div>
                  <div className="font-mono text-xs text-muted-foreground">
                    {selectedSummary?.plugin_id ?? "-"}
                  </div>
                </div>
                {detailQuery.isFetching ? <RefreshCw className="h-4 w-4 animate-spin" /> : null}
              </div>
              <PluginDetailPanel
                detail={detailQuery.data}
                loading={detailQuery.isLoading}
                savingConfig={saveConfigMutation.isPending}
                busy={busy}
                onUpdate={handleUpdate}
                onRollback={(version) => {
                  if (!selectedPluginId) return;
                  runAction("回滚插件", () =>
                    rollbackMutation.mutateAsync({ pluginId: selectedPluginId, version })
                  );
                }}
                onSaveConfig={(config) => {
                  if (!selectedPluginId) return;
                  runAction("保存配置", () =>
                    saveConfigMutation.mutateAsync({ pluginId: selectedPluginId, config })
                  );
                }}
                onGrantPendingPermissions={(pluginId, permissions) => {
                  runAction("授权权限", () =>
                    grantPermissionsMutation.mutateAsync({ pluginId, permissions })
                  );
                }}
              />
            </div>
          </div>
        )}
      </div>

      <PluginInstallPreviewDialog
        open={installPreviewState != null}
        preview={installPreviewState?.preview ?? null}
        filePath={installPreviewState?.filePath ?? null}
        confirming={installMutation.isPending}
        onClose={() => setInstallPreviewState(null)}
        onConfirm={confirmInstallPreview}
      />
      <PluginUpdatePreviewDialog
        open={updatePreviewState != null}
        diff={updatePreviewState?.diff ?? null}
        filePath={updatePreviewState?.filePath ?? null}
        confirming={updateMutation.isPending}
        onClose={() => setUpdatePreviewState(null)}
        onConfirm={confirmUpdatePreview}
      />
    </>
  );
}
