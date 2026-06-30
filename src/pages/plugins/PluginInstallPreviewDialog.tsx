// Usage: Shows local plugin package validation details before installation.

import { AlertTriangle, CheckCircle2, ShieldAlert } from "lucide-react";
import type { PluginInstallPreview } from "../../services/plugins";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { describePluginPermission, pluginRiskLabel } from "./pluginProductCopy";

type PluginInstallPreviewDialogProps = {
  preview: PluginInstallPreview | null;
  filePath: string | null;
  open: boolean;
  confirming: boolean;
  onClose: () => void;
  onConfirm: () => void;
};

type PluginLifecycleNotice = PluginInstallPreview["warnings"][number];
type PluginHookLifecycleSummary = PluginInstallPreview["hooks"][number];
type PluginContributionImpact = PluginInstallPreview["contributionImpact"];
type NoticeVariant = "warning" | "destructive";

function sourceLabel(source: string) {
  const labels: Record<string, string> = {
    local: "本地",
    market: "市场",
    github_release: "GitHub Release",
    offline: "离线",
    official: "官方",
  };
  return labels[source] ?? source;
}

function NoticeList({
  notices,
  variant = "warning",
}: {
  notices: readonly PluginLifecycleNotice[];
  variant?: NoticeVariant;
}) {
  if (notices.length === 0) return null;
  const className =
    variant === "destructive"
      ? "rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"
      : "rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning";
  return (
    <div className="grid gap-2">
      {notices.map((notice) => (
        <div key={`${notice.severity}:${notice.code}:${notice.message}`} className={className}>
          <div className="font-medium">{notice.message}</div>
          <div className="mt-0.5 font-mono text-xs opacity-80">{notice.code}</div>
        </div>
      ))}
    </div>
  );
}

function HookList({ hooks }: { hooks: readonly PluginHookLifecycleSummary[] }) {
  if (hooks.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        插件未声明 hook
      </div>
    );
  }

  return (
    <div className="grid gap-2">
      {hooks.map((hook) => (
        <div
          key={`${hook.name}:${hook.priority}:${hook.failurePolicy ?? "-"}`}
          className="rounded-md border border-border px-3 py-2 text-sm"
        >
          <div className="break-all font-mono text-foreground">{hook.name}</div>
          <div className="mt-1 text-xs text-muted-foreground">
            priority {hook.priority}
            {hook.failurePolicy ? ` / ${hook.failurePolicy}` : ""}
          </div>
        </div>
      ))}
    </div>
  );
}

