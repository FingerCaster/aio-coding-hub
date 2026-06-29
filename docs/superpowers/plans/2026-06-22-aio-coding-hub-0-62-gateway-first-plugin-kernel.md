# aio-coding-hub 0.62 Gateway-first Plugin Kernel Implementation Plan

> Status: Superseded.
>
> MUST NOT be executed. This historical plan treats declarativeRules as a
> stable public community runtime. Use the Extension Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the 0.62 Gateway-first plugin platform kernel while keeping Plugin API v1 externally compatible and keeping Provider Plugin API private.

**Architecture:** Keep the public plugin surface stable and harden the internal layers that serve it. Gateway hook semantics come from contract/descriptor metadata, runtimes own their own cache and execution concerns, and provider-specific behavior remains behind internal adapter/capability facades.

**Tech Stack:** Rust/Tauri 2, Axum gateway plugin pipeline, serde/serde_json, Node.js contract scripts, pnpm, cargo.

---

## File Structure

- `docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md`
  Source specification for this plan.
- `src-tauri/src/app/plugins/official_privacy_filter_runtime.rs`
  New official native privacy filter runtime. Owns loading, execution, cache, prune, and behavior tests for `official.privacy-filter`.
- `src-tauri/src/app/plugins/rule_runtime.rs`
  Declarative rules runtime only. Must not contain official privacy filter execution or cache ownership.
- `src-tauri/src/app/plugins/runtime_executor.rs`
  Runtime gateway executor. Dispatches to declarative rules, official privacy filter, or stable WASM-not-wired error.
- `src-tauri/src/app/plugins/mod.rs`
  Registers the new official privacy filter runtime module.
- `src-tauri/src/gateway/plugins/contract.rs`
  Rust Plugin API v1 metadata. Add uniqueness tests and fix duplicate context field entries.
- `src-tauri/src/gateway/plugins/registry.rs`
  Hook descriptor facade over contract metadata. Add all-contract descriptor coverage.
- `src-tauri/src/gateway/plugins/pipeline.rs`
  Gateway hook orchestration. Add release-gate tests proving pipeline defaults remain aligned with contract defaults.
- `scripts/check-plugin-api-contract.mjs`
  Node contract drift checker. Add duplicate-array checks for contract hook metadata.
- `scripts/check-plugin-api-contract.selftest.mjs`
  Self-tests for the contract checker. Add failing fixture for duplicate hook metadata arrays.
- `docs/plugins/architecture/audit.md`
  Maintainer architecture note. Clarify that Provider Plugin API remains private in 0.62.
- `docs/plugins/reference/compatibility.md`
  Developer-facing compatibility note. Confirm 0.62 keeps Plugin API v1 compatible and does not add public provider plugin APIs.

## Task 1: Commit Official Privacy Filter Runtime Split

**Files:**
- Create: `src-tauri/src/app/plugins/official_privacy_filter_runtime.rs`
- Modify: `src-tauri/src/app/plugins/mod.rs`
- Modify: `src-tauri/src/app/plugins/rule_runtime.rs`
- Modify: `src-tauri/src/app/plugins/runtime_executor.rs`

- [x] **Step 1: Confirm the regression test exists**

Open `src-tauri/src/app/plugins/runtime_executor.rs` and confirm the test module contains this test:

```rust
#[test]
fn runtime_executor_retain_prunes_official_privacy_filter_runtime_cache() {
    let executor = executor();
    let plugin = official_privacy_filter_plugin_detail(json!({
        "redactBeforeUpstream": true,
        "redactLogs": true
    }));
    let context = hook_context("log.beforePersist", "trace-privacy");

    executor
        .execute_plugin_sync(&plugin, context)
        .expect("official privacy filter runtime executes");
    assert_eq!(executor.privacy_filter_cache_size_for_tests(), 1);

    executor.retain_runtime_caches_for_plugins(&[]);

    assert_eq!(executor.privacy_filter_cache_size_for_tests(), 0);
}
```

- [x] **Step 2: Verify the regression test passes against the current implementation**

Run:

```bash
cd src-tauri && cargo test runtime_executor_retain_prunes_official_privacy_filter_runtime_cache --lib
```

