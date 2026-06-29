# aio-coding-hub Plugin Observability, Replay, and Publishing Implementation Plan

> Status: Superseded.
>
> MUST NOT be executed. This historical plan contains obsolete
> declarativeRules and local `replay --explain` success-path assumptions. Use
> the Extension Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the plugin observability, trace replay, developer tooling parity, example plugin, and market/publishing productization work without changing Plugin API v1.

**Architecture:** Keep Plugin API v1 and the existing gateway hook contract stable. Add host-owned runtime diagnosis, replay fixture export, and a lightweight market/publishing UI around the current plugin pipeline and `create-aio-plugin` tooling. Rust stays the source of truth for runtime facts; TypeScript tooling mirrors the contract and replay fixtures; the GUI only renders structured host results.

**Tech Stack:** Rust, Tauri 2 commands, SQLite via rusqlite/r2d2, Specta generated bindings, React 19, TanStack Query, Vitest, Cargo tests, Node.js, Markdown docs.

---

## Scope Boundaries

- Do not change the public Plugin API v1 manifest shape.
- Do not add Plugin API v2.
- Do not expose Provider Plugin API.
- Do not open `plugin.storage`, `network.fetch`, `file.read`, `file.write`, or `secret.read`.
- Do not open JS/TS/WebView/browser plugin runtimes.
- Do not enable third-party native runtime.
- Do not default-enable marketplace WASM execution.
- Do not add ratings, reviews, recommendation, payment, or operator backend features.
- Keep replay fixtures and diagnostics host-owned; plugins must not call them directly.
- Preserve current external behavior for existing local install, remote install, rollback, quarantine, and official plugin flows.

## File Structure

- Modify: `src-tauri/src/domain/plugins.rs`
  - Add plugin runtime diagnosis DTOs and replay fixture DTOs that Specta can export.
- Modify: `src-tauri/src/infra/plugins/repository.rs`
  - Add repository helpers for hook execution reports and market source access.
- Create: `src-tauri/src/infra/plugins/runtime_reports.rs`
  - Own the hook execution report repository and query helpers.
- Create: `src-tauri/src/infra/plugins/replay_export.rs`
  - Build trace-to-fixture export models from request logs, attempt logs, and plugin reports.
- Modify: `src-tauri/src/commands/plugins.rs`
  - Add plugin diagnosis, replay export, and market source command entry points.
- Modify: `src-tauri/src/commands/request_logs.rs`
  - Reuse existing trace and attempt log access for replay export inputs.
- Modify: `src-tauri/src/commands/registry.rs`
  - Register the new plugin commands for runtime and Specta export.
- Modify: `src-tauri/src/gateway/plugins/pipeline.rs`
  - Record structured execution reports from hook execution outcomes.
- Modify: `src-tauri/src/gateway/plugins/context.rs`
  - Keep bounded visible context and surface truncation data needed by diagnosis and replay export.
- Modify: `src-tauri/src/gateway/plugins/mutation.rs`
  - Reuse output mutation budgets in execution reporting.
- Modify: `src-tauri/src/app/plugin_service.rs`
  - Expose detail/query helpers for reports and replay export support.
- Modify: `src-tauri/src/app/plugins/mod.rs`
  - Export any new runtime report helpers used by the gateway or services.
- Modify: `src-tauri/src/infra/db/migrations/ensure.rs`
  - Add the new plugin execution report table and any supporting indexes.
- Modify: `src-tauri/src/infra/db/migrations/v33_to_v34.rs`
  - Seed the new plugin report table on fresh installs and upgrade existing databases.
- Modify: `src/services/plugins.ts`
  - Add IPC wrappers and TypeScript types for diagnosis, replay export, and market source handling.
- Modify: `src/query/keys.ts`
  - Add query keys for plugin runtime reports and market source data.
- Modify: `src/query/plugins.ts`
  - Add React Query hooks and mutations for the new plugin operations.
- Modify: `src/pages/PluginsPage.tsx`
  - Integrate the new observability panel, replay export action, and market source surfaces.
- Create: `src/pages/plugins/PluginRuntimeReportsPanel.tsx`
  - Render structured hook execution reports and replay export actions.
- Create: `src/pages/plugins/PluginMarketPanel.tsx`
  - Render market source loading, listing state, trust, compatibility, and install/update actions.
- Modify: `src/pages/plugins/PluginLifecyclePanel.tsx`
  - Keep lifecycle state visible alongside the new observability data.
