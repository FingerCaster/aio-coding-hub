# Merge latest upstream into main

## Goal

Integrate the latest pinned `upstream/main` into the fork in an isolated
worktree, preserve fork-specific product contracts and the user's existing
dirty main worktree, validate the complete result, and only then merge the
validated history back into local `main`.

## Background

- Pre-task local `main` is `099cf90d8b05c5fd1f39cb4f0fafd624b131da66`.
  It contains two local-only Trellis bookkeeping commits and is 12 commits
  behind `origin/main`.
- The pinned fork remote input is
  `origin/main@1a551cbee35960fbb954e475a13b2d8d55d709df`.
- The pinned upstream input is
  `upstream/main@4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`.
- The fork/upstream merge base is
  `419086fb36a4976e30d384add2fec086d99e648c`, the prior imported upstream
  revision. The new upstream drift is seven commits touching 75 files.
- The main worktree has pre-existing modified, deleted, and untracked files.
  They belong to the user and are not part of this merge.
- `origin` remains the normal repository target. `upstream` is fetch-only and
  its push URL remains `DISABLED`.

## Requirements

### R1. Fixed, auditable inputs

- Use the immutable origin and upstream SHAs above throughout analysis,
  merge, validation, and ancestry checks.
- Preserve local `main` committed ancestry by first merging the pinned
  `origin/main` into the isolated branch.
- Merge the pinned upstream SHA with a real two-parent merge commit. Do not
  cherry-pick, squash, rebase, or select only a subset of upstream commits.

### R2. Isolated integration

- Perform all origin/upstream merging, conflict resolution, dependency
  installation, and validation in
  `D:/UGit/aio-coding-hub-fork-upstream-merge-main-20260729` on branch
  `FingerCaster/merge-upstream-2026-07-29`.
- Do not modify the main worktree during integration except for Trellis task
  metadata and the final protected main update.

### R3. Conflict and behavior policy

- Carry forward every non-conflicting upstream change.
- Resolve conflicts by behavior, not blanket file-level `ours` or `theirs`.
- Preserve fork release identity, security/privacy hardening, plugin tooling,
  provider model discovery, account-usage query ownership, gateway routing,
  and Codex continuation/reasoning contracts.
- Combine compatible upstream behavior, especially Claude OAuth login
  restoration, client ChatGPT account-header isolation, OAuth expiry display
  refresh, folder/development-time usage views, provider metric trends, and
  unknown bundle-mode suppression.
- The user explicitly authorizes autonomous conflict decisions for this task.
  Use existing specs, tests, and fork behavior as the decision source and
  document every non-mechanical resolution.
- Do not fix defects that reproduce on the pinned upstream revision without
  the fork merge. Record such findings as out of scope.

### R4. Release and remote safety

- Keep the fork release version at `0.60.30` in all canonical version files.
- Do not import the upstream `0.60.16` changelog section into the fork's
  already-published release history; the upstream commits remain visible in
  merge ancestry for a later fork release.
- Do not push, open a PR, create a release, or change either remote URL.
- Never push to `upstream`.

### R5. Validation and final main integration

- Audit all conflict resolutions plus auto-merged overlap in gateway OAuth,
  auth-header cleanup, provider queries, usage data flow, generated bindings,
  and dependency metadata.
- Run focused tests for each upstream behavior and each fork contract touched
  by the merge, followed by the repository build and full precommit/prepush
  gates.
- Before updating local `main`, create a named, recoverable stash containing
  every pre-existing tracked and untracked user change and record its object
  ID. Do not include integration changes in that stash.
- Update local `main` only after the isolated branch is fully validated. Apply
  the preserved stash afterward and verify each pre-existing modified,
  deleted, and untracked path is restored.
- If stash application conflicts with the new base, resolve only to preserve
  the user's pre-merge working-tree content and keep the stash object until
  final verification succeeds.

## Acceptance Criteria

- [ ] The isolated branch contains the pre-task local `main`, pinned
      `origin/main`, and pinned `upstream/main` histories.
- [ ] The upstream SHA is the second parent of a real merge commit and an
      ancestor of the final local `main`.
- [ ] All seven upstream commits and every non-conflicting change are present.
- [ ] The 11 predicted textual conflicts and any additional real conflicts
      have documented, behavior-level resolutions with no conflict markers.
- [ ] Fork version `0.60.30`, release history, security/privacy behavior,
      plugin tooling, model discovery, provider query ownership, and gateway
      contracts remain intact.
- [ ] Upstream Claude OAuth, ChatGPT account-header isolation, OAuth expiry
      refresh, usage folder/development-time views, metrics trend UI, and
      bundle-mode display behavior are present and covered by passing tests.
- [ ] Focused tests, `pnpm build`, `pnpm check:precommit:full`, and
      `pnpm check:prepush` pass in the isolated worktree.
- [ ] Local `main` is updated only after validation and contains the validated
      integration history.
- [ ] Every pre-existing main-worktree change is restored, and no unrelated
      user file is committed, removed, or rewritten.
- [ ] `origin` remains the default remote, `upstream` remains fetch-only, and
      no push or release occurs.

## Out of Scope

- Releasing or pushing the integrated result.
- Refactoring adjacent code or fixing pinned-upstream defects unrelated to
  concrete merge conflicts.
- Updating Trellis itself from `0.6.6` to `0.6.7`.