Expected: `1 passed; 0 failed`.

- [x] **Step 3: Confirm runtime ownership boundaries in code**

Confirm `src-tauri/src/app/plugins/rule_runtime.rs` contains no privacy-filter ownership:

```bash
rg "PrivacyFilter|privacy_filter|official_privacy_filter|execute_official_privacy_filter" src-tauri/src/app/plugins/rule_runtime.rs
```

Expected: no matches.

Confirm `src-tauri/src/app/plugins/runtime_executor.rs` dispatches native privacy filter to the new runtime:

```rust
RuntimeDispatch::NativePrivacyFilter => {
    self.privacy_filter_runtime.execute_plugin(plugin, context)
}
```

- [x] **Step 4: Run focused behavior checks**

Run:

```bash
cd src-tauri && cargo test official_privacy_filter --lib
cd src-tauri && cargo test rule_runtime_prunes_cache_entries_not_in_active_plugin_keys --lib
```

Expected:

- `official_privacy_filter --lib`: `32 passed; 0 failed`
- `rule_runtime_prunes_cache_entries_not_in_active_plugin_keys --lib`: `1 passed; 0 failed`

- [x] **Step 5: Run compile and formatting checks**

Run:

```bash
cd src-tauri && cargo fmt -- --check
cd src-tauri && cargo check --locked
cd src-tauri && RUSTFLAGS=-Dwarnings cargo check --locked
```

Expected: all commands exit `0`.

- [x] **Step 6: Commit the runtime split**

Run:

```bash
git add src-tauri/src/app/plugins/mod.rs \
  src-tauri/src/app/plugins/rule_runtime.rs \
  src-tauri/src/app/plugins/runtime_executor.rs \
  src-tauri/src/app/plugins/official_privacy_filter_runtime.rs
git commit -m "refactor(plugins): split official privacy filter runtime"
```

Expected: commit succeeds and contains only the runtime split.

## Task 2: Add Rust Contract Uniqueness Gate

**Files:**
- Modify: `src-tauri/src/gateway/plugins/contract.rs`

- [x] **Step 1: Write the failing duplicate-field test**

Append this helper and test inside the existing `#[cfg(test)] mod tests` in `src-tauri/src/gateway/plugins/contract.rs`:

```rust
fn assert_unique_slice(label: &str, hook: &str, values: &[&str]) {
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        assert!(
            seen.insert(*value),
            "{hook} {label} contains duplicate value {value}"
        );
    }
}

#[test]
fn hook_contract_arrays_do_not_contain_duplicates() {
    for contract in ACTIVE_HOOKS.iter().chain(RESERVED_HOOKS.iter()) {
        assert_unique_slice("read_permissions", contract.id, contract.read_permissions);
        assert_unique_slice("write_permissions", contract.id, contract.write_permissions);
        assert_unique_slice("mutation_fields", contract.id, contract.mutation_fields);
        assert_unique_slice("context_fields", contract.id, contract.context_fields);
    }
}
```

- [x] **Step 2: Run the test and verify it fails for the current duplicate**

Run:

```bash
cd src-tauri && cargo test hook_contract_arrays_do_not_contain_duplicates --lib
```

Expected before the fix: failure containing:

```text
gateway.request.beforeSend context_fields contains duplicate value request.query
```

- [x] **Step 3: Fix the duplicate context field**

In `src-tauri/src/gateway/plugins/contract.rs`, update the `gateway.request.beforeSend` `context_fields` array from:

```rust
context_fields: &[
    "traceId",
    "request.cliKey",
    "request.method",
    "request.path",
    "request.query",
    "request.query",
    "request.headers",
    "request.body",
    "request.requestedModel",
    "request.normalizedMessages",
],
```

to:

```rust
context_fields: &[
    "traceId",
    "request.cliKey",
    "request.method",
    "request.path",
    "request.query",
    "request.headers",
    "request.body",
    "request.requestedModel",
    "request.normalizedMessages",
],
```

- [x] **Step 4: Run the test and verify it passes**

Run:

```bash
cd src-tauri && cargo test hook_contract_arrays_do_not_contain_duplicates --lib
```