- Modify: `src/pages/plugins/PluginInstallPreviewDialog.tsx`
  - Keep install preview behavior aligned with market and replay flows.
- Modify: `src/pages/plugins/PluginUpdatePreviewDialog.tsx`
  - Keep update diff behavior aligned with market and replay flows.
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`
  - Cover the new observability and market surfaces.
- Modify: `src/pages/plugins/__tests__/PluginConfigSchemaForm.test.tsx`
  - Keep existing plugin details rendering stable after new sections land.
- Modify: `packages/create-aio-plugin/src/devtools.ts`
  - Add fixture parsing, replay explain parity helpers, and publish-check output.
- Modify: `packages/create-aio-plugin/src/scaffold.ts`
  - Add or refine official example scaffolds as needed.
- Modify: `packages/create-aio-plugin/src/scaffold.test.ts`
  - Add CLI and replay parity tests for the new devtools behavior.
- Modify: `packages/create-aio-plugin/src/cli.ts`
  - Register the new CLI command if command routing changes.
- Create: `packages/create-aio-plugin/src/fixtures.ts`
  - Hold reusable replay fixture and publish-check test helpers if the devtools file becomes too large.
- Modify: `docs/plugins/developer-guide.md`
  - Document the trace export -> replay -> fix -> pack workflow.
- Modify: `docs/plugins/examples/README.md`
  - Document the new official example plugins.
- Modify: `docs/plugins/examples/privacy-filter.md`
  - Keep the official example docs aligned with the host/runtime boundary.
- Modify: `docs/plugins/reference/hooks.md`
  - Document replay support and diagnosis visibility per hook.
- Modify: `docs/plugins/reference/publishing.md`
  - Document replay fixtures, publish-check, market sources, and trust flow.
- Modify: `docs/plugins/reference/README.md`
  - Point readers to the new observability and publishing docs.
- Modify: `docs/plugins/runtime/README.md`
  - Keep runtime lifecycle wording aligned with the host-owned lifecycle boundary.

## Task 1: Add Structured Plugin Runtime Reports

**Files:**
- Modify: `src-tauri/src/domain/plugins.rs`
- Create: `src-tauri/src/infra/plugins/runtime_reports.rs`
- Modify: `src-tauri/src/infra/plugins/repository.rs`
- Modify: `src-tauri/src/infra/db/migrations/ensure.rs`
- Create: `src-tauri/src/infra/db/migrations/v33_to_v34.rs`
- Modify: `src-tauri/src/gateway/plugins/pipeline.rs`
- Modify: `src-tauri/src/app/plugin_service.rs`
- Modify: `src-tauri/src/commands/plugins.rs`
- Modify: `src-tauri/src/commands/registry.rs`

- [ ] **Step 1: Write the failing Rust repository and command tests**

Add these tests to `src-tauri/src/infra/plugins/repository.rs` near the existing plugin repository tests:

```rust
#[test]
fn repository_records_and_lists_plugin_hook_execution_reports() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

    let report = crate::infra::plugins::runtime_reports::record_hook_execution_report(
        &db,
        crate::infra::plugins::runtime_reports::RecordPluginHookExecutionReportInput {
            plugin_id: "community.prompt-helper".to_string(),
            trace_id: Some("trace-report-1".to_string()),
            hook_name: "gateway.request.afterBodyRead".to_string(),
            runtime_kind: "declarativeRules".to_string(),
            status: "completed".to_string(),
            started_at_ms: 1_000,
            duration_ms: 17,
            failure_kind: None,
            error_code: None,
            failure_policy: Some("fail-open".to_string()),
            circuit_state: Some("closed".to_string()),
            context_budget_json: serde_json::json!({"bodyBytes": 4096}),
            output_budget_json: serde_json::json!({"bodyBytes": 2048}),
            mutation_summary_json: serde_json::json!({"changed": true, "field": "requestBody"}),
            replayable: true,
            replay_export_reason: None,
        },
    )
    .unwrap();

    let list = crate::infra::plugins::runtime_reports::list_hook_execution_reports(
        &db,
        Some("community.prompt-helper"),
        Some("gateway.request.afterBodyRead"),
        Some("trace-report-1"),
        50,
    )
    .unwrap();

    assert_eq!(report.plugin_id, "community.prompt-helper");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].status, "completed");
    assert_eq!(list[0].mutation_summary["field"], "requestBody");
}
```

Add this test to `src-tauri/src/gateway/plugins/pipeline.rs` near the existing timeout/budget tests:

```rust
#[tokio::test]
async fn gateway_plugin_pipeline_records_runtime_report_for_fail_closed_timeout() {
    let executor = InMemoryGatewayPluginExecutor::new().with_request_async_handler(
        "plugin.slow",
        |_ctx| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            GatewayHookResult::continue_unchanged()
        },
    );
    let mut plugin = plugin("plugin.slow", 10, vec!["request.body.read"]);
    plugin.manifest.hooks[0].failure_policy = Some("fail-closed".to_string());
    let pipeline = GatewayPluginPipeline::for_tests(
        vec![plugin],
        Arc::new(executor),
        GatewayPluginPipelineConfig {
            hook_timeout: Duration::from_millis(1),
            circuit_failure_threshold: 1,
            circuit_cooldown: Duration::from_secs(60),
            ..GatewayPluginPipelineConfig::default()
        },
    );

    let err = pipeline
        .run_request_hook(request_input())
        .await
        .expect_err("fail-closed timeout should fail the request");

    assert_eq!(err.code(), "PLUGIN_HOOK_TIMEOUT");
    assert!(err.audit_events().iter().any(|event| {
        event.event_type == "plugin.hook.failed"
            && event.details.get("failureKind") == Some(&serde_json::json!("timeout"))
    }));
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cd src-tauri && cargo test repository_records_and_lists_plugin_hook_execution_reports --lib && cargo test gateway_plugin_pipeline_records_runtime_report_for_fail_closed_timeout --lib
```

Expected: fail because the new runtime report repository and command plumbing do not exist yet.

- [ ] **Step 3: Implement the new runtime report table, DTOs, and repository**

In `src-tauri/src/domain/plugins.rs`, add a data-only DTO for execution reports and any matching summary/detail types that Specta can export. Keep the fields host-owned and append-only:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginHookExecutionReport {
    pub id: i64,
    pub plugin_id: String,
    pub trace_id: Option<String>,
    pub hook_name: String,
    pub runtime_kind: String,
    pub status: String,
    pub started_at_ms: i64,
    pub duration_ms: i64,
    pub failure_kind: Option<String>,
    pub error_code: Option<String>,
    pub failure_policy: Option<String>,
    pub circuit_state: Option<String>,
    pub context_budget: serde_json::Value,
    pub output_budget: serde_json::Value,
    pub mutation_summary: serde_json::Value,
    pub replayable: bool,
    pub replay_export_reason: Option<String>,
    pub created_at: i64,
}
```

