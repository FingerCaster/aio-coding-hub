# Implementation Plan: External Codex retry gateway integration

## Delivery Topology

This parent task owns the shared contract foundation, worker coordination,
integration glue, combined validation, and the final user decision gate. The
four child tasks own parallel implementation:

| Wave | Task | Worktree | Depends on |
| --- | --- | --- | --- |
| Foundation | parent integration scaffold | `retry-gateway-integration` | reviewed local `main` SHA |
| Parallel | `07-15-external-gateway-runtime` | `retry-gateway-runtime` | foundation commit |
| Parallel | `07-15-codex-route-coordinator` | `retry-gateway-routing` | foundation commit |
| Parallel | `07-15-external-gateway-frontend` | `retry-gateway-frontend` | foundation commit |
| Parallel | `07-15-remove-local-reasoning-guard` | `retry-gateway-removal` | foundation commit |
| Integration | parent merge/glue/check | `retry-gateway-integration` | all four `worker_done` reports |
| Supplemental | `07-16-codex-auto-review-route-neutral` | `codex-auto-reviewer-model-routing` | exact commit `1d5d2ac904fea3b5590dbf13b61a8c44956f7c74` |
| Review | independent Codex audit | isolated review worktree | frozen integration SHA, model `gpt-5.6-sol`, effort `max` |
| Review | independent Claude audit | isolated review worktree | same frozen integration SHA |
| Decision | merge to `main` | current main worktree | reviewed and validated frozen integration commit plus user approval |

The detailed checklist sections below are source requirements for the child
plans. A worker executes only items assigned by its child task. The integration
coordinator does not duplicate those edits while workers are active.

## 0. Create the Orca Integration Foundation

- [ ] Verify Orca runtime/orchestration availability and record repo ID
      `15eeba31-c114-489d-890c-752bbcc4f61d`, exact local `main` SHA, and dirty
      main paths.
- [ ] Create `retry-gateway-integration` as a top-level Orca worktree with an
      explicit repo selector and local `main` base. Do not rely on Orca's
      upstream-oriented repository metadata and do not fetch AIO `upstream`.
- [ ] Transfer the reviewed parent/child Trellis artifacts and protected current
      `AGENTS.md` baseline into the integration branch without changing the
      current main worktree. Leave the analysis HTML in main untouched.
- [ ] Start the parent and four child Trellis tasks only after the user approves
      this revised plan.
- [ ] Load `trellis-before-dev`, add the minimum shared Rust/settings/command
      DTO scaffold, regenerate bindings once, add contract fixtures, and make
      the scaffold compile without implementing worker-owned behavior.
- [ ] Run contract/generated-binding checks and commit the foundation. Record
      the exact foundation SHA in every child dispatch.
- [ ] If the execution turn is still constrained to Codex inline mode, stop and
      request an orchestration-capable turn before creating agent workers.

## 0.1 Dispatch Four Parallel Workers

- [ ] Create four Orca child worktrees from the explicit foundation local branch
      and parent them to the integration worktree. Use agent-first creation and
      retain each full `<repoId>::<worktreePath>` ID and single startup terminal
      handle.
- [ ] Create four Orca orchestration tasks with the foundation task as their
      dependency, then dispatch with lifecycle injection to the matching worker.
- [ ] Include parent/child artifacts, exact allowed/forbidden paths, required
      focused tests, no-main/no-push rules, and `worker_done` payload fields in
      every dispatch.
- [ ] Supervise with Orca orchestration messages and decision gates. Do not use
      terminal output polling as completion authority and do not let workers
      merge one another.
- [ ] Keep worker task/journal/archive state coordinator-owned to avoid four
      branches editing shared Trellis journal files; workers commit product and
      child-owned test changes and report results.

## 1. Pre-Development and Baseline Gate

- [ ] Load `trellis-before-dev` for the root backend and cross-layer package
      before product-code edits.
