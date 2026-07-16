# Implementation Plan: NewAPI account usage

## Entry gate

- [ ] Prove child 2 is archived and the worktree is clean.
- [ ] Read the cross-layer guide and redacted NewAPI research.

## Backend protocol

- [ ] Add URL builders for status, billing subscription and billing usage without changing sub2api.
- [ ] Reuse one bounded reqwest client and bearer model token for authenticated billing endpoints.
- [ ] Add bounded response readers and application-error classification before field parsing.
- [ ] Normalize USD billing fields into total/used/balance/expiry; handle unknown display types
      explicitly.
- [ ] Remove the live path's dependence on `/api/user/self` and hardcoded quota division while
      preserving any evidence-backed legacy parser only behind a clear contract.

## Tests

- [ ] Add synthetic status/subscription/usage success fixtures and exact formula assertions.
- [ ] Add `success=false`, root error, missing field, non-finite/negative edge, unknown unit, expiry and
      partial endpoint failure tests.
- [ ] Test Base URL variants, Bearer auth, date range and response body limit.
- [ ] Verify sub2api fixtures and frontend DTO rendering remain green.

## Safe live validation

- [ ] Only after fixtures pass, use the user-authorized `muyuan` provider for the minimum read-only
      status/subscription/usage requests.
- [ ] Output only status, field presence/types, unit enum, finite/formula booleans and cache-header
      presence. Never print/persist API Key, host, body, PII, token name or numeric account values.

## Validation

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked`
- [ ] `pnpm exec vitest run src/query/__tests__/providers.test.tsx src/pages/providers/__tests__/SortableProviderCard.test.tsx src/services/providers/__tests__/providers.service.test.ts`
- [ ] `pnpm check:generated-bindings`
- [ ] `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`, `pnpm tauri:check`, `pnpm tauri:clippy`
- [ ] Secret/PII diff audit and `git diff --check`.

## Exit gate

- [ ] Confirm account usage remains display-only.
- [ ] Commit and archive child 3; do not start child 4 until complete.
