export type DiagnosticStringMode = "text" | "metadata";

export type DiagnosticRedactionOptions = {
  maxDepth?: number;
  maxArrayItems?: number;
  maxObjectKeys?: number;
  maxStringChars?: number;
  maxNodes?: number;
  maxTotalStringChars?: number;
  stringMode?: DiagnosticStringMode;
  objectTruncationKey?: string;
  objectTruncationValue?: string | ((omittedKeys: number) => string);
};

type ResolvedDiagnosticRedactionOptions = Required<
  Omit<DiagnosticRedactionOptions, "objectTruncationValue">
> & {
  objectTruncationValue: string | ((omittedKeys: number) => string);
};

type DiagnosticRedactionBudget = {
  nodesRemaining: number;
  stringCharsRemaining: number;
};

const REDACTED = "[REDACTED]";
const REDACTION_FAILED = "[REDACTION_FAILED]";
const DEFAULT_OPTIONS: ResolvedDiagnosticRedactionOptions = {
  maxDepth: 6,
  maxArrayItems: 100,
  maxObjectKeys: 100,
  maxStringChars: 4096,
  maxNodes: 2000,
  maxTotalStringChars: 65_536,
  stringMode: "text",
  objectTruncationKey: "__truncated_keys",
  objectTruncationValue: "[Truncated]",
};

const SAFE_METADATA_KEYS = new Set([
  "action",
  "category",
  "cli",
  "clikey",
  "command",
  "endpoint",
  "format",
  "kind",
  "mode",
  "operation",
  "providerid",
  "scope",
  "source",
  "status",
  "target",
  "type",
]);

const DIAGNOSTIC_URL_RE = /\bhttps?:\/\/[^\s<>"']+/gi;
const AUTHORIZATION_RE =
  /\b((?:proxy[-_])?authorization)(\s*[:=]\s*)(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;}\]]+(?:\s+[^\s,;}\]]+)?)/gi;
const BEARER_RE = /\b(bearer\s+)[^\s,;"']+/gi;
const QUOTED_SECRET_ASSIGNMENT_RE =
  /(["']?(?:[a-z0-9_-]*token|api[_-]?key|client[_-]?secret|private[_-]?key|password|passwd|secret|credential|cookie|flow[_-]?id|device[_-]?code|user[_-]?code|code[_-]?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)["']?\s*[:=]\s*)(["'])(.*?)\2/gi;
const UNTERMINATED_QUOTED_SECRET_ASSIGNMENT_RE =
  /(["']?(?:[a-z0-9_-]*token|api[_-]?key|client[_-]?secret|private[_-]?key|password|passwd|secret|credential|cookie|flow[_-]?id|device[_-]?code|user[_-]?code|code[_-]?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)["']?\s*[:=]\s*)(["'])[^"'\r\n]*$/gim;
const UNQUOTED_SECRET_ASSIGNMENT_RE =
  /(["']?(?:[a-z0-9_-]*token|api[_-]?key|client[_-]?secret|private[_-]?key|password|passwd|secret|credential|cookie|flow[_-]?id|device[_-]?code|user[_-]?code|code[_-]?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)["']?\s*[:=]\s*)(?!["'])([^\s,;&}\]]+)/gi;
const KEYLIKE_SECRET_RE =
  /\b(?:sk-proj|sk|ghp|gho|ghu|ghs|ghr|github_pat|xox[baprs])[-_][a-z0-9_-]{8,}\b/gi;

function normalizedKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

export function isSensitiveDiagnosticKey(key: string): boolean {
  const compact = normalizedKey(key);
  return (
    compact.includes("apikey") ||
    compact.includes("accesstoken") ||
    compact.includes("refreshtoken") ||
    compact.includes("idtoken") ||
    compact === "token" ||
    compact.endsWith("token") ||
    compact.includes("authorization") ||
    compact.includes("password") ||
    compact.includes("passwd") ||
    compact.includes("secret") ||
    compact.includes("credential") ||
    compact.includes("privatekey") ||
    compact === "cookie" ||
    compact === "setcookie" ||
    compact === "flowid" ||
    compact === "devicecode" ||
    compact === "usercode" ||
    compact === "codeverifier" ||
    compact === "nonce" ||
    compact === "customscript" ||
    compact === "customallowedorigins" ||
    compact === "baseurl" ||
    compact === "baseorigin" ||
    compact.includes("capability")
  );
}

