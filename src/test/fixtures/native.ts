import type {
  NativeProviderEdit,
  NativeProviderSummary,
  NativeProvidersList,
  NativeTarget,
} from "../../services/nativeCli";
import type { GatewayCatalogPreview, NativeModelSpec } from "../../services/nativeGateway";
import type {
  ChannelModelDiscovery,
  ModelCapabilitySuggestion,
} from "../../services/nativeChannels";

export function modelSuggestion(
  overrides: Partial<ModelCapabilitySuggestion> = {}
): ModelCapabilitySuggestion {
  return {
    modelId: "gpt-4o",
    displayName: null,
    input: null,
    contextWindow: null,
    maxTokens: null,
    supportsTools: null,
    reasoning: null,
    reasoningEfforts: null,
    defaultReasoningEffort: null,
    thinkingMode: null,
    supportsDisplay: null,
    requiresEffort: null,
    nativeThinking: null,
    sources: ["upstream"],
    ...overrides,
  };
}
export function channelDiscovery(
  overrides: Partial<ChannelModelDiscovery> = {}
): ChannelModelDiscovery {
  return {
    targetId: "pi:local:a",
    providerId: 101,
    providerUuid: "prov-101",
    protocol: "openai-responses",
    revision: "rev-1",
    models: [],
    discovery: { status: "ready", models: [], origin: "https://example.test", base_url_index: 0 },
    ...overrides,
  };
}

export function nativeTarget(overrides: Partial<NativeTarget> = {}): NativeTarget {
  return {
    targetId: "pi:local:a",
    client: "pi",
    environment: "local",
    profile: null,
    agentDir: "C:/agent",
    modelsPath: "C:/agent/models.json",
    source: "default",
    format: "jsonc",
    selected: true,
    writable: true,
    issue: null,
    shadowedFiles: [],
    ...overrides,
  };
}
export function nativeSummary(
  overrides: Partial<NativeProviderSummary> = {}
): NativeProviderSummary {
  return {
    profileUuid: "profile-a",
    nativeKey: "vendor",
    displayName: "Native Vendor",
    state: "present",
    managed: false,
    nodeDigest: "node-1",
    profileRevision: "profile-1",
    api: "custom-extension-api",
    modelCount: 1,
    apiKeyConfigured: true,
    ...overrides,
  };
}
export function nativeList(overrides: Partial<NativeProvidersList> = {}): NativeProvidersList {
  return {
    target: nativeTarget(),
    revision: "file-1",
    parseStatus: "ready",
    issue: null,
    providers: [nativeSummary()],
    ...overrides,
  };
}
export function nativeEdit(overrides: Partial<NativeProviderEdit> = {}): NativeProviderEdit {
  return {
    target: nativeTarget(),
    revision: "file-1",
    provider: nativeSummary(),
    node: {
      api: "custom-extension-api",
      baseUrl: "https://secret-host.test",
      apiKey: "!read-secret",
      headers: { "X-Custom": "private" },
      extension: { keep: true },
      models: [{ id: "model-a", contextWindow: 8192, maxTokens: 1024 }],
    },
    ...overrides,
  };
}
export function nativeModel(overrides: Partial<NativeModelSpec> = {}): NativeModelSpec {
  return {
    requestModelId: "model-a",
    displayName: "Explicit Model",
    input: ["text"],
    contextWindow: 8192,
    maxTokens: 1024,
    reasoning: false,
    thinking: null,
    supportsTools: null,
    ...overrides,
  };
}
export function gatewayPreview(
  overrides: Partial<GatewayCatalogPreview> = {}
): GatewayCatalogPreview {
  return {
    targetId: "pi:local:a",
    revision: "file-1",
    catalogRevision: "catalog-1",
    listenerReady: true,
    groups: [{ protocol: "openai-responses", models: [nativeModel()] }],
    entries: [
      {
        protocol: "openai-responses",
        nativeKey: "aio-responses",
        baseUrl: "http://127.0.0.1:3712/pi",
        models: [nativeModel()],
        node: { api: "openai-responses" },
      },
    ],
    manifests: [],
    ...overrides,
  };
}
export function channelProvider(
  overrides: Partial<import("../../services/nativeChannels").ChannelProvider> = {}
): import("../../services/nativeChannels").ChannelProvider {
  return {
    providerId: 101,
    providerUuid: "prov-101",
    name: "Codex Upstream",
    authMode: "api_key",
    blockedReason: null,
    modelIds: ["gpt-4o"],
    ...overrides,
  };
}

