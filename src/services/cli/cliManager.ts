import {
  commands,
  type ClaudeCliInfo as GeneratedClaudeCliInfo,
  type ClaudeEnvState as GeneratedClaudeEnvState,
  type ClaudeHookGroup as GeneratedClaudeHookGroup,
  type ClaudeHooksSetInput as GeneratedClaudeHooksSetInput,
  type ClaudeHooksState as GeneratedClaudeHooksState,
  type ClaudeSettingsPatch as GeneratedClaudeSettingsPatch,
  type ClaudeSettingsState as GeneratedClaudeSettingsState,
  type CodexConfigPatch as GeneratedCodexConfigPatch,
  type CodexConfigState as GeneratedCodexConfigState,
  type CodexConfigTomlState as GeneratedCodexConfigTomlState,
  type CodexConfigTomlValidationError as GeneratedCodexConfigTomlValidationError,
  type CodexConfigTomlValidationResult as GeneratedCodexConfigTomlValidationResult,
  type CodexModelCatalogState as GeneratedCodexModelCatalogState,
  type CodexModelCapability as GeneratedCodexModelCapability,
  type CodexModelContextCandidate as GeneratedCodexModelContextCandidate,
  type CodexModelContextCandidatesState as GeneratedCodexModelContextCandidatesState,
  type CodexReasoningEffortOption as GeneratedCodexReasoningEffortOption,
  type CodexProviderSyncResult as GeneratedCodexProviderSyncResult,
  type GeminiConfigPatch as GeneratedGeminiConfigPatch,
  type GeminiConfigState as GeneratedGeminiConfigState,
  type GrokApiBackend as GeneratedGrokApiBackend,
  type GrokConfigState as GeneratedGrokConfigState,
  type GrokProxyPreferences as GeneratedGrokProxyPreferences,
  type SimpleCliInfo as GeneratedSimpleCliInfo,
} from "../../generated/bindings";
import {
  invokeGeneratedIpc,
  mapGeneratedCommandResponse,
  type GeneratedCommandResult,
} from "../generatedIpc";
import { normalizeCodexModelContextModelId } from "../settings/codexModelContextRules";

export type ClaudeCliInfo = GeneratedClaudeCliInfo;
export type SimpleCliInfo = GeneratedSimpleCliInfo;
export type ClaudeEnvState = GeneratedClaudeEnvState;
export type ClaudeSettingsState = GeneratedClaudeSettingsState;
export type ClaudeSettingsPatch = Partial<GeneratedClaudeSettingsPatch>;
export type ClaudeHooksState = GeneratedClaudeHooksState;
export type ClaudeHookGroup = GeneratedClaudeHookGroup;
export type ClaudeHooksSetInput = GeneratedClaudeHooksSetInput;
export type CodexConfigState = GeneratedCodexConfigState;
export type CodexConfigPatch = Partial<GeneratedCodexConfigPatch>;
export type CodexConfigSetOptions = { syncHistory?: boolean };
export type CodexConfigTomlState = GeneratedCodexConfigTomlState;
export type CodexConfigTomlValidationError = GeneratedCodexConfigTomlValidationError;
export type CodexConfigTomlValidationResult = GeneratedCodexConfigTomlValidationResult;
export type CodexModelCatalogState = GeneratedCodexModelCatalogState;
export type CodexModelCapability = GeneratedCodexModelCapability;
export type CodexReasoningEffortOption = GeneratedCodexReasoningEffortOption;
export type GeminiConfigState = GeneratedGeminiConfigState;
export type GeminiConfigPatch = Partial<GeneratedGeminiConfigPatch>;
export type GrokApiBackend = GeneratedGrokApiBackend;
export type GrokConfigState = GeneratedGrokConfigState;
export type GrokProxyPreferences = GeneratedGrokProxyPreferences;
export type ClaudeEnvSetInput = {
  mcpTimeoutMs: number | null;
  disableErrorReporting: boolean;
};
export type CodexProviderSyncResult = GeneratedCodexProviderSyncResult;
export type CodexModelContextCandidate = GeneratedCodexModelContextCandidate;
export type CodexModelContextCandidatesState = GeneratedCodexModelContextCandidatesState;

