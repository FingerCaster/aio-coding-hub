# Publish merged upstream release

## Goal

Publish the validated upstream integration as `aio-coding-hub-v0.60.31` from
`origin/main`, with a complete signed multi-platform GitHub Release and the
Homebrew workflow reaching its configured terminal path.

## Confirmed Facts

- Latest published release: `aio-coding-hub-v0.60.30`, tag and Release target
  `1a551cbee35960fbb954e475a13b2d8d55d709df`.
- Local `main@d8006da9d4315bd6a59000f04a469ba98e358c3a` is 18 commits ahead of
  `origin/main`, zero behind, and is a fast-forward push.
- All canonical version sources remain `0.60.30`; the natural patch release is
  `0.60.31`.
- The merged source passed the full precommit/prepush gates, focused reviews,
  and a user-tested local Windows MSI build.
- Open PR #17 and its release-please branch incorrectly downgrade all version
  sources to `0.60.6` and contain changelog entries outside the current release
  range. The repository checker prescribes closing the stale PR, deleting the
  branch, and rerunning the release workflow.
- `.github/workflows/release.yml` is manual-only and supports an explicit
  release tag plus immutable target commit SHA.
- The repository has no `HOMEBREW_TAP_TOKEN`, and the configured fallback tap
  repository is absent or inaccessible. The workflow intentionally skips Cask
  synchronization in this state while keeping the release job successful.

## Requirements

- Operate only on `origin` and GitHub repository
  `FingerCaster/aio-coding-hub`; do not inspect or mutate upstream.
- Preserve all 28 pre-existing main-worktree status entries, zero staged paths,
  and the retained protection stash.
- Close stale PR #17 and delete its stale release-please branch before creating
  the new release PR.
- Fast-forward push the already validated local `main`, require its matching
  `ci` workflow to succeed, then explicitly dispatch `dev-build.yml` on `main`
  and require the run's `headSha` to equal the pushed commit before accepting
  its success. The dev-build push trigger intentionally ignores `main`.
- Generate a fresh release-please PR for `0.60.31`; verify its changelog range,
  version files, Cargo lock synchronization, diff scope, and required checks
  before merging it into `main`.
- Require the release commit's main-branch CI plus an explicitly dispatched
  matching-SHA dev-build to succeed before starting the asset build.
- Dispatch the release workflow with tag `aio-coding-hub-v0.60.31` and the
  release commit's full SHA as `target_commitish`.
- Monitor the workflow through release-ref resolution, all four platform build
  matrix entries, `latest.json`, Release publication, and the configured
  Homebrew terminal path. With current repository state, require explicit log
  evidence that Cask sync was safely skipped because the token is absent.
- Verify the tag resolves to the release commit, the Release is published and
  latest, and every expected asset is uploaded with a digest.

## Acceptance Criteria

- [x] `origin/main` contains the validated upstream merge and release commit,
      with local/remote ancestry verified and no force push.
- [x] The fresh release PR is `0.60.31`, contains only valid release metadata,
      passes all checks, and is merged.
- [x] Tag `aio-coding-hub-v0.60.31` resolves exactly to the immutable release
      commit SHA.
- [x] The release workflow completes successfully, including all four build
      targets, latest updater metadata, publication, and the Homebrew job's
      explicit configured skip path.
- [x] The GitHub Release is non-draft, non-prerelease, marked latest, and all
      expected assets are present, non-empty, and have SHA-256 digests.
- [x] The main worktree's protected user state is unchanged and no upstream
      operation occurred.

## Out Of Scope

- Product/source fixes unrelated to a concrete release blocker, version changes
  other than `0.60.31`, prereleases, upstream mutation, or deleting a published
  Release.
- Creating a Homebrew tap repository or adding/persisting a new tap credential.
