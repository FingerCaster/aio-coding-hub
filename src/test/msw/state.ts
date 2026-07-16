// Usage: Shared MSW in-memory state for tests that run through `invoke` -> fetch -> MSW handlers.

import type { AppAboutInfo } from "../../services/app/appAbout";
import type {
  CodexRetryGatewayApplyCommitRequest,
  CodexRetryGatewayCommitValidation,
  CodexRetryGatewayDetailsSession,
  CodexRetryGatewayEnablePlan,
  CodexRetryGatewayNodeStatus,
  CodexRetryGatewaySetEnabledRequest,
  CodexRetryGatewaySetNodeOverrideRequest,
  CodexRetryGatewayStatus,
  CodexRetryGatewayUninstallRequest,
  CodexRetryGatewayUpdateCandidate,
} from "../../services/cli/codexRetryGateway";
import { createCliProxyStatus, type CliProxyStatus } from "../../services/cli/cliProxyStatus";
import type { CliProxyResult } from "../../services/cli/cliProxy";
import type { DbDiskUsage } from "../../services/app/dataManagement";
import type { EnvConflict } from "../../services/cli/envConflicts";
import type { GatewayStatus } from "../../services/gateway/gateway";
import type { PluginDetail, PluginSummary } from "../../services/plugins";
import type { CliKey, ProviderSummary } from "../../services/providers/providers";
import type { AppSettings } from "../../services/settings/settings";
import { DEFAULT_UPSTREAM_RETRY_POLICY } from "../../services/gateway/upstreamRetryPolicy";
import type { SortModeActiveRow, SortModeSummary } from "../../services/providers/sortModes";
import type { UsageSummary } from "../../services/usage/usage";
import type { WorkspacesListResult } from "../../services/workspace/workspaces";

const DEFAULT_BASE_ORIGIN = "http://127.0.0.1:37123";

const DEFAULT_CLI_PROXY_STATUS: CliProxyStatus[] = [
  createCliProxyStatus({ cli_key: "claude", enabled: false }),
  createCliProxyStatus({ cli_key: "codex", enabled: false }),
  createCliProxyStatus({ cli_key: "gemini", enabled: false }),
];

// Default settings matching the Rust backend defaults.
const DEFAULT_SETTINGS: AppSettings = {
  schema_version: 49,
  preferred_port: 37123,
  show_home_heatmap: true,
  show_home_usage: true,
  home_usage_period: "last15",
  gateway_listen_mode: "localhost",
  gateway_custom_listen_address: "",
  wsl_auto_config: false,
  wsl_target_cli: { claude: true, codex: true, gemini: true },
  cli_priority_order: ["claude", "codex", "gemini"],
  wsl_host_address_mode: "auto",
  wsl_custom_host_address: "127.0.0.1",
  codex_home_mode: "user_home_default",
  codex_home_override: "",
  codex_oauth_compatible_proxy_mode: false,
  codex_provider_test_model: "gpt-5.4-mini",
  auto_start: false,
  start_minimized: false,
  tray_enabled: true,
  enable_cli_proxy_startup_recovery: true,
  log_retention_days: 7,
  request_log_retention_days: 0,
  provider_cooldown_seconds: 30,
  provider_base_url_ping_cache_ttl_seconds: 60,
  upstream_first_byte_timeout_seconds: 30,
  upstream_stream_idle_timeout_seconds: 300,
  upstream_request_timeout_non_streaming_seconds: 0,
  update_releases_url: "https://github.com/FingerCaster/aio-coding-hub/releases",
  failover_max_attempts_per_provider: 5,
  failover_max_providers_to_try: 5,
  upstream_retry_policy: DEFAULT_UPSTREAM_RETRY_POLICY,
  circuit_breaker_failure_threshold: 5,
  circuit_breaker_open_duration_minutes: 30,
  enable_circuit_breaker_notice: false,
  verbose_provider_error: true,
  intercept_anthropic_warmup_requests: true,
  enable_thinking_signature_rectifier: true,
  enable_thinking_budget_rectifier: true,
  enable_billing_header_rectifier: false,
  enable_codex_session_id_completion: true,
  enable_claude_metadata_user_id_injection: true,
  enable_cache_anomaly_monitor: false,
  enable_debug_log: false,
  enable_task_complete_notify: true,
  enable_notification_sound: true,
  enable_response_fixer: true,
  response_fixer_fix_encoding: true,
  response_fixer_fix_sse_format: true,
  response_fixer_fix_truncated_json: true,
  response_fixer_max_json_depth: 200,
  response_fixer_max_fix_size: 1048576,
  cx2cc_fallback_model_opus: "gpt-5.4",
  cx2cc_fallback_model_sonnet: "gpt-5.4",
  cx2cc_fallback_model_haiku: "gpt-5.4",
  cx2cc_fallback_model_main: "gpt-5.4",
  cx2cc_model_reasoning_effort: "",
  cx2cc_service_tier: "",
  cx2cc_disable_response_storage: true,
  cx2cc_enable_reasoning_to_thinking: true,
  cx2cc_drop_stop_sequences: true,
  cx2cc_clean_schema: true,
  cx2cc_filter_batch_tool: true,
  upstream_proxy_enabled: false,
  upstream_proxy_url: "",
  upstream_proxy_username: "",
  upstream_proxy_password_configured: false,
};

