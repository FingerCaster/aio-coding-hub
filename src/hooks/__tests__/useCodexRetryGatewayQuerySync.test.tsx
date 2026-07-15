import { render, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME } from "../../constants/codexRetryGatewayEvents";
import { gatewayEventNames } from "../../constants/gatewayEvents";
import { codexRetryGatewayKeys } from "../../query/keys";
import { createCodexRetryGatewayStatus } from "../../test/fixtures/codexRetryGateway";
import { emitTauriEvent, tauriListen, tauriUnlisten } from "../../test/mocks/tauri";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import { useCodexRetryGatewayQuerySync } from "../useCodexRetryGatewayQuerySync";

function Harness() {
  useCodexRetryGatewayQuerySync();
  return null;
}

describe("hooks/useCodexRetryGatewayQuerySync", () => {
  it("subscribes to the dedicated event and only accepts monotonic generations", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    client.setQueryData(
      codexRetryGatewayKeys.status(),
      createCodexRetryGatewayStatus({ generation: 7 })
    );

    const { unmount } = render(
      <QueryClientProvider client={client}>
        <Harness />
      </QueryClientProvider>
    );

    await waitFor(() => {
      expect(tauriListen).toHaveBeenCalledWith(
        CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME,
        expect.any(Function)
      );
    });
    expect(tauriListen).not.toHaveBeenCalledWith(gatewayEventNames.status, expect.any(Function));

    emitTauriEvent(
      CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME,
      createCodexRetryGatewayStatus({ generation: 8, runtime_phase: "updating" })
    );
    expect(client.getQueryData<any>(codexRetryGatewayKeys.status())).toMatchObject({
      generation: 8,
      runtime_phase: "updating",
    });

    emitTauriEvent(
      CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME,
      createCodexRetryGatewayStatus({ generation: 6, runtime_phase: "error" })
    );
    expect(client.getQueryData<any>(codexRetryGatewayKeys.status())).toMatchObject({
      generation: 8,
      runtime_phase: "updating",
    });

    unmount();
    expect(tauriUnlisten).toHaveBeenCalled();
  });
});
