import type { PluginManifest } from "@aio-coding-hub/plugin-sdk";

export type ScaffoldTemplate =
  | "rule"
  | "wasm"
  | "example:prompt-helper"
  | "example:redactor"
  | "example:response-guard";

export type ScaffoldInput = {
  id: string;
  name: string;
  template: ScaffoldTemplate;
};

export type ScaffoldFiles = Record<string, string>;

export function createPluginScaffold(input: ScaffoldInput): ScaffoldFiles {
  const id = normalizeId(input.id);
  const name = normalizeName(input.name);

  switch (input.template) {
    case "wasm":
      return wasmTemplate(id, name);
    case "example:prompt-helper":
      return promptHelperExampleTemplate(id, name);
    case "example:redactor":
      return redactorExampleTemplate(id, name);
    case "example:response-guard":
      return responseGuardExampleTemplate(id, name);
    case "rule":
    default:
      return ruleTemplate(id, name);
  }
}

function ruleTemplate(id: string, name: string): ScaffoldFiles {
  const manifest: PluginManifest = {
    id,
    name,
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
    hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
    permissions: ["request.body.read", "request.body.write"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    description: "Declarative rule plugin scaffold.",
  };

  return {
    "plugin.json": `${JSON.stringify(manifest, null, 2)}\n`,
    "rules/main.json": `${JSON.stringify(
      {
        rules: [
          {
            id: "redact-placeholder",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body", jsonPath: "$.messages[*].content" },
            match: { regex: "SECRET_[A-Za-z0-9_]+", caseSensitive: true },
            action: { kind: "replace", replacement: "[REDACTED]" },
          },
        ],
      },
      null,
      2
    )}\n`,
    "README.md": `# ${name}\n\nPlugin ID: \`${id}\`.\n\nThis scaffold uses declarative rules and does not execute JavaScript in the host.\n`,
  };
}

function wasmTemplate(id: string, name: string): ScaffoldFiles {
  const manifest: PluginManifest = {
    id,
    name,
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "wasm", abiVersion: "1.0.0", memoryLimitBytes: 16 * 1024 * 1024 },
    hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
    permissions: ["request.meta.read"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    entry: "plugin.wasm",
    description: "Experimental WASM plugin scaffold.",
  };

  return {
    "plugin.json": `${JSON.stringify(manifest, null, 2)}\n`,
    "src/lib.rs": `#[no_mangle]\npub extern "C" fn aio_plugin_handle(_ptr: i32, _len: i32) -> i64 {\n    0\n}\n`,
    "README.md": `# ${name}\n\nPlugin ID: \`${id}\`.\n\nThis template packages a WASM artifact and validates the ABI, but gateway execution remains policy-gated. The host rejects enablement with PLUGIN_RUNTIME_DISABLED until WASM execution is enabled by host policy.\n`,
  };
}

function promptHelperExampleTemplate(id: string, name: string): ScaffoldFiles {
  const manifest: PluginManifest = {
    id,
    name,
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
    hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
    permissions: ["request.body.read", "request.body.write"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    description: "Prompt helper example for request body policy hints.",
  };
  const claudeRequestBody = JSON.stringify(
    {
      model: "claude-3-5-sonnet",
      messages: [{ role: "user", content: "Summarize this release note." }],
    },
    null,
    2
  );
  const codexRequestBody = JSON.stringify(
    {
      model: "gpt-5-codex",
      input: [
        {
          role: "user",
          content: [
            {
              type: "input_text",
              text: "CODEX_PROMPT_HELPER: explain this patch.",
            },
          ],
        },
      ],
    },
    null,
    2
  );

  return {
    "plugin.json": jsonFile(manifest),
    "rules/main.json": jsonFile({
      rules: [
        {
          id: "prompt-helper-claude",
          hook: "gateway.request.afterBodyRead",
          target: { field: "request.body" },
          match: { regex: "claude-[A-Za-z0-9.-]+", caseSensitive: false },
          action: {
            kind: "appendMessage",
            role: "system",
            content: "Keep answers concise and call out assumptions explicitly.",
          },
        },
        {
          id: "prompt-helper-codex",
          hook: "gateway.request.afterBodyRead",
          target: { field: "request.body", jsonPath: "$.input[*].content[*].text" },
          match: { regex: "CODEX_PROMPT_HELPER" },
          action: {
            kind: "replace",
            replacement: "Keep answers concise",
          },
        },
      ],
    }),
    "fixtures/claude-request.json": jsonFile({
      request: { body: claudeRequestBody },
    }),
    "fixtures/codex-request.json": jsonFile({
      request: { body: codexRequestBody },
    }),
    "README.md": exampleReadme(
      name,
      id,
      "Adds lightweight prompt guidance to supported request bodies before the gateway sends them upstream.",
      [
        "create-aio-plugin validate --strict .",
        "create-aio-plugin replay --explain . fixtures/claude-request.json gateway.request.afterBodyRead",
        "create-aio-plugin replay --explain . fixtures/codex-request.json gateway.request.afterBodyRead",
      ]
    ),
  };
}

function redactorExampleTemplate(id: string, name: string): ScaffoldFiles {
  const manifest: PluginManifest = {
    id,
    name,
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
    hooks: [
      { name: "gateway.request.beforeSend", priority: 100 },
      { name: "log.beforePersist", priority: 100 },
    ],
    permissions: ["request.body.read", "request.body.write", "log.redact"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    description: "Redactor example for request bodies and log messages.",
  };

  return {
    "plugin.json": jsonFile(manifest),
    "rules/main.json": jsonFile({
      rules: [
        {
          id: "redact-request-secrets",
          hook: "gateway.request.beforeSend",
          target: { field: "request.body" },
          match: { regex: "(api_key|token|password)=[A-Za-z0-9_-]+", caseSensitive: false },
          action: { kind: "replace", replacement: "[REDACTED]" },
        },
        {
          id: "redact-log-secrets",
          hook: "log.beforePersist",
          target: { field: "log.message" },
          match: { regex: "(api_key|token|password)=[A-Za-z0-9_-]+", caseSensitive: false },
          action: { kind: "replace", replacement: "[REDACTED]" },
        },
      ],
    }),
    "fixtures/request-hit.json": jsonFile({
      request: { body: "POST /v1/chat api_key=sk_live_12345 payload=hello" },
    }),
    "fixtures/request-miss.json": jsonFile({
      request: { body: "POST /v1/chat payload=hello" },
    }),
    "fixtures/log-redact.json": jsonFile({
      log: { message: "provider retry used token=debug_98765" },
    }),
    "README.md": exampleReadme(
      name,
      id,
      "Redacts simple secret-shaped values from request bodies and log messages with Plugin API v1 declarative rules.",
      [
        "create-aio-plugin validate --strict .",
        "create-aio-plugin replay --explain . fixtures/request-hit.json gateway.request.beforeSend",
        "create-aio-plugin replay --explain . fixtures/log-redact.json log.beforePersist",
      ]
    ),
  };
}

function responseGuardExampleTemplate(id: string, name: string): ScaffoldFiles {
  const manifest: PluginManifest = {
    id,
    name,
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "declarativeRules", rules: ["rules/main.json"] },
    hooks: [{ name: "gateway.response.after", priority: 100 }],
    permissions: ["response.body.read", "response.body.write"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    description: "Response guard example for review markers in provider output.",
  };

  return {
    "plugin.json": jsonFile(manifest),
    "rules/main.json": jsonFile({
      rules: [
        {
          id: "response-guard-review-marker",
          hook: "gateway.response.after",
          target: { field: "response.body" },
          match: { regex: "(delete production|rm -rf|drop database)", caseSensitive: false },
          action: {
            kind: "replace",
            replacement: "[REVIEW_REQUIRED]",
          },
        },
      ],
    }),
    "fixtures/response-warn.json": jsonFile({
      response: { body: "The suggested next step is to run rm -rf /tmp/cache." },
    }),
    "fixtures/response-pass.json": jsonFile({
      response: { body: "The suggested next step is to review the diff and run tests." },
    }),
    "README.md": exampleReadme(
      name,
      id,
      "Marks risky response text for review after the gateway receives the provider response.",
      [
        "create-aio-plugin validate --strict .",
        "create-aio-plugin replay --explain . fixtures/response-warn.json gateway.response.after",
        "create-aio-plugin replay --explain . fixtures/response-pass.json gateway.response.after",
      ]
    ),
  };
}

function jsonFile(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function exampleReadme(
  name: string,
  id: string,
  summary: string,
  commands: readonly string[]
): string {
  const commandList = commands.map((command) => `- \`${command}\``).join("\n");
  return `# ${name}

Plugin ID: \`${id}\`.

${summary}

This example is a development template, not a default installable marketplace package.

It uses Plugin API v1 declarative rules only and does not need JavaScript, WebView, file, network, secret, or plugin storage permissions.

## Try it

${commandList}
`;
}

function normalizeId(value: string): string {
  const id = value.trim();
  if (!/^[a-z0-9][a-z0-9-]*(\.[a-z0-9][a-z0-9-]*)+$/.test(id)) {
    throw new Error("PLUGIN_INVALID_ID: expected publisher.plugin-name");
  }
  return id;
}

function normalizeName(value: string): string {
  const name = value.trim();
  if (!name) {
    throw new Error("PLUGIN_INVALID_NAME: plugin name is required");
  }
  return name;
}
