import type {
  CodexReasoningGuardCompareMode,
  CodexReasoningGuardExhaustedAction,
  CodexReasoningGuardModelRule,
  GatewayListenMode,
  SensitiveStringUpdate,
  UpstreamRetryPolicy,
  WslHostAddressMode,
} from "../../generated/bindings";

export const MAX_UPDATE_RELEASES_URL_LEN = 2048;
export const MAX_UPSTREAM_PROXY_URL_LEN = 2048;
export const MAX_UPSTREAM_PROXY_USERNAME_LEN = 256;
export const MAX_UPSTREAM_PROXY_PASSWORD_LEN = 4096;
export const MAX_CX2CC_MODEL_NAME_LEN = 128;
export const MAX_CX2CC_OPTIONAL_FIELD_LEN = 64;
export const MAX_CODEX_PROVIDER_TEST_MODEL_NAME_LEN = 128;
export const MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN = 32;
export const MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN = 32;
export const MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN = 128;
export const MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE = 1_000_000_000;
export const DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS = [516, 1034, 1552] as const;
export const DEFAULT_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS = 5;
export const DEFAULT_CODEX_REASONING_GUARD_BACKOFF_MS = 1_000;
export const DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET = 5;
export const DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET = 5;
export const DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS = 1_000;
export const DEFAULT_CODEX_REASONING_GUARD_EXHAUSTED_ACTION: CodexReasoningGuardExhaustedAction =
  "return_error";
export const MAX_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS = 100;
export const MAX_CODEX_REASONING_GUARD_BACKOFF_MS = 60_000;
export const MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET = 100;
export const MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET = 100;
export const MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS = 60_000;
export const MIN_PREFERRED_PORT = 1024;
export const MAX_PREFERRED_PORT = 65535;
export const MIN_LOG_RETENTION_DAYS = 1;
export const MAX_LOG_RETENTION_DAYS = 3650;
export const MAX_PROVIDER_COOLDOWN_SECONDS = 60 * 60;
export const MIN_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS = 1;
export const MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS = 60 * 60;
export const MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS = 60 * 60;
export const MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS = 60;
export const MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS = 60 * 60;
export const MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS = 24 * 60 * 60;
export const MIN_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER = 1;
export const MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER = 20;
export const MIN_FAILOVER_MAX_PROVIDERS_TO_TRY = 1;
export const MAX_FAILOVER_MAX_PROVIDERS_TO_TRY = 20;
export const MAX_FAILOVER_TOTAL_ATTEMPTS = 100;
export const MAX_UPSTREAM_RETRY_STATUS_CODES = 16;
export const MAX_UPSTREAM_RETRY_MAX_RETRIES = 10;
export const MAX_UPSTREAM_RETRY_BACKOFF_MS = 60_000;
export const MIN_CIRCUIT_BREAKER_FAILURE_THRESHOLD = 1;
export const MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD = 50;
export const MIN_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES = 1;
export const MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES = 24 * 60;

const CONTROL_CHAR_PATTERN = /[\u0000-\u001f\u007f-\u009f]/u;
const DECIMAL_INTEGER_PATTERN = /^\d+$/u;
const SUPPORTED_PROXY_SCHEMES = new Set(["http", "https", "socks5", "socks5h"]);
const SUPPORTED_UPSTREAM_RETRY_TRANSPORT_ERRORS = new Set(["connect", "timeout", "read"]);

export type ParsedCustomListenAddress = {
  host: string;
  port: number | null;
};

type ListenAddressParseResult =
  | { ok: true; value: ParsedCustomListenAddress }
  | { ok: false; reason: "format" | "low_port" };

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

function parsePort(raw: string): number | null {
  const trimmed = raw.trim();
  if (!DECIMAL_INTEGER_PATTERN.test(trimmed)) return null;
  const port = Number(trimmed);
  if (!Number.isInteger(port) || port > 65535) return null;
  return port;
}

