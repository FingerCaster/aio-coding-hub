# Local Reasoning-Guard Removal Inventory

This inventory is the deletion boundary for the local in-process Codex
reasoning guard. Paths are repository-relative. "Remove owned code" means
remove only the local guard branch from a shared file; it does not authorize
deleting adjacent retry, routing, logging, or Codex configuration behavior.

## 1. Dedicated backend modules: delete

These modules exist only for the local guard and continuation implementation:

- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/codex_reasoning_guard_concurrent.rs`
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_guard.rs`
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_features.rs`
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_continuation.rs`

Delete their module declarations, imports, public re-exports, fixtures, and
callers. Do not leave disabled stubs or compatibility implementations.

## 2. Shared failover/runtime files: remove owned code only

The following files mix guard behavior with unrelated gateway behavior:

- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs`
  - Remove guard concurrent-attempt dispatch and guard-specific outcomes.
  - Preserve ordinary provider attempt execution and retry accounting.
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs`
  - Remove guard retry reasons/state transitions.
  - Preserve OAuth, transient upstream, and provider-scoped recovery reasons.
- `src-tauri/src/gateway/proxy/handler/failover_loop/context.rs`
  - Remove request-scoped guard/continuation buffers, budgets, and state.
- `src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs`
  - Remove dedicated module wiring and guard initialization.
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs`
  - Remove local-guard retry reservations from effective attempt math.
  - Preserve the configured baseline, OAuth, `previous_response_id`, transient
    retry, strict discovery, failover, and total-attempt-cap contracts.
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/finalize.rs`
  - Remove guard-specific finalization and diagnostic projection.
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs`
  - Remove strict buffering/classification/interception/continuation branches.
  - Preserve ordinary streaming passthrough, usage, cancellation, and request
    finalization behavior.
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs`
  - Remove non-stream guard inspection/interception/retry branches.
  - Preserve ordinary response mapping and usage behavior.
- `src-tauri/src/gateway/proxy/handler/failover_loop/tests.rs`
  - Remove guard-only route/attempt/continuation tests; retain and strengthen
    regressions for adjacent retry and failover behavior.
- `src-tauri/src/gateway/proxy/handler/middleware/mod.rs`
- `src-tauri/src/gateway/proxy/handler/runtime_settings.rs`
- `src-tauri/src/gateway/proxy/request_context.rs`
- `src-tauri/src/gateway/proxy/request_end.rs`
- `src-tauri/src/gateway/routes.rs`
  - Remove guard settings snapshots, special markers, result kinds, and
    guard-only route fixtures while preserving generic request lifecycle code.

## 3. Settings contract: remove and migrate

- `src-tauri/src/infra/settings/defaults.rs`
- `src-tauri/src/infra/settings/mod.rs`
- `src-tauri/src/infra/settings/types.rs`
- `src-tauri/src/infra/settings/migration.rs`
- `src-tauri/src/infra/settings/persistence.rs`
- `src-tauri/src/app/settings_service.rs`
- `src/services/settings/settings.ts`
- `src/services/settings/settingsValidation.ts`
- `src/services/settings/__tests__/settingsValidation.test.ts`
- `src/__tests__/msw-default-settings.test.ts`
- `src/test/fixtures/settings.ts`
- `src/test/msw/state.ts`

Remove all `codex_reasoning_guard_*` defaults, enums, templates, settings,
validation, repair/migration code, settings command fields, frontend mappings,
and fixtures. Old JSON keys are ignored on read and disappear on the next
canonical settings write. Do not translate them into external gateway config.

Add only the new external-gateway desired state, selected commit, preferred
port, and optional Node override fields required by the reviewed PRD.

## 4. Request-log statistics and presentation: remove dedicated behavior

Backend:

- `src-tauri/src/commands/request_logs.rs`
- `src-tauri/src/commands/registry.rs`
- `src-tauri/src/infra/request_logs.rs`
- `src-tauri/src/infra/request_logs/queries.rs`
- `src-tauri/src/infra/request_logs/types.rs`

Frontend:

