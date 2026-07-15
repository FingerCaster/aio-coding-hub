import { describe, expect, it } from "vitest";
import {
  formatCodexRetryGatewayProviderSync,
  formatCodexRetryGatewayTone,
  formatCodexRetryGatewayTrustState,
  resolveRepositoryUrl,
} from "../codexRetryGatewayPresentation";

describe("codexRetryGatewayPresentation", () => {
  it("formats trust state and provider sync prompts", () => {
    expect(formatCodexRetryGatewayTrustState("aio_reviewed_recommendation")).toBe("AIO 已审阅推荐");
    expect(
      formatCodexRetryGatewayProviderSync({
        current_provider: "aio-direct",
        target_provider: "aio-codex-gateway",
        change_required: true,
        codex_must_be_closed: true,
      })
    ).toContain("需要先关闭 Codex App");
  });

  it("treats guarded runtime as success tone and resolves repository shorthands", () => {
    expect(
      formatCodexRetryGatewayTone({
        generation: 1,
        desired_enabled: true,
        runtime_phase: "guarded",
        route_mode: "guarded",
        cli_proxy_enabled: true,
        cli_proxy_applied: true,
        effective_port: 37211,
        repository: "FingerCaster/codex-retry-gateway",
        license: "MIT",
        selected_commit: "1",
        active_commit: "1",
        previous_commit: null,
        recommended_commit: "1",
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
          listener: "127.0.0.1:37211",
        },
        update_candidate: null,
        wsl_codex_unprotected: false,
        last_error: null,
        details_available: true,
        operation_pending: false,
      })
    ).toBe("success");

    expect(resolveRepositoryUrl("FingerCaster/codex-retry-gateway")).toBe(
      "https://github.com/FingerCaster/codex-retry-gateway"
    );
    expect(resolveRepositoryUrl("not a repo")).toBeNull();
  });
});