- [ ] Re-read `prd.md`, `design.md`, `research.md`, and `inventory.md`; resolve
      any drift before implementation.
- [ ] Record `git status` and preserve the pre-existing `AGENTS.md` edit and
      `analysis-codex-retry-gateway-2026-07-07.html` without overwrite or
      cleanup.
- [ ] Confirm `origin` remains the default repository for normal operations;
      this task does not fetch or mutate AIO `upstream`.
- [ ] Run focused current CLI-proxy, Codex config/provider-sync, gateway retry,
      request-log, and Codex-tab tests to establish the baseline.
- [ ] Turn `inventory.md` into a tracked implementation checklist and capture
      negative-search output before deletion.

## 2. Add Pure Contracts and Managed State

- [ ] Add Rust enums/DTOs for route mode, runtime phase, trust state, Node
      status, process status, update candidate, error projection, and public
      gateway status.
- [ ] Add settings fields for desired enable, selected full SHA, preferred
      external port, and optional Node override with strict migration/default
      validation.
- [ ] Add a versioned bounded `manager.json` parser/writer that reconstructs
      all paths under the AIO-owned root and rejects unsupported schema,
      traversal, symlinks/reparse points, invalid SHA/port, and malformed
      ownership data.
- [ ] Add the durable route transition journal with prepare/commit/recovery
      states and atomic writes.
- [ ] Unit-test settings migration, invalid state, unknown schema, path safety,
      interrupted journal cases, and monotonic public status generations.

## 3. Refactor Codex Route Coordination

- [ ] Refactor the Codex CLI-proxy manifest so its backup is a canonical
      unproxied config and its live file is an explicit route projection.
- [ ] Add exhaustive `Unproxied | DirectAio | Guarded` projection and
      verification helpers. Keep AIO and external origins typed separately and
      reject equal/recursive targets.
- [ ] Add a pure, read-only Provider Sync enable plan reporting current/target
      provider and whether session/provider-state synchronization is required.
      Recompute it under the lifecycle lock before mutation.
- [ ] Make CLI-proxy status chain-aware without regressing Claude/Gemini status
      or sync behavior.
- [ ] Route Codex CLI-proxy enable/disable, AIO gateway rebind, raw managed TOML
      writes, and provider-mode changes through the shared lifecycle section.
- [ ] Preserve existing OAuth-compatible proxy behavior and MCP re-sync after
      successful host Codex config transitions.
- [ ] Add unit/integration tests for all route transitions, exact rollback,
      interrupted transition recovery, occupied origins, and unchanged
      Claude/Gemini manifests.

## 4. Make `remote_compaction` Chain-Aware

- [ ] Build provider identity from canonical `features.remote_compaction` and
      project `aio` or `OpenAI` to the current route without learning a routed
      URL as canonical state.
- [ ] Extend the existing provider-sync transaction boundary so canonical
      config, live config, sessions, SQLite/global state, CLI-proxy backup,
      external compatibility state, and transition journal commit coherently.
- [ ] Reuse that same provider-sync transaction when gateway/CLI-proxy enable
      changes provider identity; do not add a second session rewrite path.
- [ ] Keep the existing Codex-process-closed check and stable provider-sync
      error behavior.
- [ ] Prevent raw TOML saves from bypassing the coordinator when they touch
      `remote_compaction`, `model_provider`, or managed provider tables.
- [ ] Add a matrix covering toggle on/off in unproxied, direct AIO, guarded,
      bypassed-recovering, and gateway-disabled states.
- [ ] Inject failures at backup write, provider-sync, live config write,
      compatibility-state write, and final verification; assert exact prior
      provider, feature value, route, manifest, and desired state are restored.

## 5. Implement Official Source Store

- [ ] Add fixed repository identity and build-time recommended full-SHA
      metadata. Do not accept caller-supplied repository or archive URLs.