const CODEX_MODEL_CONTEXT_CANDIDATE_MAX_MODELS = 1_000;
const CODEX_MODEL_CATALOG_STATUS_VALUES = new Set(["ready", "degraded", "unavailable"]);
const CODEX_MODEL_CATALOG_ISSUE_VALUES = new Set([
  "cli_not_found",
  "app_server_unavailable",
  "timeout",
  "protocol_error",
  "empty_catalog",
]);

function invalidCandidateResponse(message: string): never {
  throw new Error(`IPC_INVALID_RESULT: ${message}`);
}

function requireCandidateRecord(value: unknown, label: string): Record<string, unknown> {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    invalidCandidateResponse(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function candidateHasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function requireCandidateOptionalString(value: unknown, label: string): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || value.length === 0) {
    invalidCandidateResponse(`${label} must be a non-empty string or null`);
  }
  return value;
}

function requireCandidateContext(value: unknown, label: string): number | null {
  if (value === null) return null;
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    invalidCandidateResponse(`${label} must be a non-negative safe integer or null`);
  }
  return value as number;
}

function decodeCodexModelContextCandidate(
  value: unknown,
  index: number
): CodexModelContextCandidate {
  const candidate = requireCandidateRecord(value, `models[${index}]`);
  let modelId: string;
  try {
    modelId = normalizeCodexModelContextModelId(candidate.model_id, index);
  } catch {
    invalidCandidateResponse(`models[${index}].model_id is invalid`);
  }
  if (candidate.model_id !== modelId) {
    invalidCandidateResponse(`models[${index}].model_id is not canonical`);
  }
  if (typeof candidate.display_name !== "string" || candidate.display_name.trim().length === 0) {
    invalidCandidateResponse(`models[${index}].display_name must be a non-empty string`);
  }
  if (typeof candidate.hidden !== "boolean") {
    invalidCandidateResponse(`models[${index}].hidden must be a boolean`);
  }
  if (
    !candidateHasOwn(candidate, "base_context_window") ||
    !candidateHasOwn(candidate, "base_max_context_window")
  ) {
    invalidCandidateResponse(`models[${index}] is missing context fields`);
  }

  return {
    model_id: modelId,
    display_name: candidate.display_name,
    hidden: candidate.hidden,
    base_context_window: requireCandidateContext(
      candidate.base_context_window,
      `models[${index}].base_context_window`
    ),
    base_max_context_window: requireCandidateContext(
      candidate.base_max_context_window,
      `models[${index}].base_max_context_window`
    ),
  };
}

export function decodeCodexModelContextCandidatesState(
  value: unknown
): CodexModelContextCandidatesState {
  const state = requireCandidateRecord(value, "model context candidates");
  if (typeof state.status !== "string" || !CODEX_MODEL_CATALOG_STATUS_VALUES.has(state.status)) {
    invalidCandidateResponse("model context candidates status is invalid");
  }
  if (!candidateHasOwn(state, "issue")) {
    invalidCandidateResponse("model context candidates issue is required");
  }
  if (
    state.issue !== null &&
    (typeof state.issue !== "string" || !CODEX_MODEL_CATALOG_ISSUE_VALUES.has(state.issue))
  ) {
    invalidCandidateResponse("model context candidates issue is invalid");
  }

  const snapshot = requireCandidateRecord(state.snapshot, "model context candidates snapshot");
  if (typeof snapshot.config_path !== "string" || snapshot.config_path.length === 0) {
    invalidCandidateResponse("model context candidates snapshot.config_path is required");
  }
  if (!candidateHasOwn(snapshot, "executable_path") || !candidateHasOwn(snapshot, "cli_version")) {
    invalidCandidateResponse("model context candidates snapshot is incomplete");
  }
  if (!Array.isArray(state.models)) {
    invalidCandidateResponse("model context candidates models must be an array");
  }
  if (state.models.length > CODEX_MODEL_CONTEXT_CANDIDATE_MAX_MODELS) {
    invalidCandidateResponse("model context candidates contains too many models");
  }

  const models = state.models.map(decodeCodexModelContextCandidate);
  if (new Set(models.map((model) => model.model_id)).size !== models.length) {
    invalidCandidateResponse("model context candidates contains duplicate model IDs");
  }

  return {
    status: state.status as CodexModelContextCandidatesState["status"],
    issue: state.issue as CodexModelContextCandidatesState["issue"],
    snapshot: {
      config_path: snapshot.config_path,
      executable_path: requireCandidateOptionalString(
        snapshot.executable_path,
        "model context candidates snapshot.executable_path"
      ),
      cli_version: requireCandidateOptionalString(
        snapshot.cli_version,
        "model context candidates snapshot.cli_version"
      ),
    },
    models,
  };
}

