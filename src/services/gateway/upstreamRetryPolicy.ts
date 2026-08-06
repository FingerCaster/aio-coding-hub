import type {
  UpstreamHttpRetryRule,
  UpstreamRetryPolicy,
  UpstreamTransportRetryKind,
} from "../settings/settings";

export const MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES = 17;
export const MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS = 16;
export const MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS = 512;
export const MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS = 256;
export const MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS = 8;
export const MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORDS = 16;
export const MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORD_CHARS = 512;
export const MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES = 10;
export const MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS = 60_000;

export function createUpstreamHttpRetryRule(statusCode = 500): UpstreamHttpRetryRule {
  return {
    enabled: true,
    status_code: statusCode,
    body_contains: [],
    description: "",
  };
}

export const DEFAULT_UPSTREAM_RETRY_POLICY: UpstreamRetryPolicy = {
  enabled: true,
  http_rules: [
    {
      enabled: true,
      status_code: 400,
      body_contains: ["selected model is at capacity"],
      description: "Codex model capacity",
    },
  ],
  transport_errors: ["connect", "timeout", "read"],
  stream_internal_errors: {
    enabled: true,
    retry_keywords: [],
    non_retry_keywords: [],
  },
  max_retries: 1,
  backoff_ms: 100,
  counts_toward_circuit_breaker: false,
};

export const UPSTREAM_RETRY_TRANSPORT_ERRORS = [
  "connect",
  "timeout",
  "read",
] as const satisfies readonly UpstreamTransportRetryKind[];

export const UPSTREAM_RETRY_TRANSPORT_ERROR_LABELS: Record<UpstreamTransportRetryKind, string> = {
  connect: "连接失败",
  timeout: "超时",
  read: "读取失败",
};

export function cloneUpstreamRetryPolicy(
  policy: UpstreamRetryPolicy | null | undefined
): UpstreamRetryPolicy {
  const source = policy ?? DEFAULT_UPSTREAM_RETRY_POLICY;
  return {
    ...source,
    http_rules: source.http_rules.map((rule) => ({
      ...rule,
      body_contains: [...rule.body_contains],
    })),
    transport_errors: [...source.transport_errors],
    stream_internal_errors: {
      ...(source.stream_internal_errors ?? DEFAULT_UPSTREAM_RETRY_POLICY.stream_internal_errors),
      retry_keywords: [
        ...(source.stream_internal_errors?.retry_keywords ??
          DEFAULT_UPSTREAM_RETRY_POLICY.stream_internal_errors.retry_keywords),
      ],
      non_retry_keywords: [
        ...(source.stream_internal_errors?.non_retry_keywords ??
          DEFAULT_UPSTREAM_RETRY_POLICY.stream_internal_errors.non_retry_keywords),
      ],
    },
  };
}

