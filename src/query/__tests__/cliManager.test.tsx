import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  type ClaudeCliInfo,
  type ClaudeHooksState,
  type ClaudeSettingsState,
  type CodexConfigState,
  type CodexConfigTomlState,
  type CodexModelCatalogState,
  type GeminiConfigState,
  type SimpleCliInfo,
  cliManagerClaudeHooksGet,
  cliManagerClaudeHooksSet,
  cliManagerClaudeInfoGet,
  cliManagerClaudeSettingsGet,
  cliManagerClaudeSettingsSet,
  cliManagerCodexConfigGet,
  cliManagerCodexConfigSet,
  cliManagerCodexConfigTomlGet,
  cliManagerCodexConfigTomlSet,
  cliManagerCodexInfoGet,
  cliManagerCodexModelCatalogGet,
  cliManagerCodexProviderSync,
  cliManagerGeminiConfigGet,
  cliManagerGeminiConfigSet,
  cliManagerGeminiInfoGet,
} from "../../services/cli/cliManager";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import { cliManagerKeys } from "../keys";
import {
  pickCliAvailable,
  useCliManagerClaudeHooksQuery,
  useCliManagerClaudeHooksSetMutation,
  useCliManagerClaudeInfoQuery,
  useCliManagerClaudeSettingsQuery,
  useCliManagerClaudeSettingsSetMutation,
  useCliManagerCodexConfigQuery,
  useCliManagerCodexConfigSetMutation,
  useCliManagerCodexConfigTomlQuery,
  useCliManagerCodexConfigTomlSetMutation,
  useCliManagerCodexInfoQuery,
  useCliManagerCodexModelCatalogQuery,
  useCliManagerCodexModelCatalogRefresh,
  useCliManagerCodexProviderSyncMutation,
  useCliManagerCodexReasoningGuardStatsQuery,
  useCliManagerGeminiConfigQuery,
  useCliManagerGeminiConfigSetMutation,
  useCliManagerGeminiInfoQuery,
} from "../cliManager";
import { useRequestLogsCodexReasoningGuardStatsQuery } from "../requestLogs";

vi.mock("../../services/cli/cliManager", async () => {
  const actual = await vi.importActual<typeof import("../../services/cli/cliManager")>(
    "../../services/cli/cliManager"
  );
  return {
    ...actual,
    cliManagerClaudeInfoGet: vi.fn(),
    cliManagerClaudeHooksGet: vi.fn(),
    cliManagerClaudeHooksSet: vi.fn(),
    cliManagerClaudeSettingsGet: vi.fn(),
    cliManagerClaudeSettingsSet: vi.fn(),
    cliManagerCodexInfoGet: vi.fn(),
    cliManagerCodexConfigGet: vi.fn(),
    cliManagerCodexConfigSet: vi.fn(),
    cliManagerCodexConfigTomlGet: vi.fn(),
    cliManagerCodexConfigTomlSet: vi.fn(),
    cliManagerCodexModelCatalogGet: vi.fn(),
    cliManagerCodexProviderSync: vi.fn(),
    cliManagerGeminiConfigGet: vi.fn(),
    cliManagerGeminiConfigSet: vi.fn(),
    cliManagerGeminiInfoGet: vi.fn(),
  };
});

vi.mock("../requestLogs", async () => {
  const actual = await vi.importActual<typeof import("../requestLogs")>("../requestLogs");
  return {
    ...actual,
    useRequestLogsCodexReasoningGuardStatsQuery: vi.fn(),
  };
});

function makeSimpleCliInfo(overrides: Partial<SimpleCliInfo> = {}): SimpleCliInfo {
  return {
    found: true,
    executable_path: "/usr/bin/codex",
    version: "0.0.0",
    error: null,
    shell: "zsh",
    resolved_via: "PATH",
    ...overrides,
  };
}

function makeClaudeCliInfo(overrides: Partial<ClaudeCliInfo> = {}): ClaudeCliInfo {
  return {
    ...makeSimpleCliInfo(),
    config_dir: "/tmp/.claude",
    settings_path: "/tmp/.claude/settings.json",
    mcp_timeout_ms: null,
    disable_error_reporting: false,
    ...overrides,
  };
}

