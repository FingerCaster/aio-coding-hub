import { describe, expect, it } from "vitest";
import { appEventNames } from "../../constants/appEvents";
import { HOME_USAGE_PERIOD_VALUES } from "../../constants/homeUsagePeriods";
import bindingsSource from "../bindings.ts?raw";
import heartbeatSource from "../../../src-tauri/src/app/heartbeat_watchdog.rs?raw";
import noticeSource from "../../../src-tauri/src/app/notice.rs?raw";
import settingsServiceSource from "../../../src-tauri/src/app/settings_service.rs?raw";
import startupStateSource from "../../../src-tauri/src/app/startup_state.rs?raw";
import updateChannelServiceSource from "../../services/app/updateChannel.ts?raw";
import settingsServiceFrontendSource from "../../services/settings/settings.ts?raw";

function extractStringUnionLiterals(source: string, typeName: string) {
  const match = source.match(new RegExp(`export type ${typeName} = (.+)$`, "m"));
  expect(match).toBeTruthy();
  return Array.from(match![1].matchAll(/"([^"]+)"/g), (item) => item[1]);
}

function extractRustStringConst(source: string, constName: string) {
  const match = source.match(new RegExp(`const ${constName}: &str = "([^"]+)";`));
  expect(match).toBeTruthy();
  return match![1];
}

function extractTypeBody(source: string, typeName: string) {
  const match = source.match(new RegExp(`export type ${typeName} = \\{([^}]*)\\}`));
  expect(match).toBeTruthy();
  return match![1];
}

function extractGeneratedCommand(source: string, commandName: string) {
  const start = source.indexOf(`async ${commandName}(`);
  expect(start).toBeGreaterThanOrEqual(0);
  const tail = source.slice(start);
  const end = tail.search(/\n\s*},/);
  expect(end).toBeGreaterThan(0);
  return tail.slice(0, end);
}

