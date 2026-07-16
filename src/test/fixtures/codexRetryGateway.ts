import type {
  CodexRetryGatewayDetailsSession,
  CodexRetryGatewayEnablePlan,
  CodexRetryGatewayErrorCategory,
  CodexRetryGatewayNodeResolutionSource,
  CodexRetryGatewayProcessPhase,
  CodexRetryGatewayRuntimePhase,
  CodexRetryGatewayStatus,
  CodexRetryGatewayTrustState,
  CodexRouteMode,
} from "../../generated/bindings";

function defineAllEnumValues<TValue extends string>() {
  return <const TValues extends readonly TValue[]>(
    values: Exclude<TValue, TValues[number]> extends never ? TValues : never
  ) => values;
}

export const CODEX_RETRY_GATEWAY_RUNTIME_PHASES =
  defineAllEnumValues<CodexRetryGatewayRuntimePhase>()([
    "disabled",
    "preparing",
    "starting",
    "guarded",
    "bypassed_recovering",
    "recovery_paused",
    "updating",
    "stopping",
    "cleanup_needed",
    "uninstalling",
    "error",
  ]);

export const CODEX_RETRY_GATEWAY_ROUTE_MODES = defineAllEnumValues<CodexRouteMode>()([
  "unproxied",
  "direct_aio",
  "guarded",
]);

export const CODEX_RETRY_GATEWAY_TRUST_STATES = defineAllEnumValues<CodexRetryGatewayTrustState>()([
  "unavailable",
  "aio_reviewed_recommendation",
  "official_main_unreviewed",
]);

export const CODEX_RETRY_GATEWAY_NODE_SOURCES =
  defineAllEnumValues<CodexRetryGatewayNodeResolutionSource>()([
    "unavailable",
    "codex_sibling",
    "aio_discovery",
    "process_path",
    "manual_override",
  ]);

export const CODEX_RETRY_GATEWAY_PROCESS_PHASES =
  defineAllEnumValues<CodexRetryGatewayProcessPhase>()([
    "stopped",
    "starting",
    "healthy",
    "unhealthy",
    "ownership_mismatch",
  ]);

export const CODEX_RETRY_GATEWAY_ERROR_CATEGORIES =
  defineAllEnumValues<CodexRetryGatewayErrorCategory>()([
    "source_resolution",
    "source_archive",
    "node_missing",
    "node_unsupported",
    "port_conflict",
    "ownership_mismatch",
    "health_timeout",
    "route_apply",
    "route_verify",
    "provider_sync",
    "bridge_session",
    "update_rollback",
    "cleanup",
    "internal",
  ]);

export const CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT = "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2";
export const CODEX_RETRY_GATEWAY_SELECTED_COMMIT = "f12a3b4567890cdef1234567890abcdef1234567";
export const CODEX_RETRY_GATEWAY_PREVIOUS_COMMIT = "0a1b2c3d4e5f678901234567890abcdef1234567";
export const CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT = "abcdef1234567890abcdef1234567890abcdef12";

export function createCodexRetryGatewayStatus(
  overrides: Partial<CodexRetryGatewayStatus> = {}
): CodexRetryGatewayStatus {
  return {
    generation: 7,
    desired_enabled: true,
    runtime_phase: "guarded",
    route_mode: "guarded",
    cli_proxy_enabled: true,
    cli_proxy_applied: true,
    effective_port: 4610,
    repository: "nonononull/codex-retry-gateway",
    license: null,
    selected_commit: CODEX_RETRY_GATEWAY_SELECTED_COMMIT,
    active_commit: CODEX_RETRY_GATEWAY_SELECTED_COMMIT,
    previous_commit: CODEX_RETRY_GATEWAY_PREVIOUS_COMMIT,
    recommended_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT,
    trust_state: "aio_reviewed_recommendation",
    node_status: {
      available: true,
      executable: "C:\\Program Files\\nodejs\\node.exe",
      version: "v22.15.0",
      source: "aio_discovery",
      error: null,
    },
    process_status: {
      phase: "healthy",
      owned: true,
      healthy: true,
      process_id: 47211,
      listener: "http://127.0.0.1:4610",
    },
    update_candidate: null,
    wsl_codex_unprotected: true,
    last_error: null,
    details_available: true,
    operation_pending: false,
    ...overrides,
  };
}

export function createCodexRetryGatewayEnablePlan(
  overrides: Partial<CodexRetryGatewayEnablePlan> = {}
): CodexRetryGatewayEnablePlan {
  return {
    generation: 41,
    selected_commit: CODEX_RETRY_GATEWAY_CANDIDATE_COMMIT,
    trust_state: "official_main_unreviewed",
    first_download_required: true,
    unreviewed_commit: true,
    cli_proxy_enable_required: true,
    provider_sync: {
      current_provider: "OpenAI",
      target_provider: "aio",
      change_required: true,
      codex_must_be_closed: true,
    },
    node_status: {
      available: true,
      executable: "C:\\Program Files\\nodejs\\node.exe",
      version: "v22.15.0",
      source: "aio_discovery",
      error: null,
    },
    preferred_port: 4610,
    wsl_codex_unprotected: true,
    ...overrides,
  };
}

export function createCodexRetryGatewayDetailsSession(
  suffix = "first",
  iframeViewId = "a".repeat(32)
): CodexRetryGatewayDetailsSession {
  return {
    generation: 7,
    iframe_url: `http://127.0.0.1:4610/aio-bridge?session=${suffix}`,
    browser_url: `http://127.0.0.1:4610/aio-browser?session=${suffix}`,
    iframe_view_id: iframeViewId,
    expires_at_ms: 2_000_000_000_000,
  };
}
