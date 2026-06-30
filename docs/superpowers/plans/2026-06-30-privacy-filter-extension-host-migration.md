# Privacy Filter Extension Host Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `official.privacy-filter` to the normal Extension Host runtime and remove every `native:privacyFilter` runtime exception.

**Architecture:** Keep the existing Rust privacy detector as a host service, but expose it through a standard Extension Host capability named `privacy.redact`. `official.privacy-filter` becomes an ordinary Extension Host plugin with `dist/extension.js`, `contributes.gatewayHooks`, and no top-level `hooks` or `permissions`. Runtime lifecycle, timeout, diagnostics, dispose, and cache cleanup all flow through the existing Extension Host Registry.

**Tech Stack:** Rust/Tauri2 backend, rquickjs Extension Host worker, JSON-RPC host API, TypeScript plugin SDK, Vitest, Cargo tests, docs contract scripts.

---

## File Structure

- Modify `src-tauri/resources/plugins/official/privacy-filter/plugin.json`: convert official manifest to Extension Host.
- Create `src-tauri/resources/plugins/official/privacy-filter/dist/extension.js`: official plugin hook implementation.
- Move `src-tauri/src/app/plugins/official_privacy_filter_runtime.rs` to `src-tauri/src/app/plugins/privacy_redaction_service.rs`: keep redaction logic, remove runtime executor behavior.
- Modify `src-tauri/src/app/plugins/mod.rs`: export the new service module and remove old runtime module.
- Modify `src-tauri/src/app/plugins/extension_host.rs`: add `privacy.redact` host API and inject `api.privacy`.
- Modify `src-tauri/src/app/plugins/extension_host_worker.rs`: inject `api.privacy` only when the manifest declares `privacy.redact`.
- Modify `src-tauri/src/app/plugins/runtime_manager.rs`: remove `RuntimeDispatch::NativePrivacyFilter`.
- Modify `src-tauri/src/app/plugins/runtime_executor.rs`: remove dedicated Privacy Filter runtime field/cache and sync native execution path.
- Modify `src-tauri/src/domain/plugin_contributions.rs`: add `privacy.redact` to known capabilities.
- Modify `src-tauri/src/domain/plugins.rs`: remove official native validation branch and update tests.
- Modify `src-tauri/src/app/plugins/official.rs`: treat official Privacy Filter like a normal Extension Host plugin.
- Modify `src-tauri/src/app/plugin_service.rs`: remove native Privacy Filter install/update assumptions and keep official default config.
- Modify `src-tauri/src/infra/plugins/package.rs` and `src-tauri/src/infra/plugins/repository.rs`: remove official native runtime allowances.
- Modify `packages/plugin-sdk/src/index.ts`, `index.test.ts`, and `index.typecheck.ts`: add `privacy.redact` capability and `PrivacyApi` types.
- Modify `docs/plugin-manifest-v1.md`, `docs/plugins/plugin-api-v1-contract.json`, docs examples, and contract scripts: remove official native runtime concept.

## Task 1: Add Red Tests For The Desired Runtime Boundary

**Files:**
- Modify: `src-tauri/src/domain/plugins.rs`
- Modify: `src-tauri/src/app/plugins/runtime_manager.rs`
- Modify: `src-tauri/src/app/plugins/runtime_executor.rs`
- Test: domain and runtime manager tests in the same files.

- [ ] **Step 1: Add a failing domain test that official Privacy Filter validates as Extension Host**

Add this test near existing manifest validation tests in `src-tauri/src/domain/plugins.rs`:

```rust
#[test]
fn official_privacy_filter_extension_host_manifest_uses_normal_validation() {
    let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "official.privacy-filter",
        "name": "Privacy Filter",
        "version": "1.0.0",
        "apiVersion": "1.0.0",
        "runtime": { "kind": "extensionHost", "language": "typescript" },
        "main": "dist/extension.js",
        "capabilities": ["gateway.hooks", "privacy.redact"],
        "contributes": {
            "gatewayHooks": [
                { "name": "gateway.request.afterBodyRead", "priority": 5, "failurePolicy": "fail-closed" },
                { "name": "gateway.request.beforeSend", "priority": 5, "failurePolicy": "fail-closed" },
                { "name": "log.beforePersist", "priority": 1, "failurePolicy": "fail-closed" }
            ]
        },
        "hostCompatibility": { "app": ">=0.60.0 <1.0.0", "pluginApi": "^1.0.0" }
    }))
    .expect("manifest json");

    validate_manifest(&manifest, "0.62.0").expect("official privacy filter should be a normal extension host manifest");
    validate_manifest_for_official_plugin(&manifest, "0.62.0")
        .expect("official validation should also accept normal extension host manifest");
}
```

- [ ] **Step 2: Add a failing domain test that official native Privacy Filter is rejected**

Add this test beside the previous test:

```rust
#[test]
fn official_privacy_filter_native_runtime_is_rejected() {
    let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "official.privacy-filter",
        "name": "Privacy Filter",
        "version": "1.0.0",
        "apiVersion": "1.0.0",
        "runtime": { "kind": "native", "engine": "privacyFilter" },
        "hostCompatibility": { "app": ">=0.60.0 <1.0.0", "pluginApi": "^1.0.0" }
    }))
    .expect("manifest json");

    let err = validate_manifest_for_official_plugin(&manifest, "0.62.0")
        .expect_err("official native privacy filter runtime must not be allowed");

    assert_eq!(err.code, "PLUGIN_UNSUPPORTED_RUNTIME");
}
```

- [ ] **Step 3: Add a failing runtime-manager test that no native runtime is dispatched**

Replace old native Privacy Filter manager tests in `src-tauri/src/app/plugins/runtime_manager.rs` with:

```rust
#[test]
fn runtime_manager_rejects_all_native_runtimes_without_official_exceptions() {
    let manager = PluginRuntimeManager::for_tests();
    let runtime = PluginRuntime::Native {
        engine: "privacyFilter".to_string(),
    };

    let err = manager
        .runtime_dispatch("official.privacy-filter", &runtime)
        .expect_err("official native privacyFilter should be rejected");

    assert_eq!(err.code(), "PLUGIN_UNSUPPORTED_RUNTIME");
}
```

- [ ] **Step 4: Run red tests**

Run:

```bash
cargo test domain::plugins::tests::official_privacy_filter_extension_host_manifest_uses_normal_validation --lib
cargo test domain::plugins::tests::official_privacy_filter_native_runtime_is_rejected --lib
cargo test app::plugins::runtime_manager::tests::runtime_manager_rejects_all_native_runtimes_without_official_exceptions --lib
```

Expected: at least the first test fails with `PLUGIN_UNKNOWN_CAPABILITY` for `privacy.redact`, and native rejection fails until the official exception is removed.

## Task 2: Convert Official Privacy Filter Package To Extension Host

**Files:**
- Modify: `src-tauri/resources/plugins/official/privacy-filter/plugin.json`
- Create: `src-tauri/resources/plugins/official/privacy-filter/dist/extension.js`
- Modify: `src-tauri/tests/fixtures/plugins/official/privacy-filter/plugin.json`
- Create: `src-tauri/tests/fixtures/plugins/official/privacy-filter/dist/extension.js`
- Test: `src-tauri/src/app/plugins/official.rs`

- [ ] **Step 1: Replace official manifest runtime and contributions**

Use this manifest shape in both resource and test fixture `plugin.json`, preserving the existing config schema:

```json
{
  "id": "official.privacy-filter",
  "name": "Privacy Filter",
  "version": "1.0.0",
  "apiVersion": "1.0.0",
  "configVersion": 3,
  "category": "privacy",
  "description": "Official Extension Host privacy filter aligned with packyme/privacy-filter for pre-upstream prompt and log redaction.",
  "homepage": "https://github.com/packyme/privacy-filter",
  "repository": {
    "type": "git",
    "url": "https://github.com/packyme/privacy-filter.git"
  },
  "license": "MIT",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  },
  "main": "dist/extension.js",
  "activationEvents": [
    "onGatewayHook:gateway.request.afterBodyRead",
    "onGatewayHook:gateway.request.beforeSend",
    "onGatewayHook:log.beforePersist"
  ],
  "capabilities": ["gateway.hooks", "privacy.redact"],
  "contributes": {
    "gatewayHooks": [
      {
        "name": "gateway.request.afterBodyRead",
        "priority": 5,
        "failurePolicy": "fail-closed"
      },
      {
        "name": "gateway.request.beforeSend",
        "priority": 5,
        "failurePolicy": "fail-closed"
      },
      {
        "name": "log.beforePersist",
        "priority": 1,
        "failurePolicy": "fail-closed"
      }
    ]
  },
  "hostCompatibility": {
    "app": ">=0.60.0 <1.0.0",
    "pluginApi": "^1.0.0",
    "platforms": ["macos", "windows", "linux"]
  }
}
```

- [ ] **Step 2: Add official Extension Host entry**

Create `dist/extension.js` under resource and test fixture roots:

```javascript
function arrayOption(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === "string") : undefined;
}

function privacyOptions(config) {
  return {
    sensitiveTypes: arrayOption(config && config.sensitiveTypes),
    redactionScopes: arrayOption(config && config.redactionScopes),
  };
}

function handleRequestHook(api, payload) {
  const config = payload && payload.config ? payload.config : {};
  if (config.redactBeforeUpstream !== true) {
    return { action: "pass" };
  }
  const body = payload && payload.context && payload.context.request
    ? payload.context.request.body
    : undefined;
  if (typeof body !== "string" || body.length === 0) {
    return { action: "pass" };
  }
  const result = api.privacy.redactRequestBody(body, privacyOptions(config));
  return result && result.hit
    ? { action: "replace", requestBody: result.redacted }
    : { action: "pass" };
}

function handleLogHook(api, payload) {
  const config = payload && payload.config ? payload.config : {};
  if (config.redactLogs !== true) {
    return { action: "pass" };
  }
  const message = payload && payload.context && payload.context.log
    ? payload.context.log.message
    : undefined;
  if (typeof message !== "string" || message.length === 0) {
    return { action: "pass" };
  }
  const result = api.privacy.redactText(message, privacyOptions(config));
  return result && result.hit
    ? { action: "replace", logMessage: result.redacted }
    : { action: "pass" };
}

module.exports.activate = function activate(api) {
  api.gateway.registerHook("gateway.request.afterBodyRead", function onAfterBodyRead(payload) {
    return handleRequestHook(api, payload);
  });
  api.gateway.registerHook("gateway.request.beforeSend", function onBeforeSend(payload) {
    return handleRequestHook(api, payload);
  });
  api.gateway.registerHook("log.beforePersist", function onBeforePersist(payload) {
    return handleLogHook(api, payload);
  });
};
```

- [ ] **Step 3: Run official fixture tests**

Run:

```bash
cargo test app::plugins::official::tests --lib
```

Expected: fails until `privacy.redact` is added to known capabilities and official helpers stop expecting top-level permissions.

## Task 3: Extract Privacy Redaction Service And Host API

**Files:**
- Move/modify: `src-tauri/src/app/plugins/official_privacy_filter_runtime.rs` -> `src-tauri/src/app/plugins/privacy_redaction_service.rs`
- Modify: `src-tauri/src/app/plugins/mod.rs`
- Modify: `src-tauri/src/app/plugins/extension_host.rs`
- Modify: `src-tauri/src/app/plugins/extension_host_worker.rs`
- Modify: `src-tauri/src/domain/plugin_contributions.rs`
- Test: `src-tauri/src/app/plugins/privacy_redaction_service.rs`
- Test: `src-tauri/src/app/plugins/extension_host.rs`

- [ ] **Step 1: Move runtime code to service**

Rename the file and replace `OfficialPrivacyFilterRuntime` with:

```rust
#[derive(Default)]
pub(crate) struct PrivacyRedactionService {
    cache: Mutex<HashMap<String, Arc<PrivacyFilter>>>,
}
```

Expose these methods:

```rust
pub(crate) fn redact_text(
    &self,
    plugin: &PluginDetail,
    text: &str,
    options: &Value,
) -> Result<PrivacyRedactionOutput, PrivacyFilterError>;

pub(crate) fn redact_request_body(
    &self,
    plugin: &PluginDetail,
    body: &str,
    options: &Value,
) -> Result<PrivacyRedactionOutput, PrivacyFilterError>;
```

Use this output:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrivacyRedactionOutput {
    pub(crate) hit: bool,
    pub(crate) count: usize,
    pub(crate) redacted: String,
}
```

- [ ] **Step 2: Preserve request body allowlist functions**

Keep existing `redact_request_body_strings`, `PrivacyFilterRedactionScopes`, `privacy_filter_options_from_config`, and allowlist helpers. Change them to return `PrivacyRedactionOutput` through `redact_request_body`.

- [ ] **Step 3: Add service tests**

Rename existing tests from `official_privacy_filter_*` to `privacy_redaction_service_*`, and add:

```rust
#[test]
fn privacy_redaction_service_returns_no_hit_for_clean_request_body() {
    let service = PrivacyRedactionService::default();
    let plugin = privacy_filter_plugin_detail(json!({}));

    let result = service
        .redact_request_body(
            &plugin,
            r#"{"messages":[{"role":"user","content":"hello"}]}"#,
            &json!({})
        )
        .expect("redaction");

    assert!(!result.hit);
    assert_eq!(result.count, 0);
    assert_eq!(result.redacted, r#"{"messages":[{"role":"user","content":"hello"}]}"#);
}
```

- [ ] **Step 4: Add `privacy.redact` as known capability**

In `src-tauri/src/domain/plugin_contributions.rs`, add `"privacy.redact"` to the known capability set.

- [ ] **Step 5: Add parent-side host API methods**

In `ExtensionHostApiHandler`, add fields:

```rust
plugin_root: PathBuf,
privacy_redaction: Arc<PrivacyRedactionService>,
```

Add methods:

```rust
"privacy.redactText" => self.privacy_redact_text(params),
"privacy.redactRequestBody" => self.privacy_redact_request_body(params),
```

Both methods must call `self.require_capability("privacy.redact")?`, validate the calling `pluginId`, load the current plugin detail from the database, force `installed_dir` to the handler plugin root if missing, and call the service.

- [ ] **Step 6: Inject `api.privacy` in worker**

In `extension_host_worker.rs`, when `capabilities.contains("privacy.redact")`, inject:

```javascript
({
  redactText(text, options) {
    return globalThis.__aioHostApi(
      "privacy.redactText",
      { pluginId: PLUGIN_ID, text, options: options || {} }
    );
  },
  redactRequestBody(body, options) {
    return globalThis.__aioHostApi(
      "privacy.redactRequestBody",
      { pluginId: PLUGIN_ID, body, options: options || {} }
    );
  }
})
```

- [ ] **Step 7: Add Extension Host API tests**

Add tests to `src-tauri/src/app/plugins/extension_host.rs`:

```rust
#[tokio::test]
async fn extension_host_privacy_api_redacts_text_with_capability() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("rules")).expect("rules dir");
    std::fs::write(temp.path().join("rules/gitleaks.toml"), "").expect("rules file");
    write_extension_plugin_with_capabilities(
        temp.path(),
        r#"
        module.exports.activate = function(api) {
          api.commands.registerCommand("acme.echo", function() {
            return api.privacy.redactText("email a@example.com", { sensitiveTypes: ["email"] });
          });
        };
        "#,
        &["commands.execute", "privacy.redact"],
    );
    let db = init_test_db(temp.path());
    let manifest = install_extension_plugin(&db, temp.path());

    let mut host = super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
        .await
        .expect("start extension host");

    let result = host.execute_command("acme.echo", json!({})).await.expect("execute command");

    assert_eq!(result.get("hit").and_then(serde_json::Value::as_bool), Some(true));
    assert_eq!(result.get("redacted").and_then(serde_json::Value::as_str), Some("email [邮箱]"));
    host.dispose().await;
}
```

Add a matching missing capability test using `globalThis.__aioHostApi("privacy.redactText", ...)` and expecting `PLUGIN_EXTENSION_HOST_FORBIDDEN`.

- [ ] **Step 8: Run service and host tests**

Run:

```bash
cargo test app::plugins::privacy_redaction_service::tests --lib
cargo test app::plugins::extension_host::tests::extension_host_privacy_api_redacts_text_with_capability --lib
cargo test app::plugins::extension_host::tests::extension_host_privacy_api_rejects_missing_capability --lib
```

Expected: PASS after implementation.

## Task 4: Remove Native Privacy Runtime Dispatch And Executor Cache

**Files:**
- Modify: `src-tauri/src/app/plugins/runtime_manager.rs`
- Modify: `src-tauri/src/app/plugins/runtime_executor.rs`
- Modify: `src-tauri/src/app/plugins/runtime_lifecycle.rs` only if imports become unused.
- Delete: old runtime symbols in `src-tauri/src/app/plugins/official_privacy_filter_runtime.rs` after the service move.

- [ ] **Step 1: Simplify runtime dispatch**

Change `RuntimeDispatch` to:

```rust
pub(crate) enum RuntimeDispatch {
    ExtensionHost,
}
```

Change `runtime_dispatch` so every `PluginRuntime::Native` returns `PLUGIN_UNSUPPORTED_RUNTIME`.

- [ ] **Step 2: Remove dedicated runtime field**

In `RuntimeGatewayPluginExecutor`, remove:

```rust
privacy_filter_runtime: Arc<OfficialPrivacyFilterRuntime>,
```

Do not register a Privacy Filter cache in `RuntimeLifecycleRegistry`.

- [ ] **Step 3: Remove sync native execution**

Change `execute_plugin_sync` test helper so Extension Host returns `PLUGIN_EXTENSION_HOST_GATEWAY_ASYNC_REQUIRED` and native runtime returns unsupported through `runtime_dispatch`. Remove all tests that assert a privacy runtime cache exists.

- [ ] **Step 4: Run runtime tests**

Run:

```bash
cargo test app::plugins::runtime_manager::tests --lib
cargo test app::plugins::runtime_executor::tests --lib
```

Expected: PASS.

## Task 5: Wire Official Privacy Filter Through Extension Host End To End

**Files:**
- Modify: `src-tauri/src/app/plugins/official.rs`
- Modify: `src-tauri/src/app/plugin_service.rs`
- Modify: `src-tauri/src/gateway/routes.rs`
- Modify tests in those files.

- [ ] **Step 1: Update official helper tests**

Official helper assertions should expect:

```rust
assert!(matches!(
    fixture.manifest.runtime,
    PluginRuntime::ExtensionHost { ref language } if language == "typescript"
));
assert_eq!(fixture.manifest.main.as_deref(), Some("dist/extension.js"));
assert!(fixture.manifest.capabilities.iter().any(|item| item == "privacy.redact"));
```

- [ ] **Step 2: Update plugin service native checks**

Remove code paths that treat `native:privacyFilter` as installable for official plugins. Official install should materialize the Extension Host package and store summary runtime as `extensionHost`.

- [ ] **Step 3: Update gateway route tests**

Keep the existing privacy redaction route tests, but update setup so installed official plugin has:

```rust
runtime: "extensionHost"
manifest.runtime = PluginRuntime::ExtensionHost { language: "typescript".to_string() }
manifest.main = Some("dist/extension.js".to_string())
manifest.capabilities = vec!["gateway.hooks".to_string(), "privacy.redact".to_string()]
```

Do not use `execute_plugin_sync` for Privacy Filter behavior. Use the async gateway pipeline/router path.

- [ ] **Step 4: Run official/gateway tests**

Run:

```bash
cargo test app::plugins::official::tests --lib
cargo test app::plugin_service::tests --lib
cargo test gateway::routes::tests::official_privacy_filter_redacts_gzipped_codex_responses_before_upstream --lib
cargo test gateway::routes::tests::official_privacy_filter_redacts_full_codex_responses_payload_before_upstream_and_logs --lib
cargo test gateway::routes::tests::official_privacy_filter_before_send_redacts_final_upstream_body --lib
```

Expected: PASS.

## Task 6: Update SDK, Contract, Docs, And Residual Searches

**Files:**
- Modify: `packages/plugin-sdk/src/index.ts`
- Modify: `packages/plugin-sdk/src/index.test.ts`
- Modify: `packages/plugin-sdk/src/index.typecheck.ts`
- Modify: `docs/plugin-manifest-v1.md`
- Modify: `docs/plugins/plugin-api-v1-contract.json`
- Modify: `scripts/check-plugin-api-contract.selftest.mjs`
- Modify: `scripts/check-plugin-system-docs.mjs`
- Modify docs under `docs/plugins/examples/` and `docs/plugins/architecture/`.

- [ ] **Step 1: Update SDK types**

Add `privacy.redact` to `PluginCapability`, `KNOWN_CAPABILITIES`, and typecheck examples. Add:

```ts
export type PrivacyRedactionOptions = {
  sensitiveTypes?: string[];
  redactionScopes?: string[];
};