const DEFAULT_GATEWAY_STATUS: GatewayStatus = {
  running: false,
  port: null,
  base_url: null,
  listen_addr: null,
};

const DEFAULT_CODEX_RETRY_GATEWAY_NODE_STATUS: CodexRetryGatewayNodeStatus = {
  available: true,
  executable: "C:\\Program Files\\nodejs\\node.exe",
  version: "20.12.2",
  source: "aio_discovery",
  error: null,
};

const DEFAULT_CODEX_RETRY_GATEWAY_STATUS: CodexRetryGatewayStatus = {
  generation: 1,
  desired_enabled: false,
  runtime_phase: "disabled",
  route_mode: "direct_aio",
  cli_proxy_enabled: false,
  cli_proxy_applied: false,
  effective_port: null,
  repository: "nonononull/codex-retry-gateway",
  license: null,
  selected_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2",
  active_commit: null,
  previous_commit: null,
  recommended_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2",
  trust_state: "aio_reviewed_recommendation",
  node_status: DEFAULT_CODEX_RETRY_GATEWAY_NODE_STATUS,
  process_status: {
    phase: "stopped",
    owned: false,
    healthy: false,
    process_id: null,
    listener: null,
  },
  update_candidate: null,
  wsl_codex_unprotected: true,
  last_error: null,
  details_available: false,
  operation_pending: false,
};

const DEFAULT_APP_ABOUT: AppAboutInfo = {
  os: "darwin",
  arch: "aarch64",
  profile: "debug",
  app_version: "0.0.0-test",
  bundle_type: null,
  run_mode: "development",
};

const DEFAULT_DB_DISK_USAGE: DbDiskUsage = {
  db_bytes: 0,
  wal_bytes: 0,
  shm_bytes: 0,
  total_bytes: 0,
};

const DEFAULT_USAGE_SUMMARY: UsageSummary = {
  requests_total: 0,
  requests_with_usage: 0,
  requests_success: 0,
  requests_failed: 0,
  cost_covered_success: 0,
  total_duration_ms: 0,
  avg_duration_ms: null,
  avg_ttfb_ms: null,
  avg_output_tokens_per_second: null,
  input_tokens: 0,
  output_tokens: 0,
  io_total_tokens: 0,
  total_tokens: 0,
  cache_read_input_tokens: 0,
  cache_creation_input_tokens: 0,
  cache_creation_5m_input_tokens: 0,
  cache_creation_1h_input_tokens: 0,
};

