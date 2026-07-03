import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(scriptDir);
const legacyCreateAioPluginExecCommand =
  "pnpm --filter create-aio-plugin exec " + "create-aio-plugin";

const requiredDocs = [
  {
    path: "docs/plugin-system-rfc.md",
    phrases: [
      "短期不执行任意 JavaScript/TypeScript",
      "提示词优化只能在网关请求阶段可靠实现",
      "第三方代码不得直接进入主进程或 WebView",
      "Skill 市场",
      "gateway.request.afterBodyRead",
      "Final Gateway Hook Chain",
      "gateway.request.beforeSend (active upstream header/body mutation)",
      "gateway.response.chunk (active SSE chunk inspect/modify/block)",
      "log.beforePersist (active request log redaction before enqueue)",
      "WASM",
      "public manifests use `capabilities`",
      "Extension Host public manifest 不支持 top-level `permissions`",
      "capability changes",
    ],
    forbiddenPhrases: [
      "granted permissions",
      "body-read permissions",
      "write permissions",
      "add permissions",
      "permission changes",
    ],
  },
  {
    path: "docs/plugin-manifest-v1.md",
    phrases: [
      "publisher.plugin-name",
      "SemVer",
      "apiVersion",
      "hostCompatibility",
      "main",
      'runtime.kind = "extensionHost"',
      "contributes.gatewayHooks",
      'capabilities: ["gateway.hooks"]',
      "api.gateway.registerHook",
      "commands -> commands.execute",
      "providers / provider UI -> provider.extensionValues",
      "gatewayHooks -> gateway.hooks",
      "protocolBridges -> protocol.bridge",
      "Protocol bridge MVP skeleton",
      "gateway.response.chunk",
      "Active hooks in plugin API v1",
      "Reserved hooks for future host integration",
      "Reserved permissions for future host-mediated APIs",
      "request.header.readSensitive",
      "official.privacy-filter",
      "acme.prompt-helper",
      "quarantined",
      "Extension Host public manifest 不支持 top-level `permissions`",
      "High-risk 和 critical labels",
    ],
    forbiddenPhrases: ['"kind": "wasm"'],
  },
  {
    path: "docs/plugins/README.md",
    phrases: [
      "插件开发手册",
      "插件开发总指南",
      "按目标查找",
      "插件 API 参考",
      "Privacy Filter 示例",
      "Manifest",
      "Hooks",
      "Permissions",
    ],
  },
  {
    path: "docs/plugins/developer-guide.md",
    phrases: [
      "插件开发总指南",
      "plugin.json",
      "Extension Host",
      "doctor -> validate --strict -> pack -> publish-check -> install/update -> export replay fixture -> fix -> reinstall",
      "plugin_export_replay_fixture",
      "publish-check",
      "dist/extension.js",
      'runtime.kind = "extensionHost"',
      "contributes.gatewayHooks",
      "capabilities",
      "api.gateway.registerHook",
      "gateway.request.beforeSend",
      "request.normalizedMessages",
      "configSchema",
      "x-aio-ui",
      "pnpm --filter create-aio-plugin cli validate",
      "pnpm --filter create-aio-plugin cli pack",
      "PLUGIN_REPLAY_UNSUPPORTED",
      ".aio-plugin",
      "official.privacy-filter",
      "精选插件",
      "高级来源",
      "example:prompt-helper",
      "example:redactor",
      "example:response-guard",
      "示例是开发模板，不是默认可安装市场包",
    ],
    forbiddenPhrases: [
      "WASM 适合需要确定性代码逻辑的插件",
      "pnpm plugin-wasm-sdk:test",
      "最小声明式规则插件",
      "pnpm --filter create-aio-plugin cli replay",
      legacyCreateAioPluginExecCommand,
    ],
  },
  {
    path: "docs/plugins/runtime/wasm.md",
    phrases: [
      "unsupported pre-release legacy runtime",
      "not part of the public Plugin API v1 community runtime surface",
      "community plugins must migrate to Extension Host",
      'runtime.kind = "extensionHost"',
    ],
    forbiddenPhrases: [
      "WASM packages are installable only when host policy enables execution",
      "WASM 只用于宿主策略启用后",
      "插件作者应使用",
    ],
  },
  {
    path: "docs/plugins/runtime/process-poc.md",
    phrases: [
      "unsupported pre-release legacy runtime",
      "not part of the public Plugin API v1 community runtime surface",
      "Extension Host",
      "JSON-RPC over stdio",
      "disabled by default",
    ],
    forbiddenPhrases: ["服务于未来无法放进 WASM ABI"],
  },
  {
    path: "docs/plugins/developer-guide.md",
    phrases: [
      "create-aio-plugin",
      "pnpm --filter create-aio-plugin cli",
      "pnpm --filter create-aio-plugin cli validate",
      "pnpm --filter create-aio-plugin cli pack",
      "pnpm --filter create-aio-plugin cli publish-check",
      "从 Plugins 页面本地安装",
      "Claude 和 Codex request shapes",
      "@aio-coding-hub/plugin-sdk",
      "plugin.json",
      "最小 Extension Host 插件",
      "Extension Host",
    ],
  },
  {
    path: "docs/plugins/reference/sdk.md",
    phrases: [
      "@aio-coding-hub/plugin-sdk",
      "PluginManifest",
      "validateManifest",
      "permissionRisk",
      "Extension Host",
      'runtime: { kind: "extensionHost"',
      "api.gateway.registerHook",
      "SDK 边界",
    ],
    forbiddenPhrases: ["aio-plugin-wasm-sdk"],
  },
  {
    path: "docs/plugins/examples/privacy-filter.md",
    phrases: [
      "official.privacy-filter",
      "packyme/privacy-filter",
      "Extension Host",
      "privacy.redact",
      "api.privacy.redactRequestBody",
      "已移除的内置示例",
    ],
    forbiddenPhrases: ["native:privacyFilter", "host-owned built-in"],
  },
  {
    path: "docs/plugins/examples/README.md",
    phrases: [
      "example:prompt-helper",
      "example:redactor",
      "example:response-guard",
      "fixtures/claude-request.json",
      "fixtures/response-warn.json",
      "不是默认可安装市场包",
      "checksum",
      "signature",
      "托管",
      "市场索引流程",
    ],
  },
  {
    path: "docs/plugins/architecture/audit.md",
    phrases: [
      "official.privacy-filter",
      "Extension Host",
      "gatewayHooks",
      "protocolBridges",
      "unsupported pre-release legacy runtime",
      "信任边界",
      "性能与稳定性建议",
      "0.62 does not add public provider plugin APIs",
    ],
    caseInsensitivePhrases: ["provider adapter facades remain internal"],
  },
  {
    path: "docs/plugins/reference/manifest.md",
    phrases: [
      "apiVersion",
      "hostCompatibility",
      'runtime.kind = "extensionHost"',
      "main",
      "contributes.gatewayHooks",
      "capabilities",
      "Protocol bridge MVP skeleton",
    ],
    forbiddenPhrases: ['{ "kind": "wasm"'],
  },
  {
    path: "docs/plugins/reference/hooks.md",
    phrases: [
      "gateway.request.afterBodyRead",
      "gateway.response.chunk",
      "log.beforePersist",
      "plugin_hook_execution_reports",
      "plugin_export_replay_fixture",
      "默认 vNext hook timeout: 5000 ms",
    ],
  },
  {
    path: "docs/plugins/reference/permissions.md",
    phrases: ["request.body.read", "secret.read", "critical", "新增 capability 需要用户重新确认"],
  },
  {
    path: "docs/plugins/reference/config-schema.md",
    phrases: [
      "string",
      "number",
      "boolean",
      "password",
      "enum is supported as a keyword",
      "vNext does not provide host-managed secret storage",
    ],
  },
  {
    path: "docs/plugins/architecture/security.md",
    phrases: [
      "fail-closed",
      "quarantined",
      "Extension Host",
      "不在 Rust 主进程或 Tauri WebView 执行第三方插件代码",
      "默认 vNext hook timeout: 5000 ms",
    ],
  },
  {
    path: "docs/plugins/runtime/streaming.md",
    phrases: ["sliding window", "gateway.response.chunk", "stream.modify"],
  },
  {
    path: "docs/plugins/reference/publishing.md",
    phrases: [
      ".aio-plugin",
      "sha256",
      "Ed25519",
      "rollback",
      "publish-check",
      "market index URL",
      "trusted public key",
      "revoked / incompatible install blocks",
      "plugin_export_replay_fixture",
      "默认市场视图",
      "自定义 market index 属于高级来源",
      "示例模板可以运行 publish-check",
      "不代表示例已经被上传、签名、加入默认 market index",
    ],
  },
  {
    path: "docs/plugins/runtime/README.md",
    phrases: [
      "Host Runtime Lifecycle",
      "plugin_hook_execution_reports",
      "host-owned lifecycle",
      "Dispose",
    ],
  },
  {
    path: "docs/plugins/reference/compatibility.md",
    phrases: [
      "SemVer",
      "pluginApi",
      "platforms",
      "Plugin API v1 remains externally compatible in 0.62",
      "0.62 does not add public provider plugin APIs",
      "Extension Host is the only community runtime",
      "unsupported pre-release legacy runtime",
    ],
    forbiddenPhrases: ['{ "kind": "wasm"'],
  },
];