function normalizeUrl(value: string): { value: string; removedSensitiveParts: boolean } | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  try {
    const url = new URL(trimmed);
    const removedSensitiveParts = Boolean(url.username || url.password || url.search || url.hash);
    url.username = "";
    url.password = "";
    url.search = "";
    url.hash = "";
    return { value: url.toString(), removedSensitiveParts };
  } catch {
    return null;
  }
}

function truncateText(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value;
  const suffix = "[Truncated]";
  if (maxChars <= suffix.length) return suffix.slice(0, maxChars);
  return `${value.slice(0, maxChars - suffix.length)}${suffix}`;
}

export function redactDiagnosticText(
  value: string,
  maxChars = DEFAULT_OPTIONS.maxStringChars
): string {
  let redacted = value.replace(DIAGNOSTIC_URL_RE, (rawUrl) => {
    const sanitized = normalizeUrl(rawUrl);
    if (!sanitized) return REDACTED;
    return sanitized.removedSensitiveParts ? `${sanitized.value} ${REDACTED}` : sanitized.value;
  });
  redacted = redacted.replace(AUTHORIZATION_RE, `$1$2${REDACTED}`);
  redacted = redacted.replace(BEARER_RE, `$1${REDACTED}`);
  redacted = redacted.replace(QUOTED_SECRET_ASSIGNMENT_RE, `$1$2${REDACTED}$2`);
  redacted = redacted.replace(UNTERMINATED_QUOTED_SECRET_ASSIGNMENT_RE, `$1$2${REDACTED}`);
  redacted = redacted.replace(UNQUOTED_SECRET_ASSIGNMENT_RE, `$1${REDACTED}`);
  redacted = redacted.replace(KEYLIKE_SECRET_RE, REDACTED);
  redacted = redacted.replace(/SYNTHETIC_SECRET/gi, REDACTED);
  return truncateText(redacted, maxChars);
}

export function diagnosticStringMetadata(value: string): string {
  return `[String length=${value.length}]`;
}

function canPreserveMetadataString(key: string | undefined, value: string): boolean {
  if (!key || value.length > 128) return false;
  if (!SAFE_METADATA_KEYS.has(normalizedKey(key))) return false;
  return /^[a-z0-9][a-z0-9._:/-]*$/i.test(value);
}

function isDiagnosticUrlKey(key: string | undefined): boolean {
  if (!key) return false;
  const compact = normalizedKey(key);
  return compact === "href" || compact.endsWith("url");
}

function isSafeDiagnosticMarker(value: unknown): boolean {
  return (
    value === REDACTED ||
    value === REDACTION_FAILED ||
    value === "[Circular]" ||
    (typeof value === "string" && /^\[Truncated(?: [^\]]+)?\]$/.test(value))
  );
}

function isPreRedactedProjection(
  value: unknown,
  seen: WeakSet<object> = new WeakSet(),
  depth = 0
): boolean {
  if (isSafeDiagnosticMarker(value)) return true;
  if (value == null || typeof value === "boolean" || typeof value === "number") return true;
  if (!value || typeof value !== "object" || depth > 4 || seen.has(value)) return false;
  seen.add(value);
  try {
    const keys = Object.keys(value);
    if (keys.length === 0 || keys.length > 50) return false;
    return keys.every((key) =>
      isPreRedactedProjection((value as Record<string, unknown>)[key], seen, depth + 1)
    );
  } catch {
    return false;
  }
}

function resolveOptions(options: DiagnosticRedactionOptions): ResolvedDiagnosticRedactionOptions {
  return { ...DEFAULT_OPTIONS, ...options };
}

function objectTruncationValue(
  option: ResolvedDiagnosticRedactionOptions["objectTruncationValue"],
  omittedKeys: number
): string {
  return typeof option === "function" ? option(omittedKeys) : option;
}