Expected: `1 passed; 0 failed`.

- [x] **Step 5: Commit**

Run:

```bash
git add src-tauri/src/gateway/plugins/contract.rs
git commit -m "test(plugins): guard hook contract metadata uniqueness"
```

Expected: commit succeeds.

## Task 3: Add Hook Registry Full Contract Coverage

**Files:**
- Modify: `src-tauri/src/gateway/plugins/registry.rs`

- [x] **Step 1: Add a descriptor mirror test**

Inside `#[cfg(test)] mod tests` in `src-tauri/src/gateway/plugins/registry.rs`, add this test:

```rust
#[test]
fn registry_descriptors_mirror_every_hook_contract() {
    let registry = HookRegistry::new();
    for contract in crate::gateway::plugins::contract::ACTIVE_HOOKS
        .iter()
        .chain(crate::gateway::plugins::contract::RESERVED_HOOKS.iter())
    {
        let hook_name = GatewayPluginHookName::from_str(contract.id)
            .unwrap_or_else(|| panic!("missing GatewayPluginHookName for {}", contract.id));
        let descriptor = registry
            .descriptor(hook_name)
            .unwrap_or_else(|| panic!("missing descriptor for {}", contract.id));

        assert_eq!(descriptor.hook_name, hook_name);
        assert_eq!(descriptor.id, contract.id);
        assert_eq!(descriptor.kind, contract.kind);
        assert_eq!(descriptor.read_permissions, contract.read_permissions);
        assert_eq!(descriptor.write_permissions, contract.write_permissions);
        assert_eq!(descriptor.mutation_fields, contract.mutation_fields);
        assert_eq!(descriptor.timeout_ms, contract.timeout_ms);
        assert_eq!(
            descriptor.default_failure_policy,
            contract.default_failure_policy
        );
    }
}
```

- [x] **Step 2: Run the focused test**

Run:

```bash
cd src-tauri && cargo test registry_descriptors_mirror_every_hook_contract --lib
```

Expected: `1 passed; 0 failed`.

- [x] **Step 3: Run existing registry tests**

Run:

```bash
cd src-tauri && cargo test gateway::plugins::registry --lib
```

Expected: all registry tests pass.

- [x] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/gateway/plugins/registry.rs
git commit -m "test(plugins): cover hook registry contract descriptors"
```

Expected: commit succeeds.

## Task 4: Add Pipeline Contract Default Alignment Test

**Files:**
- Modify: `src-tauri/src/gateway/plugins/pipeline.rs`

- [x] **Step 1: Add the default timeout alignment test**

Inside the existing `#[cfg(test)] mod tests` in `src-tauri/src/gateway/plugins/pipeline.rs`, add:

```rust
#[test]
fn default_pipeline_timeout_matches_plugin_contract() {
    assert_eq!(
        GatewayPluginPipelineConfig::default().hook_timeout,
        std::time::Duration::from_millis(
            crate::gateway::plugins::contract::DEFAULT_HOOK_TIMEOUT_MS
        )
    );
}
```

- [x] **Step 2: Run the focused test**

Run:

```bash
cd src-tauri && cargo test default_pipeline_timeout_matches_plugin_contract --lib
```

Expected: `1 passed; 0 failed`.

- [x] **Step 3: Run pipeline plugin tests**

Run:

```bash
cd src-tauri && cargo test gateway_plugin_pipeline --lib
```

Expected: all matching pipeline tests pass, with existing performance smoke tests still ignored.

