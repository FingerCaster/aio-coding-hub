import { describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { createTestAppSettings } from "../../../test/fixtures/settings";
import * as consoleLog from "../../consoleLog";
import type { CodexModelContextRule } from "../codexModelContextRules";
import { settingsCodexModelContextRulesSet } from "../settingsCodexModelContextRules";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      settingsCodexModelContextRulesSet: vi.fn(),
    },
  };
});

const ruleCommand = () => vi.mocked(commands.settingsCodexModelContextRulesSet);

function canonicalSettings(rules: CodexModelContextRule[]) {
  return createTestAppSettings({
    schema_version: 65,
    codex_model_context_rules: rules,
  });
}

describe("services/settings/settingsCodexModelContextRules", () => {
  it("normalizes one complete collection before invoking the dedicated command", async () => {
    const canonicalRules = [
      { model_id: "model-a", context_window: 1_024, enabled: false },
      { model_id: "model-b", context_window: 372_000, enabled: true },
    ];
    ruleCommand().mockResolvedValueOnce({
      status: "ok",
      data: canonicalSettings(canonicalRules),
    });

    await expect(
      settingsCodexModelContextRulesSet([
        { model_id: " model-b ", context_window: "372000", enabled: true },
        { model_id: "model-a", context_window: 1_024, enabled: false },
      ])
    ).resolves.toMatchObject({ codex_model_context_rules: canonicalRules });

    expect(ruleCommand()).toHaveBeenCalledTimes(1);
    expect(ruleCommand()).toHaveBeenCalledWith(canonicalRules);
  });

  it("rethrows invoke failures and rejects null responses", async () => {
    const logSpy = vi.spyOn(consoleLog, "logToConsole").mockImplementation(() => undefined);
    ruleCommand().mockRejectedValueOnce(new Error("catalog sync failed"));

    await expect(settingsCodexModelContextRulesSet([])).rejects.toThrow("catalog sync failed");
    expect(logSpy).toHaveBeenCalledWith(
      "error",
      "保存 Codex 模型上下文规则失败",
      expect.objectContaining({
        cmd: "settings_codex_model_context_rules_set",
        error: expect.stringContaining("catalog sync failed"),
      })
    );

    ruleCommand().mockResolvedValueOnce(null as never);
    await expect(settingsCodexModelContextRulesSet([])).rejects.toThrow(
      "IPC_NULL_RESULT: settings_codex_model_context_rules_set"
    );
  });

  it("rejects malformed input before invoking the backend", async () => {
    await expect(settingsCodexModelContextRulesSet("rules")).rejects.toThrow(
      "CODEX_MODEL_CONTEXT_RULE_INVALID"
    );
    await expect(
      settingsCodexModelContextRulesSet([
        { model_id: "same", context_window: 1_024, enabled: true },
        { model_id: " same ", context_window: 2_048, enabled: false },
      ])
    ).rejects.toThrow("CODEX_MODEL_CONTEXT_RULE_DUPLICATE");

    expect(ruleCommand()).not.toHaveBeenCalled();
  });

  it("fails closed on malformed or non-canonical settings confirmations", async () => {
    const validRule = { model_id: "model", context_window: 1_024, enabled: true };
    const malformedResponses = [
      { schema_version: 65 },
      canonicalSettings([{ ...validRule, model_id: " model " }]),
      canonicalSettings([{ ...validRule, context_window: "1024" as unknown as number }]),
      canonicalSettings([{ ...validRule, extra: true } as CodexModelContextRule]),
      {
        ...canonicalSettings([validRule]),
        codex_gpt56_372k_context_enabled: false,
      },
      {
        ...canonicalSettings([validRule]),
        schema_version: Number.NaN,
      },
    ];

    for (const response of malformedResponses) {
      ruleCommand().mockResolvedValueOnce({
        status: "ok",
        data: response as never,
      });
      await expect(settingsCodexModelContextRulesSet([validRule])).rejects.toThrow(
        "IPC_INVALID_RESULT"
      );
    }
  });
});
