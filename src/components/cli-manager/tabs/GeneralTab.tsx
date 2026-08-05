import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useNavigate } from "react-router-dom";
import { CACHE_ANOMALY_MONITOR_GUIDE_COPY } from "../../../services/gateway/cacheAnomalyMonitorConfig";
import {
  gatewayUpstreamProxyDetectIp,
  gatewayUpstreamProxyTest,
} from "../../../services/gateway/gateway";
import { logToConsole } from "../../../services/consoleLog";
import type { AppSettings, SensitiveStringUpdate } from "../../../services/settings/settings";
import type { GatewayRectifierSettingsPatch } from "../../../services/settings/settingsGatewayRectifier";
import {
  MAX_STREAM_INTERNAL_ERROR_GUARD_MS,
  validateUpstreamProxyFields,
} from "../../../services/settings/settingsValidation";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Input } from "../../../ui/Input";
import { SettingsRow } from "../../../ui/SettingsRow";
import { RadioGroup } from "../../../ui/RadioGroup";
import { Switch } from "../../../ui/Switch";
import { NetworkSettingsCard } from "../NetworkSettingsCard";
import { WslSettingsCard } from "../WslSettingsCard";
import { Bell, ChevronDown, Shield, TrendingDown, Globe } from "lucide-react";
import type { UpstreamRetryPolicy } from "../../../services/settings/settings";
import {
  cloneUpstreamRetryPolicy,
  validateUpstreamRetryPolicy,
} from "../../../services/gateway/upstreamRetryPolicy";
import { RetryPolicyFields } from "../../gateway/RetryPolicyFields";
import { cn } from "../../../utils/cn";
import { UpstreamErrorResponseRulesCard } from "../UpstreamErrorResponseRulesCard";

export type CliManagerAvailability = "checking" | "available" | "unavailable";

export type CliManagerGeneralTabProps = {
  rectifierAvailable: CliManagerAvailability;
  settingsReadErrorMessage: string | null;
  settingsWriteBlocked: boolean;
  rectifierSaving: boolean;
  rectifier: GatewayRectifierSettingsPatch;
  onPersistRectifier: (patch: Partial<GatewayRectifierSettingsPatch>) => Promise<void> | void;

  circuitBreakerNoticeEnabled: boolean;
  circuitBreakerNoticeSaving: boolean;
  onPersistCircuitBreakerNotice: (enable: boolean) => Promise<void> | void;

  codexSessionIdCompletionEnabled: boolean;
  codexSessionIdCompletionSaving: boolean;
  onPersistCodexSessionIdCompletion: (enable: boolean) => Promise<void> | void;

  cacheAnomalyMonitorEnabled: boolean;
  cacheAnomalyMonitorSaving: boolean;
  onPersistCacheAnomalyMonitor: (enable: boolean) => Promise<void> | void;

  taskCompleteNotifyEnabled: boolean;
  taskCompleteNotifySaving: boolean;
  onPersistTaskCompleteNotify: (enable: boolean) => Promise<void> | void;

  notificationSoundEnabled: boolean;
  notificationSoundSaving: boolean;
  onPersistNotificationSound: (enable: boolean) => Promise<void> | void;

  appSettings: AppSettings | null;
  commonSettingsSaving: boolean;
  onPersistCommonSettings: (
    patch: Partial<AppSettings> & { upstream_proxy_password?: SensitiveStringUpdate }
  ) => Promise<AppSettings | null>;

  upstreamFirstByteTimeoutSeconds: number;
  setUpstreamFirstByteTimeoutSeconds: (value: number) => void;
  upstreamStreamIdleTimeoutSeconds: number;
  setUpstreamStreamIdleTimeoutSeconds: (value: number) => void;
  streamInternalErrorGuardMs: number;
  setStreamInternalErrorGuardMs: (value: number) => void;
  upstreamRequestTimeoutNonStreamingSeconds: number;
  setUpstreamRequestTimeoutNonStreamingSeconds: (value: number) => void;

  providerCooldownSeconds: number;
  setProviderCooldownSeconds: (value: number) => void;
  providerFailbackStrategy: AppSettings["provider_failback_strategy"];
  setProviderFailbackStrategy: (value: AppSettings["provider_failback_strategy"]) => void;
  naturalProbeMaxWaitSeconds: number;
  setNaturalProbeMaxWaitSeconds: (value: number) => void;
  providerBaseUrlPingCacheTtlSeconds: number;
  setProviderBaseUrlPingCacheTtlSeconds: (value: number) => void;
  circuitBreakerFailureThreshold: number;
  setCircuitBreakerFailureThreshold: (value: number) => void;
  circuitBreakerOpenDurationMinutes: number;
  setCircuitBreakerOpenDurationMinutes: (value: number) => void;

  upstreamRetryPolicy: UpstreamRetryPolicy;
  setUpstreamRetryPolicy: (value: UpstreamRetryPolicy) => void;

  blurOnEnter: (e: ReactKeyboardEvent<HTMLInputElement>) => void;
};

