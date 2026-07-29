# Design: Publish Merged Upstream Release

## Release State Machine

1. **Validated local main**: `d8006da9` is a strict fast-forward of
   `origin/main@1a551cbe` and the protected dirty worktree is untouched.
2. **Release PR reset**: close PR #17 and delete the stale release-please branch
   because it proposes the invalid lower version `0.60.6` and stale changelog
   range.
3. **Remote integration gate**: recheck `origin/main`, push `HEAD:main`, require
   the matching `ci` run, explicitly dispatch `dev-build.yml --ref main`, verify
   its `headSha` equals the pushed commit, and require both runs to complete
   successfully.
4. **Fresh release PR**: dispatch `release.yml` without a tag so release-please
   creates `0.60.31`; wait for Cargo.lock synchronization and all PR checks.
5. **Release commit gate**: inspect and merge only the verified release PR,
   fetch `origin`, require the merge commit's main CI, and explicitly dispatch
   and pass dev-build at that same commit SHA.
6. **Immutable release build**: dispatch `release.yml` with
   `release_tag=aio-coding-hub-v0.60.31` and the full release commit SHA as
   `target_commitish`.
7. **Publication gate**: require tag resolution, four-platform builds,
   `latest.json`, publication, and the Homebrew job to succeed. In the current
   no-token state, capture its explicit safe-skip log rather than claiming the
   nonexistent tap was updated.

## Safety Boundaries

- Every GitHub mutation uses explicit `-R FingerCaster/aio-coding-hub` or an
  explicit API repository path.
- `dev-build.yml` ignores `main` pushes; every main-branch dev-build gate is a
  deliberate workflow dispatch whose returned run must match the intended SHA.
- The main push is ordinary fast-forward only. No force push, reset, rebase, or
  blanket staging is permitted.
- Release PR merge is blocked on current head SHA, exact `0.60.31` version
  agreement, valid changelog range, Cargo.lock parity, and successful checks.
- The build workflow receives an immutable commit SHA. It may create the tag
  when the draft Release exists before the tag is fetchable, then verifies the
  tag resolves to the expected SHA before downstream checkout.
- Main-worktree user files are never stashed, staged, committed, or used as
  release inputs.

## Failure And Retry

- Push rejection: fetch `origin`, stop, and reassess ancestry before any retry.
- Main CI/dev-build failure: inspect logs and fix only a verified release
  blocker in a separately validated commit before continuing.
- Release PR range/version failure: do not merge; close/delete the bad release
  branch and recreate it.
- Release workflow failure before publication with unchanged release SHA: keep
  the draft Release and tag, fix only external/transient state, and rerun using
  the same tag and SHA. If a source fix changes the release SHA, stop: the
  existing Release/tag path ignores a new `target_commitish`, so retargeting or
  replacement needs a separate explicit decision.
- Published Release validation failure: do not delete or rewrite it silently;
  stop and report concrete asset/tag evidence.

## Verification Evidence

Record GitHub run IDs, PR number/head SHA, release commit, tag SHA, Release ID,
asset inventory/digests, Homebrew skip/result, and protected-worktree comparison
in task-local research before archival.
