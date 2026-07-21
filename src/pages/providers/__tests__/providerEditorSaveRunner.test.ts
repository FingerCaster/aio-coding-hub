import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import type { ProviderSummary } from "../../../services/providers/providers";
import { DEFAULT_UPSTREAM_RETRY_POLICY } from "../../../services/gateway/upstreamRetryPolicy";
import { DEFAULT_FORM_VALUES } from "../providerEditorUtils";
import { runProviderEditorSave } from "../providerEditorSaveRunner";
import type { SaveActionContext } from "../providerEditorActionContext";

vi.mock("sonner", () => ({ toast: vi.fn() }));
vi.mock("../../../services/consoleLog", () => ({ logToConsole: vi.fn() }));

function makeSavedProvider(partial: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: partial.id ?? 1,
    provider_uuid: partial.provider_uuid ?? "11111111-1111-4111-8111-111111111111",
    cli_key: partial.cli_key ?? "claude",
    name: partial.name ?? "Saved Provider",
    base_urls: partial.base_urls ?? ["https://example.com/v1"],
    base_url_mode: partial.base_url_mode ?? "order",
    claude_models: partial.claude_models ?? {},
    enabled: partial.enabled ?? true,
    priority: partial.priority ?? 0,
    cost_multiplier: partial.cost_multiplier ?? 1,
    limit_5h_usd: partial.limit_5h_usd ?? null,
    limit_daily_usd: partial.limit_daily_usd ?? null,
    daily_reset_mode: partial.daily_reset_mode ?? "fixed",
    daily_reset_time: partial.daily_reset_time ?? "00:00:00",
    limit_weekly_usd: partial.limit_weekly_usd ?? null,
    limit_monthly_usd: partial.limit_monthly_usd ?? null,
    limit_total_usd: partial.limit_total_usd ?? null,
    tags: partial.tags ?? [],
    note: partial.note ?? "",
    created_at: partial.created_at ?? 0,
    updated_at: partial.updated_at ?? 0,
    auth_mode: partial.auth_mode ?? "api_key",
    oauth_provider_type: partial.oauth_provider_type ?? null,
    oauth_email: partial.oauth_email ?? null,
    oauth_expires_at: partial.oauth_expires_at ?? null,
    oauth_last_error: partial.oauth_last_error ?? null,
    source_provider_id: partial.source_provider_id ?? null,
    bridge_type: partial.bridge_type ?? null,
    model_mapping: partial.model_mapping ?? { default_model: null, exact: {} },
    stream_idle_timeout_seconds: partial.stream_idle_timeout_seconds ?? null,
    extension_values: partial.extension_values ?? [],
    upstream_retry_policy_override: partial.upstream_retry_policy_override ?? null,
    availability_test_model: partial.availability_test_model ?? null,
    api_key_configured: partial.api_key_configured ?? true,
    newapi_account_user_id: partial.newapi_account_user_id ?? null,
    newapi_account_access_token_configured: partial.newapi_account_access_token_configured ?? false,
  };
}

function makeContext(overrides: Partial<SaveActionContext> = {}): SaveActionContext {
  const getValues = vi.fn().mockReturnValue({
    ...DEFAULT_FORM_VALUES,
    name: "Provider A",
    api_key: "sk-test",
  });

  return {
    mode: "create",
    cliKey: "claude",
    editingProviderId: null,
    editProvider: null,
    open: true,
    onOpenChange: vi.fn(),
    onSaved: vi.fn(),
    onModelFetchFailedAfterSave: vi.fn(),
    authMode: "api_key",
    codexBridgeTarget: "openai_chat",
    baseUrlMode: "order",
    baseUrlRows: [{ id: "1", url: "https://example.com/v1", ping: { status: "idle" } }],
    tags: [],
    claudeModels: {},
    modelMapping: { default_model: null, exact: {} },
    testModel: "",
    streamIdleTimeoutSeconds: "",
    upstreamRetryPolicyOverrideEnabled: false,
    upstreamRetryPolicyDraft: DEFAULT_UPSTREAM_RETRY_POLICY,
    apiKeyConfigured: false,
    isCodexGatewaySource: false,
    sourceProviderId: null,
    selectedCx2ccSourceProvider: null,
    formValues: getValues(),
    saving: false,
    setSaving: vi.fn(),
    form: {
      getValues,
      setValue: vi.fn(),
    },
    oauthStatus: null,
    setOauthStatus: vi.fn(),
    refreshOauthStatus: vi.fn().mockResolvedValue(null),
    clearAccountUsageSecretDraft: vi.fn(),
    persistProvider: vi.fn().mockResolvedValue(makeSavedProvider()),
    refreshProviderModels: vi.fn(),
    ...overrides,
  };
}

