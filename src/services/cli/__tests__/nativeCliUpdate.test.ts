import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { nativeCliCheckLatestVersion, nativeCliUpdate } from "../nativeCliUpdate";

vi.mock("../../../generated/bindings", () => ({
  commands: { nativeCliCheckLatestVersion: vi.fn(), nativeCliUpdate: vi.fn() },
}));
vi.mock("../../consoleLog", () => ({ logToConsole: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

describe("native CLI update IPC", () => {
  it("checks versions without issuing an install command", async () => {
    vi.mocked(commands.nativeCliCheckLatestVersion).mockResolvedValue({
      status: "ok",
      data: {
        client: "omp",
        installed: true,
        installedVersion: "18.3.2",
        latestVersion: "18.3.2",
        updateAvailable: false,
        installMethod: "官方独立程序",
        installDirectory: "C:/omp",
        executablePath: "C:/omp/omp.exe",
        planId: null,
        blockedReason: null,
      },
    });
    expect((await nativeCliCheckLatestVersion("omp")).installedVersion).toBe("18.3.2");
    expect(commands.nativeCliUpdate).not.toHaveBeenCalled();
  });
  it("sends only a client-bound opaque plan, not executable paths or shell input", async () => {
    vi.mocked(commands.nativeCliUpdate).mockResolvedValue({
      status: "ok",
      data: { cliKey: "pi", success: true, output: "token=secret", error: null },
    });
    const result = await nativeCliUpdate("pi", "a".repeat(32));
    expect(commands.nativeCliUpdate).toHaveBeenCalledExactlyOnceWith("pi", "a".repeat(32));
    expect(result.output).not.toContain("secret");
  });
  it("rejects invalid plans and mismatched responses", async () => {
    expect(() => nativeCliUpdate("pi", "run command")).toThrow("安装计划无效");
    expect(commands.nativeCliUpdate).not.toHaveBeenCalled();
    vi.mocked(commands.nativeCliUpdate).mockResolvedValue({
      status: "ok",
      data: { cliKey: "omp", success: true, output: "", error: null },
    });
    await expect(nativeCliUpdate("pi", "a".repeat(32))).rejects.toThrow(
      "IPC_NATIVE_CLIENT_MISMATCH"
    );
  });
  it("propagates backend plan expiration without retrying the install", async () => {
    vi.mocked(commands.nativeCliUpdate).mockResolvedValue({
      status: "error",
      error: "安装计划已失效",
    });
    await expect(nativeCliUpdate("omp", "a".repeat(32))).rejects.toThrow("安装计划已失效");
    expect(commands.nativeCliUpdate).toHaveBeenCalledTimes(1);
  });
});
