import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  cliManagerCodexConfigTomlValidate,
  type CodexConfigPatch,
  type CodexConfigState,
  type CodexModelCapability,
  type CodexModelCatalogState,
  type CodexConfigTomlState,
  type CodexConfigTomlValidationResult,
  type SimpleCliInfo,
} from "../../../services/cli/cliManager";
import type { AppSettings, CodexHomeMode } from "../../../services/settings/settings";
import { normalizeCustomCodexHome, buildConfigTomlPath } from "../../../utils/codexPaths";
import { isWindowsRuntime } from "../../../utils/platform";
import { cn } from "../../../utils/cn";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { CliVersionBadge } from "../CliVersionBadge";
import { CodexRetryGatewayManager } from "./CodexRetryGatewayManager";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Input } from "../../../ui/Input";
import { RadioGroup } from "../../../ui/RadioGroup";
import { Select } from "../../../ui/Select";
import { Switch } from "../../../ui/Switch";
import {
  resolveReasoningOptions,
  ultraConflictText,
  type ReasoningOptionView,
} from "./codexModelCapabilities";
import {
  classifyCodexApprovalReviewer,
  type CodexApprovalReviewerPresentation,
} from "./codexApprovalReviewer";
import { useCodexModelMigration } from "./useCodexModelMigration";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  ExternalLink,
  FileJson,
  FolderOpen,
  RefreshCw,
  Settings,
  Terminal,
} from "lucide-react";

const LazyCodeEditor = lazy(() =>
  import("../../../ui/CodeEditor").then((m) => ({ default: m.CodeEditor }))
);

const DEFAULT_CODEX_PROVIDER_TEST_MODEL = "gpt-5.4-mini";
const FAST_SERVICE_TIER = "fast";
const CODEX_CONFIG_LOCATION_MODE_LABEL = "目录来源";
const MODEL_REASONING_EFFORT_LABEL = "推理强度 (model_reasoning_effort)";
const MODEL_REASONING_EFFORT_DESCRIPTION =
  "调整推理强度（仅对支持的模型/Responses API 生效）。值越高通常越稳健但更慢。";
const PLAN_MODE_REASONING_EFFORT_LABEL = "计划模式推理强度 (plan_mode_reasoning_effort)";
const WEB_SEARCH_MODE_LABEL = "网络搜索模式 (web_search)";
const PERSONALITY_LABEL = "输出风格 (personality)";

function parsePositiveInt(v: string | undefined): number | null {
  if (v == null) return null;
  const n = Number(v.trim());
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.trunc(n);
}

function boolOrDefault(value: boolean | null, fallback: boolean) {
  return value ?? fallback;
}

function enumOrDefault(value: string | null, fallback: string) {
  return (value ?? fallback).trim();
}

function buildFastModePatch(enabled: boolean): CodexConfigPatch {
  return {
    features_fast_mode: enabled,
    service_tier: enabled ? FAST_SERVICE_TIER : "",
  };
}

function buildPersonalityPatch(value: string): CodexConfigPatch {
  return {
    personality: value === "none" ? "" : value,
  };
}

function validateCustomCodexHome(value: string): string | null {
  const trimmed = value.trim();
  const normalized = normalizeCustomCodexHome(trimmed);
  if (!normalized) return "请输入 .codex 目录路径。";

  const lower = trimmed.replace(/\\/g, "/").toLowerCase();
  if (lower.includes("://")) {
    return "这里填写的是本地目录路径，不要包含协议头。";
  }
  if (/[\r\n\u0000]/u.test(trimmed)) {
    return "路径中不能包含换行或控制字符。";
  }
  if (lower.endsWith(".toml") && lower !== "config.toml" && !lower.endsWith("/config.toml")) {
    return "这里填写的是 .codex 目录，不是其他 TOML 文件。";
  }

  return null;
}

function normalizeComparablePath(path: string) {
  return path
    .trim()
    .replace(/[\\/]+$/u, "")
    .replace(/\\/g, "/")
    .toLowerCase();
}

type CodexConfigDraft = {
  configKey: string;
  modelText: string;
  contextWindowText: string;
  autoCompactLimitText: string;
  sandboxModeText: string;
  webSearchText: string;
  personalityText: string;
  reasoningEffortText: string;
  planModeReasoningEffortText: string;
};

function buildCodexConfigKey(codexConfig: CodexConfigState | null) {
  if (!codexConfig) return "none";
  return [
    codexConfig.model ?? "",
    codexConfig.model_context_window ?? "",
    codexConfig.model_auto_compact_token_limit ?? "",
    codexConfig.sandbox_mode ?? "",
    codexConfig.web_search ?? "",
    codexConfig.personality ?? "",
    codexConfig.model_reasoning_effort ?? "",
    codexConfig.plan_mode_reasoning_effort ?? "",
  ].join("\u0000");
}

function buildCodexConfigDraft(codexConfig: CodexConfigState | null): CodexConfigDraft {
  return {
    configKey: buildCodexConfigKey(codexConfig),
    modelText: codexConfig?.model ?? "",
    contextWindowText:
      codexConfig?.model_context_window != null ? String(codexConfig.model_context_window) : "",
    autoCompactLimitText:
      codexConfig?.model_auto_compact_token_limit != null
        ? String(codexConfig.model_auto_compact_token_limit)
        : "",
    sandboxModeText: codexConfig?.sandbox_mode ?? "",
    webSearchText: codexConfig?.web_search ?? "cached",
    personalityText: codexConfig?.personality?.trim() || "none",
    reasoningEffortText: codexConfig?.model_reasoning_effort ?? "",
    planModeReasoningEffortText: codexConfig?.plan_mode_reasoning_effort ?? "",
  };
}

type ConfigLocationDraft = {
  settingsKey: string;
  configLocationMode: CodexHomeMode;
  customHomeText: string;
  configLocationError: string | null;
};

function readConfigLocationSettings(appSettings: AppSettings | null | undefined) {
  const savedOverride = appSettings?.codex_home_override?.trim() ?? "";
  const savedMode =
    appSettings?.codex_home_mode ?? (savedOverride ? "custom" : "user_home_default");
  return { savedMode, savedOverride };
}

function buildConfigLocationKey(appSettings: AppSettings | null | undefined) {
  const { savedMode, savedOverride } = readConfigLocationSettings(appSettings);
  return [savedMode, savedOverride].join("\u0000");
}

function buildConfigLocationDraft(
  appSettings: AppSettings | null | undefined
): ConfigLocationDraft {
  const { savedMode, savedOverride } = readConfigLocationSettings(appSettings);
  return {
    settingsKey: buildConfigLocationKey(appSettings),
    configLocationMode: savedMode,
    customHomeText: savedOverride,
    configLocationError: null,
  };
}

type TomlDraftState = {
  sourceKey: string;
  configPath: string | null;
  tomlDraft: string;
  tomlDirty: boolean;
  tomlValidating: boolean;
  tomlValidation: CodexConfigTomlValidationResult | null;
  tomlEditEnabled: boolean;
};

function buildTomlSourceKey(codexConfigToml: CodexConfigTomlState | null) {
  if (!codexConfigToml) return "none";
  return [codexConfigToml.config_path ?? "", codexConfigToml.toml ?? ""].join("\u0000");
}

function buildTomlDraftState(codexConfigToml: CodexConfigTomlState | null): TomlDraftState {
  return {
    sourceKey: buildTomlSourceKey(codexConfigToml),
    configPath: codexConfigToml?.config_path ?? null,
    tomlDraft: codexConfigToml?.toml ?? "",
    tomlDirty: false,
    tomlValidating: false,
    tomlValidation: null,
    tomlEditEnabled: false,
  };
}

type CodexTabUiState = {
  versionRefreshToken: number;
  codexDraft: CodexConfigDraft;
  configLocationDraft: ConfigLocationDraft;
  selectingCodexHomeDir: boolean;
  tomlAdvancedOpen: boolean;
  tomlState: TomlDraftState;
};

type CodexTabUiAction =
  | { type: "incrementVersionRefreshToken" }
  | { type: "setCodexDraft"; draft: CodexConfigDraft }
  | { type: "patchCodexDraft"; patch: Partial<Omit<CodexConfigDraft, "configKey">> }
  | { type: "setConfigLocationDraft"; draft: ConfigLocationDraft }
  | { type: "patchConfigLocationDraft"; patch: Partial<Omit<ConfigLocationDraft, "settingsKey">> }
  | { type: "setSelectingCodexHomeDir"; value: boolean }
  | { type: "setTomlAdvancedOpen"; value: boolean }
  | { type: "setTomlState"; state: TomlDraftState }
  | { type: "patchTomlState"; patch: Partial<Omit<TomlDraftState, "sourceKey" | "configPath">> };

function initCodexTabUiState({
  codexConfig,
  codexConfigToml,
  appSettings,
}: {
  codexConfig: CodexConfigState | null;
  codexConfigToml: CodexConfigTomlState | null;
  appSettings: AppSettings | null | undefined;
}): CodexTabUiState {
  return {
    versionRefreshToken: 0,
    codexDraft: buildCodexConfigDraft(codexConfig),
    configLocationDraft: buildConfigLocationDraft(appSettings),
    selectingCodexHomeDir: false,
    tomlAdvancedOpen: false,
    tomlState: buildTomlDraftState(codexConfigToml),
  };
}

