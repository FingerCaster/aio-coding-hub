# Implementation Plan: Remove local Codex reasoning guard

## 1. Foundation and Baseline

- [ ] Verify foundation SHA, protected `AGENTS.md` baseline, parent inventory,
      child ownership, and retained backend specs.
- [ ] Capture pre-removal negative searches and run focused guard plus adjacent
      retry/routing/logging/settings tests.
- [ ] Confirm foundation external settings/DTO fields that must survive.

## 2. Delete Dedicated Modules

- [ ] Delete four dedicated guard/continuation modules and remove declarations,
      imports, exports, fixtures, and direct callers.
- [ ] Remove guard-only test helpers and cases after identifying retained shared
      behavior coverage.

## 3. Simplify Shared Gateway Paths

- [ ] Remove guard concurrent outcomes, retry reasons/state, context buffers,
      attempt reservations, finalization diagnostics, stream buffering/
      interception/continuation, and non-stream inspection/retry branches.
- [ ] Preserve ordinary attempts/retries/failover/circuits, route diagnostics,
      stream passthrough, request finalization, usage, and cancellation.
- [ ] Add/retain regressions for every adjacent branch changed.

## 4. Remove Backend Settings and Statistics

- [ ] Remove old settings fields/defaults/enums/templates/migrations/validation/
      service mappings while preserving foundation and unrelated fields.
- [ ] Update migration/persistence tests for ignored old keys and canonical
      omission.
- [ ] Remove dedicated request-log stats command/query/types and backend marker
      parsing without modifying stored rows or generic retention.
- [ ] Remove only the obsolete registry entry needed for compilation; leave new
      external command registration to integration.

## 5. Remove Active Maintenance Material

- [ ] Remove obsolete scripts and active README/wiki/spec links/claims.
- [ ] Update attempt-budget wording for the post-guard contract without changing
      generic retry/circuit semantics.
- [ ] Remove only the obsolete continuation release clause from `AGENTS.md` and
      preserve every other current user rule.
- [ ] Keep historical `CHANGELOG.md` entries.

## 6. Validate and Handoff

- [ ] Run negative searches, Rust formatting, focused retained tests, full Rust
      suite if feasible in the worker, and `git diff --check`.
- [ ] Audit all modified/deleted paths against ownership and review settings/
      request-log data preservation.
- [ ] Commit child-owned changes and send one `worker_done` with commit, paths,
      tests, negative searches, and remaining integration cleanup.
- [ ] Do not edit frontend/generated/shared Trellis state, merge, push, release,
      or touch main.

