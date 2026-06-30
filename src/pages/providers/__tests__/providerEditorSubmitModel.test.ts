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
    codexBridgeTarget: "openai_chat",
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

  it("builds codex chat-completions bridge payload", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        cliKey: "codex",
        authMode: "cx2cc",
        codexBridgeTarget: "openai_chat",
        sourceProviderId: 7,
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "Codex Chat Bridge",
          api_key: "",
        },
      })
    );

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.payload.bridgeType).toBe("codex_to_openai_chat");
    expect(result.value.payload.sourceProviderId).toBe(7);
    expect(result.value.payload.baseUrls).toEqual([]);
    expect(result.value.payload.apiKey).toBeNull();
  });

  it("builds codex anthropic messages bridge payload", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        cliKey: "codex",
        authMode: "cx2cc",
        codexBridgeTarget: "anthropic_messages",
        sourceProviderId: 8,
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "Codex Messages Bridge",
          api_key: "",
        },
      })
    );

    expect(result.ok).toBe(true);
    if (!result.ok) return;

    expect(result.value.payload.bridgeType).toBe("codex_to_anthropic_messages");
    expect(result.value.payload.sourceProviderId).toBe(8);
  });

  it("requires source provider for codex bridge payloads", () => {
    const result = buildProviderEditorUpsertInput(
      makeContext({
        cliKey: "codex",
        authMode: "cx2cc",
        formValues: {
          ...DEFAULT_FORM_VALUES,
          name: "Codex Bridge",
          api_key: "",
        },
      })
    );

    expect(result).toEqual({
      ok: false,
      error: {
        kind: "message",
        message: "请选择上游来源",
      },
    });
  });
});
