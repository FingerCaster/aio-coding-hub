import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function writeJson(root, path, value) {
  writeFileSync(join(root, path), JSON.stringify(value, null, 2));
}

function makeRoot(name) {
  const root = join(tmpdir(), `aio-plugin-contract-${name}-${Date.now()}`);
  mkdirSync(join(root, "docs/plugins"), { recursive: true });
  mkdirSync(join(root, "docs/plugins/reference"), { recursive: true });
  mkdirSync(join(root, "docs/plugins/runtime"), { recursive: true });
  mkdirSync(join(root, "packages/plugin-sdk/src"), { recursive: true });
  mkdirSync(join(root, "packages/plugin-wasm-sdk/src"), { recursive: true });
  mkdirSync(join(root, "packages/create-aio-plugin/src"), { recursive: true });
  mkdirSync(join(root, "src-tauri/src/domain"), { recursive: true });
  mkdirSync(join(root, "src-tauri/src/gateway/plugins"), { recursive: true });
  return root;
}

function runCheck(root) {
  return spawnSync("node", ["scripts/check-plugin-api-contract.mjs"], {
    cwd: process.cwd(),
    env: { ...process.env, AIO_PLUGIN_CONTRACT_TEST_ROOT: root },
    encoding: "utf8",
  });
}

function writePassingScaffold(root) {
  writeFileSync(
    join(root, "packages/plugin-sdk/src/index.ts"),
    [
      "gateway.request.afterBodyRead gateway.response.headers",
      "request.body.read request.body.write network.fetch requestBody",
      "declarativeRules wasm",
      "export type ActiveGatewayHookName = 'gateway.request.afterBodyRead';",
      "export type ReservedGatewayHookName = 'gateway.response.headers';",
      "export type GatewayHookName = ActiveGatewayHookName | ReservedGatewayHookName;",
    ].join("\n")
  );
  writeFileSync(
    join(root, "packages/create-aio-plugin/src/scaffold.ts"),
    "declarativeRules wasm gateway.request.afterBodyRead request.body.read request.body.write"
  );
  writeFileSync(
    join(root, "src-tauri/src/gateway/plugins/contract.rs"),
    [
      "gateway.request.afterBodyRead gateway.response.headers",
      "request.body.read network.fetch",
    ].join("\n")
  );
  writeFileSync(
    join(root, "src-tauri/src/domain/plugins.rs"),
    [
      "declarativeRules wasm native privacyFilter",
      "crate::gateway::plugins::contract::is_active_hook",
      "crate::gateway::plugins::contract::is_reserved_hook",
      "crate::gateway::plugins::contract::is_reserved_permission",
      "crate::gateway::plugins::contract::hook_contract",
      "pub fn is_active_gateway_hook(hook: &str) -> bool { hook == \"gateway.request.afterBodyRead\" }",
      "pub fn is_reserved_gateway_hook(hook: &str) -> bool { hook == \"gateway.response.headers\" }",
      "pub fn is_reserved_permission(permission: &str) -> bool { permission == \"network.fetch\" }",
      "fn permission_risk(permission: &str) { request.body.read; network.fetch; }",
      "PLUGIN_RESERVED_HOOK PLUGIN_RESERVED_PERMISSION",
    ].join("\n")
  );
  writeFileSync(
    join(root, "src-tauri/src/gateway/plugins/pipeline.rs"),
    "Duration::from_millis(150) FailurePolicy::FailOpen"
  );
  writeFileSync(
    join(root, "docs/plugin-manifest-v1.md"),
    "gateway.request.afterBodyRead gateway.response.headers request.body.read network.fetch"
  );
  writeFileSync(
    join(root, "docs/plugins/reference/hooks.md"),
    "gateway.request.afterBodyRead gateway.response.headers"
  );
  writeFileSync(
    join(root, "docs/plugins/reference/permissions.md"),
    "request.body.read network.fetch"
  );
  writeFileSync(
    join(root, "docs/plugins/reference/manifest.md"),
    "declarativeRules wasm native privacyFilter"
  );
  writeFileSync(join(root, "docs/plugins/runtime/wasm.md"), "wasm PLUGIN_RUNTIME_DISABLED");
  writeFileSync(
    join(root, "packages/plugin-wasm-sdk/src/lib.rs"),
    'request_body #[serde(rename_all = "camelCase")]'
  );
}

