# Implementation Plan: Codex GPT-5.6 372K context

## 1. Contracts and persistence

- [x] Add the exact Rust policy constant `372_000` and canonical slug allowlist.
- [x] Add the default-false settings field, schema 64 migration, read-only view projection, generated bindings, and ownership/serialization tests.
- [x] Add a dedicated toggle command/service path; exclude the field from ordinary settings full-write and patch ownership.
- [x] Guard Codex home changes while the policy is enabled.

## 2. Catalog lifecycle

- [x] Generalize managed catalog preparation so profiles and the 372K policy independently request a derived catalog, including proxy-off operation.
- [x] Preserve the complete base catalog; require all three target slugs and write both window fields to 372000 only when enabled.
- [x] Extend metadata/hash with policy and original binding data while retaining snapshot validation and byte-stable regeneration.
- [x] Preserve or restore `model_catalog_json` correctly for bundled/user bases, zero/nonzero profiles, proxy enable/disable, and raw/structured Codex config saves.
- [x] Reuse the existing atomic apply/conditional rollback machinery for settings, proxy backup, generated catalog, and live config.
- [x] Wire startup and existing profile/provider/proxy lifecycle synchronization to persisted intent.

## 3. Frontend

- [x] Add typed service/query support for the dedicated toggle command and invalidate/reload affected settings, Codex config, and model catalog state.
- [x] Add the Switch to the Codex settings surface using existing compact settings-row patterns.
- [x] Show exact `372,000` token semantics and a concise “new Codex sessions” status note without instructional clutter.
- [x] Disable duplicate saves and Codex home changes in the relevant UI state; preserve confirmed state on failures.

## 4. Tests

- [x] Rust catalog tests: exact three-slug rewrite, `380928` negative case, missing target failure, unknown-field preservation, `aio/*` isolation, stable metadata/hash.
- [x] Rust lifecycle tests: proxy on/off, zero/nonzero profiles, bundled/user source restoration, CLI fingerprint rebuild, startup sync, drift and every rollback stage.
- [x] Settings tests: schema 63 to 64 migration, default false, ordinary writer non-ownership, dedicated command success/failure, conditional rollback, Codex home guard.
- [x] Frontend tests: default/off/on, pending/error states, exact display value, query invalidation and home-control disabling.
- [x] Isolate Linux managed-profile fixtures from host Codex availability and preserve the active catalog binding in the unsafe-home replacement test.
- [x] Run focused tests after each layer, then full relevant quality gates.

## 5. Verification and packaging

- [x] `pnpm typecheck`
- [x] `pnpm lint`
- [x] `pnpm test -- --run` or the repository's scoped Vitest commands for touched suites.
- [x] `pnpm tauri:fmt`
- [x] `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] Scoped Rust tests for settings, Codex config/catalog, profiles and proxy lifecycle.
- [x] `pnpm check:generated-bindings`
- [x] `pnpm tauri:build:win:x64`
- [x] Record MSI absolute path, byte size and SHA-256; inspect final git diff and commit only task changes.
  - `D:\\OrcaProjects\\aio-coding-hub-fork\\codex-gpt56-372k-context\\src-tauri\\target\\x86_64-pc-windows-msvc\\release\\bundle\\msi\\AIO Coding Hub_0.60.40_x64_en-US.msi`
  - `17,711,104` bytes; SHA-256 `accce68c570ceb3216f34f0acf59d76a212b9e23391a0caa40f8ac5343c32ea8`.

## 6. Origin integration and Beta release

- [x] Re-read `origin/main`, inspect branch drift, and preserve non-conflicting origin changes without touching `upstream`.
- [x] Commit the initial task changes with hook-visible `node` and `pnpm`, push the feature branch to `origin`, and create PR #42 against `main`.
- [ ] Push the Linux fixture follow-up and wait for required checks on the exact final head.
- [ ] Review and merge the PR, then record the immutable 40-hex merge SHA and confirm it is reachable from `origin/main`.
- [ ] Re-read the Beta promotion high-water and confirm the selected tag/Release do not exist; expected candidate is `aio-coding-hub-v0.60.41-beta.9`.
- [ ] Dispatch `release.yml` from `main` with `release_channel=beta`, the selected tag, and the exact merge SHA; monitor every job to terminal success.
- [ ] Verify the public prerelease flags, exact 14-asset matrix, signatures, four-platform `latest.json`, source/tag identity, and `release-channels` manifest/state CAS. Confirm stable latest and Homebrew are unchanged.

## Risk and Rollback Points

- The highest-risk code is `managed.rs` binding/source recovery. Stop and fix any test that shows loss of a user catalog path or unknown field.
- Settings/catalog commit ordering must be failure-injected before UI work is considered complete.
- Generated bindings may be regenerated only after Rust DTO/command signatures stabilize.
- Packaging starts only after clean focused and cross-layer checks; build artifacts are not committed unless already required by repository policy.
- Publishing starts only after the PR merge SHA passes required CI. A failed release is investigated in place; never reuse a tag for a different SHA or blindly re-dispatch after an ambiguous timeout.