const failures = [];

const localReplayBoundaryFiles = [
  "docs/plugins/README.md",
  "docs/plugins/developer-guide.md",
  "docs/plugins/reference/sdk.md",
  "docs/plugins/reference/compatibility.md",
  "docs/plugins/architecture/audit.md",
  "docs/plugins/examples/README.md",
  "docs/plugins/examples/privacy-filter.md",
  "docs/plugins/reference/publishing.md",
  "packages/create-aio-plugin/src/scaffold.ts",
  "packages/create-aio-plugin/src/scaffold.test.ts",
  "packages/create-aio-plugin/src/devtools.ts",
];

const replaySuccessPatterns = [
  /pnpm --filter create-aio-plugin cli replay/,
  /\bcreate-aio-plugin\s+replay\b/,
  /\breplay --explain\b/,
  /validate[\s\S]{0,80}replay[\s\S]{0,80}pack/,
];

const supersededHistoricalDocsFallback = [
  "docs/superpowers/plans/2026-06-22-aio-coding-hub-0-62-1-plugin-developer-loop.md",
  "docs/superpowers/plans/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel.md",
  "docs/superpowers/plans/2026-06-25-aio-coding-hub-plugin-observability-replay-publishing.md",
  "docs/superpowers/plans/2026-06-26-aio-coding-hub-plugin-example-developer-loop-phase-1.md",
  "docs/superpowers/specs/2026-06-21-aio-coding-hub-0-62-plugin-platform-kernel-design.md",
  "docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-1-plugin-developer-loop-design.md",
  "docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md",
  "docs/superpowers/specs/2026-06-25-aio-coding-hub-plugin-observability-replay-publishing-design.md",
  "docs/superpowers/specs/2026-06-26-aio-coding-hub-plugin-example-developer-loop-phase-1-design.md",
  "docs/superpowers/specs/2026-06-27-aio-coding-hub-plugin-runtime-lifecycle-registry-design.md",
];

