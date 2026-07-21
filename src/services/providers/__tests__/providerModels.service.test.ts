import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import {
  codexManagedProfileCreate,
  codexManagedProfileDelete,
  codexManagedProfilesList,
} from "../codexManagedProfiles";
import {
  formatProviderModelFeatureError,
  isCodexDirectProvider,
  providerModelCapabilitiesUpdate,
  providerModelManualUpsert,
  providerModelsGet,
} from "../providerModels";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      providerModelsGet: vi.fn(),
      providerModelsRefresh: vi.fn(),
      providerModelManualUpsert: vi.fn(),
      providerModelManualDelete: vi.fn(),
      providerModelCapabilitiesUpdate: vi.fn(),
      codexManagedProfilesList: vi.fn(),
      codexManagedProfileCreate: vi.fn(),
      codexManagedProfileDelete: vi.fn(),
    },
  };
});

const PROVIDER_UUID = "11111111-1111-4111-8111-111111111111";
const MODEL_UUID = "22222222-2222-4222-8222-222222222222";
const PROFILE_UUID = "33333333-3333-4333-8333-333333333333";

function generatedCatalog(overrides: Record<string, unknown> = {}) {
  return {
    providerId: 7,
    providerUuid: PROVIDER_UUID,
    protocol: "openai_compatible",
    stale: false,
    lastAttemptAt: 100,
    lastSuccessAt: 100,
    lastErrorCode: null,
    models: [
      {
        modelUuid: MODEL_UUID,
        providerId: 7,
        remoteModelId: "grok-4.5",
        source: "discovered",
        stale: false,
        lastSeenAt: 100,
        createdAt: 90,
        updatedAt: 100,
        capabilitiesConfigured: true,
        supportedReasoningEfforts: ["low", "medium", "high"],
        defaultReasoningEffort: "medium",
        contextWindow: 128_000,
      },
    ],
    ...overrides,
  };
}

function generatedProfile(overrides: Record<string, unknown> = {}) {
  return {
    profileUuid: PROFILE_UUID,
    profileName: "grok-work",
    modelUuid: MODEL_UUID,
    providerId: 7,
    providerUuid: PROVIDER_UUID,
    providerName: "Grok",
    remoteModelId: "grok-4.5",
    canonicalModel: "aio/grok-work",
    fileStatus: "managed",
    createdAt: 100,
    updatedAt: 100,
    ...overrides,
  };
}

describe("services/providers provider models", () => {
  beforeEach(() => vi.clearAllMocks());

  it("decodes a provider-scoped catalog and keeps model identity", async () => {
    vi.mocked(commands.providerModelsGet).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog() as never,
    });

    await expect(providerModelsGet(7, PROVIDER_UUID)).resolves.toEqual(generatedCatalog());
    expect(commands.providerModelsGet).toHaveBeenCalledWith(7, PROVIDER_UUID);
  });

  it("fails closed when a catalog contains a model from another provider", async () => {
    vi.mocked(commands.providerModelsGet).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog({
        models: [{ ...generatedCatalog().models[0], providerId: 8 }],
      }) as never,
    });

    await expect(providerModelsGet(7, PROVIDER_UUID)).rejects.toThrow(
      "IPC_PROVIDER_MODEL_SCOPE_MISMATCH"
    );
  });

  it("fails closed when a catalog belongs to another provider UUID", async () => {
    vi.mocked(commands.providerModelsGet).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog({
        providerUuid: "44444444-4444-4444-8444-444444444444",
      }) as never,
    });

    await expect(providerModelsGet(7, PROVIDER_UUID)).rejects.toThrow(
      "IPC_PROVIDER_MODEL_SCOPE_MISMATCH"
    );
  });

  it("fails closed on unknown discovery codes and malformed boolean fields", async () => {
    vi.mocked(commands.providerModelsGet)
      .mockResolvedValueOnce({
        status: "ok",
        data: generatedCatalog({ lastErrorCode: "future_code" }) as never,
      })
      .mockResolvedValueOnce({
        status: "ok",
        data: generatedCatalog({ stale: "false" }) as never,
      });

    await expect(providerModelsGet(7, PROVIDER_UUID)).rejects.toThrow(
      "IPC_INVALID_LITERAL: catalog.lastErrorCode=future_code"
    );
    await expect(providerModelsGet(7, PROVIDER_UUID)).rejects.toThrow(
      "IPC_INVALID_BOOLEAN: catalog.stale"
    );
  });

  it("normalizes manual IDs and enforces the backend UTF-8 byte limit", async () => {
    vi.mocked(commands.providerModelManualUpsert).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog() as never,
    });

    await providerModelManualUpsert(7, PROVIDER_UUID, "  grok-4.5  ");
    expect(commands.providerModelManualUpsert).toHaveBeenCalledWith(7, PROVIDER_UUID, "grok-4.5");

    await expect(providerModelManualUpsert(7, PROVIDER_UUID, "界".repeat(86))).rejects.toThrow(
      "invalid remoteModelId"
    );
  });

  it("normalizes and updates model capabilities through the provider-scoped command", async () => {
    vi.mocked(commands.providerModelCapabilitiesUpdate).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog({
        models: [
          {
            ...generatedCatalog().models[0],
            supportedReasoningEfforts: ["minimal", "max"],
            defaultReasoningEffort: "max",
            contextWindow: 1_000_000,
          },
        ],
      }) as never,
    });

    await providerModelCapabilitiesUpdate(7, PROVIDER_UUID, MODEL_UUID, {
      supportedReasoningEfforts: ["max", "minimal"],
      defaultReasoningEffort: "max",
      contextWindow: 1_000_000,
    });
    expect(commands.providerModelCapabilitiesUpdate).toHaveBeenCalledWith(
      7,
      PROVIDER_UUID,
      MODEL_UUID,
      {
        supportedReasoningEfforts: ["minimal", "max"],
        defaultReasoningEffort: "max",
        contextWindow: 1_000_000,
      }
    );
  });

  it("rejects inconsistent capabilities and malformed unconfigured catalog rows", async () => {
    await expect(
      providerModelCapabilitiesUpdate(7, PROVIDER_UUID, MODEL_UUID, {
        supportedReasoningEfforts: ["low"],
        defaultReasoningEffort: "high",
        contextWindow: 128_000,
      })
    ).rejects.toThrow("defaultReasoningEffort is not supported");
    expect(commands.providerModelCapabilitiesUpdate).not.toHaveBeenCalled();

    vi.mocked(commands.providerModelsGet).mockResolvedValueOnce({
      status: "ok",
      data: generatedCatalog({
        models: [
          {
            ...generatedCatalog().models[0],
            capabilitiesConfigured: false,
          },
        ],
      }) as never,
    });
    await expect(providerModelsGet(7, PROVIDER_UUID)).rejects.toThrow(
      "unconfigured model has capability values"
    );
  });

  it("maps only known feature errors and never returns raw error text", () => {
    expect(
      formatProviderModelFeatureError(
        new Error("CODEX_MANAGED_PROFILE_FILE_EXISTS: C:\\Users\\secret.config.toml")
      )
    ).toBe("已存在同名 Profile 文件，未覆盖");
    expect(
      formatProviderModelFeatureError(
        new Error("UNKNOWN_FAILURE: https://example.test/v1?api_key=SYNTHETIC_SECRET")
      )
    ).toBe("请稍后重试");
  });

  it("rejects a malformed expected provider UUID before invoking IPC", async () => {
    await expect(providerModelsGet(7, "not-a-uuid")).rejects.toThrow(
      "IPC_INVALID_UUID: providerUuid"
    );
    expect(commands.providerModelsGet).not.toHaveBeenCalled();
  });

  it("recognizes only direct Codex providers", () => {
    expect(
      isCodexDirectProvider({ cli_key: "codex", source_provider_id: null, bridge_type: null })
    ).toBe(true);
    expect(
      isCodexDirectProvider({
        cli_key: "codex",
        source_provider_id: 2,
        bridge_type: "codex_to_openai_responses",
      })
    ).toBe(false);
    expect(
      isCodexDirectProvider({ cli_key: "claude", source_provider_id: null, bridge_type: null })
    ).toBe(false);
  });
});