function ContributionImpactList({ impact }: { impact: PluginContributionImpact }) {
  const groups = [
    {
      label: "Provider",
      values: impact.providers.map((provider) => provider.label ?? provider.id),
    },
    {
      label: "页面区域",
      values: impact.uiSlots.map((slot) => slot.slotId),
    },
    {
      label: "协议/转译",
      values: [
        ...impact.protocols.map((protocol) => protocol.label ?? protocol.id),
        ...impact.protocolBridges.map((bridge) => bridge.label ?? bridge.id),
      ],
    },
    {
      label: "命令",
      values: impact.commands.map((command) => command.title ?? command.command),
    },
    {
      label: "网关",
      values: impact.gateway.map((gateway) => gateway.label ?? gateway.id),
    },
    {
      label: "能力",
      values: impact.capabilities,
    },
  ].filter((group) => group.values.length > 0);

  if (groups.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        插件未声明扩展范围
      </div>
    );
  }

  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {groups.map((group) => (
        <div key={group.label} className="rounded-md border border-border px-3 py-2 text-sm">
          <div className="text-xs text-muted-foreground">{group.label}</div>
          <div className="mt-1 flex flex-wrap gap-1.5">
            {group.values.map((value) => (
              <span
                key={`${group.label}:${value}`}
                className="break-all rounded-md border border-border px-2 py-0.5 font-mono text-[11px] text-foreground"
              >
                {value}
              </span>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

export function PluginInstallPreviewDialog({
  preview,
  filePath,
  open,
  confirming,
  onClose,
  onConfirm,
}: PluginInstallPreviewDialogProps) {
  const blocked = Boolean(preview && preview.blockingReasons.length > 0);
  const canConfirm = Boolean(preview) && !blocked;

  return (
    <Dialog
      open={open}
      title="安装前预检"
      description="确认本地插件包的来源、数据访问和兼容性后再安装。"
      className="max-w-2xl"
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose();
      }}
    >
      {!preview ? (
        <div className="text-sm text-muted-foreground">暂无插件预检结果。</div>
      ) : (
        <div className="space-y-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-base font-semibold text-foreground">{preview.name}</div>
              <div className="break-words font-mono text-xs text-muted-foreground">
                {preview.pluginId}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {preview.version}
              </span>
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {sourceLabel(preview.source)}
              </span>
              {preview.trust.unsigned ? (
                <span className="rounded-md border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
                  未签名
                </span>
              ) : (
                <span className="rounded-md border border-success/30 bg-success/10 px-2 py-0.5 text-xs font-semibold text-success">
                  签名已验证
                </span>
              )}
            </div>
          </div>

          {preview.description ? (
            <div className="rounded-md border border-border px-3 py-2 text-sm text-foreground">
              {preview.description}
            </div>
          ) : null}

          <div className="rounded-md border border-border bg-muted/40 px-3 py-2 text-sm text-muted-foreground">
            预检只是解释层，最终安装仍会重新校验。
          </div>

          <div className="grid gap-2 text-sm sm:grid-cols-2">
            <div className="rounded-md border border-border px-3 py-2">
              <div className="text-xs text-muted-foreground">运行方式</div>
              <div className="mt-1 text-foreground">{preview.runtime.label}</div>
              <div className="mt-1 text-xs text-muted-foreground">
                {preview.runtime.supported ? "当前宿主支持" : "当前宿主不支持"}
              </div>
            </div>
            <div className="rounded-md border border-border px-3 py-2">
              <div className="text-xs text-muted-foreground">兼容性</div>
              <div className="mt-1 flex items-center gap-2 text-foreground">
                {preview.compatibility.compatible ? (
                  <CheckCircle2 className="h-4 w-4 text-success" />
                ) : (
                  <AlertTriangle className="h-4 w-4 text-warning" />
                )}
                {preview.compatibility.compatible ? "可安装" : "需要处理阻断项"}
              </div>
              <div className="mt-1 font-mono text-xs text-muted-foreground">
                app {preview.compatibility.appRange} / api {preview.compatibility.pluginApiRange}
              </div>
            </div>
            <div className="rounded-md border border-border px-3 py-2 sm:col-span-2">
              <div className="text-xs text-muted-foreground">Checksum</div>
              <div className="mt-1 break-all font-mono text-xs text-foreground">
                {preview.trust.checksum}
              </div>
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">数据访问</div>
            <div className="grid gap-2">
              {preview.permissions.length > 0 ? (
                preview.permissions.map((permission) => {
                  const copy = describePluginPermission(permission.permission);
                  return (
                    <div
                      key={permission.permission}
                      className="flex flex-wrap items-start justify-between gap-2 rounded-md border border-border px-3 py-2 text-sm"
                    >
                      <div className="min-w-0">
                        <div className="font-medium text-foreground">{copy.label}</div>
                        <div className="mt-0.5 text-xs text-muted-foreground">{copy.detail}</div>
                        <div className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                          {permission.permission}
                        </div>
                      </div>
                      <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                        {pluginRiskLabel(permission.risk)}
                      </span>
                    </div>
                  );
                })
              ) : (
                <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
                  插件未声明额外数据访问
                </div>
              )}
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">Hooks</div>
            <HookList hooks={preview.hooks} />
          </div>

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">扩展范围</div>
            <ContributionImpactList impact={preview.contributionImpact} />
          </div>

          {preview.warnings.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-warning">
                <ShieldAlert className="h-4 w-4" />
                警告
              </div>
              <NoticeList notices={preview.warnings} />
            </div>
          ) : null}

          {preview.blockingReasons.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-destructive">
                <AlertTriangle className="h-4 w-4" />
                阻断项
              </div>
              <NoticeList notices={preview.blockingReasons} variant="destructive" />
            </div>
          ) : null}

          {filePath ? (
            <div className="break-all font-mono text-xs text-muted-foreground">{filePath}</div>
          ) : null}

          <div className="flex items-center justify-end gap-2">
            <Button variant="secondary" onClick={onClose} disabled={confirming}>
              取消
            </Button>
            <Button onClick={onConfirm} disabled={!canConfirm || confirming}>
              {confirming ? "安装中..." : "确认安装"}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
