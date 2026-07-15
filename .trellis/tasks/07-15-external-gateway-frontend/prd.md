# Build external gateway frontend

## Parent Contract

This child implements the user-facing portions of parent requirements R2-R8,
R10-R13. Parent artifacts and the shared generated-contract foundation are
authoritative.

## Goal

Replace the local reasoning-guard configuration/statistics UI with one compact
external gateway experience: truthful desired/actual status, consolidated
enable confirmation, commit/Node/update controls, persistent WSL warning, and a
sandboxed details route backed by an AIO bridge session.

## Requirements

### R1. Truthful state and linked controls

- Show desired state separately from runtime phase and effective route. Never
  label direct-AIO bypass/recovery as protected.
- Gateway enable/disable and sidebar Codex CLI-proxy disable use the foundation
  command contract: gateway-only off keeps CLI proxy on; CLI-proxy off performs
  combined external shutdown/unproxied restore.
- Ignore stale query/event generations and disable conflicting actions while an
  operation is pending.

### R2. Consolidated consent

- Build one enable plan/confirmation that conditionally shows first runtime
  download, unreviewed SHA, implicit CLI-proxy enable, exact Provider Sync
  `current -> aio/OpenAI` plus session/backup/Codex-closed impact, and WSL not
  protected.
- Send required confirmation flags and plan generation. Cancel sends no mutate
  command. Reuse existing Provider Sync running/process-check success/error
  vocabulary.

### R3. Source, Node, update, and uninstall UX

- Display official repository, full selected/active/recommended/previous SHA,
  trust state, undeclared license, effective port, Node path/version/source, and
  stable errors without environment leakage.
- Update checks only; a second dialog confirms apply with candidate summary and
  rollback target. Provide manual official SHA validation/selection, retry,
  reset-to-auto Node, gateway-only disable, and confirmed uninstall.

### R4. Details and navigation

- Add an AIO details route with back/exit, refresh, status, open-browser,
  update/retry/disable controls outside a sandboxed iframe.
- Obtain iframe/browser URLs only from the bridge-session command. Leaving the
  route, unloading the frame, or hiding the app must not stop the gateway.
- Show fallback status/browser action when the bridge or embed is unavailable.

### R5. WSL coverage

- Show the non-blocking enable warning and persistent status/details warning
  whenever existing WSL configuration targets Codex. Do not imply WSL traffic
  is protected.

### R6. Remove old frontend ownership

- Remove local guard rule/template/retry/continuation controls, stats queries,
  request-log badges/cards/detail parsing, settings adapters/fixtures, and tests.
- Preserve unrelated route diagnostics, Codex system markers, logs, provider
  settings, approvals, model/reasoning/service tier, and manual Provider Sync.

### R7. Parallel ownership boundary

- Own frontend `src/**` changes and the Tauri CSP frame-source edit, except
  generated bindings. Do not edit Rust implementation, external runtime, CLI
  route/config, backend old-guard removal, generated bindings, task/journal
  state, main, or remotes.

## Acceptance Criteria

- [ ] Status/query/event tests prove desired/actual route truth and stale
      generation protection.
- [ ] Enable confirmation covers every conditional warning, exact Provider Sync
      transition, cancel/no-mutation, backend confirmation flags, and errors.
- [ ] Update, manual SHA, Node, retry/disable/uninstall, and WSL states are fully
      testable and responsive.
- [ ] Details iframe/browser entry works through bridge sessions; navigation and
      window lifecycle never invoke disable/stop; CSP accepts loopback only.
- [ ] Frontend negative searches find no active legacy guard UI/statistics while
      retained request-log and Codex settings tests pass.
- [ ] Worker commits only allowed paths, reports focused tests/commit, and never
      touches main/push/release.

## Out of Scope

- Rust source/Node/process/bridge, Codex config/Provider Sync implementation,
  startup/cleanup, generated binding regeneration, backend guard deletion,
  packaging, integration merge, and release.

