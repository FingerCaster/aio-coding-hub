# Validation Evidence

## Static CI Contract

- `pnpm run check:ci-change-scope`: passed. This runs the dependency-free
  classifier self-test and workflow structure/result-contract self-test.
- `node --check scripts/ci-change-scope.mjs` and
  `node --check scripts/ci-workflow-contract.selftest.mjs`: passed.
- The workflow self-test validates the exact change-scope outputs, every job's
  `needs`/`if`, the complete `ci-gate` dependency and environment wiring, the
  desktop matrix aggregate, and fail-closed selected/skipped result matrices.
  It also performs fault injection against scope output, docs/full conditions,
  gate `always()`, gate dependencies/result bindings, and required self-tests.
- YAML parsing with PyYAML: passed; the expected eight job IDs were present.
- `git diff --check`: passed.

## actionlint

- Version: `actionlint 1.7.12` (`go1.26.1`, linux/amd64).
- Image digest:
  `sha256:b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667`.
- Reproducible command:

  ```powershell
  docker run --rm -v "${PWD}:/repo" -w /repo rhysd/actionlint@sha256:b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667 -color .github/workflows/ci.yml
  ```

- Target `.github/workflows/ci.yml`: passed with exit code 0.
- Running actionlint over every workflow additionally reports the pre-existing
  `.github/workflows/release.yml:70` SC2129 finding. That unrelated workflow was
  not changed by this task and does not affect the target workflow result.

## Node And Frontend

- Checked-docs contracts passed: plugin system docs, plugin API contract, and
  Trellis spec links.
- Support contracts passed: support matrix, Homebrew Cask generator, upstream
  sync policy self-test, and manual-review policy enforcement.
- `pnpm install --frozen-lockfile`: passed with the lockfile unchanged.
- Full frontend job commands passed: dependency audit, ESLint, gateway error
  codes, plugin docs/API contracts, generated bindings, plugin SDK typecheck
  and 29 tests, scaffolder 30 tests, GUI E2E smoke, unit coverage gate, and
  production build.
- The desktop CI matrix resolves to Windows, macOS, and Linux. The Windows
  support contract passed locally; workflow structure and `ci-gate` self-tests
  assert the aggregated matrix job remains required for full scope.

## Rust

- Local toolchain: `rustc 1.95.0`, `cargo 1.95.0`, Windows MSVC. CI remains
  pinned to Rust 1.90.0 on Ubuntu 22.04; no workflow pin was weakened.
- `cargo fmt -- --check`: passed.
- `cargo update --workspace`: locked 0 packages; Cargo.lock stayed clean.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo test --locked -- --test-threads=1`: passed, including 2543 library
  tests and all integration test binaries.
- `cargo install cargo-audit --locked`: passed (`cargo-audit 0.22.2` already
  installed).
- `cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195`: passed
  with exit code 0 and 22 allowed warnings reported by the current baseline.

## Origin Compatibility

- `GET repos/FingerCaster/aio-coding-hub/branches/main/protection`: 404,
  `Branch not protected`.
- `GET repos/FingerCaster/aio-coding-hub/rulesets`: `[]`.
- Therefore origin currently has no required check contexts to preserve or
  migrate. The stable `ci-gate` is suitable as a future required context.
- No GitHub setting, remote URL, branch, PR, release, or Actions state was
  modified.

## Inline Review Fixes And Residual Risk

- Inline Trellis check corrected cross-tier changes to `full`, moved untrusted
  PR title data through an environment variable, and added the reproducible
  workflow structure/result self-test required when actionlint alone is
  insufficient.
- Linux/macOS execution remains delegated to GitHub-hosted runners; local
  verification covered Windows plus the exact workflow graph and matrix
  aggregation contract.
