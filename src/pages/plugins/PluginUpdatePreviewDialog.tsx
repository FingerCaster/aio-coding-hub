// Usage: Shows local plugin package update diff before applying the update.

import { AlertTriangle, CheckCircle2, RotateCcw, ShieldAlert } from "lucide-react";
import type { PluginUpdateDiff } from "../../services/plugins";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { describePluginPermission, pluginRiskLabel } from "./pluginProductCopy";

type PluginUpdatePreviewDialogProps = {
  diff: PluginUpdateDiff | null;
  filePath: string | null;
  open: boolean;
  confirming: boolean;
  onClose: () => void;
  onConfirm: () => void;
};

type PluginLifecycleChange = PluginUpdateDiff["hookChanges"][number];
type PluginLifecycleNotice = PluginUpdateDiff["warnings"][number];
type PluginPermissionLifecycleChange = PluginUpdateDiff["permissionChanges"][number];
type PluginContributionChange = PluginUpdateDiff["contributionChanges"][number];
type NoticeVariant = "warning" | "destructive";

const LIFECYCLE_NOTICE_CODES = new Set([
  "PLUGIN_MARKET_REVOKED",
  "PLUGIN_REVOKED",
  "PLUGIN_QUARANTINED",
]);

function changeLabel(change: string) {
  const labels: Record<string, string> = {
    added: "新增",
    added_pending: "新增",
    removed: "移除",
    changed: "变更",
    unchanged: "未变",
    unchanged_granted: "仍会生效",
    unchanged_pending: "仍会生效",
    unchanged_requested: "仍会生效",
  };
  return labels[change] ?? change;
}

function contributionKindLabel(kind: string) {
  const labels: Record<string, string> = {
    provider: "Provider",
    protocol: "协议",
    protocolBridge: "协议转译",
    command: "命令",
    gatewayHook: "网关 Hook",
    gatewayRule: "网关规则",
    ui: "页面区域",
    capability: "能力",
  };
  return labels[kind] ?? kind;
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

function isQuarantineOrRevocationNotice(notice: PluginLifecycleNotice) {
  const code = notice.code.toUpperCase();
  return (
    LIFECYCLE_NOTICE_CODES.has(code) ||
    code.endsWith("_REVOKED") ||
    code.endsWith("_QUARANTINED") ||
    code.includes("_QUARANTINE")
  );
}

function LifecycleChanges({ changes }: { changes: readonly PluginLifecycleChange[] }) {
  if (changes.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        没有 hook 变化
      </div>
    );
  }

  return (
    <div className="grid gap-2">
      {changes.map((change) => (
        <div
          key={`${change.name}:${change.change}`}
          className="rounded-md border border-border px-3 py-2"
        >
          <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
            <span className="break-all font-mono text-foreground">{change.name}</span>
            <span className="rounded-md border border-border px-2 py-0.5 text-xs">
              {changeLabel(change.change)}
            </span>
          </div>
          <div className="mt-1 break-words text-xs text-muted-foreground">
            {change.before ?? "-"} -&gt; {change.after ?? "-"}
          </div>
        </div>
      ))}
    </div>
  );
}

