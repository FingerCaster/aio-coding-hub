# Implementation Follow-ups

All work is confined to the independent worktree
`D:\\OrcaProjects\\aio-coding-hub-fork\\codex-gpt56-372k-context`.
Agents share the worktree, must preserve other agents' edits, and must not
commit, push, merge, package, or publish.

## duplicate_slug_contract

- Own only `src-tauri/src/infra/codex_model_catalog/managed.rs`.
- Make a duplicated canonical GPT-5.6 target slug fail with
  `CODEX_GPT56_372K_MODELS_MISSING`, matching the missing-target contract.
- Preserve the generic base-catalog error for duplicate non-target slugs.
- Update or add focused tests for both cases and run the managed catalog test
  module, Rust formatting, and `git diff --check`.

## independent_catalog_rollback

- Own only `src-tauri/src/infra/codex_config/mod.rs` and
  `src-tauri/src/infra/codex_config/tests.rs` (plus a directly relevant
  codex-config integration test only if essential).
- Audit and fix managed-catalog compensation so live config and proxy backup
  rollback are independent committed-token operations. Drift or failure of
  one target must not suppress rollback of the other still-owned target.
- Add deterministic regression tests covering both directions and run focused
  Codex config tests, Rust formatting, and `git diff --check`.

## settings_transaction_faults

- Own only `src-tauri/src/app/settings_service.rs` and tests in that module.
- Audit the dedicated GPT-5.6 372K settings transaction for a deterministic
  failure after the settings bit commits but before catalog apply succeeds.
- Add the missing end-to-end fault-injection regression if it is not already
  present. Assert the owned settings bit and every already-committed catalog,
  config, generated, and backup target are compensated, while a concurrent
  settings winner is preserved.
- Do not duplicate lower-level codex-config tests owned by the other agent.
- Run focused settings-service tests, Rust formatting, and `git diff --check`.
