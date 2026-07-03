import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { FREE_TAG } from "../../constants/providers";
import { logToConsole } from "../../services/consoleLog";
import {
  type ProviderOAuthStatusResult,
  type ClaudeModels,
  type ModelMapping,
  type ProviderSummary,
  type UpstreamRetryPolicy,
} from "../../services/providers/providers";
import type { GatewayStatus } from "../../services/gateway/gateway";
import type { AppSettings } from "../../services/settings/settings";
import type { ProviderEditorDialogFormInput } from "../../schemas/providerEditorDialog";
import type { BaseUrlRow, ProviderBaseUrlMode } from "./types";
import type { ProviderEditorInitialValues } from "./providerDuplicate";
import type { UseFormReset, UseFormSetValue } from "react-hook-form";
import {
  type CodexBridgeTarget,
  valueOrEmpty,
  isZeroMultiplier,
  isNonZeroMultiplier,
  moveFreeTagToFront,
  areTagsEqual,
  buildFormValues,
  buildBaseUrlRows,
  deriveCodexBridgeTarget,
  deriveAuthMode,
  deriveCx2ccSourceValue,
  withCx2ccDefaultModel,
} from "./providerEditorUtils";
import {
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
} from "../../services/gateway/upstreamRetryPolicy";

export type EffectDeps = {
  open: boolean;
  mode: "create" | "edit";
  cliKey: string;
  editProvider: ProviderSummary | null;
  editingProviderId: number | null;
  createInitialValues: ProviderEditorInitialValues | null;
  authMode: "api_key" | "oauth" | "cx2cc";
  codexBridgeTarget: CodexBridgeTarget;
  costMultiplierValue: string;
  isCodexGatewaySource: boolean;
  selectedCx2ccSourceProvider: ProviderSummary | null;
  reset: UseFormReset<ProviderEditorDialogFormInput>;
  setValue: UseFormSetValue<ProviderEditorDialogFormInput>;
  editProviderSnapshotRef: React.MutableRefObject<ProviderSummary | null>;
  baseUrlRowSeqRef: React.MutableRefObject<number>;
  oauthStatusRequestSeqRef: React.MutableRefObject<number>;
  cancelActiveOAuthLoginAttempt: (resetUi?: boolean) => void;
  newBaseUrlRow: (url?: string) => BaseUrlRow;
  setBaseUrlMode: (v: ProviderBaseUrlMode) => void;
  baseUrlRows: BaseUrlRow[];
  setBaseUrlRows: (v: BaseUrlRow[]) => void;
  setPingingAll: (v: boolean) => void;
  setClaudeModels: (v: ClaudeModels) => void;
  setModelMapping: (v: ModelMapping) => void;
  setTestModel: (v: string) => void;
  setTags: React.Dispatch<React.SetStateAction<string[]>>;
  setTagInput: (v: string) => void;
  setStreamIdleTimeoutSeconds: (v: string) => void;
  setUpstreamRetryPolicyOverrideEnabled: (v: boolean) => void;
  setUpstreamRetryPolicyDraft: (v: UpstreamRetryPolicy) => void;
  setAuthMode: (v: "api_key" | "oauth" | "cx2cc") => void;
  setCx2ccSourceValue: (v: string) => void;
  setCodexBridgeTarget: (v: CodexBridgeTarget) => void;
  setOauthStatus: (v: ProviderOAuthStatusResult | null) => void;
  setOauthLoading: (v: boolean) => void;
  setCx2ccFallbackModels: (
    v: {
      main: string;
      haiku: string;
      sonnet: string;
      opus: string;
    } | null
  ) => void;
  setCodexGatewayBaseOrigin: (v: string | null) => void;
  settingsSnapshot: AppSettings | null;
  gatewayStatusSnapshot: GatewayStatus | null;
  oauthStatusSnapshot: ProviderOAuthStatusResult | null | undefined;
  oauthStatusError: unknown;
};