let traceCounter = 0;
let cliProxyStatusAllState: CliProxyStatus[] = structuredClone(DEFAULT_CLI_PROXY_STATUS);
let envConflictsState: EnvConflict[] = [];
let settingsState: AppSettings = clone(DEFAULT_SETTINGS);
let gatewayStatusState: GatewayStatus = clone(DEFAULT_GATEWAY_STATUS);
let codexRetryGatewayStatusState: CodexRetryGatewayStatus = clone(
  DEFAULT_CODEX_RETRY_GATEWAY_STATUS
);
let providersState: Map<CliKey, ProviderSummary[]> = new Map();
let usageSummaryState: UsageSummary = clone(DEFAULT_USAGE_SUMMARY);
let appAboutState: AppAboutInfo = clone(DEFAULT_APP_ABOUT);
let dbDiskUsageState: DbDiskUsage = clone(DEFAULT_DB_DISK_USAGE);
let sortModesState: SortModeSummary[] = [];
let sortModeActiveState: SortModeActiveRow[] = [];
let workspacesState: Map<CliKey, WorkspacesListResult> = new Map();
let pluginState: Map<string, PluginDetail> = new Map();

function clone<T>(value: T): T {
  return structuredClone(value);
}

function nextTraceId(): string {
  traceCounter += 1;
  return `msw-${traceCounter}`;
}

export function resetMswState() {
  traceCounter = 0;
  cliProxyStatusAllState = clone(DEFAULT_CLI_PROXY_STATUS);
  envConflictsState = [];
  settingsState = clone(DEFAULT_SETTINGS);
  gatewayStatusState = clone(DEFAULT_GATEWAY_STATUS);
  codexRetryGatewayStatusState = clone(DEFAULT_CODEX_RETRY_GATEWAY_STATUS);
  providersState = new Map();
  usageSummaryState = clone(DEFAULT_USAGE_SUMMARY);
  appAboutState = clone(DEFAULT_APP_ABOUT);
  dbDiskUsageState = clone(DEFAULT_DB_DISK_USAGE);
  sortModesState = [];
  sortModeActiveState = [];
  workspacesState = new Map();
  pluginState = new Map();
}

export function getCliProxyStatusAllState(): CliProxyStatus[] {
  return clone(cliProxyStatusAllState);
}

export function setCliProxyStatusAllState(next: CliProxyStatus[]) {
  cliProxyStatusAllState = clone(next);
}

export function getEnvConflictsState(): EnvConflict[] {
  return clone(envConflictsState);
}

// -- Settings --

export function getSettingsState(): AppSettings {
  return clone(settingsState);
}

export function mergeSettingsState(partial: Partial<AppSettings>): AppSettings {
  settingsState = { ...settingsState, ...partial };
  return clone(settingsState);
}

// -- Gateway --

export function getGatewayStatusState(): GatewayStatus {
  return clone(gatewayStatusState);
}

export function getCodexRetryGatewayStatusState(): CodexRetryGatewayStatus {
  return clone(codexRetryGatewayStatusState);
}

export function setCodexRetryGatewayStatusState(next: CodexRetryGatewayStatus) {
  codexRetryGatewayStatusState = clone(next);
}

function nextCodexRetryGatewayGeneration() {
  return codexRetryGatewayStatusState.generation + 1;
}

function syncCodexGatewayCliProxy(input: {
  enabled: boolean;
  applied: boolean;
  baseOrigin?: string | null;
}) {
  const baseOrigin = input.enabled ? (input.baseOrigin ?? DEFAULT_BASE_ORIGIN) : null;
  cliProxyStatusAllState = cliProxyStatusAllState.map((row) =>
    row.cli_key === "codex"
      ? {
          ...row,
          enabled: input.enabled,
          base_origin: baseOrigin,
          applied_to_current_gateway: input.enabled ? input.applied : null,
        }
      : row
  );
}

