import type { CodexModelContextRule as GeneratedCodexModelContextRule } from "../../generated/bindings";
import {
  CX2CC_CONTEXT_WINDOW_MAX as MODEL_CONTEXT_WINDOW_MAX_TOKENS,
  CX2CC_CONTEXT_WINDOW_MIN as MODEL_CONTEXT_WINDOW_MIN_TOKENS,
} from "../../constants/cx2cc";

export const MAX_CODEX_MODEL_CONTEXT_RULES = 128;
export const MAX_CODEX_MODEL_CONTEXT_MODEL_ID_BYTES = 256;
export const GPT56_372K_CONTEXT_TOKENS = 372_000;

export type CodexModelContextRule = GeneratedCodexModelContextRule;

export type CodexModelContextRuleDraftValue = {
  model_id: string;
  context_window: string | number;
  enabled: boolean;
};

export type CodexModelContextRuleValidationCode =
  | "CODEX_MODEL_CONTEXT_RULE_LIMIT"
  | "CODEX_MODEL_CONTEXT_RULE_INVALID"
  | "CODEX_MODEL_CONTEXT_RULE_DUPLICATE";

export class CodexModelContextRuleValidationError extends Error {
  readonly code: CodexModelContextRuleValidationCode;
  readonly rowIndex: number | null;
  readonly field: "model_id" | "context_window" | "enabled" | null;

  constructor(
    code: CodexModelContextRuleValidationCode,
    message: string,
    options: {
      rowIndex?: number;
      field?: "model_id" | "context_window" | "enabled";
    } = {}
  ) {
    super(`${code}: ${message}`);
    this.name = "CodexModelContextRuleValidationError";
    this.code = code;
    this.rowIndex = options.rowIndex ?? null;
    this.field = options.field ?? null;
  }
}

export const GPT56_372K_CONTEXT_PRESET = [
  { model_id: "gpt-5.6-sol", context_window: GPT56_372K_CONTEXT_TOKENS, enabled: true },
  { model_id: "gpt-5.6-terra", context_window: GPT56_372K_CONTEXT_TOKENS, enabled: true },
  { model_id: "gpt-5.6-luna", context_window: GPT56_372K_CONTEXT_TOKENS, enabled: true },
] as const satisfies readonly CodexModelContextRule[];

