import { describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { logToConsole } from "../../consoleLog";
import { settingsCodexGpt56372kContextSet } from "../settingsCodexGpt56372kContext";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      settingsCodexGpt56372kContextSet: vi.fn(),
    },
  };
});

vi.mock("../../consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../consoleLog")>("../../consoleLog");
  return {
    ...actual,
    logToConsole: vi.fn(),
  };
});

describe("services/settings/settingsCodexGpt56372kContext", () => {
  it("forwards the dedicated boolean argument", async () => {
    vi.mocked(commands.settingsCodexGpt56372kContextSet).mockResolvedValueOnce({
      status: "ok",
      data: { codex_gpt56_372k_context_enabled: true } as any,
    });

    await settingsCodexGpt56372kContextSet(true);

    expect(commands.settingsCodexGpt56372kContextSet).toHaveBeenCalledWith(true);
  });

  it("rethrows invoke failures and rejects null responses", async () => {
    vi.mocked(commands.settingsCodexGpt56372kContextSet).mockRejectedValueOnce(
      new Error("catalog sync failed")
    );

    await expect(settingsCodexGpt56372kContextSet(true)).rejects.toThrow("catalog sync failed");
    expect(logToConsole).toHaveBeenCalledWith(
      "error",
      "保存 Codex GPT-5.6 372K 上下文设置失败",
      expect.objectContaining({
        cmd: "settings_codex_gpt56_372k_context_set",
        error: expect.stringContaining("catalog sync failed"),
      })
    );

    vi.mocked(commands.settingsCodexGpt56372kContextSet).mockResolvedValueOnce(null as any);
    await expect(settingsCodexGpt56372kContextSet(false)).rejects.toThrow(
      "IPC_NULL_RESULT: settings_codex_gpt56_372k_context_set"
    );
  });

  it("rejects malformed input before invoking the backend", async () => {
    await expect(settingsCodexGpt56372kContextSet("yes" as any)).rejects.toThrow(
      "codexGpt56372kContextEnabled must be a boolean"
    );

    expect(commands.settingsCodexGpt56372kContextSet).not.toHaveBeenCalled();
  });
});