function PermissionChanges({ changes }: { changes: readonly PluginPermissionLifecycleChange[] }) {
  if (changes.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        没有数据访问变化
      </div>
    );
  }

  return (
    <div className="grid gap-2">
      {changes.map((change) => {
        const copy = describePluginPermission(change.permission);
        return (
          <div
            key={`${change.permission}:${change.change}`}
            className="flex flex-wrap items-start justify-between gap-2 rounded-md border border-border px-3 py-2 text-sm"
          >
            <div className="min-w-0">
              <div className="font-medium text-foreground">{copy.label}</div>
              <div className="mt-0.5 text-xs text-muted-foreground">{copy.detail}</div>
              <div className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                {change.permission}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {changeLabel(change.change)}
              </span>
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {pluginRiskLabel(change.risk)}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function ContributionChanges({ changes }: { changes: readonly PluginContributionChange[] }) {
  if (changes.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
        没有扩展范围变化
      </div>
    );
  }

  return (
    <div className="grid gap-2">
      {changes.map((change) => {
        const title = change.label ?? change.name;
        const summary =
          change.before || change.after
            ? `${change.before ?? "-"} -> ${change.after ?? "-"}`
            : null;
        return (
          <div
            key={`${change.kind}:${change.name}:${change.change}`}
            className="rounded-md border border-border px-3 py-2 text-sm"
          >
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
                    {contributionKindLabel(change.kind)}
                  </span>
                  <span className="break-words font-medium text-foreground">{title}</span>
                </div>
                <div className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                  {change.name}
                </div>
              </div>
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {changeLabel(change.change)}
              </span>
            </div>
            {summary ? (
              <div className="mt-1 break-words text-xs text-muted-foreground">{summary}</div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

export function PluginUpdatePreviewDialog({
  diff,
  filePath,
  open,
  confirming,
  onClose,
  onConfirm,
}: PluginUpdatePreviewDialogProps) {
  const blocked = Boolean(diff && diff.blockingReasons.length > 0);
  const canConfirm = Boolean(diff) && !blocked;
  const lifecycleNotices = diff ? diff.warnings.filter(isQuarantineOrRevocationNotice) : [];
  const lifecycleBlockingReasons = diff
    ? diff.blockingReasons.filter(isQuarantineOrRevocationNotice)
    : [];
  const warnings = diff
    ? diff.warnings.filter((notice) => !isQuarantineOrRevocationNotice(notice))
    : [];
  const blockingReasons = diff
    ? diff.blockingReasons.filter((notice) => !isQuarantineOrRevocationNotice(notice))
    : [];

  return (
    <Dialog
      open={open}
      title="更新预检"
      description="确认版本、数据访问和兼容性变化后再更新本地插件。"
      className="max-w-2xl"
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose();
      }}
    >
      {!diff ? (
        <div className="text-sm text-muted-foreground">暂无插件更新预检结果。</div>
      ) : (
        <div className="space-y-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-base font-semibold text-foreground">{diff.pluginId}</div>
              <div className="mt-1 font-mono text-sm text-muted-foreground">
                {diff.fromVersion} -&gt; {diff.toVersion}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                {diff.versionDirection}
              </span>
              {diff.trust.unsigned ? (
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

          <div className="grid gap-2 text-sm sm:grid-cols-2">
            <div className="rounded-md border border-border px-3 py-2">
              <div className="text-xs text-muted-foreground">兼容性</div>
              <div className="mt-1 flex items-center gap-2 text-foreground">
                {diff.compatibility.compatible ? (
                  <CheckCircle2 className="h-4 w-4 text-success" />
                ) : (
                  <AlertTriangle className="h-4 w-4 text-warning" />
                )}
                {diff.compatibility.compatible ? "可更新" : "需要处理阻断项"}
              </div>
              <div className="mt-1 font-mono text-xs text-muted-foreground">
                app {diff.compatibility.appRange} / api {diff.compatibility.pluginApiRange}
              </div>
            </div>
            <div className="rounded-md border border-border px-3 py-2">
              <div className="text-xs text-muted-foreground">回滚</div>
              <div className="mt-1 flex items-center gap-2 text-foreground">
                <RotateCcw className="h-4 w-4" />
                {diff.rollbackAvailable ? "可回滚到当前版本" : "当前版本不可回滚"}
              </div>
            </div>
            <div className="rounded-md border border-border px-3 py-2 sm:col-span-2">
              <div className="text-xs text-muted-foreground">Checksum</div>
              <div className="mt-1 break-all font-mono text-xs text-foreground">
                {diff.trust.checksum}
              </div>
            </div>
          </div>

          {diff.runtimeChange ? (
            <div className="rounded-md border border-border px-3 py-2 text-sm">
              <div className="font-semibold text-foreground">运行方式变化</div>
              <div className="mt-1 text-xs text-muted-foreground">
                {diff.runtimeChange.before ?? "-"} -&gt; {diff.runtimeChange.after ?? "-"}
              </div>
            </div>
          ) : null}

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">Hook 变化</div>
            <LifecycleChanges changes={diff.hookChanges} />
          </div>

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">数据访问变化</div>
            <PermissionChanges changes={diff.permissionChanges} />
          </div>

          <div className="space-y-2">
            <div className="text-sm font-semibold text-foreground">扩展范围变化</div>
            <ContributionChanges changes={diff.contributionChanges} />
          </div>

          {diff.configVersionChange ? (
            <div className="rounded-md border border-border px-3 py-2 text-sm">
              <div className="font-semibold text-foreground">配置版本</div>
              <div className="mt-1 text-xs text-muted-foreground">{diff.configVersionChange}</div>
            </div>
          ) : null}

          {lifecycleBlockingReasons.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-destructive">
                <AlertTriangle className="h-4 w-4" />
                隔离/撤销阻断项
              </div>
              <NoticeList notices={lifecycleBlockingReasons} variant="destructive" />
            </div>
          ) : null}

          {lifecycleNotices.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-warning">
                <ShieldAlert className="h-4 w-4" />
                隔离与撤销
              </div>
              <NoticeList notices={lifecycleNotices} />
            </div>
          ) : null}

          {warnings.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-warning">
                <ShieldAlert className="h-4 w-4" />
                警告
              </div>
              <NoticeList notices={warnings} />
            </div>
          ) : null}

          {blockingReasons.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-destructive">
                <AlertTriangle className="h-4 w-4" />
                阻断项
              </div>
              <NoticeList notices={blockingReasons} />
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
              {confirming ? "更新中..." : "确认更新"}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
