import type {
  CodexProviderSyncPlan,
  CodexRetryGatewayError,
  CodexRetryGatewayNodeResolutionSource,
  CodexRetryGatewayRuntimePhase,
  CodexRetryGatewayStatus,
  CodexRetryGatewayTrustState,
  CodexRouteMode,
} from "../../../generated/bindings";

export type GatewayTone = "success" | "warning" | "muted" | "danger";

export function formatCodexRetryGatewayRuntimePhase(phase: CodexRetryGatewayRuntimePhase): string {
  switch (phase) {
    case "disabled":
      return "已停用";
    case "preparing":
      return "准备中";
    case "starting":
      return "启动中";
    case "guarded":
      return "保护中";
    case "bypassed_recovering":
      return "旁路恢复中";
    case "recovery_paused":
      return "恢复已暂停";
    case "updating":
      return "更新中";
    case "stopping":
      return "停止中";
    case "cleanup_needed":
      return "需要清理";
    case "uninstalling":
      return "卸载中";
    case "error":
      return "错误";
  }
}

export function formatCodexRetryGatewayRouteMode(routeMode: CodexRouteMode): string {
  switch (routeMode) {
    case "guarded":
      return "外部网关保护";
    case "direct_aio":
      return "直连 AIO";
    case "unproxied":
      return "未代理";
  }
}

export function formatCodexRetryGatewayTrustState(trustState: CodexRetryGatewayTrustState): string {
  switch (trustState) {
    case "aio_reviewed_recommendation":
      return "AIO 已审阅推荐";
    case "official_main_unreviewed":
      return "官方主线未审阅";
    case "unavailable":
      return "来源未就绪";
  }
}

export function formatCodexRetryGatewayNodeSource(
  source: CodexRetryGatewayNodeResolutionSource
): string {
  switch (source) {
    case "codex_sibling":
      return "Codex 同目录";
    case "aio_discovery":
      return "AIO 自动发现";
    case "process_path":
      return "当前进程 PATH";
    case "manual_override":
      return "手动指定";
    case "unavailable":
      return "未找到";
  }
}

export function formatCodexRetryGatewayTone(status: CodexRetryGatewayStatus): GatewayTone {
  if (status.runtime_phase === "guarded" && status.route_mode === "guarded") {
    return "success";
  }
  if (status.runtime_phase === "error" || status.last_error) {
    return "danger";
  }
  if (status.desired_enabled) {
    return status.route_mode === "guarded" ? "success" : "warning";
  }
  return "muted";
}

export function formatCodexRetryGatewayDesiredState(status: CodexRetryGatewayStatus): string {
  return status.desired_enabled ? "期望启用" : "期望关闭";
}

export function isCodexRetryGatewayProtected(status: CodexRetryGatewayStatus): boolean {
  return status.route_mode === "guarded" && status.runtime_phase === "guarded";
}

export function getGatewayToneClass(tone: GatewayTone): string {
  switch (tone) {
    case "success":
      return "bg-emerald-50 text-emerald-700 ring-emerald-600/20 dark:bg-emerald-900/30 dark:text-emerald-300";
    case "warning":
      return "bg-amber-50 text-amber-700 ring-amber-600/20 dark:bg-amber-900/30 dark:text-amber-300";
    case "danger":
      return "bg-rose-50 text-rose-700 ring-rose-600/20 dark:bg-rose-900/30 dark:text-rose-300";
    case "muted":
      return "bg-slate-100 text-slate-700 ring-slate-500/15 dark:bg-slate-800/80 dark:text-slate-200";
  }
}

export function formatCodexRetryGatewayProviderSync(plan: CodexProviderSyncPlan): string {
  const current = plan.current_provider ?? "当前未配置";
  const changeText = plan.change_required ? `${current} -> ${plan.target_provider}` : "无需切换";
  const closedText = plan.codex_must_be_closed ? "需要先关闭 Codex App" : "无需关闭 Codex App";
  return `${changeText}；会同步会话与 Provider 状态、写入备份，${closedText}。`;
}

export function formatCodexRetryGatewayError(error: CodexRetryGatewayError | null): string | null {
  if (!error) return null;
  const code = error.code?.trim();
  return code ? `${error.message}（${code}）` : error.message;
}

export function resolveRepositoryUrl(repository: string): string | null {
  const normalized = repository.trim();
  if (!normalized) return null;
  if (/^https?:\/\//i.test(normalized)) {
    return normalized;
  }
  if (/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(normalized)) {
    return `https://github.com/${normalized}`;
  }
  return null;
}