function codexTabUiReducer(state: CodexTabUiState, action: CodexTabUiAction): CodexTabUiState {
  switch (action.type) {
    case "incrementVersionRefreshToken":
      return { ...state, versionRefreshToken: state.versionRefreshToken + 1 };
    case "setCodexDraft":
      return { ...state, codexDraft: action.draft };
    case "patchCodexDraft":
      return { ...state, codexDraft: { ...state.codexDraft, ...action.patch } };
    case "setConfigLocationDraft":
      return { ...state, configLocationDraft: action.draft };
    case "patchConfigLocationDraft":
      return {
        ...state,
        configLocationDraft: { ...state.configLocationDraft, ...action.patch },
      };
    case "setSelectingCodexHomeDir":
      return { ...state, selectingCodexHomeDir: action.value };
    case "setTomlAdvancedOpen":
      return { ...state, tomlAdvancedOpen: action.value };
    case "setTomlState":
      return { ...state, tomlState: action.state };
    case "patchTomlState":
      return { ...state, tomlState: { ...state.tomlState, ...action.patch } };
  }
}

export type CliManagerAvailability = "checking" | "available" | "unavailable";

export type CliManagerCodexTabProps = {
  codexAvailable: CliManagerAvailability;
  codexLoading: boolean;
  codexConfigLoading: boolean;
  codexConfigSaving: boolean;
  codexConfigTomlLoading: boolean;
  codexConfigTomlSaving: boolean;
  codexProviderSyncing?: boolean;
  codexModelCatalogLoading?: boolean;
  codexModelCatalogError?: boolean;
  codexInfo: SimpleCliInfo | null;
  codexConfig: CodexConfigState | null;
  codexConfigToml: CodexConfigTomlState | null;
  codexModelCatalog?: CodexModelCatalogState | null;
  appSettings?: AppSettings | null;
  commonSettingsSaving?: boolean;
  codexHomeSettingsSaving?: boolean;
  refreshCodex: () => Promise<void> | void;
  openCodexConfigDir: () => Promise<void> | void;
  persistCodexConfig: (patch: CodexConfigPatch) => Promise<CodexConfigState | null>;
  persistCodexConfigToml: (toml: string) => Promise<boolean> | boolean;
  syncCodexProvider?: () => Promise<void> | void;
  persistCommonSettings?: (
    patch: Partial<AppSettings>
  ) => Promise<AppSettings | null> | AppSettings | null;
  persistCodexReasoningGuardSettings?: unknown;
  persistCodexHomeSettings?: (
    codexHomeMode: CodexHomeMode,
    codexHomeOverride: string
  ) => Promise<boolean> | boolean;
  persistCodexOauthCompatibleProxyMode?: (enabled: boolean) => Promise<boolean> | boolean;
  pickCodexHomeDirectory?: (initialPath?: string) => Promise<string | null> | string | null;
  onOpenGatewayDetailsRoute?: () => void;
  showRetryGatewayManager?: boolean;
};

type PersistConfigLocationResult = "saved" | "validation_failed" | "persist_failed";
type UpdateCodexDraft = (patch: Partial<Omit<CodexConfigDraft, "configKey">>) => void;
type UpdateConfigLocationDraft = (patch: Partial<Omit<ConfigLocationDraft, "settingsKey">>) => void;
type UpdateTomlState = (patch: Partial<Omit<TomlDraftState, "sourceKey" | "configPath">>) => void;

function SettingItem({
  label,
  subtitle,
  children,
  className,
}: {
  label: string;
  subtitle: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-2 py-3 sm:flex-row sm:items-start sm:justify-between",
        className
      )}
    >
      <div className="min-w-0">
        <div className="text-sm text-secondary-foreground">{label}</div>
        <div className="mt-1 text-xs leading-relaxed text-muted-foreground">{subtitle}</div>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">{children}</div>
    </div>
  );
}

function CodexHeader({
  codexAvailable,
  codexInfo,
  loading,
  saving,
  versionRefreshToken,
  refreshCodexStatus,
}: {
  codexAvailable: CliManagerAvailability;
  codexInfo: SimpleCliInfo | null;
  loading: boolean;
  saving: boolean;
  versionRefreshToken: number;
  refreshCodexStatus: () => Promise<void>;
}) {
  return (
    <div className="flex flex-col items-start justify-between gap-4 md:flex-row md:items-center">
      <div className="flex items-center gap-4">
        <div className="flex h-14 w-14 items-center justify-center rounded-xl bg-card/5 text-secondary-foreground dark:bg-secondary">
          <Terminal className="h-8 w-8" />
        </div>
        <div>
          <h2 className="text-base font-semibold text-foreground">Codex</h2>
          <div className="mt-1 flex items-center gap-2">
            {codexAvailable === "available" && codexInfo?.found ? (
              <>
                <span className="inline-flex items-center gap-1.5 rounded-full bg-green-50 px-2.5 py-0.5 text-xs font-medium text-green-700 ring-1 ring-inset ring-green-600/20 dark:bg-green-900/30 dark:text-green-400">
                  <CheckCircle2 className="h-3 w-3" />
                  已安装 {codexInfo.version}
                </span>
                <CliVersionBadge
                  cliKey="codex"
                  installedVersion={codexInfo.version}
                  refreshToken={versionRefreshToken}
                  onUpdateComplete={refreshCodexStatus}
                />
              </>
            ) : codexAvailable === "checking" || loading ? (
              <span className="inline-flex items-center gap-1.5 rounded-full bg-blue-50 px-2.5 py-0.5 text-xs font-medium text-blue-700 ring-1 ring-inset ring-blue-600/20 dark:bg-blue-900/30 dark:text-blue-400">
                <RefreshCw className="h-3 w-3 animate-spin" />
                加载中...
              </span>
            ) : (
              <span className="inline-flex items-center gap-1.5 rounded-full bg-secondary px-2.5 py-0.5 text-xs font-medium text-muted-foreground ring-1 ring-inset ring-border">
                未检测到
              </span>
            )}
          </div>
        </div>
      </div>

      <Button
        onClick={() => void refreshCodexStatus()}
        variant="secondary"
        size="sm"
        disabled={loading || saving}
        className="gap-2"
      >
        <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
        刷新
      </Button>
    </div>
  );
}

function CodexInfoGrid({
  codexConfig,
  codexInfo,
  activeConfigDirSummaryText,
  openCodexConfigDir,
}: {
  codexConfig: CodexConfigState;
  codexInfo: SimpleCliInfo | null;
  activeConfigDirSummaryText: string;
  openCodexConfigDir: () => Promise<void> | void;
}) {
  return (
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-4">
      <div className="rounded-lg border border-border bg-secondary p-3">
        <div className="mb-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
          <FolderOpen className="h-3 w-3" />
          当前 .codex 目录
        </div>
        <div className="flex items-center gap-1.5">
          <div
            className="flex-1 truncate font-mono text-xs text-secondary-foreground"
            title={codexConfig.config_dir}
          >
            {codexConfig.config_dir}
          </div>
          <Button
            onClick={() => void openCodexConfigDir()}
            disabled={!codexConfig.can_open_config_dir}
            size="sm"
            variant="ghost"
            className="h-6 w-6 shrink-0 p-0"
            title={
              codexConfig.can_open_config_dir
                ? "打开当前生效目录"
                : "受权限限制，无法自动打开该目录"
            }
          >
            <ExternalLink className="h-3 w-3" />
          </Button>
        </div>
        <div className="mt-1 text-[11px] text-muted-foreground">{activeConfigDirSummaryText}</div>
      </div>

      <div className="rounded-lg border border-border bg-secondary p-3">
        <div className="mb-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
          <FileJson className="h-3 w-3" />
          config.toml
        </div>
        <div
          className="truncate font-mono text-xs text-secondary-foreground"
          title={codexConfig.config_path}
        >
          {codexConfig.config_path}
        </div>
        <div className="mt-1 text-[11px] text-muted-foreground">
          {codexConfig.exists ? "已存在" : "不存在（将自动创建）"}
        </div>
      </div>

      <div className="rounded-lg border border-border bg-secondary p-3">
        <div className="mb-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
          <Terminal className="h-3 w-3" />
          可执行文件
        </div>
        <div
          className="truncate font-mono text-xs text-secondary-foreground"
          title={codexInfo?.executable_path ?? "—"}
        >
          {codexInfo?.executable_path ?? "—"}
        </div>
      </div>

      <div className="rounded-lg border border-border bg-secondary p-3">
        <div className="mb-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
          <Settings className="h-3 w-3" />
          解析方式
        </div>
        <div
          className="truncate font-mono text-xs text-secondary-foreground"
          title={codexInfo?.resolved_via ?? "—"}
        >
          {codexInfo?.resolved_via ?? "—"}
        </div>
        <div className="mt-1 text-[11px] text-muted-foreground">
          SHELL: {codexInfo?.shell ?? "—"}
        </div>
      </div>
    </div>
  );
}

