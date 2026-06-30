// Usage: Manage installed community plugins and inspect manifest/permission state.

import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  BookOpen,
  Download,
  Upload,
  Power,
  PowerOff,
  RefreshCw,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { openDesktopSinglePath } from "../services/desktop/dialog";
import { openDesktopUrl } from "../services/desktop/opener";
import type {
  JsonValue,
  PluginDetail,
  PluginInstallPreview,
  PluginPermissionRisk,
  PluginRemotePackageInput,
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
  usePluginInstallFromFileMutation,
  usePluginInstallOfficialMutation,
  usePluginInstallRemoteMutation,
  usePluginPreviewFromFileMutation,
  usePluginPreviewRemoteUpdateMutation,
  usePluginPreviewUpdateFromFileMutation,
  usePluginQuery,
  usePluginRollbackMutation,
  usePluginSaveConfigMutation,
  usePluginUninstallMutation,
  usePluginUpdateFromFileMutation,
  usePluginUpdateRemoteMutation,
  usePluginsListQuery,
} from "../query/plugins";
import { PluginConfigSchemaForm } from "./plugins/PluginConfigSchemaForm";
import { PluginInstallPreviewDialog } from "./plugins/PluginInstallPreviewDialog";
import { PluginLifecyclePanel } from "./plugins/PluginLifecyclePanel";
import { PluginMarketPanel } from "./plugins/PluginMarketPanel";
import { PluginRuntimeReportsPanel } from "./plugins/PluginRuntimeReportsPanel";
import { PluginUpdatePreviewDialog } from "./plugins/PluginUpdatePreviewDialog";
import {
  describePluginPermission,
  describePluginRuntime,
  pluginRiskLabel,
  pluginStatusLabel,
} from "./plugins/pluginProductCopy";

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

const TRUST_EVENT_TYPES = new Set([
  "plugin.installed",
  "plugin.remote.installed",
  "plugin.updated",
  "plugin.rollback",
  "plugin.official.installed",
]);

const PLUGIN_DOCS_URL =
  "https://github.com/dyndynjyxa/aio-coding-hub/blob/main/docs/plugins/README.md";

const GATEWAY_HOOK_ACCESS: Record<string, string[]> = {
  "gateway.request.afterBodyRead": [
    "request.meta.read",
    "request.header.read",
    "request.header.readSensitive",
    "request.body.read",
    "request.header.write",
    "request.body.write",
  ],
  "gateway.request.beforeSend": [
    "request.meta.read",
    "request.header.read",
    "request.header.readSensitive",
    "request.body.read",
    "request.header.write",
    "request.body.write",
  ],
  "gateway.response.chunk": ["stream.inspect", "stream.modify"],
  "gateway.response.after": [
    "response.header.read",
    "response.body.read",
    "response.header.write",
    "response.body.write",
  ],
  "gateway.error": [
    "response.header.read",
    "response.body.read",
    "response.header.write",
    "response.body.write",
  ],
  "log.beforePersist": ["log.redact"],
};