export function CliManagerGeneralTab({
  rectifierAvailable,
  settingsReadErrorMessage,
  settingsWriteBlocked,
  rectifierSaving,
  rectifier,
  onPersistRectifier,
  circuitBreakerNoticeEnabled,
  circuitBreakerNoticeSaving,
  onPersistCircuitBreakerNotice,
  codexSessionIdCompletionEnabled,
  codexSessionIdCompletionSaving,
  onPersistCodexSessionIdCompletion,
  cacheAnomalyMonitorEnabled,
  cacheAnomalyMonitorSaving,
  onPersistCacheAnomalyMonitor,
  taskCompleteNotifyEnabled,
  taskCompleteNotifySaving,
  onPersistTaskCompleteNotify,
  notificationSoundEnabled,
  notificationSoundSaving,
  onPersistNotificationSound,
  appSettings,
  commonSettingsSaving,
  onPersistCommonSettings,
  upstreamFirstByteTimeoutSeconds,
  setUpstreamFirstByteTimeoutSeconds,
  upstreamStreamIdleTimeoutSeconds,
  setUpstreamStreamIdleTimeoutSeconds,
  streamInternalErrorGuardMs,
  setStreamInternalErrorGuardMs,
  upstreamRequestTimeoutNonStreamingSeconds,
  setUpstreamRequestTimeoutNonStreamingSeconds,
  providerCooldownSeconds,
  setProviderCooldownSeconds,
  providerFailbackStrategy,
  setProviderFailbackStrategy,
  naturalProbeMaxWaitSeconds,
  setNaturalProbeMaxWaitSeconds,
  providerBaseUrlPingCacheTtlSeconds,
  setProviderBaseUrlPingCacheTtlSeconds,
  circuitBreakerFailureThreshold,
  setCircuitBreakerFailureThreshold,
  circuitBreakerOpenDurationMinutes,
  setCircuitBreakerOpenDurationMinutes,
  upstreamRetryPolicy,
  setUpstreamRetryPolicy,
  blurOnEnter,
}: CliManagerGeneralTabProps) {
  const navigate = useNavigate();
  const settingsUnavailable = rectifierAvailable !== "available";
  const rectifierDisabled = rectifierSaving || settingsUnavailable || settingsWriteBlocked;
  const circuitNoticeDisabled =
    circuitBreakerNoticeSaving || settingsUnavailable || settingsWriteBlocked;
  const codexCompletionDisabled =
    codexSessionIdCompletionSaving || settingsUnavailable || settingsWriteBlocked;
  const taskNotifyDisabled =
    taskCompleteNotifySaving || settingsUnavailable || settingsWriteBlocked;
  const notificationSoundDisabled =
    notificationSoundSaving || settingsUnavailable || settingsWriteBlocked;
  const cacheMonitorDisabled =
    cacheAnomalyMonitorSaving || settingsUnavailable || settingsWriteBlocked;
  const commonSettingsDisabled =
    commonSettingsSaving || settingsUnavailable || settingsWriteBlocked;

  return (
    <div className="space-y-6">
      <Card className="overflow-hidden">
        <div className="border-b border-border p-6">
          <h2 className="text-base font-semibold text-foreground">通用配置</h2>
          <p className="mt-1 text-sm text-muted-foreground">网关整流、通知、超时与熔断策略。</p>
        </div>

        {settingsReadErrorMessage ? (
          <div className="border-b border-amber-200 bg-amber-50 px-6 py-4 text-sm text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-200">
            {settingsReadErrorMessage}
          </div>
        ) : null}

        {rectifierAvailable === "unavailable" ? (
          <div className="text-sm text-muted-foreground text-center py-8">数据不可用</div>
        ) : (
          <div className="p-6 space-y-6">
            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-3">
                <Shield className="h-4 w-4 text-muted-foreground" />
                网关整流器
              </h3>
              <div className="divide-y divide-border">
                <SettingsRow label="详细供应商错误信息" subtitle="在日志中显示完整的上游错误详情。">
                  <Switch
                    checked={rectifier.verbose_provider_error}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ verbose_provider_error: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="拦截 Anthropic Warmup 请求"
                  subtitle="自动拦截并响应 Anthropic 的预热请求，避免计费。"
                >
                  <Switch
                    checked={rectifier.intercept_anthropic_warmup_requests}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ intercept_anthropic_warmup_requests: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="Thinking 签名整流器"
                  subtitle="自动修复 extended thinking 相关的签名问题。"
                >
                  <Switch
                    checked={rectifier.enable_thinking_signature_rectifier}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ enable_thinking_signature_rectifier: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="Thinking 预算整流器"
                  subtitle="自动修复 thinking budget 相关的参数问题。"
                >
                  <Switch
                    checked={rectifier.enable_thinking_budget_rectifier}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ enable_thinking_budget_rectifier: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="Billing Header 整流器"
                  subtitle="自动移除 Claude 请求里的 billing header system 块。适合OAuth用户"
                >
                  <Switch
                    checked={rectifier.enable_billing_header_rectifier}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ enable_billing_header_rectifier: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="Claude metadata.user_id 注入"
                  subtitle="为 Claude 请求自动注入 metadata.user_id 字段。"
                >
                  <Switch
                    checked={rectifier.enable_claude_metadata_user_id_injection}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({
                        enable_claude_metadata_user_id_injection: checked,
                      })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="响应整流（FluxFix）"
                  subtitle="自动修复编码、SSE 格式、截断 JSON 等常见响应问题。"
                >
                  <Switch
                    checked={rectifier.enable_response_fixer}
                    onCheckedChange={(checked) =>
                      void onPersistRectifier({ enable_response_fixer: checked })
                    }
                    disabled={rectifierDisabled}
                  />
                </SettingsRow>
                {rectifier.enable_response_fixer && (
                  <>
                    <SettingsRow label="修复编码问题" className="pl-6">
                      <Switch
                        checked={rectifier.response_fixer_fix_encoding}
                        onCheckedChange={(checked) =>
                          void onPersistRectifier({ response_fixer_fix_encoding: checked })
                        }
                        disabled={rectifierDisabled}
                      />
                    </SettingsRow>
                    <SettingsRow label="修复 SSE 格式" className="pl-6">
                      <Switch
                        checked={rectifier.response_fixer_fix_sse_format}
                        onCheckedChange={(checked) =>
                          void onPersistRectifier({ response_fixer_fix_sse_format: checked })
                        }
                        disabled={rectifierDisabled}
                      />
                    </SettingsRow>
                    <SettingsRow label="修复截断的 JSON" className="pl-6">
                      <Switch
                        checked={rectifier.response_fixer_fix_truncated_json}
                        onCheckedChange={(checked) =>
                          void onPersistRectifier({ response_fixer_fix_truncated_json: checked })
                        }
                        disabled={rectifierDisabled}
                      />
                    </SettingsRow>
                  </>
                )}
                <SettingsRow
                  label="Codex Session ID 补全"
                  subtitle="当 Codex 请求仅提供 session_id 或 prompt_cache_key 之一时，自动补全另一侧；若两者均缺失，则生成并稳定复用会话标识。"
                >
                  <Switch
                    checked={codexSessionIdCompletionEnabled}
                    onCheckedChange={(checked) => void onPersistCodexSessionIdCompletion(checked)}
                    disabled={codexCompletionDisabled}
                  />
                </SettingsRow>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-1">
                <Bell className="h-4 w-4 text-muted-foreground" />
                通知
              </h3>
              <p className="text-xs text-muted-foreground mb-3">
                控制系统通知与音效提醒行为。
                <span className="ml-1 text-amber-600/80 dark:text-amber-400/80">
                  * 需在系统设置中授予通知权限
                </span>
              </p>
              <div className="divide-y divide-border">
                <SettingsRow
                  label="任务结束提醒"
                  subtitle="当 AI CLI 工具（Claude/Gemini：30 秒；Codex：120 秒）请求结束后静默无新请求时，发送系统通知提醒。"
                >
                  <Switch
                    checked={taskCompleteNotifyEnabled}
                    onCheckedChange={(checked) => void onPersistTaskCompleteNotify(checked)}
                    disabled={taskNotifyDisabled}
                  />
                </SettingsRow>
                <SettingsRow label="熔断通知" subtitle="当服务熔断触发或恢复时，主动发送系统通知。">
                  <Switch
                    checked={circuitBreakerNoticeEnabled}
                    onCheckedChange={(checked) => void onPersistCircuitBreakerNotice(checked)}
                    disabled={circuitNoticeDisabled}
                  />
                </SettingsRow>
                <SettingsRow
                  label="通知音效"
                  subtitle="使用自定义提示音代替系统默认通知音效，避免重复响铃。"
                >
                  <Switch
                    checked={notificationSoundEnabled}
                    onCheckedChange={(checked) => void onPersistNotificationSound(checked)}
                    disabled={notificationSoundDisabled}
                  />
                </SettingsRow>
              </div>
            </div>

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-1">
                <TrendingDown className="h-4 w-4 text-muted-foreground" />
                缓存异常监测（实验）
              </h3>
              <p className="text-xs text-muted-foreground mb-3">
                {CACHE_ANOMALY_MONITOR_GUIDE_COPY.overview}
              </p>
              <div className="divide-y divide-border">
                <SettingsRow
                  label="启用缓存异常监测"
                  subtitle={`${CACHE_ANOMALY_MONITOR_GUIDE_COPY.trigger} ${CACHE_ANOMALY_MONITOR_GUIDE_COPY.metric}`}
                >
                  <Switch
                    checked={cacheAnomalyMonitorEnabled}
                    onCheckedChange={(checked) => void onPersistCacheAnomalyMonitor(checked)}
                    disabled={cacheMonitorDisabled}
                  />
                </SettingsRow>
              </div>
              <div className="mt-3 space-y-1 text-xs text-muted-foreground">
                <p>{CACHE_ANOMALY_MONITOR_GUIDE_COPY.coldStart}</p>
                <p>{CACHE_ANOMALY_MONITOR_GUIDE_COPY.nonCachingModel}</p>
                <p>{CACHE_ANOMALY_MONITOR_GUIDE_COPY.thresholds}</p>
              </div>
              <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                <span>
                  提示：告警会以 <span className="font-mono">WARN</span>{" "}
                  写入「控制台」页（无需开启调试日志）。
                </span>
                <Button size="sm" variant="secondary" onClick={() => navigate("/console")}>
                  打开控制台
                </Button>
              </div>
            </div>

            {appSettings ? (
              <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
                <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-3">
                  <Shield className="h-4 w-4 text-muted-foreground" />
                  启动与恢复
                </h3>
                <div className="divide-y divide-border">
                  <SettingsRow
                    label="启动时 CLI 代理自愈"
                    subtitle="应用启动后仅修复异常退出导致的 CLI 代理残留状态，不会主动改写当前配置。建议保持开启。"
                  >
                    <Switch
                      checked={appSettings.enable_cli_proxy_startup_recovery}
                      onCheckedChange={(checked) =>
                        void onPersistCommonSettings({
                          enable_cli_proxy_startup_recovery: checked,
                        })
                      }
                      disabled={commonSettingsDisabled}
                    />
                  </SettingsRow>
                </div>
              </div>
            ) : null}

            {appSettings ? (
              <>
                <NetworkSettingsCard
                  available={rectifierAvailable === "available"}
                  saving={commonSettingsDisabled}
                  settings={appSettings}
                  onPersistSettings={onPersistCommonSettings}
                />
                <WslSettingsCard
                  available={rectifierAvailable === "available"}
                  saving={commonSettingsDisabled}
                  settings={appSettings}
                />
                <UpstreamProxySettingsCard
                  available={rectifierAvailable === "available"}
                  saving={commonSettingsDisabled}
                  settings={appSettings}
                  onPersistSettings={onPersistCommonSettings}
                />
              </>
            ) : null}

            <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
              <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-1">
                <Shield className="h-4 w-4 text-muted-foreground" />
                超时策略
              </h3>
              <p className="text-xs text-muted-foreground mb-3">
                控制上游请求的超时行为。0 表示禁用（交由上游/网络自行超时）。
              </p>
              <div className="divide-y divide-border">
                <SettingsRow
                  label="首字节超时（0=禁用）"
                  subtitle="等待上游返回第一个字节的最大时间。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={upstreamFirstByteTimeoutSeconds}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setUpstreamFirstByteTimeoutSeconds(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 0 || next > 3600) {
                          toast("上游首字节超时必须为 0-3600 秒");
                          setUpstreamFirstByteTimeoutSeconds(
                            appSettings.upstream_first_byte_timeout_seconds
                          );
                          return;
                        }
                        void onPersistCommonSettings({ upstream_first_byte_timeout_seconds: next });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={0}
                      max={3600}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">秒</span>
                  </div>
                </SettingsRow>

                <SettingsRow
                  label="流内部错误观察窗口"
                  subtitle="从首个有效输出开始暂存响应，以便在提交给客户端前识别并重试流内部错误。0 表示不额外等待。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={streamInternalErrorGuardMs}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setStreamInternalErrorGuardMs(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (
                          !Number.isFinite(next) ||
                          next < 0 ||
                          next > MAX_STREAM_INTERNAL_ERROR_GUARD_MS
                        ) {
                          toast(
                            `流内部错误观察窗口必须为 0-${MAX_STREAM_INTERNAL_ERROR_GUARD_MS} 毫秒`
                          );
                          setStreamInternalErrorGuardMs(appSettings.stream_internal_error_guard_ms);
                          return;
                        }
                        void onPersistCommonSettings({ stream_internal_error_guard_ms: next });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "6rem" }}
                      min={0}
                      max={MAX_STREAM_INTERNAL_ERROR_GUARD_MS}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">毫秒</span>
                  </div>
                </SettingsRow>

                <SettingsRow
                  label="流式空闲超时（0=禁用，启用时最小60秒）"
                  subtitle="流式响应中两次数据之间的最大静默时间。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={upstreamStreamIdleTimeoutSeconds}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setUpstreamStreamIdleTimeoutSeconds(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (
                          !Number.isFinite(next) ||
                          next < 0 ||
                          next > 3600 ||
                          (next > 0 && next < 60)
                        ) {
                          toast("上游流式空闲超时必须为 0（禁用）或 60-3600 秒");
                          setUpstreamStreamIdleTimeoutSeconds(
                            appSettings.upstream_stream_idle_timeout_seconds
                          );
                          return;
                        }
                        void onPersistCommonSettings({
                          upstream_stream_idle_timeout_seconds: next,
                        });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={0}
                      max={3600}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">秒</span>
                  </div>
                </SettingsRow>

                <SettingsRow label="非流式总超时（0=禁用）" subtitle="非流式请求的总超时时间。">
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={upstreamRequestTimeoutNonStreamingSeconds}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next))
                          setUpstreamRequestTimeoutNonStreamingSeconds(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 0 || next > 86400) {
                          toast("上游非流式总超时必须为 0-86400 秒");
                          setUpstreamRequestTimeoutNonStreamingSeconds(
                            appSettings.upstream_request_timeout_non_streaming_seconds
                          );
                          return;
                        }
                        void onPersistCommonSettings({
                          upstream_request_timeout_non_streaming_seconds: next,
                        });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={0}
                      max={86400}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">秒</span>
                  </div>
                </SettingsRow>
              </div>
            </div>

            <CollapsibleSettingsCard
              icon={<Shield className="h-5 w-5 text-white" />}
              title="熔断与重试"
              subtitle="控制 Provider 失败后的冷却、瞬时重试与熔断行为。"
              iconClassName="bg-orange-500"
            >
              <div className="divide-y divide-border">
                <SettingsRow
                  label="回切策略"
                  subtitle="自然回切在当前会话完成压缩后的下一轮优先检查；积极回切在同一会话的每一轮都检查更高优先级 Provider。"
                >
                  <RadioGroup
                    name="provider-failback-strategy"
                    ariaLabel="回切策略"
                    value={providerFailbackStrategy}
                    onChange={(value) => {
                      const next = value === "aggressive" ? "aggressive" : "natural";
                      setProviderFailbackStrategy(next);
                      void onPersistCommonSettings({ provider_failback_strategy: next });
                    }}
                    options={[
                      {
                        value: "natural",
                        label: "自然回切",
                        description:
                          "压缩完成后的下一轮优先检查；无可靠压缩信号时使用全局最大等待兜底。",
                      },
                      {
                        value: "aggressive",
                        label: "积极回切",
                        description:
                          "当前会话每一轮都检查更高优先级 Provider；仍受试探最短间隔和单飞限制。",
                      },
                    ]}
                    disabled={commonSettingsDisabled}
                  />
                </SettingsRow>

                {providerFailbackStrategy === "natural" ? (
                  <SettingsRow
                    label="自然模式最长回切等待"
                    subtitle="高优先级 Provider 出现会计入熔断的失败后，即使仍显示 CLOSED，超过此时间也会由下一条合格真实请求直接尝试回切；OPEN/HALF_OPEN 则申请单飞探测。Provider 全局计时，不按会话重复。"
                  >
                    <div className="flex items-center gap-2">
                      <Input
                        aria-label="自然模式最长回切等待"
                        type="number"
                        value={naturalProbeMaxWaitSeconds}
                        onChange={(e) => {
                          const next = e.currentTarget.valueAsNumber;
                          if (Number.isFinite(next)) setNaturalProbeMaxWaitSeconds(next);
                        }}
                        onBlur={(e) => {
                          if (!appSettings) return;
                          const next = e.currentTarget.valueAsNumber;
                          if (!Number.isFinite(next) || next < 1 || next > 86400) {
                            toast("自然模式最长回切等待必须为 1-86400 秒");
                            setNaturalProbeMaxWaitSeconds(
                              appSettings.natural_probe_max_wait_seconds
                            );
                            return;
                          }
                          void onPersistCommonSettings({
                            natural_probe_max_wait_seconds: next,
                          });
                        }}
                        onKeyDown={blurOnEnter}
                        style={{ width: "6rem" }}
                        min={1}
                        max={86400}
                        disabled={commonSettingsDisabled}
                      />
                      <span className="w-8 text-sm text-muted-foreground">秒</span>
                    </div>
                  </SettingsRow>
                ) : null}

                <SettingsRow
                  label="Provider 冷却 / 试探最短间隔"
                  subtitle="失败 Provider 的短暂冷却，也是自动试探的最短实际调用间隔；策略触发不会绕过它。0 表示串行下一轮可立即试探。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={providerCooldownSeconds}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setProviderCooldownSeconds(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 0 || next > 3600) {
                          toast("短熔断冷却必须为 0-3600 秒");
                          setProviderCooldownSeconds(appSettings.provider_cooldown_seconds);
                          return;
                        }
                        void onPersistCommonSettings({ provider_cooldown_seconds: next });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={0}
                      max={3600}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">秒</span>
                  </div>
                </SettingsRow>

                <SettingsRow
                  label="Ping 选择缓存 TTL"
                  subtitle="Provider 可用性 ping 结果的缓存有效期。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={providerBaseUrlPingCacheTtlSeconds}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setProviderBaseUrlPingCacheTtlSeconds(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 1 || next > 3600) {
                          toast("Ping 选择缓存 TTL 必须为 1-3600 秒");
                          setProviderBaseUrlPingCacheTtlSeconds(
                            appSettings.provider_base_url_ping_cache_ttl_seconds
                          );
                          return;
                        }
                        void onPersistCommonSettings({
                          provider_base_url_ping_cache_ttl_seconds: next,
                        });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={1}
                      max={3600}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">秒</span>
                  </div>
                </SettingsRow>

                <SettingsRow label="熔断阈值" subtitle="连续失败达到此次数后触发熔断。">
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={circuitBreakerFailureThreshold}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setCircuitBreakerFailureThreshold(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 1 || next > 50) {
                          toast("熔断阈值必须为 1-50");
                          setCircuitBreakerFailureThreshold(
                            appSettings.circuit_breaker_failure_threshold
                          );
                          return;
                        }
                        void onPersistCommonSettings({ circuit_breaker_failure_threshold: next });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={1}
                      max={50}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">次</span>
                  </div>
                </SettingsRow>

                <SettingsRow
                  label="最长熔断等待（可提前试探）"
                  subtitle="达到此时间只允许下一条合格请求申请单飞试探，不会自动解除熔断；满足策略触发和最短间隔时可以提前试探。"
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      value={circuitBreakerOpenDurationMinutes}
                      onChange={(e) => {
                        const next = e.currentTarget.valueAsNumber;
                        if (Number.isFinite(next)) setCircuitBreakerOpenDurationMinutes(next);
                      }}
                      onBlur={(e) => {
                        if (!appSettings) return;
                        const next = e.currentTarget.valueAsNumber;
                        if (!Number.isFinite(next) || next < 1 || next > 1440) {
                          toast("最长熔断等待必须为 1-1440 分钟");
                          setCircuitBreakerOpenDurationMinutes(
                            appSettings.circuit_breaker_open_duration_minutes
                          );
                          return;
                        }
                        void onPersistCommonSettings({
                          circuit_breaker_open_duration_minutes: next,
                        });
                      }}
                      onKeyDown={blurOnEnter}
                      style={{ width: "5rem" }}
                      min={1}
                      max={1440}
                      disabled={commonSettingsDisabled}
                    />
                    <span className="w-8 text-sm text-muted-foreground">分钟</span>
                  </div>
                </SettingsRow>
              </div>
              <div className="mt-5 border-t border-border pt-5">
                <RetryPolicyFields
                  policy={upstreamRetryPolicy}
                  disabled={commonSettingsDisabled}
                  onChange={(next) => {
                    setUpstreamRetryPolicy(next);
                  }}
                />
                <div className="mt-3 flex justify-end">
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={commonSettingsDisabled}
                    onClick={async () => {
                      if (!appSettings) return;
                      const validationMessage = validateUpstreamRetryPolicy(upstreamRetryPolicy);
                      if (validationMessage) {
                        toast(validationMessage);
                        return;
                      }
                      const updated = await onPersistCommonSettings({
                        upstream_retry_policy: upstreamRetryPolicy,
                      });
                      if (updated) {
                        setUpstreamRetryPolicy(
                          cloneUpstreamRetryPolicy(updated.upstream_retry_policy)
                        );
                      }
                    }}
                  >
                    保存重试策略
                  </Button>
                </div>
              </div>
            </CollapsibleSettingsCard>
            {appSettings ? (
              <UpstreamErrorResponseRulesCard
                rules={appSettings.upstream_error_response_rules}
                disabled={commonSettingsDisabled}
                onPersist={(rules) =>
                  onPersistCommonSettings({ upstream_error_response_rules: rules })
                }
              />
            ) : null}
          </div>
        )}
      </Card>
    </div>
  );
}