Create `src-tauri/src/infra/plugins/runtime_reports.rs` with repository functions that mirror the existing plugin repository style: insert, list, and list-by-trace/hook. Use the same `db_err!` and `now_unix_seconds()` helpers already used in `repository.rs`.

Add the backing table in `src-tauri/src/infra/db/migrations/ensure.rs` with a dedicated `plugin_hook_execution_reports` table and indexes on `(plugin_id, created_at)` and `trace_id`.

Add an incremental migration file `src-tauri/src/infra/db/migrations/v33_to_v34.rs` that creates the table for upgraded databases and records `schema_migrations`.

- [ ] **Step 4: Route pipeline outcomes into the report repository**

In `src-tauri/src/gateway/plugins/pipeline.rs`, add a small helper that converts each hook outcome into one report row. Record at least:

```rust
status: "completed" | "failedOpen" | "failedClosed" | "skipped" | "blocked" | "budgetRejected" | "policyRejected" | "circuitOpen"
```

Use the existing audit events and failure policy logic as the source of truth. Do not invent new behavior in the report layer. The report should summarize what already happened in the pipeline, including timeout, budget rejection, and truncated-context rejection paths.

- [ ] **Step 5: Expose runtime report commands**

In `src-tauri/src/commands/plugins.rs`, add commands such as:

```rust
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginListRuntimeReportsInput {
    pub plugin_id: Option<String>,
    pub hook_name: Option<String>,
    pub trace_id: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn plugin_list_runtime_reports(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: PluginListRuntimeReportsInput,
) -> Result<Vec<crate::infra::plugins::runtime_reports::PluginHookExecutionReport>, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("plugin_list_runtime_reports", move || {
        crate::infra::plugins::runtime_reports::list_hook_execution_reports(
            &db,
            input.plugin_id.as_deref(),
            input.hook_name.as_deref(),
            input.trace_id.as_deref(),
            input.limit.unwrap_or(50),
        )
    })
    .await
    .map_err(Into::into)
}
```

