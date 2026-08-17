import { describe, expect, it } from "vitest";
import {
  GPT56_372K_CONTEXT_PRESET,
  GPT56_372K_CONTEXT_TOKENS,
  MAX_CODEX_MODEL_CONTEXT_RULES,
  CodexModelContextRuleValidationError,
  addGpt56Preset,
  codexModelContextRulesEqual,
  codexModelContextRulesKey,
  decodeCanonicalCodexModelContextRules,
  describeCodexModelContextBase,
  hasEnabledCodexModelContextRules,
  normalizeCodexModelContextRules,
  type CodexModelContextRuleDraftValue,
} from "../codexModelContextRules";

function draftRule(
  modelId: string,
  contextWindow: string | number = "372000",
  enabled = true
): CodexModelContextRuleDraftValue {
  return {
    model_id: modelId,
    context_window: contextWindow,
    enabled,
  };
}

function expectValidationCode(input: unknown, code: string) {
  try {
    normalizeCodexModelContextRules(input);
    throw new Error("expected validation failure");
  } catch (error) {
    expect(error).toBeInstanceOf(CodexModelContextRuleValidationError);
    expect((error as CodexModelContextRuleValidationError).code).toBe(code);
  }
}

describe("codexModelContextRules", () => {
  it("trims, validates, and sorts by UTF-8 bytes", () => {
    expect(
      normalizeCodexModelContextRules([
        draftRule(` ${"\u00e9"} `, "1024", false),
        draftRule("z", 10_000_000),
        draftRule("A", "372000"),
      ])
    ).toEqual([
      { model_id: "A", context_window: 372_000, enabled: true },
      { model_id: "z", context_window: 10_000_000, enabled: true },
      { model_id: "\u00e9", context_window: 1_024, enabled: false },
    ]);
  });

  it("builds one deterministic key and equality projection", () => {
    const left = [draftRule("model-b", "372000"), draftRule("model-a", 1_024, false)];
    const right = [draftRule(" model-a ", "1024", false), draftRule("model-b", 372_000)];

    expect(codexModelContextRulesKey(left)).toBe(codexModelContextRulesKey(right));
    expect(codexModelContextRulesEqual(left, right)).toBe(true);
    expect(codexModelContextRulesEqual(left, [draftRule("model-b", 372_001)])).toBe(false);
    expect(codexModelContextRulesEqual(left, null)).toBe(false);
  });

  it("accepts the empty set and exactly 128 rules but rejects the 129th", () => {
    expect(normalizeCodexModelContextRules([])).toEqual([]);

    const maxRules = Array.from({ length: MAX_CODEX_MODEL_CONTEXT_RULES }, (_, index) =>
      draftRule(`model-${String(index).padStart(3, "0")}`, 1_024, index % 2 === 0)
    );
    expect(normalizeCodexModelContextRules(maxRules)).toHaveLength(MAX_CODEX_MODEL_CONTEXT_RULES);
    expectValidationCode(
      [...maxRules, draftRule("model-over-limit", 1_024)],
      "CODEX_MODEL_CONTEXT_RULE_LIMIT"
    );
  });

  it("enforces UTF-8 byte, control-character, empty, and reserved-ID boundaries", () => {
    expect(normalizeCodexModelContextRules([draftRule("x".repeat(256), 1_024)])).toHaveLength(1);
    expect(normalizeCodexModelContextRules([draftRule("\u00e9".repeat(128), 1_024)])).toHaveLength(
      1
    );

    for (const modelId of [
      "",
      "   ",
      "x".repeat(257),
      "\u00e9".repeat(129),
      "model\u0000id",
      "model\u0085id",
      "aio/profile",
    ]) {
      expectValidationCode([draftRule(modelId, 1_024)], "CODEX_MODEL_CONTEXT_RULE_INVALID");
    }

    expect(normalizeCodexModelContextRules([draftRule("AIO/profile", 1_024)])[0]).toMatchObject({
      model_id: "AIO/profile",
    });
  });

  it("checks duplicates across enabled and disabled rows after trimming but remains case-sensitive", () => {
    expectValidationCode(
      [draftRule("same", 1_024, true), draftRule(" same ", 2_048, false)],
      "CODEX_MODEL_CONTEXT_RULE_DUPLICATE"
    );
    expect(
      normalizeCodexModelContextRules([
        draftRule("Model", 1_024, true),
        draftRule("model", 2_048, false),
      ])
    ).toHaveLength(2);
  });

  it("accepts inclusive token bounds and rejects unsafe or non-decimal inputs", () => {
    expect(
      normalizeCodexModelContextRules([draftRule("min", "1024"), draftRule("max", "10000000")])
    ).toEqual([
      { model_id: "max", context_window: 10_000_000, enabled: true },
      { model_id: "min", context_window: 1_024, enabled: true },
    ]);

    for (const value of [
      "",
      "+1024",
      "-1024",
      "1e4",
      "1024.0",
      "1_024",
      1_023,
      10_000_001,
      1_024.5,
      Number.MAX_SAFE_INTEGER + 1,
      Number.NaN,
      Number.POSITIVE_INFINITY,
      null,
    ]) {
      expectValidationCode(
        [draftRule("model", value as string | number)],
        "CODEX_MODEL_CONTEXT_RULE_INVALID"
      );
    }
  });

  it("requires booleans even for disabled-looking input", () => {
    expectValidationCode(
      [{ model_id: "model", context_window: 1_024, enabled: 0 }],
      "CODEX_MODEL_CONTEXT_RULE_INVALID"
    );
    expectValidationCode(
      [{ model_id: "missing-disabled", context_window: "", enabled: false }],
      "CODEX_MODEL_CONTEXT_RULE_INVALID"
    );
  });

  it("fails closed when enabled-state inspection receives malformed rules", () => {
    expect(hasEnabledCodexModelContextRules([draftRule("enabled", 1_024, true)])).toBe(true);
    expect(hasEnabledCodexModelContextRules([draftRule("disabled", 1_024, false)])).toBe(false);
    expect(() => hasEnabledCodexModelContextRules([{ enabled: true }])).toThrow(
      "CODEX_MODEL_CONTEXT_RULE_INVALID"
    );
  });

  it("keeps the GPT-5.6 372K preset frontend-only and all-or-nothing", () => {
    expect(GPT56_372K_CONTEXT_TOKENS).toBe(372_000);
    expect(GPT56_372K_CONTEXT_PRESET).toEqual([
      { model_id: "gpt-5.6-sol", context_window: 372_000, enabled: true },
      { model_id: "gpt-5.6-terra", context_window: 372_000, enabled: true },
      { model_id: "gpt-5.6-luna", context_window: 372_000, enabled: true },
    ]);

    const existing = [draftRule(" gpt-5.6-sol ", 200_000, false)];
    const result = addGpt56Preset(existing);
    expect(result.added).toBe(2);
    expect(result.skipped).toEqual(["gpt-5.6-sol"]);
    expect(result.draft[0]).toEqual(existing[0]);
    expect(existing).toEqual([draftRule(" gpt-5.6-sol ", 200_000, false)]);

    const nearlyFull = Array.from({ length: 126 }, (_, index) =>
      draftRule(`custom-${index}`, 1_024)
    );
    expect(() => addGpt56Preset(nearlyFull)).toThrow("CODEX_MODEL_CONTEXT_RULE_LIMIT");
    expect(nearlyFull).toHaveLength(126);
  });

  it("describes equal, divergent, partial, unknown, and missing base values", () => {
    expect(
      describeCodexModelContextBase(
        { base_context_window: 272_000, base_max_context_window: 272_000 },
        372_000,
        true
      )
    ).toEqual({
      text: "基础 272,000 -> 目标 372,000",
      increasesBase: true,
      available: true,
    });
    expect(
      describeCodexModelContextBase(
        { base_context_window: 272_000, base_max_context_window: 300_000 },
        200_000,
        true
      )
    ).toMatchObject({
      text: "context 272,000 / max 300,000 -> 目标 200,000",
      increasesBase: false,
      available: true,
    });
    expect(
      describeCodexModelContextBase(
        { base_context_window: null, base_max_context_window: 300_000 },
        372_000,
        false
      )
    ).toMatchObject({
      text: "context 不可用 / max 300,000 -> 目标 372,000",
      increasesBase: false,
    });
    expect(
      describeCodexModelContextBase(
        { base_context_window: null, base_max_context_window: null },
        372_000,
        true
      )
    ).toMatchObject({ available: false, increasesBase: false });
    expect(describeCodexModelContextBase(null, 372_000, true)).toEqual({
      text: "当前基础目录不可用 · 目标 372,000",
      increasesBase: false,
      available: false,
    });
  });

  it("accepts only byte-for-byte canonical backend collections", () => {
    const canonical = [
      { model_id: "model-a", context_window: 1_024, enabled: false },
      { model_id: "model-b", context_window: 372_000, enabled: true },
    ];
    expect(decodeCanonicalCodexModelContextRules(canonical)).toEqual(canonical);

    for (const value of [
      [...canonical].reverse(),
      [{ ...canonical[0], model_id: " model-a " }],
      [{ ...canonical[0], context_window: "1024" }],
      [{ ...canonical[0], extra: true }],
      [{ model_id: "model-a", context_window: 1_024 }],
      null,
    ]) {
      expect(() => decodeCanonicalCodexModelContextRules(value)).toThrow("IPC_INVALID_RESULT");
    }
  });
});
