import type {
  CodexReasoningGuardCompareMode,
  CodexReasoningGuardExhaustedAction,
  CodexReasoningGuardModelRule,
  CodexReasoningGuardPostMatchStrategy,
  CodexReasoningGuardRetryPolicy,
  CodexReasoningGuardRuleTemplate,
  CodexReasoningGuardRuleMode,
  CodexReasoningGuardTemplateFilter,
  CodexReasoningGuardTemplateFilterField,
  CodexReasoningGuardTemplateFilterOperator,
  CodexReasoningGuardTemplateRuleAction,
  CodexReasoningGuardTemplateRuleFormula,
  CodexReasoningGuardTemplateRuleLogic,
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
export const MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES = 32;
export const MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES = 1056;
export const MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS = 16;
export const MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN = 96;
export const MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN = 128;
export const MAX_CODEX_REASONING_GUARD_STRING_FILTER_LIST_LEN = 32;
export const MAX_CODEX_REASONING_GUARD_STRING_FILTER_VALUE_LEN = 128;
export const DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS = [516, 1034, 1552] as const;
export const DEFAULT_CODEX_REASONING_GUARD_RULE_MODE: CodexReasoningGuardRuleMode =
  "reasoning_tokens";
export const CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID =
  "builtin-legacy-reasoning-tokens";
export const CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID =
  "builtin-reasoning-tokens-518n-minus-2";
export const CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID =
  "builtin-final-answer-only-high-xhigh";
export const CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID = "legacy-compatibility";
export const DEFAULT_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS = 5;
export const DEFAULT_CODEX_REASONING_GUARD_BACKOFF_MS = 1_000;
export const DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET = 5;
export const DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET = 5;
export const DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS = 1_000;
export const DEFAULT_CODEX_REASONING_GUARD_EXHAUSTED_ACTION: CodexReasoningGuardExhaustedAction =
  "return_error";
export const DEFAULT_CODEX_REASONING_GUARD_POST_MATCH_STRATEGY: CodexReasoningGuardPostMatchStrategy =
  "continuation_repair";
export const DEFAULT_CODEX_REASONING_GUARD_RETRY_POLICY: CodexReasoningGuardRetryPolicy = "single";
export const DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX = 5;
export const DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS = 1_000;
export const DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS = 10;
export const DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_REPAIR_ENABLED = false;
export const DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS = 3;
export const DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS = 0;
export const MAX_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS = 100;
export const MAX_CODEX_REASONING_GUARD_BACKOFF_MS = 60_000;
export const MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET = 100;
export const MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET = 100;
export const MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS = 60_000;
export const MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX = 5;
export const MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS = 60_000;
export const MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS = 100;
export const MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN = 16;
export const MIN_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS = 1;
export const MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS = 10;
export const MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS = 200_000;
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
const CODEX_REASONING_GUARD_ACTIVE_BUILTIN_TEMPLATE_IDS = new Set([
  CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
  CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID,
  CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID,
]);
const CODEX_REASONING_GUARD_RESERVED_TEMPLATE_IDS = new Set([
  CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
  CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID,
  CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID,
  CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID,
]);
const CODEX_REASONING_GUARD_TEMPLATE_RULE_ACTIONS = new Set<CodexReasoningGuardTemplateRuleAction>([
  "intercept",
  "no_intercept",
]);
const CODEX_REASONING_GUARD_TEMPLATE_RULE_LOGICS = new Set<CodexReasoningGuardTemplateRuleLogic>([
  "and",
  "or",
]);
const CODEX_REASONING_GUARD_TEMPLATE_RULE_FORMULAS =
  new Set<CodexReasoningGuardTemplateRuleFormula>(["reasoning_tokens_518n_minus_2"]);
const CODEX_REASONING_GUARD_TEMPLATE_FILTER_FIELDS =
  new Set<CodexReasoningGuardTemplateFilterField>([
    "duration_ms",
    "tps",
    "output_tokens",
    "input_tokens",
    "total_tokens",
    "reasoning_tokens",
    "final_answer_only",
    "has_tool_call",
    "has_reasoning_item",
    "commentary_observed",
    "request_reasoning_effort",
    "requested_model",
  ]);
const CODEX_REASONING_GUARD_NUMERIC_FILTER_FIELDS = new Set<CodexReasoningGuardTemplateFilterField>(
  ["duration_ms", "tps", "output_tokens", "input_tokens", "total_tokens", "reasoning_tokens"]
);
const CODEX_REASONING_GUARD_BOOL_FILTER_FIELDS = new Set<CodexReasoningGuardTemplateFilterField>([
  "final_answer_only",
  "has_tool_call",
  "has_reasoning_item",
  "commentary_observed",
]);
const CODEX_REASONING_GUARD_STRING_FILTER_FIELDS = new Set<CodexReasoningGuardTemplateFilterField>([
  "request_reasoning_effort",
  "requested_model",
]);
const CODEX_REASONING_GUARD_TEMPLATE_FILTER_OPERATORS =
  new Set<CodexReasoningGuardTemplateFilterOperator>([
    "equals",
    "not_equals",
    "less_than",
    "less_than_or_equal",
    "greater_than",
    "greater_than_or_equal",
    "in",
    "not_in",
  ]);
const CODEX_REASONING_GUARD_NUMERIC_FILTER_OPERATORS =
  new Set<CodexReasoningGuardTemplateFilterOperator>([
    "equals",
    "not_equals",
    "less_than",
    "less_than_or_equal",
    "greater_than",
    "greater_than_or_equal",
  ]);
const CODEX_REASONING_GUARD_BOOL_FILTER_OPERATORS =
  new Set<CodexReasoningGuardTemplateFilterOperator>(["equals", "not_equals"]);
const CODEX_REASONING_GUARD_STRING_FILTER_OPERATORS =
  new Set<CodexReasoningGuardTemplateFilterOperator>(["equals", "not_equals", "in", "not_in"]);

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

function validateBoundedNonEmptySettingsString(
  fieldLabel: string,
  value: unknown,
  maxBytes: number
): string | null {
  if (typeof value !== "string") return `${fieldLabel}必须是字符串`;
  const trimmed = value.trim();
  if (!trimmed) return `${fieldLabel}不能为空`;
  if (utf8Length(trimmed) > maxBytes) return `${fieldLabel}必须 <= ${maxBytes} 字符`;
  return validateNoControlChars(fieldLabel, trimmed);
}

function validateBoundedOptionalSettingsString(
  fieldLabel: string,
  value: unknown,
  maxBytes: number
): string | null {
  if (value == null) return null;
  if (typeof value !== "string") return `${fieldLabel}必须是字符串`;
  if (utf8Length(value.trim()) > maxBytes) return `${fieldLabel}必须 <= ${maxBytes} 字符`;
  return validateNoControlChars(fieldLabel, value.trim());
}

function validateCodexReasoningGuardStringValues(
  fieldLabel: string,
  values: unknown
): string | null {
  if (!Array.isArray(values) || values.length === 0) {
    return `${fieldLabel}不能为空`;
  }
  if (values.length > MAX_CODEX_REASONING_GUARD_STRING_FILTER_LIST_LEN) {
    return `${fieldLabel}最多支持 ${MAX_CODEX_REASONING_GUARD_STRING_FILTER_LIST_LEN} 个值`;
  }

  const seenValues = new Set<string>();
  for (const [index, value] of values.entries()) {
    const itemLabel = `${fieldLabel}[${index}]`;
    const message = validateBoundedNonEmptySettingsString(
      itemLabel,
      value,
      MAX_CODEX_REASONING_GUARD_STRING_FILTER_VALUE_LEN
    );
    if (message) return message;
    const normalized = value.trim();
    if (seenValues.has(normalized)) return `${fieldLabel}不能重复：${normalized}`;
    seenValues.add(normalized);
  }

  return null;
}

function validateCodexReasoningGuardTemplateFilter(
  fieldLabel: string,
  filter: CodexReasoningGuardTemplateFilter
): string | null {
  const idMessage = validateBoundedNonEmptySettingsString(
    `${fieldLabel}.id`,
    filter?.id,
    MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN
  );
  if (idMessage) return idMessage;

  if (!CODEX_REASONING_GUARD_TEMPLATE_FILTER_FIELDS.has(filter.field)) {
    return `${fieldLabel}.field 不支持`;
  }
  if (!CODEX_REASONING_GUARD_TEMPLATE_FILTER_OPERATORS.has(filter.operator)) {
    return `${fieldLabel}.operator 不支持`;
  }

  if (CODEX_REASONING_GUARD_NUMERIC_FILTER_FIELDS.has(filter.field)) {
    if (!CODEX_REASONING_GUARD_NUMERIC_FILTER_OPERATORS.has(filter.operator)) {
      return `${fieldLabel}.operator 不支持数值字段`;
    }
    if (typeof filter.number_value !== "number" || !Number.isFinite(filter.number_value)) {
      return `${fieldLabel}.number_value 必须是有限数字`;
    }
    if (filter.number_value < 0) return `${fieldLabel}.number_value 必须是非负数`;
    return null;
  }

  if (CODEX_REASONING_GUARD_BOOL_FILTER_FIELDS.has(filter.field)) {
    if (!CODEX_REASONING_GUARD_BOOL_FILTER_OPERATORS.has(filter.operator)) {
      return `${fieldLabel}.operator 不支持布尔字段`;
    }
    if (typeof filter.bool_value !== "boolean") {
      return `${fieldLabel}.bool_value 必须是布尔值`;
    }
    return null;
  }

  if (CODEX_REASONING_GUARD_STRING_FILTER_FIELDS.has(filter.field)) {
    if (!CODEX_REASONING_GUARD_STRING_FILTER_OPERATORS.has(filter.operator)) {
      return `${fieldLabel}.operator 不支持字符串字段`;
    }
    if (filter.operator === "equals" || filter.operator === "not_equals") {
      return validateBoundedNonEmptySettingsString(
        `${fieldLabel}.string_value`,
        filter.string_value,
        MAX_CODEX_REASONING_GUARD_STRING_FILTER_VALUE_LEN
      );
    }
    return validateCodexReasoningGuardStringValues(
      `${fieldLabel}.string_values`,
      filter.string_values
    );
  }

  return `${fieldLabel}.field 不支持`;
}

function validateCodexReasoningGuardTemplates(input: {
  activeTemplateId?: string | null;
  customTemplates?: CodexReasoningGuardRuleTemplate[] | null;
}): string | null {
  const activeTemplateId = input.activeTemplateId?.trim();
  if (input.activeTemplateId != null) {
    const activeMessage = validateBoundedNonEmptySettingsString(
      "Codex 降智拦截 active template id",
      input.activeTemplateId,
      MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN
    );
    if (activeMessage) return activeMessage;
  }

  const customTemplates = input.customTemplates;
  if (customTemplates == null) return null;
  if (!Array.isArray(customTemplates)) return "Codex 降智拦截自定义模板必须是列表";
  if (customTemplates.length > MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES) {
    return `Codex 降智拦截自定义模板最多支持 ${MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES} 个`;
  }

  const customTemplateIds = new Set<string>();
  for (const [templateIndex, template] of customTemplates.entries()) {
    const templateLabel = `Codex 降智拦截模板 ${templateIndex + 1}`;
    const templateId = template?.id?.trim() ?? "";
    const templateIdMessage = validateBoundedNonEmptySettingsString(
      `${templateLabel} id`,
      template?.id,
      MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN
    );
    if (templateIdMessage) return templateIdMessage;
    if (CODEX_REASONING_GUARD_RESERVED_TEMPLATE_IDS.has(templateId)) {
      return `${templateLabel} id 不能使用内置模板 id：${templateId}`;
    }
    if (customTemplateIds.has(templateId)) {
      return `Codex 降智拦截模板 id 不能重复：${templateId}`;
    }
    customTemplateIds.add(templateId);

    const nameMessage = validateBoundedNonEmptySettingsString(
      `${templateLabel}名称`,
      template.name,
      MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN
    );
    if (nameMessage) return nameMessage;
    const descriptionMessage = validateBoundedOptionalSettingsString(
      `${templateLabel}描述`,
      template.description,
      MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN
    );
    if (descriptionMessage) return descriptionMessage;

    if (!Array.isArray(template.rules) || template.rules.length === 0) {
      return `${templateLabel}至少需要一条规则`;
    }
    if (template.rules.length > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES) {
      return `${templateLabel}最多支持 ${MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES} 条规则`;
    }

    const seenRuleIds = new Set<string>();
    const seenTokens = new Set<number>();
    const seenFormulas = new Set<CodexReasoningGuardTemplateRuleFormula>();
    let catchAllWildcardSeen = false;
    for (const [ruleIndex, rule] of template.rules.entries()) {
      const ruleLabel = `${templateLabel}规则 ${ruleIndex + 1}`;
      const ruleId = rule?.id?.trim() ?? "";
      const ruleIdMessage = validateBoundedNonEmptySettingsString(
        `${ruleLabel} id`,
        rule?.id,
        MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN
      );
      if (ruleIdMessage) return ruleIdMessage;
      if (seenRuleIds.has(ruleId)) return `${templateLabel}规则 id 不能重复：${ruleId}`;
      seenRuleIds.add(ruleId);

      const ruleNameMessage = validateBoundedNonEmptySettingsString(
        `${ruleLabel}名称`,
        rule.name,
        MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN
      );
      if (ruleNameMessage) return ruleNameMessage;
      if (!CODEX_REASONING_GUARD_TEMPLATE_RULE_ACTIONS.has(rule.action)) {
        return `${ruleLabel} action 仅支持 intercept 或 no_intercept`;
      }
      if (!CODEX_REASONING_GUARD_TEMPLATE_RULE_LOGICS.has(rule.logic)) {
        return `${ruleLabel} logic 仅支持 and 或 or`;
      }

      const reasoningFormula = rule.reasoning_tokens_formula ?? null;
      if (rule.reasoning_tokens != null && reasoningFormula != null) {
        return `${ruleLabel} 不能同时配置 reasoning_tokens 和 reasoning_tokens_formula`;
      }
      if (reasoningFormula != null) {
        if (!CODEX_REASONING_GUARD_TEMPLATE_RULE_FORMULAS.has(reasoningFormula)) {
          return `${ruleLabel} reasoning_tokens_formula 不支持`;
        }
        if (seenFormulas.has(reasoningFormula)) {
          return `${templateLabel}不能重复配置公式：${reasoningFormula}`;
        }
        seenFormulas.add(reasoningFormula);
      } else if (rule.reasoning_tokens == null) {
        const isCatchAllWildcard = Array.isArray(rule.filters) && rule.filters.length === 0;
        if (isCatchAllWildcard && catchAllWildcardSeen) {
          return `${templateLabel}只能有一条无过滤 wildcard 规则`;
        }
        catchAllWildcardSeen ||= isCatchAllWildcard;
      } else {
        if (!Number.isSafeInteger(rule.reasoning_tokens)) {
          return `${ruleLabel} reasoning_tokens 必须是整数`;
        }
        if (
          rule.reasoning_tokens < 0 ||
          rule.reasoning_tokens > MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE
        ) {
          return `${ruleLabel} reasoning_tokens 必须在 0 到 ${MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE} 之间`;
        }
        if (seenTokens.has(rule.reasoning_tokens)) {
          return `${templateLabel}不能重复配置 token：${rule.reasoning_tokens}`;
        }
        seenTokens.add(rule.reasoning_tokens);
      }

      if (!Array.isArray(rule.filters)) return `${ruleLabel} filters 必须是列表`;
      if (rule.filters.length > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS) {
        return `${ruleLabel}最多支持 ${MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS} 个过滤器`;
      }
      const seenFilterIds = new Set<string>();
      for (const [filterIndex, filter] of rule.filters.entries()) {
        const filterId = filter?.id?.trim() ?? "";
        if (seenFilterIds.has(filterId)) return `${ruleLabel}过滤器 id 不能重复：${filterId}`;
        seenFilterIds.add(filterId);
        const filterMessage = validateCodexReasoningGuardTemplateFilter(
          `${ruleLabel}过滤器 ${filterIndex + 1}`,
          filter
        );
        if (filterMessage) return filterMessage;
      }
    }
  }

  if (
    activeTemplateId &&
    !CODEX_REASONING_GUARD_ACTIVE_BUILTIN_TEMPLATE_IDS.has(activeTemplateId) &&
    !customTemplateIds.has(activeTemplateId)
  ) {
    return `Codex 降智拦截 active template 不存在：${activeTemplateId}`;
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
  codexReasoningGuardRuleMode?: CodexReasoningGuardRuleMode | null;
  codexReasoningGuardReasoningEquals?: number[] | null;
  codexReasoningGuardCompareMode?: CodexReasoningGuardCompareMode | null;
  codexReasoningGuardModelRules?: CodexReasoningGuardModelRule[] | null;
  codexReasoningGuardActiveTemplateId?: string | null;
  codexReasoningGuardCustomTemplates?: CodexReasoningGuardRuleTemplate[] | null;
  codexReasoningGuardPostMatchStrategy?: CodexReasoningGuardPostMatchStrategy | null;
  codexReasoningGuardImmediateRetryBudget?: number | null;
  codexReasoningGuardDelayedRetryBudget?: number | null;
  codexReasoningGuardDelayedRetryMs?: number | null;
  codexReasoningGuardExhaustedAction?: CodexReasoningGuardExhaustedAction | null;
  codexReasoningGuardRetryPolicy?: CodexReasoningGuardRetryPolicy | null;
  codexReasoningGuardConcurrentMax?: number | null;
  codexReasoningGuardConcurrentIntervalMs?: number | null;
  codexReasoningGuardConcurrentMaxAttempts?: number | null;
  codexReasoningGuardModelFallbacks?: string[] | null;
  codexReasoningGuardContinuationRepairEnabled?: boolean | null;
  codexReasoningGuardContinuationMaxRounds?: number | null;
  codexReasoningGuardContinuationMaxOutputTokens?: number | null;
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

  const templateMessage = validateCodexReasoningGuardTemplates({
    activeTemplateId: input.codexReasoningGuardActiveTemplateId,
    customTemplates: input.codexReasoningGuardCustomTemplates,
  });
  if (templateMessage) return templateMessage;

  if (input.codexReasoningGuardRuleMode != null) {
    if (
      input.codexReasoningGuardRuleMode !== "reasoning_tokens" &&
      input.codexReasoningGuardRuleMode !== "final_answer_only_high_xhigh"
    ) {
      return "Codex 降智拦截规则模式仅支持 reasoning_tokens 或 final_answer_only_high_xhigh";
    }
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
      input.codexReasoningGuardExhaustedAction !== "switch_provider" &&
      input.codexReasoningGuardExhaustedAction !== "switch_model"
    ) {
      return "Codex 降智拦截预算耗尽动作仅支持 return_error、switch_provider 或 switch_model";
    }
  }

  if (input.codexReasoningGuardPostMatchStrategy != null) {
    if (
      input.codexReasoningGuardPostMatchStrategy !== "retry_same_provider" &&
      input.codexReasoningGuardPostMatchStrategy !== "continuation_repair"
    ) {
      return "Codex 降智拦截命中后策略仅支持 retry_same_provider 或 continuation_repair";
    }
  }

  if (input.codexReasoningGuardRetryPolicy != null) {
    if (
      input.codexReasoningGuardRetryPolicy !== "single" &&
      input.codexReasoningGuardRetryPolicy !== "concurrent"
    ) {
      return "Codex 降智拦截重试策略仅支持 single 或 concurrent";
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
      "Codex 降智拦截并发数量",
      input.codexReasoningGuardConcurrentMax,
      MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX,
    ],
    [
      "Codex 降智拦截并发间隔",
      input.codexReasoningGuardConcurrentIntervalMs,
      MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS,
    ],
    [
      "Codex 降智拦截并发最大尝试次数",
      input.codexReasoningGuardConcurrentMaxAttempts,
      MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS,
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

  if (
    input.codexReasoningGuardConcurrentMax != null &&
    input.codexReasoningGuardConcurrentMax < 1
  ) {
    return `Codex 降智拦截并发数量必须为 1-${MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX} 的整数`;
  }

  const continuationRoundsMessage = validateIntegerRange(
    "Codex 思考续写最大轮数",
    input.codexReasoningGuardContinuationMaxRounds,
    MIN_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS,
    MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS
  );
  if (continuationRoundsMessage) return continuationRoundsMessage;

  const continuationOutputTokensMessage = validateIntegerRange(
    "Codex 思考续写最大 output tokens",
    input.codexReasoningGuardContinuationMaxOutputTokens,
    0,
    MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
  );
  if (continuationOutputTokensMessage) return continuationOutputTokensMessage;

  if (input.codexReasoningGuardModelFallbacks != null) {
    const fallbacks = input.codexReasoningGuardModelFallbacks;
    if (!Array.isArray(fallbacks)) {
      return "Codex 降智拦截模型回退列表必须是列表";
    }
    if (fallbacks.length > MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN) {
      return `Codex 降智拦截模型回退最多支持 ${MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN} 个模型`;
    }
    const seenModels = new Set<string>();
    for (const model of fallbacks) {
      const trimmed = model?.trim() ?? "";
      if (!trimmed) return "Codex 降智拦截模型回退不能包含空模型名";
      if (utf8Length(trimmed) > MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN) {
        return `Codex 降智拦截模型回退名必须 <= ${MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN} 字符`;
      }
      if (CONTROL_CHAR_PATTERN.test(trimmed)) {
        return "Codex 降智拦截模型回退名不能包含控制字符";
      }
      if (seenModels.has(trimmed)) {
        return `Codex 降智拦截模型回退不能重复：${trimmed}`;
      }
      seenModels.add(trimmed);
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