function parseCustomListenAddressDetailed(input: string): ListenAddressParseResult {
  const raw = input.trim();
  if (!raw) {
    return { ok: true, value: { host: "0.0.0.0", port: null } };
  }
  if (raw.includes("://") || raw.includes("/")) {
    return { ok: false, reason: "format" };
  }

  if (raw.startsWith("[")) {
    const idx = raw.indexOf("]");
    if (idx < 0) return { ok: false, reason: "format" };

    const host = raw.slice(1, idx).trim();
    if (!host) return { ok: false, reason: "format" };

    const tail = raw.slice(idx + 1).trim();
    if (!tail) return { ok: true, value: { host, port: null } };
    if (!tail.startsWith(":")) return { ok: false, reason: "format" };

    const port = parsePort(tail.slice(1));
    if (port == null) return { ok: false, reason: "format" };
    if (port < 1024) return { ok: false, reason: "low_port" };
    return { ok: true, value: { host, port } };
  }

  const parts = raw.split(":");
  if (parts.length === 1) {
    return { ok: true, value: { host: raw, port: null } };
  }
  if (parts.length === 2) {
    const host = parts[0]?.trim() ?? "";
    if (!host) return { ok: false, reason: "format" };

    const port = parsePort(parts[1] ?? "");
    if (port == null) return { ok: false, reason: "format" };
    if (port < 1024) return { ok: false, reason: "low_port" };
    return { ok: true, value: { host, port } };
  }

  return { ok: false, reason: "format" };
}

export function parseCustomListenAddress(input: string): ParsedCustomListenAddress | null {
  const parsed = parseCustomListenAddressDetailed(input);
  return parsed.ok ? parsed.value : null;
}

export function formatHostPort(host: string, port: number): string {
  return host.includes(":") ? `[${host}]:${port}` : `${host}:${port}`;
}

export function validateGatewayCustomListenAddress(input: string): string | null {
  const parsed = parseCustomListenAddressDetailed(input);
  if (parsed.ok) return null;
  if (parsed.reason === "low_port") return "端口必须 >= 1024";
  return "自定义地址仅支持 host 或 host:port（IPv6 请使用 [addr]:port）";
}

function isValidBareIpv6Host(value: string): boolean {
  try {
    const parsed = new URL(`http://[${value}]/`);
    return parsed.hostname.length > 0;
  } catch {
    return false;
  }
}

export function parseCustomHostAddress(input: string): string | null {
  const raw = input.trim();
  if (!raw) return null;
  if (raw.includes("://") || raw.includes("/") || raw.includes("\\")) return null;

  if (raw.startsWith("[")) {
    const idx = raw.indexOf("]");
    if (idx < 0) return null;
    const host = raw.slice(1, idx).trim();
    if (!host) return null;
    const tail = raw.slice(idx + 1).trim();
    if (tail) return null;
    return host;
  }

  if (raw.includes("[") || raw.includes("]")) return null;
  if (raw.includes(":") && !isValidBareIpv6Host(raw)) return null;

  return raw;
}

export function validateWslCustomHostAddress(input: string): string | null {
  const raw = input.trim();
  if (!raw) return null;
  if (raw.includes("://") || raw.includes("/") || raw.includes("\\")) {
    return "宿主机地址仅支持 host/IP（不要包含协议或路径）";
  }
  if (raw.startsWith("[") && raw.indexOf("]") < 0) {
    return "IPv6 宿主机地址缺少右方括号";
  }
  if (raw.includes("[") || raw.includes("]")) {
    return parseCustomHostAddress(raw) ? null : "IPv6 宿主机地址请使用 [addr]，且不要包含端口";
  }
  if (raw.includes(":") && !isValidBareIpv6Host(raw)) {
    return "宿主机地址不支持端口；请只填写 host/IP（IPv6 可直接填写 ::1）";
  }
  return parseCustomHostAddress(raw) ? null : "宿主机地址仅支持 host/IP";
}

function parseUrl(value: string): URL | null {
  try {
    return new URL(value);
  } catch {
    return null;
  }
}

export function validateUpdateReleasesUrl(value: string): string | null {
  const raw = value.trim();
  if (!raw) return null;
  if (utf8Length(raw) > MAX_UPDATE_RELEASES_URL_LEN) {
    return `更新地址必须 <= ${MAX_UPDATE_RELEASES_URL_LEN} 字符`;
  }

  const parsed = parseUrl(raw);
  if (!parsed) return "更新地址不是有效 URL";
  if (!["http:", "https:"].includes(parsed.protocol)) {
    return "更新地址仅支持 http 或 https";
  }
  if (!parsed.hostname) return "更新地址必须包含 host";
  if (parsed.username || parsed.password) return "更新地址不能包含用户名或密码";
  return null;
}

type UpstreamProxyValidationInput = {
  enabled?: boolean | null;
  requireUrl?: boolean;
  validateUrlWhenPresent?: boolean;
  url?: string | null;
  username?: string | null;
  password?: string | null;
  passwordUpdate?: SensitiveStringUpdate | null;
};

