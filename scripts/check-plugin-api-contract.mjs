import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const defaultRepoRoot = dirname(scriptDir);
const repoRoot = process.env.AIO_PLUGIN_CONTRACT_TEST_ROOT ?? defaultRepoRoot;

const failures = [];

function readText(path) {
  const fullPath = join(repoRoot, path);
  if (!existsSync(fullPath)) {
    failures.push(`${path} is missing`);
    return "";
  }
  return readFileSync(fullPath, "utf8");
}

function readJson(path) {
  const text = readText(path);
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch (error) {
    failures.push(`${path} is invalid JSON: ${error.message}`);
    return null;
  }
}

function requireIncludes(path, text, values, label) {
  for (const value of values) {
    if (!text.includes(value)) {
      failures.push(`${path} is missing ${label} ${value}`);
    }
  }
}

function requireIncludesCaseInsensitive(path, text, values, label) {
  const haystack = text.toLowerCase();
  for (const value of values) {
    if (!haystack.includes(value.toLowerCase())) {
      failures.push(`${path} is missing ${label} ${value}`);
    }
  }
}

function requireNotIncludes(path, text, values, label) {
  for (const value of values) {
    if (text.includes(value)) {
      failures.push(`${path} must not include ${label} ${value}`);
    }
  }
}

function requireRegex(path, text, regex, label) {
  if (!regex.test(text)) {
    failures.push(`${path} is missing ${label}`);
  }
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function functionBody(text, functionName) {
  const signature = new RegExp(`function\\s+${escapeRegex(functionName)}\\s*\\(`).exec(text);
  if (!signature) return null;
  const openBrace = text.indexOf("{", signature.index);
  if (openBrace === -1) return null;

  let depth = 0;
  for (let index = openBrace; index < text.length; index += 1) {
    const char = text[index];
    if (char === "{") depth += 1;
    if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        return text.slice(openBrace + 1, index);
      }
    }
  }
  return null;
}

function dependencyEntries(contract, matrix) {
  const entries = [];
  for (const hook of contract.activeHooks ?? []) {
    const hookContract = matrix?.[hook];
    if (hookContract == null || typeof hookContract !== "object" || Array.isArray(hookContract)) {
      continue;
    }
    const dependencies = hookContract.permissionDependencies;
    if (dependencies == null || typeof dependencies !== "object" || Array.isArray(dependencies)) {
      continue;
    }
    for (const [permission, requiredPermissions] of Object.entries(dependencies)) {
      if (!Array.isArray(requiredPermissions)) continue;
      for (const requiredPermission of requiredPermissions) {
        entries.push({ hook, permission, requiredPermission });
      }
    }
  }
  return entries;
}

function requireSdkPermissionDependencyContract(path, text, contract, matrix) {
  if (!/validatePermissionSet\s*\(\s*manifest\s*:\s*PluginManifest\b/.test(text)) {
    failures.push(`${path} validatePermissionSet must accept PluginManifest`);
  }
  if (!/validatePermissionSet\s*\(\s*manifest\s*\)/.test(text)) {
    failures.push(`${path} validateManifest must pass manifest to validatePermissionSet`);
  }

  const body = functionBody(text, "validatePermissionSet");
  if (body == null) {
    failures.push(`${path} is missing validatePermissionSet body`);
    return;
  }
  requireIncludes(
    path,
    body,
    ["manifest.permissions", "manifest.hooks"],
    "hook-aware permission validation"
  );

  for (const { hook, permission, requiredPermission } of dependencyEntries(contract, matrix)) {
    const hookGuard = new RegExp(`hooks\\.has\\(\\s*["']${escapeRegex(hook)}["']\\s*\\)`, "g");
    let guarded = false;
    let match = hookGuard.exec(body);
    while (match) {
      const window = body.slice(match.index, match.index + 700);
      if (window.includes(permission) && window.includes(requiredPermission)) {
        guarded = true;
        break;
      }
      match = hookGuard.exec(body);
    }
    if (!guarded) {
      failures.push(
        `${path} validatePermissionSet must guard ${permission} requires ${requiredPermission} behind ${hook}`
      );
    }
  }
}

function requireObject(path, value) {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    failures.push(`${path} must be an object`);
    return null;
  }
  return value;
}

