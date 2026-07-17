import { formatUnknownError } from "../utils/errors";
import { logToConsole } from "./consoleLog";

export type GeneratedCommandResult<T> =
  | { status: "ok"; data: T | null | undefined }
  | { status: "error"; error: unknown };

export type GeneratedCommandResponse<T> = GeneratedCommandResult<T> | T | null | undefined;

type InvokeGeneratedIpcOptions<T> = {
  title: string;
  cmd: string;
  args?: Record<string, unknown>;
  invoke: () => Promise<GeneratedCommandResponse<T>>;
  fallback?: unknown;
  nullResultBehavior?: "throw" | "return_fallback";
};

const LOG_PAYLOAD_MAX_DEPTH = 6;
const LOG_PAYLOAD_MAX_ARRAY_ITEMS = 50;
const LOG_PAYLOAD_MAX_OBJECT_KEYS = 50;
const LOG_PAYLOAD_MAX_STRING_CHARS = 2048;

function isSensitiveLogKey(key: string): boolean {
  const normalized = key.toLowerCase();
  const compact = normalized.replace(/[^a-z0-9]/g, "");
  return (
    normalized.includes("api_key") ||
    normalized.includes("apikey") ||
    normalized.includes("access_token") ||
    normalized.includes("refreshtoken") ||
    normalized.includes("refresh_token") ||
    normalized.includes("authorization") ||
    normalized === "token" ||
    normalized.endsWith("_token") ||
    normalized.endsWith("token") ||
    normalized.includes("secret") ||
    normalized.includes("password") ||
    compact === "flowid" ||
    compact === "devicecode" ||
    compact === "usercode" ||
    compact === "codeverifier" ||
    compact === "nonce" ||
    compact.includes("capability")
  );
}

function truncateLogString(value: string): string {
  if (value.length <= LOG_PAYLOAD_MAX_STRING_CHARS) return value;
  return `${value.slice(0, LOG_PAYLOAD_MAX_STRING_CHARS)}[Truncated ${
    value.length - LOG_PAYLOAD_MAX_STRING_CHARS
  } chars]`;
}

function redactLogPayload(value: unknown, seen: WeakSet<object>, depth: number): unknown {
  if (value == null) return value;
  if (typeof value === "string") return truncateLogString(value);
  if (depth > LOG_PAYLOAD_MAX_DEPTH) return "[Truncated]";
  if (typeof value !== "object") return value;
  if (seen.has(value)) return "[Circular]";

  seen.add(value);

  if (Array.isArray(value)) {
    const items = value
      .slice(0, LOG_PAYLOAD_MAX_ARRAY_ITEMS)
      .map((item) => redactLogPayload(item, seen, depth + 1));
    if (value.length > LOG_PAYLOAD_MAX_ARRAY_ITEMS) {
      items.push(`[Truncated ${value.length - LOG_PAYLOAD_MAX_ARRAY_ITEMS} items]`);
    }
    return items;
  }

  const record = value as Record<string, unknown>;
  const output: Record<string, unknown> = {};
  const entries = Object.entries(record);
  for (const [key, item] of entries.slice(0, LOG_PAYLOAD_MAX_OBJECT_KEYS)) {
    output[key] = isSensitiveLogKey(key) ? "[REDACTED]" : redactLogPayload(item, seen, depth + 1);
  }
  if (entries.length > LOG_PAYLOAD_MAX_OBJECT_KEYS) {
    output.__truncated__ = `${entries.length - LOG_PAYLOAD_MAX_OBJECT_KEYS} keys truncated`;
  }
  return output;
}

function sanitizeLogArgs(value: Record<string, unknown> | undefined) {
  if (value === undefined) return undefined;
  try {
    return redactLogPayload(value, new WeakSet(), 0) as Record<string, unknown>;
  } catch {
    return { error: "LOG_ARG_REDACTION_FAILED" };
  }
}

function generatedCommandError(cmd: string, error: unknown) {
  if (error instanceof Error) return error;
  const message = typeof error === "string" ? error : formatUnknownError(error);
  const wrapped = new Error(message || `IPC_ERROR_RESULT: ${cmd}`) as Error & { cause?: unknown };
  wrapped.cause = error;
  return wrapped;
}

function sanitizeSensitiveErrorText(value: string): string {
  return value
    .replace(/SYNTHETIC_SECRET/gi, "[REDACTED]")
    .replace(
      /(["']?(?:flow_?id|device_?code|user_?code|code_?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)["']?\s*:\s*["'])([^"']*)(["'])/gi,
      "$1[REDACTED]$3"
    )
    .replace(
      /(flow_?id|device_?code|user_?code|code_?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)\s*[:=]\s*\S+/gi,
      "$1=[REDACTED]"
    )
    .slice(0, LOG_PAYLOAD_MAX_STRING_CHARS);
}

function sanitizeLogError(error: unknown): string {
  let source = error;
  if (error instanceof Error) {
    source = (error as Error & { cause?: unknown }).cause ?? error.message;
  }
  if (typeof source === "string") {
    const stringSource = source;
    try {
      source = JSON.parse(stringSource) as unknown;
    } catch {
      return sanitizeSensitiveErrorText(stringSource);
    }
  }
  try {
    const redacted = redactLogPayload(source, new WeakSet(), 0);
    return sanitizeSensitiveErrorText(formatUnknownError(redacted));
  } catch {
    return "LOG_ERROR_REDACTION_FAILED";
  }
}

function isGeneratedCommandResult<T>(value: unknown): value is GeneratedCommandResult<T> {
  if (value == null || typeof value !== "object") {
    return false;
  }

  const candidate = value as Record<string, unknown>;
  if (candidate.status !== "ok" && candidate.status !== "error") {
    return false;
  }

  return "data" in candidate || "error" in candidate;
}

export function mapGeneratedCommandResponse<TValue, TMapped>(
  value: GeneratedCommandResponse<TValue>,
  map: (value: TValue) => TMapped
): GeneratedCommandResponse<TMapped> {
  if (value == null) {
    return value as GeneratedCommandResponse<TMapped>;
  }

  if (isGeneratedCommandResult<TValue>(value)) {
    if (value.status === "error") {
      return value;
    }
    if (value.data == null) {
      return {
        status: "ok",
        data: value.data as TMapped | null | undefined,
      };
    }
    return {
      status: "ok",
      data: map(value.data),
    };
  }

  return map(value);
}

export async function invokeGeneratedIpc<T, Fallback = never>(
  options: InvokeGeneratedIpcOptions<T>
): Promise<T | Fallback> {
  const fallback = options.fallback as Fallback;

  try {
    const result = await options.invoke();
    if (isGeneratedCommandResult<T>(result)) {
      if (result.status === "error") {
        throw generatedCommandError(options.cmd, result.error);
      }
      if (result.data != null) {
        return result.data;
      }
    } else if (result != null) {
      return result;
    }
    if (options.nullResultBehavior === "return_fallback") {
      return fallback;
    }
    throw new Error(`IPC_NULL_RESULT: ${options.cmd}`);
  } catch (err) {
    logToConsole("error", options.title, {
      cmd: options.cmd,
      args: sanitizeLogArgs(options.args),
      error: sanitizeLogError(err),
    });
    throw err;
  }
}
