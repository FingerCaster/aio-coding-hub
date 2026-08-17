# Build Artifact

## Local Windows x64 MSI

- Source branch: `FingerCaster/codex-custom-model-context-rules`
- Product name: `AIO Coding Hub`
- Product version: `0.60.40`
- MSI summary template: `x64;0`
- Absolute path: `D:\\OrcaProjects\\aio-coding-hub-fork\\codex-custom-model-context-rules\\src-tauri\\target\\x86_64-pc-windows-msvc\\release\\bundle\\msi\\AIO Coding Hub_0.60.40_x64_en-US.msi`
- Size: `17776640` bytes
- SHA-256: `DD7E466AB4F6C3C9C5F33473C1DF3E0CECFDD7022DAFCB7043E60AAB257334D9`
- Built at (UTC): `2026-08-17T08:52:34.7000000Z`
- Local updater signing: disabled because `TAURI_SIGNING_PRIVATE_KEY` was not set; the release workflow owns official signed Beta assets.

The MSI metadata was read from the Windows Installer Property table and Summary Information after the build. The build output remains ignored by Git.

## Origin Integration

- Pull request: `https://github.com/FingerCaster/aio-coding-hub/pull/44`
- Immutable squash merge SHA: `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`
- Exact-head CI run: `32022192089` (success)
- Exact-head Windows dev-build run: `32022187319` (success)

## Official Beta Release

- Tag: `aio-coding-hub-v0.60.41-beta.10`
- Release: `https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v0.60.41-beta.10`
- Release workflow: `https://github.com/FingerCaster/aio-coding-hub/actions/runs/32024411558` (success)
- Release ID: `371702027`
- Source/tag/origin-main SHA: `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`
- State: public, `draft=false`, `prerelease=true`, stable latest unchanged at `aio-coding-hub-v0.60.40`
- Assets: exact 14-file official matrix, all non-empty and carrying GitHub SHA-256 digests

### Official Windows x64 MSI

- Download: `https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41-beta.10/aio-coding-hub-win64.msi`
- Verified local path: `D:\\OrcaProjects\\aio-coding-hub-fork\\codex-custom-model-context-rules\\src-tauri\\target\\release-verification\\aio-coding-hub-v0.60.41-beta.10\\aio-coding-hub-win64.msi`
- Size: `17797120` bytes
- SHA-256: `084F6311F9B233EC2DEF51A0B3ED3CAEE25A7D9077E831395F3108D55A097E39`
- MSI metadata: product `AIO Coding Hub`, product version `0.60.41.10`, summary template `x64;0`
- Updater signature SHA-256: `9C771A524F38933F55910D87A99A1208A2780F82B2FC0ED3FB87B78730593654`
- The downloaded signature matches the `windows-x86_64` manifest entry and the configured updater public-key ID.

### Update Channel

- Previous `release-channels` head: `4ba6d61832956a39bf9ab7d0dde91e9b2607807b`
- Published `release-channels` head: `58f248ff0fd7a5798907615c2133d96de53a99d0`
- Promotion high-water: `0.60.41-beta.10`; previous selected tag: `aio-coding-hub-v0.60.41-beta.9`
- `latest.json` / `latest-beta.json` SHA-256: `B34629B058A1820535FABA33E05BCBF7B25351FCB5FECF17A57F0EE6CB4CD143`
- Manifest platforms: `windows-x86_64`, `darwin-x86_64`, `darwin-aarch64`, `linux-x86_64`; every signature exactly matches its release `.sig` asset.
- Homebrew publication job was skipped for Beta. Before and after release, no override variable or accessible default tap (`FingerCaster/homebrew-aio-coding-hub`, HTTP 404) existed, so Homebrew state did not move.