const utf8Encoder = new TextEncoder();
const CANONICAL_RULE_KEYS = new Set(["model_id", "context_window", "enabled"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function hasOnlyCanonicalRuleKeys(value: Record<string, unknown>): boolean {
  const keys = Object.keys(value);
  return (
    keys.length === CANONICAL_RULE_KEYS.size && keys.every((key) => CANONICAL_RULE_KEYS.has(key))
  );
}

function invalidRule(
  message: string,
  rowIndex: number,
  field: "model_id" | "context_window" | "enabled"
): never {
  throw new CodexModelContextRuleValidationError("CODEX_MODEL_CONTEXT_RULE_INVALID", message, {
    rowIndex,
    field,
  });
}

function normalizeContextWindow(value: unknown, rowIndex: number): number {
  let normalized: number;
  if (typeof value === "number") {
    normalized = value;
  } else if (typeof value === "string") {
    const trimmed = value.trim();
    if (!/^[0-9]+$/u.test(trimmed)) {
      invalidRule(`第 ${rowIndex + 1} 条规则的上下文必须是十进制整数`, rowIndex, "context_window");
    }
    normalized = Number(trimmed);
  } else {
    invalidRule(`第 ${rowIndex + 1} 条规则的上下文必须是十进制整数`, rowIndex, "context_window");
  }

  if (
    !Number.isSafeInteger(normalized) ||
    normalized < MODEL_CONTEXT_WINDOW_MIN_TOKENS ||
    normalized > MODEL_CONTEXT_WINDOW_MAX_TOKENS
  ) {
    invalidRule(
      `第 ${rowIndex + 1} 条规则的上下文必须在 ${MODEL_CONTEXT_WINDOW_MIN_TOKENS.toLocaleString("en-US")} 到 ${MODEL_CONTEXT_WINDOW_MAX_TOKENS.toLocaleString("en-US")} 之间`,
      rowIndex,
      "context_window"
    );
  }

  return normalized;
}

export function normalizeCodexModelContextModelId(value: unknown, rowIndex = 0): string {
  if (typeof value !== "string") {
    invalidRule(`第 ${rowIndex + 1} 条规则的模型 ID 必须是字符串`, rowIndex, "model_id");
  }

  const modelId = value.trim();
  const modelIdBytes = utf8Encoder.encode(modelId).length;
  if (modelIdBytes === 0 || modelIdBytes > MAX_CODEX_MODEL_CONTEXT_MODEL_ID_BYTES) {
    invalidRule(
      `第 ${rowIndex + 1} 条规则的模型 ID 必须为 1 到 ${MAX_CODEX_MODEL_CONTEXT_MODEL_ID_BYTES} 个 UTF-8 字节`,
      rowIndex,
      "model_id"
    );
  }
  if (/\p{Cc}/u.test(modelId)) {
    invalidRule(`第 ${rowIndex + 1} 条规则的模型 ID 不能包含控制字符`, rowIndex, "model_id");
  }
  if (modelId.startsWith("aio/")) {
    invalidRule(`第 ${rowIndex + 1} 条规则不能覆盖 aio/ managed 模型`, rowIndex, "model_id");
  }

  return modelId;
}

function compareUtf8Bytes(left: string, right: string): number {
  const leftBytes = utf8Encoder.encode(left);
  const rightBytes = utf8Encoder.encode(right);
  const length = Math.min(leftBytes.length, rightBytes.length);
  for (let index = 0; index < length; index += 1) {
    const difference = leftBytes[index] - rightBytes[index];
    if (difference !== 0) return difference;
  }
  return leftBytes.length - rightBytes.length;
}

export function normalizeCodexModelContextRules(input: unknown): CodexModelContextRule[] {
  if (!Array.isArray(input)) {
    throw new CodexModelContextRuleValidationError(
      "CODEX_MODEL_CONTEXT_RULE_INVALID",
      "规则集合必须是数组"
    );
  }
  if (input.length > MAX_CODEX_MODEL_CONTEXT_RULES) {
    throw new CodexModelContextRuleValidationError(
      "CODEX_MODEL_CONTEXT_RULE_LIMIT",
      `规则最多允许 ${MAX_CODEX_MODEL_CONTEXT_RULES} 条`
    );
  }

  const seenModelIds = new Map<string, number>();
  const normalized = input.map((value, rowIndex): CodexModelContextRule => {
    if (value == null || typeof value !== "object") {
      invalidRule(`第 ${rowIndex + 1} 条规则格式无效`, rowIndex, "model_id");
    }

    const candidate = value as Partial<Record<keyof CodexModelContextRule, unknown>>;
    const modelId = normalizeCodexModelContextModelId(candidate.model_id, rowIndex);

    const duplicateIndex = seenModelIds.get(modelId);
    if (duplicateIndex != null) {
      throw new CodexModelContextRuleValidationError(
        "CODEX_MODEL_CONTEXT_RULE_DUPLICATE",
        `第 ${rowIndex + 1} 条规则与第 ${duplicateIndex + 1} 条规则使用了相同的模型 ID`,
        { rowIndex, field: "model_id" }
      );
    }
    seenModelIds.set(modelId, rowIndex);

    if (typeof candidate.enabled !== "boolean") {
      invalidRule(`第 ${rowIndex + 1} 条规则的启用状态必须是布尔值`, rowIndex, "enabled");
    }

    return {
      model_id: modelId,
      context_window: normalizeContextWindow(candidate.context_window, rowIndex),
      enabled: candidate.enabled,
    };
  });

  normalized.sort((left, right) => compareUtf8Bytes(left.model_id, right.model_id));
  return normalized;
}

/**
 * Decodes the backend-owned canonical collection without silently repairing a
 * malformed, padded, unsorted, or additive response.
 */
export function decodeCanonicalCodexModelContextRules(input: unknown): CodexModelContextRule[] {
  if (!Array.isArray(input)) {
    throw new Error("IPC_INVALID_RESULT: codex_model_context_rules must be an array");
  }

  for (const value of input) {
    if (
      !isRecord(value) ||
      !hasOnlyCanonicalRuleKeys(value) ||
      typeof value.model_id !== "string" ||
      typeof value.context_window !== "number" ||
      typeof value.enabled !== "boolean"
    ) {
      throw new Error("IPC_INVALID_RESULT: codex_model_context_rules contains an invalid rule");
    }
  }

  let normalized: CodexModelContextRule[];
  try {
    normalized = normalizeCodexModelContextRules(input);
  } catch {
    throw new Error("IPC_INVALID_RESULT: codex_model_context_rules is not canonical");
  }

  const isCanonical = normalized.every((rule, index) => {
    const source = input[index] as Record<string, unknown>;
    return (
      source.model_id === rule.model_id &&
      source.context_window === rule.context_window &&
      source.enabled === rule.enabled
    );
  });
  if (!isCanonical) {
    throw new Error("IPC_INVALID_RESULT: codex_model_context_rules is not canonical");
  }

  return normalized;
}

export function codexModelContextRulesKey(input: unknown): string {
  return JSON.stringify(normalizeCodexModelContextRules(input));
}

export function codexModelContextRulesEqual(left: unknown, right: unknown): boolean {
  try {
    return codexModelContextRulesKey(left) === codexModelContextRulesKey(right);
  } catch {
    return false;
  }
}

export function hasEnabledCodexModelContextRules(input: unknown): boolean {
  return normalizeCodexModelContextRules(input).some((rule) => rule.enabled);
}

export function addGpt56Preset(draft: readonly CodexModelContextRuleDraftValue[]): {
  draft: CodexModelContextRuleDraftValue[];
  addedRules: CodexModelContextRule[];
  added: number;
  skipped: string[];
} {
  if (!Array.isArray(draft)) {
    throw new CodexModelContextRuleValidationError(
      "CODEX_MODEL_CONTEXT_RULE_INVALID",
      "规则集合必须是数组"
    );
  }

  const existingModelIds = new Set(
    draft.map((rule) => (typeof rule?.model_id === "string" ? rule.model_id.trim() : ""))
  );
  const addedRules = GPT56_372K_CONTEXT_PRESET.filter(
    (rule) => !existingModelIds.has(rule.model_id)
  ).map((rule) => ({ ...rule }));
  const skipped = GPT56_372K_CONTEXT_PRESET.filter((rule) =>
    existingModelIds.has(rule.model_id)
  ).map((rule) => rule.model_id);

  if (draft.length + addedRules.length > MAX_CODEX_MODEL_CONTEXT_RULES) {
    throw new CodexModelContextRuleValidationError(
      "CODEX_MODEL_CONTEXT_RULE_LIMIT",
      `剩余容量不足，GPT-5.6 预设未添加；规则最多允许 ${MAX_CODEX_MODEL_CONTEXT_RULES} 条`
    );
  }

  return {
    draft: [...draft.map((rule) => ({ ...rule })), ...addedRules],
    addedRules,
    added: addedRules.length,
    skipped,
  };
}

export type CodexModelContextBaseCandidate = {
  base_context_window: number | null;
  base_max_context_window: number | null;
};

export type CodexModelContextBaseComparison = {
  text: string;
  increasesBase: boolean;
  available: boolean;
};

function readableBaseValue(value: number | null | undefined): number | null {
  return Number.isSafeInteger(value) && (value ?? -1) >= 0 ? (value as number) : null;
}

export function formatCodexContextTokens(value: number): string {
  return value.toLocaleString("en-US");
}

export function describeCodexModelContextBase(
  candidate: CodexModelContextBaseCandidate | null | undefined,
  target: number,
  enabled: boolean
): CodexModelContextBaseComparison {
  if (!candidate) {
    return {
      text: `当前基础目录不可用 · 目标 ${formatCodexContextTokens(target)}`,
      increasesBase: false,
      available: false,
    };
  }

  const contextWindow = readableBaseValue(candidate.base_context_window);
  const maxContextWindow = readableBaseValue(candidate.base_max_context_window);
  const knownValues = [contextWindow, maxContextWindow].filter(
    (value): value is number => value != null
  );
  const increasesBase = enabled && knownValues.some((value) => target > value);

  if (contextWindow == null && maxContextWindow == null) {
    return {
      text: `基础值不可用 · 目标 ${formatCodexContextTokens(target)}`,
      increasesBase,
      available: false,
    };
  }

  if (contextWindow != null && contextWindow === maxContextWindow) {
    return {
      text: `基础 ${formatCodexContextTokens(contextWindow)} -> 目标 ${formatCodexContextTokens(target)}`,
      increasesBase,
      available: true,
    };
  }

  const contextText =
    contextWindow == null ? "context 不可用" : `context ${formatCodexContextTokens(contextWindow)}`;
  const maxText =
    maxContextWindow == null ? "max 不可用" : `max ${formatCodexContextTokens(maxContextWindow)}`;
  return {
    text: `${contextText} / ${maxText} -> 目标 ${formatCodexContextTokens(target)}`,
    increasesBase,
    available: true,
  };
}