const DEFAULT_CODEX_CONFIG_PATCH = {
  model: null,
  approval_policy: null,
  approvals_reviewer: null,
  sandbox_mode: null,
  model_reasoning_effort: null,
  plan_mode_reasoning_effort: null,
  web_search: null,
  personality: null,
  service_tier: null,
  sandbox_workspace_write_network_access: null,
  features_unified_exec: null,
  features_shell_snapshot: null,
  features_apply_patch_freeform: null,
  features_shell_tool: null,
  features_exec_policy: null,
  features_remote_compaction: null,
  features_fast_mode: null,
  features_responses_websockets_v2: null,
  features_multi_agent: null,
} satisfies GeneratedCodexConfigPatch;

const DEFAULT_GEMINI_CONFIG_PATCH = {
  modelName: null,
  modelMaxSessionTurns: null,
  modelCompressionThreshold: null,
  defaultApprovalMode: null,
  enableAutoUpdate: null,
  enableNotifications: null,
  vimMode: null,
  retryFetchErrors: null,
  maxAttempts: null,
  uiTheme: null,
  uiHideBanner: null,
  uiHideTips: null,
  uiShowLineNumbers: null,
  uiInlineThinkingMode: null,
  usageStatisticsEnabled: null,
  sessionRetentionEnabled: null,
  sessionRetentionMaxAge: null,
  planModelRouting: null,
  securityAuthSelectedType: null,
} satisfies GeneratedGeminiConfigPatch;

const DEFAULT_CLAUDE_SETTINGS_PATCH = {
  model: null,
  output_style: null,
  language: null,
  always_thinking_enabled: null,
  show_turn_duration: null,
  spinner_tips_enabled: null,
  terminal_progress_bar_enabled: null,
  respect_gitignore: null,
  disable_git_participant: null,
  permissions_allow: null,
  permissions_ask: null,
  permissions_deny: null,
  env_mcp_timeout_ms: null,
  env_mcp_tool_timeout_ms: null,
  env_experimental_agent_teams: null,
  env_claude_code_auto_compact_window: null,
  env_disable_background_tasks: null,
  env_disable_terminal_title: null,
  env_claude_bash_no_login: null,
  env_claude_code_attribution_header: null,
  env_claude_code_blocking_limit_override: null,
  env_claude_code_max_output_tokens: null,
  env_enable_experimental_mcp_cli: null,
  env_enable_tool_search: null,
  env_max_mcp_output_tokens: null,
  env_claude_code_disable_nonessential_traffic: null,
  env_claude_code_disable_1m_context: null,
  env_claude_code_proxy_resolves_hosts: null,
  env_claude_code_skip_prompt_history: null,
} satisfies GeneratedClaudeSettingsPatch;

function withGeneratedPatchDefaults<TPatch extends object>(
  defaults: TPatch,
  patch: Partial<TPatch>
): TPatch {
  return {
    ...defaults,
    ...patch,
  };
}

function toCodexConfigPatch(patch: CodexConfigPatch): GeneratedCodexConfigPatch {
  return {
    ...DEFAULT_CODEX_CONFIG_PATCH,
    ...patch,
  };
}

function toGeminiConfigPatch(patch: GeminiConfigPatch): GeneratedGeminiConfigPatch {
  return withGeneratedPatchDefaults(DEFAULT_GEMINI_CONFIG_PATCH, patch);
}

function toClaudeSettingsPatch(patch: ClaudeSettingsPatch): GeneratedClaudeSettingsPatch {
  return withGeneratedPatchDefaults(DEFAULT_CLAUDE_SETTINGS_PATCH, patch);
}

export async function cliManagerClaudeInfoGet() {
  return invokeGeneratedIpc<ClaudeCliInfo>({
    title: "获取 Claude CLI 信息失败",
    cmd: "cli_manager_claude_info_get",
    invoke: () =>
      commands.cliManagerClaudeInfoGet() as Promise<GeneratedCommandResult<ClaudeCliInfo>>,
  });
}

