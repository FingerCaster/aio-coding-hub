import { commands } from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import { decodeCanonicalCodexModelContextRules } from "./codexModelContextRules";
import type { AppSettings } from "./settings";

function decodeSettingsResponse(value: unknown): AppSettings {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("IPC_INVALID_RESULT: settings response must be an object");
  }

  const response = value as Record<string, unknown>;
  if (!Number.isSafeInteger(response.schema_version) || (response.schema_version as number) < 0) {
    throw new Error("IPC_INVALID_RESULT: settings response has an invalid schema_version");
  }
  if (
    response.codex_managed_model_catalog_enabled != null &&
    typeof response.codex_managed_model_catalog_enabled !== "boolean"
  ) {
    throw new Error("IPC_INVALID_RESULT: codex_managed_model_catalog_enabled must be a boolean");
  }

  const canonicalRules = decodeCanonicalCodexModelContextRules(response.codex_model_context_rules);
  return {
    ...response,
    codex_model_context_rules: canonicalRules,
    codex_managed_model_catalog_enabled:
      response.codex_managed_model_catalog_enabled == null
        ? null
        : response.codex_managed_model_catalog_enabled,
  } as AppSettings;
}

export async function settingsCodexManagedModelCatalogSet(enabled: boolean) {
  return invokeGeneratedIpc<AppSettings>({
    title: "切换本地 model_catalog_json 托管失败",
    cmd: "settings_codex_managed_model_catalog_set",
    args: { enabled },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.settingsCodexManagedModelCatalogSet(enabled),
        decodeSettingsResponse
      ),
  });
}
