# Local Windows MSI Build Result

## Artifact

- Source: `fab7a968146fe1210049353ba171ef3ca999ecd4` in a clean detached
  worktree.
- Path:
  `D:/UGit/aio-coding-hub-fork-msi-main-20260729/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/AIO Coding Hub_0.60.30_x64_en-US.msi`.
- Size: `16,420,864` bytes.
- SHA-256:
  `546419cadc464bc890780c14c34e126e15a54252b2a88aeb88e50a1d3dac5a5f`.
- Authenticode status: `NotSigned`, as expected for the local unsigned build.

## Commands

- `pnpm install --frozen-lockfile`: exit 0, 6.2 seconds.
- `pnpm tauri:build:win:x64`: exit 0, 522.3 seconds.
- Resolved Tauri arguments: `build -c .local/tauri.build.local.json
  --bundles msi --target x86_64-pc-windows-msvc`.
- `pnpm typecheck`: passed independently.
- `pnpm lint`: passed independently.

## Windows Installer Metadata

- ProductName: `AIO Coding Hub`.
- ProductVersion: `0.60.30`.
- ProductCode: `{C8CA11A3-572D-4A7B-8B42-BB4E2F6398D4}`.
- Manufacturer: `aio`.
- ProductLanguage: `1033`.

The Windows Installer COM API opened the package and returned every property
above successfully. The bundle directory contained exactly one non-empty MSI.

## Safety And Preservation

- The detached build worktree remained clean after generated/ignored build
  output was excluded by Git.
- The main worktree retained all 28 protected status entries and zero staged
  paths.
- `upstream` push remained `DISABLED`.
- No fetch, push, upload, publish, tag, release, signing, or remote mutation was
  performed.
- The local overlay set `bundle.createUpdaterArtifacts` to `false`.

## Non-Blocking Warnings

- Node emitted `DEP0190` for shell-spawn argument handling.
- Browserslist reported data approximately seven months old.
- Vite reported a generated chunk larger than 500 kB.

All warnings were informational and did not affect the build, MSI database, or
independent verification.
