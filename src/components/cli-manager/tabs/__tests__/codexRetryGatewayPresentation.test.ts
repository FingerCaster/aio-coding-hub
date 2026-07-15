import { describe, expect, it } from "vitest";
import {
  formatCodexRetryGatewayNodeSource,
  formatCodexRetryGatewayProviderSync,
  formatCodexRetryGatewayRouteMode,
  formatCodexRetryGatewayRuntimePhase,
  formatCodexRetryGatewayTone,
  formatCodexRetryGatewayTrustState,
  resolveRepositoryUrl,
} from "../codexRetryGatewayPresentation";
import {
  CODEX_RETRY_GATEWAY_ERROR_CATEGORIES,
  CODEX_RETRY_GATEWAY_NODE_SOURCES,
  CODEX_RETRY_GATEWAY_PROCESS_PHASES,
  CODEX_RETRY_GATEWAY_ROUTE_MODES,
  CODEX_RETRY_GATEWAY_RUNTIME_PHASES,
  CODEX_RETRY_GATEWAY_TRUST_STATES,
  createCodexRetryGatewayStatus,
} from "../../../../test/fixtures/codexRetryGateway";

describe("codexRetryGatewayPresentation", () => {
  it("formats trust state and provider sync prompts", () => {
    expect(formatCodexRetryGatewayTrustState("aio_reviewed_recommendation")).toBe("AIO 已审阅推荐");
    expect(
      formatCodexRetryGatewayProviderSync({
        current_provider: "OpenAI",
        target_provider: "aio",
        change_required: true,
        codex_must_be_closed: true,
      })
    ).toBe("OpenAI -> aio；会同步会话与 Provider 状态、写入备份，需要先关闭 Codex App。");
  });

  it("treats guarded runtime as success tone and resolves repository shorthands", () => {
    expect(formatCodexRetryGatewayTone(createCodexRetryGatewayStatus())).toBe("success");

    expect(resolveRepositoryUrl("nonononull/codex-retry-gateway")).toBe(
      "https://github.com/nonononull/codex-retry-gateway"
    );
    expect(resolveRepositoryUrl("not a repo")).toBeNull();
  });

  it("keeps exhaustive fixtures for every generated foundation enum", () => {
    expect(CODEX_RETRY_GATEWAY_RUNTIME_PHASES).toHaveLength(11);
    expect(CODEX_RETRY_GATEWAY_ROUTE_MODES).toEqual(["unproxied", "direct_aio", "guarded"]);
    expect(CODEX_RETRY_GATEWAY_TRUST_STATES).toEqual([
      "unavailable",
      "aio_reviewed_recommendation",
      "official_main_unreviewed",
    ]);
    expect(CODEX_RETRY_GATEWAY_NODE_SOURCES).toHaveLength(5);
    expect(CODEX_RETRY_GATEWAY_PROCESS_PHASES).toEqual([
      "stopped",
      "starting",
      "healthy",
      "unhealthy",
      "ownership_mismatch",
    ]);
    expect(CODEX_RETRY_GATEWAY_ERROR_CATEGORIES).toHaveLength(14);

    for (const phase of CODEX_RETRY_GATEWAY_RUNTIME_PHASES) {
      expect(formatCodexRetryGatewayRuntimePhase(phase)).not.toHaveLength(0);
    }
    for (const route of CODEX_RETRY_GATEWAY_ROUTE_MODES) {
      expect(formatCodexRetryGatewayRouteMode(route)).not.toHaveLength(0);
    }
    for (const trust of CODEX_RETRY_GATEWAY_TRUST_STATES) {
      expect(formatCodexRetryGatewayTrustState(trust)).not.toHaveLength(0);
    }
    for (const source of CODEX_RETRY_GATEWAY_NODE_SOURCES) {
      expect(formatCodexRetryGatewayNodeSource(source)).not.toHaveLength(0);
    }
  });
});