export async function cliManagerCodexInfoGet() {
  return invokeGeneratedIpc<SimpleCliInfo>({
    title: "获取 Codex CLI 信息失败",
    cmd: "cli_manager_codex_info_get",
    invoke: () =>
      commands.cliManagerCodexInfoGet() as Promise<GeneratedCommandResult<SimpleCliInfo>>,
  });
}

export async function cliManagerCodexModelCatalogGet() {
  return invokeGeneratedIpc<CodexModelCatalogState>({
    title: "读取 Codex 模型能力失败",
    cmd: "cli_manager_codex_model_catalog_get",
    invoke: () =>
      commands.cliManagerCodexModelCatalogGet() as Promise<
        GeneratedCommandResult<CodexModelCatalogState>
      >,
  });
}

export async function cliManagerCodexModelContextCandidatesGet() {
  return invokeGeneratedIpc<CodexModelContextCandidatesState>({
    title: "读取 Codex 模型上下文候选失败",
    cmd: "cli_manager_codex_model_context_candidates_get",
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.cliManagerCodexModelContextCandidatesGet(),
        decodeCodexModelContextCandidatesState
      ),
  });
}

export async function cliManagerCodexConfigGet() {
  return invokeGeneratedIpc<CodexConfigState>({
    title: "读取 Codex 配置失败",
    cmd: "cli_manager_codex_config_get",
    invoke: () =>
      commands.cliManagerCodexConfigGet() as Promise<GeneratedCommandResult<CodexConfigState>>,
  });
}

export async function cliManagerCodexConfigSet(
  patch: CodexConfigPatch,
  options: CodexConfigSetOptions = {}
) {
  const normalizedPatch = toCodexConfigPatch(patch);
  const syncHistory = options.syncHistory ?? false;
  return invokeGeneratedIpc<CodexConfigState>({
    title: "保存 Codex 配置失败",
    cmd: "cli_manager_codex_config_set",
    args: { patch: normalizedPatch, syncHistory },
    invoke: () =>
      commands.cliManagerCodexConfigSet(normalizedPatch, syncHistory) as Promise<
        GeneratedCommandResult<CodexConfigState>
      >,
  });
}

export async function cliManagerCodexConfigTomlGet() {
  return invokeGeneratedIpc<CodexConfigTomlState>({
    title: "读取 Codex TOML 配置失败",
    cmd: "cli_manager_codex_config_toml_get",
    invoke: () =>
      commands.cliManagerCodexConfigTomlGet() as Promise<
        GeneratedCommandResult<CodexConfigTomlState>
      >,
  });
}

export async function cliManagerCodexConfigTomlValidate(toml: string) {
  return invokeGeneratedIpc<CodexConfigTomlValidationResult>({
    title: "校验 Codex TOML 配置失败",
    cmd: "cli_manager_codex_config_toml_validate",
    args: { toml },
    invoke: () =>
      commands.cliManagerCodexConfigTomlValidate(toml) as Promise<
        GeneratedCommandResult<CodexConfigTomlValidationResult>
      >,
  });
}

export async function cliManagerCodexConfigTomlSet(toml: string) {
  return invokeGeneratedIpc<CodexConfigState>({
    title: "保存 Codex TOML 配置失败",
    cmd: "cli_manager_codex_config_toml_set",
    args: { toml },
    invoke: () =>
      commands.cliManagerCodexConfigTomlSet(toml) as Promise<
        GeneratedCommandResult<CodexConfigState>
      >,
  });
}

export async function cliManagerCodexProviderSync() {
  return invokeGeneratedIpc<CodexProviderSyncResult>({
    title: "同步 Codex Provider 失败",
    cmd: "cli_manager_codex_provider_sync",
    invoke: () =>
      commands.cliManagerCodexProviderSync() as Promise<
        GeneratedCommandResult<CodexProviderSyncResult>
      >,
  });
}

export async function cliManagerGeminiInfoGet() {
  return invokeGeneratedIpc<SimpleCliInfo>({
    title: "获取 Gemini CLI 信息失败",
    cmd: "cli_manager_gemini_info_get",
    invoke: () =>
      commands.cliManagerGeminiInfoGet() as Promise<GeneratedCommandResult<SimpleCliInfo>>,
  });
}

