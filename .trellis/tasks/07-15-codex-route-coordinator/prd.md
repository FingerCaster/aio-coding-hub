# Make Codex routing chain-aware

## Parent Contract

This child implements parent requirements R2-R4 and the routing portion of R10.
The parent artifacts are authoritative. Work starts from the shared foundation
commit and must not depend on uncommitted main state.

## Goal

Refactor the host Codex CLI-proxy/config pipeline into explicit unproxied,
direct-AIO, and guarded route modes while preserving canonical backups,
Provider Sync, `remote_compaction`, OAuth-compatible auth, MCP sync, and exact
failure rollback.

## Requirements

### R1. Exhaustive route modes

- Model `Unproxied`, `DirectAio`, and `Guarded` explicitly with distinct AIO and
  external origins and no recursive/equal target.
- Keep Claude/Gemini CLI proxy behavior unchanged and make Codex status truthful
  for the current route rather than assuming direct AIO.

### R2. Canonical configuration projection

- Treat the CLI-proxy backup as canonical unproxied Codex config, not a copy of
  a live routed file. Project only AIO-owned provider/base-url/auth/sandbox
  overlays for the active route.
- Preserve unrelated TOML, comments, auth strategy, custom Codex Home, and MCP
  updates. Route changes and raw managed TOML writes must share one serialized
  lifecycle/transition journal contract.

### R3. Provider Sync confirmation plan and transaction

- Add a pure read-only enable plan reporting current provider, target
  `aio/OpenAI`, whether synchronization is required, and Codex-process
  preconditions.
- Recompute the plan under the lifecycle critical section. Reuse the existing
  Provider Sync core for session files, SQLite/global state, backups, and error
  semantics; do not add a second history rewriter.

### R4. `remote_compaction` coherence

- Toggle `aio`/`OpenAI` atomically in unproxied, direct AIO, guarded, bypassed,
  and gateway-disabled states while keeping the correct route and external
  compatibility projection callback.
- A failure at any stage restores the exact prior config, provider-sync data,
  manifest, route, and generation. Codex-running safety remains unchanged.

### R5. Linked CLI-proxy behavior

- Expose operations needed for combined external enable, gateway-only disable,
  explicit CLI-proxy disable, crash direct-AIO fallback, update bypass, startup
  reconciliation, and normal exit restoration.
- This worker implements route primitives and the existing CLI-proxy service
  boundary; the parent integration service orders external process operations.

### R6. Parallel ownership boundary

- Own existing Rust CLI proxy, Codex config/provider sync, their focused app/
  command services, and tests only. Do not edit new external runtime modules,
  local guard/failover removal files, frontend/generated bindings, shared
  startup/cleanup/registry glue, main, or remotes.

## Acceptance Criteria

- [ ] All route-mode transition and rollback tests pass without regressions to
      Claude/Gemini, OAuth-compatible auth, MCP, or Codex Home handling.
- [ ] Provider enable plan and backend revalidation cover no-change, provider
      change, stale plan, running Codex, process-check error, and sync failure.
- [ ] `remote_compaction` succeeds and rolls back exactly across every required
      route state and injected failure boundary.
- [ ] Startup/exit/crash/update route primitives are generation-aware and never
      point Codex at an unverified or recursive origin.
- [ ] Worker stays within ownership, commits focused changes, reports tests and
      integration callbacks, and never touches main/push/release.

## Out of Scope

- Source download, Node/process control, management bridge, supervisor loop, or
  external config implementation.
- Frontend UX/CSP, local guard deletion, generated bindings, registry/startup/
  cleanup integration, packaging, and main merge.

