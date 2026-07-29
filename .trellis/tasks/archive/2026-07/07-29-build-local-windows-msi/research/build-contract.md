# Local Windows MSI Build Contract

## Source

- Build exact commit `fab7a968146fe1210049353ba171ef3ca999ecd4`.
- Use a clean detached worktree outside the dirty main worktree.
- Confirm `package.json` and `src-tauri/tauri.conf.json` both report version
  `0.60.30` before building.

## Canonical Build

```powershell
pnpm install --frozen-lockfile
pnpm tauri:build:win:x64
```

The repository wrapper invokes Tauri for target `x86_64-pc-windows-msvc`,
defaults the target to `--bundles msi`, and locally disables updater artifact
generation when `TAURI_SIGNING_PRIVATE_KEY` is absent.

Expected bundle directory:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi
```

## Validation

- Require both build commands to exit successfully.
- Require exactly one non-empty `.msi` in the expected bundle directory.
- Open the MSI through the Windows Installer COM API and read at least
  `ProductName`, `ProductVersion`, `ProductCode`, and `Manufacturer`.
- Record byte size and SHA-256.
- Confirm the detached worktree source SHA and the protected main worktree Git
  state after the build.

## Safety

- Do not set or use updater signing secrets.
- Do not push, upload, publish, tag, create a release, or change remote URLs.
- Do not build from or modify the dirty main worktree beyond Trellis-managed
  task bookkeeping.