const reservedHookRoot = makeRoot("reserved-hook");
writeJson(reservedHookRoot, "docs/plugins/plugin-api-v1-contract.json", {
  apiVersion: "1.0.0",
  defaultHookTimeoutMs: 150,
  defaultFailurePolicy: "fail-open",
  activeHooks: ["gateway.request.afterBodyRead"],
  reservedHooks: ["gateway.response.headers"],
  activeMutationFields: ["requestBody"],
  configSchemaTypes: ["object"],
  activePermissions: ["request.body.read"],
  reservedPermissions: ["network.fetch"],
  communityRuntimes: ["declarativeRules"],
  policyGatedRuntimes: ["wasm"],
  officialRuntimes: ["native:privacyFilter"],
});
writeFileSync(
  join(reservedHookRoot, "packages/plugin-sdk/src/index.ts"),
  "gateway.request.afterBodyRead request.body.read declarativeRules"
);
writeFileSync(
  join(reservedHookRoot, "packages/create-aio-plugin/src/scaffold.ts"),
  "declarativeRules gateway.request.afterBodyRead request.body.read"
);
writeFileSync(
  join(reservedHookRoot, "src-tauri/src/domain/plugins.rs"),
  "gateway.request.afterBodyRead request.body.read declarativeRules"
);
writeFileSync(
  join(reservedHookRoot, "docs/plugin-manifest-v1.md"),
  "gateway.request.afterBodyRead request.body.read"
);
writeFileSync(join(reservedHookRoot, "docs/plugins/reference/hooks.md"), "gateway.request.afterBodyRead");
writeFileSync(join(reservedHookRoot, "docs/plugins/reference/permissions.md"), "request.body.read");
writeFileSync(
  join(reservedHookRoot, "docs/plugins/reference/manifest.md"),
  "declarativeRules wasm native privacyFilter"
);
writeFileSync(join(reservedHookRoot, "docs/plugins/runtime/wasm.md"), "wasm PLUGIN_RUNTIME_DISABLED");

const reservedHookResult = runCheck(reservedHookRoot);
if (reservedHookResult.status === 0 || !reservedHookResult.stderr.includes("gateway.response.headers")) {
  throw new Error(
    `expected structural contract failure, got status ${reservedHookResult.status}\n${reservedHookResult.stderr}`
  );
}

const missingHookMetadataRoot = makeRoot("missing-hook-metadata");
writeJson(missingHookMetadataRoot, "docs/plugins/plugin-api-v1-contract.json", {
  apiVersion: "1.0.0",
  defaultHookTimeoutMs: 150,
  defaultFailurePolicy: "fail-open",
  activeHooks: ["gateway.request.afterBodyRead"],
  reservedHooks: ["gateway.response.headers"],
  activeMutationFields: ["requestBody"],
  configSchemaTypes: ["object"],
  activePermissions: ["request.body.read"],
  reservedPermissions: ["network.fetch"],
  hookMatrix: {
    "gateway.request.afterBodyRead": {
      phase: "after request body read and before upstream provider send",
      readPermissions: ["request.body.read"],
      writePermissions: [],
      contextFields: ["traceId"],
      timeoutMs: 150,
    },
  },
  communityRuntimes: ["declarativeRules"],
  policyGatedRuntimes: ["wasm"],
  officialRuntimes: ["native:privacyFilter"],
});
writePassingScaffold(missingHookMetadataRoot);

const missingHookMetadataResult = runCheck(missingHookMetadataRoot);
if (
  missingHookMetadataResult.status === 0 ||
  !missingHookMetadataResult.stderr.includes("hookMatrix.gateway.request.afterBodyRead.kind") ||
  !missingHookMetadataResult.stderr.includes("hookMatrix.gateway.request.afterBodyRead.status") ||
  !missingHookMetadataResult.stderr.includes("hookMatrix.gateway.request.afterBodyRead.mutationFields")
) {
  throw new Error(
    `expected hookMatrix metadata failure, got status ${missingHookMetadataResult.status}\n${missingHookMetadataResult.stderr}`
  );
}

const inconsistentHookMetadataRoot = makeRoot("inconsistent-hook-metadata");
writeJson(inconsistentHookMetadataRoot, "docs/plugins/plugin-api-v1-contract.json", {
  apiVersion: "1.0.0",
  defaultHookTimeoutMs: 150,
  defaultFailurePolicy: "fail-open",
  activeHooks: ["gateway.request.afterBodyRead"],
  reservedHooks: ["gateway.response.headers"],
  activeMutationFields: ["requestBody"],
  configSchemaTypes: ["object"],
  activePermissions: ["request.body.read"],
  reservedPermissions: ["network.fetch"],
  hookMatrix: {
    "gateway.request.afterBodyRead": {
      phase: "after request body read and before upstream provider send",
      kind: "request",
      status: "reserved",
      defaultFailurePolicy: "fail-closed",
      timeoutMs: 150,
      reservedHeaderPolicy: "allow-all",
      readPermissions: ["request.body.read"],
      writePermissions: [],
      mutationFields: ["requestBody"],
      contextFields: ["traceId"],
    },
  },
  communityRuntimes: ["declarativeRules"],
  policyGatedRuntimes: ["wasm"],
  officialRuntimes: ["native:privacyFilter"],
});
writePassingScaffold(inconsistentHookMetadataRoot);

const inconsistentHookMetadataResult = runCheck(inconsistentHookMetadataRoot);
if (
  inconsistentHookMetadataResult.status === 0 ||
  !inconsistentHookMetadataResult.stderr.includes(
    "hookMatrix.gateway.request.afterBodyRead.status must be active"
  ) ||
  !inconsistentHookMetadataResult.stderr.includes(
    "hookMatrix.gateway.request.afterBodyRead.defaultFailurePolicy must equal defaultFailurePolicy"
  ) ||
  !inconsistentHookMetadataResult.stderr.includes(
    "hookMatrix.gateway.request.afterBodyRead.reservedHeaderPolicy must be one of block-gateway-owned"
  )
) {
  throw new Error(
    `expected hookMatrix consistency failure, got status ${inconsistentHookMetadataResult.status}\n${inconsistentHookMetadataResult.stderr}`
  );
}
