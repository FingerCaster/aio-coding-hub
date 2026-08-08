import type { ModelRoutingPolicy, ModelRoutingRule } from "../../generated/bindings";

export const MAX_MODEL_ROUTING_RULES = 128;
export const MAX_MODEL_ROUTING_MODEL_BYTES = 256;
export const MAX_MODEL_ROUTING_EFFORT_CHARS = 64;

export const DEFAULT_MODEL_ROUTING_POLICY: ModelRoutingPolicy = {
  enabled: false,
  rules: [],
};

export function cloneModelRoutingPolicy(
  policy: ModelRoutingPolicy | null | undefined
): ModelRoutingPolicy {
  const source = policy ?? DEFAULT_MODEL_ROUTING_POLICY;
  return {
    enabled: source.enabled,
    rules: source.rules.map((rule) => ({ ...rule })),
  };
}

export function emptyModelRoutingRule(): ModelRoutingRule {
  return {
    source_model: "",
    target_model: null,
    reasoning_effort: null,
  };
}

function normalizedOptional(value: string | null | undefined): string | null {
  const normalized = value?.trim() ?? "";
  return normalized.length > 0 ? normalized : null;
}

function containsControlCharacter(value: string): boolean {
  return [...value].some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 0x1f || (codePoint >= 0x7f && codePoint <= 0x9f);
  });
}

export function normalizeModelRoutingPolicy(policy: ModelRoutingPolicy): ModelRoutingPolicy {
  return {
    enabled: policy.enabled,
    rules: policy.rules.map((rule) => ({
      source_model: rule.source_model.trim(),
      target_model: normalizedOptional(rule.target_model),
      reasoning_effort: normalizedOptional(rule.reasoning_effort),
    })),
  };
}

export function validateModelRoutingPolicy(policy: ModelRoutingPolicy): string | null {
  if (policy.rules.length > MAX_MODEL_ROUTING_RULES) {
    return `模型路由规则最多 ${MAX_MODEL_ROUTING_RULES} 条`;
  }
  if (policy.enabled && policy.rules.length === 0) {
    return "启用模型路由时至少需要一条规则";
  }

  const seen = new Set<string>();
  for (const [index, rawRule] of policy.rules.entries()) {
    const rule = {
      source_model: rawRule.source_model.trim(),
      target_model: normalizedOptional(rawRule.target_model),
      reasoning_effort: normalizedOptional(rawRule.reasoning_effort),
    };
    const label = `第 ${index + 1} 条模型路由`;
    if (!rule.source_model) return `${label}必须填写来源模型`;
    if (new TextEncoder().encode(rule.source_model).length > MAX_MODEL_ROUTING_MODEL_BYTES) {
      return `${label}的来源模型不能超过 ${MAX_MODEL_ROUTING_MODEL_BYTES} 字节`;
    }
    if (containsControlCharacter(rule.source_model)) return `${label}的来源模型包含控制字符`;
    if (seen.has(rule.source_model)) return `${label}与已有来源模型重复`;
    seen.add(rule.source_model);

    if (rule.target_model) {
      if (new TextEncoder().encode(rule.target_model).length > MAX_MODEL_ROUTING_MODEL_BYTES) {
        return `${label}的目标模型不能超过 ${MAX_MODEL_ROUTING_MODEL_BYTES} 字节`;
      }
      if (containsControlCharacter(rule.target_model)) return `${label}的目标模型包含控制字符`;
    }
    if (rule.reasoning_effort) {
      if ([...rule.reasoning_effort].length > MAX_MODEL_ROUTING_EFFORT_CHARS) {
        return `${label}的思考强度不能超过 ${MAX_MODEL_ROUTING_EFFORT_CHARS} 个字符`;
      }
      if (containsControlCharacter(rule.reasoning_effort)) {
        return `${label}的思考强度包含控制字符`;
      }
    }
    if (!rule.target_model && !rule.reasoning_effort) {
      return `${label}至少填写目标模型或思考强度`;
    }
  }

  return null;
}