- [ ] Resolve exact commits and official `main` ancestry through bounded GitHub
      API requests with explicit 404/transient/rate-limit handling.
- [ ] Download only by canonical SHA with a GitHub/codeload redirect allowlist.
- [ ] Reuse/factor existing bounded ZIP extraction and package hashing rather
      than copy a weaker extractor.
- [ ] Validate expected gateway/scripts layout, run bounded Node syntax checks,
      fingerprint the source, and atomically promote to `sources/<sha>`.
- [ ] Revalidate cached source before each execution and implement same-SHA
      repair without changing the approved selection.
- [ ] Test abbreviated/invalid SHA, fork/branch rejection, non-ancestor commit,
      redirects, timeouts, oversized/archive-bomb shapes, traversal, duplicate
      paths, symlink/reparse entries, partial staging, corrupt cache, offline
      cached start, and offline first install.

## 6. Review and Pin the Initial Recommendation

- [ ] Fetch the exact external candidate
      `ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2` from the official repository
      and verify its ancestry immediately before review.
- [ ] Review its process/config/status/UI/restore contracts against
      `research.md` and `design.md`, including changes since the research
      snapshot if the candidate changes.
- [ ] Run its available syntax, unit, install/restore, launch, and gateway E2E
      tests in an isolated external checkout; do not copy its source into AIO.
- [ ] Pin the exact reviewed SHA in AIO build metadata only after those checks
      pass. If the reviewed SHA changes, update research/design evidence and
      show the user the concrete difference before changing product behavior.

## 7. Implement Node and Process Ownership

- [ ] Add automatic Node discovery using existing CLI-manager path patterns and
      a direct bounded `--version` probe requiring major version 18+.
- [ ] Add validated manual absolute-path selection, persistence only after a
      successful probe, per-launch revalidation, and reset-to-automatic.
- [ ] Spawn the exact Node executable with argument vectors, controlled cwd/PATH,
      hidden Windows process settings, bounded startup output, and explicit
      shutdown behavior.
- [ ] Generate AIO-owned external config and compatibility state with fixed
      loopback listener, AIO upstream, current provider, and instance nonce;
      never write a usable external whole-file restore backup.
- [ ] Verify PID, process start identity, executable/source/config/state paths,
      commit, nonce, listener, health `process_id`, and upstream before reuse,
      stop, update, or route commit.
- [ ] Implement preferred/persisted port selection with non-owned conflict
      avoidance and post-verification persistence.
- [ ] Test missing/old/timed-out Node, manual path replacement safety, stale PID,
      PID reuse signals, mismatched health, foreign port occupant, fallback
      port, child crash, stop timeout, and no-kill-on-identity-mismatch.

## 8. Add Lifecycle Service and Supervisor

- [ ] Implement enable from both unproxied and direct-AIO states using the
      prepare/start/verify/route/commit transaction and exact rollback.
- [ ] Implement gateway-only disable to direct AIO, explicit CLI-proxy disable
      to unproxied, retry, confirmed uninstall, and cleanup-needed state.
- [ ] Implement update staging, direct-AIO bypass, candidate switch, old-source
      rollback, and recovery-paused fallback.
- [ ] Add startup reconciliation after AIO starts and before ordinary Codex CLI
      sync can overwrite the selected route.
- [ ] Reorder normal exit cleanup so host Codex is safe, the external process is
      stopped, desired state is retained, and AIO then stops within bounded
      cleanup deadlines.
- [ ] Add periodic health supervision, direct-AIO fail-open, serialized bounded
      exponential backoff, storm protection, explicit retry, and status events.
- [ ] Ensure first-install/update preparation cannot commit after a newer
      disable, selection, app-exit, or operation generation.
- [ ] Test every transition from `design.md`, application restart, interrupted
      update, external crash/recovery, recovery pause, startup with corrupt
      state/cache, and desired-versus-actual status truthfulness.

## 9. Add the AIO Management Bridge