function CodexConfigLocationSection({
  codexConfig,
  customHomeInputId,
  configLocationMode,
  customHomeText,
  configLocationError,
  selectingCodexHomeDir,
  configLocationControlsDisabled,
  activeConfigModeBadgeText,
  activeConfigDirPrimaryText,
  activeConfigDirSummaryText,
  configLocationSummaryText,
  followModeLabel,
  followModeMatchesDefault,
  userDefaultResolvedHomeDir,
  followCodexHomeResolvedDir,
  configLocationPreviewPath,
  resetConfigLocation,
  handleConfigLocationModeChange,
  updateConfigLocationDraft,
  persistConfigLocation,
  restoreSavedConfigLocationState,
  handlePickCustomHome,
}: {
  codexConfig: CodexConfigState;
  customHomeInputId: string;
  configLocationMode: CodexHomeMode;
  customHomeText: string;
  configLocationError: string | null;
  selectingCodexHomeDir: boolean;
  configLocationControlsDisabled: boolean;
  activeConfigModeBadgeText: string;
  activeConfigDirPrimaryText: string;
  activeConfigDirSummaryText: string;
  configLocationSummaryText: string;
  followModeLabel: string;
  followModeMatchesDefault: boolean;
  userDefaultResolvedHomeDir: string;
  followCodexHomeResolvedDir: string;
  configLocationPreviewPath: string;
  resetConfigLocation: () => Promise<void>;
  handleConfigLocationModeChange: (nextMode: CodexHomeMode) => Promise<void>;
  updateConfigLocationDraft: UpdateConfigLocationDraft;
  persistConfigLocation: (
    nextMode: CodexHomeMode,
    nextCustomHome?: string
  ) => Promise<PersistConfigLocationResult>;
  restoreSavedConfigLocationState: () => void;
  handlePickCustomHome: () => Promise<void>;
}) {
  return (
    <div className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20">
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
          <div className="min-w-0">
            <div className="text-sm font-semibold text-foreground">Windows 本机配置</div>
            <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
              仅影响 Windows 本机上的 Codex 用户级 <span className="font-mono">.codex</span>{" "}
              目录，不改写 WSL 内各 distro 的目标路径。
            </div>
          </div>

          <div className="flex items-center gap-2">
            <span className="inline-flex items-center rounded-full border border-border bg-secondary px-2.5 py-1 text-[11px] font-medium text-secondary-foreground">
              {activeConfigModeBadgeText}
            </span>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => void resetConfigLocation()}
              disabled={
                configLocationControlsDisabled ||
                (configLocationMode === "user_home_default" && customHomeText.trim().length === 0)
              }
            >
              恢复默认
            </Button>
          </div>
        </div>

        <div className="rounded-lg border border-border/70 bg-secondary/80 p-3">
          <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
            <div className="min-w-0">
              <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                当前会使用
              </div>
              <div
                className="mt-1 break-all font-mono text-xs text-secondary-foreground"
                title={activeConfigDirPrimaryText}
              >
                {activeConfigDirPrimaryText}
              </div>
              <div className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                {configLocationSummaryText}
              </div>
            </div>

            <div className="min-w-0 md:max-w-[320px]">
              <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
                config.toml
              </div>
              <div
                className="mt-1 break-all font-mono text-xs text-secondary-foreground"
                title={codexConfig.config_path}
              >
                {codexConfig.config_path}
              </div>
              <div className="mt-1 text-[11px] text-muted-foreground">
                {activeConfigDirSummaryText}
              </div>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-border/70 bg-white/70 p-3 dark:bg-card/20">
          <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
            {CODEX_CONFIG_LOCATION_MODE_LABEL}
          </div>
          <div className="mt-2">
            <RadioGroup
              name="codex_config_location_mode"
              ariaLabel={CODEX_CONFIG_LOCATION_MODE_LABEL}
              value={configLocationMode}
              onChange={(value) =>
                void handleConfigLocationModeChange(
                  value === "follow_codex_home"
                    ? "follow_codex_home"
                    : value === "custom"
                      ? "custom"
                      : "user_home_default"
                )
              }
              options={[
                { value: "user_home_default", label: "固定到 Windows 用户目录" },
                { value: "follow_codex_home", label: followModeLabel },
                { value: "custom", label: "手动指定目录" },
              ]}
              disabled={configLocationControlsDisabled}
            />
          </div>
          <div className="mt-2 space-y-1 text-[11px] leading-relaxed text-muted-foreground">
            <div>
              固定目录：<span className="ml-1 font-mono">{userDefaultResolvedHomeDir}</span>
            </div>
            <div>
              <span className="font-mono">$CODEX_HOME</span> 当前解析：
              <span className="ml-1 font-mono">{followCodexHomeResolvedDir}</span>
              {followModeMatchesDefault ? (
                <span className="ml-2 text-amber-700 dark:text-amber-400">
                  当前路径相同，但后续会随 $CODEX_HOME 变化。
                </span>
              ) : null}
            </div>
          </div>
        </div>

        {configLocationMode === "custom" ? (
          <div className="rounded-lg border border-border/70 bg-secondary/80 p-3">
            <label
              htmlFor={customHomeInputId}
              className="text-xs font-medium text-secondary-foreground"
            >
              自定义 .codex 目录
            </label>

            <div className="mt-3 flex flex-col gap-2 lg:flex-row">
              <Input
                id={customHomeInputId}
                value={customHomeText}
                onChange={(e) => {
                  const next = e.currentTarget.value;
                  updateConfigLocationDraft({
                    customHomeText: next,
                    configLocationError: configLocationError
                      ? validateCustomCodexHome(next)
                      : configLocationError,
                  });
                }}
                onBlur={() => {
                  if (configLocationMode !== "custom") return;
                  void persistConfigLocation("custom", customHomeText).then((result) => {
                    if (result === "persist_failed") {
                      restoreSavedConfigLocationState();
                    }
                  });
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") e.currentTarget.blur();
                }}
                placeholder="例如：D:\\Users\\you\\.codex"
                className={cn(
                  "font-mono text-xs lg:flex-1",
                  configLocationError &&
                    "border-rose-300 focus-visible:ring-rose-200 dark:border-rose-700"
                )}
                disabled={configLocationControlsDisabled}
              />

              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  onClick={() => void handlePickCustomHome()}
                  disabled={configLocationControlsDisabled}
                >
                  <FolderOpen className="mr-1.5 h-3.5 w-3.5" />
                  {selectingCodexHomeDir ? "选择中..." : "选择目录"}
                </Button>
              </div>
            </div>

            <div
              className={cn(
                "mt-2 text-[11px] leading-relaxed",
                configLocationError ? "text-rose-600 dark:text-rose-400" : "text-muted-foreground"
              )}
            >
              {configLocationError
                ? configLocationError
                : configLocationPreviewPath
                  ? `保存后将使用 ${configLocationPreviewPath}。支持普通 Windows 路径、UNC 路径，也可以点“选择目录”。`
                  : "请输入一个 .codex 目录路径，然后按 Enter、移出输入框，或直接使用目录选择器保存。"}
            </div>
          </div>
        ) : (
          <div className="rounded-lg border border-dashed border-border/80 bg-secondary/50 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
            {configLocationMode === "follow_codex_home"
              ? `当前为跟随模式，手动目录选择器已收起；现在会使用 ${followCodexHomeResolvedDir}。`
              : `当前为默认模式，手动目录选择器已收起；固定使用 ${userDefaultResolvedHomeDir}。`}
          </div>
        )}
      </div>
    </div>
  );
}

function CodexOauthProxySection({
  appSettings,
  proxyModeControlsDisabled,
  persistCodexOauthCompatibleProxyMode,
}: {
  appSettings: AppSettings;
  proxyModeControlsDisabled: boolean;
  persistCodexOauthCompatibleProxyMode?: (enabled: boolean) => Promise<boolean> | boolean;
}) {
  return (
    <div className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20">
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-foreground">OAuth 兼容代理模式</div>
          <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
            开启后，AIO 接管 Codex 代理时只写入 <span className="font-mono">config.toml</span> 的
            AIO provider，不创建、不备份、不恢复 <span className="font-mono">auth.json</span>。
          </div>
          <div className="mt-2 text-[11px] leading-relaxed text-amber-700 dark:text-amber-400">
            该模式不会写入 <span className="font-mono">preferred_auth_method = "chatgpt"</span>
            ，会保留 <span className="font-mono">requires_openai_auth = true</span>。
          </div>
        </div>
        <Switch
          aria-label="切换 Codex OAuth 兼容代理模式"
          checked={appSettings.codex_oauth_compatible_proxy_mode}
          onCheckedChange={(checked) => void persistCodexOauthCompatibleProxyMode?.(checked)}
          disabled={proxyModeControlsDisabled}
        />
      </div>
    </div>
  );
}

