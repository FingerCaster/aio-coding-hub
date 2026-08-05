export const MAX_UPSTREAM_ERROR_RESPONSE_RULES = 32;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS = 100;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS = 256;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES = 16;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS = 16;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS = 512;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_PROVIDER_IDS = 128;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY = 9999;
export const MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS = 4096;

export const UPSTREAM_ERROR_RESPONSE_RULE_CLI_KEYS = ["claude", "codex", "gemini", "grok"] as const;

export type UpstreamErrorResponseRuleCliKey =
  (typeof UPSTREAM_ERROR_RESPONSE_RULE_CLI_KEYS)[number];
export type UpstreamErrorResponseMatchMode = "any" | "all";
export type UpstreamErrorStatusBehavior =
  | { mode: "passthrough" }
  | { mode: "override"; status_code: number };
export type UpstreamErrorMessageBehavior =
  | { mode: "passthrough" }
  | { mode: "override"; message: string };

export type UpstreamErrorResponseRule = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  priority: number;
  status_codes: number[];
  keywords: string[];
  match_mode: UpstreamErrorResponseMatchMode;
  cli_keys: string[];
  provider_ids: number[];
  status_behavior: UpstreamErrorStatusBehavior;
  message_behavior: UpstreamErrorMessageBehavior;
};

const CANONICAL_UUID_V4_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const CONTROL_CHAR_PATTERN = /[\u0000-\u001f\u007f-\u009f]/u;
const MULTILINE_CONTROL_CHAR_PATTERN = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u;

function charCount(value: string): number {
  return Array.from(value).length;
}

