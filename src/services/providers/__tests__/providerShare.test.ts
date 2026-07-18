import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type ProviderShareImportPreview } from "../../../generated/bindings";
import { logToConsole } from "../../consoleLog";
import {
  decodeProviderShareImportPreview,
  providerShareCopyToClipboard,
  providerShareImportConfirm,
  providerShareImportPreviewFromContent,
  providerShareImportPreviewFromFile,
  providerShareSaveToFile,
} from "../providerShare";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      providerShareCopyToClipboard: vi.fn(),
      providerShareSaveToFile: vi.fn(),
      providerShareImportPreviewFromFile: vi.fn(),
      providerShareImportPreviewFromContent: vi.fn(),
      providerShareImportConfirm: vi.fn(),
    },
  };
});

vi.mock("../../consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../consoleLog")>("../../consoleLog");
  return { ...actual, logToConsole: vi.fn() };
});

const TOKEN = "a".repeat(64);

function preview(overrides: Partial<ProviderShareImportPreview> = {}): ProviderShareImportPreview {
  return {
    previewToken: TOKEN,
    cliKey: "claude",
    sourceName: "Source",
    finalName: "Source 副本",
    sourceEnabled: true,
    importEnabled: false,
    authMode: "api_key",
    credentialStatus: "configured",
    extensionCount: 0,
    extensions: [],
    canImport: true,
    ...overrides,
  };
}

describe("services/providers/providerShare", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("passes pasted content only to the invoke closure and redacts diagnostics", async () => {
    const secret = `SYNTHETIC_PROVIDER_SHARE_${crypto.randomUUID()}`;
    vi.mocked(commands.providerShareImportPreviewFromContent).mockRejectedValueOnce(
      new Error("preview failed")
    );

    await expect(providerShareImportPreviewFromContent(secret)).rejects.toThrow("preview failed");

    expect(commands.providerShareImportPreviewFromContent).toHaveBeenCalledWith(secret);
    const logged = JSON.stringify(vi.mocked(logToConsole).mock.calls);
    expect(logged).not.toContain(secret);
    expect(logged).toContain("[REDACTED]");
    expect(logged).toContain(String(new TextEncoder().encode(secret).byteLength));
  });

  it("uses provider-scoped export confirmation without logging the confirmation payload", async () => {
    vi.mocked(commands.providerShareCopyToClipboard).mockResolvedValueOnce({
      status: "ok",
      data: true,
    });

    await expect(providerShareCopyToClipboard(42)).resolves.toBe(true);

    expect(commands.providerShareCopyToClipboard).toHaveBeenCalledWith(
      42,
      expect.objectContaining({
        confirm: expect.objectContaining({
          action: "provider_share_copy_to_clipboard",
          resource: "provider:42:share",
          nonce: expect.any(String),
        }),
      })
    );
    expect(logToConsole).not.toHaveBeenCalled();
  });

  it("uses the save-specific confirmation action and provider-scoped resource", async () => {
    vi.mocked(commands.providerShareSaveToFile).mockResolvedValueOnce({
      status: "ok",
      data: true,
    });

    await expect(providerShareSaveToFile(42)).resolves.toBe(true);

    expect(commands.providerShareSaveToFile).toHaveBeenCalledWith(
      42,
      expect.objectContaining({
        confirm: expect.objectContaining({
          action: "provider_share_save_to_file",
          resource: "provider:42:share",
          nonce: expect.any(String),
        }),
      })
    );
    expect(logToConsole).not.toHaveBeenCalled();
  });

  it("returns null when the file picker is cancelled", async () => {
    vi.mocked(commands.providerShareImportPreviewFromFile).mockResolvedValueOnce({
      status: "ok",
      data: null,
    });

    await expect(providerShareImportPreviewFromFile()).resolves.toBeNull();
    expect(commands.providerShareImportPreviewFromFile).toHaveBeenCalledOnce();
  });

  it("never places the real preview token in confirm failure diagnostics", async () => {
    vi.mocked(commands.providerShareImportConfirm).mockRejectedValueOnce(
      new Error("confirm failed")
    );

    await expect(providerShareImportConfirm(TOKEN)).rejects.toThrow("confirm failed");

    expect(commands.providerShareImportConfirm).toHaveBeenCalledWith(
      TOKEN,
      expect.objectContaining({
        confirm: expect.objectContaining({
          action: "provider_share_import_confirm",
          resource: `provider-share-preview:${TOKEN}`,
        }),
      })
    );
    const logged = JSON.stringify(vi.mocked(logToConsole).mock.calls);
    expect(logged).not.toContain(TOKEN);
    expect(logged).toContain("[REDACTED]");
  });

  it("rejects an inconsistent canImport projection", () => {
    expect(() =>
      decodeProviderShareImportPreview(
        preview({
          extensionCount: 1,
          extensions: [
            {
              pluginId: "example.plugin",
              namespace: "providerConfig",
              requiredVersion: "1.0.0",
              installedVersion: null,
              compatibility: "missing_plugin",
            },
          ],
          canImport: true,
        })
      )
    ).toThrow("canImport is inconsistent");
  });

  it("decodes a valid strict preview", () => {
    expect(decodeProviderShareImportPreview(preview()).cliKey).toBe("claude");
  });
});