function CodexApprovalReviewerNotice({
  presentation,
  approvalPolicy,
  saving,
  persistCodexConfig,
}: {
  presentation: CodexApprovalReviewerPresentation;
  approvalPolicy: string | null;
  saving: boolean;
  persistCodexConfig: CliManagerCodexTabProps["persistCodexConfig"];
}) {
  if (presentation.notice === "none") return null;

  if (presentation.notice === "auto-review-inherited-policy") {
    return (
      <div className="text-left text-[11px] leading-relaxed text-muted-foreground">
        当前 approval_policy 未显式设置，是否会产生可供 auto-review 处理的审批请求取决于上层配置。
      </div>
    );
  }

  let message: ReactNode;
  if (presentation.notice === "unknown-reviewer") {
    message = (
      <>
        当前值
        <span className="mx-1 break-all font-mono">{presentation.unknownReviewer}</span>
        不受支持；在你明确选择前会原样保留。
      </>
    );
  } else if (presentation.notice === "user-never") {
    message = "当前 approval_policy=never 不会产生需要你处理的审批请求。";
  } else if (approvalPolicy === "never") {
    message = "当前 approval_policy=never 不会产生审批请求，替我审批不会生效。";
  } else {
    message = (
      <>
        当前 approval_policy=<span className="break-all font-mono">{approvalPolicy}</span>
        不支持 auto-review，替我审批不会生效。
      </>
    );
  }

  return (
    <div className="flex items-start gap-2 text-left text-[11px] leading-relaxed text-amber-700 dark:text-amber-400">
      <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div>{message}</div>
        {presentation.canSwitchToOnRequest ? (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="mt-1 h-7 gap-1 px-2 text-[11px]"
            onClick={() => void persistCodexConfig({ approval_policy: "on-request" })}
            disabled={saving}
          >
            <ArrowRight className="h-3.5 w-3.5" aria-hidden="true" />
            切换为 on-request
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function CodexBasicConfigSection({
  codexConfig,
  saving,
  modelText,
  modelSuggestions,
  contextWindowText,
  autoCompactLimitText,
  sandboxModeText,
  reasoningEffortText,
  planModeReasoningEffortText,
  webSearchText,
  personalityText,
  reasoningOptions,
  reasoningStatusText,
  reasoningStatusRetryable,
  ultraConflictText: ultraConflictWarning,
  onModelInputChange,
  onEffortInputChange,
  persistModel,
  refreshCodexStatus,
  updateCodexDraft,
  persistCodexConfig,
}: {
  codexConfig: CodexConfigState;
  saving: boolean;
  modelText: string;
  modelSuggestions: CodexModelCapability[];
  contextWindowText: string;
  autoCompactLimitText: string;
  sandboxModeText: string;
  reasoningEffortText: string;
  planModeReasoningEffortText: string;
  webSearchText: string;
  personalityText: string;
  reasoningOptions: ReasoningOptionView[];
  reasoningStatusText: string | null;
  reasoningStatusRetryable: boolean;
  ultraConflictText: string | null;
  onModelInputChange: () => void;
  onEffortInputChange: () => void;
  persistModel: (modelText: string, currentEffort: string) => Promise<CodexConfigState | null>;
  refreshCodexStatus: () => Promise<void>;
  updateCodexDraft: UpdateCodexDraft;
  persistCodexConfig: CliManagerCodexTabProps["persistCodexConfig"];
}) {
  const approvalReviewerPresentation = classifyCodexApprovalReviewer(
    codexConfig.approvals_reviewer,
    codexConfig.approval_policy
  );

  return (
    <div className="rounded-lg border border-border bg-white p-5 dark:bg-secondary">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
        <Settings className="h-4 w-4 text-muted-foreground" />
        基础配置
      </h3>
      <div className="divide-y divide-border">
        <SettingItem
          label="默认模型 (model)"
          subtitle="设置 Codex 默认使用的模型。留空表示不设置，交由 Codex 默认或上层配置决定。"
        >
          <Input
            value={modelText}
            onChange={(e) => {
              onModelInputChange();
              updateCodexDraft({ modelText: e.currentTarget.value });
            }}
            onBlur={() => {
              updateCodexDraft({ modelText: modelText.trim() });
              void persistModel(modelText, reasoningEffortText);
            }}
            placeholder="例如：gpt-5-codex"
            list="codex-model-suggestions"
            aria-label="默认模型 (model)"
            className="w-[280px] max-w-full font-mono"
            disabled={saving}
          />
          <datalist id="codex-model-suggestions">
            {modelSuggestions.map((suggestion) => (
              <option
                key={`${suggestion.id}:${suggestion.model}`}
                value={suggestion.model}
                label={suggestion.display_name}
              />
            ))}
          </datalist>
        </SettingItem>

        <SettingItem
          label="model_context_window"
          subtitle="模型上下文窗口覆盖值。留空表示删除覆盖，使用 Codex 或上层默认行为。"
        >
          <Input
            type="number"
            value={contextWindowText}
            onChange={(e) => updateCodexDraft({ contextWindowText: e.currentTarget.value })}
            onBlur={() =>
              void persistCodexConfig({
                model_context_window: parsePositiveInt(contextWindowText),
              })
            }
            placeholder="例如：1000000"
            className="w-[220px] max-w-full font-mono"
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label="model_auto_compact_token_limit"
          subtitle="自动压缩 token 上限覆盖值。留空表示删除覆盖，使用 Codex 或上层默认行为。"
        >
          <Input
            type="number"
            value={autoCompactLimitText}
            onChange={(e) => updateCodexDraft({ autoCompactLimitText: e.currentTarget.value })}
            onBlur={() =>
              void persistCodexConfig({
                model_auto_compact_token_limit: parsePositiveInt(autoCompactLimitText),
              })
            }
            placeholder="例如：900000"
            className="w-[220px] max-w-full font-mono"
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label="审批策略 (approval_policy)"
          subtitle="控制何时需要你确认才会执行命令。推荐 on-request 或 on-failure。"
        >
          <Select
            value={codexConfig.approval_policy ?? ""}
            onChange={(e) => void persistCodexConfig({ approval_policy: e.currentTarget.value })}
            disabled={saving}
            className="w-[220px] max-w-full font-mono"
          >
            <option value="">默认（不设置）</option>
            <option value="untrusted">不信任（untrusted）</option>
            <option value="on-failure">失败时（on-failure）</option>
            <option value="on-request">请求时（on-request）</option>
            <option value="never">从不询问（never）</option>
          </Select>
        </SettingItem>

        <SettingItem
          label="审批者 (approvals_reviewer)"
          subtitle="决定符合条件的审批请求由你处理，还是交给独立 reviewer 评估；不会扩大沙箱权限。"
        >
          <div className="w-[360px] max-w-full space-y-2">
            <Select
              aria-label="审批者 (approvals_reviewer)"
              value={codexConfig.approvals_reviewer ?? ""}
              onChange={(e) =>
                void persistCodexConfig({ approvals_reviewer: e.currentTarget.value })
              }
              disabled={saving}
              className="w-full font-mono"
            >
              <option value="">默认（不设置）</option>
              <option value="user">由我审批（user）</option>
              <option value="auto_review">替我审批（auto_review）</option>
              {approvalReviewerPresentation.unknownReviewer !== null ? (
                <option value={approvalReviewerPresentation.unknownReviewer}>
                  不支持的当前值（{approvalReviewerPresentation.unknownReviewer}）
                </option>
              ) : null}
            </Select>
            <CodexApprovalReviewerNotice
              presentation={approvalReviewerPresentation}
              approvalPolicy={codexConfig.approval_policy}
              saving={saving}
              persistCodexConfig={persistCodexConfig}
            />
          </div>
        </SettingItem>

        <SettingItem
          label="沙箱模式 (sandbox_mode)"
          subtitle="控制文件和网络访问策略。danger-full-access 风险极高，仅在完全信任的环境使用。"
        >
          <Select
            value={sandboxModeText}
            onChange={(e) => {
              const next = e.currentTarget.value;
              void (async () => {
                if (next === "danger-full-access") {
                  const ok = await confirmDesktopDialog(
                    "你选择了 danger-full-access（危险：完全访问）。确认要继续吗？"
                  );
                  if (!ok) {
                    updateCodexDraft({ sandboxModeText: codexConfig.sandbox_mode ?? "" });
                    return;
                  }
                }
                updateCodexDraft({ sandboxModeText: next });
                await persistCodexConfig({ sandbox_mode: next });
              })();
            }}
            disabled={saving}
            className="w-[220px] max-w-full font-mono"
          >
            <option value="">默认（不设置）</option>
            <option value="read-only">只读（read-only）</option>
            <option value="workspace-write">工作区写入（workspace-write）</option>
            <option value="danger-full-access">危险：完全访问（danger-full-access）</option>
          </Select>
        </SettingItem>

        <SettingItem
          label={MODEL_REASONING_EFFORT_LABEL}
          subtitle={MODEL_REASONING_EFFORT_DESCRIPTION}
        >
          <RadioGroup
            name="model_reasoning_effort"
            ariaLabel={MODEL_REASONING_EFFORT_LABEL}
            ariaDescription={MODEL_REASONING_EFFORT_DESCRIPTION}
            value={reasoningEffortText}
            onChange={(value) => {
              onEffortInputChange();
              updateCodexDraft({ reasoningEffortText: value });
              void persistCodexConfig({ model_reasoning_effort: value });
            }}
            options={reasoningOptions.map((option) => ({
              value: option.reasoning_effort,
              label: option.label,
              description: option.description,
            }))}
            disabled={saving}
          />
          {reasoningStatusText || reasoningStatusRetryable ? (
            <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] leading-relaxed text-muted-foreground">
              {reasoningStatusText ? <span>{reasoningStatusText}</span> : null}
              {reasoningStatusRetryable ? (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="h-7 gap-1 px-2 text-[11px]"
                  onClick={() => void refreshCodexStatus()}
                  disabled={saving}
                >
                  <RefreshCw className="h-3 w-3" />
                  重试能力目录
                </Button>
              ) : null}
            </div>
          ) : null}
          {ultraConflictWarning ? (
            <div className="mt-2 flex items-start gap-2 text-[11px] leading-relaxed text-amber-700 dark:text-amber-400">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>{ultraConflictWarning}</span>
            </div>
          ) : null}
        </SettingItem>

        <SettingItem
          label={PLAN_MODE_REASONING_EFFORT_LABEL}
          subtitle="调整计划模式下的推理强度。值越高通常规划越充分但更慢。"
        >
          <RadioGroup
            name="plan_mode_reasoning_effort"
            ariaLabel={PLAN_MODE_REASONING_EFFORT_LABEL}
            value={planModeReasoningEffortText}
            onChange={(value) => {
              updateCodexDraft({ planModeReasoningEffortText: value });
              void persistCodexConfig({ plan_mode_reasoning_effort: value });
            }}
            options={[
              { value: "", label: "默认" },
              { value: "low", label: "低 (low)" },
              { value: "medium", label: "中 (medium)" },
              { value: "high", label: "高 (high)" },
              { value: "xhigh", label: "极高 (xhigh)" },
            ]}
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label={WEB_SEARCH_MODE_LABEL}
          subtitle="控制 Web Search 工具的行为。cached 使用缓存结果，live 获取最新数据，disabled 禁用。"
        >
          <RadioGroup
            name="web_search"
            ariaLabel={WEB_SEARCH_MODE_LABEL}
            value={webSearchText}
            onChange={(value) => {
              updateCodexDraft({ webSearchText: value });
              void persistCodexConfig({ web_search: value });
            }}
            options={[
              { value: "cached", label: "缓存 (cached)" },
              { value: "live", label: "实时 (live)" },
              { value: "disabled", label: "禁用 (disabled)" },
            ]}
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label={PERSONALITY_LABEL}
          subtitle="控制 web_search 结果的输出风格。none 会删除该配置，交给 Codex 默认行为。"
        >
          <RadioGroup
            name="personality"
            ariaLabel={PERSONALITY_LABEL}
            value={personalityText}
            onChange={(value) => {
              updateCodexDraft({ personalityText: value });
              void persistCodexConfig(buildPersonalityPatch(value));
            }}
            options={[
              { value: "pragmatic", label: "务实 (pragmatic)" },
              { value: "friendly", label: "友好 (friendly)" },
              { value: "none", label: "默认 / 删除配置 (none)" },
            ]}
            disabled={saving}
          />
        </SettingItem>
      </div>
    </div>
  );
}

function CodexProviderSection({
  providerTestModelText,
  setProviderTestModelText,
  saveProviderTestModel,
  providerSyncControlsDisabled,
  providerTestModelControlsDisabled,
  codexProviderSyncing,
  syncCodexProvider,
}: {
  providerTestModelText: string;
  setProviderTestModelText: (value: string) => void;
  saveProviderTestModel: (nextValue: string) => Promise<void>;
  providerSyncControlsDisabled: boolean;
  providerTestModelControlsDisabled: boolean;
  codexProviderSyncing: boolean;
  syncCodexProvider?: () => Promise<void> | void;
}) {
  return (
    <div className="rounded-lg border border-border bg-white p-5 dark:bg-secondary">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
        <Settings className="h-4 w-4 text-muted-foreground" />
        AIO Provider
      </h3>
      <div className="divide-y divide-border">
        <SettingItem
          label="供应商测试默认模型"
          subtitle={`Codex 供应商做可用性测试时使用的全局模型。Provider 页未单独填写时，会回退到这里；默认值是 ${DEFAULT_CODEX_PROVIDER_TEST_MODEL}。`}
        >
          <Input
            value={providerTestModelText}
            onChange={(event) => setProviderTestModelText(event.currentTarget.value)}
            onBlur={() => void saveProviderTestModel(providerTestModelText)}
            placeholder={DEFAULT_CODEX_PROVIDER_TEST_MODEL}
            className="w-[280px] max-w-full font-mono"
            disabled={providerTestModelControlsDisabled}
          />
        </SettingItem>

        <SettingItem
          label="Provider Sync"
          subtitle="手动同步 Codex 历史到当前受管理的 provider。同步可能更新备份、历史会话和 session 关联信息。"
        >
          <Button
            type="button"
            size="sm"
            onClick={() => void syncCodexProvider?.()}
            disabled={providerSyncControlsDisabled}
            className="gap-2"
          >
            <RefreshCw className={cn("h-4 w-4", codexProviderSyncing && "animate-spin")} />
            {codexProviderSyncing ? "同步中…" : "手动 Provider Sync"}
          </Button>
        </SettingItem>
      </div>
    </div>
  );
}

function CodexSandboxSection({
  codexConfig,
  saving,
  effectiveSandboxMode,
  persistCodexConfig,
}: {
  codexConfig: CodexConfigState;
  saving: boolean;
  effectiveSandboxMode: string;
  persistCodexConfig: CliManagerCodexTabProps["persistCodexConfig"];
}) {
  return (
    <div className="rounded-lg border border-border bg-white p-5 dark:bg-secondary">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
        <Settings className="h-4 w-4 text-muted-foreground" />
        Sandbox（workspace-write）
      </h3>
      <div className="divide-y divide-border">
        <SettingItem
          label="允许联网 (sandbox_workspace_write.network_access)"
          subtitle="仅在 sandbox_mode=workspace-write 时生效。开启写入 network_access=true；关闭删除该项。"
        >
          <Switch
            checked={boolOrDefault(codexConfig.sandbox_workspace_write_network_access, false)}
            onCheckedChange={(checked) =>
              void persistCodexConfig({ sandbox_workspace_write_network_access: checked })
            }
            disabled={saving}
          />
        </SettingItem>
      </div>
      {effectiveSandboxMode !== "workspace-write" ? (
        <div className="mt-3 flex items-start gap-2 rounded-lg bg-amber-50 p-3 text-xs text-amber-700 dark:bg-amber-900/30 dark:text-amber-400">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            当前 sandbox_mode 不是 <span className="font-mono">workspace-write</span>
            ，此分区设置可能不会生效。
          </div>
        </div>
      ) : null}
    </div>
  );
}

function CodexFeaturesSection({
  codexConfig,
  saving,
  effectiveFastModeEnabled,
  persistCodexConfig,
}: {
  codexConfig: CodexConfigState;
  saving: boolean;
  effectiveFastModeEnabled: boolean;
  persistCodexConfig: CliManagerCodexTabProps["persistCodexConfig"];
}) {
  return (
    <div className="rounded-lg border border-border bg-white p-5 dark:bg-secondary">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
        <Settings className="h-4 w-4 text-muted-foreground" />
        Features（实验 / 可选能力）
      </h3>
      <div className="divide-y divide-border">
        <SettingItem
          label="remote_compaction"
          subtitle="实验性：启用 remote compaction（需要 ChatGPT 身份验证）。"
        >
          <Switch
            checked={boolOrDefault(codexConfig.features_remote_compaction, false)}
            onCheckedChange={(checked) =>
              void persistCodexConfig({ features_remote_compaction: checked })
            }
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label="fast_mode"
          subtitle='开启同时写入 fast_mode=true 与 service_tier="fast"；关闭则删除这两项。'
        >
          <Switch
            checked={effectiveFastModeEnabled}
            onCheckedChange={(checked) => void persistCodexConfig(buildFastModePatch(checked))}
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label="responses_websockets_v2"
          subtitle="实验性：启用 Responses API websocket 支持（需要中转站支持）。"
        >
          <Switch
            checked={boolOrDefault(codexConfig.features_responses_websockets_v2, false)}
            onCheckedChange={(checked) =>
              void persistCodexConfig({ features_responses_websockets_v2: checked })
            }
            disabled={saving}
          />
        </SettingItem>

        <SettingItem
          label="multi_agent"
          subtitle={
            codexConfig.features_multi_agent == null
              ? "实验性：通过并行生成多个专门化代理来协作完成复杂任务。当前未设置，使用 Codex 默认行为。"
              : "实验性：通过并行生成多个专门化代理来协作完成复杂任务。"
          }
        >
          <Switch
            checked={boolOrDefault(codexConfig.features_multi_agent, false)}
            onCheckedChange={(checked) =>
              void persistCodexConfig({ features_multi_agent: checked })
            }
            disabled={saving}
          />
        </SettingItem>
      </div>
    </div>
  );
}

function CodexTomlAdvancedSection({
  codexConfig,
  codexConfigToml,
  codexConfigTomlLoading,
  tomlAdvancedOpen,
  tomlBusy,
  tomlEditEnabled,
  tomlDraft,
  tomlDirty,
  tomlValidating,
  tomlValidation,
  setTomlAdvancedOpen,
  updateTomlState,
  validateToml,
  saveTomlDraft,
}: {
  codexConfig: CodexConfigState;
  codexConfigToml: CodexConfigTomlState | null;
  codexConfigTomlLoading: boolean;
  tomlAdvancedOpen: boolean;
  tomlBusy: boolean;
  tomlEditEnabled: boolean;
  tomlDraft: string;
  tomlDirty: boolean;
  tomlValidating: boolean;
  tomlValidation: CodexConfigTomlValidationResult | null;
  setTomlAdvancedOpen: (value: boolean) => void;
  updateTomlState: UpdateTomlState;
  validateToml: (toml: string) => Promise<CodexConfigTomlValidationResult | null>;
  saveTomlDraft: () => Promise<void>;
}) {
  return (
    <div className="rounded-lg border border-border bg-white p-5 dark:bg-secondary">
      <details
        className="group"
        open={tomlAdvancedOpen}
        onToggle={(e) => setTomlAdvancedOpen((e.currentTarget as HTMLDetailsElement).open)}
      >
        <summary className="flex cursor-pointer select-none items-center justify-between text-sm font-semibold text-foreground">
          <span className="flex items-center gap-2">
            <Settings className="h-4 w-4 text-muted-foreground" />
            高级配置（config.toml）
          </span>
          <span className="text-xs font-normal text-muted-foreground">
            仅在需要编辑原始 TOML 时使用
          </span>
        </summary>

        {tomlAdvancedOpen ? (
          <div className="mt-4 space-y-3">
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="text-xs text-muted-foreground">路径</div>
                <div className="mt-1 truncate font-mono text-xs text-secondary-foreground">
                  {codexConfig.config_path ?? codexConfigToml?.config_path ?? "—"}
                </div>
              </div>
              <div className="flex items-center justify-end gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    updateTomlState({
                      tomlDraft: codexConfigToml?.toml ?? "",
                      tomlDirty: false,
                      tomlValidation: null,
                    });
                  }}
                  disabled={tomlBusy || tomlEditEnabled}
                >
                  重新加载
                </Button>

                {!tomlEditEnabled ? (
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => {
                      updateTomlState({
                        tomlEditEnabled: true,
                        tomlDraft: codexConfigToml?.toml ?? "",
                        tomlDirty: false,
                        tomlValidation: null,
                      });
                      void validateToml(codexConfigToml?.toml ?? "");
                    }}
                    disabled={tomlBusy}
                  >
                    编辑
                  </Button>
                ) : (
                  <>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => {
                        updateTomlState({
                          tomlEditEnabled: false,
                          tomlDraft: codexConfigToml?.toml ?? "",
                          tomlDirty: false,
                          tomlValidation: null,
                        });
                      }}
                      disabled={tomlBusy}
                    >
                      取消
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => void saveTomlDraft()}
                      disabled={
                        tomlBusy ||
                        tomlValidating ||
                        !tomlDirty ||
                        (tomlValidation ? !tomlValidation.ok : false)
                      }
                    >
                      {tomlValidating ? "校验中…" : "保存"}
                    </Button>
                  </>
                )}
              </div>
            </div>

            {codexConfigTomlLoading ? (
              <div className="py-6 text-center text-sm text-muted-foreground">加载中…</div>
            ) : (
              <Suspense
                fallback={
                  <div className="py-6 text-center text-sm text-muted-foreground">加载编辑器…</div>
                }
              >
                <LazyCodeEditor
                  value={tomlDraft}
                  onChange={
                    tomlEditEnabled
                      ? (next) => {
                          updateTomlState({ tomlDraft: next, tomlDirty: true });
                        }
                      : undefined
                  }
                  readOnly={!tomlEditEnabled || tomlBusy}
                  language="toml"
                  minHeight="260px"
                  placeholder='例如：approval_policy = "on-request"'
                />
              </Suspense>
            )}

            {tomlValidation?.ok === false && tomlValidation.error ? (
              <div className="flex items-start gap-2 rounded-lg bg-rose-50 p-3 text-xs text-rose-700 dark:bg-rose-900/30 dark:text-rose-400">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                <div className="min-w-0">
                  <div className="font-semibold">TOML 校验失败</div>
                  <div className="mt-1 break-words">
                    {tomlValidation.error.message}
                    {tomlValidation.error.line ? (
                      <span className="ml-2 font-mono text-rose-600">
                        (line {tomlValidation.error.line}
                        {tomlValidation.error.column
                          ? `, column ${tomlValidation.error.column}`
                          : ""}
                        )
                      </span>
                    ) : null}
                  </div>
                </div>
              </div>
            ) : (
              <div className="text-xs text-muted-foreground">
                保存前会进行后端 TOML 校验；校验失败不会写入文件。
              </div>
            )}
          </div>
        ) : null}
      </details>
    </div>
  );
}

