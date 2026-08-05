import { formatUnknownError } from "../utils/errors";
import { logToConsole } from "./consoleLog";
import { redactDiagnosticText, redactDiagnosticValue } from "./diagnosticRedaction";

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

const LOG_PAYLOAD_MAX_STRING_CHARS = 2048;

function sanitizeLogArgs(value: Record<string, unknown> | undefined) {
  if (value === undefined) return undefined;
  return redactDiagnosticValue(value, {
    maxDepth: 6,
    maxArrayItems: 50,
    maxObjectKeys: 50,
    maxStringChars: LOG_PAYLOAD_MAX_STRING_CHARS,
    stringMode: "metadata",
    objectTruncationKey: "__truncated__",
    objectTruncationValue: (omittedKeys) => `${omittedKeys} keys truncated`,
  }) as Record<string, unknown>;
}

function generatedCommandError(cmd: string, error: unknown) {
  if (error instanceof Error) return error;
  const message = typeof error === "string" ? error : formatUnknownError(error);
  const wrapped = new Error(message || `IPC_ERROR_RESULT: ${cmd}`) as Error & { cause?: unknown };
  wrapped.cause = error;
  return wrapped;
}

function sanitizeLogError(error: unknown): string {
  let source = error;
  if (error instanceof Error) {
    try {
      source = (error as Error & { cause?: unknown }).cause ?? error.message;
    } catch {
      return "[REDACTION_FAILED]";
    }
  }
  if (typeof source === "string") {
    const stringSource = source;
    try {
      source = JSON.parse(stringSource) as unknown;
    } catch {
      return redactDiagnosticText(stringSource, LOG_PAYLOAD_MAX_STRING_CHARS);
    }
  }
  const redacted = redactDiagnosticValue(source, {
    maxDepth: 6,
    maxArrayItems: 50,
    maxObjectKeys: 50,
    maxStringChars: LOG_PAYLOAD_MAX_STRING_CHARS,
    objectTruncationKey: "__truncated__",
    objectTruncationValue: (omittedKeys) => `${omittedKeys} keys truncated`,
  });
  return redactDiagnosticText(formatUnknownError(redacted), LOG_PAYLOAD_MAX_STRING_CHARS);
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