function resolveProxyPasswordValue(input: UpstreamProxyValidationInput): string {
  if (input.passwordUpdate?.mode === "replace") return input.passwordUpdate.value;
  return input.password ?? "";
}

export function validateUpstreamProxyFields(input: UpstreamProxyValidationInput): string | null {
  const rawUrl = input.url ?? "";
  const url = rawUrl.trim();
  const username = (input.username ?? "").trim();
  const password = resolveProxyPasswordValue(input);

  if (utf8Length(url) > MAX_UPSTREAM_PROXY_URL_LEN) {
    return `代理地址必须 <= ${MAX_UPSTREAM_PROXY_URL_LEN} 字符`;
  }
  if (utf8Length(username) > MAX_UPSTREAM_PROXY_USERNAME_LEN) {
    return `代理用户名必须 <= ${MAX_UPSTREAM_PROXY_USERNAME_LEN} 字符`;
  }
  if (utf8Length(password) > MAX_UPSTREAM_PROXY_PASSWORD_LEN) {
    return `代理密码必须 <= ${MAX_UPSTREAM_PROXY_PASSWORD_LEN} 字符`;
  }

  const needsUrl = input.enabled === true || input.requireUrl === true;
  if (needsUrl && !url) return "代理地址不能为空";

  const hasSeparateCredentials = Boolean(username) || password.length > 0;
  if (password.length > 0 && !username) return "填写代理密码时也需要填写用户名";

  if (!url) return null;
  if (!needsUrl && !input.validateUrlWhenPresent && !hasSeparateCredentials) return null;

  const parsed = parseUrl(url);
  if (!parsed) return "代理地址不是有效 URL";

  const scheme = parsed.protocol.replace(/:$/u, "");
  if (!SUPPORTED_PROXY_SCHEMES.has(scheme)) {
    return "代理地址协议仅支持 http/https/socks5/socks5h";
  }

  const urlHasCredentials = Boolean(parsed.username) || Boolean(parsed.password);
  if (urlHasCredentials && hasSeparateCredentials) {
    return "代理认证信息不要同时写在 URL 和用户名/密码里";
  }

  return null;
}

function validateNoControlChars(fieldLabel: string, value: string): string | null {
  return CONTROL_CHAR_PATTERN.test(value) ? `${fieldLabel}不能包含控制字符` : null;
}

export function validateCx2ccFallbackModel(fieldLabel: string, value: string): string | null {
  const raw = value.trim();
  if (!raw) return `${fieldLabel}不能为空`;
  if (utf8Length(raw) > MAX_CX2CC_MODEL_NAME_LEN) {
    return `${fieldLabel}必须 <= ${MAX_CX2CC_MODEL_NAME_LEN} 字符`;
  }
  return validateNoControlChars(fieldLabel, raw);
}

function validateCodexProviderTestModel(fieldLabel: string, value: string): string | null {
  const raw = value.trim();
  if (!raw) return `${fieldLabel}不能为空`;
  if (utf8Length(raw) > MAX_CODEX_PROVIDER_TEST_MODEL_NAME_LEN) {
    return `${fieldLabel}必须 <= ${MAX_CODEX_PROVIDER_TEST_MODEL_NAME_LEN} 字符`;
  }
  return validateNoControlChars(fieldLabel, raw);
}

export function validateCx2ccOptionalField(fieldLabel: string, value: string): string | null {
  const raw = value.trim();
  if (!raw) return null;
  if (utf8Length(raw) > MAX_CX2CC_OPTIONAL_FIELD_LEN) {
    return `${fieldLabel}必须 <= ${MAX_CX2CC_OPTIONAL_FIELD_LEN} 字符`;
  }
  return validateNoControlChars(fieldLabel, raw);
}

function validateIntegerRange(
  fieldLabel: string,
  value: number | null | undefined,
  min: number,
  max: number
): string | null {
  if (value == null) return null;
  if (!Number.isSafeInteger(value)) return `${fieldLabel}必须是整数`;
  if (value < min) return `${fieldLabel}必须 >= ${min}`;
  if (value > max) return `${fieldLabel}必须 <= ${max}`;
  return null;
}

function validateUpstreamStreamIdleTimeout(value: number | null | undefined): string | null {
  if (value == null) return null;
  if (!Number.isSafeInteger(value)) return "流式空闲超时必须是整数";
  if (value > MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS) {
    return `流式空闲超时必须 <= ${MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS}`;
  }
  if (value > 0 && value < MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS) {
    return `流式空闲超时必须为 0（禁用）或 >= ${MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS}`;
  }
  return null;
}