function CollapsibleSettingsCard({
  icon,
  iconClassName,
  title,
  subtitle,
  children,
}: {
  icon: ReactNode;
  iconClassName?: string;
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-white dark:bg-secondary">
      <button
        type="button"
        className="flex w-full items-center justify-between gap-4 px-6 py-5 text-left transition-colors hover:bg-secondary/50 dark:hover:bg-secondary/40"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
      >
        <div className="flex min-w-0 items-center gap-4">
          <div
            className={cn(
              "flex h-12 w-12 shrink-0 items-center justify-center rounded-full",
              iconClassName
            )}
          >
            {icon}
          </div>
          <div className="min-w-0">
            <div className="text-base font-semibold text-foreground">{title}</div>
            <div className="mt-1 text-sm text-muted-foreground">{subtitle}</div>
          </div>
        </div>
        <ChevronDown
          className={cn(
            "h-5 w-5 shrink-0 text-muted-foreground transition-transform",
            open && "rotate-180"
          )}
        />
      </button>
      {open ? <div className="border-t border-border px-5 py-5">{children}</div> : null}
    </div>
  );
}

type UpstreamProxySettingsCardProps = {
  available: boolean;
  saving: boolean;
  settings: AppSettings;
  onPersistSettings: (
    patch: Partial<AppSettings> & { upstream_proxy_password?: SensitiveStringUpdate }
  ) => Promise<AppSettings | null>;
};

