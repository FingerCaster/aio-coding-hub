# Implementation Plan: Publish 0.60.31

## 1. Preflight

- [x] Reconfirm local/remote ancestry, GitHub authentication/default repo,
      latest Release/tag, canonical versions, protected worktree state, and
      release workflow support matrix.
- [x] Verify the current task artifacts and curated sub-agent contexts.
- [x] Independently audit the release plan for unsafe mutations or missing
      stop conditions.

## 2. Push Validated Main

- [x] Push local `HEAD` to `origin/main` with a normal fast-forward push.
- [x] Fetch origin and verify local/remote SHA equality.
- [x] Wait for matching `ci`; explicitly dispatch `dev-build.yml --ref main`,
      verify its `headSha` is the pushed SHA, and require both runs to succeed.

## 3. Replace Stale Release PR

- [x] Capture PR #17 head/diff evidence, close it, and delete its stale remote
      release-please branch.
- [x] Dispatch `release.yml` without release inputs to create a fresh PR.
- [x] Identify the new PR and wait for Cargo.lock synchronization to settle.
- [x] Verify `0.60.31` across manifest/package/Cargo/Tauri files, exact diff
      scope, changelog range, and complete successful PR checks.

## 4. Merge Release Commit

- [x] Merge the verified release PR using the repository's normal merge method.
- [x] Fetch origin and record the immutable release commit SHA.
- [x] Verify all canonical version sources and changelog at the release commit.
- [x] Wait for release-commit `ci`; explicitly dispatch `dev-build.yml` on
      `main`, verify the exact release commit `headSha`, and require success.

## 5. Build And Publish

- [x] Dispatch `release.yml` with tag `aio-coding-hub-v0.60.31` and the full
      release commit SHA as `target_commitish`.
- [x] Monitor every job until terminal; inspect logs for any failure.
- [x] Require the release workflow, all four platform matrix builds,
      `assemble-latest-json`, `publish`, and `publish-homebrew-cask` to pass;
      capture the current no-token Homebrew safe-skip log.

## 6. Final Audit

- [x] Verify tag SHA equals the release commit SHA.
- [x] Verify Release metadata and complete non-empty asset inventory/digests.
- [x] Verify `latest.json` targets `aio-coding-hub-v0.60.31` and covers the
      supported updater platforms.
- [x] Verify the Homebrew job reached its configured explicit skip path; do not
      claim a Cask update or create a missing tap repository.
- [x] Reconfirm `origin/main`, protected main-worktree state, retained stash,
      and absence of upstream operations.
- [x] Record completion evidence, run Trellis validation, archive the task, and
      push any final task/journal bookkeeping only after classifying it as
      non-release metadata.

## Rollback Points

- Before main push: no remote changes.
- Before release PR merge: close/delete only the generated release PR branch.
- Before release dispatch: no new tag or Release exists.
- After draft creation: retain evidence and retry only the failed workflow path;
  retry only with the same immutable SHA. A changed SHA requires stopping for a
  separate draft/tag decision; never retarget an existing Release silently.
