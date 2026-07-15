# Merge upstream main 057c0682

## Goal

Integrate `upstream/main@057c06821b51` into the fork without regressing
fork-specific behavior, validate the integrated result in an isolated
worktree, and merge it back to local `main` only after every required check
passes.

## Background

- The isolated worktree is
  `D:/OrcaProjects/aio-coding-hub-fork/merge-upstream-2026-07-15` on branch
  `FingerCaster/merge-upstream-2026-07-15`.
- The branch starts from local `main@ae3e62340be1`; the existing uncommitted
  `AGENTS.md` and `analysis-codex-retry-gateway-2026-07-07.html` changes in the
  main worktree are not present in the isolated checkout.
- The merge base is `b4e1f8d97136`. Local `main` has 163 unique commits and
  `upstream/main` has 3 unique commits.
- The upstream commits add Codex system-request classification and circuit
  breaker isolation (`8b821825`, `fa1f0e26`) and publish upstream version
  `0.60.13` (`057c0682`). The fork is already at version `0.60.26`.
- Of the 40 files changed upstream, 33 were also changed by the fork after the
  merge base. `git merge-tree` predicts 16 textual conflicts: 6 release or
  version metadata conflicts and 10 functional or test conflicts across
  gateway routing, failover/repair paths, request-log parsing, and request-log
  UI.

## Requirements

- Create a real Git merge of `upstream/main@057c06821b51` so the resulting
  history records the imported upstream ancestry.
- Carry forward all non-conflicting upstream behavior.
- Resolve functional conflicts by comparing base, fork, and upstream behavior
  at the semantic level. Preserve every fork-added feature and safety contract,
  especially model-routing detection and Codex continuation-repair guards,
  while also integrating every compatible upstream change.
- Do not apply a blanket "ours" or "theirs" resolution to functional files.
  Choose per behavior unit and combine both sides whenever they can coexist.
  If a concrete behavior is mutually exclusive and no compatibility design can
  preserve both, stop with file/behavior evidence and ask the user before
  choosing either side.
- Preserve the fork's release version and release history; upstream `0.60.13`
  metadata must not downgrade the fork from `0.60.26`.
- Add or adapt tests for any manually reconciled behavior instead of resolving
  conflicts only at the textual level.
- Perform all merge, conflict-resolution, install, build, and test work in the
  isolated worktree. Do not modify the current main worktree before the
  isolated branch is approved for integration.
- Merge the validated branch back into local `main` only after all required
  checks pass and the main worktree still has the expected base and unrelated
  user changes can be preserved.
- Do not push branches, create a release, modify the upstream remote, or push
  to upstream. Upstream access is limited to fetching and reading the exact
  target commit for the local merge.

## Acceptance Criteria

- [ ] The integration branch contains `057c06821b51` as an ancestor of its
  final merge result.
- [ ] Codex system requests are classified, propagated to logs, displayed in
  the UI, and isolated from ordinary circuit-breaker accounting as intended by
  upstream.
- [ ] Existing fork model-routing mismatch detection and continuation-repair
  behavior remain covered and passing, including the repository-level Codex
  reasoning/continuation safety requirements in `AGENTS.md`.
- [ ] Package metadata remains on the fork release line and does not regress to
  upstream version `0.60.13`.
- [ ] Focused tests for touched gateway and request-log behavior pass.
- [ ] The repository's full pre-commit/full-scope quality gates pass in the
  isolated worktree, including formatting, generated-contract checks,
  TypeScript checks/tests, and Rust checks/tests required by the changed area.
- [ ] The integration branch is committed with a clean worktree before it is
  merged back.
- [ ] Local `main` receives the tested integration without losing or altering
  its pre-existing uncommitted `AGENTS.md` and analysis HTML changes.
- [ ] No remote push or release is performed.