- `src/services/gateway/requestLogs.ts`
- `src/services/gateway/requestLogSpecialSettings.ts`
- `src/query/requestLogs.ts`
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`
- `src/pages/HomePage.tsx`
- `src/pages/LogsPage.tsx`
- `src/components/home/HomeRequestLogsPanel.tsx`
- `src/components/home/RealtimeTraceCards.tsx`
- `src/components/home/RequestLogDetailSummaryTab.tsx`
- `src/components/home/requestLogPresentation.ts`

Tests/fixtures:

- `src/services/gateway/__tests__/requestLogs.test.ts`
- `src/services/gateway/__tests__/requestLogSpecialSettings.test.ts`
- `src/services/gateway/__tests__/requestActivityProjection.test.ts`
- `src/services/gateway/__tests__/traceStore.test.ts`
- `src/query/__tests__/requestLogs.test.tsx`
- `src/components/home/__tests__/HomeRequestLogsPanel.test.tsx`
- `src/components/home/__tests__/RealtimeTraceCards.test.tsx`
- `src/components/home/__tests__/RequestLogDetailDialog.test.tsx`
- `src/components/home/__tests__/requestLogPresentation.test.ts`

Delete the `request_logs_codex_reasoning_guard_stats` command, stats types and
queries, guard badges/cards/labels, continuation summaries, and specialized
JSON-marker parsers. Do not rewrite or delete existing `request_logs` rows;
`special_settings_json` remains a generic column and old markers become inert.

## 5. Codex settings UI: replace, do not layer

- `src/components/cli-manager/tabs/CodexTab.tsx`
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx`
- `src/pages/__tests__/CliManagerPage.test.tsx`
- `src/query/cliManager.ts`
- `src/query/__tests__/cliManager.test.tsx`

Delete the local rule/template/retry/continuation configuration and statistics
surface. Replace it with the compact external gateway entry, desired-state
switch, truthful runtime status, details navigation, WSL coverage warning, and
install/update confirmation flows. Do not keep both old and new controls.

## 6. Generated bindings: regenerate

- `src/generated/bindings.ts`

Remove generated local guard settings, enums, stats, and command wrappers by
changing Rust sources and running `pnpm tauri:gen-types`. Add generated external
gateway command/status types. Never hand-edit generated declarations as the
source of truth.

## 7. Active docs, scripts, and repository rules

Remove or replace active local-guard operational material:

- `scripts/probe-codex-continuation.mjs`
- `scripts/validate-codex-experimental-continuation.ps1`
- `README.md` local guard setup/configuration sections
- `omx_wiki/codex-reasoning-continuation-response-contract.md`
- `omx_wiki/codex-reasoning-guard-retry-count-confusion.md`
- `omx_wiki/index.md` and `omx_wiki/log.md` active links/claims
- `AGENTS.md` release rule that requires local continuation-repair verification
- `.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md`
  references to continuation/guard attempt reservations

Preserve historical release notes in `CHANGELOG.md`; past versions did contain
the feature. New release notes describe removal and external integration.

## 8. Explicitly retained adjacent behavior

Implementation and review must prove that these remain:

- generic transient upstream retry and its provider override;
- OAuth reactive refresh;
- provider-scoped Codex `previous_response_id` recovery;
- configured per-provider attempt baseline and global attempt cap;
- provider sorting, failover, circuit accounting, and health-neutral model
  discovery;
- model-route diagnostics and ordinary request/attempt logs;
- normal streaming/non-streaming passthrough, usage, cost, and cancellation;
- Claude/Gemini CLI proxy synchronization;
- native AIO gateway lifecycle and WSL direct-AIO configuration;
- Codex approval, model, reasoning-effort, service-tier, and unrelated config
  fields.

## 9. Required negative searches after removal

After generated output is refreshed, source searches must find no active local
implementation references for:

```text
codex_reasoning_guard
CodexReasoningGuard
codex_reasoning_features
codex_reasoning_continuation
continuation_repair
request_logs_codex_reasoning_guard_stats
```

Allowed exceptions are historical changelog entries and archived task/research
documents. Build artifact directories are not source evidence and must not be
edited or committed.
