# Release State Before 0.60.31

## Git And Version

- Local main: `d8006da9d4315bd6a59000f04a469ba98e358c3a`.
- Origin main: `1a551cbee35960fbb954e475a13b2d8d55d709df`.
- Ahead/behind (`origin/main...HEAD`): `0 / 18`.
- Manifest, package, Cargo package/lock root, and Tauri versions: `0.60.30`.
- Full validation and user-tested local MSI evidence are archived in the two
  preceding `07-29` tasks.

## Published Baseline

- Latest release/tag: `aio-coding-hub-v0.60.30`.
- Release ID: `357476007`.
- Release and lightweight tag target:
  `1a551cbee35960fbb954e475a13b2d8d55d709df`.
- State: published, non-prerelease, latest, with complete updater/install assets.

## Stale Release PR

- PR #17:
  `https://github.com/FingerCaster/aio-coding-hub/pull/17`.
- Head branch:
  `release-please--branches--main--components--aio-coding-hub`.
- Head SHA: `5772f3639841238a28822796a4a5c9f74f2b7ead`.
- It changes all canonical release versions from `0.60.30` to `0.60.6` and
  includes historical changelog entries outside the new release range.
- `scripts/check-release-pr-changelog.mjs` explicitly instructs closing the
  stale PR, deleting this branch, and rerunning the release workflow.

## Workflow Contract

- `release.yml` is `workflow_dispatch` only.
- `dev-build.yml` supports `workflow_dispatch` but its push trigger ignores
  `main`; main-branch dev-build gates must be explicitly dispatched and matched
  to the intended commit SHA.
- Dispatch without `release_tag` runs release-please and creates/updates a
  release PR.
- Dispatch with `release_tag` uses/creates a draft release and accepts an
  explicit `target_commitish`; the workflow resolves or creates the tag and
  passes a verified immutable commit SHA to all build jobs.
- Matrix: Windows x64 MSI, macOS Intel app, macOS ARM app, Linux x64
  DEB/AppImage.
- Terminal jobs: `assemble-latest-json`, `publish`, and
  `publish-homebrew-cask`.
- Repository secrets currently contain only `RELEASE_PLEASE_TOKEN` and the two
  Tauri signing secrets. `HOMEBREW_TAP_TOKEN` is absent, and
  `FingerCaster/homebrew-aio-coding-hub` is absent or inaccessible. The
  Homebrew job must therefore take its explicit safe-skip path; creating a repo
  or new credential is outside this task.

## Completed Integration And Release PR

- Validated main source pushed without force:
  `d8006da9d4315bd6a59000f04a469ba98e358c3a`.
- Matching main CI: run `30455868362`, success.
- Matching explicit dev-build: run `30457923540`, success; artifact
  `dev-build-win64`, 16,504,421 bytes.
- Stale PR #17 was closed without merge and its release-please branch deleted.
- Replacement PR #18 used stable head
  `2bdaebcf5df95487b98d54a9285658183c8ab2f0` and changed only the manifest,
  changelog, package manifest, Cargo manifest/lock, and Tauri config.
- Manifest, package, Cargo package/lock root, and Tauri versions all resolved
  to `0.60.31`. The Tauri config was semantically identical except for the
  version; its other textual changes were generator formatting.
- `scripts/check-release-pr-changelog.mjs` accepted all seven referenced
  commits as members of `aio-coding-hub-v0.60.30..d8006da9`.
- PR #18 passed all nine checks and was squash-merged as
  `07e5455be3490053b172bd0277a7a03ca416ed07`. Its single parent is `d8006da9`,
  and its tree `1c12293dbd1254362c07922231db284a13f5f64c` exactly matches the reviewed
  PR head tree.

## Release Commit Gates

- Release-commit CI: run `30461826502`, success at `07e5455b`.
- Release-commit explicit dev-build: run `30462952549`, success at the same
  SHA. Artifact `dev-build-win64` is 16,506,333 bytes with digest
  `sha256:aa09692604595b08586f8fbb9b0a26162270df68ba561be5626bc7d1c6571297`.
- Local `main`, local `origin/main`, GitHub `main`, the release target, and the
  lightweight release tag all resolve to
  `07e5455be3490053b172bd0277a7a03ca416ed07`.

## Published Release

- Release workflow: run `30464721974`, success, dispatched exactly once with
  tag `aio-coding-hub-v0.60.31` and immutable target `07e5455b`.
- Release: ID `361849244`, published, non-prerelease, returned by the GitHub
  latest-release endpoint.
- Jobs: release-ref resolution, Linux x64, macOS Intel, macOS ARM, Windows x64,
  `assemble-latest-json`, `publish`, and `publish-homebrew-cask` all succeeded.
- Homebrew Cask generation succeeded. With no token, the workflow logged
  `HOMEBREW_TAP_TOKEN is not configured; generated Cask will not be pushed.`
  The actual tap-sync step was skipped as designed.
- `latest.json` is 4,132 bytes with digest
  `sha256:c32c52e619a40067064261a634e478c226008825f49548b56e78b2b4d76c0f76`.
  It declares version `0.60.31`, has non-empty signatures, and maps exactly
  `windows-x86_64`, `darwin-x86_64`, `darwin-aarch64`, and `linux-x86_64` to
  stable assets under the new tag.

## Asset Inventory