function UpstreamProxySettingsCard({
  available,
  saving,
  settings,
  onPersistSettings,
}: UpstreamProxySettingsCardProps) {
  const [proxyUrl, setProxyUrl] = useState(settings.upstream_proxy_url ?? "");
  const [proxyUsername, setProxyUsername] = useState(settings.upstream_proxy_username ?? "");
  const [proxyPassword, setProxyPassword] = useState("");
  const [clearSavedPassword, setClearSavedPassword] = useState(false);
  const [testingConnection, setTestingConnection] = useState(false);
  const [detectingExitIp, setDetectingExitIp] = useState(false);
  const [hasPendingEdits, setHasPendingEdits] = useState(false);
  const disabled = !available || saving;

  useEffect(() => {
    if (hasPendingEdits) return;
    setProxyUrl(settings.upstream_proxy_url ?? "");
    setProxyUsername(settings.upstream_proxy_username ?? "");
    setProxyPassword("");
    setClearSavedPassword(false);
  }, [
    hasPendingEdits,
    settings.upstream_proxy_password_configured,
    settings.upstream_proxy_url,
    settings.upstream_proxy_username,
  ]);

  function resolveProxyPasswordPatch(): SensitiveStringUpdate {
    if (clearSavedPassword) {
      return { mode: "clear" };
    }
    if (proxyPassword.trim()) {
      return { mode: "replace", value: proxyPassword };
    }
    return { mode: "preserve" };
  }

  function resetProxyDraft() {
    setProxyUrl(settings.upstream_proxy_url);
    setProxyUsername(settings.upstream_proxy_username);
    setProxyPassword("");
    setClearSavedPassword(false);
    setHasPendingEdits(false);
  }

  function validateProxyDraft(options: {
    enabled: boolean;
    requireUrl?: boolean;
    validateUrlWhenPresent?: boolean;
  }) {
    const message = validateUpstreamProxyFields({
      enabled: options.enabled,
      requireUrl: options.requireUrl,
      validateUrlWhenPresent: options.validateUrlWhenPresent,
      url: proxyUrl,
      username: proxyUsername,
      passwordUpdate: resolveProxyPasswordPatch(),
    });
    if (message) {
      toast(message);
      return false;
    }
    return true;
  }

  async function handleProxyEnabledChange(enabled: boolean) {
    if (disabled) return;
    if (enabled && !proxyUrl.trim()) {
      toast("请先输入代理地址");
      return;
    }
    if (!validateProxyDraft({ enabled, validateUrlWhenPresent: enabled })) {
      return;
    }
    const updated = await onPersistSettings({
      upstream_proxy_enabled: enabled,
      upstream_proxy_url: proxyUrl.trim(),
      upstream_proxy_username: proxyUsername.trim(),
      upstream_proxy_password: resolveProxyPasswordPatch(),
    });
    if (updated) {
      setProxyPassword("");
      setClearSavedPassword(false);
      toast.success(enabled ? "代理已启用" : "代理已禁用");
    }
  }

  async function persistProxyFields(options?: { successMessage?: string }) {
    if (disabled) return;
    const trimmedUrl = proxyUrl.trim();
    const trimmedUsername = proxyUsername.trim();
    const sensitiveChanged = clearSavedPassword || proxyPassword.trim().length > 0;
    const fieldsChanged =
      trimmedUrl !== settings.upstream_proxy_url ||
      trimmedUsername !== settings.upstream_proxy_username ||
      sensitiveChanged;

    if (!fieldsChanged) {
      setHasPendingEdits(false);
      return;
    }
    if (settings.upstream_proxy_enabled && !trimmedUrl) {
      toast("代理已启用时地址不能为空");
      resetProxyDraft();
      return;
    }
    if (
      !validateProxyDraft({
        enabled: settings.upstream_proxy_enabled,
        validateUrlWhenPresent: true,
      })
    ) {
      resetProxyDraft();
      return;
    }
    const updated = await onPersistSettings({
      upstream_proxy_url: trimmedUrl,
      upstream_proxy_username: trimmedUsername,
      upstream_proxy_password: resolveProxyPasswordPatch(),
    });
    setHasPendingEdits(false);
    if (!updated) {
      resetProxyDraft();
      return;
    }
    setProxyPassword("");
    setClearSavedPassword(false);
    if (options?.successMessage) {
      toast.success(options.successMessage);
    }
  }

  async function handleTestProxy() {
    if (disabled || testingConnection || detectingExitIp) return;
    const trimmed = proxyUrl.trim();
    if (!trimmed) {
      toast("请先输入代理地址");
      return;
    }
    const validationMessage = validateUpstreamProxyFields({
      requireUrl: true,
      url: trimmed,
      username: proxyUsername,
      password: proxyPassword,
    });
    if (validationMessage) {
      toast(validationMessage);
      return;
    }
    setTestingConnection(true);
    try {
      await gatewayUpstreamProxyTest({
        proxyUrl: trimmed,
        proxyUsername: proxyUsername.trim() || undefined,
        proxyPassword: proxyPassword || undefined,
      });
      toast.success("代理连接测试成功");
    } catch (err) {
      logToConsole("error", "代理连接测试失败", { error: String(err) });
      toast.error(`代理连接测试失败: ${String(err)}`);
    } finally {
      setTestingConnection(false);
    }
  }

  async function handleDetectProxyExitIp() {
    if (disabled || testingConnection || detectingExitIp) return;
    const trimmed = proxyUrl.trim();
    if (!trimmed) {
      toast("请先输入代理地址");
      return;
    }
    const validationMessage = validateUpstreamProxyFields({
      requireUrl: true,
      url: trimmed,
      username: proxyUsername,
      password: proxyPassword,
    });
    if (validationMessage) {
      toast(validationMessage);
      return;
    }
    setDetectingExitIp(true);
    try {
      const exitIp = await gatewayUpstreamProxyDetectIp({
        proxyUrl: trimmed,
        proxyUsername: proxyUsername.trim() || undefined,
        proxyPassword: proxyPassword || undefined,
      });
      toast.success(`代理出口 IP: ${exitIp}`);
    } catch (err) {
      logToConsole("error", "代理出口 IP 检测失败", { error: String(err) });
      toast.error(`代理出口 IP 检测失败: ${String(err)}`);
    } finally {
      setDetectingExitIp(false);
    }
  }

  return (
    <div className="rounded-lg border border-border bg-white dark:bg-secondary p-5">
      <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-1">
        <Globe className="h-4 w-4 text-muted-foreground" />
        上游代理
      </h3>
      <p className="text-xs text-muted-foreground mb-3">
        网关向上游 AI 服务（Claude/Codex/Gemini）发起请求时使用的代理。支持
        http/https/socks5/socks5h 协议。
      </p>
      <div className="divide-y divide-border">
        <SettingsRow label="启用上游代理" subtitle="启用后，所有上游请求将通过指定代理发送。">
          <Switch
            checked={settings.upstream_proxy_enabled}
            onCheckedChange={handleProxyEnabledChange}
            disabled={disabled}
          />
        </SettingsRow>
        <SettingsRow
          label="代理地址"
          subtitle="格式：protocol://host:port（如 socks5://127.0.0.1:1080）"
        >
          <div className="flex flex-wrap items-center gap-2">
            <Input
              type="text"
              value={proxyUrl}
              onChange={(e) => {
                setHasPendingEdits(true);
                setProxyUrl(e.currentTarget.value);
              }}
              onBlur={() =>
                void persistProxyFields({
                  successMessage: settings.upstream_proxy_enabled ? "代理地址已更新" : undefined,
                })
              }
              placeholder="http://127.0.0.1:7890"
              style={{ width: "16rem" }}
              disabled={disabled}
            />
            <Button
              size="sm"
              variant="secondary"
              onClick={handleTestProxy}
              disabled={disabled || testingConnection || detectingExitIp || !proxyUrl.trim()}
            >
              {testingConnection ? "测试中…" : "测试连接"}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={handleDetectProxyExitIp}
              disabled={disabled || testingConnection || detectingExitIp || !proxyUrl.trim()}
            >
              {detectingExitIp ? "检测中…" : "检测出口 IP"}
            </Button>
          </div>
        </SettingsRow>
        <SettingsRow label="用户名" subtitle="可选。建议在此填写，而不是把用户名写进 URL。">
          <Input
            type="text"
            value={proxyUsername}
            onChange={(e) => {
              setHasPendingEdits(true);
              setProxyUsername(e.currentTarget.value);
            }}
            onBlur={() =>
              void persistProxyFields({
                successMessage: settings.upstream_proxy_enabled ? "代理认证信息已更新" : undefined,
              })
            }
            placeholder="proxy-user"
            style={{ width: "16rem" }}
            disabled={disabled}
          />
        </SettingsRow>
        <SettingsRow label="密码" subtitle="可选。密码会单独保存，不需要手动写进代理 URL。">
          <Input
            type="password"
            value={proxyPassword}
            onChange={(e) => {
              setHasPendingEdits(true);
              setProxyPassword(e.currentTarget.value);
              setClearSavedPassword(false);
            }}
            onBlur={() =>
              void persistProxyFields({
                successMessage: settings.upstream_proxy_enabled ? "代理认证信息已更新" : undefined,
              })
            }
            placeholder={
              settings.upstream_proxy_password_configured
                ? "留空表示保留已保存密码"
                : "proxy-password"
            }
            style={{ width: "16rem" }}
            disabled={disabled}
          />
          {settings.upstream_proxy_password_configured ? (
            <div className="flex items-center gap-3 text-xs text-muted-foreground">
              <span>{clearSavedPassword ? "保存后会删除已保存密码" : "已保存代理密码"}</span>
              <button
                type="button"
                className="text-accent hover:text-accent/80"
                disabled={disabled}
                onClick={() => {
                  setHasPendingEdits(true);
                  setProxyPassword("");
                  setClearSavedPassword((prev) => !prev);
                }}
              >
                {clearSavedPassword ? "取消清空" : "清空已保存密码"}
              </button>
            </div>
          ) : null}
        </SettingsRow>
      </div>
    </div>
  );
}