function deriveGatewayEnablePlan(
  status = codexRetryGatewayStatusState
): CodexRetryGatewayEnablePlan {
  return {
    generation: status.generation,
    selected_commit: status.selected_commit,
    trust_state: status.trust_state,
    first_download_required: status.active_commit == null,
    unreviewed_commit: status.trust_state === "official_main_unreviewed",
    cli_proxy_enable_required: !status.cli_proxy_enabled,
    provider_sync: {
      current_provider: status.desired_enabled ? "aio-codex-gateway" : "aio-direct",
      target_provider: "aio-codex-gateway",
      change_required: true,
      codex_must_be_closed: true,
    },
    node_status: clone(status.node_status),
    preferred_port: 4610,
    wsl_codex_unprotected: status.wsl_codex_unprotected,
  };
}

export function buildCodexRetryGatewayEnablePlanState(): CodexRetryGatewayEnablePlan {
  return deriveGatewayEnablePlan();
}

export function buildCodexRetryGatewaySetEnabledState(
  request: CodexRetryGatewaySetEnabledRequest | null | undefined
): CodexRetryGatewayStatus {
  if (!request) {
    return getCodexRetryGatewayStatusState();
  }
  const generation = Math.max(nextCodexRetryGatewayGeneration(), request.planGeneration + 1);
  const current = codexRetryGatewayStatusState;

  if (!request.enabled) {
    const next: CodexRetryGatewayStatus = {
      ...current,
      generation,
      desired_enabled: false,
      runtime_phase: "disabled",
      route_mode: "direct_aio",
      cli_proxy_enabled: true,
      cli_proxy_applied: false,
      effective_port: null,
      active_commit: null,
      process_status: {
        phase: "stopped",
        owned: false,
        healthy: false,
        process_id: null,
        listener: null,
      },
      details_available: false,
      last_error: null,
      operation_pending: false,
    };
    codexRetryGatewayStatusState = clone(next);
    syncCodexGatewayCliProxy({ enabled: true, applied: false });
    return getCodexRetryGatewayStatusState();
  }

  const next: CodexRetryGatewayStatus = {
    ...current,
    generation,
    desired_enabled: true,
    runtime_phase: "guarded",
    route_mode: "guarded",
    cli_proxy_enabled: true,
    cli_proxy_applied: true,
    effective_port: 4610,
    active_commit: current.selected_commit,
    process_status: {
      phase: "healthy",
      owned: true,
      healthy: true,
      process_id: 4242,
      listener: "http://127.0.0.1:4610",
    },
    details_available: true,
    last_error: null,
    operation_pending: false,
  };
  codexRetryGatewayStatusState = clone(next);
  syncCodexGatewayCliProxy({ enabled: true, applied: true, baseOrigin: "http://127.0.0.1:4610" });
  return getCodexRetryGatewayStatusState();
}

export function buildCodexRetryGatewayCheckUpdateState(): CodexRetryGatewayUpdateCandidate | null {
  const candidate: CodexRetryGatewayUpdateCandidate = {
    commit: "2222222222222222222222222222222222222222",
    current_commit: codexRetryGatewayStatusState.active_commit,
    previous_commit: codexRetryGatewayStatusState.selected_commit,
    rollback_commit: codexRetryGatewayStatusState.active_commit,
    official_main_commit: "3333333333333333333333333333333333333333",
    commits_ahead: 2,
    summary: "MSW 候选提交：包含新的健康检查和桥接修复。",
    trust_state: "official_main_unreviewed",
  };
  codexRetryGatewayStatusState = {
    ...codexRetryGatewayStatusState,
    update_candidate: candidate,
  };
  return clone(candidate);
}