describe("services/providers Codex managed profiles", () => {
  beforeEach(() => vi.clearAllMocks());

  it("lists and creates profiles through the singular create command", async () => {
    vi.mocked(commands.codexManagedProfilesList).mockResolvedValueOnce({
      status: "ok",
      data: [generatedProfile()] as never,
    });
    vi.mocked(commands.codexManagedProfileCreate).mockResolvedValueOnce({
      status: "ok",
      data: generatedProfile() as never,
    });

    await expect(codexManagedProfilesList()).resolves.toHaveLength(1);
    await expect(codexManagedProfileCreate(" grok-work ", MODEL_UUID)).resolves.toMatchObject({
      profileName: "grok-work",
      providerId: 7,
    });
    expect(commands.codexManagedProfileCreate).toHaveBeenCalledWith("grok-work", MODEL_UUID);
  });

  it("rejects a profile whose canonical alias does not match its normalized profile name", async () => {
    vi.mocked(commands.codexManagedProfilesList).mockResolvedValueOnce({
      status: "ok",
      data: [generatedProfile({ canonicalModel: "aio/another-model" })] as never,
    });

    await expect(codexManagedProfilesList()).rejects.toThrow("IPC_MANAGED_PROFILE_ALIAS_MISMATCH");
  });

  it("rejects UUID-shaped profile names reserved for legacy model aliases", async () => {
    await expect(codexManagedProfileCreate(MODEL_UUID, MODEL_UUID)).rejects.toThrow(
      "SEC_INVALID_INPUT: invalid profileName"
    );
    expect(commands.codexManagedProfileCreate).not.toHaveBeenCalled();
  });

  it("rejects a profile with a non-canonical provider UUID", async () => {
    vi.mocked(commands.codexManagedProfilesList).mockResolvedValueOnce({
      status: "ok",
      data: [generatedProfile({ providerUuid: "NOT-A-UUID" })] as never,
    });

    await expect(codexManagedProfilesList()).rejects.toThrow(
      "IPC_INVALID_UUID: profile.providerUuid"
    );
  });

  it("returns the external-file-preserved deletion result", async () => {
    vi.mocked(commands.codexManagedProfileDelete).mockResolvedValueOnce({
      status: "ok",
      data: { deleted: true, externalFilePreserved: true },
    });

    await expect(codexManagedProfileDelete(PROFILE_UUID)).resolves.toEqual({
      deleted: true,
      externalFilePreserved: true,
    });
    expect(commands.codexManagedProfileDelete).toHaveBeenCalledWith(PROFILE_UUID);
  });

  it("rejects malformed profile deletion booleans", async () => {
    vi.mocked(commands.codexManagedProfileDelete).mockResolvedValueOnce({
      status: "ok",
      data: { deleted: "true", externalFilePreserved: false } as never,
    });

    await expect(codexManagedProfileDelete(PROFILE_UUID)).rejects.toThrow(
      "IPC_INVALID_BOOLEAN: managedProfileDeleteResult"
    );
  });
});