- [ ] Start a dedicated loopback-only bridge with short-lived session launch
      URLs and HttpOnly SameSite cookies; never expose it through the AIO
      WSL/LAN gateway listener.
- [ ] Add an explicit method/path allowlist, body/response/time bounds, streamed
      forwarding, and per-request managed-instance revalidation.
- [ ] Proxy the external UI and supported status/log/analytics/import/export/
      probe APIs.
- [ ] Parse config writes and reject changes to managed listener, upstream, and
      health fields while forwarding external-owned behavior settings.
- [ ] Intercept restore/exit and invoke AIO gateway-only disable; do not forward
      external whole-file restore.
- [ ] Overlay authoritative provider/route/commit state in status responses so
      an `aio`/`OpenAI` rename cannot leave stale UI identity.
- [ ] Add bridge tests for missing/expired session, stale generation, foreign
      process, path/method escape, oversized body, protected config mutation,
      restore interception, post-write health failure, and external process
      disappearance during streaming.

## 10. Add Typed Commands and Frontend State

- [ ] Add narrow Tauri commands and register them; require backend confirmation
      flags for first download, implicit CLI-proxy enable, unreviewed commit,
      provider/session sync, uninstall, and WSL coverage warning. Reject a
      stale enable-plan generation.
- [ ] Regenerate and format `src/generated/bindings.ts`; update service/query
      adapters and complete typed fixtures. Do not hand-author generated DTOs.
- [ ] Add React Query/event reconciliation that ignores stale generations and
      exposes desired state, actual route, process health, update state, and
      errors separately.
- [ ] Replace the old Codex guard settings/statistics UI with the compact
      external gateway switch/status/details entry, Node selector, commit
      selector/trust, check/apply update, retry, disable, and uninstall flows.
- [ ] When enabling from a disabled Codex CLI proxy, show that both will be
      enabled. Gateway-only disable keeps the CLI proxy on; sidebar CLI-proxy
      disable invokes the combined shutdown path.
- [ ] If enable changes provider identity, add the exact `current -> aio/OpenAI`
      transition and existing session/provider synchronization impact to the
      consolidated confirmation. Reuse Provider Sync success counts and the
      existing Codex-running/process-check error guidance.
- [ ] Show the persistent WSL-not-protected warning in enable confirmation,
      status, and details whenever existing WSL Codex targeting is active.
- [ ] Add the details route with AIO-owned back/refresh/open-browser/lifecycle
      controls and a sandboxed iframe sourced only from a bridge session.
- [ ] Confirm route exit, iframe unload, window hide, and system-browser open do
      not stop the gateway; only lifecycle actions do.
- [ ] Update CSP to loopback-only frame access and verify no remote/LAN frame
      origin is accepted.

## 11. Remove the Local Reasoning Guard

- [ ] Delete the four dedicated backend modules and remove their declarations,
      imports, outcomes, buffers, classifiers, continuation branches, and
      guard-only tests from shared failover/runtime files in `inventory.md`.
- [ ] Remove all local guard settings/defaults/migrations/validation/frontend
      mappings and add only the external integration settings.
- [ ] Remove dedicated request-log statistics commands/queries/types, guard
      badges/cards/detail parsing, and tests while retaining generic rows and
      unrelated special-setting projections.
- [ ] Remove local guard configuration/statistics controls from Codex UI rather
      than layering the external gateway beside them.
- [ ] Remove obsolete probe/validation scripts and active documentation; keep
      historical changelog entries.
- [ ] Surgically update `AGENTS.md` and the active attempt-budget/spec wording
      without discarding unrelated user changes.
- [ ] Run the inventory negative searches excluding archived task/history and
      build outputs; investigate every remaining active match.

## 12. Focused Validation

- [ ] Run Rust unit/integration tests for settings migration, source store,
      Node/process ownership, route coordinator, provider sync, CLI proxy,
      bridge, lifecycle/startup/cleanup, and WSL separation.
