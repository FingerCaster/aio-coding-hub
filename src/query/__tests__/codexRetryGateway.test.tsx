import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  codexRetryGatewaySetEnabled,
  codexRetryGatewayStatus,
  codexRetryGatewaySetNodeOverride,
} from "../../services/cli/codexRetryGateway";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import { cliProxyKeys, codexRetryGatewayKeys } from "../keys";
import {
  useCodexRetryGatewaySetEnabledMutation,
  useCodexRetryGatewaySetNodeOverrideMutation,
  useCodexRetryGatewayStatusQuery,
} from "../codexRetryGateway";

vi.mock("../../services/cli/codexRetryGateway", async () => {
  const actual = await vi.importActual<typeof import("../../services/cli/codexRetryGateway")>(
    "../../services/cli/codexRetryGateway"
  );
  return {
    ...actual,
    codexRetryGatewayStatus: vi.fn(),
    codexRetryGatewaySetEnabled: vi.fn(),
    codexRetryGatewaySetNodeOverride: vi.fn(),
  };
});

describe("query/codexRetryGateway", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps the newer cached generation when the query returns an older snapshot", async () => {
    setTauriRuntime();
    vi.mocked(codexRetryGatewayStatus).mockResolvedValue({
      generation: 4,
      desired_enabled: false,
      runtime_phase: "disabled",
      route_mode: "direct_aio",
      cli_proxy_enabled: false,
      cli_proxy_applied: false,
      effective_port: null,
      repository: "nonononull/codex-retry-gateway",
      license: null,
      selected_commit: "1111111111111111111111111111111111111111",
      active_commit: null,
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
        phase: "stopped",
        owned: false,
        healthy: false,
        process_id: null,
        listener: null,
      },
      update_candidate: null,
      wsl_codex_unprotected: false,
      last_error: null,
      details_available: false,
      operation_pending: false,
    });

    const client = createTestQueryClient();
    client.setQueryData(codexRetryGatewayKeys.status(), {
      generation: 5,
      desired_enabled: true,
      runtime_phase: "guarded",
      route_mode: "guarded",
      cli_proxy_enabled: true,
      cli_proxy_applied: true,
      effective_port: 4610,
      repository: "nonononull/codex-retry-gateway",
      license: null,
      selected_commit: "5555555555555555555555555555555555555555",
      active_commit: "5555555555555555555555555555555555555555",
      previous_commit: null,
      recommended_commit: "5555555555555555555555555555555555555555",
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
    });
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCodexRetryGatewayStatusQuery(), { wrapper });

    await waitFor(() => {
      expect(result.current.data?.generation).toBe(5);
    });
  });

  it("writes the returned gateway status into cache after enable/disable mutations", async () => {
    setTauriRuntime();
    vi.mocked(codexRetryGatewaySetEnabled).mockResolvedValue({
      generation: 6,
      desired_enabled: true,
      runtime_phase: "guarded",
      route_mode: "guarded",
      cli_proxy_enabled: true,
      cli_proxy_applied: true,
      effective_port: 4610,
      repository: "nonononull/codex-retry-gateway",
      license: null,
      selected_commit: "6666666666666666666666666666666666666666",
      active_commit: "6666666666666666666666666666666666666666",
      previous_commit: null,
      recommended_commit: "6666666666666666666666666666666666666666",
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
    });

    const client = createTestQueryClient();
    client.setQueryData(cliProxyKeys.statusAll(), []);
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useCodexRetryGatewaySetEnabledMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        enabled: true,
        planGeneration: 5,
        confirmation: {
          acceptedFirstDownload: true,
          acceptedUnreviewedCommit: false,
          acceptedCliProxyEnable: true,
          acceptedProviderSync: true,
          acceptedWslUnprotected: false,
        },
      });
    });

    expect(client.getQueryData<any>(codexRetryGatewayKeys.status())?.generation).toBe(6);
  });

  it("only patches node status when the cached generation matches the mutation request", async () => {
    setTauriRuntime();
    vi.mocked(codexRetryGatewaySetNodeOverride).mockResolvedValue({
      available: true,
      executable: "C:\\custom\\node.exe",
      version: "20.99.0",
      source: "manual_override",
      error: null,
    });

    const client = createTestQueryClient();
    client.setQueryData(codexRetryGatewayKeys.status(), {
      generation: 10,
      desired_enabled: true,
      runtime_phase: "guarded",
      route_mode: "guarded",
      cli_proxy_enabled: true,
      cli_proxy_applied: true,
      effective_port: 4610,
      repository: "nonononull/codex-retry-gateway",
      license: null,
      selected_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      active_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      previous_commit: null,
      recommended_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
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
    });
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useCodexRetryGatewaySetNodeOverrideMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        generation: 9,
        executable: "C:\\custom\\node.exe",
      });
    });

    expect(client.getQueryData<any>(codexRetryGatewayKeys.status())?.node_status.executable).toBe(
      "node"
    );
  });
});
