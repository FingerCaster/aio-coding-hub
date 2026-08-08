import { describe, expect, it } from "vitest";
import { DEFAULT_UPSTREAM_RETRY_POLICY } from "../../../services/gateway/upstreamRetryPolicy";
import { DEFAULT_FORM_VALUES } from "../providerEditorUtils";
import { buildProviderEditorUpsertInput } from "../providerEditorSubmitModel";
import type { ProviderEditorPayloadContext } from "../providerEditorActionContext";

function makeContext(
  overrides: Partial<ProviderEditorPayloadContext> = {}
): ProviderEditorPayloadContext {
  return {
    mode: "create",
    cliKey: "claude",
    editingProviderId: null,
    authMode: "api_key",
    baseUrlMode: "order",
    baseUrlRows: [{ id: "1", url: "https://example.com/v1", ping: { status: "idle" } }],
    tags: [],
    claudeModels: {},
    testModel: "",
    streamIdleTimeoutSeconds: "",
    upstreamRetryPolicyOverrideEnabled: false,
    upstreamRetryPolicyDraft: DEFAULT_UPSTREAM_RETRY_POLICY,
    apiKeyConfigured: false,
    isCodexGatewaySource: false,
    sourceProviderId: null,
    selectedCx2ccSourceProvider: null,
    formValues: {
      ...DEFAULT_FORM_VALUES,
      name: "Provider A",
      api_key: "sk-test",
    },
    ...overrides,
  };
}

describe("pages/providers/providerEditorSubmitModel", () => {
  it("requires an api key when editing an api-key provider without a saved secret", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        mode: "edit",
        editingProviderId: 8,
        apiKeyConfigured: false,
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "Provider A",
          api_key: "",
        },
      })
    );

    expect(result).toEqual({
      ok: false,
      error: {
        kind: "message",
        message: "请输入 API Key",
      },
    });
  });

  it("clears base urls and api key for oauth providers", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        authMode: "oauth",
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "OAuth Provider",
          api_key: "",
          auth_mode: "oauth",
        },
      })
    );

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.payload.baseUrls).toEqual([]);
    expect(result.value.payload.apiKey).toBeNull();
    expect(result.value.payload.authMode).toBe("oauth");
  });

  it("forces cx2cc gateway sources to use zero cost and no source provider id", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        authMode: "cx2cc",
        isCodexGatewaySource: true,
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "CX2CC Provider",
          api_key: "",
        },
      })
    );

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.payload.costMultiplier).toBe(0);
    expect(result.value.payload.bridgeType).toBe("cx2cc");
    expect(result.value.payload.sourceProviderId).toBeNull();
    expect(result.value.payload.authMode).toBe("api_key");
  });

  it("rejects cx2cc mode for Codex providers", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        cliKey: "codex",
        authMode: "cx2cc",
      })
    );

    expect(result).toEqual({
      ok: false,
      error: {
        kind: "message",
        message: "仅 Claude 供应商支持 CX2CC",
      },
    });
  });

  it("passes codex availability test model through the payload", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        cliKey: "codex",
        testModel: "gpt-5.4",
      })
    );

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.payload.availabilityTestModel).toBe("gpt-5.4");
  });

  it("passes explicit NewAPI account credential preserve and clear semantics", () => {
    const preserve = buildProviderEditorUpsertInput(
      makeContext({
        accountUsageCredentials: {
          newApiUserId: "42",
          newApiAccessToken: null,
          clearNewApiAccessToken: false,
        },
      })
    );
    expect(preserve.ok).toBe(true);
    if (!preserve.ok) return;
    expect(preserve.value.payload.accountUsageCredentials).toEqual({
      newApiUserId: "42",
      newApiAccessToken: null,
      clearNewApiAccessToken: false,
    });

    const clear = buildProviderEditorUpsertInput(
      makeContext({
        accountUsageCredentials: {
          newApiUserId: null,
          newApiAccessToken: null,
          clearNewApiAccessToken: true,
        },
      })
    );
    expect(clear.ok).toBe(true);
    if (!clear.ok) return;
    expect(clear.value.payload.accountUsageCredentials).toEqual({
      newApiUserId: null,
      newApiAccessToken: null,
      clearNewApiAccessToken: true,
    });
  });
});