Register the new command in `src-tauri/src/commands/registry.rs` so `src/generated/bindings.ts` can be regenerated from the same source of truth.

- [ ] **Step 6: Run the repository and command tests again**

Run:

```bash
cd src-tauri && cargo test repository_records_and_lists_plugin_hook_execution_reports --lib && cargo test gateway_plugin_pipeline_records_runtime_report_for_fail_closed_timeout --lib
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/domain/plugins.rs src-tauri/src/infra/plugins/runtime_reports.rs src-tauri/src/infra/plugins/repository.rs src-tauri/src/infra/db/migrations/ensure.rs src-tauri/src/infra/db/migrations/v33_to_v34.rs src-tauri/src/gateway/plugins/pipeline.rs src-tauri/src/commands/plugins.rs src-tauri/src/commands/registry.rs
git commit -m "feat(plugins): add structured runtime reports"
```

## Task 2: Add Trace Replay Fixture Export

**Files:**
- Create: `src-tauri/src/infra/plugins/replay_export.rs`
- Modify: `src-tauri/src/commands/request_logs.rs`
- Modify: `src-tauri/src/commands/plugins.rs`
- Modify: `src-tauri/src/commands/registry.rs`
- Modify: `src-tauri/src/domain/plugins.rs`
- Modify: `src/services/gateway/requestLogs.ts`
- Modify: `src/services/plugins.ts`
- Modify: `src/query/keys.ts`
- Modify: `src/query/plugins.ts`
- Create: `src/pages/plugins/PluginRuntimeReportsPanel.tsx`

- [ ] **Step 1: Write a failing Rust exporter test**

Add this test to `src-tauri/src/infra/plugins/replay_export.rs`:

```rust
#[test]
fn export_replay_fixture_uses_trace_and_attempt_logs() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

    // Seed request_logs, request_attempt_logs, and plugin reports for one trace.
    // The fixture should include the trace id, hook name, normalized messages, and attempts.

    let fixture = export_plugin_replay_fixture(
        &db,
        ExportPluginReplayFixtureInput {
            trace_id: "trace-replay-1".to_string(),
            hook_name: "gateway.request.afterBodyRead".to_string(),
            plugin_id: Some("community.prompt-helper".to_string()),
        },
    )
    .unwrap();

    assert_eq!(fixture.trace_id, "trace-replay-1");
    assert_eq!(fixture.hook_name, "gateway.request.afterBodyRead");
    assert_eq!(fixture.source.trace_id, "trace-replay-1");
    assert!(!fixture.attempts.is_empty());
    assert!(fixture.request.body.is_some());
}
```

- [ ] **Step 2: Run the exporter test and verify it fails**

Run:

```bash
cd src-tauri && cargo test export_replay_fixture_uses_trace_and_attempt_logs --lib
```

Expected: FAIL because the exporter does not exist yet.

- [ ] **Step 3: Implement the replay fixture exporter**

Create `src-tauri/src/infra/plugins/replay_export.rs` and keep it narrow:

```rust
pub struct ExportPluginReplayFixtureInput {
    pub trace_id: String,
    pub hook_name: String,
    pub plugin_id: Option<String>,
}

pub struct PluginReplayFixture {
    pub schema_version: u32,
    pub source: PluginReplayFixtureSource,
    pub hook_name: String,
    pub request: PluginReplayFixtureRequest,
    pub response: PluginReplayFixtureResponse,
    pub log: PluginReplayFixtureLog,
    pub attempts: Vec<crate::infra::request_attempt_logs::RequestAttemptLog>,
    pub notes: Vec<String>,
}
```

Build the fixture from `request_log_get_by_trace_id` and `request_attempt_logs_by_trace_id`. Use the new runtime report table to include plugin-specific execution metadata when available.

The exporter should return stable `PLUGIN_REPLAY_UNAVAILABLE` style errors when the trace is missing, the hook is unsupported, or the payload exceeds the host export limit. Do not silently drop required fields.

- [ ] **Step 4: Expose the replay exporter through Tauri commands**

In `src-tauri/src/commands/plugins.rs`, add:

