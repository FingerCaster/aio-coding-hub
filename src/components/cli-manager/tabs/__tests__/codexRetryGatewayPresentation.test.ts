import { describe, expect, it } from "vitest";
import {
  formatCodexRetryGatewayError,
  formatCodexRetryGatewayNodeSource,
  formatCodexRetryGatewayProviderSync,
  formatCodexRetryGatewayProviderSyncResult,
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
    expect(
      formatCodexRetryGatewayProviderSyncResult({
        status: "ok",
        target_provider: "aio",
        trigger: "external_gateway_enable",
        backup_dir: "C:\\Users\\test\\provider-sync\\1",
        changed_session_files: ["session-1.jsonl", "session-2.jsonl"],
        sqlite_provider_rows_updated: 3,
        sqlite_user_event_rows_updated: 4,
        sqlite_cwd_rows_updated: 5,
        updated_workspace_roots: ["workspace-1"],
        warning: "有 1 个旧记录保持不变",
      })
    ).toBe(
      "Provider Sync 已完成：aio；会话文件 2；SQLite Provider 3；用户事件 4；工作目录 5；工作区 1；备份已创建；有 1 个旧记录保持不变"
    );
  });

  it("treats guarded runtime as success tone and resolves repository shorthands", () => {
    expect(formatCodexRetryGatewayTone(createCodexRetryGatewayStatus())).toBe("success");

    expect(resolveRepositoryUrl("nonononull/codex-retry-gateway")).toBe(
      "https://github.com/nonononull/codex-retry-gateway"
    );
    expect(resolveRepositoryUrl("not a repo")).toBeNull();
  });

  it.each([
    [
      "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
      "本地 Git 无法同步官方网关仓库，请检查 Git 网络或代理配置后重试。",
    ],
    [
      "CODEX_RETRY_GATEWAY_SOURCE_RATE_LIMITED",
      "GitHub 匿名 API 请求已限流，请稍后重试；安装本地 Git 后可避免该限制。",
    ],
  ])("shows actionable source guidance for %s", (code, expected) => {
    expect(
      formatCodexRetryGatewayError({
        code,
        category: "source_resolution",
        message: "raw failure",
        retryable: true,
      })
    ).toBe(`${expected}（${code}）`);
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
