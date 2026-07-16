import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ExternalLink,
  GitBranch,
  HardDriveDownload,
  Loader2,
  RefreshCw,
  RotateCcw,
  Shield,
  ShieldOff,
  SquareArrowOutUpRight,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "../../../ui/Button";
import { Switch } from "../../../ui/Switch";
import { Input } from "../../../ui/Input";
import { ConfirmDialog } from "../../../ui/ConfirmDialog";
import { QueryStateView } from "../../../ui/QueryStateView";
import { openDesktopSinglePath } from "../../../services/desktop/dialog";
import { openDesktopUrl } from "../../../services/desktop/opener";
import { logToConsole } from "../../../services/consoleLog";
import {
  useCodexRetryGatewayApplyCommitMutation,
  useCodexRetryGatewayCheckUpdateMutation,
  useCodexRetryGatewayCreateDetailsSessionMutation,
  useCodexRetryGatewayEnablePlanMutation,
  useCodexRetryGatewayRetryMutation,
  useCodexRetryGatewayRevokeDetailsSessionMutation,
  useCodexRetryGatewaySetEnabledMutation,
  useCodexRetryGatewaySetNodeOverrideMutation,
  useCodexRetryGatewayStatusQuery,
  useCodexRetryGatewayUninstallMutation,
  useCodexRetryGatewayValidateCommitMutation,
} from "../../../query/codexRetryGateway";
import { formatActionFailureToast } from "../../../utils/errors";
import { cn } from "../../../utils/cn";
import {
  formatCodexRetryGatewayDesiredState,
  formatCodexRetryGatewayError,
  formatCodexRetryGatewayNodeSource,
  formatCodexRetryGatewayProviderSync,
  formatCodexRetryGatewayProviderSyncResult,
  formatCodexRetryGatewayRouteMode,
  formatCodexRetryGatewayRuntimePhase,
  formatCodexRetryGatewayTone,
  formatCodexRetryGatewayTrustState,
  getGatewayToneClass,
  getCodexRetryGatewayErrorGuidance,
  isCodexRetryGatewayProtected,
  resolveRepositoryUrl,
} from "./codexRetryGatewayPresentation";
import type {
  CodexRetryGatewayCommitValidation,
  CodexRetryGatewayDetailsSession,
  CodexRetryGatewayEnablePlan,
  CodexRetryGatewayUpdateCandidate,
} from "../../../services/cli/codexRetryGateway";

type RequirementKey =
  | "firstDownload"
  | "unreviewedCommit"
  | "cliProxyEnable"
  | "providerSync"
  | "wslUnprotected";

type GatewayManagerProps = {
  enabled?: boolean;
  showDetailsFrame?: boolean;
  onOpenDetailsRoute?: () => void;
};

type GatewayRequirement = {
  key: RequirementKey;
  required: boolean;
  label: string;
};

function InlineBadge({
  label,
  tone,
}: {
  label: string;
  tone: "success" | "warning" | "danger" | "muted";
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2.5 py-1 text-xs font-medium ring-1 ring-inset",
        getGatewayToneClass(tone)
      )}
    >
      {label}
    </span>
  );
}

function ReadonlyValue({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string | number | null | undefined;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0 border-l-2 border-line-subtle py-1 pl-3 pr-2">
      <div className="text-[11px] uppercase tracking-normal text-muted-foreground">{label}</div>
      <div
        className={cn(
          "mt-1 min-w-0 break-all text-sm font-medium text-foreground",
          mono && "font-mono text-xs"
        )}
      >
        {value == null || value === "" ? "—" : String(value)}
      </div>
    </div>
  );
}

function RequirementChecklist({
  requirements,
  values,
  onChange,
}: {
  requirements: GatewayRequirement[];
  values: Record<RequirementKey, boolean>;
  onChange: (key: RequirementKey, checked: boolean) => void;
}) {
  const active = requirements.filter((row) => row.required);
  if (active.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-line bg-surface-inset px-3 py-3 text-sm text-muted-foreground">
        当前没有额外确认项，确认后会直接执行。
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {active.map((row) => (
        <label
          key={row.key}
          className="flex items-start gap-3 rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-sm text-foreground"
        >
          <input
            type="checkbox"
            className="mt-0.5 h-4 w-4 rounded border-line"
            checked={values[row.key]}
            onChange={(event) => onChange(row.key, event.currentTarget.checked)}
          />
          <span className="leading-relaxed">{row.label}</span>
        </label>
      ))}
    </div>
  );
}

function buildEnableRequirements(plan: CodexRetryGatewayEnablePlan): GatewayRequirement[] {
  return [
    {
      key: "firstDownload",
      required: plan.first_download_required,
      label: "首次启用会下载并准备外部网关源码与运行环境。",
    },
    {
      key: "unreviewedCommit",
      required: plan.unreviewed_commit,
      label: `当前将运行未审阅提交 ${plan.selected_commit}。`,
    },
    {
      key: "cliProxyEnable",
      required: plan.cli_proxy_enable_required,
      label: "会同时启用 Codex CLI 代理，并改为 外部网关 -> AIO 的链路。",
    },
    {
      key: "providerSync",
      required: plan.provider_sync.change_required,
      label: `Provider Sync：${formatCodexRetryGatewayProviderSync(plan.provider_sync)}`,
    },
    {
      key: "wslUnprotected",
      required: plan.wsl_codex_unprotected,
      label: "WSL 中的 Codex 仍然直连 AIO，不受外部网关保护。",
    },
  ];
}

