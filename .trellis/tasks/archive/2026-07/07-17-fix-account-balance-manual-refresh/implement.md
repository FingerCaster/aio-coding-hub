# Implementation Plan: account usage refresh ordering

## Entry gate

- [x] Prove child 1 is archived and the worktree is clean.
- [x] Read cross-layer guidance and `research/manual-refresh-race.md`.

## Query layer

- [x] Extract a shared provider-account-usage query options factory.
- [x] Refactor the hook to consume it without changing enable or interval semantics.
- [x] Replace direct IPC + `setQueryData` manual refresh with exact cancellation followed by a forced
      query-owned fetch.
- [x] Keep provider ID validation and return type behavior unchanged.

## Component and tests

- [x] Make refresh loading state follow the authoritative query fetch.
- [x] Add deferred old-auto/new-manual ordering coverage and assert final cache/data is manual-new.
- [x] Add manual-while-initial-fetching and manual-while-timed-refetching cases.
- [x] Assert a manual refresh invokes no circuit reset, availability test, provider mutation or
      unrelated query invalidation.
- [x] Retain interval, disabled-provider and provider edit/delete cache tests.

## Validation

- [x] `pnpm exec vitest run src/query/__tests__/providers.test.tsx src/pages/providers/__tests__/SortableProviderCard.test.tsx`
- [x] `pnpm exec vitest run src/services/providers/__tests__/providers.service.test.ts`
- [x] `pnpm typecheck`
- [x] `pnpm lint`
- [x] `pnpm format:check`
- [x] `git diff --check`

## Exit gate

- [x] Review the diff for account-display-only isolation.
- [x] Commit and archive child 2; do not start child 3 before both are complete.