function makeClaudeSettingsState(
  overrides: Partial<ClaudeSettingsState> = {}
): ClaudeSettingsState {
  return {
    config_dir: "/tmp/.claude",
    settings_path: "/tmp/.claude/settings.json",
    exists: true,
    model: null,
    output_style: null,
    language: null,
    always_thinking_enabled: null,
    show_turn_duration: null,
    spinner_tips_enabled: null,
    terminal_progress_bar_enabled: null,
    respect_gitignore: null,
    disable_git_participant: false,
    permissions_allow: [],
    permissions_ask: [],
    permissions_deny: [],
    env_mcp_timeout_ms: null,
    env_mcp_tool_timeout_ms: null,
    env_experimental_agent_teams: false,
    env_claude_code_auto_compact_window: null,
    env_disable_background_tasks: false,
    env_disable_terminal_title: false,
    env_claude_bash_no_login: false,
    env_claude_code_attribution_header: false,
    env_claude_code_blocking_limit_override: null,
    env_claude_code_max_output_tokens: null,
    env_enable_experimental_mcp_cli: false,
    env_enable_tool_search: false,
    env_max_mcp_output_tokens: null,
    env_claude_code_disable_nonessential_traffic: false,
    env_claude_code_disable_1m_context: false,
    env_claude_code_proxy_resolves_hosts: false,
    env_claude_code_skip_prompt_history: false,
    ...overrides,
  };
}

function makeClaudeHooksState(overrides: Partial<ClaudeHooksState> = {}): ClaudeHooksState {
  return {
    settings_path: "/tmp/.claude/settings.json",
    groups: [],
    ...overrides,
  };
}

function makeCodexConfigState(overrides: Partial<CodexConfigState> = {}): CodexConfigState {
  return {
    config_dir: "/tmp/.codex",
    config_path: "/tmp/.codex/config.toml",
    user_home_default_dir: "/tmp/.codex",
    user_home_default_path: "/tmp/.codex/config.toml",
    follow_codex_home_dir: "/tmp/.codex",
    follow_codex_home_path: "/tmp/.codex/config.toml",
    can_open_config_dir: true,
    exists: true,
    model: null,
    approval_policy: null,
    approvals_reviewer: null,
    sandbox_mode: null,
    model_reasoning_effort: null,
    plan_mode_reasoning_effort: null,
    web_search: null,
    personality: null,
    model_context_window: null,
    model_auto_compact_token_limit: null,
    service_tier: null,
    sandbox_workspace_write_network_access: null,
    features_unified_exec: null,
    features_shell_snapshot: null,
    features_apply_patch_freeform: null,
    features_shell_tool: null,
    features_exec_policy: null,
    features_remote_compaction: null,
    features_fast_mode: null,
    features_responses_websockets_v2: null,
    features_multi_agent: null,
    ...overrides,
  };
}

function makeCodexConfigTomlState(
  overrides: Partial<CodexConfigTomlState> = {}
): CodexConfigTomlState {
  return {
    config_path: "/tmp/.codex/config.toml",
    exists: true,
    toml: "",
    ...overrides,
  };
}

function makeGeminiConfigState(overrides: Partial<GeminiConfigState> = {}): GeminiConfigState {
  return {
    configDir: "/tmp/.gemini",
    configPath: "/tmp/.gemini/settings.json",
    exists: true,
    modelName: null,
    modelMaxSessionTurns: null,
    modelCompressionThreshold: null,
    defaultApprovalMode: null,
    enableAutoUpdate: null,
    enableNotifications: null,
    vimMode: null,
    retryFetchErrors: null,
    maxAttempts: null,
    uiTheme: null,
    uiHideBanner: null,
    uiHideTips: null,
    uiShowLineNumbers: null,
    uiInlineThinkingMode: null,
    usageStatisticsEnabled: null,
    sessionRetentionEnabled: null,
    sessionRetentionMaxAge: null,
    planModelRouting: null,
    securityAuthSelectedType: null,
    ...overrides,
  };
}