function newRuleId(): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi && typeof cryptoApi.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  const bytes = new Uint8Array(16);
  if (cryptoApi) {
    cryptoApi.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(
    16,
    20
  )}-${hex.slice(20)}`;
}

export function createUpstreamErrorResponseRule(
  existingRules: readonly UpstreamErrorResponseRule[]
): UpstreamErrorResponseRule {
  const maxPriority = existingRules.reduce((max, rule) => Math.max(max, rule.priority), 0);
  return {
    id: newRuleId(),
    name: "",
    description: "",
    enabled: true,
    priority: Math.min(MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY, maxPriority + 10),
    status_codes: [],
    keywords: [],
    match_mode: "any",
    cli_keys: [],
    provider_ids: [],
    status_behavior: { mode: "passthrough" },
    message_behavior: { mode: "passthrough" },
  };
}

export function cloneUpstreamErrorResponseRules(
  rules: readonly UpstreamErrorResponseRule[] | null | undefined
): UpstreamErrorResponseRule[] {
  if (!Array.isArray(rules)) return [];
  return rules.flatMap((value) => {
    if (!isRuntimeRule(value)) return [];
    const rule: UpstreamErrorResponseRule = {
      ...value,
      status_codes: [...value.status_codes],
      keywords: [...value.keywords],
      cli_keys: [...value.cli_keys],
      provider_ids: [...value.provider_ids],
      status_behavior: { ...value.status_behavior },
      message_behavior: { ...value.message_behavior },
    };
    return validateUpstreamErrorResponseRules([rule]) == null ? [rule] : [];
  });
}

function isRuntimeRule(value: unknown): value is UpstreamErrorResponseRule {
  if (!value || typeof value !== "object") return false;
  const rule = value as Partial<UpstreamErrorResponseRule>;
  const statusBehavior = rule.status_behavior;
  const messageBehavior = rule.message_behavior;
  return (
    typeof rule.id === "string" &&
    typeof rule.name === "string" &&
    typeof rule.description === "string" &&
    typeof rule.enabled === "boolean" &&
    typeof rule.priority === "number" &&
    (rule.match_mode === "any" || rule.match_mode === "all") &&
    Array.isArray(rule.status_codes) &&
    rule.status_codes.every((status) => typeof status === "number") &&
    Array.isArray(rule.keywords) &&
    rule.keywords.every((keyword) => typeof keyword === "string") &&
    Array.isArray(rule.cli_keys) &&
    rule.cli_keys.every((key) => typeof key === "string") &&
    Array.isArray(rule.provider_ids) &&
    rule.provider_ids.every((id) => typeof id === "number") &&
    !!statusBehavior &&
    (statusBehavior.mode === "passthrough" ||
      (statusBehavior.mode === "override" && typeof statusBehavior.status_code === "number")) &&
    !!messageBehavior &&
    (messageBehavior.mode === "passthrough" ||
      (messageBehavior.mode === "override" && typeof messageBehavior.message === "string"))
  );
}

export function validateUpstreamErrorResponseRules(
  rules: readonly UpstreamErrorResponseRule[] | null | undefined
): string | null {
  if (rules == null) return null;
  if (!Array.isArray(rules)) return "上游错误响应规则格式无效";
  if (rules.length > MAX_UPSTREAM_ERROR_RESPONSE_RULES) {
    return `上游错误响应规则最多 ${MAX_UPSTREAM_ERROR_RESPONSE_RULES} 条`;
  }

  const seenIds = new Set<string>();
  for (let index = 0; index < rules.length; index += 1) {
    const rule = rules[index];
    if (!rule || typeof rule !== "object") return `第 ${index + 1} 条规则格式无效`;
    if (!CANONICAL_UUID_V4_PATTERN.test(rule.id) || seenIds.has(rule.id)) {
      return `第 ${index + 1} 条规则 ID 无效或重复`;
    }
    seenIds.add(rule.id);

    const name = rule.name.trim();
    if (
      !name ||
      charCount(name) > MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS ||
      CONTROL_CHAR_PATTERN.test(name)
    ) {
      return `第 ${index + 1} 条规则名称不能为空，且最多 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS} 个字符`;
    }
    if (
      charCount(rule.description.trim()) > MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS ||
      CONTROL_CHAR_PATTERN.test(rule.description)
    ) {
      return `第 ${index + 1} 条规则说明最多 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS} 个字符`;
    }
    if (!Number.isSafeInteger(rule.priority) || rule.priority < 0 || rule.priority > 9999) {
      return `第 ${index + 1} 条规则优先级必须为 0-${MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY}`;
    }

    if (rule.status_codes.length > MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES) {
      return `第 ${index + 1} 条规则最多配置 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES} 个状态码`;
    }
    if (
      rule.status_codes.some(
        (status: number) => !Number.isSafeInteger(status) || status < 400 || status > 599
      )
    ) {
      return `第 ${index + 1} 条规则的匹配状态码必须为 400-599`;
    }
    if (rule.keywords.length > MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS) {
      return `第 ${index + 1} 条规则最多配置 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS} 个关键词`;
    }
    if (
      rule.keywords.some((keyword: string) => {
        const trimmed = keyword.trim();
        return (
          !trimmed ||
          charCount(trimmed) > MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS ||
          CONTROL_CHAR_PATTERN.test(trimmed)
        );
      })
    ) {
      return `第 ${index + 1} 条规则的关键词不能为空，且每项最多 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS} 个字符`;
    }
    if (rule.status_codes.length === 0 && rule.keywords.length === 0) {
      return `第 ${index + 1} 条规则至少需要一个状态码或关键词`;
    }
    if (!(["any", "all"] as const).includes(rule.match_mode)) {
      return `第 ${index + 1} 条规则的匹配模式无效`;
    }
    if (
      rule.cli_keys.length > UPSTREAM_ERROR_RESPONSE_RULE_CLI_KEYS.length ||
      rule.cli_keys.some(
        (key: string) =>
          !UPSTREAM_ERROR_RESPONSE_RULE_CLI_KEYS.includes(key as UpstreamErrorResponseRuleCliKey)
      )
    ) {
      return `第 ${index + 1} 条规则包含未知 CLI`;
    }
    if (
      rule.provider_ids.length > MAX_UPSTREAM_ERROR_RESPONSE_RULE_PROVIDER_IDS ||
      rule.provider_ids.some((id: number) => !Number.isSafeInteger(id) || id <= 0)
    ) {
      return `第 ${index + 1} 条规则的供应商范围无效`;
    }

    if (
      rule.status_behavior.mode === "override" &&
      (!Number.isSafeInteger(rule.status_behavior.status_code) ||
        rule.status_behavior.status_code < 400 ||
        rule.status_behavior.status_code > 599)
    ) {
      return `第 ${index + 1} 条规则的响应状态码必须为 400-599`;
    }
    if (rule.message_behavior.mode === "override") {
      const message = rule.message_behavior.message.trim();
      if (
        !message ||
        charCount(message) > MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS ||
        MULTILINE_CONTROL_CHAR_PATTERN.test(message)
      ) {
        return `第 ${index + 1} 条规则的自定义信息不能为空，且最多 ${MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS} 个字符`;
      }
    }
  }

  return null;
}