export function buildCodexRetryGatewayValidateCommitState(
  commit: string
): CodexRetryGatewayCommitValidation {
  const normalized = commit.trim().toLowerCase();
  if (!/^[0-9a-f]{7,40}$/u.test(normalized)) {
    return {
      requested_commit: commit,
      canonical_commit: null,
      official_main_commit: "3333333333333333333333333333333333333333",
      official_main_ancestor: false,
      trust_state: null,
      summary: null,
      error: {
        code: "INVALID_COMMIT",
        category: "source_resolution",
        message: "提交 SHA 非法或当前不可解析。",
        retryable: false,
      },
    };
  }

  return {
    requested_commit: commit,
    canonical_commit: normalized.padEnd(40, "0").slice(0, 40),
    official_main_commit: "3333333333333333333333333333333333333333",
    official_main_ancestor: true,
    trust_state:
      normalized === codexRetryGatewayStatusState.recommended_commit
        ? "aio_reviewed_recommendation"
        : "official_main_unreviewed",
    summary: "MSW 校验通过：该提交可用于切换测试。",
    error: null,
  };
}

export function buildCodexRetryGatewayApplyCommitState(
  request: CodexRetryGatewayApplyCommitRequest | null | undefined
): CodexRetryGatewayStatus {
  if (!request) {
    return getCodexRetryGatewayStatusState();
  }
  const current = codexRetryGatewayStatusState;
  const next: CodexRetryGatewayStatus = {
    ...current,
    generation: Math.max(nextCodexRetryGatewayGeneration(), request.planGeneration + 1),
    previous_commit: current.selected_commit,
    selected_commit: request.commit,
    active_commit: current.desired_enabled ? request.commit : null,
    trust_state: request.acceptedUnreviewedCommit
      ? "official_main_unreviewed"
      : request.commit === current.recommended_commit
        ? "aio_reviewed_recommendation"
        : current.trust_state,
    update_candidate: null,
    last_error: null,
  };
  codexRetryGatewayStatusState = clone(next);
  return getCodexRetryGatewayStatusState();
}

export function buildCodexRetryGatewaySetNodeOverrideState(
  request: CodexRetryGatewaySetNodeOverrideRequest | null | undefined
): CodexRetryGatewayNodeStatus {
  if (!request) {
    return clone(codexRetryGatewayStatusState.node_status);
  }
  const nextNodeStatus: CodexRetryGatewayNodeStatus = request.executable
    ? {
        available: true,
        executable: request.executable,
        version: "20.12.2",
        source: "manual_override",
        error: null,
      }
    : clone(DEFAULT_CODEX_RETRY_GATEWAY_NODE_STATUS);
  codexRetryGatewayStatusState = {
    ...codexRetryGatewayStatusState,
    generation: Math.max(nextCodexRetryGatewayGeneration(), request.generation + 1),
    node_status: nextNodeStatus,
  };
  return clone(nextNodeStatus);
}

export function buildCodexRetryGatewayRetryState(generation: number): CodexRetryGatewayStatus {
  const current = codexRetryGatewayStatusState;
  const next: CodexRetryGatewayStatus = {
    ...current,
    generation: Math.max(nextCodexRetryGatewayGeneration(), generation + 1),
    runtime_phase: current.desired_enabled ? "guarded" : "disabled",
    route_mode: current.desired_enabled ? "guarded" : "direct_aio",
    last_error: null,
    process_status: current.desired_enabled
      ? {
          phase: "healthy",
          owned: true,
          healthy: true,
          process_id: 4242,
          listener: "http://127.0.0.1:4610",
        }
      : {
          phase: "stopped",
          owned: false,
          healthy: false,
          process_id: null,
          listener: null,
        },
  };
  codexRetryGatewayStatusState = clone(next);
  return getCodexRetryGatewayStatusState();
}

