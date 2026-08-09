# Self-Check Complete

## Review Target

- Base range: `097704c1..a750a6af4197d48c1b62f04d0ad8585f697abd89`
- Method: Trellis check workflow, task PRD/design/research/implementation artifacts,
  relevant backend/cross-layer specs, plugin security/audit documentation, and
  `docs/plugins/plugin-api-v1-contract.json`.
- Scope guard: neither the target range nor the review fixes change
  `packages/plugin-sdk/**` or `packages/create-aio-plugin/**`. No LAN auth,
  activation quarantine, recovery journal, Usage Ledger, or other excluded
  feature was added.

## Files Checked

- Contract and wire surface: `docs/plugins/plugin-api-v1-contract.json`,
  `src/generated/bindings.ts`, `src-tauri/src/commands/plugins.rs`,
  `src-tauri/src/domain/plugins.rs`, `src/services/plugins.ts`,
  `src/services/__tests__/plugins.test.ts`.
- Install, persistence, and UI: `src-tauri/src/app/plugin_service.rs`,
  `src-tauri/src/infra/plugins/repository.rs`, `src/query/plugins.ts`,
  `src/query/__tests__/plugins.test.tsx`, `src/pages/PluginsPage.tsx`,
  `src/pages/__tests__/PluginsPage.test.tsx`,
  `src/pages/plugins/PluginConfigSchemaForm.tsx`, and its test.
- Gateway runtime: `src-tauri/src/gateway/plugins/context.rs`, `contract.rs`,
  `mutation.rs`, `pipeline.rs`, and `src-tauri/src/gateway/proxy/logging.rs`.
- Extension Host: `src-tauri/src/app/plugins/extension_host.rs`,
  `extension_host_process.rs`, `extension_host_registry.rs`,
  `extension_host_worker.rs`, and `runtime_executor.rs`.

## Issues Found and Fixed

1. `src-tauri/src/app/plugin_service.rs:411` - official install and the legacy
   test helper committed plugin, config, permissions, and audit rows in separate
   writes. They now use one repository transaction, validate before materializing,
   and remove the promoted directory when the transaction fails. An adverse
   trigger test proves all five tables and files roll back.
2. `src-tauri/src/infra/plugins/repository.rs:590` - Extension Host storage writes
   derived `config_version` from the manifest and replaced `sensitive_keys` with
   an empty list. They now load and preserve the stored config metadata inside an
   immediate transaction.
3. `src-tauri/src/infra/plugins/repository.rs:551` - scalar config or scalar
   `storage` could be silently replaced, losing user or runtime data. Both config
   and storage writes now reject invalid roots without mutating persisted state;
   direct tests cover metadata retention and both data-loss paths.
4. `src-tauri/src/app/plugins/extension_host_registry.rs:383` - `Duration::MAX`
   could overflow deadline construction, while a timed-out warm instance only
   scheduled a later abort. Deadline creation is checked and timeout cleanup
   performs an immediate abort whenever the process mutex is available.
5. `src-tauri/src/app/plugins/extension_host_registry.rs:536` - the active recycler
   ignored a child's `recycled=true` response and raced same-plugin calls. It now
   serializes on the plugin operation lock, verifies exact instance identity,
   removes/disposes recycled instances, and prunes idle orphan lock entries.
6. `src-tauri/src/gateway/plugins/pipeline.rs:1336` - half-open probe state was not
   bound to a plugin snapshot. Refreshes could strand a circuit or let stale and
   current snapshots repeatedly steal the single probe. Probe ownership now uses
   a weak snapshot identity, rejects stale claims, and has deterministic refresh
   race tests.
7. `src/query/plugins.ts:225` - successful install commands returning no detail
   skipped list/contribution invalidation, and remote install cache updates could
   use the request ID instead of the returned detail ID. Local/remote installs now
   invalidate broad state in both branches and use a stable detail key; query tests
   prove all null-result adverse paths.

## Contract and Security Conclusions

- Local GUI preview/confirm passes the preview SHA-256 into install/update, and
  install hashes and extracts one captured byte buffer before promoting it.
- Recorded current and historical versions are rejected under the package
  mutation lock; rollback continues to use the recorded manifest and directory.
- Canonical Extension Host context serialization is camelCase and bounded;
  legacy snake_case aliases stay only at the QuickJS compatibility boundary.
- Request sensitive headers remain gated by `request.header.readSensitive`.
  Full response-header visibility under `response.header.read` matches the
  published contract, which deliberately classifies that label as low risk and
  defines no response-sensitive variant.
- Header mutation remains transactional and follows fail-open/fail-closed policy;
  fail-closed log persistence remains enforced on invalid output, unknown policy,
  timeout, and circuit-open early exits.
- Deadline accounting covers operation gate, same-plugin queue, cold/warm start,
  RPC, validation, and cleanup. Lock acquisition order was checked across calls,
  recycler, per-plugin disposal, retain, idle disposal, and dispose-all paths.

## Issues Not Fixed

- Cross-resource crash atomicity cannot be absolute without a recovery journal:
  a process kill after directory promotion but before SQLite commit can leave an
  orphan directory. All ordinary error paths clean up; a recovery journal is
  explicitly excluded from this task.
- `expectedChecksum` remains nullable on the generated local-install IPC input so
  older callers keep wire compatibility. The current UI always supplies it, but a
  legacy direct IPC caller retains the pre-existing unbound-install behavior.
  Making the field required would be a separately approved breaking contract
  change.

No open in-scope correctness defect remains after the fixes above.

## Verification Results

- Focused Rust: circuit refresh/half-open (4 passed), active recycler (2 passed),
  idle lock cleanup (1 passed), plus direct deadline, install rollback, storage,
  header/log, context-budget, and crash paths in the complete lib run.
- Focused frontend: `src/query/__tests__/plugins.test.tsx` (8 passed).
- Plugin hardening: API contract passed; SDK tests 29 passed; SDK typecheck passed;
  Rust lib 2685 passed, 4 ignored.
- Full frontend: 304 files and 2750 tests passed.
- Full Rust: 2685 passed, 4 ignored, followed by every integration test group
  passing under `--test-threads=1`.
- Static checks: TypeScript typecheck, ESLint, Prettier, Cargo fmt, Cargo check,
  Clippy `-D warnings`, generated bindings, plugin API contract, plugin docs, and
  plugin completion all passed.
- Scaffold compatibility: `create-aio-plugin` tests 30 passed without modifying
  the package.
- Diff checks: `git diff --check` passed; forbidden package path counts are zero
  in both target and review diffs.

## Summary

Checked all 24 files in the target commit, found 7 issue groups, fixed all 7,
and left 0 open in-scope defects. Two explicit residual boundaries remain: the
excluded crash-recovery journal and nullable checksum compatibility for legacy
direct IPC callers.