function initialRequirementState(): Record<RequirementKey, boolean> {
  return {
    firstDownload: false,
    unreviewedCommit: false,
    cliProxyEnable: false,
    providerSync: false,
    wslUnprotected: false,
  };
}

function requirementsSatisfied(
  requirements: GatewayRequirement[],
  values: Record<RequirementKey, boolean>
) {
  return requirements.every((row) => !row.required || values[row.key]);
}

function getProviderSyncAlignedToast(formatted: ReturnType<typeof formatActionFailureToast>) {
  if (formatted.error_code) {
    const guidance = getCodexRetryGatewayErrorGuidance(formatted.error_code);
    if (guidance) return guidance.replace(/[。；]$/, "");
  }
  if (formatted.error_code === "CODEX_RETRY_GATEWAY_STALE_GENERATION") {
    return "网关状态已发生变化，请刷新状态后重试";
  }
  if (
    formatted.error_code === "SEC_INVALID_INPUT" &&
    /codex retry gateway generation/i.test(formatted.message)
  ) {
    return "网关状态版本无效，请刷新状态后重试";
  }
  if (formatted.error_code === "CODEX_PROVIDER_SYNC_PROCESS_RUNNING") {
    return "Codex App 正在运行，请先关闭 Codex App 后重试";
  }
  if (formatted.error_code === "CODEX_PROVIDER_SYNC_PROCESS_CHECK_FAILED") {
    return "无法确认 Codex App 是否已完全关闭，请先手动确认已退出后重试；详情见 Console 日志";
  }
  return formatted.toast;
}

async function handleOpenUrl(label: string, url: string | null) {
  if (!url) {
    toast(`${label} 暂不可用`);
    return;
  }
  await openDesktopUrl(url);
}