function useCodexTabController({
  codexLoading,
  codexConfigLoading,
  codexConfigSaving,
  codexConfigTomlLoading,
  codexConfigTomlSaving,
  codexProviderSyncing = false,
  codexModelCatalogLoading = false,
  codexModelCatalogError = false,
  codexInfo,
  codexConfig,
  codexConfigToml,
  codexModelCatalog = null,
  appSettings,
  commonSettingsSaving = false,
  codexHomeSettingsSaving = false,
  refreshCodex,
  persistCodexConfig,
  persistCodexConfigToml,
  syncCodexProvider,
  persistCommonSettings,
  persistCodexHomeSettings,
  persistCodexOauthCompatibleProxyMode,
  pickCodexHomeDirectory,
}: CliManagerCodexTabProps) {
  const customHomeInputId = useId();
  const [uiState, dispatchUiState] = useReducer(
    codexTabUiReducer,
    { codexConfig, codexConfigToml, appSettings },
    initCodexTabUiState
  );
  const {
    versionRefreshToken,
    codexDraft,
    configLocationDraft,
    selectingCodexHomeDir,
    tomlAdvancedOpen,
    tomlState,
  } = uiState;
  const [providerTestModelText, setProviderTestModelText] = useState(
    DEFAULT_CODEX_PROVIDER_TEST_MODEL
  );
  const validateSeqRef = useRef(0);
  const validateTimerRef = useRef<number | null>(null);

  const codexConfigKey = buildCodexConfigKey(codexConfig);
  let effectiveCodexDraft = codexDraft;
  if (codexDraft.configKey !== codexConfigKey) {
    effectiveCodexDraft = buildCodexConfigDraft(codexConfig);
    dispatchUiState({ type: "setCodexDraft", draft: effectiveCodexDraft });
  }

  const configLocationKey = buildConfigLocationKey(appSettings);
  let effectiveConfigLocationDraft = configLocationDraft;
  if (configLocationDraft.settingsKey !== configLocationKey) {
    effectiveConfigLocationDraft = buildConfigLocationDraft(appSettings);
    dispatchUiState({ type: "setConfigLocationDraft", draft: effectiveConfigLocationDraft });
  }

  const nextTomlSourceKey = buildTomlSourceKey(codexConfigToml);
  const nextTomlConfigPath = codexConfigToml?.config_path ?? null;
  let effectiveTomlState = tomlState;
  if (
    tomlState.configPath !== nextTomlConfigPath ||
    (!tomlState.tomlDirty && tomlState.sourceKey !== nextTomlSourceKey)
  ) {
    if (tomlState.configPath !== nextTomlConfigPath) {
      if (validateTimerRef.current) {
        window.clearTimeout(validateTimerRef.current);
        validateTimerRef.current = null;
      }
      validateSeqRef.current += 1;
    }
    effectiveTomlState = buildTomlDraftState(codexConfigToml);
    dispatchUiState({ type: "setTomlState", state: effectiveTomlState });
  }

  const {
    modelText,
    contextWindowText,
    autoCompactLimitText,
    sandboxModeText,
    webSearchText,
    personalityText,
    reasoningEffortText,
    planModeReasoningEffortText,
  } = effectiveCodexDraft;
  const { configLocationMode, customHomeText, configLocationError } = effectiveConfigLocationDraft;
  const { tomlDraft, tomlDirty, tomlValidating, tomlValidation, tomlEditEnabled } =
    effectiveTomlState;

  const modelMigration = useCodexModelMigration({
    codexConfig,
    codexInfo,
    codexModelCatalog,
    persistCodexConfig,
  });
  const reasoningResolution = useMemo(
    () =>
      resolveReasoningOptions(
        modelMigration.catalog,
        codexConfig?.model ?? "",
        reasoningEffortText
      ),
    [codexConfig?.model, modelMigration.catalog, reasoningEffortText]
  );
  const modelSuggestions = useMemo(() => {
    const seen = new Set<string>();
    return (modelMigration.catalog?.models ?? []).filter((entry) => {
      const model = entry.model.trim();
      if (entry.hidden || !model || seen.has(model)) return false;
      seen.add(model);
      return true;
    });
  }, [modelMigration.catalog]);
  const reasoningStatusText = codexModelCatalogLoading
    ? "正在读取模型能力目录…"
    : (modelMigration.statusText ??
      (codexModelCatalogError
        ? "读取模型能力失败，当前推理选项仅供编辑。"
        : reasoningResolution.statusText));
  const reasoningStatusRetryable =
    !codexModelCatalogLoading &&
    (codexModelCatalogError ||
      codexModelCatalog?.status === "degraded" ||
      codexModelCatalog?.status === "unavailable");
  const ultraConflictWarning = ultraConflictText(
    reasoningEffortText,
    codexConfig?.features_multi_agent ?? null
  );

  const updateCodexDraft = useCallback<UpdateCodexDraft>(
    (patch) => dispatchUiState({ type: "patchCodexDraft", patch }),
    []
  );
  const updateConfigLocationDraft = useCallback<UpdateConfigLocationDraft>(
    (patch) => dispatchUiState({ type: "patchConfigLocationDraft", patch }),
    []
  );
  const updateTomlState = useCallback<UpdateTomlState>(
    (patch) => dispatchUiState({ type: "patchTomlState", patch }),
    []
  );

  const validateToml = useCallback(
    async (toml: string): Promise<CodexConfigTomlValidationResult | null> => {
      const seq = validateSeqRef.current + 1;
      validateSeqRef.current = seq;
      dispatchUiState({ type: "patchTomlState", patch: { tomlValidating: true } });
      try {
        if (seq !== validateSeqRef.current) return null;
        const result = await cliManagerCodexConfigTomlValidate(toml);
        if (seq === validateSeqRef.current && result) {
          dispatchUiState({ type: "patchTomlState", patch: { tomlValidation: result } });
          return result;
        }
        return null;
      } finally {
        if (seq === validateSeqRef.current) {
          dispatchUiState({ type: "patchTomlState", patch: { tomlValidating: false } });
        }
      }
    },
    []
  );

  useEffect(() => {
    setProviderTestModelText(
      appSettings?.codex_provider_test_model?.trim() || DEFAULT_CODEX_PROVIDER_TEST_MODEL
    );
  }, [appSettings?.codex_provider_test_model]);

  const saving = codexConfigSaving;
  const loading = codexLoading || codexConfigLoading;
  const tomlBusy = codexConfigSaving || codexConfigTomlLoading || codexConfigTomlSaving;
  const providerSyncControlsDisabled =
    codexConfigSaving || codexConfigTomlSaving || codexProviderSyncing || !syncCodexProvider;
  const configLocationBusy = saving || codexHomeSettingsSaving;
  const configLocationControlsDisabled = configLocationBusy || selectingCodexHomeDir;
  const commonSettingsControlsDisabled = codexHomeSettingsSaving || !appSettings;
  const providerTestModelControlsDisabled =
    commonSettingsSaving || !appSettings || !persistCommonSettings;
  const proxyModeControlsDisabled =
    commonSettingsControlsDisabled || !persistCodexOauthCompatibleProxyMode;

  async function refreshCodexStatus() {
    if (saving) return;
    try {
      await refreshCodex();
    } finally {
      dispatchUiState({ type: "incrementVersionRefreshToken" });
    }
  }

  async function saveProviderTestModel(nextValue: string) {
    if (!persistCommonSettings || !appSettings || providerTestModelControlsDisabled) return;
    const normalized = nextValue.trim() || DEFAULT_CODEX_PROVIDER_TEST_MODEL;
    setProviderTestModelText(normalized);
    try {
      const updated = await persistCommonSettings({ codex_provider_test_model: normalized });
      const saved = updated?.codex_provider_test_model?.trim() || normalized;
      setProviderTestModelText(saved);
    } catch {
      setProviderTestModelText(appSettings.codex_provider_test_model || normalized);
    }
  }

  const defaults = useMemo(() => ({ sandbox_mode: "workspace-write" }), []);
  const effectiveSandboxMode = useMemo(
    () => enumOrDefault(sandboxModeText.trim() || null, defaults.sandbox_mode),
    [defaults.sandbox_mode, sandboxModeText]
  );
  const effectiveFastModeEnabled = useMemo(() => {
    if (!codexConfig) return false;
    return (
      boolOrDefault(codexConfig.features_fast_mode, false) ||
      codexConfig.service_tier === FAST_SERVICE_TIER
    );
  }, [codexConfig]);

  const configLocationPreviewPath = useMemo(
    () => buildConfigTomlPath(customHomeText),
    [customHomeText]
  );
  const userDefaultResolvedHomeDir = useMemo(
    () => codexConfig?.user_home_default_dir?.trim() || "~/.codex",
    [codexConfig?.user_home_default_dir]
  );
  const followCodexHomeResolvedDir = useMemo(
    () => codexConfig?.follow_codex_home_dir?.trim() || "~/.codex",
    [codexConfig?.follow_codex_home_dir]
  );
  const followModeMatchesDefault = useMemo(
    () =>
      normalizeComparablePath(followCodexHomeResolvedDir) ===
      normalizeComparablePath(userDefaultResolvedHomeDir),
    [followCodexHomeResolvedDir, userDefaultResolvedHomeDir]
  );
  const followModeLabel = useMemo(
    () =>
      followModeMatchesDefault
        ? "跟随环境变量 $CODEX_HOME（当前路径与固定目录一致）"
        : "跟随环境变量 $CODEX_HOME",
    [followModeMatchesDefault]
  );
  const configLocationBrowsePath = useMemo(() => {
    const trimmedCustomHome = customHomeText.trim();
    if (trimmedCustomHome) {
      return normalizeCustomCodexHome(trimmedCustomHome);
    }
    const savedOverride = appSettings?.codex_home_override?.trim();
    if (configLocationMode === "custom" && savedOverride) {
      return savedOverride;
    }
    if (configLocationMode === "follow_codex_home") {
      return codexConfig?.follow_codex_home_dir?.trim() || "";
    }
    return codexConfig?.user_home_default_dir?.trim() || "";
  }, [
    appSettings?.codex_home_override,
    codexConfig?.follow_codex_home_dir,
    codexConfig?.user_home_default_dir,
    configLocationMode,
    customHomeText,
  ]);
  const configLocationSummaryText = useMemo(() => {
    if (configLocationMode === "custom") {
      return customHomeText.trim()
        ? "自定义模式已启用。应用会在你指定的 .codex 目录下读写 config.toml。"
        : "自定义模式待保存。请输入一个 .codex 目录路径后按 Enter 或移出输入框保存。";
    }
    if (configLocationMode === "follow_codex_home") {
      return `跟随模式已启用。当前将使用 ${followCodexHomeResolvedDir}；如果没有设置 $CODEX_HOME，则回退到 Windows 用户目录下的 .codex。`;
    }
    return `固定模式已启用。当前固定使用 Windows 用户目录下的 .codex：${userDefaultResolvedHomeDir}。`;
  }, [configLocationMode, customHomeText, followCodexHomeResolvedDir, userDefaultResolvedHomeDir]);
  const activeConfigDirSummaryText = useMemo(() => {
    if (configLocationMode === "custom") return "当前为手动指定目录模式。";
    if (configLocationMode === "follow_codex_home") {
      return "当前路径跟随 $CODEX_HOME 解析；后续会随环境变量变化。";
    }
    return isWindowsRuntime()
      ? "当前固定使用 Windows 用户目录下的 .codex。"
      : "当前固定使用用户主目录下的 .codex。";
  }, [configLocationMode]);
  const activeConfigModeBadgeText = useMemo(() => {
    if (configLocationMode === "custom") return "手动指定";
    if (configLocationMode === "follow_codex_home") return "跟随变量";
    return "固定目录";
  }, [configLocationMode]);
  const activeConfigDirPrimaryText = useMemo(() => {
    if (configLocationMode === "custom") {
      return customHomeText.trim() || codexConfig?.config_dir || "";
    }
    if (configLocationMode === "follow_codex_home") {
      return followCodexHomeResolvedDir;
    }
    return userDefaultResolvedHomeDir;
  }, [
    codexConfig?.config_dir,
    configLocationMode,
    customHomeText,
    followCodexHomeResolvedDir,
    userDefaultResolvedHomeDir,
  ]);

  useEffect(() => {
    if (!tomlAdvancedOpen || !tomlEditEnabled || !tomlDirty) return;
    if (validateTimerRef.current) {
      window.clearTimeout(validateTimerRef.current);
    }
    validateTimerRef.current = window.setTimeout(() => {
      void validateToml(tomlDraft);
    }, 500);
    return () => {
      if (validateTimerRef.current) {
        window.clearTimeout(validateTimerRef.current);
        validateTimerRef.current = null;
      }
    };
  }, [tomlDraft, tomlDirty, tomlAdvancedOpen, tomlEditEnabled, validateToml]);

  async function saveTomlDraft() {
    if (tomlBusy) return;
    if (validateTimerRef.current) {
      window.clearTimeout(validateTimerRef.current);
      validateTimerRef.current = null;
    }
    const result = await validateToml(tomlDraft);
    if (!result || !result.ok) return;
    const ok = await persistCodexConfigToml(tomlDraft);
    if (!ok) return;
    updateTomlState({ tomlEditEnabled: false, tomlDirty: false });
  }

  async function persistConfigLocation(
    nextMode: CodexHomeMode,
    nextCustomHome = customHomeText
  ): Promise<PersistConfigLocationResult> {
    if (!persistCodexHomeSettings) return "persist_failed";

    const trimmed = nextCustomHome.trim();
    const normalized = normalizeCustomCodexHome(trimmed);
    if (nextMode === "custom") {
      const error = validateCustomCodexHome(trimmed);
      updateConfigLocationDraft({ configLocationError: error });
      if (error) return "validation_failed";
    } else {
      updateConfigLocationDraft({ configLocationError: null });
    }

    const nextOverride = nextMode === "custom" ? normalized : "";
    const saved = Boolean(await persistCodexHomeSettings(nextMode, nextOverride));
    if (!saved) return "persist_failed";

    updateConfigLocationDraft({
      configLocationMode: nextMode,
      customHomeText: nextMode === "custom" ? nextOverride : "",
      configLocationError: null,
    });
    return "saved";
  }

  function readSavedConfigLocationState() {
    const { savedMode, savedOverride } = readConfigLocationSettings(appSettings);
    return { savedMode, savedOverride };
  }

  function restoreSavedConfigLocationState() {
    const { savedMode, savedOverride } = readSavedConfigLocationState();
    updateConfigLocationDraft({
      configLocationMode: savedMode,
      customHomeText: savedOverride,
      configLocationError: null,
    });
  }

  async function handleConfigLocationModeChange(nextMode: CodexHomeMode) {
    updateConfigLocationDraft({ configLocationMode: nextMode });

    if (nextMode !== "custom") {
      updateConfigLocationDraft({ customHomeText: "", configLocationError: null });
      const result = await persistConfigLocation(nextMode, "");
      if (result === "persist_failed") {
        restoreSavedConfigLocationState();
      }
      return;
    }

    const error = validateCustomCodexHome(customHomeText);
    updateConfigLocationDraft({ configLocationError: error });
    if (error) return;

    const result = await persistConfigLocation("custom", customHomeText);
    if (result === "persist_failed") {
      restoreSavedConfigLocationState();
    }
  }

  async function resetConfigLocation() {
    updateConfigLocationDraft({
      configLocationMode: "user_home_default",
      customHomeText: "",
      configLocationError: null,
    });
    const result = await persistConfigLocation("user_home_default", "");
    if (result === "persist_failed") {
      restoreSavedConfigLocationState();
    }
  }

  async function handlePickCustomHome() {
    if (!pickCodexHomeDirectory || configLocationControlsDisabled) return;
    dispatchUiState({ type: "setSelectingCodexHomeDir", value: true });
    try {
      const picked = await pickCodexHomeDirectory(configLocationBrowsePath || undefined);
      if (!picked) return;

      const normalized = normalizeCustomCodexHome(picked);
      updateConfigLocationDraft({ configLocationMode: "custom", customHomeText: normalized });
      const error = validateCustomCodexHome(normalized);
      updateConfigLocationDraft({ configLocationError: error });
      if (error) return;

      const result = await persistConfigLocation("custom", normalized);
      if (result === "persist_failed") {
        restoreSavedConfigLocationState();
      }
    } finally {
      dispatchUiState({ type: "setSelectingCodexHomeDir", value: false });
    }
  }

  function setTomlAdvancedOpen(value: boolean) {
    dispatchUiState({ type: "setTomlAdvancedOpen", value });
  }

  return {
    customHomeInputId,
    versionRefreshToken,
    modelText,
    contextWindowText,
    autoCompactLimitText,
    sandboxModeText,
    webSearchText,
    personalityText,
    reasoningEffortText,
    planModeReasoningEffortText,
    configLocationMode,
    customHomeText,
    configLocationError,
    selectingCodexHomeDir,
    tomlAdvancedOpen,
    tomlDraft,
    tomlDirty,
    tomlValidating,
    tomlValidation,
    tomlEditEnabled,
    saving,
    loading,
    tomlBusy,
    providerSyncControlsDisabled,
    providerTestModelControlsDisabled,
    configLocationControlsDisabled,
    proxyModeControlsDisabled,
    effectiveSandboxMode,
    effectiveFastModeEnabled,
    modelSuggestions,
    reasoningOptions: reasoningResolution.options,
    reasoningStatusText,
    reasoningStatusRetryable,
    ultraConflictText: ultraConflictWarning,
    configLocationPreviewPath,
    userDefaultResolvedHomeDir,
    followCodexHomeResolvedDir,
    followModeMatchesDefault,
    followModeLabel,
    configLocationSummaryText,
    activeConfigDirSummaryText,
    activeConfigModeBadgeText,
    activeConfigDirPrimaryText,
    providerTestModelText,
    setProviderTestModelText,
    refreshCodexStatus,
    updateCodexDraft,
    updateConfigLocationDraft,
    updateTomlState,
    validateToml,
    saveTomlDraft,
    saveProviderTestModel,
    persistConfigLocation,
    restoreSavedConfigLocationState,
    handleConfigLocationModeChange,
    resetConfigLocation,
    handlePickCustomHome,
    setTomlAdvancedOpen,
    onModelInputChange: modelMigration.onModelInputChange,
    onEffortInputChange: modelMigration.onEffortInputChange,
    persistModel: modelMigration.persistModel,
  };
}

