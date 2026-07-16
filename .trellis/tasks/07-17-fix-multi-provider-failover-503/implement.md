# Implementation Plan: multi-provider failover observability

## Entry gate

- [ ] Confirm this is the first child and no later child is active.
- [ ] Read the backend index, attempt-budget contract and task research before editing.
- [ ] Confirm no local reasoning-guard/continuation surface is reintroduced.

## Backend

- [ ] Change session-bound provider resolution so temporary circuit/cooldown denial does not delete an
      otherwise eligible candidate before the common gate.
- [ ] Update unit tests that currently expect the denied bound provider to be removed.
- [ ] Add a route regression matching the three-provider field chain: bound provider denied, other
      providers skipped, three skipped rows, 503, zero upstream calls.
- [ ] Add/retain a three-eligible-provider test proving the third provider runs when the Ready budget
      allows it, plus a configured-limit boundary test.

## Frontend

- [ ] Derive provider count and transition count from route hops; keep attempt count separate.
- [ ] Update presentation and Home panel tests for skips, retries, three providers and failed/successful
      failover.

## Validation

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider_selection --lib --locked`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider_max_attempts --lib --locked`
- [ ] Run the new route regression and existing model-discovery/health-neutral route tests.
- [ ] `pnpm exec vitest run src/components/home/__tests__/requestLogPresentation.test.ts src/components/home/__tests__/HomeRequestLogsPanel.test.tsx`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`
- [ ] `pnpm check:generated-bindings`, `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`
- [ ] `git diff --check` and negative search for restored reasoning-guard/continuation product symbols.

## Exit gate

- [ ] Review attempts/route semantics against the redacted field evidence.
- [ ] Commit only child 1 files, run Trellis check/spec judgment, archive child 1.
- [ ] Do not start child 2 until the archive and clean-worktree checks pass.
