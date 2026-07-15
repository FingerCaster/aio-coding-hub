import { describe, expect, it } from "vitest";
import {
  buildCliProxySetEnabledResult,
  buildCodexRetryGatewayEnablePlanState,
  buildCodexRetryGatewaySetEnabledState,
  getCodexRetryGatewayStatusState,
  resetMswState,
} from "../test/msw/state";

describe("MSW codex retry gateway state", () => {
  it("builds an enable plan from the current state and turns the gateway on", () => {
    resetMswState();

    const plan = buildCodexRetryGatewayEnablePlanState();
    expect(plan.cli_proxy_enable_required).toBe(true);
    expect(plan.first_download_required).toBe(true);

    const enabled = buildCodexRetryGatewaySetEnabledState({
      enabled: true,
      planGeneration: plan.generation,
      confirmation: {
        acceptedFirstDownload: true,
        acceptedUnreviewedCommit: false,
        acceptedCliProxyEnable: true,
        acceptedProviderSync: true,
        acceptedWslUnprotected: true,
      },
    });
    expect(enabled.runtime_phase).toBe("guarded");
    expect(enabled.cli_proxy_enabled).toBe(true);
    expect(enabled.details_available).toBe(true);
  });

  it("reflects codex cli proxy disablement back into gateway status", () => {
    resetMswState();
    const plan = buildCodexRetryGatewayEnablePlanState();
    buildCodexRetryGatewaySetEnabledState({
      enabled: true,
      planGeneration: plan.generation,
      confirmation: {
        acceptedFirstDownload: true,
        acceptedUnreviewedCommit: false,
        acceptedCliProxyEnable: true,
        acceptedProviderSync: true,
        acceptedWslUnprotected: false,
      },
    });

    buildCliProxySetEnabledResult({ cli_key: "codex", enabled: false });

    const status = getCodexRetryGatewayStatusState();
    expect(status.cli_proxy_enabled).toBe(false);
    expect(status.route_mode).toBe("unproxied");
    expect(status.runtime_phase).toBe("recovery_paused");
  });
});