export function CodexRetryGatewayManager({
  enabled = true,
  showDetailsFrame = false,
  onOpenDetailsRoute,
}: GatewayManagerProps) {
  const statusQuery = useCodexRetryGatewayStatusQuery({
    enabled,
    refetchIntervalMs: showDetailsFrame ? 5_000 : 8_000,
  });
  const enablePlanMutation = useCodexRetryGatewayEnablePlanMutation();
  const setEnabledMutation = useCodexRetryGatewaySetEnabledMutation();
  const checkUpdateMutation = useCodexRetryGatewayCheckUpdateMutation();
  const validateCommitMutation = useCodexRetryGatewayValidateCommitMutation();
  const applyCommitMutation = useCodexRetryGatewayApplyCommitMutation();
  const nodeOverrideMutation = useCodexRetryGatewaySetNodeOverrideMutation();
  const retryMutation = useCodexRetryGatewayRetryMutation();
  const uninstallMutation = useCodexRetryGatewayUninstallMutation();
  const detailsSessionMutation = useCodexRetryGatewayCreateDetailsSessionMutation();
  const revokeDetailsSessionMutation = useCodexRetryGatewayRevokeDetailsSessionMutation();
  const createDetailsSessionRef = useRef(detailsSessionMutation.mutateAsync);
  const revokeDetailsSessionRef = useRef(revokeDetailsSessionMutation.mutateAsync);
  const detailsSessionRequestRef = useRef(0);
  const activeDetailsSessionRef = useRef<CodexRetryGatewayDetailsSession | null>(null);

  const [enablePlan, setEnablePlan] = useState<CodexRetryGatewayEnablePlan | null>(null);
  const [enableDialogOpen, setEnableDialogOpen] = useState(false);
  const [requirementValues, setRequirementValues] =
    useState<Record<RequirementKey, boolean>>(initialRequirementState);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [updateCandidate, setUpdateCandidate] = useState<CodexRetryGatewayUpdateCandidate | null>(
    null
  );
  const [updateTrustAccepted, setUpdateTrustAccepted] = useState(false);
  const [manualCommit, setManualCommit] = useState("");
  const [manualValidation, setManualValidation] =
    useState<CodexRetryGatewayCommitValidation | null>(null);
  const [manualDialogOpen, setManualDialogOpen] = useState(false);
  const [manualTrustAccepted, setManualTrustAccepted] = useState(false);
  const [uninstallOpen, setUninstallOpen] = useState(false);
  const [detailsSession, setDetailsSession] = useState<CodexRetryGatewayDetailsSession | null>(
    null
  );
  const [detailsSessionError, setDetailsSessionError] = useState<string | null>(null);
  const [iframeBroken, setIframeBroken] = useState(false);
  const [iframeLoaded, setIframeLoaded] = useState(false);

  const status = statusQuery.data ?? null;
  const uninstallReady = Boolean(
    status &&
    !status.desired_enabled &&
    status.route_mode !== "guarded" &&
    status.process_status.phase === "stopped"
  );
  const requirements = useMemo(
    () => (enablePlan ? buildEnableRequirements(enablePlan) : []),
    [enablePlan]
  );
  const managerBusy =
    status?.operation_pending ||
    enablePlanMutation.isPending ||
    setEnabledMutation.isPending ||
    checkUpdateMutation.isPending ||
    validateCommitMutation.isPending ||
    applyCommitMutation.isPending ||
    nodeOverrideMutation.isPending ||
    retryMutation.isPending ||
    uninstallMutation.isPending ||
    detailsSessionMutation.isPending;

  useEffect(() => {
    createDetailsSessionRef.current = detailsSessionMutation.mutateAsync;
  }, [detailsSessionMutation.mutateAsync]);

  useEffect(() => {
    revokeDetailsSessionRef.current = revokeDetailsSessionMutation.mutateAsync;
  }, [revokeDetailsSessionMutation.mutateAsync]);

  const revokeIframeSession = useCallback(
    async (session: CodexRetryGatewayDetailsSession | null) => {
      if (!session) return;
      try {
        await revokeDetailsSessionRef.current(session.iframe_view_id);
      } catch (error) {
        const formatted = formatActionFailureToast("撤销详情会话", error);
        logToConsole("warn", "撤销 Codex 外部网关详情会话失败", { error: formatted.raw });
      }
    },
    []
  );

  const refreshDetailsSession = useCallback(async () => {
    const requestId = detailsSessionRequestRef.current + 1;
    detailsSessionRequestRef.current = requestId;
    setDetailsSessionError(null);
    setIframeBroken(false);
    setIframeLoaded(false);
    const session = await createDetailsSessionRef.current();
    if (detailsSessionRequestRef.current === requestId) {
      const previous = activeDetailsSessionRef.current;
      activeDetailsSessionRef.current = session;
      setDetailsSession(session);
      void revokeIframeSession(previous);
    } else {
      void revokeIframeSession(session);
    }
    return session;
  }, [revokeIframeSession]);

  useEffect(() => {
    if (!showDetailsFrame || !status?.desired_enabled || !status?.details_available) {
      detailsSessionRequestRef.current += 1;
      const previous = activeDetailsSessionRef.current;
      activeDetailsSessionRef.current = null;
      setDetailsSession(null);
      void revokeIframeSession(previous);
      return;
    }
    let active = true;
    void refreshDetailsSession().catch((error) => {
      if (!active) return;
      const formatted = formatActionFailureToast("创建详情会话", error);
      logToConsole("error", "创建 Codex 外部网关详情会话失败", { error: formatted.raw });
      setDetailsSession(null);
      setDetailsSessionError(formatted.toast);
    });
    return () => {
      active = false;
      detailsSessionRequestRef.current += 1;
      const previous = activeDetailsSessionRef.current;
      activeDetailsSessionRef.current = null;
      void revokeIframeSession(previous);
    };
  }, [
    refreshDetailsSession,
    revokeIframeSession,
    showDetailsFrame,
    status?.desired_enabled,
    status?.details_available,
    status?.generation,
  ]);

  const openBrowser = useCallback(async () => {
    if (!status?.desired_enabled) {
      toast("请先启用 Codex 外部网关，再打开管理页");
      return;
    }
    if (!status.details_available) {
      toast("管理桥接暂不可用，请刷新状态后重试");
      return;
    }
    try {
      const browserSession = await createDetailsSessionRef.current();
      try {
        await handleOpenUrl("浏览器入口", browserSession.browser_url);
      } finally {
        void revokeIframeSession(browserSession);
      }
    } catch (error) {
      const formatted = formatActionFailureToast("打开浏览器入口", error);
      logToConsole("error", "打开 Codex 外部网关浏览器入口失败", { error: formatted.raw });
      toast(formatted.toast);
    }
  }, [revokeIframeSession, status?.details_available, status?.desired_enabled]);

  const handleMutationError = useCallback(
    (action: string, error: unknown, providerSync = false) => {
      const formatted = formatActionFailureToast(action, error);
      logToConsole("error", `${action}失败`, {
        error: formatted.raw,
        error_code: formatted.error_code,
      });
      const friendlyToast = getProviderSyncAlignedToast(formatted);
      toast(providerSync || friendlyToast !== formatted.toast ? friendlyToast : formatted.toast);
    },
    []
  );

  const onToggleEnabled = useCallback(
    async (next: boolean) => {
      if (!status || managerBusy) return;

      if (!next) {
        try {
          await setEnabledMutation.mutateAsync({
            enabled: false,
            planGeneration: status.generation,
            confirmation: {
              acceptedFirstDownload: false,
              acceptedUnreviewedCommit: false,
              acceptedCliProxyEnable: false,
              acceptedProviderSync: false,
              acceptedWslUnprotected: false,
            },
          });
          toast("已停用 Codex 外部网关");
        } catch (error) {
          handleMutationError("停用 Codex 外部网关", error);
        }
        return;
      }

      try {
        const plan = await enablePlanMutation.mutateAsync();
        setEnablePlan(plan);
        setRequirementValues(initialRequirementState());
        setEnableDialogOpen(true);
      } catch (error) {
        handleMutationError("生成启用计划", error);
      }
    },
    [enablePlanMutation, handleMutationError, managerBusy, setEnabledMutation, status]
  );

  const onConfirmEnable = useCallback(async () => {
    if (!enablePlan) return;
    try {
      const enabled = await setEnabledMutation.mutateAsync({
        enabled: true,
        planGeneration: enablePlan.generation,
        confirmation: {
          acceptedFirstDownload: enablePlan.first_download_required
            ? requirementValues.firstDownload
            : false,
          acceptedUnreviewedCommit: enablePlan.unreviewed_commit
            ? requirementValues.unreviewedCommit
            : false,
          acceptedCliProxyEnable: enablePlan.cli_proxy_enable_required
            ? requirementValues.cliProxyEnable
            : false,
          acceptedProviderSync: enablePlan.provider_sync.change_required
            ? requirementValues.providerSync
            : false,
          acceptedWslUnprotected: enablePlan.wsl_codex_unprotected
            ? requirementValues.wslUnprotected
            : false,
        },
      });
      setEnableDialogOpen(false);
      setEnablePlan(null);
      toast(
        enabled.provider_sync
          ? formatCodexRetryGatewayProviderSyncResult(enabled.provider_sync)
          : "Codex 外部网关已启用"
      );
    } catch (error) {
      handleMutationError("启用 Codex 外部网关", error, true);
    }
  }, [enablePlan, handleMutationError, requirementValues, setEnabledMutation]);

  const onCheckUpdate = useCallback(async () => {
    try {
      const candidate = await checkUpdateMutation.mutateAsync();
      if (!candidate) {
        toast("当前已是最新官方提交");
        return;
      }
      setUpdateCandidate(candidate);
      setUpdateTrustAccepted(false);
      setUpdateDialogOpen(true);
    } catch (error) {
      handleMutationError("检查外部网关更新", error);
    }
  }, [checkUpdateMutation, handleMutationError]);

  const onApplyUpdate = useCallback(async () => {
    if (!status || !updateCandidate) return;
    try {
      await applyCommitMutation.mutateAsync({
        planGeneration: status.generation,
        commit: updateCandidate.commit,
        acceptedUpdate: true,
        acceptedUnreviewedCommit:
          updateCandidate.trust_state === "official_main_unreviewed" ? updateTrustAccepted : false,
      });
      setUpdateDialogOpen(false);
      setUpdateCandidate(null);
      toast("更新流程已提交");
    } catch (error) {
      handleMutationError("应用外部网关更新", error);
    }
  }, [applyCommitMutation, handleMutationError, status, updateCandidate, updateTrustAccepted]);

  const onValidateManualCommit = useCallback(async () => {
    try {
      const validation = await validateCommitMutation.mutateAsync(manualCommit);
      setManualValidation(validation);
      if (validation.error || !validation.canonical_commit) {
        toast(validation.error?.message ?? "该提交当前不可用");
        return;
      }
      setManualTrustAccepted(false);
      setManualDialogOpen(true);
    } catch (error) {
      handleMutationError("校验提交 SHA", error);
    }
  }, [handleMutationError, manualCommit, validateCommitMutation]);

  const onApplyManualCommit = useCallback(async () => {
    if (!status || !manualValidation?.canonical_commit) return;
    try {
      await applyCommitMutation.mutateAsync({
        planGeneration: status.generation,
        commit: manualValidation.canonical_commit,
        acceptedUpdate: true,
        acceptedUnreviewedCommit:
          manualValidation.trust_state === "official_main_unreviewed" ? manualTrustAccepted : false,
      });
      setManualDialogOpen(false);
      setManualValidation(null);
      toast("指定提交已进入切换流程");
    } catch (error) {
      handleMutationError("应用指定提交", error);
    }
  }, [applyCommitMutation, handleMutationError, manualTrustAccepted, manualValidation, status]);

  const onPickNode = useCallback(async () => {
    if (!status) return;
    try {
      const selection = await openDesktopSinglePath({
        title: "选择 Node 可执行文件",
        defaultPath: status.node_status.executable ?? undefined,
      });
      if (!selection) return;
      await nodeOverrideMutation.mutateAsync({
        generation: status.generation,
        executable: selection,
      });
      toast("已更新 Node 运行时");
    } catch (error) {
      handleMutationError("选择 Node 运行时", error);
    }
  }, [handleMutationError, nodeOverrideMutation, status]);

  const onResetNode = useCallback(async () => {
    if (!status) return;
    try {
      await nodeOverrideMutation.mutateAsync({
        generation: status.generation,
        executable: null,
      });
      toast("已恢复自动探测 Node");
    } catch (error) {
      handleMutationError("恢复自动探测 Node", error);
    }
  }, [handleMutationError, nodeOverrideMutation, status]);

  const onRetry = useCallback(async () => {
    if (!status) return;
    try {
      await retryMutation.mutateAsync(status.generation);
      toast("已提交恢复重试");
    } catch (error) {
      handleMutationError("重试外部网关恢复", error);
    }
  }, [handleMutationError, retryMutation, status]);

  const onConfirmUninstall = useCallback(async () => {
    if (!status || !uninstallReady) return;
    try {
      await uninstallMutation.mutateAsync({
        generation: status.generation,
        confirmedDataRemoval: true,
      });
      setUninstallOpen(false);
      toast("卸载流程已提交");
    } catch (error) {
      handleMutationError("卸载外部网关", error);
    }
  }, [handleMutationError, status, uninstallMutation, uninstallReady]);

  const repoUrl = status ? resolveRepositoryUrl(status.repository) : null;
  const statusTone = status ? formatCodexRetryGatewayTone(status) : "muted";
  const enableConfirmDisabled =
    !enablePlan || !requirementsSatisfied(requirements, requirementValues);
  const updateUnreviewed =
    updateCandidate?.trust_state != null &&
    updateCandidate.trust_state === "official_main_unreviewed";
  const manualUnreviewed =
    manualValidation?.trust_state != null &&
    manualValidation.trust_state === "official_main_unreviewed";

  return (
    <>
      <div
        data-testid="codex-retry-gateway-card"
        className="rounded-lg border border-line-subtle bg-surface-panel p-4 shadow-sm"
      >
        <QueryStateView
          query={statusQuery}
          loading={
            <div className="flex items-center gap-3 py-4">
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
              <span className="text-sm text-muted-foreground">读取 Codex 外部网关状态…</span>
            </div>
          }
          error={
            <div className="space-y-3 py-4">
              <div className="flex items-start gap-3 break-words text-sm text-rose-600 dark:text-rose-300">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{String(statusQuery.error)}</span>
              </div>
              <Button variant="secondary" size="sm" onClick={() => void statusQuery.refetch()}>
                重试
              </Button>
            </div>
          }
          isEmpty={(data) => data == null}
          empty={
            <div className="py-4 text-sm text-muted-foreground">
              当前还没有 Codex 外部网关状态。
            </div>
          }
        >
          {(currentStatus) => (
            <div className="space-y-4">
              <section data-testid="codex-retry-gateway-section" className="space-y-4">
                <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                  <div className="min-w-0 space-y-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="flex items-center gap-2 text-base font-semibold text-foreground">
                        {isCodexRetryGatewayProtected(currentStatus) ? (
                          <Shield className="h-4 w-4 text-emerald-500" aria-hidden="true" />
                        ) : (
                          <ShieldOff className="h-4 w-4 text-amber-500" aria-hidden="true" />
                        )}
                        Codex 外部网关
                      </div>
                      <InlineBadge
                        label={formatCodexRetryGatewayDesiredState(currentStatus)}
                        tone={currentStatus.desired_enabled ? "success" : "muted"}
                      />
                      <InlineBadge
                        label={formatCodexRetryGatewayRouteMode(currentStatus.route_mode)}
                        tone={statusTone}
                      />
                      <InlineBadge
                        label={formatCodexRetryGatewayRuntimePhase(currentStatus.runtime_phase)}
                        tone={statusTone}
                      />
                      <InlineBadge
                        label={formatCodexRetryGatewayTrustState(currentStatus.trust_state)}
                        tone={
                          currentStatus.trust_state === "aio_reviewed_recommendation"
                            ? "success"
                            : currentStatus.trust_state === "official_main_unreviewed"
                              ? "warning"
                              : "muted"
                        }
                      />
                    </div>
                    <div className="text-sm leading-relaxed text-muted-foreground">
                      {currentStatus.desired_enabled
                        ? isCodexRetryGatewayProtected(currentStatus)
                          ? "当前 Codex 请求会先进入外部网关，再转发到 AIO。"
                          : "已请求启用，但当前并未处于外部网关保护链路。"
                        : "当前保持非外部网关模式。"}
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center justify-end gap-2">
                    <Switch
                      checked={currentStatus.desired_enabled}
                      disabled={Boolean(managerBusy)}
                      onCheckedChange={(next) => void onToggleEnabled(next)}
                      aria-label="切换 Codex 外部网关"
                    />
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      className="gap-2"
                      onClick={() => void statusQuery.refetch()}
                      disabled={Boolean(managerBusy)}
                    >
                      <RefreshCw
                        className={cn("h-4 w-4", statusQuery.isFetching && "animate-spin")}
                        aria-hidden="true"
                      />
                      刷新状态
                    </Button>
                    {showDetailsFrame ? null : (
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        className="gap-2"
                        onClick={onOpenDetailsRoute}
                        disabled={
                          !onOpenDetailsRoute ||
                          !currentStatus.desired_enabled ||
                          !currentStatus.details_available ||
                          Boolean(managerBusy)
                        }
                      >
                        <SquareArrowOutUpRight className="h-4 w-4" aria-hidden="true" />
                        详情
                      </Button>
                    )}
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      className="gap-2"
                      onClick={() => void openBrowser()}
                      disabled={
                        !currentStatus.desired_enabled ||
                        !currentStatus.details_available ||
                        Boolean(managerBusy)
                      }
                    >
                      <ExternalLink className="h-4 w-4" aria-hidden="true" />
                      浏览器打开
                    </Button>
                  </div>
                </div>

                <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                  <ReadonlyValue
                    label="实际链路"
                    value={formatCodexRetryGatewayRouteMode(currentStatus.route_mode)}
                  />
                  <ReadonlyValue
                    label="运行阶段"
                    value={formatCodexRetryGatewayRuntimePhase(currentStatus.runtime_phase)}
                  />
                  <ReadonlyValue label="生效端口" value={currentStatus.effective_port ?? "—"} />
                  <ReadonlyValue
                    label="Node 运行时"
                    value={
                      currentStatus.node_status.available
                        ? `${currentStatus.node_status.version ?? "未知版本"} / ${formatCodexRetryGatewayNodeSource(currentStatus.node_status.source)}`
                        : "未找到可用 Node 18+"
                    }
                  />
                </div>

                {currentStatus.wsl_codex_unprotected ? (
                  <div className="break-words rounded-lg border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-800 dark:border-amber-800/70 dark:bg-amber-900/30 dark:text-amber-200">
                    已检测到 WSL 中的 Codex 仍然直连 AIO。启用外部网关不会保护 WSL 流量。
                  </div>
                ) : null}

                {currentStatus.last_error ? (
                  <div className="break-words rounded-lg border border-rose-200 bg-rose-50 px-3 py-3 text-sm text-rose-800 dark:border-rose-800/70 dark:bg-rose-900/30 dark:text-rose-200">
                    {formatCodexRetryGatewayError(currentStatus.last_error)}
                  </div>
                ) : null}

                <div className="grid gap-4 xl:grid-cols-[1.3fr,0.7fr]">
                  <div className="space-y-3">
                    <div className="rounded-lg border border-line-subtle bg-surface-inset p-4">
                      <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
                        <GitBranch className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
                        源码与提交
                      </div>
                      <div className="mt-3 grid gap-3 md:grid-cols-2">
                        <ReadonlyValue label="官方仓库" value={currentStatus.repository} mono />
                        <ReadonlyValue label="许可证" value={currentStatus.license ?? "未声明"} />
                        <ReadonlyValue
                          label="已选提交"
                          value={currentStatus.selected_commit}
                          mono
                        />
                        <ReadonlyValue
                          label="当前运行提交"
                          value={currentStatus.active_commit}
                          mono
                        />
                        <ReadonlyValue
                          label="推荐提交"
                          value={currentStatus.recommended_commit}
                          mono
                        />
                        <ReadonlyValue
                          label="回滚提交"
                          value={currentStatus.previous_commit}
                          mono
                        />
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          onClick={() => void handleOpenUrl("官方仓库", repoUrl)}
                          disabled={!repoUrl}
                        >
                          打开仓库
                        </Button>
                      </div>
                    </div>

                    <div className="rounded-lg border border-line-subtle bg-surface-inset p-4">
                      <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
                        <HardDriveDownload
                          className="h-4 w-4 text-muted-foreground"
                          aria-hidden="true"
                        />
                        更新与手动 SHA
                      </div>
                      <div className="mt-3 space-y-3">
                        <div className="flex flex-wrap items-center gap-2">
                          <Button
                            type="button"
                            size="sm"
                            className="gap-2"
                            onClick={() => void onCheckUpdate()}
                            disabled={Boolean(managerBusy)}
                          >
                            <RefreshCw
                              className={cn(
                                "h-4 w-4",
                                checkUpdateMutation.isPending && "animate-spin"
                              )}
                              aria-hidden="true"
                            />
                            检查更新
                          </Button>
                          <div className="text-xs text-muted-foreground">
                            只检查候选提交，不会直接替换当前源码。
                          </div>
                        </div>

                        <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
                          <Input
                            value={manualCommit}
                            onChange={(event) => {
                              setManualCommit(event.currentTarget.value);
                              setManualValidation(null);
                            }}
                            placeholder="输入官方仓库完整提交 SHA"
                            className="font-mono text-xs"
                          />
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => void onValidateManualCommit()}
                            disabled={Boolean(managerBusy) || manualCommit.trim().length === 0}
                          >
                            校验并选择
                          </Button>
                        </div>

                        {manualValidation ? (
                          <div className="min-w-0 break-words rounded-lg border border-line-subtle bg-surface-panel px-3 py-3 text-xs text-muted-foreground">
                            <div className="font-mono text-foreground">
                              规范 SHA：{manualValidation.canonical_commit ?? "—"}
                            </div>
                            <div className="mt-1">
                              官方主线：{manualValidation.official_main_commit ?? "—"}
                            </div>
                            <div className="mt-1">
                              信任状态：
                              {manualValidation.trust_state
                                ? formatCodexRetryGatewayTrustState(manualValidation.trust_state)
                                : "—"}
                            </div>
                            <div className="mt-1">
                              祖先校验：
                              {manualValidation.official_main_ancestor ? "通过" : "未通过"}
                            </div>
                            {manualValidation.summary ? (
                              <div className="mt-1 leading-relaxed">{manualValidation.summary}</div>
                            ) : null}
                            {manualValidation.error ? (
                              <div className="mt-1 text-rose-600 dark:text-rose-300">
                                {formatCodexRetryGatewayError(manualValidation.error)}
                              </div>
                            ) : null}
                          </div>
                        ) : null}
                      </div>
                    </div>
                  </div>

                  <div className="space-y-3">
                    <div className="rounded-lg border border-line-subtle bg-surface-inset p-4">
                      <div className="text-sm font-semibold text-foreground">Node.js</div>
                      <div className="mt-3 space-y-3">
                        <ReadonlyValue
                          label="可执行文件"
                          value={currentStatus.node_status.executable}
                          mono
                        />
                        <ReadonlyValue
                          label="版本 / 来源"
                          value={
                            currentStatus.node_status.available
                              ? `${currentStatus.node_status.version ?? "未知版本"} / ${formatCodexRetryGatewayNodeSource(
                                  currentStatus.node_status.source
                                )}`
                              : formatCodexRetryGatewayNodeSource(currentStatus.node_status.source)
                          }
                        />
                        {currentStatus.node_status.error ? (
                          <div className="break-words text-xs text-rose-600 dark:text-rose-300">
                            {formatCodexRetryGatewayError(currentStatus.node_status.error)}
                          </div>
                        ) : null}
                        <div className="flex flex-wrap gap-2">
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => void onPickNode()}
                            disabled={Boolean(managerBusy) || currentStatus.desired_enabled}
                          >
                            选择 Node
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => void onResetNode()}
                            disabled={Boolean(managerBusy) || currentStatus.desired_enabled}
                          >
                            恢复自动
                          </Button>
                        </div>
                        {currentStatus.desired_enabled ? (
                          <div className="text-xs text-muted-foreground">
                            请先关闭拦截网关，再修改 Node.js 运行时。
                          </div>
                        ) : null}
                      </div>
                    </div>

                    <div className="rounded-lg border border-line-subtle bg-surface-inset p-4">
                      <div className="text-sm font-semibold text-foreground">运行操作</div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          className="gap-2"
                          onClick={() => void onRetry()}
                          disabled={Boolean(managerBusy)}
                        >
                          <RotateCcw className="h-4 w-4" aria-hidden="true" />
                          重试恢复
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          className="gap-2"
                          onClick={() => setUninstallOpen(true)}
                          disabled={Boolean(managerBusy) || !uninstallReady}
                        >
                          <Trash2 className="h-4 w-4" aria-hidden="true" />
                          卸载并清理
                        </Button>
                      </div>
                      <div className="mt-3 text-xs leading-relaxed text-muted-foreground">
                        {uninstallReady
                          ? "卸载会移除已管理的外部网关数据与源码缓存。"
                          : "请先停用拦截网关；确认路由已恢复且受管进程完全停止后，才可卸载。"}
                      </div>
                    </div>
                  </div>
                </div>
              </section>

              {showDetailsFrame ? (
                <section className="space-y-4 border-b border-line-subtle pb-4">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold text-foreground">管理页嵌入</div>
                      <div className="mt-1 text-xs text-muted-foreground">
                        仅限 127.0.0.1 的临时桥接会话
                      </div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        className="gap-2"
                        onClick={() => {
                          void refreshDetailsSession().catch((error) => {
                            const formatted = formatActionFailureToast("刷新详情会话", error);
                            logToConsole("error", "刷新 Codex 外部网关详情会话失败", {
                              error: formatted.raw,
                            });
                            setDetailsSession(null);
                            setDetailsSessionError(formatted.toast);
                          });
                        }}
                        disabled={
                          !currentStatus.desired_enabled ||
                          !currentStatus.details_available ||
                          Boolean(managerBusy)
                        }
                      >
                        <RefreshCw
                          className={cn(
                            "h-4 w-4",
                            (detailsSessionMutation.isPending || !iframeLoaded) && "animate-spin"
                          )}
                          aria-hidden="true"
                        />
                        刷新嵌入
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        className="gap-2"
                        onClick={() => void openBrowser()}
                        disabled={
                          !currentStatus.desired_enabled ||
                          !currentStatus.details_available ||
                          Boolean(managerBusy)
                        }
                      >
                        <ExternalLink className="h-4 w-4" aria-hidden="true" />
                        浏览器打开
                      </Button>
                    </div>
                  </div>

                  {!currentStatus.desired_enabled ? (
                    <div className="rounded-lg border border-dashed border-line bg-surface-inset px-4 py-6 text-sm text-muted-foreground">
                      请先启用 Codex 外部网关，再进入管理页。
                    </div>
                  ) : !currentStatus.details_available ? (
                    <div className="rounded-lg border border-dashed border-line bg-surface-inset px-4 py-6 text-sm text-muted-foreground">
                      管理桥接暂不可用。请刷新状态；若网关正在恢复，请使用“重试恢复”。
                    </div>
                  ) : detailsSessionError ? (
                    <div className="break-words rounded-lg border border-rose-200 bg-rose-50 px-4 py-4 text-sm text-rose-800 dark:border-rose-800/70 dark:bg-rose-900/30 dark:text-rose-200">
                      {detailsSessionError}
                    </div>
                  ) : iframeBroken || !detailsSession ? (
                    <div className="rounded-lg border border-dashed border-line bg-surface-inset px-4 py-6 text-sm text-muted-foreground">
                      当前无法嵌入管理页。可先使用“浏览器打开”，或点击“刷新嵌入”重建桥接会话。
                    </div>
                  ) : (
                    <div className="min-w-0 overflow-hidden rounded-lg border border-line-subtle bg-black/5">
                      <iframe
                        key={detailsSession.iframe_url}
                        title="Codex 外部网关管理页"
                        src={detailsSession.iframe_url}
                        className="h-[70vh] min-h-[420px] max-h-[760px] w-full bg-white"
                        sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-downloads"
                        onLoad={() => setIframeLoaded(true)}
                        onError={() => {
                          setIframeBroken(true);
                          setIframeLoaded(false);
                        }}
                      />
                    </div>
                  )}
                </section>
              ) : null}
            </div>
          )}
        </QueryStateView>
      </div>

      <ConfirmDialog
        open={enableDialogOpen}
        title="启用 Codex 外部网关"
        description={
          enablePlan
            ? `目标提交 ${enablePlan.selected_commit}，端口 ${enablePlan.preferred_port}。`
            : undefined
        }
        onClose={() => {
          setEnableDialogOpen(false);
          setEnablePlan(null);
        }}
        onConfirm={() => void onConfirmEnable()}
        confirmLabel="确认启用"
        confirmingLabel="启用中…"
        confirming={setEnabledMutation.isPending}
        disabled={enableConfirmDisabled}
      >
        {enablePlan ? (
          <div className="space-y-3">
            <RequirementChecklist
              requirements={requirements}
              values={requirementValues}
              onChange={(key, checked) =>
                setRequirementValues((current) => ({ ...current, [key]: checked }))
              }
            />
            <div className="rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-xs text-muted-foreground">
              <div>信任状态：{formatCodexRetryGatewayTrustState(enablePlan.trust_state)}</div>
              <div className="mt-1">
                Node：
                {enablePlan.node_status.available
                  ? `${enablePlan.node_status.version ?? "未知版本"} / ${formatCodexRetryGatewayNodeSource(enablePlan.node_status.source)}`
                  : "未找到可用 Node 18+"}
              </div>
            </div>
          </div>
        ) : null}
      </ConfirmDialog>

      <ConfirmDialog
        open={updateDialogOpen}
        title="应用外部网关更新"
        description={updateCandidate ? `候选提交 ${updateCandidate.commit}` : undefined}
        onClose={() => {
          setUpdateDialogOpen(false);
          setUpdateCandidate(null);
        }}
        onConfirm={() => void onApplyUpdate()}
        confirmLabel="确认更新"
        confirmingLabel="更新中…"
        confirming={applyCommitMutation.isPending}
        disabled={updateUnreviewed ? !updateTrustAccepted : false}
      >
        {updateCandidate ? (
          <div className="space-y-3">
            <div className="break-words rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-sm text-muted-foreground">
              <div className="font-mono text-xs text-foreground">{updateCandidate.commit}</div>
              <div className="mt-1">
                官方主线：{updateCandidate.official_main_commit}，领先提交数：
                {updateCandidate.commits_ahead ?? "未知"}
              </div>
              <div className="mt-1">
                回滚目标：{updateCandidate.rollback_commit ?? "无可用回滚提交"}
              </div>
              {updateCandidate.summary ? (
                <div className="mt-1 leading-relaxed">{updateCandidate.summary}</div>
              ) : null}
            </div>
            {updateUnreviewed ? (
              <label className="flex items-start gap-3 rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-sm text-foreground">
                <input
                  type="checkbox"
                  className="mt-0.5 h-4 w-4 rounded border-line"
                  checked={updateTrustAccepted}
                  onChange={(event) => setUpdateTrustAccepted(event.currentTarget.checked)}
                />
                <span>我确认要运行官方主线未审阅提交。</span>
              </label>
            ) : null}
          </div>
        ) : null}
      </ConfirmDialog>

      <ConfirmDialog
        open={manualDialogOpen}
        title="应用指定提交"
        description={
          manualValidation?.canonical_commit
            ? `规范 SHA：${manualValidation.canonical_commit}`
            : undefined
        }
        onClose={() => setManualDialogOpen(false)}
        onConfirm={() => void onApplyManualCommit()}
        confirmLabel="应用此提交"
        confirmingLabel="切换中…"
        confirming={applyCommitMutation.isPending}
        disabled={manualUnreviewed ? !manualTrustAccepted : false}
      >
        {manualValidation ? (
          <div className="space-y-3">
            <div className="break-words rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-sm text-muted-foreground">
              <div>
                信任状态：
                {manualValidation.trust_state
                  ? formatCodexRetryGatewayTrustState(manualValidation.trust_state)
                  : "—"}
              </div>
              <div className="mt-1">
                官方主线祖先：{manualValidation.official_main_ancestor ? "是" : "否"}
              </div>
              {manualValidation.summary ? (
                <div className="mt-1 leading-relaxed">{manualValidation.summary}</div>
              ) : null}
            </div>
            {manualUnreviewed ? (
              <label className="flex items-start gap-3 rounded-lg border border-line-subtle bg-surface-inset px-3 py-3 text-sm text-foreground">
                <input
                  type="checkbox"
                  className="mt-0.5 h-4 w-4 rounded border-line"
                  checked={manualTrustAccepted}
                  onChange={(event) => setManualTrustAccepted(event.currentTarget.checked)}
                />
                <span>我确认要运行官方主线未审阅提交。</span>
              </label>
            ) : null}
          </div>
        ) : null}
      </ConfirmDialog>

      <ConfirmDialog
        open={uninstallOpen}
        title="卸载 Codex 外部网关"
        description="仅在外部网关已停用且受管进程已停止时，删除 AIO 维护的外部网关数据。"
        onClose={() => setUninstallOpen(false)}
        onConfirm={() => void onConfirmUninstall()}
        confirmLabel="确认卸载"
        confirmingLabel="卸载中…"
        confirming={uninstallMutation.isPending}
        disabled={!uninstallReady}
        confirmVariant="danger"
      >
        <div className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-800 dark:border-amber-800/70 dark:bg-amber-900/30 dark:text-amber-200">
          此操作会清理当前受管实例数据；如只想恢复直连 AIO，请直接关闭开关。
        </div>
      </ConfirmDialog>
    </>
  );
}
