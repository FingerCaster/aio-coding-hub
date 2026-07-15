import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import {
  codexRetryGatewayCheckUpdate,
  codexRetryGatewaySetEnabled,
  codexRetryGatewayValidateCommit,
} from "../codexRetryGateway";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      codexRetryGatewayCheckUpdate: vi.fn(),
      codexRetryGatewaySetEnabled: vi.fn(),
      codexRetryGatewayValidateCommit: vi.fn(),
    },
  };
});

describe("services/cli/codexRetryGateway", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("normalizes setEnabled requests before invoking generated commands", async () => {
    vi.mocked(commands.codexRetryGatewaySetEnabled).mockResolvedValueOnce({
      status: "ok",
      data: {
        generation: 8,
        desired_enabled: true,
        runtime_phase: "guarded",
        route_mode: "guarded",
        cli_proxy_enabled: true,
        cli_proxy_applied: true,
        effective_port: 4610,
        repository: "nonononull/codex-retry-gateway",
        license: null,
        selected_commit: "1111111111111111111111111111111111111111",
        active_commit: "1111111111111111111111111111111111111111",
        previous_commit: null,
        recommended_commit: "1111111111111111111111111111111111111111",
        trust_state: "aio_reviewed_recommendation",
        node_status: {
          available: true,
          executable: "node",
          version: "20.12.2",
          source: "aio_discovery",
          error: null,
        },
        process_status: {
          phase: "healthy",
          owned: true,
          healthy: true,
          process_id: 1,
          listener: "http://127.0.0.1:4610",
        },
        update_candidate: null,
        wsl_codex_unprotected: false,
        last_error: null,
        details_available: true,
        operation_pending: false,
      },
    });

    await codexRetryGatewaySetEnabled({
      enabled: true,
      planGeneration: 7,
      confirmation: {
        acceptedFirstDownload: 1 as never,
        acceptedUnreviewedCommit: 0 as never,
        acceptedCliProxyEnable: true,
        acceptedProviderSync: false,
        acceptedWslUnprotected: "yes" as never,
      },
    });

    expect(commands.codexRetryGatewaySetEnabled).toHaveBeenCalledWith({
      enabled: true,
      planGeneration: 7,
      confirmation: {
        acceptedFirstDownload: true,
        acceptedUnreviewedCommit: false,
        acceptedCliProxyEnable: true,
        acceptedProviderSync: false,
        acceptedWslUnprotected: true,
      },
    });
  });

  it("returns null when update check succeeds with null payload", async () => {
    vi.mocked(commands.codexRetryGatewayCheckUpdate).mockResolvedValueOnce({
      status: "ok",
      data: null,
    });

    await expect(codexRetryGatewayCheckUpdate()).resolves.toBeNull();
  });

  it("rejects invalid commit inputs before invoking generated commands", async () => {
    await expect(codexRetryGatewayValidateCommit("   ")).rejects.toThrow("SEC_INVALID_INPUT");
    expect(commands.codexRetryGatewayValidateCommit).not.toHaveBeenCalled();
  });
});