function requireArray(path, value) {
  if (!Array.isArray(value)) {
    failures.push(`${path} must be an array`);
    return [];
  }
  return value;
}

function requireUniqueArray(path, values) {
  const seen = new Set();
  for (const value of values) {
    if (seen.has(value)) {
      failures.push(`${path} contains duplicate ${value}`);
    }
    seen.add(value);
  }
}

function requireOneOf(path, value, allowed) {
  if (!allowed.includes(value)) {
    failures.push(`${path} must be one of ${allowed.join(", ")}`);
  }
}

function runtimeTokens(contract) {
  return [...contract.communityRuntimes, ...contract.policyGatedRuntimes];
}

function officialRuntimeTokens(contract) {
  return contract.officialRuntimes.flatMap((runtime) => runtime.split(":"));
}

function snakeCase(value) {
  return value.replace(/[A-Z]/g, (match) => `_${match.toLowerCase()}`);
}

const contractPath = "docs/plugins/plugin-api-v1-contract.json";
const contract = readJson(contractPath);

if (contract) {
  const matrix = requireObject(`${contractPath}.hookMatrix`, contract.hookMatrix) ?? {};
  for (const hook of contract.activeHooks ?? []) {
    const entry = requireObject(`hookMatrix.${hook}`, matrix[hook]);
    if (!entry) continue;
    requireOneOf(`hookMatrix.${hook}.kind`, entry.kind, ["request", "response", "stream", "log"]);
    requireOneOf(`hookMatrix.${hook}.status`, entry.status, ["active", "reserved"]);
    if (entry.status !== "active") {
      failures.push(`hookMatrix.${hook}.status must be active`);
    }
    const readPermissions = requireArray(
      `hookMatrix.${hook}.readPermissions`,
      entry.readPermissions
    );
    const writePermissions = requireArray(
      `hookMatrix.${hook}.writePermissions`,
      entry.writePermissions
    );
    requireUniqueArray(`hookMatrix.${hook}.readPermissions`, readPermissions);
    requireUniqueArray(`hookMatrix.${hook}.writePermissions`, writePermissions);
    const permissionDependencies =
      requireObject(`hookMatrix.${hook}.permissionDependencies`, entry.permissionDependencies) ??
      {};
    for (const [permission, requires] of Object.entries(permissionDependencies)) {
      if (!writePermissions.includes(permission)) {
        failures.push(
          `hookMatrix.${hook}.permissionDependencies.${permission} must be a write permission`
        );
      }
      const requiredPermissions = requireArray(
        `hookMatrix.${hook}.permissionDependencies.${permission}`,
        requires
      );
      for (const requiredPermission of requiredPermissions) {
        if (!readPermissions.includes(requiredPermission)) {
          failures.push(
            `hookMatrix.${hook}.permissionDependencies.${permission} requires unknown read permission ${requiredPermission}`
          );
        }
      }
    }
    const mutationFields = requireArray(`hookMatrix.${hook}.mutationFields`, entry.mutationFields);
    const contextFields = requireArray(`hookMatrix.${hook}.contextFields`, entry.contextFields);
    requireUniqueArray(`hookMatrix.${hook}.mutationFields`, mutationFields);
    requireUniqueArray(`hookMatrix.${hook}.contextFields`, contextFields);
    if (entry.defaultFailurePolicy !== contract.defaultFailurePolicy) {
      failures.push(`hookMatrix.${hook}.defaultFailurePolicy must equal defaultFailurePolicy`);
    }
    if (entry.timeoutMs !== contract.defaultHookTimeoutMs) {
      failures.push(`hookMatrix.${hook}.timeoutMs must equal defaultHookTimeoutMs`);
    }
    requireOneOf(`hookMatrix.${hook}.reservedHeaderPolicy`, entry.reservedHeaderPolicy, [
      "block-gateway-owned",
    ]);
  }

  const sdk = readText("packages/plugin-sdk/src/index.ts");
  requireIncludes("packages/plugin-sdk/src/index.ts", sdk, contract.activeHooks, "active hook");
  requireIncludes("packages/plugin-sdk/src/index.ts", sdk, contract.reservedHooks, "reserved hook");
  requireIncludes(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    contract.activePermissions,
    "active permission"
  );
  requireIncludes(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    contract.reservedPermissions,
    "reserved permission"
  );
  requireIncludes("packages/plugin-sdk/src/index.ts", sdk, runtimeTokens(contract), "runtime");
  requireIncludes(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    contract.activeMutationFields ?? [],
    "active mutation field"
  );
  requireSdkPermissionDependencyContract("packages/plugin-sdk/src/index.ts", sdk, contract, matrix);

  const scaffold = readText("packages/create-aio-plugin/src/scaffold.ts");
  requireIncludes(
    "packages/create-aio-plugin/src/scaffold.ts",
    scaffold,
    contract.communityRuntimes,
    "community runtime"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/scaffold.ts",
    scaffold,
    contract.policyGatedRuntimes,
    "policy-gated runtime"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/scaffold.ts",
    scaffold,
    ["gateway.request.afterBodyRead", "request.body.read", "request.body.write"],
    "default scaffold contract token"
  );

  const devtools = readText("packages/create-aio-plugin/src/devtools.ts");
  requireIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    contract.activeHooks,
    "developer tool active hook"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    contract.activePermissions,
    "developer tool active permission"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    runtimeTokens(contract),
    "developer tool runtime"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    contract.activeMutationFields ?? [],
    "developer tool mutation field"
  );
  requireIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    [
      "doctorPluginFiles",
      "validatePluginFilesStrict",
      "replayHookExplain",
      "PLUGIN_RULE_PERMISSION_MISMATCH",
      "PLUGIN_REPLAY_UNSUPPORTED_RUNTIME",
      "PLUGIN_WASM_POLICY_GATED",
    ],
    "developer tool diagnostic surface"
  );
  requireNotIncludes(
    "packages/create-aio-plugin/src/devtools.ts",
    devtools,
    ["contextPatch"],
    "legacy mutation field"
  );

  const rustContract = readText("src-tauri/src/gateway/plugins/contract.rs");
  requireIncludes(
    "src-tauri/src/gateway/plugins/contract.rs",
    rustContract,
    contract.activeHooks,
    "active hook"
  );
  requireIncludes(
    "src-tauri/src/gateway/plugins/contract.rs",
    rustContract,
    contract.reservedHooks,
    "reserved hook"
  );
  requireIncludes(
    "src-tauri/src/gateway/plugins/contract.rs",
    rustContract,
    contract.activePermissions,
    "active permission"
  );
  requireIncludes(
    "src-tauri/src/gateway/plugins/contract.rs",
    rustContract,
    contract.reservedPermissions,
    "reserved permission"
  );

  const rust = readText("src-tauri/src/domain/plugins.rs");
  requireIncludes(
    "src-tauri/src/domain/plugins.rs",
    rust,
    [...contract.activePermissions, ...contract.reservedPermissions],
    "permission risk"
  );
  requireIncludes(
    "src-tauri/src/domain/plugins.rs",
    rust,
    [
      "crate::gateway::plugins::contract::is_active_hook",
      "crate::gateway::plugins::contract::is_reserved_hook",
      "crate::gateway::plugins::contract::is_reserved_permission",
      "crate::gateway::plugins::contract::hook_contract",
    ],
    "contract metadata call-through"
  );
  requireIncludesCaseInsensitive(
    "src-tauri/src/domain/plugins.rs",
    rust,
    [...runtimeTokens(contract), ...officialRuntimeTokens(contract)],
    "runtime"
  );
  requireIncludes(
    "src-tauri/src/gateway/plugins/pipeline.rs",
    readText("src-tauri/src/gateway/plugins/pipeline.rs"),
    [`Duration::from_millis(${contract.defaultHookTimeoutMs})`, "FailurePolicy::FailOpen"],
    "default hook policy"
  );

  const manifestSpec = readText("docs/plugin-manifest-v1.md");
  requireIncludes("docs/plugin-manifest-v1.md", manifestSpec, contract.activeHooks, "active hook");
  requireIncludes(
    "docs/plugin-manifest-v1.md",
    manifestSpec,
    contract.reservedHooks,
    "reserved hook"
  );
  requireIncludes(
    "docs/plugin-manifest-v1.md",
    manifestSpec,
    contract.activePermissions,
    "active permission"
  );
  requireIncludes(
    "docs/plugin-manifest-v1.md",
    manifestSpec,
    contract.reservedPermissions,
    "reserved permission"
  );
  for (const hook of contract.activeHooks ?? []) {
    const entry = matrix?.[hook];
    if (!entry) continue;
    const timeoutToken = `${entry.timeoutMs} ms`;
    const hookRow = manifestSpec
      .split("\n")
      .find((line) => line.includes(`| \`${hook}\``));
    if (!hookRow) {
      failures.push(`docs/plugin-manifest-v1.md is missing hook table row for ${hook}`);
      continue;
    }
    if (!hookRow.includes(timeoutToken)) {
      failures.push(
        `docs/plugin-manifest-v1.md hook ${hook} row must include timeout ${timeoutToken}`
      );
    }
  }

  const hooksDocPath = "docs/plugins/reference/hooks.md";
  const hooksDoc = readText(hooksDocPath);
  requireIncludes(hooksDocPath, hooksDoc, contract.activeHooks, "active hook");
  requireIncludes(hooksDocPath, hooksDoc, contract.reservedHooks, "reserved hook");

  const permissionsDocPath = "docs/plugins/reference/permissions.md";
  const permissionsDoc = readText(permissionsDocPath);
  requireIncludes(
    permissionsDocPath,
    permissionsDoc,
    contract.activePermissions,
    "active permission"
  );
  requireIncludes(
    permissionsDocPath,
    permissionsDoc,
    contract.reservedPermissions,
    "reserved permission"
  );

  const manifestGuidePath = "docs/plugins/reference/manifest.md";
  const manifestGuide = readText(manifestGuidePath);
  requireIncludes(
    manifestGuidePath,
    manifestGuide,
    [...runtimeTokens(contract), ...officialRuntimeTokens(contract)],
    "runtime"
  );

  const wasmGuidePath = "docs/plugins/runtime/wasm.md";
  const wasmGuide = readText(wasmGuidePath);
  requireIncludes(
    wasmGuidePath,
    wasmGuide,
    ["wasm", "PLUGIN_RUNTIME_DISABLED"],
    "WASM policy token"
  );

  requireRegex(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    /export type ActiveGatewayHookName\s*=([\s\S]*?)export type ReservedGatewayHookName/,
    "ActiveGatewayHookName union"
  );
  requireRegex(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    /export type ReservedGatewayHookName\s*=([\s\S]*?)export type GatewayHookName/,
    "ReservedGatewayHookName union"
  );
  requireRegex(
    "src-tauri/src/domain/plugins.rs",
    rust,
    /pub fn is_active_gateway_hook\(hook: &str\)([\s\S]*?)pub fn is_reserved_gateway_hook/,
    "active hook validation helper"
  );
  requireRegex(
    "src-tauri/src/domain/plugins.rs",
    rust,
    /pub fn is_reserved_gateway_hook\(hook: &str\)([\s\S]*?)pub fn is_reserved_permission/,
    "reserved hook validation helper"
  );
  requireIncludes(
    "src-tauri/src/domain/plugins.rs",
    rust,
    ["PLUGIN_RESERVED_HOOK", "PLUGIN_RESERVED_PERMISSION"],
    "reserved validation error"
  );
  requireNotIncludes(
    "packages/plugin-sdk/src/index.ts",
    sdk,
    ["contextPatch"],
    "legacy mutation field"
  );
  requireNotIncludes(
    "packages/create-aio-plugin/src/scaffold.ts",
    scaffold,
    ["contextPatch"],
    "legacy mutation field"
  );

  const wasmSdk = readText("packages/plugin-wasm-sdk/src/lib.rs");
  requireIncludes(
    "packages/plugin-wasm-sdk/src/lib.rs",
    wasmSdk,
    (contract.activeMutationFields ?? []).map(snakeCase),
    "active mutation field"
  );
  requireIncludes(
    "packages/plugin-wasm-sdk/src/lib.rs",
    wasmSdk,
    ['#[serde(rename_all = "camelCase")]'],
    "camelCase serde ABI"
  );
}

if (failures.length > 0) {
  console.error("Plugin API contract check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}