Every asset is non-empty, in `uploaded` state, and has a GitHub-provided
SHA-256 digest. The 24-name set exactly matches the prior release after the
expected `0.60.30` to `0.60.31` substitution.

| Asset | Bytes | SHA-256 |
| --- | ---: | --- |
| `aio-coding-hub-linux-amd64-wayland.AppImage` | 92,400,120 | `9ba36073628338afa3ad04685b4aaa7e99a85fede3c719370881ddd71401fd71` |
| `aio-coding-hub-linux-amd64.AppImage` | 92,400,120 | `e61796aae673ea70463824b9a18e5b113ac4e8c40978372b3c958682c6deaa50` |
| `aio-coding-hub-linux-amd64.AppImage.sig` | 432 | `6d96c9e3686ba18c87414ec98263156af709b12694d66c9d458ce4f573744f50` |
| `aio-coding-hub-linux-amd64.deb` | 15,817,826 | `10eb417e0eadfe683add1b00f1473648d95d0669961c2ab246c93f165f1b5def` |
| `aio-coding-hub-macos-arm.tar.gz` | 16,123,478 | `a99541bbce7995b60f18de191cd5222b1a30da3435863e1c7d1d43886124510d` |
| `aio-coding-hub-macos-arm.tar.gz.sig` | 416 | `f84687d703a32bf67c192daf1d25d895282418a6add96a422571b860535e4069` |
| `aio-coding-hub-macos-arm.zip` | 15,658,176 | `a3e8722f65b9573b519475f0e722c38c25ba15582715be1788495b17cf8c7faa` |
| `aio-coding-hub-macos-intel.tar.gz` | 16,697,384 | `ca79333471fa963bb2d53bf61a62e4d8f4de5f19a90de4ca32ad002829676146` |
| `aio-coding-hub-macos-intel.tar.gz.sig` | 416 | `469e1ce53158cfe0cc5a6b0d58f7fa90084eff4393c248406841682cbf967602` |
| `aio-coding-hub-macos-intel.zip` | 16,394,960 | `3ce28c9a0d73918815617b9044801f339aa0eb2da23e54fea0f36d2822a91259` |
| `aio-coding-hub-win64-portable.zip` | 16,633,230 | `8ec8d522df9de787ee04a86e967b16b2db0b85345be388d544ca5a961f946b52` |
| `aio-coding-hub-win64.msi` | 16,445,440 | `d487b1a2a1aff5f0c487e59ab0c1519522114b51d0104308282d3907acd49075` |
| `aio-coding-hub-win64.msi.sig` | 428 | `bce636337310c1496e47e07f4628a53f3522a46bf1a55cc19ac3661bda370c42` |
| `AIO.Coding.Hub_0.60.31_amd64.AppImage` | 92,400,120 | `e61796aae673ea70463824b9a18e5b113ac4e8c40978372b3c958682c6deaa50` |
| `AIO.Coding.Hub_0.60.31_amd64.AppImage.sig` | 432 | `6d96c9e3686ba18c87414ec98263156af709b12694d66c9d458ce4f573744f50` |
| `AIO.Coding.Hub_0.60.31_amd64.deb` | 15,817,826 | `10eb417e0eadfe683add1b00f1473648d95d0669961c2ab246c93f165f1b5def` |
| `AIO.Coding.Hub_0.60.31_amd64.deb.sig` | 424 | `97b168f38e5eb48d4be829e9d7a57ff3b2f8f343ba6d063859f886570da6e9e1` |
| `AIO.Coding.Hub_0.60.31_x64_en-US.msi` | 16,445,440 | `d487b1a2a1aff5f0c487e59ab0c1519522114b51d0104308282d3907acd49075` |
| `AIO.Coding.Hub_0.60.31_x64_en-US.msi.sig` | 428 | `bce636337310c1496e47e07f4628a53f3522a46bf1a55cc19ac3661bda370c42` |
| `AIO.Coding.Hub_aarch64.app.tar.gz` | 16,123,478 | `a99541bbce7995b60f18de191cd5222b1a30da3435863e1c7d1d43886124510d` |
| `AIO.Coding.Hub_aarch64.app.tar.gz.sig` | 416 | `f84687d703a32bf67c192daf1d25d895282418a6add96a422571b860535e4069` |
| `AIO.Coding.Hub_x64.app.tar.gz` | 16,697,384 | `ca79333471fa963bb2d53bf61a62e4d8f4de5f19a90de4ca32ad002829676146` |
| `AIO.Coding.Hub_x64.app.tar.gz.sig` | 416 | `469e1ce53158cfe0cc5a6b0d58f7fa90084eff4393c248406841682cbf967602` |
| `latest.json` | 4,132 | `c32c52e619a40067064261a634e478c226008825f49548b56e78b2b4d76c0f76` |

## Final Worktree Audit

- The retained protection stash is still
  `426522f5f3860b961066f0962a266d01ccf91e45`.
- Comparing directly against the stash tree and untracked parent proves the 14
  tracked protected paths and all 14 original untracked blobs are unchanged.
- Before task archival, status is exactly 35 entries: the original protected
  28 plus this task's seven files. The index has zero staged paths.
- Final independent Trellis review reported zero findings at every severity.
- No code-spec update was required because this integration-only publication
  changed no code, API, data, environment, or infrastructure contract.
