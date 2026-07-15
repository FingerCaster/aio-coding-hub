# Implementation Plan: Merge upstream main 057c0682

## 1. Pre-Merge Gate

- [x] Load `trellis-before-dev` for the root cross-layer package.
- [x] Fetch `upstream` and assert `upstream/main` still resolves to
  `057c06821b5159fda202bce5cfbf1ef3afb410f9`.
- [x] Assert the isolated branch starts from or contains
  `main@ae3e62340be1` and the main worktree has not been modified by this task.
- [x] Install dependencies in the isolated worktree with
  `pnpm install --frozen-lockfile` if they are not already available.
- [x] Record pre-merge status and run `git merge-tree` once more to detect drift
  from the researched 16-conflict set.

## 2. Create the Isolated Merge

- [x] Run `git merge --no-ff --no-commit 057c06821b5159fda202bce5cfbf1ef3afb410f9`.
- [x] Verify `MERGE_HEAD` is the exact upstream target.
- [x] Confirm no path outside the predicted scope is unexpectedly conflicted.

## 3. Resolve Release Metadata

- [x] Keep fork `0.60.26` in `.release-please-manifest.json`, `package.json`,
  `src-tauri/Cargo.toml`, the root package in `src-tauri/Cargo.lock`, and
  `src-tauri/tauri.conf.json`.
- [x] Keep the fork `CHANGELOG.md`; do not add a duplicate upstream `0.60.13`
  release section.
- [x] Verify all five version sources agree structurally after resolution.

## 4. Resolve Gateway Semantics

- [x] Preserve the configured per-request attempt limit independently of the
  circuit threshold, while reserving OAuth, continuation-id, and enabled
  transient-retry budget in `provider_iterator.rs`; strict model discovery
  still takes precedence.
- [x] Chain trigger/timeout attribution and health-neutral propagation in
  `success_non_stream.rs`, `thinking_signature_rectifier_400.rs`, and
  `upstream_error.rs`.
- [x] Preserve conditional timeout attribution and import neutral circuit
  behavior in `provider_router.rs`; keep both test families.
- [x] Keep all fork and upstream route test helpers and tests in `routes.rs`.
- [x] Audit every auto-merged `provider_health_neutral` propagation site and
  every terminal/early-error special-settings path.
- [x] Stop for user direction if any real behavior cannot satisfy both the fork
  and upstream contracts in `design.md`.

## 5. Resolve Frontend Semantics

- [x] Combine all fork special-setting summaries with the upstream Codex system
  marker constant and predicate.
- [x] Retain both existing test suites and add a mixed-marker parser regression
  if coexistence is not already explicit.
- [x] Compute route display metadata and Codex system-request state together in
  `HomeRequestLogsPanel.tsx`.
- [x] Retain detail-dialog reasoning/route tests and the upstream assertion that
  the system marker remains list-only.
- [x] Add a list-row regression that renders both route mismatch and system
  request signals when the same log contains both settings.

## 6. Static Merge Audit

- [x] Assert no conflict markers remain with
  `rg -n '^(<<<<<<<|=======|>>>>>>>)'` over tracked source/task files.
- [x] Run `git diff --check`.
- [x] Review `git diff --cc` and the complete staged diff; confirm no functional
  conflict was resolved with a blanket side selection.
- [x] Verify the merge result keeps all upstream classifier, model-discovery,
  health-neutral, logging, and UI symbols plus all fork model-route and
  continuation symbols.
- [x] Assert all five version sources are `0.60.26` and there is no duplicate
  upstream changelog heading.

## 7. Focused Tests

- [x] Run frontend conflict-area tests:

  ```powershell
  pnpm exec vitest run `
    src/services/gateway/__tests__/requestLogSpecialSettings.test.ts `
    src/components/home/__tests__/HomeRequestLogsPanel.test.tsx `
    src/components/home/__tests__/RequestLogDetailDialog.test.tsx `
    src/constants/__tests__/crossLayerContracts.test.ts
  ```

- [x] Run focused Rust tests:

  ```powershell
  cargo test --manifest-path src-tauri/Cargo.toml codex_request_classifier --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml provider_health_neutral --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml provider_max_attempts --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml mock_runtime_router_large_known_length_5xx_uses_bounded_error_preview --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml codex_models --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml model_route_mapping --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning_continuation --lib --locked
  cargo test --manifest-path src-tauri/Cargo.toml success_event_stream --lib --locked
  ```

## 8. Full Validation

- [x] Run `pnpm build`.
- [x] Run `pnpm check:precommit:full`.
- [x] Run `pnpm check:prepush`.
- [x] If a check fails, fix only in the isolated worktree, rerun the focused
  failing check, then rerun both aggregate gates.
- [x] Record commands and outcomes in the task/session journal.

### Validation Record

- Frontend conflict-area tests: 4 files, 107 tests passed.
- Rust focused tests passed for request classification, health-neutral routing,
  provider budgets, model discovery, model-route mapping, continuation repair,
  and streaming.
- `pnpm build`, `pnpm check:precommit:full` (13/13), and
  `pnpm check:prepush` (15/15) passed.
- Full Rust suite: 2041 passed, 3 ignored, 0 failed.
- `git diff --cached --check` passed.

### Debug Retrospective

- Root cause: implicit assumption. Upstream coupled the circuit failure
  threshold to one request's attempt budget even though the two counters have
  different lifecycles.
- Detection gap: focused calculation tests did not expose the route-level
  multi-request behavior; the full pre-push suite did.
- Prevention: an explicit budget formula regression, full Rust validation after
  shared failover-input changes, and the backend contract in
  `.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md`.

## 9. Commit and Integrate

- [ ] Assert `057c06821b5159fda202bce5cfbf1ef3afb410f9` will be a parent/ancestor
  of the merge result and the isolated worktree is otherwise ready to commit.
- [ ] Ensure the hook environment resolves `node` and `pnpm`, then create the
  merge commit.
- [ ] Complete Trellis spec/update/finish steps and archive the task on the
  isolated branch; commit resulting task metadata as required.
- [ ] Recheck the main worktree base, dirty paths, and overlap with the
  integration diff.
- [ ] If main changed, integrate it into the isolated branch and repeat the
  required validation before continuing.
- [ ] Fast-forward local `main` to the clean, validated integration branch.
- [ ] Verify local `main` contains the upstream target, retains the pre-existing
  user changes, and has no new untracked/generated residue from validation.
- [ ] Do not push or release.

## Rollback Points

- Before the merge commit: use `git merge --abort` only if abandoning the
  isolated merge; the main worktree is untouched.
- After the merge commit but before main integration: leave the branch/worktree
  intact for inspection; do not reset main.
- If final main integration preconditions fail: stop and report the changed
  base or overlapping dirty paths instead of forcing the merge.
