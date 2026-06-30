import { describe, expect, it, test } from "vitest";
import contract from "../../../docs/plugins/plugin-api-v1-contract.json";
import {
  type PluginHookContext,
  type PluginHookResult,
  type PluginApi,
  type PluginManifest,
  permissionRisk,
  validateManifest,
} from "./index";

const openRouterManifest: PluginManifest = {
  id: "acme.openrouter",
  name: "OpenRouter Provider",
  version: "0.1.0",
  apiVersion: "1.0.0",
  main: "dist/extension.js",
  runtime: { kind: "extensionHost", language: "typescript" },
  activationEvents: ["onStartup", "onProviderEditor:openrouter"],
  contributes: {
    providers: [
      {
        providerType: "openrouter",
        displayName: "OpenRouter",
        targetCliKeys: ["claude", "codex"],
        extensionNamespace: "openrouter",
      },
    ],
    ui: {
      "providers.editor.sections": [
        {
          id: "openrouter-routing",
          title: "OpenRouter 路由",
          order: 100,
          schema: {
            type: "section",
            fields: [
              { type: "text", key: "route", label: "Route" },
              { type: "boolean", key: "fallbackEnabled", label: "启用模型兜底" },
            ],
          },
        },
      ],
    },
    commands: [
      {
        command: "acme.openrouter.refreshModels",
        title: "刷新 OpenRouter 模型",
        category: "Provider",
      },
    ],
  },
  capabilities: ["provider.extensionValues", "commands.execute"],
  hostCompatibility: {
    app: ">=0.62.0 <1.0.0",
    pluginApi: "^1.0.0",
    platforms: ["macos", "windows", "linux"],
  },
};

