import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../generated/bindings";
import {
  nativeChannelCatalogPreview,
  nativeChannelModelsGet,
  nativeChannelModelsDiscover,
  nativeChannelModelsSet,
  nativeChannelPreview,
  nativeChannelApply,
  type ChannelMutationResult,
  type ChannelPreview,
} from "../nativeChannels";
import {
  channelCatalog,
  channelPreview,
  channelDiscovery,
  nativeModel,
} from "../../test/fixtures/native";
import type { GatewayModelsSnapshot } from "../../generated/bindings";

vi.mock("../consoleLog", () => ({
  logToConsole: vi.fn(),
}));

vi.mock("../../generated/bindings", async (original) => {
  const actual = await original<typeof import("../../generated/bindings")>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      nativeChannelCatalogPreview: vi.fn(),
      nativeChannelModelsGet: vi.fn(),
      nativeChannelModelsDiscover: vi.fn(),
      nativeChannelModelsSet: vi.fn(),
      nativeChannelPreview: vi.fn(),
      nativeChannelApply: vi.fn(),
    },
  };
});

const typedCommands = commands as unknown as {
  nativeChannelCatalogPreview: ReturnType<typeof vi.fn>;
  nativeChannelModelsGet: ReturnType<typeof vi.fn>;
  nativeChannelModelsSet: ReturnType<typeof vi.fn>;
  nativeChannelPreview: ReturnType<typeof vi.fn>;
  nativeChannelApply: ReturnType<typeof vi.fn>;
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("nativeChannels service IPC layer", () => {
  it("discovery validates target, provider UUID and protocol without a mutation", async () => {
    const data = channelDiscovery();
    const mock = vi.mocked(commands.nativeChannelModelsDiscover);
    mock.mockResolvedValue({ status: "ok", data });
    expect(
      await nativeChannelModelsDiscover(
        data.targetId,
        data.providerId,
        data.providerUuid,
        data.protocol
      )
    ).toEqual(data);
    for (const patch of [
      { targetId: "wrong" },
      { providerId: 999 },
      { providerUuid: "other" },
      { protocol: "anthropic-messages" as const },
      { revision: "" },
    ]) {
      mock.mockResolvedValue({ status: "ok", data: { ...data, ...patch } });
      await expect(
        nativeChannelModelsDiscover(
          data.targetId,
          data.providerId,
          data.providerUuid,
          data.protocol
        )
      ).rejects.toThrow(/IPC_NATIVE_/);
    }
    expect(commands.nativeChannelModelsSet).not.toHaveBeenCalled();
  });
  it("nativeChannelCatalogPreview validates targetId match and forwards data", async () => {
    const catalog = channelCatalog({ targetId: "pi:local:a" });
    typedCommands.nativeChannelCatalogPreview.mockResolvedValue({
      status: "ok",
      data: catalog,
    });

    const result = await nativeChannelCatalogPreview("pi:local:a");
    expect(result).toEqual(catalog);
    expect(typedCommands.nativeChannelCatalogPreview).toHaveBeenCalledWith("pi:local:a");

    // Mismatched targetId must throw
    typedCommands.nativeChannelCatalogPreview.mockResolvedValue({
      status: "ok",
      data: channelCatalog({ targetId: "wrong-target" }),
    });
    await expect(nativeChannelCatalogPreview("pi:local:a")).rejects.toThrow(
      "IPC_NATIVE_TARGET_MISMATCH"
    );
  });

  it("nativeChannelModelsGet validates provider identity and throws on mismatch", async () => {
    expect(() => nativeChannelModelsGet("pi:local:a", 0, "uuid-1", "openai-responses")).toThrow(
      "SEC_INVALID_INPUT"
    );

    const snapshot: GatewayModelsSnapshot = {
      providerId: 101,
      providerUuid: "uuid-1",
      revision: "rev-1",
      stale: false,
      models: [nativeModel()],
    };
    typedCommands.nativeChannelModelsGet.mockResolvedValue({
      status: "ok",
      data: snapshot,
    });

    const result = await nativeChannelModelsGet("pi:local:a", 101, "uuid-1", "openai-responses");
    expect(result).toEqual(snapshot);

    // Mismatched providerUuid
    typedCommands.nativeChannelModelsGet.mockResolvedValue({
      status: "ok",
      data: {
        ...snapshot,
        providerUuid: "different-uuid",
      },
    });
    await expect(
      nativeChannelModelsGet("pi:local:a", 101, "uuid-1", "openai-responses")
    ).rejects.toThrow("IPC_NATIVE_PROVIDER_MISMATCH");
  });

  it("nativeChannelModelsSet validates provider revision and throws on missing revision", async () => {
    const snapshot: GatewayModelsSnapshot = {
      providerId: 101,
      providerUuid: "uuid-1",
      revision: "rev-2",
      stale: false,
      models: [nativeModel()],
    };
    typedCommands.nativeChannelModelsSet.mockResolvedValue({
      status: "ok",
      data: snapshot,
    });

    const result = await nativeChannelModelsSet(
      "pi:local:a",
      101,
      "uuid-1",
      "openai-responses",
      "rev-1",
      [nativeModel()]
    );
    expect(result).toEqual(snapshot);

    // Missing revision
    typedCommands.nativeChannelModelsSet.mockResolvedValue({
      status: "ok",
      data: {
        ...snapshot,
        revision: "",
      },
    });
    await expect(
      nativeChannelModelsSet("pi:local:a", 101, "uuid-1", "openai-responses", "rev-1", [
        nativeModel(),
      ])
    ).rejects.toThrow("IPC_NATIVE_PROVIDER_MISMATCH");
  });

  it("nativeChannelPreview validates targetId match", async () => {
    const preview: ChannelPreview = channelPreview({ targetId: "pi:local:a" });
    typedCommands.nativeChannelPreview.mockResolvedValue({
      status: "ok",
      data: preview,
    });

    const input = {
      targetId: "pi:local:a",
      expectedRevision: "rev-1",
      catalogRevision: "cat-1",
      selections: [
        {
          sourceChannel: "codex" as const,
          protocol: "openai-responses" as const,
          modelIds: ["gpt-4o"],
        },
      ],
      removeBindingIds: [],
    };

    const result = await nativeChannelPreview(input);
    expect(result).toEqual(preview);
    expect(typedCommands.nativeChannelPreview).toHaveBeenCalledWith(input);

    typedCommands.nativeChannelPreview.mockResolvedValue({
      status: "ok",
      data: channelPreview({ targetId: "wrong-target" }),
    });
    await expect(nativeChannelPreview(input)).rejects.toThrow("IPC_NATIVE_TARGET_MISMATCH");
  });

  it("nativeChannelApply validates targetId match and returns mutation result", async () => {
    const mutationResult: ChannelMutationResult = {
      targetId: "pi:local:a",
      revision: "rev-2",
      changed: true,
      backupPath: "/backup/path",
      bindings: [],
    };
    typedCommands.nativeChannelApply.mockResolvedValue({
      status: "ok",
      data: mutationResult,
    });

    const input = {
      targetId: "pi:local:a",
      expectedRevision: "rev-1",
      catalogRevision: "cat-1",
      selections: [],
      removeBindingIds: ["bind-1"],
    };

    const result = await nativeChannelApply(input);
    expect(result).toEqual(mutationResult);
    expect(typedCommands.nativeChannelApply).toHaveBeenCalledWith(input);

    typedCommands.nativeChannelApply.mockResolvedValue({
      status: "ok",
      data: {
        ...mutationResult,
        targetId: "other-target",
      },
    });
    await expect(nativeChannelApply(input)).rejects.toThrow("IPC_NATIVE_TARGET_MISMATCH");
  });
});
