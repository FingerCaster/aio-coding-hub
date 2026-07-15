# Design: Merge upstream main 057c0682

## Decision Summary

- Merge the exact commit `057c06821b5159fda202bce5cfbf1ef3afb410f9`
  with a real two-parent Git merge.
- Resolve conflicts by behavior unit, not by file-level `ours` or `theirs`.
- Preserve all fork-added behavior and all compatible upstream behavior.
- Full validation exposed one incompatible retry-budget rule: the fork treats
  `failover_max_attempts_per_provider` as the per-request limit, while upstream
  raises it to the circuit failure threshold. The user selected the fork
  behavior; circuit failures continue to accumulate across requests.
- Preserve the fork release identity at `0.60.26`. Upstream release metadata
  for `0.60.13` is source-repository metadata and must not downgrade or
  duplicate the fork release line.
- Stop and ask the user only if the real merge exposes a behavior that cannot
  satisfy both contracts described below.

## Repository Topology

| Item | Value |
| --- | --- |
| Integration base | `main@ae3e62340be1` |
| Upstream target | `upstream/main@057c06821b51` |
| Merge base | `b4e1f8d97136` |
| Fork-only commits | 163 |
| Upstream-only commits | 3 |
| Isolated branch | `FingerCaster/merge-upstream-2026-07-15` |
| Isolated worktree | `D:/OrcaProjects/aio-coding-hub-fork/merge-upstream-2026-07-15` |

The upstream target must be re-fetched and verified immediately before the
merge. A newer upstream commit is outside this task unless separately reviewed.

## Behavior Contracts

### Upstream behavior to import

1. Classify a request as a Codex system request only for an exact Codex POST
   Responses path with trusted nested turn metadata whose
   `thread_source == "system"`; malformed, oversized, or mismatched metadata
   fails closed.
2. Persist a structured `codex_system_request` marker through normal, streaming,
   early-error, abort, and provider-failure request-log paths.
3. Keep system requests provider-health-neutral: success, failure, and cooldown
   handling must not mutate circuit-breaker state.
4. Treat Codex model discovery GET requests as unobserved, health-neutral, and
   limited to one attempt per provider while still allowing failover to another
   provider.
5. Display a `Codex system request` badge in request-list rows, while keeping
   that marker out of the detail metrics surface.

### Fork behavior to preserve

1. Model-route auditing records requested versus actual model and reasoning
   effort after bridge, plugin, stream, and repair transformations, scoped to
   the final provider.
2. Model-route mismatch presentation remains visible in list rows and request
   details without falling back to stale or another provider's mapping.
3. Codex reasoning continuation repair retains the single-visible-response,
   fail-closed classifier, split client/provider usage, response-id, pre-commit
   timeout, duration diagnostics, and 20 MiB aggregate-cap contracts.
4. The configured per-provider attempt limit is not raised to the circuit
   failure threshold; circuit failures accumulate across requests instead.
5. Configured transient retries reserve provider attempt budget only when the
   effective provider retry policy is enabled, including provider overrides.
6. OAuth refresh and `previous_response_id` recovery retain their internal
   retry budget.
7. Circuit failure attribution retains the triggering gateway error and records
   first-byte timeout seconds only for `GW_UPSTREAM_TIMEOUT`.

## Conflict Matrix

### Release metadata

| Path | Fork side | Upstream side | Resolution |
| --- | --- | --- | --- |
| `.release-please-manifest.json` | `0.60.26` | `0.60.13` | Keep fork value. |
| `package.json` | `0.60.26` | `0.60.13` | Keep fork value; no other upstream change exists in this conflict. |
| `src-tauri/Cargo.toml` | `0.60.26` | `0.60.13` | Keep fork value. |
| `src-tauri/Cargo.lock` | root package `0.60.26` | root package `0.60.13` | Keep fork root-package value. |
| `src-tauri/tauri.conf.json` | `0.60.26` | `0.60.13` | Keep fork value. |
| `CHANGELOG.md` | Fork releases through `0.60.26`, including a fork-specific `0.60.13` | A different upstream `0.60.13` section | Keep the fork file. The imported conventional feature/fix commits remain in merge ancestry and are eligible for the next fork release; do not create duplicate `0.60.13` headings or rewrite an already published fork release. |

### Gateway retry and circuit behavior

