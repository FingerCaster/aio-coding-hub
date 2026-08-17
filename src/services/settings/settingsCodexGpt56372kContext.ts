import { commands } from "../../generated/bindings";
import type { AppSettings } from "./settings";
import { invokeGeneratedIpc, type GeneratedCommandResult } from "../generatedIpc";
import { normalizeBooleanSetting } from "./settingsPrimitiveValidation";

export async function settingsCodexGpt56372kContextSet(enabled: boolean) {
  const normalizedEnabled = normalizeBooleanSetting(enabled, "codexGpt56372kContextEnabled");

  return invokeGeneratedIpc<AppSettings>({
    title: "保存 Codex GPT-5.6 372K 上下文设置失败",
    cmd: "settings_codex_gpt56_372k_context_set",
    args: { enabled: normalizedEnabled },
    invoke: () =>
      commands.settingsCodexGpt56372kContextSet(normalizedEnabled) as Promise<
        GeneratedCommandResult<AppSettings>
      >,
  });
}