export function channelSource(
  overrides: Partial<import("../../services/nativeChannels").ChannelSource> = {}
): import("../../services/nativeChannels").ChannelSource {
  return {
    sourceChannel: "codex",
    protocol: "openai-responses",
    providers: [channelProvider()],
    models: [nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" })],
    blockedReason: null,
    ...overrides,
  };
}

export function channelBinding(
  overrides: Partial<import("../../services/nativeChannels").ChannelBindingSummary> = {}
): import("../../services/nativeChannels").ChannelBindingSummary {
  return {
    bindingId: "bind-codex-1",
    sourceChannel: "codex",
    protocol: "openai-responses",
    nativeKey: "aio-channel-codex-openai-responses",
    models: [nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" })],
    state: "applied",
    modified: false,
    stale: false,
    ...overrides,
  };
}

export function channelCatalog(
  overrides: Partial<import("../../services/nativeChannels").ChannelCatalog> = {}
): import("../../services/nativeChannels").ChannelCatalog {
  return {
    targetId: "pi:local:a",
    revision: "file-1",
    catalogRevision: "cat-1",
    listenerReady: true,
    sources: [
      channelSource({
        sourceChannel: "claude",
        protocol: "anthropic-messages",
        providers: [
          channelProvider({
            providerId: 201,
            providerUuid: "prov-claude",
            name: "Claude Code Upstream",
            modelIds: ["claude-3-7-sonnet"],
          }),
        ],
        models: [
          nativeModel({ requestModelId: "claude-3-7-sonnet", displayName: "Claude 3.7 Sonnet" }),
        ],
      }),
      channelSource({
        sourceChannel: "codex",
        protocol: "openai-responses",
        providers: [
          channelProvider({
            providerId: 101,
            providerUuid: "prov-codex",
            name: "Codex Upstream",
            modelIds: ["gpt-4o"],
          }),
        ],
        models: [nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" })],
      }),
      channelSource({
        sourceChannel: "grok",
        protocol: "openai-completions",
        providers: [
          channelProvider({
            providerId: 301,
            providerUuid: "prov-grok-chat",
            name: "Grok Chat Upstream",
            modelIds: ["grok-2"],
          }),
        ],
        models: [nativeModel({ requestModelId: "grok-2", displayName: "Grok 2" })],
      }),
      channelSource({
        sourceChannel: "grok",
        protocol: "openai-responses",
        providers: [
          channelProvider({
            providerId: 302,
            providerUuid: "prov-grok-resp",
            name: "Grok Responses Upstream",
            modelIds: ["grok-2"],
          }),
        ],
        models: [nativeModel({ requestModelId: "grok-2", displayName: "Grok 2" })],
      }),
      channelSource({
        sourceChannel: "gemini",
        protocol: "google-generative-ai",
        providers: [
          channelProvider({
            providerId: 401,
            providerUuid: "prov-gemini-key",
            name: "Gemini API Upstream",
            authMode: "api_key",
            modelIds: ["gemini-2.0-flash"],
          }),
          channelProvider({
            providerId: 402,
            providerUuid: "prov-gemini-ent-oauth",
            name: "Gemini Enterprise OAuth",
            authMode: "oauth",
            blockedReason: "企业 Code Assist OAuth 需核实许可与适配",
            modelIds: ["gemini-2.0-flash"],
          }),
          channelProvider({
            providerId: 403,
            providerUuid: "prov-gemini-ret-oauth",
            name: "Gemini Retired Personal OAuth",
            authMode: "oauth",
            blockedReason: "个人 Code Assist 旧服务已于 2026-06-18 退役，请迁往 Antigravity",
            modelIds: ["gemini-2.0-flash"],
          }),
        ],
        models: [
          nativeModel({ requestModelId: "gemini-2.0-flash", displayName: "Gemini 2.0 Flash" }),
        ],
      }),
    ],
    bindings: [],
    ...overrides,
  };
}

export function channelPreview(
  overrides: Partial<import("../../services/nativeChannels").ChannelPreview> = {}
): import("../../services/nativeChannels").ChannelPreview {
  return {
    targetId: "pi:local:a",
    revision: "file-1",
    catalogRevision: "cat-1",
    entries: [
      {
        protocol: "openai-responses",
        nativeKey: "aio-channel-codex-openai-responses",
        baseUrl: "http://127.0.0.1:3712/pi/_aio/channel/bind-codex-1/v1",
        models: [nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" })],
        node: { api: "openai-responses" },
      },
    ],
    removedNativeKeys: [],
    ...overrides,
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
