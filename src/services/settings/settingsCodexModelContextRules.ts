import { commands } from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import {
  decodeCanonicalCodexModelContextRules,
  normalizeCodexModelContextRules,
} from "./codexModelContextRules";
import type { AppSettings } from "./settings";

function decodeSettingsResponse(value: unknown): AppSettings {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("IPC_INVALID_RESULT: settings response must be an object");
  }

  const response = value as Record<string, unknown>;
  if (Object.prototype.hasOwnProperty.call(response, "codex_gpt56_372k_context_enabled")) {
    throw new Error("IPC_INVALID_RESULT: settings response contains the retired Codex policy");
  }
  if (!Number.isSafeInteger(response.schema_version) || (response.schema_version as number) < 0) {
    throw new Error("IPC_INVALID_RESULT: settings response has an invalid schema_version");
  }

  const canonicalRules = decodeCanonicalCodexModelContextRules(response.codex_model_context_rules);
  return {
    ...response,
    codex_model_context_rules: canonicalRules,
  } as AppSettings;
}

export async function settingsCodexModelContextRulesSet(rules: unknown) {
  const normalizedRules = normalizeCodexModelContextRules(rules);

  return invokeGeneratedIpc<AppSettings>({
    title: "保存 Codex 模型上下文规则失败",
    cmd: "settings_codex_model_context_rules_set",
    args: { rules: normalizedRules },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.settingsCodexModelContextRulesSet(normalizedRules),
        decodeSettingsResponse
      ),
  });
}