- [ ] Run retained gateway route tests for transient retry, OAuth refresh,
      `previous_response_id`, failover/circuit, model-route diagnostics,
      streaming/non-streaming, usage, cancellation, and request logging.
- [ ] Run frontend tests for commands/services/query state, confirmations,
      route-mode status, update/rollback state, WSL warnings, iframe navigation,
      CLI-proxy linkage, `remote_compaction`, and removal of old guard UI.
- [ ] Run generated-binding and cross-layer contract checks.
- [ ] Run `git diff --check` and negative searches for conflict markers, local
      guard symbols, accidental external source, and stale generated fields.

Suggested focused commands, adjusted to final test module names:

```powershell
Push-Location src-tauri
cargo test --lib codex_retry_gateway
cargo test --lib infra::cli_proxy
cargo test --lib infra::codex_config
cargo test --test cli_proxy_startup_recovery
cargo test --test codex_provider_sync
Pop-Location

pnpm exec vitest run `
  src/query/__tests__/cliProxy.test.tsx `
  src/hooks/__tests__/useCliProxyControls.test.tsx `
  src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx `
  src/app/__tests__/AppRoutes.test.tsx

pnpm check:generated-bindings
```

## 13. Full Quality and Packaging Gate

- [ ] Treat any aggregate run before the independent review wave as preliminary.
      Run the authoritative full gate again only after both reviewers clear the
      final candidate and all accepted findings are fixed.
- [ ] Run `pnpm lint`, `pnpm typecheck`, and `pnpm tauri:fmt`.
- [ ] Run `pnpm build`.
- [ ] Run `pnpm check:precommit:full` and `pnpm check:prepush`.
- [ ] Run the full Rust suite after the shared failover/local-guard removal.
- [ ] Build at least the current-platform Tauri bundle and inspect packaged
      resources to prove no external source or Node runtime is included.
- [ ] Verify release/update metadata contains only the recommended SHA metadata,
      not mutable refs or external code.
- [ ] Rerun focused failures after each fix, then rerun both aggregate gates.

## 14. End-to-End Acceptance Scenarios

- [ ] CLI proxy off -> enable gateway -> explicit combined confirmation ->
      guarded route; gateway-only off -> direct AIO; CLI proxy off -> unproxied.
- [ ] Repeat enable with a provider-name change and verify the confirmation
      names both providers and session sync; cancel, stale-plan, running-Codex,
      and injected Provider Sync failures leave source/process/route/settings
      at the exact prior committed state.
- [ ] Toggle `remote_compaction` off/on while guarded with Codex closed and
      verify `aio`/`OpenAI`, external route, status, provider-sync data, and
      later direct/unproxied restoration.
- [ ] Attempt the same change while Codex is running and verify the existing
      safe refusal leaves every route/state byte unchanged.
- [ ] Enter and exit embedded details repeatedly, hide/show the app, and open
      the bridge in the system browser without stopping interception.
- [ ] Use the external page restore action and verify AIO turns desired state
      off, routes direct AIO, stops the owned process, and preserves newer Codex
      settings.
- [ ] Kill/corrupt the owned process and verify immediate direct-AIO fallback,
      truthful recovery status, bounded retry, and verified guarded recovery.
- [ ] Apply a deliberately failing candidate and verify rollback to the previous
      source; make both fail and verify direct-AIO recovery-paused state.
- [ ] Restart AIO with desired state on and verify automatic restoration without
      modifying the system autostart preference.
- [ ] Enable WSL Codex targeting and verify host traffic is guarded, WSL remains
      direct AIO, and every required warning remains visible.
- [ ] Occupy port `4610` with a foreign listener and verify fallback selection
      without adoption or termination.
- [ ] Disable versus confirmed uninstall and verify only uninstall removes the
      AIO-owned feature root; a manual/default external installation is intact.