export async function cliManagerGeminiConfigGet() {
  return invokeGeneratedIpc<GeminiConfigState>({
    title: "读取 Gemini 配置失败",
    cmd: "cli_manager_gemini_config_get",
    invoke: () =>
      commands.cliManagerGeminiConfigGet() as Promise<GeneratedCommandResult<GeminiConfigState>>,
  });
}

export async function cliManagerGeminiConfigSet(patch: GeminiConfigPatch) {
  const normalizedPatch = toGeminiConfigPatch(patch);
  return invokeGeneratedIpc<GeminiConfigState>({
    title: "保存 Gemini 配置失败",
    cmd: "cli_manager_gemini_config_set",
    args: { patch: normalizedPatch },
    invoke: () =>
      commands.cliManagerGeminiConfigSet(normalizedPatch) as Promise<
        GeneratedCommandResult<GeminiConfigState>
      >,
  });
}

export async function cliManagerGrokInfoGet() {
  return invokeGeneratedIpc<SimpleCliInfo>({
    title: "获取 Grok CLI 信息失败",
    cmd: "cli_manager_grok_info_get",
    invoke: () =>
      commands.cliManagerGrokInfoGet() as Promise<GeneratedCommandResult<SimpleCliInfo>>,
  });
}

export async function cliManagerGrokConfigGet() {
  return invokeGeneratedIpc<GrokConfigState>({
    title: "读取 Grok 配置失败",
    cmd: "cli_manager_grok_config_get",
    invoke: () =>
      commands.cliManagerGrokConfigGet() as Promise<GeneratedCommandResult<GrokConfigState>>,
  });
}

export async function cliManagerGrokConfigSet(preferences: GrokProxyPreferences) {
  return invokeGeneratedIpc<GrokConfigState>({
    title: "保存 Grok 配置失败",
    cmd: "cli_manager_grok_config_set",
    args: { preferences },
    invoke: () =>
      commands.cliManagerGrokConfigSet(preferences) as Promise<
        GeneratedCommandResult<GrokConfigState>
      >,
  });
}

export async function cliManagerClaudeEnvSet(input: ClaudeEnvSetInput) {
  return invokeGeneratedIpc<ClaudeEnvState>({
    title: "保存 Claude 环境变量失败",
    cmd: "cli_manager_claude_env_set",
    args: {
      mcpTimeoutMs: input.mcpTimeoutMs,
      disableErrorReporting: input.disableErrorReporting,
    },
    invoke: () =>
      commands.cliManagerClaudeEnvSet(input.mcpTimeoutMs, input.disableErrorReporting) as Promise<
        GeneratedCommandResult<ClaudeEnvState>
      >,
  });
}

export async function cliManagerClaudeSettingsGet() {
  return invokeGeneratedIpc<ClaudeSettingsState>({
    title: "读取 Claude 设置失败",
    cmd: "cli_manager_claude_settings_get",
    invoke: () =>
      commands.cliManagerClaudeSettingsGet() as Promise<
        GeneratedCommandResult<ClaudeSettingsState>
      >,
  });
}

export async function cliManagerClaudeSettingsSet(patch: ClaudeSettingsPatch) {
  const normalizedPatch = toClaudeSettingsPatch(patch);
  return invokeGeneratedIpc<ClaudeSettingsState>({
    title: "保存 Claude 设置失败",
    cmd: "cli_manager_claude_settings_set",
    args: { patch: normalizedPatch },
    invoke: () =>
      commands.cliManagerClaudeSettingsSet(normalizedPatch) as Promise<
        GeneratedCommandResult<ClaudeSettingsState>
      >,
  });
}

export async function cliManagerClaudeHooksGet() {
  return invokeGeneratedIpc<ClaudeHooksState>({
    title: "读取 Claude Hooks 失败",
    cmd: "cli_manager_claude_hooks_get",
    invoke: () =>
      commands.cliManagerClaudeHooksGet() as Promise<GeneratedCommandResult<ClaudeHooksState>>,
  });
}

export async function cliManagerClaudeHooksSet(input: ClaudeHooksSetInput) {
  return invokeGeneratedIpc<ClaudeHooksState>({
    title: "保存 Claude Hooks 失败",
    cmd: "cli_manager_claude_hooks_set",
    args: { input },
    invoke: () =>
      commands.cliManagerClaudeHooksSet(input) as Promise<GeneratedCommandResult<ClaudeHooksState>>,
  });
}