export function bodyContainsFromTextarea(value: string): string[] {
  return value
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function bodyContainsToTextarea(values: readonly string[]): string {
  return values.join("\n");
}

export function toggleRetryTransportError(
  policy: UpstreamRetryPolicy,
  kind: UpstreamTransportRetryKind
) {
  const selected = new Set(policy.transport_errors);
  if (selected.has(kind)) {
    selected.delete(kind);
  } else {
    selected.add(kind);
  }
  return {
    ...policy,
    transport_errors: UPSTREAM_RETRY_TRANSPORT_ERRORS.filter((item) => selected.has(item)),
  };
}

const CONTROL_CHAR_PATTERN = /[\u0000-\u001f\u007f-\u009f]/u;
const SUPPORTED_TRANSPORT_ERRORS = new Set<UpstreamTransportRetryKind>(
  UPSTREAM_RETRY_TRANSPORT_ERRORS
);

function characterCount(value: string) {
  return Array.from(value).length;
}

export function validateUpstreamRetryPolicy(policy: UpstreamRetryPolicy) {
  if (!Array.isArray(policy.http_rules)) return "瞬时错误重试 HTTP 规则必须是列表";
  if (policy.http_rules.length > MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES) {
    return `瞬时错误重试 HTTP 规则最多支持 ${MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES} 条`;
  }
  for (const [ruleIndex, rule] of policy.http_rules.entries()) {
    const label = `HTTP 规则 ${ruleIndex + 1}`;
    if (typeof rule.enabled !== "boolean") return `${label}启用状态无效`;
    if (!Number.isSafeInteger(rule.status_code)) return `${label}错误码必须是整数`;
    if (rule.status_code < 400 || rule.status_code > 599) {
      return `${label}错误码必须在 400-599`;
    }
    if (!Array.isArray(rule.body_contains)) return `${label}匹配内容必须是列表`;
    if (rule.body_contains.length > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS) {
      return `${label}最多支持 ${MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS} 个匹配内容`;
    }
    for (const content of rule.body_contains) {
      if (typeof content !== "string" || !content.trim()) return `${label}匹配内容不能为空`;
      if (characterCount(content.trim()) > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS) {
        return `${label}每个匹配内容最多 ${MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS} 个字符`;
      }
      if (
        characterCount(content.trim().toLowerCase()) > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS
      ) {
        return `${label}每个匹配内容规范化后最多 ${MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS} 个字符`;
      }
    }
    if (typeof rule.description !== "string") return `${label}描述必须是文本`;
    if (CONTROL_CHAR_PATTERN.test(rule.description)) return `${label}描述不能包含控制字符`;
    if (characterCount(rule.description.trim()) > MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS) {
      return `${label}描述最多 ${MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS} 个字符`;
    }
  }

  if (!Array.isArray(policy.transport_errors)) return "瞬时错误重试传输错误必须是列表";
  if (policy.transport_errors.length > MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS) {
    return `瞬时错误重试传输错误最多支持 ${MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS} 个`;
  }
  for (const kind of policy.transport_errors) {
    if (!SUPPORTED_TRANSPORT_ERRORS.has(kind)) {
      return "瞬时错误重试传输错误仅支持 connect、timeout、read";
    }
  }
  const streamPolicy = policy.stream_internal_errors;
  if (!streamPolicy || typeof streamPolicy.enabled !== "boolean") {
    return "流内部错误重试配置无效";
  }
  for (const [field, label] of [
    [streamPolicy.retry_keywords, "重试关键词"],
    [streamPolicy.non_retry_keywords, "不重试关键词"],
  ] as const) {
    if (!Array.isArray(field)) return `流内部错误${label}必须是列表`;
    if (field.length > MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORDS) {
      return `流内部错误${label}最多支持 ${MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORDS} 项`;
    }
    for (const keyword of field) {
      if (typeof keyword !== "string" || !keyword.trim()) {
        return `流内部错误${label}不能为空`;
      }
      if (CONTROL_CHAR_PATTERN.test(keyword)) return `流内部错误${label}不能包含控制字符`;
      if (characterCount(keyword.trim()) > MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORD_CHARS) {
        return `流内部错误${label}每项最多 ${MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORD_CHARS} 个字符`;
      }
      if (
        characterCount(keyword.trim().toLowerCase()) >
        MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORD_CHARS
      ) {
        return `流内部错误${label}每项规范化后最多 ${MAX_UPSTREAM_STREAM_INTERNAL_ERROR_KEYWORD_CHARS} 个字符`;
      }
    }
  }
  if (
    !Number.isSafeInteger(policy.max_retries) ||
    policy.max_retries < 0 ||
    policy.max_retries > MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES
  ) {
    return `瞬时错误重试次数必须为 0-${MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES}`;
  }
  if (
    !Number.isSafeInteger(policy.backoff_ms) ||
    policy.backoff_ms < 0 ||
    policy.backoff_ms > MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS
  ) {
    return `重试间隔必须为 0-${MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS} 毫秒`;
  }
  if (
    policy.enabled &&
    !policy.http_rules.some((rule) => rule.enabled) &&
    policy.transport_errors.length === 0 &&
    !policy.stream_internal_errors.enabled
  ) {
    return "启用重试时至少需要一条已启用 HTTP 规则、一个传输错误或流内部错误策略";
  }
  return null;
}
