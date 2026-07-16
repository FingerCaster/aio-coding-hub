# Implementation Plan: account usage refresh ordering

## Entry gate

- [ ] Prove child 1 is archived and the worktree is clean.
- [ ] Read cross-layer guidance and `research/manual-refresh-race.md`.

## Query layer

- [ ] Extract a shared provider-account-usage query options factory.
- [ ] Refactor the hook to consume it without changing enable or interval semantics.
- [ ] Replace direct IPC + `setQueryData` manual refresh with exact cancellation followed by a forced
      query-owned fetch.
- [ ] Keep provider ID validation and return type behavior unchanged.

## Component and tests

- [ ] Make refresh loading state follow the authoritative query fetch.
- [ ] Add deferred old-auto/new-manual ordering coverage and assert final cache/data is manual-new.
- [ ] Add manual-while-initial-fetching and manual-while-timed-refetching cases.
- [ ] Assert a manual refresh invokes no circuit reset, availability test, provider mutation or
      unrelated query invalidation.
- [ ] Retain interval, disabled-provider and provider edit/delete cache tests.

## Validation

- [ ] `pnpm exec vitest run src/query/__tests__/providers.test.tsx src/pages/providers/__tests__/SortableProviderCard.test.tsx`
- [ ] `pnpm exec vitest run src/services/providers/__tests__/providers.service.test.ts`
- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `pnpm format:check`
- [ ] `git diff --check`

## Exit gate

- [ ] Review the diff for account-display-only isolation.
- [ ] Commit and archive child 2; do not start child 3 before both are complete.