function validateUpstreamRetryPolicy(policy: UpstreamRetryPolicy | null | undefined) {
  if (policy == null) return null;
  if (!Array.isArray(policy.status_codes)) return "瞬时错误重试 HTTP 状态码必须是列表";
  if (policy.status_codes.length > MAX_UPSTREAM_RETRY_STATUS_CODES) {
    return `瞬时错误重试 HTTP 状态码最多支持 ${MAX_UPSTREAM_RETRY_STATUS_CODES} 个`;
  }
  for (const statusCode of policy.status_codes) {
    if (!Number.isSafeInteger(statusCode)) return "瞬时错误重试 HTTP 状态码必须是整数";
    if (statusCode < 400 || statusCode > 599) return "瞬时错误重试 HTTP 状态码必须在 400-599";
  }
  if (!Array.isArray(policy.transport_errors)) return "瞬时错误重试传输错误必须是列表";
  for (const kind of policy.transport_errors) {
    if (!SUPPORTED_UPSTREAM_RETRY_TRANSPORT_ERRORS.has(kind)) {
      return "瞬时错误重试传输错误仅支持 connect、timeout、read";
    }
  }
  if (
    !Number.isSafeInteger(policy.max_retries) ||
    policy.max_retries < 0 ||
    policy.max_retries > MAX_UPSTREAM_RETRY_MAX_RETRIES
  ) {
    return `瞬时错误重试次数必须为 0-${MAX_UPSTREAM_RETRY_MAX_RETRIES}`;
  }
  if (
    !Number.isSafeInteger(policy.backoff_ms) ||
    policy.backoff_ms < 0 ||
    policy.backoff_ms > MAX_UPSTREAM_RETRY_BACKOFF_MS
  ) {
    return `重试间隔必须为 0-${MAX_UPSTREAM_RETRY_BACKOFF_MS} 毫秒`;
  }
  if (policy.enabled && policy.status_codes.length === 0 && policy.transport_errors.length === 0) {
    return "启用重试时至少选择一个 HTTP 状态码或传输错误";
  }
  return null;
}

export type SettingsSetValidationInput = {
  preferredPort?: number | null;
  logRetentionDays?: number | null;
  providerCooldownSeconds?: number | null;
  providerBaseUrlPingCacheTtlSeconds?: number | null;
  upstreamFirstByteTimeoutSeconds?: number | null;
  upstreamStreamIdleTimeoutSeconds?: number | null;
  upstreamRequestTimeoutNonStreamingSeconds?: number | null;
  failoverMaxAttemptsPerProvider?: number | null;
  failoverMaxProvidersToTry?: number | null;
  upstreamRetryPolicy?: UpstreamRetryPolicy | null;
  circuitBreakerFailureThreshold?: number | null;
  circuitBreakerOpenDurationMinutes?: number | null;
  gatewayListenMode?: GatewayListenMode | null;
  gatewayCustomListenAddress?: string | null;
  wslHostAddressMode?: WslHostAddressMode | null;
  wslCustomHostAddress?: string | null;
  updateReleasesUrl?: string | null;
  upstreamProxyEnabled?: boolean | null;
  upstreamProxyUrl?: string | null;
  upstreamProxyUsername?: string | null;
  upstreamProxyPassword?: SensitiveStringUpdate | null;
  cx2CcFallbackModelOpus?: string | null;
  cx2CcFallbackModelSonnet?: string | null;
  cx2CcFallbackModelHaiku?: string | null;
  cx2CcFallbackModelMain?: string | null;
  cx2CcModelReasoningEffort?: string | null;
  cx2CcServiceTier?: string | null;
  codexProviderTestModel?: string | null;
  codexReasoningGuardReasoningEquals?: number[] | null;
  codexReasoningGuardCompareMode?: CodexReasoningGuardCompareMode | null;
  codexReasoningGuardModelRules?: CodexReasoningGuardModelRule[] | null;
  codexReasoningGuardImmediateRetryBudget?: number | null;
  codexReasoningGuardDelayedRetryBudget?: number | null;
  codexReasoningGuardDelayedRetryMs?: number | null;
  codexReasoningGuardExhaustedAction?: CodexReasoningGuardExhaustedAction | null;
  codexReasoningGuardBackoffAfterHits?: number | null;
  codexReasoningGuardBackoffMs?: number | null;
};

