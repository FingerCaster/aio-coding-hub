import { describe, expect, it, test } from "vitest";
import contract from "../../../docs/plugins/plugin-api-v1-contract.json";
import {
  type PluginHookContext,
  type PluginHookResult,
  type PluginManifest,
  permissionRisk,
  validateManifest,
} from "./index";

const manifest: PluginManifest = {
  id: "acme.redactor",
  name: "Redactor",
  version: "1.0.0",
  apiVersion: "1.0.0",
  runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
  hooks: [{ name: "gateway.request.afterBodyRead", priority: 10 }],
  permissions: ["request.body.read", "request.body.write"],
  hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
};

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

  it("rejects reserved hooks until the host wires them", () => {
    const result = validateManifest({
      ...manifest,
      hooks: [{ name: "gateway.request.received" }],
      permissions: ["request.meta.read"],
    });

    expect(result).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_RESERVED_HOOK",
        message:
          "hook is reserved for a future host integration and is not active in plugin API v1: gateway.request.received",
      },
    });
  });

  it("rejects every reserved hook from the contract", () => {
    for (const hook of contract.reservedHooks) {
      const result = validateManifest({
        ...manifest,
        hooks: [{ name: hook as never }],
        permissions: ["request.meta.read"],
      });

      expect(result).toMatchObject({
        ok: false,
        error: { code: "PLUGIN_RESERVED_HOOK" },
      });
    }
  });

  it("rejects reserved permissions until host-mediated APIs exist", () => {
    const result = validateManifest({
      ...manifest,
      permissions: ["request.body.read", "network.fetch"],
    });

    expect(result).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_RESERVED_PERMISSION",
        message:
          "permission is reserved for a future host-mediated API and is not active in plugin API v1: network.fetch",
      },
    });
  });

  it("rejects write permissions without their required read permissions", () => {
    expect(
      validateManifest({
        ...manifest,
        permissions: ["request.body.write"],
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PERMISSION_SET" },
    });

    expect(
      validateManifest({
        ...manifest,
        hooks: [{ name: "gateway.response.after" }],
        permissions: ["response.body.write"],
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PERMISSION_SET" },
    });

    expect(
      validateManifest({
        ...manifest,
        hooks: [{ name: "gateway.response.chunk" }],
        permissions: ["stream.modify"],
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_PERMISSION_SET" },
    });
  });

  it("allows beforeSend request body write-only manifests for host compatibility", () => {
    const result = validateManifest({
      ...manifest,
      hooks: [{ name: "gateway.request.beforeSend", priority: 10 }],
      permissions: ["request.body.write"],
    });

    expect(result).toEqual({ ok: true });
  });

  it("allows gateway error response body write-only manifests for host compatibility", () => {
    const result = validateManifest({
      ...manifest,
      hooks: [{ name: "gateway.error", priority: 10 }],
      permissions: ["response.body.write"],
    });

    expect(result).toEqual({ ok: true });
  });

  it("rejects permissions that do not apply to declared hooks", () => {
    const scopedManifest = {
      ...manifest,
      hooks: [{ name: "log.beforePersist" as const, priority: 10 }],
      permissions: ["request.body.read", "log.redact"] as const,
    };

    expect(validateManifest(scopedManifest as never)).toEqual({
      ok: false,
      error: {
        code: "PLUGIN_PERMISSION_SCOPE_MISMATCH",
        message: "permission request.body.read does not apply to any declared hook",
      },
    });
  });

  it("rejects manifests without a supported host compatibility range", () => {
    expect(
      validateManifest({
        ...manifest,
        hostCompatibility: { app: "", pluginApi: "^1.0.0" },
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INVALID_HOST_COMPATIBILITY" },
    });

    expect(
      validateManifest({
        ...manifest,
        hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^2.0.0" },
      })
    ).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_UNSUPPORTED_PLUGIN_API" },
    });
  });

  it("rejects future manifest apiVersion majors even when hostCompatibility supports v1", () => {
    const result = validateManifest({
      ...manifest,
      apiVersion: "2.0.0",
      hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    });

    expect(result).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_INCOMPATIBLE_API" },
    });
  });

  it("rejects wasm ABI versions outside v1", () => {
    const result = validateManifest({
      ...manifest,
      runtime: { kind: "wasm", abiVersion: "2.0.0" },
    });

    expect(result).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_UNSUPPORTED_WASM_ABI" },
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