function makeCodexModelCatalogState(
  overrides: Partial<CodexModelCatalogState> = {}
): CodexModelCatalogState {
  return {
    status: "ready",
    issue: null,
    snapshot: {
      config_path: "/tmp/.codex/config.toml",
      executable_path: "/usr/bin/codex",
      cli_version: "0.0.0",
    },
    models: [],
    ...overrides,
  };
}

describe("query/cliManager", () => {
  it("calls cliManager queries with tauri runtime", async () => {
    setTauriRuntime();

    vi.mocked(cliManagerClaudeInfoGet).mockResolvedValue(makeClaudeCliInfo());
    vi.mocked(cliManagerClaudeHooksGet).mockResolvedValue(makeClaudeHooksState());
    vi.mocked(cliManagerClaudeSettingsGet).mockResolvedValue(makeClaudeSettingsState());
    vi.mocked(cliManagerCodexInfoGet).mockResolvedValue(makeSimpleCliInfo());
    vi.mocked(cliManagerCodexModelCatalogGet).mockResolvedValue(makeCodexModelCatalogState());
    vi.mocked(cliManagerCodexConfigGet).mockResolvedValue(makeCodexConfigState());
    vi.mocked(cliManagerCodexConfigTomlGet).mockResolvedValue(makeCodexConfigTomlState());
    vi.mocked(cliManagerCodexProviderSync).mockResolvedValue({
      status: "ok",
      target_provider: "aio",
      trigger: "manual",
      backup_dir: null,
      changed_session_files: [],
      sqlite_provider_rows_updated: 0,
      sqlite_user_event_rows_updated: 0,
      sqlite_cwd_rows_updated: 0,
      updated_workspace_roots: [],
      warning: null,
    });
    vi.mocked(cliManagerGeminiConfigGet).mockResolvedValue(makeGeminiConfigState());
    vi.mocked(cliManagerGeminiInfoGet).mockResolvedValue(makeSimpleCliInfo());

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useCliManagerClaudeInfoQuery(), { wrapper });
    renderHook(() => useCliManagerClaudeHooksQuery(), { wrapper });
    renderHook(() => useCliManagerClaudeSettingsQuery(), { wrapper });
    renderHook(() => useCliManagerCodexInfoQuery(), { wrapper });
    renderHook(
      () =>
        useCliManagerCodexModelCatalogQuery({
          snapshot: {
            configPath: "/tmp/.codex/config.toml",
            executablePath: "/usr/bin/codex",
            cliVersion: "0.0.0",
          },
        }),
      { wrapper }
    );
    renderHook(() => useCliManagerCodexConfigQuery(), { wrapper });
    renderHook(() => useCliManagerCodexConfigTomlQuery(), { wrapper });
    renderHook(() => useCliManagerCodexProviderSyncMutation(), { wrapper });
    renderHook(() => useCliManagerGeminiConfigQuery(), { wrapper });
    renderHook(() => useCliManagerGeminiInfoQuery(), { wrapper });

    await waitFor(() => {
      expect(cliManagerClaudeInfoGet).toHaveBeenCalled();
      expect(cliManagerClaudeHooksGet).toHaveBeenCalled();
      expect(cliManagerClaudeSettingsGet).toHaveBeenCalled();
      expect(cliManagerCodexInfoGet).toHaveBeenCalled();
      expect(cliManagerCodexModelCatalogGet).toHaveBeenCalled();
      expect(cliManagerCodexConfigGet).toHaveBeenCalled();
      expect(cliManagerCodexConfigTomlGet).toHaveBeenCalled();
      expect(cliManagerCodexProviderSync).not.toHaveBeenCalled();
      expect(cliManagerGeminiConfigGet).toHaveBeenCalled();
      expect(cliManagerGeminiInfoGet).toHaveBeenCalled();
    });
  });

  it("useCliManagerClaudeInfoQuery enters error state when service rejects", async () => {
    setTauriRuntime();

    vi.mocked(cliManagerClaudeInfoGet).mockRejectedValue(new Error("cli manager query boom"));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerClaudeInfoQuery(), { wrapper });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("useCliManagerCodexModelCatalogQuery enters error state when service rejects", async () => {
    setTauriRuntime();

    vi.mocked(cliManagerCodexModelCatalogGet).mockRejectedValue(new Error("catalog query boom"));

    const client = new QueryClient({
      defaultOptions: { queries: { retry: 1, retryDelay: 0 } },
    });
    const wrapper = createQueryWrapper(client);

    const { result, unmount } = renderHook(
      () =>
        useCliManagerCodexModelCatalogQuery({
          snapshot: {
            configPath: "/tmp/.codex/config.toml",
            executablePath: "/usr/bin/codex",
            cliVersion: "0.0.0",
          },
        }),
      { wrapper }
    );

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);

    unmount();
    const remounted = renderHook(
      () =>
        useCliManagerCodexModelCatalogQuery({
          snapshot: {
            configPath: "/tmp/.codex/config.toml",
            executablePath: "/usr/bin/codex",
            cliVersion: "0.0.0",
          },
        }),
      { wrapper }
    );
    expect(remounted.result.current.isError).toBe(true);
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
  });

  it("respects options.enabled=false for all cliManager info/config queries", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useCliManagerClaudeInfoQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerClaudeHooksQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerClaudeSettingsQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerCodexInfoQuery({ enabled: false }), { wrapper });
    renderHook(
      () =>
        useCliManagerCodexModelCatalogQuery({
          enabled: false,
          snapshot: {
            configPath: "/tmp/.codex/config.toml",
            executablePath: "/usr/bin/codex",
            cliVersion: "0.0.0",
          },
        }),
      { wrapper }
    );
    renderHook(() => useCliManagerCodexConfigQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerCodexConfigTomlQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerCodexProviderSyncMutation(), { wrapper });
    renderHook(() => useCliManagerGeminiConfigQuery({ enabled: false }), { wrapper });
    renderHook(() => useCliManagerGeminiInfoQuery({ enabled: false }), { wrapper });

    await Promise.resolve();

    expect(cliManagerClaudeInfoGet).not.toHaveBeenCalled();
    expect(cliManagerClaudeHooksGet).not.toHaveBeenCalled();
    expect(cliManagerClaudeSettingsGet).not.toHaveBeenCalled();
    expect(cliManagerCodexInfoGet).not.toHaveBeenCalled();
    expect(cliManagerCodexModelCatalogGet).not.toHaveBeenCalled();
    expect(cliManagerCodexConfigGet).not.toHaveBeenCalled();
    expect(cliManagerCodexConfigTomlGet).not.toHaveBeenCalled();
    expect(cliManagerCodexProviderSync).not.toHaveBeenCalled();
    expect(cliManagerGeminiConfigGet).not.toHaveBeenCalled();
    expect(cliManagerGeminiInfoGet).not.toHaveBeenCalled();
  });

  it("useCliManagerClaudeSettingsSetMutation updates cache and invalidates", async () => {
    setTauriRuntime();

    const updated = makeClaudeSettingsState({ model: "claude" });
    vi.mocked(cliManagerClaudeSettingsSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(cliManagerKeys.claudeSettings(), makeClaudeSettingsState({ model: "old" }));
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerClaudeSettingsSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ model: "claude" });
    });

    expect(client.getQueryData(cliManagerKeys.claudeSettings())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.claudeSettings() });
  });

  it("useCliManagerClaudeHooksSetMutation updates cache and invalidates", async () => {
    setTauriRuntime();

    const updated = makeClaudeHooksState({
      groups: [
        {
          event: "PreToolUse",
          matcher: "",
          hooks: [{ hook_type: "command", command: "echo ok", timeout: null }],
        },
      ],
    });
    vi.mocked(cliManagerClaudeHooksSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(cliManagerKeys.claudeHooks(), makeClaudeHooksState());
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerClaudeHooksSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ groups: updated.groups });
    });

    expect(client.getQueryData(cliManagerKeys.claudeHooks())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.claudeHooks() });
  });

  it("useCliManagerCodexConfigSetMutation updates cache and invalidates", async () => {
    setTauriRuntime();

    const updated = makeCodexConfigState({ model: "gpt-5" });
    vi.mocked(cliManagerCodexConfigSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(cliManagerKeys.codexConfig(), makeCodexConfigState({ model: "old" }));
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerCodexConfigSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ model: "gpt-5" });
    });

    expect(client.getQueryData(cliManagerKeys.codexConfig())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfig() });
  });

  it("useCliManagerCodexConfigTomlSetMutation updates config cache and invalidates config+toml", async () => {
    setTauriRuntime();

    const updated = makeCodexConfigState({ model: "gpt-5" });
    vi.mocked(cliManagerCodexConfigTomlSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerCodexConfigTomlSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ toml: 'model = "gpt-5"' });
    });

    expect(cliManagerCodexConfigTomlSet).toHaveBeenCalledWith('model = "gpt-5"');
    expect(client.getQueryData(cliManagerKeys.codexConfig())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfig() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfigToml() });
  });

  it("useCliManagerCodexProviderSyncMutation invalidates codex config and toml", async () => {
    setTauriRuntime();

    const synced = {
      status: "ok" as const,
      target_provider: "aio",
      trigger: "manual",
      backup_dir: null,
      changed_session_files: [],
      sqlite_provider_rows_updated: 1,
      sqlite_user_event_rows_updated: 0,
      sqlite_cwd_rows_updated: 0,
      updated_workspace_roots: [],
      warning: null,
    };
    vi.mocked(cliManagerCodexProviderSync).mockResolvedValue(synced);

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerCodexProviderSyncMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync();
    });

    expect(cliManagerCodexProviderSync).toHaveBeenCalledWith();
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfig() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.codexConfigToml() });
  });

  it("useCliManagerGeminiConfigSetMutation updates cache and invalidates", async () => {
    setTauriRuntime();

    const updated = makeGeminiConfigState({ modelName: "gemini-2.5-pro" });
    vi.mocked(cliManagerGeminiConfigSet).mockResolvedValue(updated);

    const client = createTestQueryClient();
    client.setQueryData(cliManagerKeys.geminiConfig(), makeGeminiConfigState({ modelName: "old" }));
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useCliManagerGeminiConfigSetMutation(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ modelName: "gemini-2.5-pro" });
    });

    expect(client.getQueryData(cliManagerKeys.geminiConfig())).toEqual(updated);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliManagerKeys.geminiConfig() });
  });

  it("refreshes only the requested model catalog snapshot", async () => {
    setTauriRuntime();

    const oldSnapshot = {
      configPath: "/tmp/.codex/config.toml",
      executablePath: "/usr/bin/codex",
      cliVersion: "0.0.0",
    };
    const nextSnapshot = {
      configPath: "/tmp/next/.codex/config.toml",
      executablePath: "/opt/codex/bin/codex",
      cliVersion: "1.0.0",
    };
    const nextCatalog = makeCodexModelCatalogState({
      snapshot: {
        config_path: nextSnapshot.configPath,
        executable_path: nextSnapshot.executablePath,
        cli_version: nextSnapshot.cliVersion,
      },
    });
    vi.mocked(cliManagerCodexModelCatalogGet)
      .mockResolvedValueOnce(makeCodexModelCatalogState())
      .mockResolvedValueOnce(nextCatalog);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result, rerender } = renderHook(
      ({ snapshot }) => ({
        catalog: useCliManagerCodexModelCatalogQuery({ snapshot }),
        refresh: useCliManagerCodexModelCatalogRefresh(),
      }),
      { wrapper, initialProps: { snapshot: oldSnapshot } }
    );

    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));
    vi.mocked(cliManagerCodexModelCatalogGet).mockClear();

    await act(async () => {
      await result.current.refresh(nextSnapshot);
    });

    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
    rerender({ snapshot: nextSnapshot });
    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
    expect(client.getQueryData(cliManagerKeys.codexModelCatalog(nextSnapshot))).toEqual(
      nextCatalog
    );
    expect(client.getQueryState(cliManagerKeys.codexModelCatalog(oldSnapshot))?.isInvalidated).toBe(
      false
    );

    vi.mocked(cliManagerCodexModelCatalogGet).mockClear().mockResolvedValueOnce(nextCatalog);
    await act(async () => {
      await result.current.refresh(nextSnapshot);
    });
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
  });

  it("does not duplicate a failed target snapshot prefetch when the observer switches keys", async () => {
    setTauriRuntime();

    const oldSnapshot = {
      configPath: "/tmp/.codex/config.toml",
      executablePath: "/usr/bin/codex",
      cliVersion: "0.0.0",
    };
    const nextSnapshot = {
      configPath: "/tmp/next/.codex/config.toml",
      executablePath: "/opt/codex/bin/codex",
      cliVersion: "1.0.0",
    };
    vi.mocked(cliManagerCodexModelCatalogGet).mockResolvedValueOnce(makeCodexModelCatalogState());

    const client = new QueryClient({
      defaultOptions: { queries: { retry: 1, retryDelay: 0 } },
    });
    const wrapper = createQueryWrapper(client);
    const { result, rerender } = renderHook(
      ({ snapshot }) => ({
        catalog: useCliManagerCodexModelCatalogQuery({ snapshot }),
        refresh: useCliManagerCodexModelCatalogRefresh(),
      }),
      { wrapper, initialProps: { snapshot: oldSnapshot } }
    );

    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));
    vi.mocked(cliManagerCodexModelCatalogGet)
      .mockReset()
      .mockRejectedValue(new Error("next snapshot failed"));

    await act(async () => {
      await result.current.refresh(nextSnapshot);
    });
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);

    rerender({ snapshot: nextSnapshot });
    await waitFor(() => expect(result.current.catalog.isError).toBe(true));
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
  });

  it("refreshes an inactive failed target snapshot before the observer switches keys", async () => {
    setTauriRuntime();

    const oldSnapshot = {
      configPath: "/tmp/.codex/config.toml",
      executablePath: "/usr/bin/codex",
      cliVersion: "0.0.0",
    };
    const nextSnapshot = {
      configPath: "/tmp/next/.codex/config.toml",
      executablePath: "/opt/codex/bin/codex",
      cliVersion: "1.0.0",
    };
    const nextCatalog = makeCodexModelCatalogState({
      snapshot: {
        config_path: nextSnapshot.configPath,
        executable_path: nextSnapshot.executablePath,
        cli_version: nextSnapshot.cliVersion,
      },
    });
    vi.mocked(cliManagerCodexModelCatalogGet).mockResolvedValueOnce(makeCodexModelCatalogState());

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result, rerender } = renderHook(
      ({ snapshot }) => ({
        catalog: useCliManagerCodexModelCatalogQuery({ snapshot }),
        refresh: useCliManagerCodexModelCatalogRefresh(),
      }),
      { wrapper, initialProps: { snapshot: oldSnapshot } }
    );

    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));
    vi.mocked(cliManagerCodexModelCatalogGet)
      .mockReset()
      .mockRejectedValueOnce(new Error("cached target failure"));
    await client.prefetchQuery({
      queryKey: cliManagerKeys.codexModelCatalog(nextSnapshot),
      queryFn: () => cliManagerCodexModelCatalogGet(),
      retry: false,
    });
    expect(client.getQueryState(cliManagerKeys.codexModelCatalog(nextSnapshot))?.status).toBe(
      "error"
    );

    vi.mocked(cliManagerCodexModelCatalogGet).mockReset().mockResolvedValue(nextCatalog);
    await act(async () => {
      await result.current.refresh(nextSnapshot);
    });
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);

    rerender({ snapshot: nextSnapshot });
    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
    expect(client.getQueryData(cliManagerKeys.codexModelCatalog(nextSnapshot))).toEqual(
      nextCatalog
    );
  });

  it("reuses an in-flight stale target snapshot fetch during refresh", async () => {
    setTauriRuntime();

    const oldSnapshot = {
      configPath: "/tmp/.codex/config.toml",
      executablePath: "/usr/bin/codex",
      cliVersion: "0.0.0",
    };
    const nextSnapshot = {
      configPath: "/tmp/next/.codex/config.toml",
      executablePath: "/opt/codex/bin/codex",
      cliVersion: "1.0.0",
    };
    const staleCatalog = makeCodexModelCatalogState({
      snapshot: {
        config_path: nextSnapshot.configPath,
        executable_path: nextSnapshot.executablePath,
        cli_version: nextSnapshot.cliVersion,
      },
    });
    const nextCatalog = makeCodexModelCatalogState({
      snapshot: {
        config_path: nextSnapshot.configPath,
        executable_path: nextSnapshot.executablePath,
        cli_version: nextSnapshot.cliVersion,
      },
    });
    vi.mocked(cliManagerCodexModelCatalogGet).mockResolvedValueOnce(makeCodexModelCatalogState());

    const client = createTestQueryClient();
    client.setQueryData(cliManagerKeys.codexModelCatalog(nextSnapshot), staleCatalog, {
      updatedAt: 1,
    });
    const wrapper = createQueryWrapper(client);
    const { result, rerender } = renderHook(
      ({ snapshot }) => ({
        catalog: useCliManagerCodexModelCatalogQuery({ snapshot }),
        refresh: useCliManagerCodexModelCatalogRefresh(),
      }),
      { wrapper, initialProps: { snapshot: oldSnapshot } }
    );

    await waitFor(() => expect(result.current.catalog.isSuccess).toBe(true));

    let resolveNextCatalog!: (value: CodexModelCatalogState) => void;
    const nextCatalogRequest = new Promise<CodexModelCatalogState>((resolve) => {
      resolveNextCatalog = resolve;
    });
    vi.mocked(cliManagerCodexModelCatalogGet).mockReset().mockReturnValue(nextCatalogRequest);

    rerender({ snapshot: nextSnapshot });
    await waitFor(() => expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1));

    let refreshRequest!: Promise<void>;
    act(() => {
      refreshRequest = result.current.refresh(nextSnapshot);
    });
    await Promise.resolve();
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveNextCatalog(nextCatalog);
      await nextCatalogRequest;
      await refreshRequest;
    });
    expect(cliManagerCodexModelCatalogGet).toHaveBeenCalledTimes(1);
    expect(client.getQueryData(cliManagerKeys.codexModelCatalog(nextSnapshot))).toEqual(
      nextCatalog
    );
  });

  it("serializes ordinary Codex config and TOML writes in one mutation scope", async () => {
    setTauriRuntime();

    let resolveConfig!: (value: CodexConfigState) => void;
    let resolveToml!: (value: CodexConfigState) => void;
    const configPromise = new Promise<CodexConfigState>((resolve) => {
      resolveConfig = resolve;
    });
    const tomlPromise = new Promise<CodexConfigState>((resolve) => {
      resolveToml = resolve;
    });
    vi.mocked(cliManagerCodexConfigSet).mockReset().mockReturnValueOnce(configPromise);
    vi.mocked(cliManagerCodexConfigTomlSet).mockReset().mockReturnValueOnce(tomlPromise);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const configMutation = renderHook(() => useCliManagerCodexConfigSetMutation(), { wrapper });
    const tomlMutation = renderHook(() => useCliManagerCodexConfigTomlSetMutation(), { wrapper });

    let configCall!: Promise<CodexConfigState>;
    await act(async () => {
      configCall = configMutation.result.current.mutateAsync({ model: "first" });
      await Promise.resolve();
    });
    await waitFor(() => expect(cliManagerCodexConfigSet).toHaveBeenCalledTimes(1));

    let tomlCall!: Promise<CodexConfigState>;
    await act(async () => {
      tomlCall = tomlMutation.result.current.mutateAsync({ toml: 'model = "second"' });
      await Promise.resolve();
    });
    expect(cliManagerCodexConfigTomlSet).not.toHaveBeenCalled();

    await act(async () => {
      resolveConfig(makeCodexConfigState({ model: "first" }));
      await configCall;
    });
    await waitFor(() => expect(cliManagerCodexConfigTomlSet).toHaveBeenCalledTimes(1));

    await act(async () => {
      resolveToml(makeCodexConfigState({ model: "second" }));
      await tomlCall;
    });
    expect(cliManagerCodexConfigTomlSet).toHaveBeenCalledWith('model = "second"');
  });

  it("mutation hooks keep cache unchanged when service returns null", async () => {
    setTauriRuntime();

    vi.mocked(cliManagerClaudeSettingsSet).mockResolvedValue(null as never);
    vi.mocked(cliManagerCodexConfigSet).mockResolvedValue(null as never);
    vi.mocked(cliManagerCodexConfigTomlSet).mockResolvedValue(null as never);
    vi.mocked(cliManagerGeminiConfigSet).mockResolvedValue(null as never);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    client.setQueryData(cliManagerKeys.claudeSettings(), { exists: true, model: "old-claude" });
    client.setQueryData(cliManagerKeys.codexConfig(), { exists: true, model: "old-codex" });
    client.setQueryData(
      cliManagerKeys.geminiConfig(),
      makeGeminiConfigState({ modelName: "old-gemini" })
    );

    const claudeMutation = renderHook(() => useCliManagerClaudeSettingsSetMutation(), { wrapper });
    const codexMutation = renderHook(() => useCliManagerCodexConfigSetMutation(), { wrapper });
    const tomlMutation = renderHook(() => useCliManagerCodexConfigTomlSetMutation(), { wrapper });
    const geminiMutation = renderHook(() => useCliManagerGeminiConfigSetMutation(), { wrapper });

    await act(async () => {
      await claudeMutation.result.current.mutateAsync({ model: "new-claude" });
      await codexMutation.result.current.mutateAsync({ model: "new-codex" });
      await tomlMutation.result.current.mutateAsync({ toml: 'model = "new-codex"' });
      await geminiMutation.result.current.mutateAsync({ modelName: "new-gemini" });
    });

    expect(client.getQueryData(cliManagerKeys.claudeSettings())).toEqual({
      exists: true,
      model: "old-claude",
    });
    expect(client.getQueryData(cliManagerKeys.codexConfig())).toEqual({
      exists: true,
      model: "old-codex",
    });
    expect(client.getQueryData(cliManagerKeys.geminiConfig())).toEqual(
      makeGeminiConfigState({ modelName: "old-gemini" })
    );
  });

  it("pickCliAvailable maps info to availability state", () => {
    expect(pickCliAvailable(null)).toBe("unavailable");
    expect(pickCliAvailable(makeSimpleCliInfo({ found: false }))).toBe("unavailable");
    expect(pickCliAvailable(makeSimpleCliInfo({ found: true }))).toBe("available");
  });

  it("forwards Codex reasoning guard stats queries with the requested time window", () => {
    vi.mocked(useRequestLogsCodexReasoningGuardStatsQuery).mockReturnValue({
      data: null,
      isFetching: false,
    } as any);

    const sessionOptions = { enabled: true };
    const allTimeOptions = { enabled: false };

    useCliManagerCodexReasoningGuardStatsQuery(
      { startCreatedAtMs: 1_770_000_000_000, endCreatedAtMs: 1_770_086_400_000 },
      sessionOptions
    );
    useCliManagerCodexReasoningGuardStatsQuery(
      { startCreatedAtMs: null, endCreatedAtMs: null },
      allTimeOptions
    );

    expect(useRequestLogsCodexReasoningGuardStatsQuery).toHaveBeenNthCalledWith(
      1,
      { startCreatedAtMs: 1_770_000_000_000, endCreatedAtMs: 1_770_086_400_000 },
      sessionOptions
    );
    expect(useRequestLogsCodexReasoningGuardStatsQuery).toHaveBeenNthCalledWith(
      2,
      { startCreatedAtMs: null, endCreatedAtMs: null },
      allTimeOptions
    );
  });
});
