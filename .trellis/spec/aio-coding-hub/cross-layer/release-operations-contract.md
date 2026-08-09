# Release Operations Contract

This contract owns stable version selection, release-please PR validation,
immutable source resolution, candidate promotion, and publication for the
`FingerCaster/aio-coding-hub` origin repository.

## 1. Scope / Trigger

Apply this contract when changing or running any of the following:

- `release-please-config.json`, `.release-please-manifest.json`, or a
  `Release-As:` override.
- `.github/workflows/release.yml`, release signing, support-matrix assets, or
  Homebrew publication.
- A stable release PR, tag, draft GitHub Release, or release workflow dispatch.

Normal release work operates only on `origin`. A separate user decision is
required before inspecting or mutating `upstream`.

## 2. Signatures

The normal release-please entry point has no workflow inputs:

```powershell
gh workflow run release.yml -R FingerCaster/aio-coding-hub --ref main
```

The manual path accepts an existing or new stable tag and an optional immutable
target:

```yaml
workflow_dispatch:
  inputs:
    release_tag:       # optional aio-coding-hub-vMAJOR.MINOR.PATCH
    target_commitish:  # optional 40-hex commit SHA for a new manual tag
```

The release job exports the identity consumed by every downstream job:

```text
release_created: "true" | "false"
tag_name: aio-coding-hub-vMAJOR.MINOR.PATCH
release_id: positive GitHub release ID
checkout_ref: 40-hex commit SHA
build_matrix: validated support-matrix JSON
```

When historical conventional commits contain an obsolete `Release-As:` value,
select the intended version with an empty commit and an empty index:

```text
chore(release): prepare aio-coding-hub 0.60.40

Release-As: 0.60.40
```

## 3. Contracts

- Verify `origin/main`, the local release source, and the current manifest
  before selecting a version. Never infer safety from the PR title alone.
- An override commit contains no file changes. Abort if the index is non-empty,
  and derive the hook-visible `node` and `pnpm` directories from the current
  shell rather than a machine-specific path.
- Stable release-please publication is a two-dispatch flow. The first
  no-input dispatch creates or refreshes the release PR and must not build when
  `release_created` is false. Merge the PR only after its final head passes CI.
  The second no-input dispatch runs from the merged version commit and creates
  the tag, draft, builds, candidate, promotion, and publication.
- The release PR must update exactly the expected release files:
  `.release-please-manifest.json`, `CHANGELOG.md`, `package.json`,
  `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
  `src-tauri/tauri.conf.json`. All six must contain the same intended version;
  wait for the Cargo-lock synchronization commit before final review.
- Resolve or create the release tag before any build and convert it to a
  40-hex commit SHA. Every build, candidate manifest, promotion check, and
  publication check consumes that SHA, never a release tag alone.
- Promotion accepts only an empty, non-prerelease draft with the expected
  release ID, tag, source SHA, run ID, run attempt, exact asset names, sizes,
  and SHA-256 digests. Assets are uploaded without overwrite and reverified
  before publication.
- `latest.json` must name the intended version and contain non-empty URLs and
  signatures for `windows-x86_64`, `darwin-x86_64`, `darwin-aarch64`, and
  `linux-x86_64`.
- A missing Homebrew token may skip tap synchronization only through the
  workflow's explicit skip branch; Cask generation must still succeed.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Override commit has staged content | Abort; do not create the commit |
| Remote `main` moved before push | Fetch and re-audit; never force-push |
| PR title is correct but any version file differs | Do not merge |
| Cargo.lock still has the prior version | Wait for sync and review the new PR head |
| Required PR check is pending, failed, or belongs to an older head | Do not merge |
| Local check is blocked by an environment defect | Record it and require the equivalent clean CI check to pass before merge |
| Draft tag is not yet fetchable | Resolve/create the tag, then pass its commit SHA downstream |
| `checkout_ref` is not 40-hex or differs from the tag SHA | Fail before build/promotion |
| Candidate asset is missing, extra, changed, or lacks its digest | Fail promotion; do not partially publish |
| Target Release is published, prerelease, non-empty, or has another identity | Fail closed; do not overwrite assets |
| Homebrew token is absent | Generate the Cask and explicitly skip tap sync |

## 5. Good / Base / Bad Cases

- Good: an empty `Release-As: 0.60.40` commit refreshes an obsolete release PR;
  six files agree on `0.60.40`, CI passes on the final PR head, and the merged
  commit becomes the tag, Release target, candidate source, and `origin/main`.
- Base: the first no-input dispatch updates a PR, returns
  `release_created=false`, and all build/publish jobs are skipped.
- Bad: merge because the PR title says `0.60.40` while the manifest or
  Cargo.lock still says another version.
- Bad: checkout a draft release by tag in build jobs before proving that the
  tag is fetchable and resolving it to a commit SHA.

## 6. Tests Required

- Run `scripts/release-source.selftest.mjs`,
  `scripts/release-promotion.selftest.mjs`, the signing-scope self-test and
  contract, support-matrix validation, Homebrew Cask self-test, CI scope
  contracts, and `git diff --check` before the override push.
- Review the final release PR head, all six version files, and changelog range;
  require `ci-gate`, frontend, Rust, generated bindings, Cargo-lock sync, and
  Windows build success on that exact head.
- The release workflow must assert tag-to-SHA continuity before build, before
  promotion, and before publication, then verify the exact immutable candidate
  after download and after upload.
- After publication, independently assert the workflow conclusion, tag SHA,
  `origin/main`, Release target/state, asset matrix/digests, and every
  `latest.json` platform URL/signature.

## 7. Wrong vs Correct

### Wrong

```yaml
- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683
  with:
    ref: ${{ needs.release-please.outputs.tag_name }}
```

A draft Release can exist before its tag is fetchable, and a mutable tag name
does not prove which source was built.

### Correct

```yaml
- name: Resolve release source
  run: git fetch --force --no-tags origin "refs/tags/$RELEASE_TAG"

- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683
  with:
    ref: ${{ needs.release-please.outputs.checkout_ref }} # verified 40-hex SHA
```

### Wrong

```text
PR title is 0.60.40, so merge it.
```

### Correct

```text
Read the final PR head and prove manifest, changelog, package, Cargo.toml,
Cargo.lock, and tauri.conf.json all select 0.60.40 before merging.
```
