import {
  type ActivationEvent,
  type GatewayHookName,
  type PluginApi,
  type PluginCapability,
  type PluginContributes,
  type PluginHookResult,
  type PluginManifest,
  type PluginPermission,
  type PluginRuntime,
  type UiContributionSlot,
  permissionRisk,
  validateManifest,
} from "./index";

const gatewayHook: GatewayHookName = "gateway.request.afterBodyRead";
const permission: PluginPermission = "request.body.read";
const activationEvent: ActivationEvent = "onProviderEditor:openrouter";
const capability: PluginCapability = "provider.extensionValues";
const privacyCapability: PluginCapability = "privacy.redact";
const slot: UiContributionSlot = "providers.editor.sections";

if (permissionRisk(permission) !== "high") {
  throw new Error("unexpected risk");
}

const manifest: PluginManifest = {
  id: "acme.openrouter",
  name: "OpenRouter Provider",
  version: "0.1.0",
  apiVersion: "1.0.0",
  main: "dist/extension.js",
  runtime: { kind: "extensionHost", language: "typescript" },
  activationEvents: [
    "onStartup",
    "onCommand:acme.openrouter.refreshModels",
    activationEvent,
    "onGatewayHook:gateway.request.afterBodyRead",
    "onProtocolBridge:acme.bridge.openai-gemini",
  ],
  contributes: {
    providers: [
      {
        providerType: "openrouter",
        displayName: "OpenRouter",
        targetCliKeys: ["claude", "codex"],
        extensionNamespace: "openrouter",
      },
    ],
    commands: [
      {
        command: "acme.openrouter.refreshModels",
        title: "Refresh OpenRouter models",
        category: "Provider",
      },
    ],
    gatewayHooks: [{ name: gatewayHook, priority: 10 }],
    protocolBridges: [
      {
        bridgeType: "acme.bridge.openai-gemini",
        inboundProtocol: "openai.chat",
        outboundProtocol: "gemini.generateContent",
        supportsStreaming: true,
      },
    ],
    ui: {
      [slot]: [
        {
          id: "openrouter-routing",
          title: "OpenRouter routing",
          schema: {
            type: "section",
            fields: [{ type: "text", key: "route", label: "Route" }],
          },
        },
      ],
      "settings.sections": [
        {
          id: "openrouter-refresh",
          title: "OpenRouter refresh",
          schema: {
            type: "panel",
            fields: [
              {
                type: "button",
                key: "refresh",
                label: "Refresh",
                command: "acme.openrouter.refreshModels",
              },
            ],
          },
        },
      ],
    },
  },
  capabilities: [
    capability,
    privacyCapability,
    "commands.execute",
    "gateway.hooks",
    "protocol.bridge",
  ],
  hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
};

const runtime: PluginRuntime = manifest.runtime;
if (runtime.kind !== "extensionHost") {
  throw new Error("unexpected extension runtime");
}

const contributes: PluginContributes = manifest.contributes ?? {};
if (contributes.commands?.[0]?.command !== "acme.openrouter.refreshModels") {
  throw new Error("command contributions should be representable");
}

if (contributes.ui?.[slot]?.[0]?.schema.type !== "section") {
  throw new Error("extension UI contributions should be representable");
}

if (contributes.providers?.[0]?.extensionNamespace !== "openrouter") {
  throw new Error("provider contributions should be representable");
}

if (contributes.gatewayHooks?.[0]?.name !== gatewayHook) {
  throw new Error("gatewayHooks contributions should be representable");
}

if (contributes.protocolBridges?.[0]?.bridgeType !== "acme.bridge.openai-gemini") {
  throw new Error("protocol bridge contributions should be representable");
}

const result = validateManifest(manifest);
if (!result.ok) {
  throw new Error(result.error.message);
}

const replaceRequestResult: PluginHookResult = {
  action: "replace",
  requestBody: "{\"messages\":[]}",
};

const replaceResponseHeadersResult: PluginHookResult = {
  action: "replace",
  headers: { "x-plugin-redacted": "1" },
  responseBody: "{\"ok\":true}",
};

const passResult: PluginHookResult = { action: "pass" };

if (
  replaceRequestResult.action !== "replace" ||
  !replaceResponseHeadersResult.headers ||
  passResult.action !== "pass"
) {
  throw new Error("host mutation hook results should be representable");
}

const pluginApi: PluginApi = {
  commands: {
    registerCommand: (_command, _handler) => undefined,
  },
  gateway: {
    registerHook: (_name, _handler) => undefined,
  },
  privacy: {
    redactText: (text) => ({ hit: true, count: 1, redacted: text }),
    redactRequestBody: (body) => ({ hit: false, count: 0, redacted: body }),
  },
};

if (
  pluginApi.privacy?.redactText("secret").count !== 1 ||
  !pluginApi.commands ||
  !pluginApi.gateway
) {
  throw new Error("host PluginApi should be representable");
}
