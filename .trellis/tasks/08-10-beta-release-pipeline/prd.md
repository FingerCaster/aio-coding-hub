# Beta 发布流水线与频道指针

## Goal

在不改变稳定发布默认行为的前提下，增加可审计的手动 Beta Release、完整签名资产、确定性版本 overlay、公开 Beta manifest 和可暂停的频道指针。

## Dependencies

- Parent contract: 08-10-beta-release-channel R4-R6、R8。
- No implementation dependency on the other children.
- This child must publish the exact endpoint, Tauri manifest shape, channel-state schema, Release URL rule and candidate attestation consumed by beta-updater-core. beta-updater-core cannot invent a second parser or endpoint.

## Requirements

- P1: Accept only explicit manual Beta tag plus full target SHA; resolve or create the immutable tag ref before any Release/build checkout, then validate tag peel and origin/main ancestry before build and again before publication. Every downstream checkout receives the SHA, never the tag alone.
- P2: Build the official support matrix with the same signing and no-overwrite guarantees as stable. Apply a deterministic overlay to package.json, Cargo.toml, Cargo.lock and tauri.conf.json; emit identical cross-platform attestation.
- P3: Parameterize candidate/promotion assertions for stable versus prerelease while preserving the current stable default, exact 14-asset matrix and Homebrew isolation.
- P4: Publish Beta as public prerelease with make_latest=false and advance a release-channels branch pointer atomically only after Release and assets are independently verified.
- P5: Provide a manual pause operation that points to an independently verified safe Release without deleting or overwriting the withdrawn Release.

## Acceptance Criteria

- [ ] Stable no-input dispatch and all existing release selftests remain green.
- [ ] Invalid, non-main, non-40-hex, source-drifting, mismatched existing tag, already-public Release or non-empty draft targets fail before build/promotion; an exact same-SHA retry is allowed only while the matching draft remains empty and otherwise preserves no-overwrite.
- [ ] Every platform attestation has the same source SHA, tag, version and overlay digest; mismatch stops assembly.
- [ ] Beta Release is draft/prerelease validated through candidate, then public prerelease and not latest; Homebrew jobs are skipped.
- [ ] Pointer promote/pause uses expected ref SHA and force=false; a race cannot overwrite the other commit.
- [ ] Pointer state records old/new ref, selected Release identity, manifest digest and workflow identity; unsafe or incomplete targets are rejected.