function lineExplainsReplayUnsupported(line) {
  return (
    line.includes("PLUGIN_REPLAY_UNSUPPORTED") ||
    line.includes("unsupported for Extension Host") ||
    line.includes("当前不执行 Extension Host gateway hooks") ||
    line.includes("不在本地执行 Extension Host gateway hooks") ||
    line.includes("not local `create-aio-plugin replay` execution") ||
    line.includes("not.toContain")
  );
}

function trackedSuperpowersMarkdownDocs() {
  const result = spawnSync(
    "git",
    ["ls-files", "docs/superpowers/plans", "docs/superpowers/specs"],
    {
      cwd: repoRoot,
      encoding: "utf8",
    }
  );
  if (result.status !== 0) {
    return supersededHistoricalDocsFallback;
  }
  return result.stdout.split(/\r?\n/).filter((path) => path.endsWith(".md"));
}

function hasReplaySuccessPath(text) {
  return replaySuccessPatterns.some((pattern) => pattern.test(text));
}

function hasSupersededHistoricalSuccessPath(text) {
  return hasReplaySuccessPath(text);
}

for (const doc of requiredDocs) {
  const fullPath = join(repoRoot, doc.path);
  if (!existsSync(fullPath)) {
    failures.push(`${doc.path}: missing required document`);
    continue;
  }

  const text = readFileSync(fullPath, "utf8");
  for (const phrase of doc.phrases) {
    if (!text.includes(phrase)) {
      failures.push(`${doc.path}: missing required phrase "${phrase}"`);
    }
  }

  const normalizedText = text.toLowerCase();
  for (const phrase of doc.caseInsensitivePhrases ?? []) {
    if (!normalizedText.includes(phrase.toLowerCase())) {
      failures.push(`${doc.path}: missing required phrase "${phrase}"`);
    }
  }

  for (const phrase of doc.forbiddenPhrases ?? []) {
    if (text.includes(phrase)) {
      failures.push(`${doc.path}: forbidden phrase "${phrase}"`);
    }
  }
}

for (const path of localReplayBoundaryFiles) {
  const fullPath = join(repoRoot, path);
  if (!existsSync(fullPath)) {
    failures.push(`${path}: missing local replay boundary file`);
    continue;
  }
  const lines = readFileSync(fullPath, "utf8").split(/\r?\n/);
  lines.forEach((line, index) => {
    if (lineExplainsReplayUnsupported(line)) return;
    if (replaySuccessPatterns.some((pattern) => pattern.test(line))) {
      failures.push(
        `${path}:${index + 1}: local create-aio-plugin replay must not be documented as a successful Extension Host hook path`
      );
    }
  });
}

for (const path of trackedSuperpowersMarkdownDocs()) {
  const fullPath = join(repoRoot, path);
  if (!existsSync(fullPath)) {
    failures.push(`${path}: missing superseded historical document`);
    continue;
  }
  const text = readFileSync(fullPath, "utf8");
  if (!hasSupersededHistoricalSuccessPath(text)) continue;
  const head = text.split(/\r?\n/).slice(0, 16).join("\n");
  if (!head.includes("Status: Superseded.") || !head.includes("MUST NOT be executed")) {
    failures.push(`${path}: historical local replay public runtime plan must be marked superseded`);
  }
}

if (failures.length > 0) {
  console.error("Plugin system documentation contract failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}
