import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useCodexRetryGatewayApplyCommitMutation,
  useCodexRetryGatewayCheckUpdateMutation,
  useCodexRetryGatewayCreateDetailsSessionMutation,
  useCodexRetryGatewayEnablePlanMutation,
  useCodexRetryGatewayRetryMutation,
  useCodexRetryGatewaySetEnabledMutation,
  useCodexRetryGatewaySetNodeOverrideMutation,
  useCodexRetryGatewayStatusQuery,
  useCodexRetryGatewayUninstallMutation,
  useCodexRetryGatewayValidateCommitMutation,
} from "../../../../query/codexRetryGateway";
import { CodexRetryGatewayManager } from "../CodexRetryGatewayManager";

vi.mock("../../../../query/codexRetryGateway", () => ({
  useCodexRetryGatewayStatusQuery: vi.fn(),
  useCodexRetryGatewayEnablePlanMutation: vi.fn(),
  useCodexRetryGatewaySetEnabledMutation: vi.fn(),
  useCodexRetryGatewayCheckUpdateMutation: vi.fn(),
  useCodexRetryGatewayValidateCommitMutation: vi.fn(),
  useCodexRetryGatewayApplyCommitMutation: vi.fn(),
  useCodexRetryGatewaySetNodeOverrideMutation: vi.fn(),
  useCodexRetryGatewayRetryMutation: vi.fn(),
  useCodexRetryGatewayUninstallMutation: vi.fn(),
  useCodexRetryGatewayCreateDetailsSessionMutation: vi.fn(),
}));

vi.mock("../../../../services/desktop/dialog", () => ({
  openDesktopSinglePath: vi.fn(),
}));

vi.mock("../../../../services/desktop/opener", () => ({
  openDesktopUrl: vi.fn(),
}));

vi.mock("../../../../services/consoleLog", () => ({
  logToConsole: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: vi.fn(),
}));

function createMutation(
  overrides: Partial<{ isPending: boolean; mutateAsync: ReturnType<typeof vi.fn> }> = {}
) {
  return {
    isPending: false,
    mutateAsync: vi.fn(),
    ...overrides,
  } as any;
}

const baseStatus = {
  generation: 7,
  desired_enabled: true,
  runtime_phase: "guarded" as const,
  route_mode: "guarded" as const,
  cli_proxy_enabled: true,
  cli_proxy_applied: true,
  effective_port: 37211,
  repository: "FingerCaster/aio-codex-gateway",
  license: "MIT",
  selected_commit: "1111111111111111111111111111111111111111",
  active_commit: "1111111111111111111111111111111111111111",
  previous_commit: "0000000000000000000000000000000000000000",
  recommended_commit: "2222222222222222222222222222222222222222",
  trust_state: "aio_reviewed_recommendation" as const,
  node_status: {
    available: true,
    executable: "C:/Program Files/nodejs/node.exe",
    version: "v22.15.0",
    source: "manual_override" as const,
    error: null,
  },
  process_status: {
    phase: "healthy" as const,
    owned: true,
    healthy: true,
    process_id: 47211,
    listener: "127.0.0.1:37211",
  },
  update_candidate: null,
  wsl_codex_unprotected: true,
  last_error: null,
  details_available: true,
  operation_pending: false,
};

describe("components/cli-manager/tabs/CodexRetryGatewayManager", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    vi.mocked(useCodexRetryGatewayStatusQuery).mockReturnValue({
      data: baseStatus,
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    } as any);

    vi.mocked(useCodexRetryGatewayEnablePlanMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewaySetEnabledMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewayCheckUpdateMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewayValidateCommitMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewayApplyCommitMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewaySetNodeOverrideMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewayRetryMutation).mockReturnValue(createMutation());
    vi.mocked(useCodexRetryGatewayUninstallMutation).mockReturnValue(createMutation());
  });

  it("creates the details bridge session only once per rendered generation", async () => {
    const mutateAsync = vi.fn().mockResolvedValue({
      generation: 7,
      iframe_url: "http://127.0.0.1:37211/iframe?generation=7",
      browser_url: "http://127.0.0.1:37211/browser?generation=7",
      expires_at_ms: Date.now() + 30_000,
    });

    vi.mocked(useCodexRetryGatewayCreateDetailsSessionMutation).mockReturnValue(
      createMutation({ mutateAsync })
    );

    render(<CodexRetryGatewayManager showDetailsFrame />);

    await waitFor(() => expect(screen.getByTitle("Codex 外部网关管理页")).toBeInTheDocument());
    expect(mutateAsync).toHaveBeenCalledTimes(1);
  });
});
