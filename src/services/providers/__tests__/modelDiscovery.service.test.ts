import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { logToConsole } from "../../consoleLog";
import { providerModelsDiscover, type ProviderModelDiscoveryInput } from "../modelDiscovery";

vi.mock("../../../generated/bindings", () => ({ commands: { providerModelsDiscover: vi.fn() } }));
vi.mock("../../consoleLog", () => ({ logToConsole: vi.fn() }));
const input: ProviderModelDiscoveryInput = {
  providerId: 7,
  cliKey: "codex",
  authMode: "api_key",
  baseUrls: ["https://example.test/v1"],
  baseUrlMode: "order",
  apiKey: "SYNTHETIC_DISCOVERY_KEY",
  sourceProviderId: null,
  bridgeType: null,
};

describe("provider model discovery IPC", () => {
  beforeEach(() => vi.clearAllMocks());
  it("passes saved-provider identity and returns candidates without persistence", async () => {
    const result = {
      status: "ready" as const,
      models: ["gpt-test"],
      origin: "openai",
      base_url_index: 0,
    };
    vi.mocked(commands.providerModelsDiscover).mockResolvedValue({ status: "ok", data: result });
    await expect(providerModelsDiscover(input)).resolves.toEqual(result);
    expect(commands.providerModelsDiscover).toHaveBeenCalledExactlyOnceWith(input);
    expect(logToConsole).not.toHaveBeenCalled();
  });
  it("redacts draft credentials if IPC fails", async () => {
    vi.mocked(commands.providerModelsDiscover).mockResolvedValue({
      status: "error",
      error: "DISCOVERY_FAILED",
    });
    await expect(providerModelsDiscover(input)).rejects.toThrow("DISCOVERY_FAILED");
    expect(logToConsole).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(vi.mocked(logToConsole).mock.calls)).not.toContain(input.apiKey);
  });
  it("rejects a missing IPC result instead of treating it as an empty catalog", async () => {
    vi.mocked(commands.providerModelsDiscover).mockResolvedValue(undefined as never);
    await expect(providerModelsDiscover(input)).rejects.toThrow("IPC_NULL_RESULT");
  });
});