export function useProviderEditorEffects(d: EffectDeps) {
  const {
    open,
    mode,
    cliKey,
    editProvider,
    editingProviderId,
    createInitialValues,
    authMode,
    costMultiplierValue,
    isCodexGatewaySource,
    selectedCx2ccSourceProvider,
    reset,
    setValue,
    editProviderSnapshotRef,
    baseUrlRowSeqRef,
    oauthStatusRequestSeqRef,
    cancelActiveOAuthLoginAttempt,
    codexBridgeTarget,
    newBaseUrlRow,
    setBaseUrlMode,
    baseUrlRows,
    setBaseUrlRows,
    setPingingAll,
    setClaudeModels,
    setModelMapping,
    setTestModel,
    setTags,
    setTagInput,
    setStreamIdleTimeoutSeconds,
    setUpstreamRetryPolicyOverrideEnabled,
    setUpstreamRetryPolicyDraft,
    setAuthMode,
    setCx2ccSourceValue,
    setCodexBridgeTarget,
    setOauthStatus,
    setOauthLoading,
    setCx2ccFallbackModels,
    setCodexGatewayBaseOrigin,
    settingsSnapshot,
    gatewayStatusSnapshot,
    oauthStatusSnapshot,
    oauthStatusError,
  } = d;
  const oauthStatusErrorRef = useRef<string | null>(null);

  useEffect(() => {
    if (mode !== "edit" || !open || !editProvider) return;
    editProviderSnapshotRef.current = editProvider;
  }, [editProvider, editProviderSnapshotRef, mode, open]);

  useEffect(() => {
    setOauthLoading(false);

    if (!open) {
      cancelActiveOAuthLoginAttempt();
      setOauthStatus(null);
      return () => {
        oauthStatusRequestSeqRef.current += 1;
        cancelActiveOAuthLoginAttempt(false);
      };
    }

    cancelActiveOAuthLoginAttempt();

    baseUrlRowSeqRef.current = 1;

    if (mode === "create") {
      const initialCx2ccSourceValue = deriveCx2ccSourceValue(createInitialValues);
      setBaseUrlMode(createInitialValues?.base_url_mode ?? "order");
      setBaseUrlRows(buildBaseUrlRows(createInitialValues, newBaseUrlRow));
      setPingingAll(false);
      setClaudeModels(
        initialCx2ccSourceValue
          ? withCx2ccDefaultModel(createInitialValues?.claude_models ?? {})
          : (createInitialValues?.claude_models ?? {})
      );
      setModelMapping(createInitialValues?.model_mapping ?? { default_model: null, exact: {} });
      setTestModel(createInitialValues?.availability_test_model ?? "");
      setTags(createInitialValues?.tags ?? []);
      setTagInput("");
      setStreamIdleTimeoutSeconds(valueOrEmpty(createInitialValues?.stream_idle_timeout_seconds));
      setUpstreamRetryPolicyOverrideEnabled(
        createInitialValues?.upstream_retry_policy_override != null
      );
      setUpstreamRetryPolicyDraft(
        cloneUpstreamRetryPolicy(
          createInitialValues?.upstream_retry_policy_override ?? DEFAULT_UPSTREAM_RETRY_POLICY
        )
      );
      setCx2ccSourceValue(initialCx2ccSourceValue);
      setCodexBridgeTarget(deriveCodexBridgeTarget(createInitialValues));
      setAuthMode(
        initialCx2ccSourceValue ? "cx2cc" : (createInitialValues?.auth_mode ?? "api_key")
      );
      setOauthStatus(null);
      reset(buildFormValues(createInitialValues));
      return () => {
        cancelActiveOAuthLoginAttempt(false);
      };
    }

    const snapshot = editProviderSnapshotRef.current;
    if (!snapshot) {
      return () => {
        cancelActiveOAuthLoginAttempt(false);
      };
    }

    const initialAuthMode = deriveAuthMode(snapshot);
    const initialCx2ccSourceValue = deriveCx2ccSourceValue(snapshot);
    setAuthMode(initialAuthMode);
    setCx2ccSourceValue(initialCx2ccSourceValue);
    setCodexBridgeTarget(deriveCodexBridgeTarget(snapshot));
    setOauthStatus(null);
    setBaseUrlMode(snapshot.base_url_mode);
    setBaseUrlRows(snapshot.base_urls.map((url) => newBaseUrlRow(url)));
    setPingingAll(false);
    setClaudeModels(
      initialAuthMode === "cx2cc"
        ? withCx2ccDefaultModel(snapshot.claude_models ?? {})
        : (snapshot.claude_models ?? {})
    );
    setModelMapping(snapshot.model_mapping ?? { default_model: null, exact: {} });
    setTestModel(snapshot.availability_test_model ?? "");
    setTags(snapshot.tags ?? []);
    setTagInput("");
    setStreamIdleTimeoutSeconds(valueOrEmpty(snapshot.stream_idle_timeout_seconds));
    setUpstreamRetryPolicyOverrideEnabled(snapshot.upstream_retry_policy_override != null);
    setUpstreamRetryPolicyDraft(
      cloneUpstreamRetryPolicy(
        snapshot.upstream_retry_policy_override ?? DEFAULT_UPSTREAM_RETRY_POLICY
      )
    );
    reset({
      name: snapshot.name,
      api_key: "",
      auth_mode: initialAuthMode === "cx2cc" ? "api_key" : initialAuthMode,
      cost_multiplier: String(snapshot.cost_multiplier ?? 1.0),
      limit_5h_usd: snapshot.limit_5h_usd != null ? String(snapshot.limit_5h_usd) : "",
      limit_daily_usd: snapshot.limit_daily_usd != null ? String(snapshot.limit_daily_usd) : "",
      limit_weekly_usd: snapshot.limit_weekly_usd != null ? String(snapshot.limit_weekly_usd) : "",
      limit_monthly_usd:
        snapshot.limit_monthly_usd != null ? String(snapshot.limit_monthly_usd) : "",
      limit_total_usd: snapshot.limit_total_usd != null ? String(snapshot.limit_total_usd) : "",
      daily_reset_mode: snapshot.daily_reset_mode ?? "fixed",
      daily_reset_time: snapshot.daily_reset_time ?? "00:00:00",
      enabled: snapshot.enabled,
      note: snapshot.note ?? "",
    });
    return () => {
      oauthStatusRequestSeqRef.current += 1;
      cancelActiveOAuthLoginAttempt(false);
    };
  }, [
    baseUrlRowSeqRef,
    cancelActiveOAuthLoginAttempt,
    cliKey,
    createInitialValues,
    editProviderSnapshotRef,
    editingProviderId,
    mode,
    newBaseUrlRow,
    oauthStatusRequestSeqRef,
    open,
    reset,
    setAuthMode,
    setBaseUrlMode,
    setBaseUrlRows,
    setCodexBridgeTarget,
    setClaudeModels,
    setModelMapping,
    setTestModel,
    setCx2ccSourceValue,
    setOauthLoading,
    setOauthStatus,
    setPingingAll,
    setStreamIdleTimeoutSeconds,
    setUpstreamRetryPolicyDraft,
    setUpstreamRetryPolicyOverrideEnabled,
    setTagInput,
    setTags,
  ]);

  useEffect(() => {
    if (!open || authMode === "oauth") return;
    cancelActiveOAuthLoginAttempt();
  }, [authMode, cancelActiveOAuthLoginAttempt, open]);

  useEffect(() => {
    if (!open || authMode !== "api_key") return;
    if (baseUrlRows.length > 0) return;
    setBaseUrlRows([newBaseUrlRow()]);
  }, [authMode, baseUrlRows, newBaseUrlRow, open, setBaseUrlRows]);

  useEffect(() => {
    if (authMode !== "cx2cc") return;
    const inheritedMultiplier = isCodexGatewaySource
      ? "0"
      : String(selectedCx2ccSourceProvider?.cost_multiplier ?? 1.0);
    if (Number(costMultiplierValue) === Number(inheritedMultiplier)) return;
    setValue("cost_multiplier", inheritedMultiplier, {
      shouldDirty: true,
      shouldTouch: false,
      shouldValidate: false,
    });
  }, [authMode, costMultiplierValue, isCodexGatewaySource, selectedCx2ccSourceProvider, setValue]);

  useEffect(() => {
    if (!open || authMode !== "cx2cc" || cliKey !== "codex") return;
    if (!selectedCx2ccSourceProvider) return;
    if (
      selectedCx2ccSourceProvider.id === editingProviderId ||
      selectedCx2ccSourceProvider.source_provider_id != null ||
      selectedCx2ccSourceProvider.bridge_type ||
      !selectedCx2ccSourceProvider.enabled
    ) {
      setCx2ccSourceValue("");
    }
  }, [
    authMode,
    cliKey,
    codexBridgeTarget,
    editingProviderId,
    open,
    selectedCx2ccSourceProvider,
    setCx2ccSourceValue,
  ]);

  useEffect(() => {
    if (!open || cliKey !== "claude") return;

    if (settingsSnapshot) {
      setCx2ccFallbackModels({
        main: settingsSnapshot.cx2cc_fallback_model_main.trim(),
        haiku: settingsSnapshot.cx2cc_fallback_model_haiku.trim(),
        sonnet: settingsSnapshot.cx2cc_fallback_model_sonnet.trim(),
        opus: settingsSnapshot.cx2cc_fallback_model_opus.trim(),
      });
      setCodexGatewayBaseOrigin(
        gatewayStatusSnapshot?.base_url?.trim() ||
          `http://127.0.0.1:${settingsSnapshot.preferred_port}`
      );
      return;
    }

    setCx2ccFallbackModels(null);
    setCodexGatewayBaseOrigin(gatewayStatusSnapshot?.base_url?.trim() || null);
  }, [
    cliKey,
    gatewayStatusSnapshot?.base_url,
    open,
    setCodexGatewayBaseOrigin,
    setCx2ccFallbackModels,
    settingsSnapshot,
  ]);

  useEffect(() => {
    if (!open) return;

    setTags((prev) => {
      const hasFreeTag = prev.includes(FREE_TAG);

      if (isZeroMultiplier(costMultiplierValue)) {
        const next = hasFreeTag ? moveFreeTagToFront(prev) : [FREE_TAG, ...prev];
        return areTagsEqual(prev, next) ? prev : next;
      }

      if (isNonZeroMultiplier(costMultiplierValue) && hasFreeTag) {
        return prev.filter((tag) => tag !== FREE_TAG);
      }

      return prev;
    });
  }, [costMultiplierValue, open, setTags]);

  useEffect(() => {
    if (!open || editProvider?.auth_mode !== "oauth") return;
    if (oauthStatusSnapshot === undefined) return;
    oauthStatusErrorRef.current = null;
    setOauthStatus(oauthStatusSnapshot);
  }, [editProvider?.auth_mode, oauthStatusSnapshot, open, setOauthStatus]);

  useEffect(() => {
    if (!open || editProvider?.auth_mode !== "oauth" || !oauthStatusError) return;
    const errorText = String(oauthStatusError);
    if (oauthStatusErrorRef.current === errorText) return;
    oauthStatusErrorRef.current = errorText;
    logToConsole("error", "加载 OAuth 状态失败", {
      provider_id: editProvider.id,
      cli_key: editProvider.cli_key,
      error: errorText,
    });
    toast(`加载 OAuth 状态失败：${errorText}`);
  }, [editProvider?.auth_mode, editProvider?.cli_key, editProvider?.id, oauthStatusError, open]);
}