| Path | Fork side | Upstream side | Combined resolution |
| --- | --- | --- | --- |
| `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs` | Attempt capacity includes OAuth refresh, Codex continuation-id recovery, and enabled transient retry budget. | Attempt capacity also respects circuit failure threshold; Codex model discovery uses a strict configured limit. | Use the user-selected fork semantics for the incompatible limit: do not let the circuit threshold expand one request's attempt budget. Retain OAuth, continuation-id, and enabled transient-retry reservations, plus upstream's strict one-attempt model discovery path. Circuit failures still accumulate across requests. |
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs` | Circuit recording carries gateway trigger and timeout attribution. | Circuit recording carries `provider_health_neutral`. | Chain both builders on the same `RecordCircuitArgs`; neither field replaces the other. |
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/thinking_signature_rectifier_400.rs` | Same trigger/timeout attribution on rectifier failure. | Same health-neutral propagation. | Chain both builders and keep existing retry/rectifier behavior. |
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs` | Same trigger/timeout attribution plus fork transient-retry decisions. | Same health-neutral propagation. | Chain both builders. Health-neutral wraps all success/failure/cooldown mutation, while fork retry decisions and error attribution remain intact. |
| `src-tauri/src/gateway/proxy/provider_router.rs` | `with_trigger` stores timeout seconds only for timeout errors and tests non-timeout clearing. | Adds `provider_health_neutral`, unchanged circuit snapshots, and tests neutral success/failure/cooldown. | Preserve the fork's conditional timeout behavior, add upstream's neutral field/builder/guards, and retain assertions for both properties in the builder test. |

### Routing integration tests

| Path | Fork side | Upstream side | Combined resolution |
| --- | --- | --- | --- |
| `src-tauri/src/gateway/routes.rs` | Adds finite repeating/sequence upstream fixtures for continuation, model-route, plugin, and retry tests. | Adds an open-ended counting status fixture and model-discovery single-attempt/neutral-circuit tests. | Keep all helpers under distinct names and retain all tests. This is a test-helper insertion conflict, not a production behavior choice. |

### Request-log parsing and UI

| Path | Fork side | Upstream side | Combined resolution |
| --- | --- | --- | --- |
| `src/services/gateway/requestLogSpecialSettings.ts` | Adds reasoning guard/feature/continuation summaries, reasoning effort resolution, model-route mappings, and terminal-setting selection. | Adds the structured Codex system marker constant and fail-closed predicate. | Keep all fork types/functions and add the upstream constant/predicate next to the shared parser. Mixed arrays must support all marker kinds simultaneously. |
| `src/services/gateway/__tests__/requestLogSpecialSettings.test.ts` | Covers fork reasoning and model-route semantics. | Covers valid, invalid, malformed, and object/array Codex system markers. | Retain both suites and add a mixed-marker regression if existing tests do not prove coexistence. |
| `src/components/home/HomeRequestLogsPanel.tsx` | Computes route-aware model display, mismatch title, and mismatch styling. | Computes the Codex system marker and renders the row badge. | Compute both values and render both visual signals. A system request with a route mismatch must show both, without changing row sizing unexpectedly. |
| `src/components/home/__tests__/RequestLogDetailDialog.test.tsx` | Covers explicit/default reasoning effort and model-route details. | Verifies the system marker is not promoted into details. | Retain every test; they assert different surfaces. |

## Auto-Merge Audit

The remaining upstream edits are predicted to auto-merge, but must still be
reviewed semantically after the real merge:

1. `CodexRequestClassifierMiddleware` runs after body parsing and before model
   inference. It must append to the shared special-settings collection rather
   than replacing fork reasoning or route settings.
2. Codex model discovery is identified before middleware execution, starts
   health-neutral, is excluded from request observation, and receives a strict
   per-provider attempt limit of one.
3. `provider_health_neutral` must propagate through middleware context, request
   context, failover context, attempt recording, streaming finalization, early
   errors, and abort handling.
4. System POST Responses requests remain observable and included in statistics;
   only circuit health is neutral. Existing model inference, model-route audit,
   response repair, and continuation guard behavior must still execute.
5. Special settings must preserve all markers at terminal log selection; a
   late model-route setting must not erase the early system marker, and an early
   marker must not force selection of stale route data.

## Validation Design

### Focused cross-behavior regressions

- Provider attempt calculation preserves an explicit one-attempt cap when no
  retry reason is enabled, reserves internal/transient retries when required,
  and keeps strict model discovery at exactly one attempt.
- `RecordCircuitArgs` proves trigger/timeout attribution and health-neutral
  state coexist in one value.
- Rust route tests prove model discovery is one attempt per provider, can move
  to the next provider, and never mutates either circuit.
- Classifier tests prove exact path/method/CLI checks, nested metadata shape,
  byte limit, malformed input, and case-sensitive system source.
- Request-log tests prove the system marker survives success, failure, stream,
  early-error, and abort paths without excluding usage statistics.
- Frontend parser tests prove system, reasoning, continuation, and model-route
  settings can coexist in one JSON array.
- UI tests prove route-mismatch display and the system-request badge can render
  together, while details continue to omit the system marker.
- Existing model-route and continuation repair focused tests remain green.

### Full gates

After focused tests, run both aggregate repository gates:

- `pnpm check:precommit:full`
- `pnpm check:prepush`

Also run `pnpm build`, because the aggregate stages type-check but do not build
the production frontend bundle.

## Integration and Rollback

1. Perform the real merge with `--no-commit` in the isolated worktree.
2. If an unplanned mutually exclusive product behavior appears, leave the merge
   paused and ask the user with the base/fork/upstream behavior and viable
   options.
3. Resolve, audit, and test before creating the merge commit.
4. Commit and archive the Trellis task only after all gates pass and the branch
   is clean.
5. Before updating local `main`, verify it still has the expected ancestry and
   that its existing uncommitted `AGENTS.md` and analysis HTML changes do not
   overlap the integration diff.
6. Prefer a fast-forward of local `main` to the fully validated branch. If
   `main` advanced, integrate the new base in the isolated worktree and rerun
   affected/full gates before touching the main worktree.
7. Before the final main update, the branch can be abandoned safely without any
   main worktree change. No remote push or release belongs to this task.
