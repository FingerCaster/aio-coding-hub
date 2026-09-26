import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../generated/bindings";
import { logToConsole } from "../consoleLog";
import { ompAgentRead, ompAgentSave, ompSettingsSave } from "../ompSettings";

vi.mock("../consoleLog", () => ({ logToConsole: vi.fn() }));
vi.mock("../../generated/bindings", async (original) => {
  const actual = await original<typeof import("../../generated/bindings")>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      ompAgentRead: vi.fn(),
      ompAgentSave: vi.fn(),
      ompSettingsSave: vi.fn(),
    },
  };
});
beforeEach(() => vi.resetAllMocks());
describe("OMP settings IPC", () => {
  it("rejects a mismatched target on mutation results", async () => {
    vi.mocked(commands.ompSettingsSave).mockResolvedValue({
      status: "ok",
      data: { targetId: "other-target", revision: "r2", changed: true, backupPath: null },
    });
    await expect(
      ompSettingsSave({ targetId: "omp-target", expectedRevision: "r1", patches: [] })
    ).rejects.toThrow("IPC_NATIVE_TARGET_MISMATCH");
  });
  it("binds explicit Agent reads to both target and exact file name", async () => {
    vi.mocked(commands.ompAgentRead).mockResolvedValue({
      status: "ok",
      data: {
        targetId: "omp-target",
        fileName: "wrong.md",
        revision: "r1",
        content: "secret prompt",
      },
    });
    await expect(ompAgentRead("omp-target", "agent.md")).rejects.toThrow("IPC_OMP_AGENT_MISMATCH");
    expect(JSON.stringify(vi.mocked(logToConsole).mock.calls)).not.toContain("secret prompt");
  });
  it("does not put prompt contents in diagnostic arguments on failed writes", async () => {
    vi.mocked(commands.ompAgentSave).mockResolvedValue({
      status: "error",
      error: "OMP_SETTINGS_CONFLICT",
    });
    await expect(
      ompAgentSave({
        targetId: "omp-target",
        fileName: "agent.md",
        expectedRevision: "r1",
        content: "PRIVATE_PROMPT_TOKEN",
      })
    ).rejects.toThrow("OMP_SETTINGS_CONFLICT");
    expect(JSON.stringify(vi.mocked(logToConsole).mock.calls)).not.toContain(
      "PRIVATE_PROMPT_TOKEN"
    );
  });
});