describe("validateManifest", () => {
  test("validates extension host provider manifest", () => {
    expect(validateManifest(openRouterManifest)).toEqual({ ok: true });
  });

  test("rejects wasm as unsupported public runtime", () => {
    const manifest = {
      ...openRouterManifest,
      runtime: { kind: "wasm", abiVersion: "1.0.0" },
      hooks: [{ name: "gateway.request.afterBodyRead" }],
      permissions: ["request.body.read"],
    } as unknown as PluginManifest;

    expect(validateManifest(manifest)).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_UNSUPPORTED_RUNTIME",
        message: "community plugins must use extensionHost runtime",
      },
    });
  });

  test("rejects unknown contribution fields", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: {
        legacyRules: [{ rules: ["rules/main.json"] }],
      },
    };

    expect(validateManifest(manifest as PluginManifest)).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_INVALID_CONTRIBUTION",
        message: "unsupported contribution field: legacyRules",
      },
    });
  });

  test("rejects extension host manifest without main", () => {
    const manifest = { ...openRouterManifest, main: undefined };
    expect(validateManifest(manifest as PluginManifest)).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_MISSING_MAIN",
        message: "extensionHost runtime requires main",
      },
    });
  });

  test("rejects unknown UI contribution slot", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: {
        ui: {
          "providers.editor.unknown": [],
        },
      },
    };
    expect(validateManifest(manifest as PluginManifest).ok).toBe(false);
  });

  test("validates protocol bridge manifest", () => {
    const manifest: PluginManifest = {
      id: "acme.bridge",
      name: "Claude OpenAI Gemini Bridge",
      version: "0.1.0",
      apiVersion: "1.0.0",
      main: "dist/extension.js",
      runtime: { kind: "extensionHost", language: "typescript" },
      activationEvents: ["onProtocolBridge:acme.bridge.openai-gemini"],
      contributes: {
        protocols: [
          { protocolId: "openai.chat", direction: "both" },
          { protocolId: "gemini.generateContent", direction: "both" },
        ],
        protocolBridges: [
          {
            bridgeType: "acme.bridge.openai-gemini",
            inboundProtocol: "openai.chat",
            outboundProtocol: "gemini.generateContent",
            supportsStreaming: true,
          },
        ],
      },
      capabilities: ["protocol.bridge"],
      hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
    };

    expect(validateManifest(manifest)).toEqual({ ok: true });
  });

  test("rejects non-namespaced protocol bridge contribution", () => {
    const manifest: PluginManifest = {
      id: "acme.bridge",
      name: "Claude OpenAI Gemini Bridge",
      version: "0.1.0",
      apiVersion: "1.0.0",
      main: "dist/extension.js",
      runtime: { kind: "extensionHost", language: "typescript" },
      contributes: {
        protocolBridges: [
          {
            bridgeType: "openai-gemini",
            inboundProtocol: "openai.chat",
            outboundProtocol: "gemini.generateContent",
          },
        ],
      },
      capabilities: ["protocol.bridge"],
      hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
    };

    expect(validateManifest(manifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION" },
    });
  });

  test("rejects invalid protocol bridge contribution id", () => {
    const manifest: PluginManifest = {
      id: "acme.bridge",
      name: "Claude OpenAI Gemini Bridge",
      version: "0.1.0",
      apiVersion: "1.0.0",
      main: "dist/extension.js",
      runtime: { kind: "extensionHost", language: "typescript" },
      contributes: {
        protocolBridges: [
          {
            bridgeType: "acme.bridge.OpenAI",
            inboundProtocol: "openai.chat",
            outboundProtocol: "gemini.generateContent",
          },
        ],
      },
      capabilities: ["protocol.bridge"],
      hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
    };

    expect(validateManifest(manifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION" },
    });
  });

  test("rejects malformed provider contribution", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: {
        providers: [
          {
            providerType: "",
            displayName: "OpenRouter",
            targetCliKeys: ["claude", "openai"],
            extensionNamespace: "openrouter",
          },
        ],
      },
    };

    expect(validateManifest(manifest as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PROVIDER_CONTRIBUTION" },
    });
  });

  test("rejects non-object contributes", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: [],
    };

    expect(validateManifest(manifest as unknown as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_CONTRIBUTES" },
    });
  });

  test("rejects malformed protocol bridge contribution", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: {
        protocolBridges: [
          {
            bridgeType: "acme.bridge.openai-gemini",
            inboundProtocol: "openai.chat",
            outboundProtocol: "gemini.generateContent",
            supportsStreaming: "yes",
          },
        ],
      },
    };

    expect(validateManifest(manifest as unknown as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION" },
    });
  });

  test("rejects invalid UI field schema", () => {
    const manifest = {
      ...openRouterManifest,
      contributes: {
        ui: {
          "providers.editor.sections": [
            {
              id: "openrouter-routing",
              schema: {
                type: "section",
                fields: [{ type: "button", key: "refresh", label: "Refresh" }],
              },
            },
          ],
        },
      },
    };

    expect(validateManifest(manifest as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_UI_CONTRIBUTION" },
    });
  });

  test("rejects invalid activation event", () => {
    const manifest = {
      ...openRouterManifest,
      activationEvents: ["onStartup", "onCommand:"],
    };

    expect(validateManifest(manifest as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_ACTIVATION_EVENT" },
    });
  });

  test("validates gatewayHooks manifest", () => {
    const manifest: PluginManifest = {
      ...openRouterManifest,
      contributes: {
        gatewayHooks: [{ name: "gateway.request.afterBodyRead", priority: 10 }],
      },
      capabilities: ["gateway.hooks"],
    };

    expect(validateManifest(manifest)).toEqual({ ok: true });
  });

  test("validates privacy redaction capability for extension host plugins", () => {
    const manifest: PluginManifest = {
      ...openRouterManifest,
      contributes: {
        gatewayHooks: [
          { name: "gateway.request.afterBodyRead", priority: 5, failurePolicy: "fail-closed" },
          { name: "log.beforePersist", priority: 1, failurePolicy: "fail-closed" },
        ],
      },
      capabilities: ["gateway.hooks", "privacy.redact"],
    };

    expect(validateManifest(manifest)).toEqual({ ok: true });
  });

  test("rejects gatewayHooks with reserved or unknown hook", () => {
    const reservedHookManifest = {
      ...openRouterManifest,
      contributes: {
        gatewayHooks: [{ name: "gateway.request.received" }],
      },
      capabilities: ["gateway.hooks"],
    };
    expect(validateManifest(reservedHookManifest as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_RESERVED_HOOK" },
    });

    const unknownHookManifest = {
      ...openRouterManifest,
      contributes: {
        gatewayHooks: [{ name: "gateway.request.missing" }],
      },
      capabilities: ["gateway.hooks"],
    };
    expect(validateManifest(unknownHookManifest as unknown as PluginManifest)).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_UNKNOWN_HOOK" },
    });
  });

  test("rejects top-level legacy hooks and permissions", () => {
    expect(
      validateManifest({
        ...openRouterManifest,
        hooks: [{ name: "gateway.request.afterBodyRead" }],
      } as unknown as PluginManifest)
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_MANIFEST" },
    });

    expect(
      validateManifest({
        ...openRouterManifest,
        permissions: ["request.body.read"],
      } as unknown as PluginManifest)
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_MANIFEST" },
    });
  });

  it("rejects every reserved hook from the contract", () => {
    for (const hook of contract.reservedHooks) {
      const result = validateManifest({
        ...openRouterManifest,
        contributes: { gatewayHooks: [{ name: hook as never }] },
        capabilities: ["gateway.hooks"],
      } as PluginManifest);

      expect(result).toMatchObject({
        ok: false,
        error: { code: "PLUGIN_RESERVED_HOOK" },
      });
    }
  });

  test("rejects contributions without required capabilities", () => {
    const cases: Array<{ manifest: PluginManifest; message: string }> = [
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            commands: [{ command: "acme.openrouter.refreshModels", title: "Refresh models" }],
          },
          capabilities: [],
        },
        message: "commands contribution requires commands.execute",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            providers: [
              {
                providerType: "openrouter",
                displayName: "OpenRouter",
                targetCliKeys: ["claude", "codex"],
                extensionNamespace: "openrouter",
              },
            ],
          },
          capabilities: [],
        },
        message: "provider contribution requires provider.extensionValues",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            gatewayHooks: [{ name: "gateway.request.afterBodyRead" }],
          },
          capabilities: [],
        },
        message: "gatewayHooks contribution requires gateway.hooks",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            protocolBridges: [
              {
                bridgeType: "acme.openrouter.openai-gemini",
                inboundProtocol: "openai.chat",
                outboundProtocol: "gemini.generateContent",
              },
            ],
          },
          capabilities: [],
        },
        message: "protocolBridges contribution requires protocol.bridge",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            ui: {
              "providers.editor.sections": [
                {
                  id: "openrouter-routing",
                  schema: {
                    type: "section",
                    fields: [{ type: "text", key: "route", label: "Route" }],
                  },
                },
              ],
            },
          },
          capabilities: [],
        },
        message: "providers.editor.sections UI contribution requires provider.extensionValues",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            ui: {
              "providers.editor.fields": [
                {
                  id: "openrouter-models",
                  schema: {
                    type: "section",
                    fields: [
                      {
                        type: "select",
                        key: "model",
                        label: "Model",
                        options: [{ value: "auto", label: "Auto" }],
                      },
                    ],
                  },
                },
              ],
            },
          },
          capabilities: [],
        },
        message: "providers.editor.fields UI contribution requires provider.extensionValues",
      },
      {
        manifest: {
          ...openRouterManifest,
          contributes: {
            ui: {
              "settings.sections": [
                {
                  id: "openrouter-refresh",
                  schema: {
                    type: "section",
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
          capabilities: [],
        },
        message: "UI command field requires commands.execute",
      },
    ];

    for (const { manifest, message } of cases) {
      expect(validateManifest(manifest)).toEqual({
        ok: false,
        error: {
          code: "PLUGIN_MISSING_CAPABILITY",
          message,
        },
      });
    }
  });

  it("rejects manifests without a supported host compatibility range", () => {
    expect(
      validateManifest({
        ...openRouterManifest,
        hostCompatibility: { app: "", pluginApi: "^1.0.0" },
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_HOST_COMPATIBILITY" },
    });

    expect(
      validateManifest({
        ...openRouterManifest,
        hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^2.0.0" },
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_UNSUPPORTED_PLUGIN_API" },
    });
  });

  it("rejects future manifest apiVersion majors even when hostCompatibility supports v1", () => {
    const result = validateManifest({
      ...openRouterManifest,
      apiVersion: "2.0.0",
      hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
    });

    expect(result).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INCOMPATIBLE_API" },
    });
  });

  it("rejects wasm as an unsupported public runtime", () => {
    const result = validateManifest({
      ...openRouterManifest,
      runtime: { kind: "wasm", abiVersion: "2.0.0" },
    } as unknown as PluginManifest);

    expect(result).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_UNSUPPORTED_RUNTIME" },
    });
  });
});