```rust
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginExportReplayFixtureInput {
    pub trace_id: String,
    pub hook_name: String,
    pub plugin_id: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn plugin_export_replay_fixture(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: PluginExportReplayFixtureInput,
) -> Result<crate::infra::plugins::replay_export::PluginReplayFixture, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("plugin_export_replay_fixture", move || {
        crate::infra::plugins::replay_export::export_plugin_replay_fixture(
            &db,
            crate::infra::plugins::replay_export::ExportPluginReplayFixtureInput {
                trace_id: input.trace_id,
                hook_name: input.hook_name,
                plugin_id: input.plugin_id,
            },
        )
    })
    .await
    .map_err(Into::into)
}
```

Thread the command through `src-tauri/src/commands/registry.rs` so the generated bindings can pick it up.

- [ ] **Step 5: Add frontend service/query support**

In `src/services/gateway/requestLogs.ts`, add a replay-export helper that accepts a trace id and hook name and returns a fixture JSON shape from the new command.

In `src/services/plugins.ts` and `src/query/plugins.ts`, add wrappers for listing runtime reports and exporting replay fixtures.

In `src/pages/plugins/PluginRuntimeReportsPanel.tsx`, render the runtime report rows with:

```tsx
// status, hook, trace id, duration, failure kind, and a replay export button
```

- [ ] **Step 6: Write React and service tests**

Add tests to:

```text
src/services/__tests__/plugins.test.ts
src/query/__tests__/plugins.test.tsx
src/pages/__tests__/PluginsPage.test.tsx
```

The tests should verify:

- the replay export helper normalizes trace ids and hook names;
- the query layer invalidates and caches runtime reports;
- the page renders hook status rows and a replay export action;
- empty-state behavior stays readable when no reports exist.

- [ ] **Step 7: Run the Rust, service, query, and page tests**

Run:

```bash
cd src-tauri && cargo test export_replay_fixture_uses_trace_and_attempt_logs --lib
pnpm test:unit src/services/__tests__/plugins.test.ts src/query/__tests__/plugins.test.tsx src/pages/__tests__/PluginsPage.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/infra/plugins/replay_export.rs src-tauri/src/commands/request_logs.rs src-tauri/src/commands/plugins.rs src-tauri/src/commands/registry.rs src-tauri/src/domain/plugins.rs src/services/gateway/requestLogs.ts src/services/plugins.ts src/query/keys.ts src/query/plugins.ts src/pages/plugins/PluginRuntimeReportsPanel.tsx src/pages/__tests__/PluginsPage.test.tsx src/services/__tests__/plugins.test.ts src/query/__tests__/plugins.test.tsx
git commit -m "feat(plugins): export trace replay fixtures"
```

## Task 3: Keep `create-aio-plugin` In Parity With The Host

**Files:**
- Modify: `packages/create-aio-plugin/src/devtools.ts`
- Modify: `packages/create-aio-plugin/src/scaffold.ts`
- Modify: `packages/create-aio-plugin/src/scaffold.test.ts`
- Modify: `packages/create-aio-plugin/src/cli.ts`
- Create: `packages/create-aio-plugin/src/fixtures.ts`

- [ ] **Step 1: Write failing tests for replay fixture parity and publish-check**

Add tests to `packages/create-aio-plugin/src/scaffold.test.ts`:

```ts
it("replay explain accepts exported host fixtures without changing the host contract shape", () => {
  const fixture = {
    schemaVersion: 1,
    source: {
      appVersion: "0.62.3",
      traceId: "trace-replay-1",
      exportedAtMs: 1_000,
    },
    hookName: "gateway.request.afterBodyRead",
    request: {
      body: { messages: [{ role: "user", content: "SECRET_TOKEN" }] },
      normalizedMessages: [{ role: "user", content: "SECRET_TOKEN" }],
    },
    response: null,
    log: null,
    attempts: [],
    notes: [],
  };

  const result = replayHookExplain(files, "gateway.request.afterBodyRead", fixture);

  expect(result.pluginId).toBe("community.redactor");
  expect(result.outputKind).toMatch(/pass|replace|block|warn/);
});

it("publish-check emits package metadata needed by the market flow", () => {
  const signed = signPackage(packed.bytes, keyPair.privateKey);
  const result = publishCheckPluginBytes(packed.bytes, {
    checksum: signed.checksum,
    signature: signed.signature,
    publicKey: signed.publicKey,
    manifest: files["plugin.json"],
  });

  expect(result).toMatchObject({
    ok: true,
    checksum: signed.checksum,
    signatureVerified: true,
  });
});
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
```