export function CliManagerCodexTab(props: CliManagerCodexTabProps) {
  const controller = useCodexTabController(props);
  const {
    codexAvailable,
    codexInfo,
    codexConfig,
    codexConfigToml,
    appSettings,
    openCodexConfigDir,
    persistCodexConfig,
    syncCodexProvider,
    codexProviderSyncing = false,
    onOpenGatewayDetailsRoute,
    showRetryGatewayManager = false,
  } = props;

  return (
    <div className="space-y-6">
      <Card className="space-y-4">
        <CodexHeader
          codexAvailable={codexAvailable}
          codexInfo={codexInfo}
          loading={controller.loading}
          saving={controller.saving}
          versionRefreshToken={controller.versionRefreshToken}
          refreshCodexStatus={controller.refreshCodexStatus}
        />

        {!codexConfig ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {codexAvailable === "unavailable" ? "数据不可用" : "暂无配置，请尝试刷新"}
          </div>
        ) : (
          <div className="space-y-4">
            <CodexInfoGrid
              codexConfig={codexConfig}
              codexInfo={codexInfo}
              activeConfigDirSummaryText={controller.activeConfigDirSummaryText}
              openCodexConfigDir={openCodexConfigDir}
            />
            {showRetryGatewayManager ? (
              <CodexRetryGatewayManager onOpenDetailsRoute={onOpenGatewayDetailsRoute} />
            ) : null}
          </div>
        )}
      </Card>

      {codexConfig ? (
        <div className="space-y-4">
          <CodexConfigLocationSection
            codexConfig={codexConfig}
            customHomeInputId={controller.customHomeInputId}
            configLocationMode={controller.configLocationMode}
            customHomeText={controller.customHomeText}
            configLocationError={controller.configLocationError}
            selectingCodexHomeDir={controller.selectingCodexHomeDir}
            configLocationControlsDisabled={controller.configLocationControlsDisabled}
            activeConfigModeBadgeText={controller.activeConfigModeBadgeText}
            activeConfigDirPrimaryText={controller.activeConfigDirPrimaryText}
            activeConfigDirSummaryText={controller.activeConfigDirSummaryText}
            configLocationSummaryText={controller.configLocationSummaryText}
            followModeLabel={controller.followModeLabel}
            followModeMatchesDefault={controller.followModeMatchesDefault}
            userDefaultResolvedHomeDir={controller.userDefaultResolvedHomeDir}
            followCodexHomeResolvedDir={controller.followCodexHomeResolvedDir}
            configLocationPreviewPath={controller.configLocationPreviewPath}
            resetConfigLocation={controller.resetConfigLocation}
            handleConfigLocationModeChange={controller.handleConfigLocationModeChange}
            updateConfigLocationDraft={controller.updateConfigLocationDraft}
            persistConfigLocation={controller.persistConfigLocation}
            restoreSavedConfigLocationState={controller.restoreSavedConfigLocationState}
            handlePickCustomHome={controller.handlePickCustomHome}
          />

          {appSettings ? (
            <CodexOauthProxySection
              appSettings={appSettings}
              proxyModeControlsDisabled={controller.proxyModeControlsDisabled}
              persistCodexOauthCompatibleProxyMode={props.persistCodexOauthCompatibleProxyMode}
            />
          ) : null}

          <CodexBasicConfigSection
            codexConfig={codexConfig}
            saving={controller.saving}
            modelText={controller.modelText}
            modelSuggestions={controller.modelSuggestions}
            contextWindowText={controller.contextWindowText}
            autoCompactLimitText={controller.autoCompactLimitText}
            sandboxModeText={controller.sandboxModeText}
            reasoningEffortText={controller.reasoningEffortText}
            planModeReasoningEffortText={controller.planModeReasoningEffortText}
            webSearchText={controller.webSearchText}
            personalityText={controller.personalityText}
            reasoningOptions={controller.reasoningOptions}
            reasoningStatusText={controller.reasoningStatusText}
            reasoningStatusRetryable={controller.reasoningStatusRetryable}
            ultraConflictText={controller.ultraConflictText}
            onModelInputChange={controller.onModelInputChange}
            onEffortInputChange={controller.onEffortInputChange}
            persistModel={controller.persistModel}
            refreshCodexStatus={controller.refreshCodexStatus}
            updateCodexDraft={controller.updateCodexDraft}
            persistCodexConfig={persistCodexConfig}
          />

          <CodexProviderSection
            providerTestModelText={controller.providerTestModelText}
            setProviderTestModelText={controller.setProviderTestModelText}
            saveProviderTestModel={controller.saveProviderTestModel}
            providerSyncControlsDisabled={controller.providerSyncControlsDisabled}
            providerTestModelControlsDisabled={controller.providerTestModelControlsDisabled}
            codexProviderSyncing={codexProviderSyncing}
            syncCodexProvider={syncCodexProvider}
          />

          <CodexSandboxSection
            codexConfig={codexConfig}
            saving={controller.saving}
            effectiveSandboxMode={controller.effectiveSandboxMode}
            persistCodexConfig={persistCodexConfig}
          />

          <CodexFeaturesSection
            codexConfig={codexConfig}
            saving={controller.saving}
            effectiveFastModeEnabled={controller.effectiveFastModeEnabled}
            persistCodexConfig={persistCodexConfig}
          />

          <CodexTomlAdvancedSection
            codexConfig={codexConfig}
            codexConfigToml={codexConfigToml}
            codexConfigTomlLoading={props.codexConfigTomlLoading}
            tomlAdvancedOpen={controller.tomlAdvancedOpen}
            tomlBusy={controller.tomlBusy}
            tomlEditEnabled={controller.tomlEditEnabled}
            tomlDraft={controller.tomlDraft}
            tomlDirty={controller.tomlDirty}
            tomlValidating={controller.tomlValidating}
            tomlValidation={controller.tomlValidation}
            setTomlAdvancedOpen={controller.setTomlAdvancedOpen}
            updateTomlState={controller.updateTomlState}
            validateToml={controller.validateToml}
            saveTomlDraft={controller.saveTomlDraft}
          />
        </div>
      ) : null}

      {codexInfo?.error ? (
        <div className="mt-4 flex items-start gap-2 rounded-lg bg-rose-50 p-4 text-sm text-rose-600 dark:bg-rose-900/30 dark:text-rose-400">
          <AlertTriangle className="h-5 w-5 shrink-0" />
          <div>
            <span className="font-semibold">检测失败：</span>
            {codexInfo.error}
          </div>
        </div>
      ) : null}
    </div>
  );
}
