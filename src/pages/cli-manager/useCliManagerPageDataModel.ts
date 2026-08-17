// Usage: Data-model hook for CLI manager page orchestration.

import { useEffect, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import {
  type ClaudeSettingsPatch,
  type CodexConfigPatch,
  type CodexConfigState,
  type GeminiConfigPatch,
} from "../../services/cli/cliManager";
import {
  codexModelContextRulesEqual,
  normalizeCodexModelContextRules,
  type CodexModelContextRule,
} from "../../services/settings/codexModelContextRules";
import { logToConsole } from "../../services/consoleLog";
import { openDesktopSinglePath } from "../../services/desktop/dialog";
import { openDesktopPath } from "../../services/desktop/opener";
import { type GatewayRectifierSettingsPatch } from "../../services/settings/settingsGatewayRectifier";
import type {
  AppSettings,
  AppSettingsPatch,
  ModelRoutingPolicy,
  UpstreamRetryPolicy,
} from "../../services/settings/settings";
import {
  getSettingsReadProtection,
  SETTINGS_READONLY_MESSAGE,
  useSettingsCircuitBreakerNoticeSetMutation,
  useSettingsCodexModelContextRulesSetMutation,
  useSettingsCodexSessionIdCompletionSetMutation,
  useSettingsGatewayRectifierSetMutation,
  useSettingsPatchMutation,
  useSettingsQuery,
} from "../../query/settings";
import { useProvidersListQuery } from "../../query/providers";
import {
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
} from "../../services/gateway/upstreamRetryPolicy";
import {
  cloneModelRoutingPolicy,
  DEFAULT_MODEL_ROUTING_POLICY,
} from "../../services/gateway/modelRoutingPolicy";
import {
  useCliManagerClaudeInfoQuery,
  useCliManagerClaudeSettingsQuery,
  useCliManagerClaudeSettingsSetMutation,
  useCliManagerCodexConfigQuery,
  useCliManagerCodexConfigSetMutation,
  useCliManagerCodexConfigTomlQuery,
  useCliManagerCodexConfigTomlSetMutation,
  useCliManagerCodexInfoQuery,
  useCliManagerCodexModelCatalogQuery,
  useCliManagerCodexModelCatalogRefresh,
  useCliManagerCodexModelContextCandidatesQuery,
  useCliManagerCodexProviderSyncMutation,
  useCliManagerGeminiConfigQuery,
  useCliManagerGeminiConfigSetMutation,
  useCliManagerGeminiInfoQuery,
} from "../../query/cliManager";
import { formatActionFailureToast } from "../../utils/errors";
import { useGrokTabDataModel } from "../../components/cli-manager/tabs/useGrokTabDataModel";
import type { CodexModelContextRulesSaveResult } from "../../components/cli-manager/tabs/CodexModelContextRulesSection";

export type CliManagerTabKey = "general" | "claude" | "codex" | "cx2cc" | "gemini" | "grok";

export const CLI_MANAGER_TABS: Array<{ key: CliManagerTabKey; label: string }> = [
  { key: "general", label: "通用" },
  { key: "claude", label: "Claude Code" },
  { key: "codex", label: "Codex" },
  { key: "gemini", label: "Gemini" },
  { key: "grok", label: "Grok" },
  { key: "cx2cc", label: "CX2CC" },
];

function cliManagerTabFromSearchParam(value: string | null): CliManagerTabKey | null {
  return CLI_MANAGER_TABS.some((item) => item.key === value) ? (value as CliManagerTabKey) : null;
}

const DEFAULT_RECTIFIER: GatewayRectifierSettingsPatch = {
  verbose_provider_error: true,
  intercept_anthropic_warmup_requests: true,
  enable_thinking_signature_rectifier: true,
  enable_thinking_budget_rectifier: true,
  enable_billing_header_rectifier: true,
  enable_claude_metadata_user_id_injection: true,
  enable_response_fixer: true,
  response_fixer_fix_encoding: true,
  response_fixer_fix_sse_format: true,
  response_fixer_fix_truncated_json: true,
  response_fixer_max_json_depth: 200,
  response_fixer_max_fix_size: 1024 * 1024,
};

export function useCliManagerPageDataModel() {
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const requestedTab = cliManagerTabFromSearchParam(searchParams.get("tab"));
  const [tab, setTab] = useState<CliManagerTabKey>(() => requestedTab ?? "general");

  useEffect(() => {
    if (requestedTab) setTab(requestedTab);
  }, [requestedTab]);

  const grokTabProps = useGrokTabDataModel({ enabled: tab === "grok" });

  const settingsQuery = useSettingsQuery();
  const appSettings = settingsQuery.data ?? null;
  const { settingsReadErrorMessage, settingsWriteBlocked } =
    getSettingsReadProtection(settingsQuery);

  const rectifierAvailable: "checking" | "available" | "unavailable" = settingsQuery.isLoading
    ? "checking"
    : appSettings
      ? "available"
      : "unavailable";

  const rectifierMutation = useSettingsGatewayRectifierSetMutation();
  const circuitBreakerNoticeMutation = useSettingsCircuitBreakerNoticeSetMutation();
  const codexSessionIdCompletionMutation = useSettingsCodexSessionIdCompletionSetMutation();
  const codexModelContextRulesMutation = useSettingsCodexModelContextRulesSetMutation();
  const commonSettingsMutation = useSettingsPatchMutation();

  const rectifierSaving = rectifierMutation.isPending;
  const circuitBreakerNoticeSaving = circuitBreakerNoticeMutation.isPending;
  const codexSessionIdCompletionSaving = codexSessionIdCompletionMutation.isPending;
  const codexModelContextRulesSaving = codexModelContextRulesMutation.isPending;
  const commonSettingsSaving = commonSettingsMutation.isPending;

  const [rectifier, setRectifier] = useState<GatewayRectifierSettingsPatch>(DEFAULT_RECTIFIER);
  const [circuitBreakerNoticeEnabled, setCircuitBreakerNoticeEnabled] = useState(false);
  const [codexSessionIdCompletionEnabled, setCodexSessionIdCompletionEnabled] = useState(true);
  const [upstreamFirstByteTimeoutSeconds, setUpstreamFirstByteTimeoutSeconds] = useState<number>(0);
  const [upstreamStreamIdleTimeoutSeconds, setUpstreamStreamIdleTimeoutSeconds] =
    useState<number>(0);
  const [streamInternalErrorGuardMs, setStreamInternalErrorGuardMs] = useState<number>(500);
  const [upstreamRequestTimeoutNonStreamingSeconds, setUpstreamRequestTimeoutNonStreamingSeconds] =
    useState<number>(0);
  const [providerCooldownSeconds, setProviderCooldownSeconds] = useState<number>(30);
  const [providerFailbackStrategy, setProviderFailbackStrategy] =
    useState<AppSettings["provider_failback_strategy"]>("natural");
  const [naturalProbeMaxWaitSeconds, setNaturalProbeMaxWaitSeconds] = useState<number>(300);
  const [providerBaseUrlPingCacheTtlSeconds, setProviderBaseUrlPingCacheTtlSeconds] =
    useState<number>(60);
  const [circuitBreakerFailureThreshold, setCircuitBreakerFailureThreshold] = useState<number>(5);
  const [circuitBreakerOpenDurationMinutes, setCircuitBreakerOpenDurationMinutes] =
    useState<number>(30);
  const [upstreamRetryPolicy, setUpstreamRetryPolicy] = useState<UpstreamRetryPolicy>(
    DEFAULT_UPSTREAM_RETRY_POLICY
  );
  const [modelRoutingPolicy, setModelRoutingPolicy] = useState<ModelRoutingPolicy>(
    DEFAULT_MODEL_ROUTING_POLICY
  );
  const cacheAnomalyMonitorEnabled = appSettings?.enable_cache_anomaly_monitor ?? false;
  const taskCompleteNotifyEnabled = appSettings?.enable_task_complete_notify ?? true;
  const notificationSoundEnabled = appSettings?.enable_notification_sound ?? true;

  function blockSettingsWrite() {
    toast(settingsReadErrorMessage ?? SETTINGS_READONLY_MESSAGE);
  }

  const claudeInfoQuery = useCliManagerClaudeInfoQuery({ enabled: tab === "claude" });
  const claudeSettingsQuery = useCliManagerClaudeSettingsQuery({ enabled: tab === "claude" });
  const claudeSettingsSetMutation = useCliManagerClaudeSettingsSetMutation();
  const claudeProvidersQuery = useProvidersListQuery("claude", { enabled: tab === "claude" });

  const claudeInfo = claudeInfoQuery.data ?? null;
  const claudeSettings = claudeSettingsQuery.data ?? null;
  const claudeProviders = claudeProvidersQuery.data ?? null;
  const claudeAvailable: "checking" | "available" | "unavailable" =
    claudeInfoQuery.isFetching && !claudeInfo
      ? "checking"
      : claudeInfo
        ? "available"
        : "unavailable";
  const claudeLoading = claudeInfoQuery.isFetching;
  const claudeSettingsLoading = claudeSettingsQuery.isFetching;
  const claudeSettingsSaving = claudeSettingsSetMutation.isPending;

  const codexInfoQuery = useCliManagerCodexInfoQuery({ enabled: tab === "codex" });
  const codexConfigQuery = useCliManagerCodexConfigQuery({ enabled: tab === "codex" });
  const codexConfigTomlQuery = useCliManagerCodexConfigTomlQuery({ enabled: tab === "codex" });
  const codexConfigSetMutation = useCliManagerCodexConfigSetMutation();
  const codexConfigTomlSetMutation = useCliManagerCodexConfigTomlSetMutation();
  const codexProviderSyncMutation = useCliManagerCodexProviderSyncMutation();
  const refreshCodexModelCatalog = useCliManagerCodexModelCatalogRefresh();
  const codexModelCatalogQuery = useCliManagerCodexModelCatalogQuery({
    enabled:
      tab === "codex" && codexInfoQuery.data?.found === true && codexConfigQuery.data != null,
    snapshot: {
      configPath: codexConfigQuery.data?.config_path,
      executablePath: codexInfoQuery.data?.executable_path,
      cliVersion: codexInfoQuery.data?.version,
    },
  });
  const codexModelContextCandidatesQuery = useCliManagerCodexModelContextCandidatesQuery({
    enabled: tab === "codex" && codexConfigQuery.data != null,
    snapshot: {
      configPath: codexConfigQuery.data?.config_path,
      executablePath: codexInfoQuery.data?.executable_path,
      cliVersion: codexInfoQuery.data?.version,
    },
  });

  const codexInfo = codexInfoQuery.data ?? null;
  const codexConfig = codexConfigQuery.data ?? null;
  const codexConfigToml = codexConfigTomlQuery.data ?? null;
  const codexModelCatalog = codexModelCatalogQuery.isError
    ? null
    : (codexModelCatalogQuery.data ?? null);
  const candidateSnapshot = codexModelContextCandidatesQuery.data?.snapshot;
  const codexModelContextCandidatesCurrent = Boolean(
    candidateSnapshot &&
    codexConfigQuery.data &&
    codexInfoQuery.data &&
    candidateSnapshot.config_path === codexConfigQuery.data.config_path &&
    candidateSnapshot.executable_path === codexInfoQuery.data.executable_path &&
    candidateSnapshot.cli_version === codexInfoQuery.data.version
  );
  const codexModelContextCandidates =
    !codexModelContextCandidatesQuery.isError && codexModelContextCandidatesCurrent
      ? (codexModelContextCandidatesQuery.data ?? null)
      : null;
  const codexAvailable: "checking" | "available" | "unavailable" =
    codexInfoQuery.isFetching && !codexInfo
      ? "checking"
      : codexInfo?.found === true
        ? "available"
        : "unavailable";
  const codexLoading = codexInfoQuery.isFetching;
  const codexConfigLoading = codexConfigQuery.isFetching;
  const codexConfigTomlLoading = codexConfigTomlQuery.isFetching;
  const codexConfigTomlSaving = codexConfigTomlSetMutation.isPending;
  const codexProviderSyncing = codexProviderSyncMutation.isPending;
  const codexConfigWriting =
    codexConfigSetMutation.isPending || codexConfigTomlSaving || codexModelContextRulesSaving;
  const codexConfigSaving = codexConfigWriting;
  const codexModelCatalogLoading = codexModelCatalogQuery.isFetching;
  const codexModelCatalogError = codexModelCatalogQuery.isError;
  const codexModelContextCandidatesLoading = codexModelContextCandidatesQuery.isFetching;
  const codexModelContextCandidatesError = codexModelContextCandidatesQuery.isError;

  const geminiInfoQuery = useCliManagerGeminiInfoQuery({ enabled: tab === "gemini" });
  const geminiConfigQuery = useCliManagerGeminiConfigQuery({ enabled: tab === "gemini" });
  const geminiConfigSetMutation = useCliManagerGeminiConfigSetMutation();
  const geminiInfo = geminiInfoQuery.data ?? null;
  const geminiConfig = geminiConfigQuery.data ?? null;
  const geminiAvailable: "checking" | "available" | "unavailable" =
    geminiInfoQuery.isFetching && !geminiInfo
      ? "checking"
      : geminiInfo
        ? "available"
        : "unavailable";
  const geminiLoading = geminiInfoQuery.isFetching;
  const geminiConfigLoading = geminiConfigQuery.isFetching;
  const geminiConfigSaving = geminiConfigSetMutation.isPending;

  useEffect(() => {
    if (!appSettings) return;
    setRectifier({
      verbose_provider_error: appSettings.verbose_provider_error,
      intercept_anthropic_warmup_requests: appSettings.intercept_anthropic_warmup_requests,
      enable_thinking_signature_rectifier: appSettings.enable_thinking_signature_rectifier,
      enable_thinking_budget_rectifier: appSettings.enable_thinking_budget_rectifier,
      enable_billing_header_rectifier: appSettings.enable_billing_header_rectifier,
      enable_claude_metadata_user_id_injection:
        appSettings.enable_claude_metadata_user_id_injection,
      enable_response_fixer: appSettings.enable_response_fixer,
      response_fixer_fix_encoding: appSettings.response_fixer_fix_encoding,
      response_fixer_fix_sse_format: appSettings.response_fixer_fix_sse_format,
      response_fixer_fix_truncated_json: appSettings.response_fixer_fix_truncated_json,
      response_fixer_max_json_depth: appSettings.response_fixer_max_json_depth,
      response_fixer_max_fix_size: appSettings.response_fixer_max_fix_size,
    });
    setCircuitBreakerNoticeEnabled(appSettings.enable_circuit_breaker_notice ?? false);
    setCodexSessionIdCompletionEnabled(appSettings.enable_codex_session_id_completion ?? true);
    setUpstreamFirstByteTimeoutSeconds(appSettings.upstream_first_byte_timeout_seconds);
    setUpstreamStreamIdleTimeoutSeconds(appSettings.upstream_stream_idle_timeout_seconds);
    setStreamInternalErrorGuardMs(appSettings.stream_internal_error_guard_ms);
    setUpstreamRequestTimeoutNonStreamingSeconds(
      appSettings.upstream_request_timeout_non_streaming_seconds
    );
    setProviderCooldownSeconds(appSettings.provider_cooldown_seconds);
    setProviderFailbackStrategy(appSettings.provider_failback_strategy);
    setNaturalProbeMaxWaitSeconds(appSettings.natural_probe_max_wait_seconds);
    setProviderBaseUrlPingCacheTtlSeconds(appSettings.provider_base_url_ping_cache_ttl_seconds);
    setCircuitBreakerFailureThreshold(appSettings.circuit_breaker_failure_threshold);
    setCircuitBreakerOpenDurationMinutes(appSettings.circuit_breaker_open_duration_minutes);
    setUpstreamRetryPolicy(cloneUpstreamRetryPolicy(appSettings.upstream_retry_policy));
    setModelRoutingPolicy(cloneModelRoutingPolicy(appSettings.model_routing_policy));
  }, [appSettings]);

  async function persistRectifier(patch: Partial<GatewayRectifierSettingsPatch>) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (rectifierSaving) return;
    if (rectifierAvailable !== "available") return;

    const prev = rectifier;
    const next = { ...prev, ...patch };
    setRectifier(next);
    try {
      const updated = await rectifierMutation.mutateAsync(next);
      if (!updated) {
        setRectifier(prev);
        return;
      }

      setRectifier({
        verbose_provider_error: updated.verbose_provider_error,
        intercept_anthropic_warmup_requests: updated.intercept_anthropic_warmup_requests,
        enable_thinking_signature_rectifier: updated.enable_thinking_signature_rectifier,
        enable_thinking_budget_rectifier: updated.enable_thinking_budget_rectifier,
        enable_billing_header_rectifier: updated.enable_billing_header_rectifier,
        enable_claude_metadata_user_id_injection: updated.enable_claude_metadata_user_id_injection,
        enable_response_fixer: updated.enable_response_fixer,
        response_fixer_fix_encoding: updated.response_fixer_fix_encoding,
        response_fixer_fix_sse_format: updated.response_fixer_fix_sse_format,
        response_fixer_fix_truncated_json: updated.response_fixer_fix_truncated_json,
        response_fixer_max_json_depth: updated.response_fixer_max_json_depth,
        response_fixer_max_fix_size: updated.response_fixer_max_fix_size,
      });
    } catch (err) {
      logToConsole("error", "更新网关整流配置失败", { error: String(err) });
      toast("更新网关整流配置失败：请稍后重试");
      setRectifier(prev);
    }
  }

  async function persistCircuitBreakerNotice(enable: boolean) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (circuitBreakerNoticeSaving) return;
    if (rectifierAvailable !== "available") return;

    const prev = circuitBreakerNoticeEnabled;
    setCircuitBreakerNoticeEnabled(enable);
    try {
      const updated = await circuitBreakerNoticeMutation.mutateAsync(enable);
      if (!updated) {
        setCircuitBreakerNoticeEnabled(prev);
        return;
      }

      setCircuitBreakerNoticeEnabled(updated.enable_circuit_breaker_notice ?? enable);
      toast(enable ? "已开启熔断通知" : "已关闭熔断通知");
    } catch (err) {
      logToConsole("error", "更新熔断通知配置失败", { error: String(err) });
      toast("更新熔断通知配置失败：请稍后重试");
      setCircuitBreakerNoticeEnabled(prev);
    }
  }

  async function persistCodexSessionIdCompletion(enable: boolean) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (codexSessionIdCompletionSaving) return;
    if (rectifierAvailable !== "available") return;

    const prev = codexSessionIdCompletionEnabled;
    setCodexSessionIdCompletionEnabled(enable);
    try {
      const updated = await codexSessionIdCompletionMutation.mutateAsync(enable);
      if (!updated) {
        setCodexSessionIdCompletionEnabled(prev);
        return;
      }

      setCodexSessionIdCompletionEnabled(updated.enable_codex_session_id_completion ?? enable);
      toast(enable ? "已开启 Codex Session ID 补全" : "已关闭 Codex Session ID 补全");
    } catch (err) {
      logToConsole("error", "更新 Codex Session ID 补全配置失败", { error: String(err) });
      toast("更新 Codex Session ID 补全配置失败：请稍后重试");
      setCodexSessionIdCompletionEnabled(prev);
    }
  }

  async function persistCacheAnomalyMonitor(enable: boolean) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (commonSettingsSaving) return;
    if (rectifierAvailable !== "available") return;

    try {
      const updated = await persistCommonSettings({ enable_cache_anomaly_monitor: enable });
      if (!updated) return;
      const next = updated.enable_cache_anomaly_monitor ?? enable;
      toast(next ? "已开启缓存异常监测（实验）" : "已关闭缓存异常监测（实验）");
    } catch {}
  }

  async function persistTaskCompleteNotify(enable: boolean) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (commonSettingsSaving) return;
    if (rectifierAvailable !== "available") return;

    try {
      const updated = await persistCommonSettings({ enable_task_complete_notify: enable });
      if (!updated) return;
      const next = updated.enable_task_complete_notify ?? enable;
      toast(next ? "已开启任务结束提醒" : "已关闭任务结束提醒");
    } catch {}
  }

  async function persistNotificationSound(enable: boolean) {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return;
    }
    if (commonSettingsSaving) return;
    if (rectifierAvailable !== "available") return;

    try {
      const updated = await persistCommonSettings({ enable_notification_sound: enable });
      if (!updated) return;
      const next = updated.enable_notification_sound ?? enable;
      toast(next ? "已开启通知音效" : "已关闭通知音效");
    } catch {}
  }

  async function persistCommonSettings(patch: AppSettingsPatch): Promise<AppSettings | null> {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return null;
    }
    if (commonSettingsSaving) return null;
    if (rectifierAvailable !== "available") return null;
    if (!appSettings) return null;

    const prev = appSettings;
    try {
      const updated = await commonSettingsMutation.mutateAsync({
        ...patch,
        upstream_proxy_password: patch.upstream_proxy_password ?? { mode: "preserve" },
      });

      if (!updated) {
        setStreamInternalErrorGuardMs(prev.stream_internal_error_guard_ms);
        return null;
      }
      const updatedSettings = updated.settings;

      setUpstreamFirstByteTimeoutSeconds(updatedSettings.upstream_first_byte_timeout_seconds);
      setUpstreamStreamIdleTimeoutSeconds(updatedSettings.upstream_stream_idle_timeout_seconds);
      setStreamInternalErrorGuardMs(updatedSettings.stream_internal_error_guard_ms);
      setUpstreamRequestTimeoutNonStreamingSeconds(
        updatedSettings.upstream_request_timeout_non_streaming_seconds
      );
      setProviderCooldownSeconds(updatedSettings.provider_cooldown_seconds);
      setProviderFailbackStrategy(updatedSettings.provider_failback_strategy);
      setNaturalProbeMaxWaitSeconds(updatedSettings.natural_probe_max_wait_seconds);
      setProviderBaseUrlPingCacheTtlSeconds(
        updatedSettings.provider_base_url_ping_cache_ttl_seconds
      );
      setCircuitBreakerFailureThreshold(updatedSettings.circuit_breaker_failure_threshold);
      setCircuitBreakerOpenDurationMinutes(updatedSettings.circuit_breaker_open_duration_minutes);
      setUpstreamRetryPolicy(cloneUpstreamRetryPolicy(updatedSettings.upstream_retry_policy));
      setModelRoutingPolicy(cloneModelRoutingPolicy(updatedSettings.model_routing_policy));
      toast("已保存");
      return updatedSettings;
    } catch (err) {
      const formatted = formatActionFailureToast("更新通用网关参数", err);
      logToConsole("error", "更新通用网关参数失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
      });
      toast(formatted.toast);
      setUpstreamFirstByteTimeoutSeconds(prev.upstream_first_byte_timeout_seconds);
      setUpstreamStreamIdleTimeoutSeconds(prev.upstream_stream_idle_timeout_seconds);
      setStreamInternalErrorGuardMs(prev.stream_internal_error_guard_ms);
      setUpstreamRequestTimeoutNonStreamingSeconds(
        prev.upstream_request_timeout_non_streaming_seconds
      );
      setProviderCooldownSeconds(prev.provider_cooldown_seconds);
      setProviderFailbackStrategy(prev.provider_failback_strategy);
      setNaturalProbeMaxWaitSeconds(prev.natural_probe_max_wait_seconds);
      setProviderBaseUrlPingCacheTtlSeconds(prev.provider_base_url_ping_cache_ttl_seconds);
      setCircuitBreakerFailureThreshold(prev.circuit_breaker_failure_threshold);
      setCircuitBreakerOpenDurationMinutes(prev.circuit_breaker_open_duration_minutes);
      setUpstreamRetryPolicy(cloneUpstreamRetryPolicy(prev.upstream_retry_policy));
      setModelRoutingPolicy(cloneModelRoutingPolicy(prev.model_routing_policy));
      return null;
    }
  }

  async function refreshClaude() {
    await Promise.all([claudeSettingsQuery.refetch(), claudeInfoQuery.refetch()]);
  }

  async function refreshCodex() {
    if (codexConfigWriting) return;
    const [configResult, , infoResult] = await Promise.all([
      codexConfigQuery.refetch(),
      codexConfigTomlQuery.refetch(),
      codexInfoQuery.refetch(),
    ]);
    const nextConfig = configResult.data ?? null;
    const nextInfo = infoResult.data ?? null;
    if (configResult.isError || infoResult.isError || !nextConfig || nextInfo?.found !== true) {
      return;
    }
    await refreshCodexModelCatalog({
      configPath: nextConfig.config_path,
      executablePath: nextInfo.executable_path,
      cliVersion: nextInfo.version,
    });
  }

  async function refreshGeminiInfo() {
    await Promise.all([geminiInfoQuery.refetch(), geminiConfigQuery.refetch()]);
  }

  async function persistGeminiConfig(patch: GeminiConfigPatch) {
    if (geminiConfigSaving) return;
    if (geminiAvailable !== "available") return;

    try {
      const updated = await geminiConfigSetMutation.mutateAsync(patch);
      if (!updated) {
        return;
      }
      toast("已更新 Gemini 配置");
    } catch (err) {
      const formatted = formatActionFailureToast("更新 Gemini 配置", err);
      logToConsole("error", "更新 Gemini 配置失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        patch,
      });
      toast(formatted.toast);
    }
  }

  async function persistCodexHomeSettings(
    codexHomeMode: AppSettings["codex_home_mode"],
    codexHomeOverride: string
  ) {
    const updated = await persistCommonSettings({
      codex_home_mode: codexHomeMode,
      codex_home_override: codexHomeOverride,
    });
    if (!updated) {
      return false;
    }

    await refreshCodex();
    toast("Codex 目录已切换");
    return true;
  }

  async function persistCodexOauthCompatibleProxyMode(enabled: boolean) {
    const updated = await persistCommonSettings({
      codex_oauth_compatible_proxy_mode: enabled,
    });
    if (!updated) {
      return false;
    }

    toast(enabled ? "已开启 Codex OAuth 兼容代理模式" : "已关闭 Codex OAuth 兼容代理模式");
    return true;
  }

  async function persistCodexModelContextRules(
    rules: CodexModelContextRule[]
  ): Promise<CodexModelContextRulesSaveResult> {
    if (settingsWriteBlocked) {
      blockSettingsWrite();
      return { status: "blocked", settings: appSettings };
    }
    if (
      codexModelContextRulesSaving ||
      commonSettingsSaving ||
      codexConfigSetMutation.isPending ||
      codexConfigTomlSaving ||
      !appSettings
    ) {
      return { status: "blocked", settings: appSettings };
    }

    const normalizedRules = normalizeCodexModelContextRules(rules);
    let failure: unknown = null;

    try {
      const updated = await codexModelContextRulesMutation.mutateAsync(normalizedRules);
      if (
        updated &&
        codexModelContextRulesEqual(updated.codex_model_context_rules, normalizedRules)
      ) {
        toast("已应用 Codex 模型上下文规则；请新启动 Codex 会话。");
        return { status: "confirmed", settings: updated };
      }
      failure = new Error("后端返回的 canonical 规则与提交集合不一致");
    } catch (err) {
      failure = err;
    }

    const formatted = formatActionFailureToast("保存 Codex 模型上下文规则", failure);
    logToConsole("error", "保存 Codex 模型上下文规则失败", {
      error: formatted.raw,
      error_code: formatted.error_code ?? undefined,
      rule_count: normalizedRules.length,
    });

    const reread = await settingsQuery.refetch();
    if (!reread.isError && reread.data) {
      toast(`${formatted.toast}；已恢复为最新确认状态`);
      return { status: "reverted", settings: reread.data };
    }

    toast("保存 Codex 模型上下文规则失败，且设置回读失败；已进入只读保护");
    return { status: "blocked", settings: reread.data ?? appSettings };
  }

  async function pickCodexHomeDirectory(initialPath?: string): Promise<string | null> {
    try {
      return await openDesktopSinglePath({
        directory: true,
        multiple: false,
        title: "选择 Codex .codex 目录",
        defaultPath:
          initialPath ||
          codexConfig?.user_home_default_dir ||
          codexConfig?.follow_codex_home_dir ||
          codexConfig?.config_dir ||
          undefined,
      });
    } catch (err) {
      logToConsole("error", "打开 Codex 目录选择器失败", { error: String(err) });
      toast("打开目录选择器失败：请稍后重试");
      return null;
    }
  }

  async function persistCodexConfig(
    patch: CodexConfigPatch,
    options: { syncHistory?: boolean } = {}
  ): Promise<CodexConfigState | null> {
    if (codexConfigWriting) return null;
    if (!codexConfig) return null;

    try {
      const updated = await codexConfigSetMutation.mutateAsync({ patch, ...options });
      if (!updated) {
        toast("更新 Codex 配置失败：未返回更新后的配置");
        return null;
      }
      toast("已更新 Codex 配置");
      return updated;
    } catch (err) {
      const formatted = formatActionFailureToast("更新 Codex 配置", err);
      logToConsole("error", "更新 Codex 配置失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        patch,
      });
      toast(formatted.toast);
      return null;
    }
  }

  async function persistCodexConfigToml(toml: string): Promise<boolean> {
    if (codexConfigWriting) return false;
    if (!codexConfig) return false;

    try {
      const updated = await codexConfigTomlSetMutation.mutateAsync({ toml });
      if (!updated) {
        toast("保存 config.toml 失败：未返回更新后的配置");
        return false;
      }
      toast("已保存 config.toml");
      return true;
    } catch (err) {
      const formatted = formatActionFailureToast("保存 Codex TOML 配置", err);
      logToConsole("error", "保存 Codex TOML 配置失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
      });
      toast(formatted.toast);
      return false;
    }
  }

  async function syncCodexProvider() {
    if (codexConfigSaving || codexConfigTomlSaving || codexProviderSyncing) return;
    if (codexAvailable !== "available") return;

    try {
      const result = await codexProviderSyncMutation.mutateAsync();
      toast(`已同步 Codex Provider 到 ${result.target_provider}`);
    } catch (err) {
      const formatted = formatActionFailureToast("同步 Codex Provider", err);
      logToConsole("error", "同步 Codex Provider 失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
      });
      if (formatted.error_code === "CODEX_PROVIDER_SYNC_PROCESS_RUNNING") {
        toast("Codex App 正在运行，请先关闭 Codex App 后重试");
        return;
      }
      if (formatted.error_code === "CODEX_PROVIDER_SYNC_PROCESS_CHECK_FAILED") {
        toast("无法确认 Codex App 是否已完全关闭，请先手动确认已退出后重试；详情见 Console 日志");
        return;
      }
      toast(formatted.toast);
    }
  }

  async function persistClaudeSettings(patch: ClaudeSettingsPatch) {
    if (claudeSettingsSaving) return;
    if (claudeAvailable !== "available") return;

    try {
      const updated = await claudeSettingsSetMutation.mutateAsync(patch);
      if (!updated) {
        return;
      }
      toast("已更新 Claude Code 配置");
    } catch (err) {
      logToConsole("error", "更新 Claude Code settings.json 失败", { error: String(err) });
      toast("更新 Claude Code 配置失败：请稍后重试");
    }
  }

  async function openClaudeConfigDir() {
    const dir = claudeInfo?.config_dir ?? claudeSettings?.config_dir;
    if (!dir) return;
    try {
      await openDesktopPath(dir);
    } catch (err) {
      logToConsole("error", "打开 Claude 配置目录失败", { error: String(err) });
      toast("打开目录失败：请查看控制台日志");
    }
  }

  async function openCodexConfigDir() {
    if (!codexConfig) return;
    if (!codexConfig.can_open_config_dir) {
      toast("受权限限制，无法自动打开该目录");
      return;
    }
    try {
      await openDesktopPath(codexConfig.config_dir);
    } catch (err) {
      logToConsole("error", "打开 Codex 配置目录失败", { error: String(err) });
      toast("打开目录失败：请查看控制台日志");
    }
  }

  function blurOnEnter(e: ReactKeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") e.currentTarget.blur();
  }

  return {
    tab,
    setTab,
    generalTabProps: {
      rectifierAvailable,
      settingsReadErrorMessage,
      settingsWriteBlocked,
      rectifierSaving,
      rectifier,
      onPersistRectifier: persistRectifier,
      circuitBreakerNoticeEnabled,
      circuitBreakerNoticeSaving,
      onPersistCircuitBreakerNotice: persistCircuitBreakerNotice,
      codexSessionIdCompletionEnabled,
      codexSessionIdCompletionSaving,
      onPersistCodexSessionIdCompletion: persistCodexSessionIdCompletion,
      cacheAnomalyMonitorEnabled,
      cacheAnomalyMonitorSaving: commonSettingsSaving || settingsWriteBlocked,
      onPersistCacheAnomalyMonitor: persistCacheAnomalyMonitor,
      taskCompleteNotifyEnabled,
      taskCompleteNotifySaving: commonSettingsSaving || settingsWriteBlocked,
      onPersistTaskCompleteNotify: persistTaskCompleteNotify,
      notificationSoundEnabled,
      notificationSoundSaving: commonSettingsSaving || settingsWriteBlocked,
      onPersistNotificationSound: persistNotificationSound,
      appSettings,
      commonSettingsSaving: commonSettingsSaving || settingsWriteBlocked,
      onPersistCommonSettings: persistCommonSettings,
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
      modelRoutingPolicy,
      setModelRoutingPolicy,
      blurOnEnter,
    },
    claudeTabProps: {
      claudeAvailable,
      claudeLoading,
      claudeInfo,
      claudeSettingsLoading,
      claudeSettingsSaving,
      claudeSettings,
      providers: claudeProviders,
      refreshClaude,
      openClaudeConfigDir,
      persistClaudeSettings,
    },
    codexTabProps: {
      codexAvailable,
      codexLoading,
      codexConfigLoading,
      codexConfigSaving,
      codexConfigTomlLoading,
      codexConfigTomlSaving,
      codexProviderSyncing,
      codexModelCatalogLoading,
      codexModelCatalogError,
      codexModelContextCandidatesLoading,
      codexModelContextCandidatesError,
      codexInfo,
      codexConfig,
      codexConfigToml,
      codexModelCatalog,
      codexModelContextCandidates,
      appSettings,
      commonSettingsSaving,
      upstreamRetryPolicy,
      setUpstreamRetryPolicy,
      streamInternalErrorGuardMs,
      setStreamInternalErrorGuardMs,
      codexHomeSettingsSaving:
        commonSettingsSaving || codexModelContextRulesSaving || settingsWriteBlocked,
      codexModelContextRulesSaving,
      codexModelContextRulesReadOnly: settingsWriteBlocked,
      codexModelContextRulesReadErrorMessage: settingsReadErrorMessage,
      codexModelContextRulesAutoOpenRequestKey:
        searchParams.get("focus") === "model-context-rules" ? location.key : null,
      refreshCodex,
      openCodexConfigDir,
      persistCodexConfig,
      persistCodexConfigToml,
      syncCodexProvider,
      persistCommonSettings,
      persistCodexHomeSettings,
      persistCodexOauthCompatibleProxyMode,
      persistCodexModelContextRules,
      retryCodexModelContextCandidates: () => codexModelContextCandidatesQuery.refetch(),
      retrySettings: () => settingsQuery.refetch(),
      pickCodexHomeDirectory,
    },
    cx2ccTabProps: {
      appSettings,
      commonSettingsSaving,
      onPersistCommonSettings: persistCommonSettings,
    },
    geminiTabProps: {
      geminiAvailable,
      geminiLoading,
      geminiInfo,
      geminiConfigLoading,
      geminiConfigSaving,
      geminiConfig,
      refreshGeminiInfo,
      persistGeminiConfig,
    },
    grokTabProps,
  };
}
