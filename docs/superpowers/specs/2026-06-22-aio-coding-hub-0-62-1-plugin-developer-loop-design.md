# aio-coding-hub 0.62.1 Plugin Developer Loop and Observability Design

## Summary

0.62.1 is the plugin developer-loop and observability release. 0.62.0 stabilized the gateway-first plugin kernel while keeping Plugin API v1 externally compatible and Provider Plugin API private. 0.62.1 keeps those boundaries and makes the existing plugin system easier to develop, validate, replay, diagnose, and maintain.

The main goal is not to add more public plugin power. The main goal is to make the current Plugin API v1 trustworthy in day-to-day use: a plugin author can scaffold, diagnose, strictly validate, replay with explanations, pack, install, enable, and then inspect runtime behavior from the desktop GUI.

## Current State

The current project already has the right foundations:

- `packages/create-aio-plugin/src/devtools.ts` provides `validate`, `replay`, `pack`, `sign`, and `verify`.
- `packages/create-aio-plugin/src/scaffold.ts` scaffolds declarative rules and experimental WASM plugin templates.
- `packages/plugin-sdk/src/index.ts` validates Plugin API v1 manifests and hook-scoped permission dependencies.
- `src-tauri/src/domain/plugins.rs` validates manifests against the Rust contract metadata and exposes `PluginDetail.audit_logs` plus `PluginDetail.runtime_failures`.
- `src-tauri/src/gateway/plugins/audit.rs` persists gateway plugin audit events and runtime failures.
- `src-tauri/src/commands/plugins.rs` exposes plugin management commands and `plugin_list_audit_logs`.
- `src/pages/PluginsPage.tsx` already shows plugin details, permissions, config, hooks, and a small audit preview.
- `src-tauri/src/gateway/proxy/protocol_bridge/*` and provider-related gateway modules already provide internal provider adapter and bridge foundations.

The main gaps are operational:

- `create-aio-plugin validate` checks the manifest but does not deeply diagnose plugin package health.
- `create-aio-plugin replay` is a lightweight TypeScript declarative-rules simulator and does not explain enough for users to debug rules.
- The GUI has audit data, but it does not yet present runtime failures and hook-level behavior as a clear troubleshooting surface.
- Rust contract metadata, SDK metadata, docs, and scaffolds are still partly hand-maintained.
- Provider-specific behavior has internal adapter foundations, but needs more acceptance tests before any future provider plugin RFC is considered.

## Goals

0.62.1 must deliver:

1. A stronger local plugin development loop.
2. Better plugin runtime observability in the Tauri desktop GUI.
3. Better declarative-rules explainability without changing Plugin API v1.
4. A first stage of contract-driven metadata generation or drift prevention across Rust, SDK, docs, and scaffold.
5. Internal provider adapter acceptance coverage without opening Provider Plugin API.

## Non-Goals

0.62.1 does not:

- change the public Plugin API v1 manifest shape;
- introduce Plugin API v2;
- expose Provider Plugin API;
- expose JS or WebView plugin runtime;
- enable arbitrary marketplace WASM execution by default;
- let plugins directly control provider selection, failover, OAuth, token counting, or session binding;
- build a browser-like plugin container inside aio-coding-hub;
- build a full plugin marketplace product experience.

## Product Direction

The target user experience is:

```text
create-aio-plugin acme.redactor
create-aio-plugin doctor ./acme.redactor
create-aio-plugin validate --strict ./acme.redactor
create-aio-plugin replay --explain ./acme.redactor fixtures/request.json gateway.request.afterBodyRead
create-aio-plugin pack ./acme.redactor
```

Then in the desktop app:

1. Import the `.aio-plugin` package.
2. Grant permissions and enable the plugin.
3. Send a request through the gateway.
4. Open the plugin detail view.
5. See recent runtime failures, audit events, hook names, event types, failure kinds, trace IDs, and concise mutation summaries.

This release should make it clear whether a plugin did nothing, matched a rule, changed a body/header/chunk/log, warned, blocked, failed open, failed closed, timed out, or tripped a circuit.

## Architecture

0.62.1 uses four layers.

