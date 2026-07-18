# Round 7 Implementation Plan

## Preconditions

- [x] Confirm the branch is still based on `35db0f32` plus task-planning-only changes and preserve all unrelated
      dirty files listed in the parent PRD.
- [x] Confirm no Claude, Pi, or other review terminal is running; future review ownership is Sol max only.
- [x] Launch exactly one fresh Orca Codex `gpt-5.6-sol / effort=max` execution terminal with a closed execution
      prompt: follow this checklist only; do not explore unrelated code, hunt additional findings, refactor,
      compare alternate architectures, or expand scope. Stop and report if a checklist item cannot be completed
      without a scope decision.
- [x] Read the curated config-migration, Image Gen, settings rollback, task archive, and cross-layer thinking specs.

## Ordered Work

1. [x] Add Image Gen controller regressions for a successful retry of persisted done and error rows, a failed retry,
       reference rehydration, and an in-flight first persistence race. Make the tests assert distinct IDs and no
       overwrite/delete of the source row.
2. [x] Implement fresh immutable retry attempts in the frontend, correct upsert language, and update the Image Gen
       trust-boundary contract to match the new attempt lifecycle.
3. [x] Add Rust config-export tests for aggregate installed/local payload and file-count exhaustion, including a
       sentinel target file and a legal 1-8 MiB round trip.
4. [x] Implement the shared export aggregate budget before Base64 allocation, retaining all existing per-Skill and
       final bundle checks. Update the Skill bundle contract.
5. [x] Add the same-value `auto_start` ABA regression(s) through the real config-import runtime-failure path.
6. [x] Implement generation-gated whole rollback so stale imports cannot restore `auto_start`; update the settings
       ownership contract with the explicit ABA invariant.
7. [x] Audit archive evidence for child tasks 1-5 and truthfully mark completed PRD/implement entries. Run a narrow
       unchecked-marker audit plus `task.py validate --all`.
8. [x] Run focused frontend/Rust tests, then the required full quality gates and Linux/Docker watchdog where the
       repository requires it. Record failures and retries honestly.
9. [ ] Run `trellis-check`, update applicable specs, commit, archive this child, then freeze the commit for one
       fresh Sol max read-only final review.

## Validation

```powershell
pnpm exec vitest run src/pages/image-gen/__tests__/useImageGenController.test.tsx
pnpm check:precommit:full
pnpm check:prepush
python .trellis/scripts/task.py validate --all
git diff --check
```

Run the focused Rust config-migration, autostart/settings, and Image Gen suites selected by the changed modules
before broader Rust validation. If Unix-only watchdog coverage cannot run locally, use the authorized Docker Linux
environment and record the exact command/result.

## Review Gate

Do not start a second review agent. After all checks pass, create one fresh independent Orca worktree/terminal for
Codex `gpt-5.6-sol / effort=max`, give it a frozen SHA and a read-only review prompt, then aggregate only its report.

## Implementation Evidence (2026-07-18)

- Baseline/branch: `ef924951d4ab49e585f21f5d6ce53f1e17aed63a` on
  `FingerCaster/round7-sol-implementation`; merge-base with frozen review commit is
  `35db0f3287ec957e3479fc47b05f8ae1fd882eeb`. Initial tracked worktree was clean.
- Image Gen: controller file `80 passed`; Rust `history_persist_` filter `10 passed`.
  The first Rust invocation timed out during initial compilation, and the first new fixture was correctly rejected
  for retaining thumbs after clearing images; the corrected fixture passed without production-boundary changes.
- Config migration/settings: `config_migrate` filter `70 passed`; same-value ABA filter `2 passed`;
  `settings_service` filter with `--test-threads=1` `11 passed`; aggregate payload/file tests `2 passed`;
  legal `>1 MiB` arbitrary-byte production export/import `1 passed`.
- `cargo test ... autostart --lib --locked` initially produced `13 passed; 2 failed` because parallel
  generation-overflow tests shared global generation/hook state. The exact serial reproduction
  `cargo test --manifest-path src-tauri/Cargo.toml autostart --lib --locked -- --test-threads=1`
  passed `15 passed; 0 failed`; no unrelated test-infrastructure change was made.
- Full gates: `pnpm check:precommit:full` passed `13/13`; `pnpm check:prepush` passed `15/15`;
  `python .trellis/scripts/task.py validate --all` passed all 24 manifests; child 1-5 unchecked-marker
  audit returned 0; `git diff --check` passed.
- The worktree initially lacked `node_modules`, so the first Vitest command could not resolve `vitest`.
  `pnpm install --frozen-lockfile` restored the locked dependency environment; the final controller run passed.
- Local Skill candidates missing `SKILL.md` or carrying the managed marker return `Ok(None)` before
  `SkillFileCollector` is created, so they consume no aggregate budget. An invalid parsed payload aborts the
  whole export and the export-scoped budget is discarded; no partial bundle or persistent consumption remains.
- Same-session Trellis check reviewed all production/test/spec/task diffs against `check.jsonl`; per the explicit
  single-session constraint, no `trellis-check`, Claude, Pi, or other review agent was started.
- Docker/Linux command (exit 0):

  ```powershell
  $repo = (Get-Location).Path
  $script = @'
  set -e
  export PATH=/usr/local/cargo/bin:$PATH
  command -v cargo
  cargo --version
  export DEBIAN_FRONTEND=noninteractive
  apt-get -qq update
  apt-get -o Acquire::Retries=2 install -y --fix-missing --no-install-recommends libasound2-dev librsvg2-dev patchelf libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev pkg-config
  cd /workspace/src-tauri
  CARGO_TARGET_DIR=/tmp/aio-target cargo test -q --locked --lib config_export_fifo_replacement_fails_closed_under_external_watchdog -- --test-threads=1 --nocapture
  '@
  docker run --rm --mount "type=bind,source=$repo,target=/workspace" rust:1.90-bookworm bash -c $script
  ```

  Container toolchain was Cargo 1.90.0. The external-watchdog parent and its production child each reported
  `1 passed; 0 failed`; the command completed in 207.9 seconds. Existing Linux-only `unused_mut`/`dead_code`
  warnings were recorded and left out of scope.
