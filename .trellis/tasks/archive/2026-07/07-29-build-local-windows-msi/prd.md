# Build local Windows MSI

## Goal

Produce a locally installable Windows x64 MSI from the exact merged `main`
revision so the user can test the upstream integration without including any
pre-existing dirty working-tree state.

## Confirmed Facts

- The source revision is `main@fab7a968146fe1210049353ba171ef3ca999ecd4`.
- The supported Windows release target is `x86_64-pc-windows-msvc` with an MSI
  bundle.
- The application and Tauri bundle version are both `0.60.30`.
- `scripts/tauri-build.mjs` disables updater artifacts for an unsigned local
  build and defaults the Windows x64 target to `--bundles msi`.
- The main worktree contains protected user changes, so it is not a valid build
  source.

## Requirements

- Build in a clean detached worktree pinned to the source revision.
- Install dependencies with the frozen lockfile and use the repository's
  canonical Windows x64 Tauri build command.
- Do not sign, publish, upload, release, or modify any remote state.
- Leave the main worktree's existing modified, deleted, untracked, and staged
  state unchanged.
- Report the final MSI path, byte size, SHA-256, and Windows Installer product
  metadata.

## Acceptance Criteria

- [x] The build command exits successfully and produces exactly one MSI for
      AIO Coding Hub `0.60.30` under the Windows x64 release bundle directory.
- [x] Windows Installer can open the package database and reports the expected
      product name and version.
- [x] The MSI has a recorded non-zero size and SHA-256 digest.
- [x] The source worktree is pinned to the exact `main` SHA and remains clean
      apart from ignored/generated build output.
- [x] The main worktree retains its protected pre-build Git status and no
      remote operation is performed.

## Out Of Scope

- Code changes, version bumps, signing, updater artifacts, portable ZIPs,
  GitHub Releases, and remote pushes.
