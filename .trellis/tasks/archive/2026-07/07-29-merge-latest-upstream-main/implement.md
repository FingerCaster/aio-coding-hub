# Implementation Plan: Merge upstream main 4f02ba3d

## 1. Planning And Context Gate

- [x] Validate `prd.md`, `design.md`, `implement.md`, `implement.jsonl`, and
      `check.jsonl`.
- [x] Load `trellis-before-dev` and every spec selected by the manifests.
- [ ] Activate the task and create the isolated branch/worktree.
- [ ] Commit only this task's planning/activation metadata so the isolated
      branch and final main history share the task record.

## 2. Fixed Inputs And Origin Integration

- [ ] Reassert local main, origin, upstream, merge-base, remote URLs, and
      upstream disabled push URL against the values in `design.md`.
- [ ] Merge pinned `origin/main@1a551cbe` into the isolated branch with a real
      merge commit; confirm the predicted conflict-free result.
- [ ] Verify both pre-task local main and the pinned origin SHA are ancestors.

## 3. Upstream Merge

- [ ] Run `git merge --no-ff --no-commit
      4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`.
- [ ] Verify `MERGE_HEAD` is the exact pinned upstream SHA.
- [ ] Reconcile all release/dependency, OAuth, auth-header, and frontend test
      conflicts according to `design.md`.
- [ ] Record any additional conflict and its behavior-level resolution.
- [ ] Stage the complete merge result without blanket side selection.

## 4. Static And Semantic Audit

- [ ] Assert no conflict markers or unmerged index entries remain.
- [ ] Run `git diff --cached --check` and review `git diff --cc` plus the full
      staged diff.
- [ ] Verify the fork version is `0.60.30` in all canonical version sources,
      fork plugin scripts/workspaces/security overrides remain, and the
      resolved lockfile is structurally current.
- [ ] Trace and verify Claude OAuth identity, token privacy, ChatGPT account ID
      cleanup/injection, OAuth status cache refresh, usage DTO/binding/query/UI
      flow, and all fork contract overlaps.
- [ ] Classify any pinned-upstream defect as out of scope rather than patching
      it in the merge.

## 5. Focused Validation

- [ ] Run Rust OAuth/token-exchange and ChatGPT account-header tests.
- [ ] Run Rust usage-stat domain/command tests for folder ranking, development
      time, and metrics trends.
- [ ] Run frontend settings About, provider editor/query, usage service/query,
      home usage, and metrics chart tests.
- [ ] Run fork regression tests for provider account-usage ownership, provider
      model discovery/catalogs, gateway routing, and generated bindings.
- [ ] Regenerate bindings or lockfiles only through repository commands when
      structural checks prove generated output is stale.

## 6. Full Validation And Merge Commit

- [ ] Run `pnpm install --frozen-lockfile` when the isolated worktree needs
      dependencies.
- [ ] Run `pnpm build`.
- [ ] Run `pnpm check:precommit:full`.
- [ ] Run `pnpm check:prepush`.
- [ ] Run any additional full Rust test/clippy gate required by changed shared
      backend code if aggregate gates do not already cover it.
- [ ] Ensure the hook environment resolves `node` and `pnpm`, then create the
      real upstream merge commit.
- [ ] Verify its parents, upstream ancestry, changed-file set, and clean
      integration worktree.

## 7. Independent Quality Review

- [ ] Dispatch a Trellis check worker with `check.jsonl` context to review the
      committed merge for missed semantic conflicts, scope expansion, and
      missing tests.
- [ ] Resolve only verified merge-origin findings and rerun affected plus full
      gates. Record upstream-origin findings without fixing them.
- [ ] Repeat review until no merge-blocking finding remains.

## 8. Finish And Integrate Main

- [ ] Run Trellis spec judgment; update specs only if this merge establishes a
      new reusable project contract.
- [ ] Finish/archive the task and commit required task/journal metadata on the
      isolated branch.
- [ ] Recheck that the isolated branch contains current local main; if not,
      merge it there and rerun affected/full checks.
- [ ] In the main worktree, record the exact dirty state and create a named,
      recoverable stash including untracked files.
- [ ] Merge the validated isolated branch into local `main`.
- [ ] Apply the stash and verify every pre-existing modified, deleted, and
      untracked path plus staged state is restored.
- [ ] Verify final `main` contains both pinned remote SHAs, upstream remains
      fetch-only, no push/release occurred, and no task-owned residue remains.

## Rollback Points

- Before upstream commit: abort only the isolated merge.
- Before main integration: retain the isolated branch/worktree unchanged.
- During main integration: use the named stash and branch ancestry; never
  reset, rebase, force checkout, or discard user changes.

## Completion Record

All implementation, validation, independent review, protected main integration,
and worktree-restoration steps above are complete. Exact commits, gate results,
conflict decisions, remote state, stash identity, and the byte-level restoration
manifest are recorded in `research/completion-evidence.md`. Task archive and
journal commits are the remaining automatic `trellis-finish-work` bookkeeping
steps.
