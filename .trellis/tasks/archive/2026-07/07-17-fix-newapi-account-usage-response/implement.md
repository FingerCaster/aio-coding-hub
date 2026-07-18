# Implementation Plan: NewAPI account usage

## Entry gate

- [x] Prove child 2 is archived and the worktree is clean.
- [x] Read the cross-layer guide and redacted NewAPI research.

## Backend protocol

- [x] Add URL builders for status, billing subscription and billing usage without changing sub2api.
- [x] Reuse one bounded reqwest client and bearer model token for authenticated billing endpoints.
- [x] Add bounded response readers and application-error classification before field parsing.
- [x] Normalize USD billing fields into total/used/balance/expiry; handle unknown display types
      explicitly.
- [x] Remove the live path's dependence on `/api/user/self` and hardcoded quota division while
      preserving any evidence-backed legacy parser only behind a clear contract.

## Tests

- [x] Add synthetic status/subscription/usage success fixtures and exact formula assertions.
- [x] Add `success=false`, root error, missing field, non-finite/negative edge, unknown unit, expiry and
      partial endpoint failure tests.
- [x] Test Base URL variants, Bearer auth, date range and response body limit.
- [x] Verify sub2api fixtures and frontend DTO rendering remain green.

## Safe live validation

- [x] Only after fixtures pass, use the user-authorized `muyuan` provider for the minimum read-only
      status/subscription/usage requests.
- [x] Output only status, field presence/types, unit enum, finite/formula booleans and cache-header
      presence. Never print/persist API Key, host, body, PII, token name or numeric account values.

## Validation

- [x] `cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked`
- [x] `pnpm exec vitest run src/query/__tests__/providers.test.tsx src/pages/providers/__tests__/SortableProviderCard.test.tsx src/services/providers/__tests__/providers.service.test.ts`
- [x] `pnpm check:generated-bindings`
- [x] `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`, `pnpm tauri:check`, `pnpm tauri:clippy`
- [x] Secret/PII diff audit and `git diff --check`.

## Exit gate

- [x] Confirm account usage remains display-only.
- [x] Commit and archive child 3; do not start child 4 until complete.

## Post-fix live acceptance evidence

- Completed on 2026-07-17 under child `07-17-final-review-findings-round-2` using the locally configured
  `muyuan` provider and three read-only GETs. All response classes, field types, finite/formula assertions
  passed without persisting secrets, host, query, body, PII or account values.
- Evidence: `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2/research/muyuan-post-fix-readonly-validation.md`.