### 1. Contract Source

Plugin API v1 metadata remains the source of truth for hook names, hook status, permissions, mutation fields, permission dependencies, failure policy defaults, timeout defaults, and runtime availability.

Implementation may either generate TypeScript/docs/scaffold metadata from the contract or strengthen checkers that prove they remain aligned. The important invariant is that future hook or permission changes cannot silently drift between Rust, SDK, docs, and scaffold.

### 2. Developer Tools

`packages/create-aio-plugin` becomes the local diagnostic surface.

`doctor` reports structured diagnostics:

- `severity`: `error`, `warn`, or `info`;
- `code`: stable machine-readable code;
- `message`: short human-readable explanation;
- `path`: file or JSON path when applicable;
- `hint`: actionable next step.

`validate --strict` preserves the existing success/failure behavior while adding package-level checks:

- `plugin.json` exists and parses;
- manifest passes SDK validation;
- declarative rules files exist;
- declarative rule documents parse;
- rule hooks are declared in manifest hooks;
- rule targets are compatible with the hook;
- requested permissions apply to at least one declared hook;
- runtime policy limitations are reported clearly for WASM and native runtimes.

`replay --explain` continues to support declarative-rules fixtures and returns an explanation model instead of only the final action:

- input hook;
- plugin id and runtime;
- evaluated rule count;
- matched rule ids;
- target field and JSON path;
- action kind;
- output kind: `pass`, `replace`, `block`, `warn`;
- mutation summary without storing or printing full sensitive payloads by default;
- warnings for unsupported replay constructs.

### 3. Host Observability

The Rust gateway remains the authority for real runtime behavior. It already persists audit events and runtime failures. 0.62.1 should expose that data more clearly through existing command/query patterns.

The plugin detail panel should add:

- a runtime status summary;
- runtime failures grouped by hook and failure kind;
- audit logs grouped or labeled by hook, event type, risk, and trace ID;
- copyable trace IDs where available;
- refresh behavior that invalidates plugin detail and audit queries;
- empty states that explain when no hook has run yet.

If implemented, clearing audit logs or runtime failures must be a secondary action and should not be required for 0.62.1 acceptance.

### 4. Provider Internal Boundary

Provider adapter and protocol bridge work stays internal. 0.62.1 should add tests around host-owned behavior:

- route ordering;
- provider failover;
- OAuth limit snapshots;
- token counting;
- session binding;
- cx2cc protocol bridge request and response translation;
- provider-specific request preparation.

These tests are not a public provider plugin API. They make future provider extension work safer by proving current behavior before any separate RFC.

## Functional Scope

### Required

- Add `create-aio-plugin doctor`.
- Add `create-aio-plugin validate --strict`.
- Add `create-aio-plugin replay --explain`.
- Improve declarative-rules diagnostics and replay explanation.
- Improve plugin detail observability in the GUI.
- Add or strengthen contract drift gates for SDK/docs/scaffold metadata.
- Add provider internal acceptance tests.

### Optional

- Add GUI actions to clear runtime failures or audit logs.
- Add `replay --fixture-from-log <traceId>`.
- Add richer rule examples beyond the minimal fixtures.
- Generate documentation tables directly from the contract.

### Deferred

- WASM explain/replay parity.
- Marketplace productization.
- Provider Plugin API.
- Plugin API v2.
- JS/WebView runtime.

## Data Flow

### Local Development

```text
plugin directory
  -> create-aio-plugin doctor
  -> create-aio-plugin validate --strict
  -> create-aio-plugin replay --explain
  -> create-aio-plugin pack
  -> .aio-plugin package
```

`doctor` and `validate --strict` use SDK validation plus package-level file and rule checks. `replay --explain` uses the declarative-rules replay model and emits a stable explanation JSON shape.

### Runtime Observability

```text
gateway hook execution
  -> GatewayPluginAuditEvent
  -> plugin_audit_logs / plugin_runtime_failures
  -> plugin_get / plugin_list_audit_logs
  -> React Query
  -> PluginsPage detail panel
```

The GUI should treat persisted audit data as runtime evidence and not infer sensitive details from raw request or response bodies.

