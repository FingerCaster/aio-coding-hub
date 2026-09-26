import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../generated/bindings";
import { logToConsole } from "../consoleLog";
import {
  nativeCliProviderReadForEdit,
  nativeCliProviderSave,
  nativeCliProvidersList,
  nativeCliTargetValidate,
} from "../nativeCli";
import {
  nativeGatewayCatalogPreview,
  nativeGatewayImportConfirm,
  nativeGatewayModelsGet,
  nativeGatewayModelsSet,
} from "../nativeGateway";
import {
  gatewayPreview,
  nativeEdit,
  nativeList,
  nativeModel,
  nativeTarget,
} from "../../test/fixtures/native";
vi.mock("../consoleLog", () => ({ logToConsole: vi.fn() }));
vi.mock("../../generated/bindings", async (original) => {
  const actual = await original<typeof import("../../generated/bindings")>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      nativeCliProviderReadForEdit: vi.fn(),
      nativeCliProviderSave: vi.fn(),
      nativeCliProvidersList: vi.fn(),
      nativeCliTargetValidate: vi.fn(),
      nativeGatewayModelsGet: vi.fn(),
      nativeGatewayModelsSet: vi.fn(),
      nativeGatewayImportConfirm: vi.fn(),
      nativeGatewayCatalogPreview: vi.fn(),
    },
  };
});
beforeEach(() => vi.clearAllMocks());
describe("native generated IPC services", () => {
  it("rejects cross-target results instead of adopting foreign configuration", async () => {
    vi.mocked(commands.nativeCliProvidersList).mockResolvedValue({
      status: "ok",
      data: nativeList({ target: nativeTarget({ targetId: "wrong-target" }) }),
    });
    await expect(nativeCliProvidersList("pi:local:a")).rejects.toThrow(
      "IPC_NATIVE_TARGET_MISMATCH"
    );
    vi.mocked(commands.nativeGatewayCatalogPreview).mockResolvedValue({
      status: "ok",
      data: gatewayPreview({ targetId: "wrong-target" }),
    });
    await expect(nativeGatewayCatalogPreview("pi:local:a")).rejects.toThrow(
      "IPC_NATIVE_TARGET_MISMATCH"
    );
  });
  it("rejects a native target validation result for another CLI", async () => {
    vi.mocked(commands.nativeCliTargetValidate).mockResolvedValue({
      status: "ok",
      data: nativeTarget({ client: "omp" }),
    });
    await expect(
      nativeCliTargetValidate({ client: "pi", mode: "default", agentDir: null, profile: null })
    ).rejects.toThrow("IPC_NATIVE_TARGET_MISMATCH");
  });
  it("reads a secret-bearing node only through explicit edit IPC", async () => {
    vi.mocked(commands.nativeCliProviderReadForEdit).mockResolvedValue({
      status: "ok",
      data: nativeEdit(),
    });
    expect(await nativeCliProviderReadForEdit("pi:local:a", "vendor")).toEqual(nativeEdit());
    expect(commands.nativeCliProviderReadForEdit).toHaveBeenCalledWith("pi:local:a", "vendor");
    expect(logToConsole).not.toHaveBeenCalled();
  });
  it("never includes raw native nodes, patches or imported credentials in diagnostics", async () => {
    vi.mocked(commands.nativeCliProviderSave).mockResolvedValue({
      status: "error",
      error: "NATIVE_CONFLICT",
    });
    await expect(
      nativeCliProviderSave({
        targetId: "pi:local:a",
        nativeKey: "vendor",
        displayName: "Vendor",
        expectedRevision: "file-1",
        expectedNodeDigest: null,
        expectedProfileRevision: null,
        node: nativeEdit().node,
        patch: [{ path: ["apiKey"], value: "unique-secret" }],
        apply: false,
      })
    ).rejects.toThrow();
    vi.mocked(commands.nativeGatewayImportConfirm).mockResolvedValue({
      status: "error",
      error: "NATIVE_CONFLICT",
    });
    await expect(
      nativeGatewayImportConfirm({
        targetId: "pi:local:a",
        nativeKey: "vendor",
        expectedRevision: "file-1",
        credentials: [{ groupId: "group-a", apiKey: "unique-secret" }],
      })
    ).rejects.toThrow();
    expect(logToConsole).toHaveBeenCalledTimes(2);
    expect(JSON.stringify(vi.mocked(logToConsole).mock.calls)).not.toMatch(
      /unique-secret|read-secret|secret-host|private/
    );
    for (const call of vi.mocked(logToConsole).mock.calls)
      expect(call[2]).toEqual(expect.objectContaining({ args: undefined }));
  });
  it("binds model reads and CAS writes to provider UUID and rejects reused ids", async () => {
    const snapshot = {
      providerId: 7,
      providerUuid: "uuid-current",
      revision: "models-1",
      stale: false,
      models: [nativeModel()],
    };
    vi.mocked(commands.nativeGatewayModelsGet).mockResolvedValue({ status: "ok", data: snapshot });
    await expect(nativeGatewayModelsGet(7, "uuid-current")).resolves.toEqual(snapshot);
    expect(commands.nativeGatewayModelsGet).toHaveBeenCalledWith(7, "uuid-current");
    vi.mocked(commands.nativeGatewayModelsSet).mockResolvedValue({
      status: "ok",
      data: { ...snapshot, revision: "models-2" },
    });
    await nativeGatewayModelsSet(7, "uuid-current", "models-1", snapshot.models);
    expect(commands.nativeGatewayModelsSet).toHaveBeenCalledWith(
      7,
      "uuid-current",
      "models-1",
      snapshot.models
    );
    await expect(nativeGatewayModelsGet(7, "uuid-stale")).rejects.toThrow(
      "IPC_NATIVE_PROVIDER_MISMATCH"
    );
  });
});