Expected: FAIL because the new host fixture shape and `publish-check` helper are not implemented yet.

- [ ] **Step 3: Add fixture-aware replay and `publish-check` helpers**

In `packages/create-aio-plugin/src/devtools.ts`, add a small parser that accepts the exported host fixture schema and maps it to the existing declarative-rules replay engine without mutating the host contract shape.

Also add:

```ts
export function publishCheckPluginBytes(
  bytes: Uint8Array,
  input: {
    checksum: string;
    signature?: string | null;
    publicKey?: string | null;
    manifest: string;
  }
): {
  ok: boolean;
  checksum: string;
  signatureVerified: boolean;
  manifestId: string;
  version: string;
  runtime: PluginManifest["runtime"]["kind"];
} {
  // Parse manifest, verify package checksum/signature, and summarize publish metadata.
}
```

Keep `pack`, `sign`, and `verify` as separate responsibilities. `publish-check` should summarize release metadata, not repack the package.

- [ ] **Step 4: Add official example scaffolds if needed**

Update `packages/create-aio-plugin/src/scaffold.ts` only if the current rule and wasm templates need a small official-example adjustment to cover prompt helper, redactor, or response guard fixtures. Keep the existing scaffold style; do not introduce a brand-new scaffold system.

- [ ] **Step 5: Run the create-aio-plugin tests**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
pnpm --filter create-aio-plugin typecheck
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add packages/create-aio-plugin/src/devtools.ts packages/create-aio-plugin/src/scaffold.ts packages/create-aio-plugin/src/scaffold.test.ts packages/create-aio-plugin/src/cli.ts packages/create-aio-plugin/src/fixtures.ts
git commit -m "feat(create-aio-plugin): add replay and publish parity"
```

## Task 4: Productize Example Plugins And The Plugins GUI

**Files:**
- Create or modify example plugin files under `docs/plugins/examples/`
- Modify: `docs/plugins/examples/README.md`
- Modify: `docs/plugins/examples/privacy-filter.md`
- Modify: `src/pages/PluginsPage.tsx`
- Create: `src/pages/plugins/PluginMarketPanel.tsx`
- Modify: `src/pages/plugins/PluginLifecyclePanel.tsx`
- Modify: `src/pages/plugins/PluginInstallPreviewDialog.tsx`
- Modify: `src/pages/plugins/PluginUpdatePreviewDialog.tsx`
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`
- Modify: `src/pages/plugins/__tests__/pluginProductCopy.test.ts`

- [ ] **Step 1: Write failing UI tests for runtime reports, market, and example coverage**

Add or extend tests in `src/pages/__tests__/PluginsPage.test.tsx` to assert:

```tsx
it("renders runtime reports and replay export actions", async () => {
  // Render a plugin with structured runtime reports and expect status, duration,
  // trace id, and export actions in the observability section.
});

it("renders market state and disables revoked or incompatible installs", async () => {
  // Render market listings with revoked and incompatible states and assert
  // the install button is disabled and the reason is visible.
});

it("keeps privacy filter, prompt helper, and redactor example guidance visible", async () => {
  // Ensure the plugin page or example docs references the official examples.
});
```

- [ ] **Step 2: Run the UI tests and verify they fail**

Run:

```bash
pnpm test:unit src/pages/__tests__/PluginsPage.test.tsx src/pages/plugins/__tests__/pluginProductCopy.test.ts
```

Expected: FAIL because runtime reports, market panel, and example guidance are not yet rendered.

- [ ] **Step 3: Add the runtime report and market panels**

In `src/pages/plugins/PluginRuntimeReportsPanel.tsx`, render the new execution report rows and add a replay export button per row.

In `src/pages/plugins/PluginMarketPanel.tsx`, render:

```tsx
// market source url, listing status, trust summary, install/update buttons,
// revoked/incompatible blocks, and trusted public key info
```

Wire both panels into `src/pages/PluginsPage.tsx` alongside the existing lifecycle panel.

- [ ] **Step 4: Keep lifecycle, preview, and update dialogs consistent**

Update `PluginLifecyclePanel.tsx`, `PluginInstallPreviewDialog.tsx`, and `PluginUpdatePreviewDialog.tsx` so trust, compatibility, and rollback phrasing stays consistent with the market and replay flows.

Avoid adding new copy that implies the GUI can bypass host checks.

- [ ] **Step 5: Update the example docs**