describe("PluginHookResult", () => {
  it("represents host mutation fields without legacy contextPatch", () => {
    const result: PluginHookResult = {
      action: "replace",
      requestBody: '{"messages":[]}',
      responseBody: '{"ok":true}',
      headers: { "x-plugin-redacted": "1" },
    };

    expect(result).toEqual({
      action: "replace",
      requestBody: '{"messages":[]}',
      responseBody: '{"ok":true}',
      headers: { "x-plugin-redacted": "1" },
    });
    expect("contextPatch" in result).toBe(false);
  });
});

describe("PluginHookContext", () => {
  it("types provider-neutral normalized request messages", () => {
    const context: PluginHookContext = {
      hook: "gateway.request.afterBodyRead",
      traceId: "trace-sdk",
      config: {},
      context: {
        request: {
          normalizedMessages: [
            {
              role: "user",
              text: "hello from codex",
              source: "openai.responses.input_text",
            },
          ],
        },
      },
    };

    expect(context.context.request?.normalizedMessages?.[0]?.text).toBe("hello from codex");
  });
});

describe("PluginApi", () => {
  it("represents the host command, gateway, and privacy APIs", () => {
    const api: PluginApi = {
      commands: {
        registerCommand: (_command, _handler) => undefined,
      },
      gateway: {
        registerHook: (_name, _handler) => undefined,
      },
      privacy: {
        redactText: (text) => ({ hit: true, count: 1, redacted: text.replace("secret", "[密钥]") }),
        redactRequestBody: (body) => ({ hit: false, count: 0, redacted: body }),
      },
    };

    expect(api.commands).toBeDefined();
    expect(api.gateway).toBeDefined();
    expect(api.privacy?.redactText("secret").redacted).toBe("[密钥]");
  });
});

describe("permissionRisk", () => {
  it("keeps permissionRisk defined for every v1 permission", () => {
    for (const permission of [...contract.activePermissions, ...contract.reservedPermissions]) {
      expect(permissionRisk(permission as never)).toMatch(/^(low|medium|high|critical)$/);
    }
  });

  it("matches the host permission risk table", () => {
    expect(permissionRisk("response.header.read")).toBe("low");
    expect(permissionRisk("response.header.write")).toBe("medium");
    expect(permissionRisk("file.read")).toBe("high");
    expect(permissionRisk("file.write")).toBe("high");
    expect(permissionRisk("secret.read")).toBe("critical");
  });
});