## 15. Integrate Worker Branches

- [ ] Verify each active dispatch produced one valid `worker_done`, a reviewable
      commit SHA, declared modified paths, focused test results, and no forbidden
      main/upstream/push operation.
- [ ] Audit each branch against its child ownership before merge; send out-of-
      scope changes back to the owning worker or exclude them explicitly.
- [ ] Merge routing, runtime, removal, and frontend branches in the documented
      order into `retry-gateway-integration` with real merge ancestry.
- [ ] Merge exact supplemental commit
      `1d5d2ac904fea3b5590dbf13b61a8c44956f7c74` after frontend. Exclude
      `.codex-review-last.md` and resolve the shared detail-summary surface so
      both gateway and route-neutral behavior survive.
- [ ] Resolve only shared registry/startup/cleanup/settings/binding/task metadata
      in the integration worktree. Stop for a user decision if a conflict reveals
      product behavior that cannot coexist.
- [ ] Add cross-worker lifecycle glue, regenerate final bindings, run focused
      checks after each merge, then run Sections 12-14 in full.
- [ ] Archive/finish child tasks and record worker commits/results only after the
      combined tree passes their acceptance criteria.

## 16. Run Independent Candidate Reviews

- [ ] Commit all source merges and integration glue, ensure the integration
      worktree is clean, and record one immutable candidate SHA.
- [ ] Verify the local Codex CLI accepts model `gpt-5.6-sol` with
      `model_reasoning_effort="max"`; do not replace either value if launch
      fails.
- [ ] Create two isolated Orca review runs from the same candidate SHA. Assign
      one read-only audit to Codex `gpt-5.6-sol` with effort `max` and one to
      Claude, and require each report to state its model/effort or agent
      identity and reviewed SHA.
- [ ] Require findings first, ordered by severity, with file/line evidence,
      behavioral impact, and missing-test coverage. Reviewers do not edit,
      merge, push, release, or operate on `main`.
- [ ] Resolve accepted findings only in `retry-gateway-integration`, rerun
      focused checks, freeze a replacement SHA, and request confirmation for
      every affected finding. Repeat until both review tracks are clear.

## 17. Run Final Validation and Ask Before Main Merge

- [ ] After both independent reviews clear the final candidate, run Sections
      12-14 as the authoritative focused/full/package/end-to-end validation.
- [ ] Commit any validation fixes and task records, repeat affected independent
      review confirmation when behavior changes, freeze the validated
      integration SHA, and ensure its worktree is clean.
- [ ] Verify current local `main` still contains the expected base and inventory
      its user-owned dirty paths. Preserve the current `AGENTS.md` intent and
      untracked analysis HTML.
- [ ] If local `main` advanced, merge it into the integration branch and rerun
      affected focused tests plus all aggregate/packaging gates.
- [ ] Report foundation SHA, four worker merge commits, supplemental commit,
      integration fixes, conflicts, both independent review results, full
      test/package results, and final integration SHA.
- [ ] Ask the user whether to merge/fast-forward the validated integration
      branch into `main`. Do not perform that operation before the answer.
- [ ] Do not push, release, or remove Orca worktrees as part of this gate.

## Rollback Checkpoints

- Before route-coordinator migration: old CLI-proxy tests define the preserved
  backup/auth/MCP behavior.
- Before local-guard deletion: the new external route, crash fallback,
  `remote_compaction`, and bridge restore tests must already pass.
- Before generated binding refresh: Rust DTOs and command registry must be
  stable enough to avoid repeated unrelated churn.
- Before recommendation labeling: external source review and compatibility
  tests must pass for the exact full SHA.
- Before completion: full validation, package-content audit, negative searches,
  and the end-to-end acceptance scenarios must pass.
- Before main integration: the frozen integration SHA and full validation report
  must be presented and the user must explicitly approve the merge.
- No push or release is part of this task unless separately requested after the
  main-merge decision.