export function validateSettingsSetInput(input: SettingsSetValidationInput): string | null {
  for (const [fieldLabel, value, min, max] of [
    ["首选端口", input.preferredPort, MIN_PREFERRED_PORT, MAX_PREFERRED_PORT],
    ["日志保留天数", input.logRetentionDays, MIN_LOG_RETENTION_DAYS, MAX_LOG_RETENTION_DAYS],
    ["Provider 冷却时间", input.providerCooldownSeconds, 0, MAX_PROVIDER_COOLDOWN_SECONDS],
    [
      "Provider Base URL 探测缓存 TTL",
      input.providerBaseUrlPingCacheTtlSeconds,
      MIN_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS,
      MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS,
    ],
    [
      "首字节超时",
      input.upstreamFirstByteTimeoutSeconds,
      0,
      MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS,
    ],
    [
      "非流式请求超时",
      input.upstreamRequestTimeoutNonStreamingSeconds,
      0,
      MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS,
    ],
    [
      "单 Provider 最大重试次数",
      input.failoverMaxAttemptsPerProvider,
      MIN_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER,
      MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER,
    ],
    [
      "最大尝试 Provider 数",
      input.failoverMaxProvidersToTry,
      MIN_FAILOVER_MAX_PROVIDERS_TO_TRY,
      MAX_FAILOVER_MAX_PROVIDERS_TO_TRY,
    ],
    [
      "熔断失败阈值",
      input.circuitBreakerFailureThreshold,
      MIN_CIRCUIT_BREAKER_FAILURE_THRESHOLD,
      MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD,
    ],
    [
      "熔断打开时长",
      input.circuitBreakerOpenDurationMinutes,
      MIN_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES,
      MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES,
    ],
  ] as const) {
    const message = validateIntegerRange(fieldLabel, value, min, max);
    if (message) return message;
  }

  const streamIdleMessage = validateUpstreamStreamIdleTimeout(
    input.upstreamStreamIdleTimeoutSeconds
  );
  if (streamIdleMessage) return streamIdleMessage;

  if (
    input.failoverMaxAttemptsPerProvider != null &&
    input.failoverMaxProvidersToTry != null &&
    Number.isSafeInteger(input.failoverMaxAttemptsPerProvider) &&
    Number.isSafeInteger(input.failoverMaxProvidersToTry) &&
    input.failoverMaxAttemptsPerProvider * input.failoverMaxProvidersToTry >
      MAX_FAILOVER_TOTAL_ATTEMPTS
  ) {
    return `Failover 总尝试次数必须 <= ${MAX_FAILOVER_TOTAL_ATTEMPTS}`;
  }

  const upstreamRetryPolicyMessage = validateUpstreamRetryPolicy(input.upstreamRetryPolicy);
  if (upstreamRetryPolicyMessage) return upstreamRetryPolicyMessage;

  if (input.gatewayListenMode === "custom" && input.gatewayCustomListenAddress != null) {
    const message = validateGatewayCustomListenAddress(input.gatewayCustomListenAddress);
    if (message) return message;
  }

  if (input.wslHostAddressMode === "custom" && input.wslCustomHostAddress != null) {
    const message = validateWslCustomHostAddress(input.wslCustomHostAddress);
    if (message) return message;
  }

  if (input.updateReleasesUrl != null) {
    const message = validateUpdateReleasesUrl(input.updateReleasesUrl);
    if (message) return message;
  }

  const proxyMessage = validateUpstreamProxyFields({
    enabled: input.upstreamProxyEnabled,
    url: input.upstreamProxyUrl,
    username: input.upstreamProxyUsername,
    passwordUpdate: input.upstreamProxyPassword,
  });
  if (proxyMessage) return proxyMessage;

  for (const [fieldLabel, value] of [
    ["CX2CC Opus 默认模型", input.cx2CcFallbackModelOpus],
    ["CX2CC Sonnet 默认模型", input.cx2CcFallbackModelSonnet],
    ["CX2CC Haiku 默认模型", input.cx2CcFallbackModelHaiku],
    ["CX2CC 主模型默认", input.cx2CcFallbackModelMain],
  ] as const) {
    if (value == null) continue;
    const message = validateCx2ccFallbackModel(fieldLabel, value);
    if (message) return message;
  }

  for (const [fieldLabel, value] of [
    ["CX2CC 推理强度", input.cx2CcModelReasoningEffort],
    ["CX2CC 服务层级", input.cx2CcServiceTier],
  ] as const) {
    if (value == null) continue;
    const message = validateCx2ccOptionalField(fieldLabel, value);
    if (message) return message;
  }

  if (input.codexProviderTestModel != null) {
    const message = validateCodexProviderTestModel(
      "Codex 全局测试模型",
      input.codexProviderTestModel
    );
    if (message) return message;
  }

  if (input.codexReasoningGuardReasoningEquals != null) {
    const values = input.codexReasoningGuardReasoningEquals;
    if (!Array.isArray(values) || values.length === 0) {
      return "Codex 降智拦截规则至少需要一个 reasoning_tokens 值";
    }
    if (values.length > MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN) {
      return `Codex 降智拦截规则最多支持 ${MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN} 个值`;
    }
    for (const value of values) {
      if (!Number.isSafeInteger(value)) {
        return "Codex 降智拦截规则必须是整数列表";
      }
      if (value < 0 || value > MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE) {
        return `Codex 降智拦截值必须在 0 到 ${MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE} 之间`;
      }
    }
  }

  if (input.codexReasoningGuardCompareMode != null) {
    if (
      input.codexReasoningGuardCompareMode !== "equals" &&
      input.codexReasoningGuardCompareMode !== "less_than_or_equal"
    ) {
      return "Codex 降智拦截比较模式仅支持 equals 或 less_than_or_equal";
    }
  }

  if (input.codexReasoningGuardExhaustedAction != null) {
    if (
      input.codexReasoningGuardExhaustedAction !== "return_error" &&
      input.codexReasoningGuardExhaustedAction !== "switch_provider"
    ) {
      return "Codex 降智拦截预算耗尽动作仅支持 return_error 或 switch_provider";
    }
  }

  for (const [fieldLabel, value, max] of [
    [
      "Codex 降智拦截立即重试预算",
      input.codexReasoningGuardImmediateRetryBudget,
      MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET,
    ],
    [
      "Codex 降智拦截等待重试预算",
      input.codexReasoningGuardDelayedRetryBudget,
      MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET,
    ],
    [
      "Codex 降智拦截等待时间",
      input.codexReasoningGuardDelayedRetryMs,
      MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS,
    ],
    [
      "Codex 降智拦截等待触发次数",
      input.codexReasoningGuardBackoffAfterHits,
      MAX_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS,
    ],
    [
      "Codex 降智拦截等待时间",
      input.codexReasoningGuardBackoffMs,
      MAX_CODEX_REASONING_GUARD_BACKOFF_MS,
    ],
  ] as const) {
    if (value == null) continue;
    if (!Number.isSafeInteger(value) || value < 0 || value > max) {
      return `${fieldLabel}必须为 0-${max} 的整数`;
    }
  }

  if (input.codexReasoningGuardModelRules != null) {
    const rules = input.codexReasoningGuardModelRules;
    if (!Array.isArray(rules)) {
      return "Codex 模型规则必须是列表";
    }
    if (rules.length > MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN) {
      return `Codex 模型规则最多支持 ${MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN} 条`;
    }

    const seenModels = new Set<string>();
    for (const rule of rules) {
      const requestedModel = rule?.requested_model?.trim() ?? "";
      if (!requestedModel) {
        return "Codex 模型规则必须填写模型名";
      }
      if (utf8Length(requestedModel) > MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN) {
        return `Codex 模型名必须 <= ${MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN} 字符`;
      }
      if (CONTROL_CHAR_PATTERN.test(requestedModel)) {
        return "Codex 模型名不能包含控制字符";
      }
      if (seenModels.has(requestedModel)) {
        return `Codex 模型规则不能重复：${requestedModel}`;
      }
      seenModels.add(requestedModel);

      const compareMode = rule.compare_mode ?? "equals";
      if (compareMode !== "equals" && compareMode !== "less_than_or_equal") {
        return "Codex 模型规则比较模式仅支持 equals 或 less_than_or_equal";
      }

      const values = rule.reasoning_equals;
      if (!Array.isArray(values) || values.length === 0) {
        return `Codex 模型规则 ${requestedModel} 至少需要一个 reasoning_tokens 值`;
      }
      if (values.length > MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN) {
        return `Codex 模型规则 ${requestedModel} 最多支持 ${MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN} 个值`;
      }
      for (const value of values) {
        if (!Number.isSafeInteger(value)) {
          return `Codex 模型规则 ${requestedModel} 必须是整数列表`;
        }
        if (value < 0 || value > MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE) {
          return `Codex 模型规则 ${requestedModel} 的值必须在 0 到 ${MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE} 之间`;
        }
      }
    }
  }

  return null;
}