Expand `docs/plugins/examples/README.md` so it lists:

- `official.privacy-filter`
- `examples/prompt-helper`
- `examples/redactor`
- `examples/response-guard`

Make each example explain which hooks, permissions, and fixtures it covers.

- [ ] **Step 6: Run the UI tests and doc checks**

Run:

```bash
pnpm test:unit src/pages/__tests__/PluginsPage.test.tsx src/pages/plugins/__tests__/pluginProductCopy.test.ts
pnpm check:spec-links
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/pages/PluginsPage.tsx src/pages/plugins/PluginRuntimeReportsPanel.tsx src/pages/plugins/PluginMarketPanel.tsx src/pages/plugins/PluginLifecyclePanel.tsx src/pages/plugins/PluginInstallPreviewDialog.tsx src/pages/plugins/PluginUpdatePreviewDialog.tsx src/pages/__tests__/PluginsPage.test.tsx src/pages/plugins/__tests__/pluginProductCopy.test.ts docs/plugins/examples/README.md docs/plugins/examples/privacy-filter.md
git commit -m "feat(plugins): productize observability and market UI"
```

## Task 5: Sync Publishing And Runtime Docs

**Files:**
- Modify: `docs/plugins/developer-guide.md`
- Modify: `docs/plugins/reference/hooks.md`
- Modify: `docs/plugins/reference/publishing.md`
- Modify: `docs/plugins/reference/README.md`
- Modify: `docs/plugins/runtime/README.md`

- [ ] **Step 1: Write failing docs checks**

Extend the existing docs checks or add a focused test script entry so the new replay export and publish-check documentation must exist before release. The docs should mention:

```text
trace export -> replay -> fix -> pack
plugin_hook_execution_reports
plugin_export_replay_fixture
publish-check
market index URL
trusted public key
revoked / incompatible install blocks
```

- [ ] **Step 2: Run the docs checks and verify they fail**

Run:

```bash
pnpm check:plugin-system-docs
pnpm check:spec-links
```

Expected: fail until the docs are updated.

- [ ] **Step 3: Update developer and publishing docs**

Add the new workflow to `docs/plugins/developer-guide.md`:

```text
doctor -> validate --strict -> replay --explain -> export replay fixture -> fix -> pack -> publish-check -> install/update
```

Update `docs/plugins/reference/publishing.md` to explain:

- publish-check output
- market source URL and trusted public key
- signature and checksum handling
- replay fixtures as a developer workflow artifact
- revoked/incompatible state at install time

Update `docs/plugins/reference/hooks.md` to describe hook-level observability and replay support per hook.

Update `docs/plugins/runtime/README.md` to keep the runtime lifecycle boundary explicit and host-owned.

Update `docs/plugins/reference/README.md` so the new docs are easy to find.

- [ ] **Step 4: Run the docs checks again**

Run:

```bash
pnpm check:plugin-system-docs
pnpm check:spec-links
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add docs/plugins/developer-guide.md docs/plugins/reference/hooks.md docs/plugins/reference/publishing.md docs/plugins/reference/README.md docs/plugins/runtime/README.md
git commit -m "docs(plugins): sync observability and publishing workflow"
```

## Verification Strategy

- Run targeted Rust tests for the runtime report repository and replay exporter before expanding to the full `cargo test --lib`.
- Run `pnpm --filter create-aio-plugin test -- src/scaffold.test.ts` and `pnpm --filter create-aio-plugin typecheck` before touching the GUI.
- Run focused UI tests for `PluginsPage` and plugin product copy before broader `pnpm test:unit`.
- Run `pnpm check:plugin-system-docs`, `pnpm check:spec-links`, and `pnpm check:generated-bindings` before claiming the docs and IPC contracts are stable.
- Use small commits after each task so review can isolate backend, tooling, UI, and docs concerns.

## Acceptance Criteria

- The plugin detail surface shows structured runtime evidence instead of only raw audit text.
- A trace can be exported into a replay fixture and replayed by `create-aio-plugin`.
- The host and `create-aio-plugin` remain aligned on core declarative-rule replay behavior.
- Official example plugins cover privacy filter, prompt helper, redactor, and response guard workflows.
- The market UI can load listings, explain revoked/incompatible states, and surface trust information.
- `publish-check` produces release metadata without changing Plugin API v1.
- The docs explain the full developer loop and publishing flow without implying any new high-risk plugin API.