function redactValue(
  value: unknown,
  options: ResolvedDiagnosticRedactionOptions,
  budget: DiagnosticRedactionBudget,
  seen: WeakSet<object>,
  depth: number,
  key?: string
): unknown {
  if (budget.nodesRemaining <= 0) return "[Truncated]";
  budget.nodesRemaining -= 1;
  if (value == null) return value;
  if (typeof value === "string") {
    if (budget.stringCharsRemaining <= 0) return "[Truncated]";
    if (options.stringMode === "metadata") {
      let metadata: string;
      if (isSafeDiagnosticMarker(value)) {
        metadata = value;
      } else if (isDiagnosticUrlKey(key)) {
        const sanitized = normalizeUrl(value);
        metadata = sanitized
          ? sanitized.removedSensitiveParts
            ? sanitized.value
            : value.trim()
          : diagnosticStringMetadata(value);
      } else if (canPreserveMetadataString(key, value)) {
        metadata = redactDiagnosticText(value, options.maxStringChars);
      } else {
        metadata = diagnosticStringMetadata(value);
      }
      const bounded = truncateText(
        metadata,
        Math.min(options.maxStringChars, budget.stringCharsRemaining)
      );
      budget.stringCharsRemaining -= Math.min(bounded.length, budget.stringCharsRemaining);
      return bounded;
    }
    const redacted = redactDiagnosticText(
      value,
      Math.min(options.maxStringChars, budget.stringCharsRemaining)
    );
    budget.stringCharsRemaining -= Math.min(redacted.length, budget.stringCharsRemaining);
    return redacted;
  }
  if (typeof value === "number" || typeof value === "boolean") return value;
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "function") return "[Function]";
  if (typeof value === "symbol") return "[Symbol]";
  if (typeof value !== "object") return String(value);
  if (depth > options.maxDepth) return "[Truncated]";
  if (seen.has(value)) return "[Circular]";
  seen.add(value);

  if (value instanceof Error) {
    const readField = (field: "name" | "message" | "stack") => {
      try {
        const fieldValue = value[field];
        return typeof fieldValue === "string"
          ? redactValue(fieldValue, options, budget, seen, depth + 1, field)
          : null;
      } catch {
        return REDACTION_FAILED;
      }
    };
    return {
      name: readField("name"),
      message: readField("message"),
      stack: readField("stack"),
    };
  }

  if (Array.isArray(value)) {
    const output: unknown[] = [];
    const itemCount = Math.min(value.length, options.maxArrayItems);
    for (let index = 0; index < itemCount; index += 1) {
      try {
        output.push(redactValue(value[index], options, budget, seen, depth + 1));
      } catch {
        output.push(REDACTION_FAILED);
      }
    }
    if (value.length > options.maxArrayItems) {
      output.push(`[Truncated ${value.length - options.maxArrayItems} items]`);
    }
    return output;
  }

  const output: Record<string, unknown> = {};
  const keys = Object.keys(value);
  for (const itemKey of keys.slice(0, options.maxObjectKeys)) {
    let item: unknown;
    try {
      item = (value as Record<string, unknown>)[itemKey];
    } catch {
      output[itemKey] = REDACTION_FAILED;
      continue;
    }
    const preservesRedactedProjection =
      isSafeDiagnosticMarker(item) ||
      (typeof item === "object" && item !== null && isPreRedactedProjection(item));
    if (isSensitiveDiagnosticKey(itemKey) && !preservesRedactedProjection) {
      output[itemKey] = REDACTED;
      continue;
    }
    try {
      output[itemKey] = redactValue(item, options, budget, seen, depth + 1, itemKey);
    } catch {
      output[itemKey] = REDACTION_FAILED;
    }
  }
  const omittedKeys = keys.length - options.maxObjectKeys;
  if (omittedKeys > 0) {
    output[options.objectTruncationKey] = objectTruncationValue(
      options.objectTruncationValue,
      omittedKeys
    );
  }
  return output;
}

export function redactDiagnosticValue(
  value: unknown,
  options: DiagnosticRedactionOptions = {}
): unknown {
  try {
    const resolvedOptions = resolveOptions(options);
    return redactValue(
      value,
      resolvedOptions,
      {
        nodesRemaining: resolvedOptions.maxNodes,
        stringCharsRemaining: resolvedOptions.maxTotalStringChars,
      },
      new WeakSet(),
      0
    );
  } catch {
    return REDACTION_FAILED;
  }
}

export function redactDiagnosticJsonText(value: string, maxChars = 16_384): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  try {
    const redacted = redactDiagnosticValue(JSON.parse(trimmed) as unknown, {
      maxStringChars: maxChars,
    });
    const serialized = JSON.stringify(redacted);
    return truncateText(serialized || REDACTION_FAILED, maxChars);
  } catch {
    return redactDiagnosticText(trimmed, maxChars);
  }
}

export function sanitizeDiagnosticUrl(
  value: string | null | undefined,
  maxChars = 2048
): string | null {
  if (value == null) return null;
  const sanitized = normalizeUrl(value);
  if (!sanitized) return null;
  return truncateText(sanitized.value, maxChars);
}