describe("generated/bindings.ts contract", () => {
  it("documents the generated IPC ownership surface", () => {
    expect(bindingsSource).toContain(
      "NOTE: Generated IPC contract for settings, config migration, desktop, app management, gateway, request-log, CLI update, CLI proxy, provider, WSL, sort-mode, provider-limit, usage, model-price, prompt, workspace, skills, MCP, CLI manager, CLI sessions, Claude validation, notice, and env-conflict command families."
    );
    expect(bindingsSource).toContain("settings_get");
    expect(bindingsSource).toContain("settings_gateway_rectifier_set");
    expect(bindingsSource).toContain("settings_circuit_breaker_notice_set");
    expect(bindingsSource).toContain("settings_codex_session_id_completion_set");
    expect(bindingsSource).toContain("desktop_clipboard_write_text");
    expect(bindingsSource).toContain("desktop_dialog_open");
    expect(bindingsSource).toContain("desktop_dialog_save");
    expect(bindingsSource).toContain("desktop_notification_notify");
    expect(bindingsSource).toContain("desktop_opener_open_path");
    expect(bindingsSource).toContain("desktop_opener_open_url");
    expect(bindingsSource).toContain("desktop_opener_reveal_item_in_dir");
    expect(bindingsSource).toContain("desktop_updater_check");
    expect(bindingsSource).toContain("app_about_get");
    expect(bindingsSource).toContain("app_heartbeat_pong");
    expect(bindingsSource).toContain("app_startup_status_get");
    expect(bindingsSource).toContain("app_startup_retry");
    expect(bindingsSource).toContain("app_frontend_error_report");
    expect(bindingsSource).toContain("app_data_reset");
    expect(bindingsSource).toContain("gateway_status");
    expect(bindingsSource).toContain("gateway_sessions_list");
    expect(bindingsSource).toContain("gateway_upstream_proxy_validate");
    expect(bindingsSource).toContain("request_logs_list_all");
    expect(bindingsSource).toContain("request_attempt_logs_by_trace_id");
    expect(bindingsSource).toContain("cli_proxy_status_all");
    expect(bindingsSource).toContain("cli_proxy_set_enabled");
    expect(bindingsSource).toContain("provider_upsert");
    expect(bindingsSource).toContain("provider_oauth_fetch_limits");
    expect(bindingsSource).toContain("wsl_detect");
    expect(bindingsSource).toContain("sort_modes_list");
    expect(bindingsSource).toContain("provider_limit_usage_v1");
    expect(bindingsSource).toContain("usage_summary_v2");
    for (const removedCostCommand of [
      "cost_summary_v1",
      "cost_trend_v1",
      "cost_breakdown_provider_v1",
      "cost_breakdown_model_v1",
      "cost_scatter_cli_provider_model_v1",
      "cost_top_requests_v1",
      "cost_backfill_missing_v1",
    ]) {
      expect(bindingsSource).not.toContain(removedCostCommand);
    }
    expect(bindingsSource).toContain("model_prices_list");
    expect(bindingsSource).toContain("model_price_upsert");
    expect(bindingsSource).toContain("prompts_list");
    expect(bindingsSource).toContain("workspaces_list");
    expect(bindingsSource).toContain("skills_installed_list");
    expect(bindingsSource).toContain("mcp_servers_list");
    expect(bindingsSource).toContain("cli_manager_codex_config_get");
    expect(bindingsSource).toContain("cli_sessions_projects_list");
    expect(bindingsSource).toContain("claude_provider_validate_model");
    expect(bindingsSource).toContain("notice_send");
    expect(bindingsSource).toContain("env_conflicts_check");
    expect(bindingsSource).toContain("config_export");
    expect(bindingsSource).toContain("cli_update");
    expect(bindingsSource).toContain("providers_reorder");
  });

  it("exports the home usage period literals used by runtime settings", () => {
    expect(extractStringUnionLiterals(bindingsSource, "HomeUsagePeriod")).toEqual([
      ...HOME_USAGE_PERIOD_VALUES,
    ]);
    expect(bindingsSource).not.toContain('"last_7"');
    expect(bindingsSource).not.toContain('"last_15"');
    expect(bindingsSource).not.toContain('"last_30"');
  });

  it("includes secret-safe upstream proxy settings in the generated settings contract", () => {
    expect(bindingsSource).toContain("codex_oauth_compatible_proxy_mode");
    expect(bindingsSource).toContain("codexOauthCompatibleProxyMode");
    expect(bindingsSource).toContain("upstream_proxy_enabled");
    expect(bindingsSource).toContain("upstream_proxy_url");
    expect(bindingsSource).toContain("upstream_proxy_username");
    expect(bindingsSource).toContain("upstream_proxy_password_configured");
    expect(bindingsSource).toContain("upstreamProxyEnabled");
    expect(bindingsSource).toContain("upstreamProxyUrl");
    expect(bindingsSource).toContain("upstreamProxyUsername");
    expect(bindingsSource).toContain("upstreamProxyPassword: SensitiveStringUpdate | null");
    expect(bindingsSource).toContain("export type SensitiveStringUpdate");
    expect(bindingsSource).toContain("export type SettingsMutationResult");
  });

  it("gives update channel a dedicated generated settings ownership surface", () => {
    expect(extractStringUnionLiterals(bindingsSource, "UpdateChannel")).toEqual(["beta", "stable"]);
    expect(extractTypeBody(bindingsSource, "SettingsView")).toContain(
      "update_channel: UpdateChannel"
    );
    expect(extractTypeBody(bindingsSource, "SettingsUpdate")).not.toContain("updateChannel");
    expect(extractTypeBody(bindingsSource, "SettingsPatch")).not.toContain("updateChannel");

    const command = extractGeneratedCommand(bindingsSource, "settingsUpdateChannelSet");
    expect(command).toMatch(
      /settingsUpdateChannelSet\(\s*channel: UpdateChannel,\s*confirm: RiskyIpcConfirm \| null\s*\)/
    );
    expect(command).toContain('TAURI_INVOKE("settings_update_channel_set", { channel, confirm })');
  });

  it("gives Codex model context rules a dedicated generated ownership surface", () => {
    const rule = extractTypeBody(bindingsSource, "CodexModelContextRule");
    expect(rule).toContain("model_id: string");
    expect(rule).toContain("context_window: number");
    expect(rule).toContain("enabled: boolean");

    expect(extractTypeBody(bindingsSource, "SettingsView")).toContain(
      "codex_model_context_rules: CodexModelContextRule[]"
    );
    expect(extractTypeBody(bindingsSource, "SettingsUpdate")).not.toContain(
      "codexModelContextRules"
    );
    expect(extractTypeBody(bindingsSource, "SettingsPatch")).not.toContain(
      "codexModelContextRules"
    );

    const setRules = extractGeneratedCommand(bindingsSource, "settingsCodexModelContextRulesSet");
    expect(setRules).toMatch(
      /settingsCodexModelContextRulesSet\(\s*rules: CodexModelContextRule\[\]\s*\)/
    );
    expect(setRules).toContain('TAURI_INVOKE("settings_codex_model_context_rules_set", { rules })');

    const candidate = extractTypeBody(bindingsSource, "CodexModelContextCandidate");
    expect(candidate).toContain("model_id: string");
    expect(candidate).toContain("display_name: string");
    expect(candidate).toContain("hidden: boolean");
    expect(candidate).toContain("base_context_window: number | null");
    expect(candidate).toContain("base_max_context_window: number | null");
    expect(extractTypeBody(bindingsSource, "CodexModelContextCandidatesState")).toContain(
      "models: CodexModelContextCandidate[]"
    );
    expect(
      extractGeneratedCommand(bindingsSource, "cliManagerCodexModelContextCandidatesGet")
    ).toContain('TAURI_INVOKE("cli_manager_codex_model_context_candidates_get")');

    expect(bindingsSource).not.toContain("settings_codex_gpt56_372k_context_set");
    expect(bindingsSource).not.toContain("settingsCodexGpt56372kContextSet");
    expect(bindingsSource).not.toContain("codex_gpt56_372k_context_enabled");
  });

  it("freezes the Beta confirmation resource across Rust and frontend owners", () => {
    const resource = "update_channel:beta";
    expect(settingsServiceSource).toContain(
      `UPDATE_CHANNEL_BETA_CONFIRM_RESOURCE: &str = "${resource}"`
    );
    expect(updateChannelServiceSource).toContain(
      `UPDATE_CHANNEL_BETA_CONFIRM_RESOURCE = "${resource}"`
    );
    expect(settingsServiceFrontendSource).toContain(
      'export { settingsUpdateChannelSet } from "../app/updateChannel"'
    );
    expect(settingsServiceFrontendSource).not.toContain(resource);
  });

  it("pins acronym casing for usage bridge filter DTO fields", () => {
    expect(extractTypeBody(bindingsSource, "UsageQueryParams")).toContain(
      "dayStartHour: number | null"
    );
    expect(extractTypeBody(bindingsSource, "UsageQueryParams")).toContain(
      "fullIdleGapMinutes: number | null"
    );
    expect(extractTypeBody(bindingsSource, "UsageQueryParams")).toContain(
      "sessionBreakGapMinutes: number | null"
    );
    expect(extractTypeBody(bindingsSource, "UsageDayDetailParams")).toContain(
      "dayStartHour: number | null"
    );
    expect(extractTypeBody(bindingsSource, "UsageQueryParams")).toContain(
      "excludeCx2CcGatewayBridge: boolean | null"
    );
    expect(extractTypeBody(bindingsSource, "UsageDayDetailParams")).toContain(
      "excludeCx2CcGatewayBridge: boolean | null"
    );
    expect(bindingsSource).not.toContain("excludeCx2ccGatewayBridge: boolean | null;");
  });

  it("keeps Result commands wrapped in ok/error envelopes while raw commands stay raw", () => {
    const settingsGet = extractGeneratedCommand(bindingsSource, "settingsGet");
    const gatewayStart = extractGeneratedCommand(bindingsSource, "gatewayStart");
    const requestLogsList = extractGeneratedCommand(bindingsSource, "requestLogsList");
    const gatewayStatus = extractGeneratedCommand(bindingsSource, "gatewayStatus");

    for (const body of [settingsGet, gatewayStart, requestLogsList]) {
      expect(body).toContain("Promise<Result<");
      expect(body).toContain('return { status: "ok", data: await TAURI_INVOKE(');
      expect(body).toContain('return { status: "error", error: e as any };');
    }

    expect(gatewayStatus).toContain("Promise<GatewayStatus>");
    expect(gatewayStatus).toContain('return await TAURI_INVOKE("gateway_status");');
    expect(gatewayStatus).not.toContain('status: "error"');
  });

  it("leaves updater install outside generated bindings when a Channel callback is required", () => {
    expect(bindingsSource).toContain("desktop_updater_check");
    expect(bindingsSource).toContain("desktop_updater_discard");
    expect(bindingsSource).not.toContain("desktop_updater_download_and_install");
    expect(bindingsSource).not.toContain("desktopUpdaterDownloadAndInstall");
  });

  it("requires channel-bound updater metadata in generated bindings", () => {
    const metadata = extractTypeBody(bindingsSource, "DesktopUpdaterMetadata");
    expect(metadata).toContain("rid: number");
    expect(metadata).toContain("channel: UpdateChannel");
    expect(metadata).toContain("isPrerelease: boolean");
    expect(metadata).toContain("currentVersion: string");
    expect(metadata).toContain("version: string");
    expect(metadata).toContain("releaseUrl: string");

    const check = extractGeneratedCommand(bindingsSource, "desktopUpdaterCheck");
    expect(check).toMatch(
      /desktopUpdaterCheck\(\s*expectedChannel: UpdateChannel,\s*timeout: number \| null\s*\)/
    );
    expect(check).toContain('TAURI_INVOKE("desktop_updater_check", { expectedChannel, timeout })');
  });

  it("keeps Rust app event emitters aligned with shared frontend constants", () => {
    expect(extractRustStringConst(heartbeatSource, "HEARTBEAT_EVENT_NAME")).toBe(
      appEventNames.heartbeat
    );
    expect(extractRustStringConst(noticeSource, "NOTICE_EVENT_NAME")).toBe(appEventNames.notice);
    expect(extractRustStringConst(startupStateSource, "APP_STARTUP_STATUS_EVENT_NAME")).toBe(
      appEventNames.startupStatus
    );
  });
});