- [x] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/gateway/plugins/pipeline.rs
git commit -m "test(plugins): align pipeline defaults with contract"
```

Expected: commit succeeds.

## Task 5: Add Contract Checker Duplicate Metadata Gate

**Files:**
- Modify: `scripts/check-plugin-api-contract.mjs`
- Modify: `scripts/check-plugin-api-contract.selftest.mjs`

- [x] **Step 1: Add the failing self-test fixture**

In `scripts/check-plugin-api-contract.selftest.mjs`, append this fixture near the other hook metadata fixtures:

```js
const duplicateHookMetadataRoot = makeRoot("duplicate-hook-metadata");
writeJson(duplicateHookMetadataRoot, "docs/plugins/plugin-api-v1-contract.json", {
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
      status: "active",
      defaultFailurePolicy: "fail-open",
      timeoutMs: 150,
      reservedHeaderPolicy: "block-gateway-owned",
      readPermissions: ["request.body.read", "request.body.read"],
      writePermissions: [],
      permissionDependencies: {},
      mutationFields: ["requestBody"],
      contextFields: ["traceId"],
    },
  },
  communityRuntimes: ["declarativeRules"],
  policyGatedRuntimes: ["wasm"],
  officialRuntimes: ["native:privacyFilter"],
});
writePassingScaffold(duplicateHookMetadataRoot);

const duplicateHookMetadataResult = runCheck(duplicateHookMetadataRoot);
if (
  duplicateHookMetadataResult.status === 0 ||
  !duplicateHookMetadataResult.stderr.includes(
    "hookMatrix.gateway.request.afterBodyRead.readPermissions contains duplicate request.body.read"
  )
) {
  throw new Error(
    `expected duplicate hookMatrix metadata failure, got status ${duplicateHookMetadataResult.status}\n${duplicateHookMetadataResult.stderr}`
  );
}
```

- [x] **Step 2: Run the self-test and verify it fails**

Run:

```bash
node scripts/check-plugin-api-contract.selftest.mjs
```

Expected before implementation: failure because the checker does not yet report duplicate `readPermissions`.

- [x] **Step 3: Implement duplicate array detection**

In `scripts/check-plugin-api-contract.mjs`, add this helper after `requireArray`:

```js
function requireUniqueArray(path, values) {
  const seen = new Set();
  for (const value of values) {
    if (seen.has(value)) {
      failures.push(`${path} contains duplicate ${value}`);
    }
    seen.add(value);
  }
}
```

Then, in the active hook loop, update the array reads to call the helper:

```js
const readPermissions = requireArray(`hookMatrix.${hook}.readPermissions`, entry.readPermissions);
requireUniqueArray(`hookMatrix.${hook}.readPermissions`, readPermissions);
const writePermissions = requireArray(`hookMatrix.${hook}.writePermissions`, entry.writePermissions);
requireUniqueArray(`hookMatrix.${hook}.writePermissions`, writePermissions);
const mutationFields = requireArray(`hookMatrix.${hook}.mutationFields`, entry.mutationFields);
requireUniqueArray(`hookMatrix.${hook}.mutationFields`, mutationFields);
const contextFields = requireArray(`hookMatrix.${hook}.contextFields`, entry.contextFields);
requireUniqueArray(`hookMatrix.${hook}.contextFields`, contextFields);
```

Keep the existing permission dependency checks using `readPermissions` and `writePermissions`.

- [x] **Step 4: Run contract checks**

Run:

```bash
pnpm check:plugin-api-contract
node scripts/check-plugin-api-contract.selftest.mjs
```

Expected: both commands exit `0`.

- [x] **Step 5: Commit**

Run:

```bash
git add scripts/check-plugin-api-contract.mjs scripts/check-plugin-api-contract.selftest.mjs
git commit -m "test(plugins): reject duplicate hook contract metadata"
```

Expected: commit succeeds.

## Task 5A: Align SDK Permission Dependencies With Hook Contract

**Files:**
- Modify: `packages/plugin-sdk/src/index.ts`
- Modify: `packages/plugin-sdk/src/index.test.ts`
- Modify: `scripts/check-plugin-api-contract.mjs`
- Modify: `scripts/check-plugin-api-contract.selftest.mjs`
- Modify: `docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md`
- Modify: `docs/superpowers/plans/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel.md`

- [x] **Step 1: Write failing SDK tests for host-compatible write-only hooks**

Add these tests to `packages/plugin-sdk/src/index.test.ts`:

```ts
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
```

- [x] **Step 2: Run SDK tests and verify they fail before the fix**

Run:

```bash
pnpm --filter @aio-coding-hub/plugin-sdk test
```

Expected before implementation: tests fail with `PLUGIN_INVALID_PERMISSION_SET` because SDK applies write/read dependencies globally.

- [x] **Step 3: Make SDK permission dependencies hook-aware**

Change `packages/plugin-sdk/src/index.ts` so `validatePermissionSet` receives the full manifest and only applies dependencies for hooks that declare them in Plugin API v1:

```ts
const permissionSetError = validatePermissionSet(manifest);
```

```ts
function validatePermissionSet(manifest: PluginManifest): ValidationResult | null {
  const set = new Set(manifest.permissions);
  const hooks = new Set(manifest.hooks.map((hook) => hook.name));

  if (
    hooks.has("gateway.request.afterBodyRead") &&
    set.has("request.body.write") &&
    !set.has("request.body.read")
  ) {
    return invalid(
      "PLUGIN_INVALID_PERMISSION_SET",
      "request.body.write requires request.body.read"
    );
  }
  if (
    hooks.has("gateway.response.after") &&
    set.has("response.body.write") &&
    !set.has("response.body.read")
  ) {
    return invalid(
      "PLUGIN_INVALID_PERMISSION_SET",
      "response.body.write requires response.body.read"
    );
  }
  if (
    hooks.has("gateway.response.chunk") &&
    set.has("stream.modify") &&
    !set.has("stream.inspect")
  ) {
    return invalid("PLUGIN_INVALID_PERMISSION_SET", "stream.modify requires stream.inspect");
  }
  return null;
}
```

- [x] **Step 4: Add contract checker self-test for global dependency drift**

Append a fixture to `scripts/check-plugin-api-contract.selftest.mjs` that creates a temporary SDK with `validatePermissionSet(permissions)` and a global `request.body.write` -> `request.body.read` rule while the contract says `gateway.request.beforeSend.permissionDependencies` is `{}`.

Run:

```bash
node scripts/check-plugin-api-contract.selftest.mjs
```

Expected before checker implementation: failure because `check-plugin-api-contract.mjs` returns status `0` for the bad SDK fixture.

- [x] **Step 5: Check SDK permission dependencies from contract metadata**

Update `scripts/check-plugin-api-contract.mjs` so it derives dependency entries from `hookMatrix.*.permissionDependencies` and checks that SDK validation:

- calls `validatePermissionSet(manifest)`;
- defines `validatePermissionSet(manifest: PluginManifest)`;
- reads both `manifest.permissions` and `manifest.hooks`;
- guards every dependency behind `hooks.has("<hook>")`.

- [x] **Step 6: Run verification**

Run:

```bash
pnpm exec prettier --check packages/plugin-sdk/src/index.ts packages/plugin-sdk/src/index.test.ts scripts/check-plugin-api-contract.mjs scripts/check-plugin-api-contract.selftest.mjs
node scripts/check-plugin-api-contract.selftest.mjs
pnpm check:plugin-api-contract
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
pnpm --filter create-aio-plugin test
pnpm typecheck
git diff --check
```

Expected: every command exits `0`; SDK accepts host-compatible write-only hooks and rejects only hook-scoped permission dependency violations.

- [x] **Step 7: Commit**

Run:

```bash
git add packages/plugin-sdk/src/index.ts \
  packages/plugin-sdk/src/index.test.ts \
  scripts/check-plugin-api-contract.mjs \
  scripts/check-plugin-api-contract.selftest.mjs \
  docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md \
  docs/superpowers/plans/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel.md