type UpdatePreviewState =
  | {
      source: "file";
      filePath: string;
      diff: PluginUpdateDiff;
    }
  | {
      source: "remote";
      input: PluginRemotePackageInput;
      diff: PluginUpdateDiff;
    };

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
  const permissions = effectiveDataAccessPermissions(detail);
  if (permissions.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        插件未声明网关数据访问
      </div>
    );
  }

  return (
    <div className="grid gap-2">
      {permissions.map((permission) => {
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
            <span className="rounded-md border border-border px-2 py-0.5 text-xs">
              {pluginRiskLabel(copy.risk)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

function effectiveDataAccessPermissions(detail: PluginDetail): string[] {
  if (detail.manifest.runtime.kind !== "extensionHost") return [];
  if (!(detail.manifest.capabilities ?? []).includes("gateway.hooks")) return [];

  const permissions = new Set<string>();
  for (const hook of detail.manifest.contributes?.gatewayHooks ?? []) {
    for (const permission of GATEWAY_HOOK_ACCESS[hook.name] ?? []) {
      permissions.add(permission);
    }
  }
  return Array.from(permissions).sort();
}

function PluginDetailPanel({
  detail,
  loading,
  onSaveConfig,
  onUpdate,
  onRollback,
  savingConfig,
  busy,
}: {
  detail: PluginDetail | null | undefined;
  loading: boolean;
  onSaveConfig: (config: JsonValue) => void;
  onUpdate: () => void;
  onRollback: (version: string) => void;
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

  const runtime = detail.summary.runtime;
  const runtimeCopy = describePluginRuntime(detail.summary.runtime);
  const unsigned = isUnsigned(detail);
  const rollbackVersion = previousVersion(detail);
  const gatewayHooks = detail.manifest.contributes?.gatewayHooks ?? detail.manifest.hooks ?? [];

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
        </div>
        <div className="flex flex-wrap gap-2">
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
            高危数据访问需要确认插件来源和用途。
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

      <PluginRuntimeReportsPanel detail={detail} />

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
          {gatewayHooks.map((hook) => (
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
  const [updatePreviewState, setUpdatePreviewState] = useState<UpdatePreviewState | null>(null);
  const previewInstallMutation = usePluginPreviewFromFileMutation();
  const previewUpdateMutation = usePluginPreviewUpdateFromFileMutation();
  const previewRemoteUpdateMutation = usePluginPreviewRemoteUpdateMutation();
  const installMutation = usePluginInstallFromFileMutation();
  const installOfficialMutation = usePluginInstallOfficialMutation();
  const installRemoteMutation = usePluginInstallRemoteMutation();
  const updateMutation = usePluginUpdateFromFileMutation();
  const updateRemoteMutation = usePluginUpdateRemoteMutation();
  const rollbackMutation = usePluginRollbackMutation();
  const enableMutation = usePluginEnableMutation();
  const disableMutation = usePluginDisableMutation();
  const uninstallMutation = usePluginUninstallMutation();
  const saveConfigMutation = usePluginSaveConfigMutation();

  const selectedSummary = useMemo(
    () => plugins.find((plugin) => plugin.plugin_id === selectedPluginId) ?? null,
    [plugins, selectedPluginId]
  );
  const effectiveSelectedPluginId = selectedSummary?.plugin_id ?? null;
  const detailQuery = usePluginQuery(effectiveSelectedPluginId, {
    enabled: Boolean(effectiveSelectedPluginId),
  });

  useEffect(() => {
    if (plugins.length === 0) {
      if (selectedPluginId !== null) {
        setSelectedPluginId(null);
      }
      return;
    }

    const selectedStillExists = plugins.some((plugin) => plugin.plugin_id === selectedPluginId);
    if (!selectedStillExists) {
      setSelectedPluginId(plugins[0].plugin_id);
    }
  }, [plugins, selectedPluginId]);
  const busy =
    previewInstallMutation.isPending ||
    previewUpdateMutation.isPending ||
    previewRemoteUpdateMutation.isPending ||
    installMutation.isPending ||
    installOfficialMutation.isPending ||
    installRemoteMutation.isPending ||
    updateMutation.isPending ||
    updateRemoteMutation.isPending ||
    rollbackMutation.isPending ||
    enableMutation.isPending ||
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

  async function handleOpenPluginDocs() {
    try {
      await openDesktopUrl(PLUGIN_DOCS_URL);
    } catch (error) {
      toast.error(formatActionFailureToast("打开插件文档", error).toast);
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
      setUpdatePreviewState({ source: "file", filePath, diff });
    } catch (error) {
      toast.error(formatActionFailureToast("预览更新", error).toast);
    }
  }

  async function handleRemoteUpdate(input: PluginRemotePackageInput) {
    try {
      const diff = await previewRemoteUpdateMutation.mutateAsync(input);
      setUpdatePreviewState({ source: "remote", input, diff });
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
    const done = await runAction("更新插件", () => {
      if (updatePreviewState.source === "remote") {
        return updateRemoteMutation.mutateAsync(updatePreviewState.input);
      }
      return updateMutation.mutateAsync(updatePreviewState.filePath);
    });
    if (done) setUpdatePreviewState(null);
  }

  const updatePreviewLabel =
    updatePreviewState?.source === "remote"
      ? updatePreviewState.input.downloadUrl
      : (updatePreviewState?.filePath ?? null);

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
            <>
              <Button variant="secondary" onClick={handleOpenPluginDocs}>
                <BookOpen className="h-4 w-4" />
                插件文档
              </Button>
              <Button onClick={handleImport} disabled={busy}>
                <Download className="h-4 w-4" />
                导入 .aio-plugin
              </Button>
            </>
          }
        />

        {listQuery.error ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
            插件列表加载失败：{formatUnknownError(listQuery.error)}
          </div>
        ) : null}

        <div className="grid gap-3">
          <PluginMarketPanel
            plugins={plugins}
            busy={busy}
            onInstall={(input) =>
              runAction("安装市场插件", () => installRemoteMutation.mutateAsync(input))
            }
            onUpdate={handleRemoteUpdate}
            onInstallOfficial={(pluginId) =>
              runAction("安装官方插件", () => installOfficialMutation.mutateAsync(pluginId))
            }
            onSelectInstalled={(pluginId) => setSelectedPluginId(pluginId)}
          />
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
                  if (!effectiveSelectedPluginId) return;
                  runAction("回滚插件", () =>
                    rollbackMutation.mutateAsync({ pluginId: effectiveSelectedPluginId, version })
                  );
                }}
                onSaveConfig={(config) => {
                  if (!effectiveSelectedPluginId) return;
                  runAction("保存配置", () =>
                    saveConfigMutation.mutateAsync({ pluginId: effectiveSelectedPluginId, config })
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
        filePath={updatePreviewLabel}
        confirming={updateMutation.isPending || updateRemoteMutation.isPending}
        onClose={() => setUpdatePreviewState(null)}
        onConfirm={confirmUpdatePreview}
      />
    </>
  );
}