## Error Handling

Developer tools should prefer stable diagnostic codes over free-form text. A command with any `error` severity diagnostics should exit non-zero. Warnings keep exit code zero in 0.62.1; warning-as-error behavior is outside this spec and would require a separate flag and acceptance criteria.

Runtime observability should handle missing audit tables the same way existing plugin loading repairs missing plugin schema: fail softly when possible and show a readable UI error state when recovery is not possible.

Replay explain must clearly distinguish:

- unsupported runtime;
- unsupported declarative rule shape;
- rule did not match;
- rule matched but produced no mutation;
- rule matched and produced a mutation;
- invalid fixture shape.

## Compatibility

Plugin API v1 remains externally compatible. Existing plugin manifests that validate today should continue to validate unless they rely on behavior that the current host already rejects. `validate --strict` may report additional warnings or errors for package-level defects such as missing rule files, but it must not redefine the public manifest contract.

Provider Plugin API remains private. Any provider adapter changes are internal refactors or tests.

WASM remains policy-gated. The WASM scaffold can still exist, but 0.62.1 does not enable arbitrary WASM marketplace execution.

## Testing Strategy

### TypeScript Unit Tests

`packages/create-aio-plugin/src/scaffold.test.ts` or new focused tests should cover:

- `doctor` reports missing `plugin.json`;
- `doctor` reports missing declarative rule files;
- `validate --strict` rejects unknown hooks and permission mismatch;
- `validate --strict` rejects malformed rule documents;
- `replay --explain` reports pass when no rule matches;
- `replay --explain` reports matched rule id and replace summary;
- `replay --explain` reports block and warn actions;
- pack still preserves binary-safe behavior.

`packages/plugin-sdk/src/index.test.ts` should continue to prove Plugin API v1 compatibility and hook-scoped permission dependencies.

### Rust Tests

Gateway/plugin tests should cover:

- audit events still persist for hook failures;
- runtime failures still persist with hook name, failure kind, message, and trace ID;
- plugin detail loads runtime failures and audit logs;
- declarative-rules behavior remains aligned with replay fixtures where practical;
- provider adapter acceptance scenarios remain host-owned and pass.

### Frontend Tests

Plugin page tests should cover:

- runtime failure section renders when failures exist;
- audit events render with hook/event/risk/trace metadata;
- empty state renders when no runtime events exist;
- refresh invalidates plugin detail or audit queries;
- error states stay readable.

### Release Gates

Expected release verification:

```bash
pnpm --filter create-aio-plugin test
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
pnpm typecheck
cd src-tauri && cargo test plugin --lib
cd src-tauri && cargo test gateway_plugin --lib
cd src-tauri && cargo test provider --lib
git diff --check
```

The final branch should also pass the repository pre-push hook.

## Acceptance Criteria

0.62.1 is accepted when:

1. A declarative-rules plugin can be scaffolded, diagnosed, strictly validated, replayed with explanation, packed, installed, enabled, and inspected in the GUI.
2. `doctor` and `validate --strict` produce stable structured diagnostics for manifest, rule-file, hook, permission, and runtime-policy problems.
3. `replay --explain` reports evaluated rules, matched rules, action kind, and mutation summary without dumping full sensitive payloads by default.
4. The plugin detail panel shows runtime failures and audit events in a way that identifies hook name, failure kind, event type, risk, and trace ID.
5. Contract drift gates prove Rust, SDK, docs, and scaffold metadata remain aligned for Plugin API v1.
6. Provider adapter work remains internal and covered by acceptance tests.
7. Plugin API v1 remains externally compatible.
8. Provider Plugin API remains closed.
9. WASM remains policy-gated.
10. All release gates pass.

## Implementation Notes

The implementation plan should keep commits small:

1. Developer tool diagnostic model and `doctor`.
2. Strict validation package/rule checks.
3. Replay explanation output.
4. GUI observability improvements.
5. Contract drift generation/checker work.
6. Provider internal acceptance tests.
7. Documentation and release verification.

Each task should be independently testable. If any task becomes large enough to require unrelated refactors, split it into a follow-up plan rather than widening 0.62.1.