git commit -m "fix(plugin-sdk): align permission dependencies with hook contract"
```

Expected: commit succeeds and contains only the SDK permission dependency alignment plus its contract drift guard.

## Task 6: Strengthen Provider API Non-Exposure Docs Gate

**Files:**
- Modify: `docs/plugins/architecture/audit.md`
- Modify: `docs/plugins/reference/compatibility.md`
- Modify: `scripts/check-plugin-system-docs.mjs`

- [x] **Step 1: Add required phrases to docs check**

In `scripts/check-plugin-system-docs.mjs`, extend the `docs/plugins/architecture/audit.md` entry so its `phrases` array includes:

```js
"0.62 does not add public provider plugin APIs",
"provider adapter facades remain internal",
```

The entry should still include the existing phrases for `official.privacy-filter`, `declarativeRules`, `WASM`, `native`, `信任边界`, and `性能与稳定性建议`.

- [x] **Step 2: Run docs check and verify it fails**

Run:

```bash
pnpm check:plugin-system-docs
```

Expected before doc updates: failure pointing at the missing provider API phrases in `docs/plugins/architecture/audit.md`.

- [x] **Step 3: Update architecture audit**

In `docs/plugins/architecture/audit.md`, extend the `0.62 Platform Kernel Decision` section with this paragraph:

```markdown
0.62 does not add public provider plugin APIs. Provider adapter facades remain internal so gateway selection, failover, circuit breaking, limits, OAuth handling, and session binding stay owned by the Rust gateway core.
```

- [x] **Step 4: Confirm compatibility doc still has the public API boundary**

Ensure `docs/plugins/reference/compatibility.md` contains:

```markdown
Plugin API v1 remains externally compatible in 0.62
0.62 does not add public provider plugin APIs
0.62 keeps third-party JavaScript and WebView plugin execution unsupported
```

If any line is missing, add it to the 0.62 compatibility section.

- [x] **Step 5: Run docs check**

Run:

```bash
pnpm check:plugin-system-docs
```

Expected: command exits `0`.

- [x] **Step 6: Commit**

Run:

```bash
git add docs/plugins/architecture/audit.md docs/plugins/reference/compatibility.md scripts/check-plugin-system-docs.mjs
git commit -m "docs(plugins): guard 0.62 provider api boundary"
```

Expected: commit succeeds.

## Task 7: Run 0.62 Gateway-first Release Verification

**Files:**
- Read: `docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md`
- Read: `docs/superpowers/plans/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel.md`

- [x] **Step 1: Verify no public plugin API files changed unexpectedly**

Run:

```bash
git diff --name-only b53075ba..HEAD
```

Inspect the output. Expected for this plan:

- No changes under `packages/plugin-sdk/` unless caused by a deliberate contract check fix.
- No changes under `packages/create-aio-plugin/` unless caused by a deliberate contract check fix.
- No `plugin.json` schema shape changes.
- No public provider plugin API docs.

- [x] **Step 2: Run Rust formatting and compile gates**

Run:

```bash
cd src-tauri && cargo fmt -- --check
cd src-tauri && cargo check --locked
cd src-tauri && RUSTFLAGS=-Dwarnings cargo check --locked
```

Expected: all commands exit `0`.

- [x] **Step 3: Run plugin-focused Rust tests**

Run:

```bash
cd src-tauri && cargo test plugin --lib
```

Expected: all plugin tests pass, with existing performance smoke tests still ignored.

- [x] **Step 4: Run provider-focused Rust tests**

Run:

```bash
cd src-tauri && cargo test provider --lib
```

Expected: all provider tests pass.

- [x] **Step 5: Run targeted gateway hook tests**

Run:

```bash
cd src-tauri && cargo test gateway_plugin_pipeline --lib
cd src-tauri && cargo test gateway_plugin_request --lib
cd src-tauri && cargo test gateway_plugin_response --lib
cd src-tauri && cargo test plugin_log_redaction --lib
```

Expected: all matching tests pass.

- [x] **Step 6: Run contract and docs gates**

Run:

```bash
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
node scripts/check-plugin-api-contract.selftest.mjs
```

Expected: all commands exit `0`.

- [x] **Step 7: Commit final verification notes if docs changed**

If verification requires a small documentation note, commit it:

```bash
git add docs/plugins
git add -f docs/superpowers/specs docs/superpowers/plans
git commit -m "docs(plugins): record 0.62 gateway-first verification"
```

If no files changed, do not create an empty commit.

- [x] **Step 8: Report final status**

Report:

- final commit list,
- verification commands and pass/fail status,
- any ignored performance smoke tests,
- confirmation that Plugin API v1 remains externally compatible,
- confirmation that Provider Plugin API was not opened.

### Verification Run: 2026-06-22

Step 1 review of `git diff --name-only b53075ba..HEAD` showed this branch contains both 0.62 plugin-kernel work and the merged provider route-order work from `origin/main`. Public plugin API changes are limited to the deliberate SDK contract-alignment fix in `packages/plugin-sdk`; no `packages/create-aio-plugin` changes, no `plugin.json` schema shape changes, and no public provider plugin API docs were introduced.

Commands run:

```bash
cd src-tauri && cargo fmt -- --check && cargo check --locked && RUSTFLAGS=-Dwarnings cargo check --locked
cd src-tauri && cargo test plugin --lib
cd src-tauri && cargo test provider --lib
cd src-tauri && cargo test gateway_plugin_pipeline --lib
cd src-tauri && cargo test gateway_plugin_request --lib
cd src-tauri && cargo test gateway_plugin_response --lib
cd src-tauri && cargo test plugin_log_redaction --lib
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
node scripts/check-plugin-api-contract.selftest.mjs
pnpm plugin:perf-smoke
```

Observed results:

- Rust formatting and compile gates exited `0`.
- `cargo test plugin --lib`: `180 passed; 0 failed; 2 ignored; 1296 filtered out`. The ignored tests are the existing performance smoke tests `perf_empty_pipeline_request_hook_budget` and `perf_one_noop_plugin_request_hook_budget`.
- `cargo test provider --lib`: `219 passed; 0 failed; 0 ignored; 1259 filtered out`.
- `cargo test gateway_plugin_pipeline --lib`: `9 passed; 0 failed`.
- `cargo test gateway_plugin_request --lib`: `4 passed; 0 failed`.
- `cargo test gateway_plugin_response --lib`: `5 passed; 0 failed`.
- `cargo test plugin_log_redaction --lib`: `1 passed; 0 failed`.
- Contract and docs gates exited `0`.
- `pnpm plugin:perf-smoke`: `2 passed; 0 failed`; observed `perf_empty_pipeline_request_hook_budget` average `961ns` against the `25us` budget, and `perf_one_noop_plugin_request_hook_budget` average `4505ns` against the `250us` budget.

Compatibility conclusions:

- Plugin API v1 remains externally compatible for this plan scope.
- Provider Plugin API remains closed; provider adapter facades are still internal.
- Performance smoke did not show gateway hot path regression for the measured plugin pipeline budgets.

### Completion Audit: 2026-06-22

Requirement-by-requirement evidence:

- **Document 0.62 goals and architecture:** covered by `docs/superpowers/specs/2026-06-21-aio-coding-hub-0-62-plugin-platform-kernel-design.md` and `docs/superpowers/specs/2026-06-22-aio-coding-hub-0-62-gateway-first-plugin-kernel-design.md`.
- **Preserve Plugin API v1 external compatibility:** verified by the Task 7 diff review, `pnpm check:plugin-api-contract`, SDK tests, and the SDK hook-scoped permission dependency fix in `5a105c0a`.
- **Contract layer drift detection:** covered by `0ef09198`, `5a105c0a`, `pnpm check:plugin-api-contract`, and `node scripts/check-plugin-api-contract.selftest.mjs`.
- **Hook registry and descriptor alignment:** covered by `53068274`, `214d6163`, `03791477`, `84b5a354`, `cargo test plugin --lib`, and targeted gateway hook tests.
- **Runtime ownership clarity:** covered by `3b8ed702`, runtime-focused plugin tests, and `cargo test plugin --lib`.
- **Provider adapter remains internal:** covered by `773992b2`, `d7d54ed1`, `0431be89`, `cargo test provider --lib`, and `pnpm check:plugin-system-docs`.
- **Frontend/backend boundary remains host-owned for plugin execution:** covered by the spec non-goals and the absence of public provider plugin API or WebView/JS runtime changes in the Task 7 diff review.
- **Release verification gates:** covered by the 2026-06-22 verification run, `pnpm plugin:perf-smoke`, and the pushed branch pre-push checks.

Final status for this plan: the 0.62 Gateway-first plugin kernel plan is implemented and verified on branch `codex/plugin-platform-kernel-0-62`. Future work should be tracked as new 0.62 follow-up specs or post-0.62 provider/plugin API RFCs rather than extending this completed plan.