export function buildCodexRetryGatewayUninstallState(
  request: CodexRetryGatewayUninstallRequest | null | undefined
): CodexRetryGatewayStatus {
  const next: CodexRetryGatewayStatus = {
    ...clone(DEFAULT_CODEX_RETRY_GATEWAY_STATUS),
    generation: Math.max(nextCodexRetryGatewayGeneration(), (request?.generation ?? 0) + 1),
  };
  codexRetryGatewayStatusState = clone(next);
  syncCodexGatewayCliProxy({ enabled: false, applied: false });
  return getCodexRetryGatewayStatusState();
}

export function buildCodexRetryGatewayDetailsSessionState(): CodexRetryGatewayDetailsSession {
  const generation = codexRetryGatewayStatusState.generation;
  const port = codexRetryGatewayStatusState.effective_port ?? 4610;
  return {
    generation,
    iframe_url: `http://127.0.0.1:${port}/aio-bridge?session=msw-${generation}`,
    browser_url: `http://127.0.0.1:${port}/`,
    expires_at_ms: Date.now() + 5 * 60 * 1000,
  };
}

// -- Plugins --

function officialPrivacyFilterDetail(): PluginDetail {
  const summary: PluginSummary = {
    id: 1,
    plugin_id: "official.privacy-filter",
    name: "Privacy Filter",
    current_version: "1.0.0",
    status: "disabled",
    runtime: "extensionHost",
    permission_risk: "high",
    update_available: false,
    last_error: null,
    created_at: 1,
    updated_at: 1,
  };

  return {
    summary,
    manifest: {
      id: "official.privacy-filter",
      name: "Privacy Filter",
      version: "1.0.0",
      apiVersion: "1.0.0",
      runtime: { kind: "extensionHost", language: "typescript" },
      main: "dist/extension.js",
      activationEvents: [
        "onGatewayHook:gateway.request.afterBodyRead",
        "onGatewayHook:gateway.request.beforeSend",
        "onGatewayHook:log.beforePersist",
      ],
      capabilities: ["gateway.hooks", "privacy.redact"],
      contributes: {
        gatewayHooks: [
          {
            name: "gateway.request.afterBodyRead",
            priority: 5,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
          {
            name: "gateway.request.beforeSend",
            priority: 5,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
          {
            name: "log.beforePersist",
            priority: 1,
            failurePolicy: "fail-closed",
            timeoutMs: 5000,
          },
        ],
      },
      hostCompatibility: {
        app: ">=0.56.0 <1.0.0",
        pluginApi: "^1.0.0",
        platforms: ["macos", "windows", "linux"],
      },
      configSchema: {
        type: "object",
        required: ["redactBeforeUpstream", "redactLogs", "profile"],
        "x-aio-ui": {
          sections: [
            {
              id: "routing",
              title: "处理位置",
              description: "选择隐私过滤在哪些阶段生效。",
              order: 10,
            },
            {
              id: "content",
              title: "检测策略",
              description:
                "这里展示的是可配置的策略大类；密钥类检测由打包的 200+ Gitleaks 规则、上下文规则和熵检测共同支撑。",
              order: 20,
            },
          ],
        },
        properties: {
          redactBeforeUpstream: {
            type: "boolean",
            title: "发送给模型前处理",
            description: "在请求离开本机前替换你选择的敏感信息。",
            default: true,
            "x-aio-ui": { section: "routing", widget: "switch", order: 10 },
          },
          redactLogs: {
            type: "boolean",
            title: "保存日志前处理",
            description: "在本地日志写入前替换你选择的敏感信息。",
            default: true,
            "x-aio-ui": { section: "routing", widget: "switch", order: 20 },
          },
          profile: {
            type: "string",
            title: "保护强度",
            description:
              "平衡模式会覆盖常见个人信息、200+ Gitleaks 密钥规则、上下文密钥和高熵密钥候选。",
            default: "balanced",
            enum: ["balanced"],
            "x-aio-ui": {
              section: "routing",
              widget: "select",
              order: 30,
              enumLabels: { balanced: "平衡" },
            },
          },
          sensitiveTypes: {
            type: "array",
            title: "策略大类",
            description:
              "这些不是全部底层规则。密钥相关选项会控制打包的 200+ Gitleaks 规则以及上下文/熵检测结果是否生效。",
            default: [
              "email",
              "cn_phone",
              "cn_id_card",
              "bank_card_candidate",
              "ipv4",
              "openai_key",
              "aws_access_key",
              "github_token",
              "google_api_key",
              "slack_token",
              "jwt",
              "private_key",
              "context_secret",
            ],
            items: {
              type: "string",
              enum: [
                "email",
                "cn_phone",
                "cn_id_card",
                "bank_card_candidate",
                "ipv4",
                "openai_key",
                "aws_access_key",
                "github_token",
                "google_api_key",
                "slack_token",
                "jwt",
                "private_key",
                "context_secret",
              ],
              "x-aio-ui": {
                enumLabels: {
                  email: "邮箱地址",
                  cn_phone: "中国手机号",
                  cn_id_card: "身份证号",
                  bank_card_candidate: "银行卡号",
                  ipv4: "IP 地址",
                  openai_key: "OpenAI Key",
                  aws_access_key: "AWS Access Key",
                  github_token: "GitHub Token",
                  google_api_key: "Google API Key",
                  slack_token: "Slack Token",
                  jwt: "JWT",
                  private_key: "私钥片段",
                  context_secret: "上下文密钥",
                },
                enumDescriptions: {
                  email: "例如 name@example.com。",
                  cn_phone: "例如 13344441520。",
                  cn_id_card: "中国大陆居民身份证号码。",
                  bank_card_candidate: "通过校验规则识别常见银行卡号。",
                  ipv4: "例如 192.168.1.10。",
                  openai_key: "常见 sk- 开头的 OpenAI 密钥。",
                  aws_access_key: "常见 AKIA 开头的访问密钥。",
                  github_token: "ghp、github_pat 等令牌。",
                  google_api_key: "常见 AIza 开头的 Google API Key。",
                  slack_token: "Slack bot、user、app token。",
                  jwt: "常见 JSON Web Token。",
                  private_key: "PEM 私钥内容。",
                  context_secret: "password、api_key、token 等上下文中的敏感值。",
                },
              },
            },
            "x-aio-ui": {
              section: "content",
              widget: "checkboxGroup",
              order: 10,
              warningWhenPartial: "关闭后，这类内容会原样发送给模型，也可能出现在本地日志中。",
            },
          },
        },
      },
      description: "Official privacy filter for PII and secrets.",
      homepage: "https://github.com/packyme/privacy-filter",
    },
    install_source: "official",
    installed_dir: null,
    config: {
      redactBeforeUpstream: true,
      redactLogs: true,
      profile: "balanced",
      sensitiveTypes: [
        "email",
        "cn_phone",
        "cn_id_card",
        "bank_card_candidate",
        "ipv4",
        "openai_key",
        "aws_access_key",
        "github_token",
        "google_api_key",
        "slack_token",
        "jwt",
        "private_key",
        "context_secret",
      ],
    },
    granted_permissions: ["request.body.read", "request.body.write", "log.redact"],
    pending_permissions: [],
    audit_logs: [
      {
        id: 1,
        plugin_id: "official.privacy-filter",
        trace_id: null,
        event_type: "plugin.installed",
        risk_level: "low",
        message: "Plugin installed",
        details: { source: "official" },
        created_at: 1,
      },
    ],
    runtime_failures: [],
    rollback_versions: [],
  };
}

export function getPluginSummariesState(): PluginSummary[] {
  return Array.from(pluginState.values()).map((detail) => clone(detail.summary));
}

export function getPluginDetailState(pluginId: string): PluginDetail | null {
  return pluginState.has(pluginId) ? clone(pluginState.get(pluginId)!) : null;
}

export function installOfficialPluginState(pluginId: string): PluginDetail {
  if (pluginId !== "official.privacy-filter") {
    throw new Error(`unknown official plugin: ${pluginId}`);
  }
  const detail = officialPrivacyFilterDetail();
  pluginState.set(pluginId, clone(detail));
  return clone(detail);
}

// -- Providers --

export function getProvidersState(cliKey: CliKey): ProviderSummary[] {
  return clone(providersState.get(cliKey) ?? []);
}

export function setProvidersState(cliKey: CliKey, next: ProviderSummary[]) {
  providersState.set(cliKey, clone(next));
}

// -- Usage --

export function getUsageSummaryState(): UsageSummary {
  return clone(usageSummaryState);
}

// -- App About --

export function getAppAboutState(): AppAboutInfo {
  return clone(appAboutState);
}

// -- DB Disk Usage --

export function getDbDiskUsageState(): DbDiskUsage {
  return clone(dbDiskUsageState);
}

// -- Sort Modes --

export function getSortModesState(): SortModeSummary[] {
  return clone(sortModesState);
}

export function getSortModeActiveState(): SortModeActiveRow[] {
  return clone(sortModeActiveState);
}

// -- Workspaces --

export function getWorkspacesState(cliKey: CliKey): WorkspacesListResult {
  return clone(workspacesState.get(cliKey) ?? { active_id: null, items: [] });
}

function setCliProxyEnabledState(cliKey: CliKey, enabled: boolean): CliProxyStatus[] {
  const rowIndex = cliProxyStatusAllState.findIndex((row) => row.cli_key === cliKey);
  const baseOrigin = enabled ? DEFAULT_BASE_ORIGIN : null;
  if (rowIndex < 0) {
    cliProxyStatusAllState = [
      createCliProxyStatus({
        cli_key: cliKey,
        enabled,
        base_origin: baseOrigin,
        applied_to_current_gateway: enabled ? true : null,
      }),
      ...cliProxyStatusAllState,
    ];
    return getCliProxyStatusAllState();
  }

  const next = clone(cliProxyStatusAllState);
  next[rowIndex] = {
    ...next[rowIndex],
    enabled,
    base_origin: baseOrigin,
    applied_to_current_gateway: enabled ? true : null,
  };
  cliProxyStatusAllState = next;
  return getCliProxyStatusAllState();
}

export function buildCliProxySetEnabledResult(input: {
  cli_key: string;
  enabled: boolean;
}): CliProxyResult {
  const cliKey = input.cli_key;
  const enabled = input.enabled;

  if (cliKey !== "claude" && cliKey !== "codex" && cliKey !== "gemini") {
    return {
      trace_id: nextTraceId(),
      cli_key: cliKey as CliKey,
      enabled,
      ok: false,
      error_code: "UNSUPPORTED_CLI",
      message: `unsupported cli_key: ${cliKey}`,
      base_origin: null,
    };
  }

  const cli_key = cliKey as CliKey;
  const base_origin = enabled ? DEFAULT_BASE_ORIGIN : null;
  setCliProxyEnabledState(cli_key, enabled);

  if (cli_key === "codex") {
    codexRetryGatewayStatusState = {
      ...codexRetryGatewayStatusState,
      generation: nextCodexRetryGatewayGeneration(),
      cli_proxy_enabled: enabled,
      cli_proxy_applied: enabled && codexRetryGatewayStatusState.desired_enabled,
      route_mode: enabled
        ? codexRetryGatewayStatusState.desired_enabled
          ? "guarded"
          : "direct_aio"
        : "unproxied",
      runtime_phase:
        codexRetryGatewayStatusState.desired_enabled && enabled
          ? "guarded"
          : codexRetryGatewayStatusState.desired_enabled
            ? "recovery_paused"
            : "disabled",
    };
  }

  return {
    trace_id: nextTraceId(),
    cli_key,
    enabled,
    ok: true,
    error_code: null,
    message: "",
    base_origin,
  };
}