describe("pages/providers/providerEditorSaveRunner", () => {
  it("stops before persist when payload validation fails", async () => {
    const ctx = makeContext({
      form: {
        getValues: vi.fn().mockReturnValue({
          ...DEFAULT_FORM_VALUES,
          name: "",
          api_key: "",
        }),
        setValue: vi.fn(),
      },
    });

    await runProviderEditorSave(ctx);

    expect(vi.mocked(toast)).toHaveBeenCalled();
    expect(ctx.persistProvider).not.toHaveBeenCalled();
  });

  it("blocks oauth save when the provider is still disconnected", async () => {
    const ctx = makeContext({
      mode: "edit",
      editingProviderId: 7,
      authMode: "oauth",
      apiKeyConfigured: true,
      form: {
        getValues: vi.fn().mockReturnValue({
          ...DEFAULT_FORM_VALUES,
          name: "OAuth Provider",
          api_key: "",
          auth_mode: "oauth",
        }),
        setValue: vi.fn(),
      },
      refreshOauthStatus: vi.fn().mockResolvedValue({
        connected: false,
        provider_type: null,
        email: null,
        expires_at: null,
        has_refresh_token: null,
      }),
    });

    await runProviderEditorSave(ctx);

    expect(ctx.refreshOauthStatus).toHaveBeenCalledWith(7);
    expect(vi.mocked(toast)).toHaveBeenCalledWith("请先完成 OAuth 登录");
    expect(ctx.persistProvider).not.toHaveBeenCalled();
  });

  it("persists the provider and clears both secret drafts on success", async () => {
    const ctx = makeContext();

    await runProviderEditorSave(ctx);

    expect(ctx.setSaving).toHaveBeenNthCalledWith(1, true);
    expect(ctx.persistProvider).toHaveBeenCalledTimes(1);
    expect(ctx.form.setValue).toHaveBeenCalledWith("api_key", "", {
      shouldDirty: false,
      shouldValidate: false,
    });
    expect(ctx.clearAccountUsageSecretDraft).toHaveBeenCalledOnce();
    expect(ctx.onOpenChange).toHaveBeenCalledWith(false);
    expect(ctx.setSaving).toHaveBeenLastCalledWith(false);
  });

  it("saves first and closes only after model refresh succeeds", async () => {
    const saved = makeSavedProvider({ id: 7, cli_key: "codex" });
    const ctx = makeContext({
      cliKey: "codex",
      persistProvider: vi.fn().mockResolvedValue(saved),
      refreshProviderModels: vi.fn().mockResolvedValue({
        providerId: 7,
        providerUuid: "11111111-1111-4111-8111-111111111111",
        protocol: "openai_compatible",
        stale: false,
        lastAttemptAt: 10,
        lastSuccessAt: 10,
        lastErrorCode: null,
        models: [],
      }),
    });

    await runProviderEditorSave(ctx, { refreshModels: true });

    expect(ctx.persistProvider).toHaveBeenCalledOnce();
    expect(ctx.refreshProviderModels).toHaveBeenCalledWith(7, saved.provider_uuid);
    expect(ctx.onSaved).toHaveBeenCalledWith("codex");
    expect(ctx.onOpenChange).toHaveBeenCalledWith(false);
    expect(vi.mocked(toast)).toHaveBeenCalledWith("Provider 已保存，已获取 0 个模型");
  });

  it("keeps the editor open when model refresh fails after save", async () => {
    const saved = makeSavedProvider({ id: 7, cli_key: "codex" });
    const ctx = makeContext({
      cliKey: "codex",
      persistProvider: vi.fn().mockResolvedValue(saved),
      refreshProviderModels: vi
        .fn()
        .mockRejectedValue(
          new Error("UNKNOWN_FAILURE: https://example.test/v1?api_key=SYNTHETIC_REFRESH_SECRET")
        ),
    });

    await runProviderEditorSave(ctx, { refreshModels: true });

    expect(ctx.persistProvider).toHaveBeenCalledOnce();
    expect(ctx.refreshProviderModels).toHaveBeenCalledWith(7, saved.provider_uuid);
    expect(ctx.onSaved).toHaveBeenCalledWith("codex");
    expect(ctx.onModelFetchFailedAfterSave).toHaveBeenCalledWith(saved);
    expect(ctx.onOpenChange).not.toHaveBeenCalledWith(false);
    expect(vi.mocked(toast)).toHaveBeenCalledWith(
      expect.stringContaining("Provider 已保存，模型获取失败")
    );
    expect(JSON.stringify(vi.mocked(toast).mock.calls)).not.toContain("SYNTHETIC_REFRESH_SECRET");
    expect(ctx.setSaving).toHaveBeenLastCalledWith(false);
  });

  it("keeps the saved provider editable when discovery returns a catalog error", async () => {
    const saved = makeSavedProvider({ id: 7, cli_key: "codex" });
    const ctx = makeContext({
      cliKey: "codex",
      persistProvider: vi.fn().mockResolvedValue(saved),
      refreshProviderModels: vi.fn().mockResolvedValue({
        providerId: 7,
        providerUuid: "11111111-1111-4111-8111-111111111111",
        protocol: "openai_compatible",
        stale: true,
        lastAttemptAt: 10,
        lastSuccessAt: null,
        lastErrorCode: "timeout",
        models: [],
      }),
    });

    await runProviderEditorSave(ctx, { refreshModels: true });

    expect(ctx.onSaved).toHaveBeenCalledWith("codex");
    expect(ctx.onModelFetchFailedAfterSave).toHaveBeenCalledWith(saved);
    expect(ctx.onOpenChange).not.toHaveBeenCalledWith(false);
    expect(vi.mocked(toast)).toHaveBeenCalledWith(
      "Provider 已保存，模型获取失败：模型接口请求超时"
    );
  });
});