export type PrivacyRedactionResult = {
  hit: boolean;
  count: number;
  redacted: string;
};

export type PrivacyApi = {
  redactText(text: string, options?: PrivacyRedactionOptions): PrivacyRedactionResult;
  redactRequestBody(body: string, options?: PrivacyRedactionOptions): PrivacyRedactionResult;
};
```

- [ ] **Step 2: Update SDK tests**

Add a test that validates a manifest with `capabilities: ["gateway.hooks", "privacy.redact"]` and gateway hook contributions.

- [ ] **Step 3: Remove official native runtime from contract**

In `docs/plugins/plugin-api-v1-contract.json`, remove:

```json
"native:privacyFilter"
```

and set official runtime references to Extension Host only.

- [ ] **Step 4: Update docs wording**

Docs must say:

```text
Privacy Filter is an official bundled Extension Host plugin. Its official status affects distribution and default configuration only; it does not use a private runtime.
```

Docs must not say it is `native:privacyFilter`, `host-owned native`, or a runtime exception.

- [ ] **Step 5: Run JS/docs checks**

Run:

```bash
node scripts/check-plugin-api-contract.selftest.mjs
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
```

Expected: PASS.

## Task 7: Final Verification And Commit

**Files:**
- All changed files.

- [ ] **Step 1: Residual tracked search**

Run:

```bash
git grep -n -i "native:privacyFilter\\|engine.*privacyFilter\\|NativePrivacyFilter\\|official native runtime\\|host-owned native" -- .
```

Expected: no matches in tracked files, except historical docs already marked as stale only if they are intentionally retained. For this migration, prefer no matches.

- [ ] **Step 2: Run focused full plugin verification**

Run:

```bash
cargo test app::plugins::privacy_redaction_service::tests --lib
cargo test app::plugins::extension_host::tests --lib
cargo test app::plugins::runtime_manager::tests --lib
cargo test app::plugins::runtime_executor::tests --lib
cargo test app::plugins::official::tests --lib
cargo test app::plugin_service::tests --lib
cargo test domain::plugins::tests --lib
cargo test gateway::routes::tests --lib
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
```

Expected: all commands exit 0.

- [ ] **Step 3: Run formatting and staged diff checks**

Run:

```bash
cargo fmt -- --check
git diff --check
```

Expected: both exit 0.

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri packages docs scripts
git commit -m "refactor(plugins): migrate privacy filter to extension host"
```

Expected: local commit created; do not push.
