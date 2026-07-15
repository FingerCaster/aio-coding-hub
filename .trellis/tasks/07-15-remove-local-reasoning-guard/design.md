# Design: Remove local Codex reasoning guard

## Deletion Source of Truth

Parent `inventory.md` is the path/behavior boundary. A file listed as shared is
edited surgically; a dedicated file is deleted. Search results are reviewed by
semantic owner rather than blanket deletion of every Codex retry symbol.

## Backend Removal Groups

1. Delete dedicated attempt/response guard and continuation modules.
2. Simplify shared failover context, attempt execution/reasons, provider budget,
   stream/non-stream response routing, finalization, request context/end, and
   route tests to ordinary behavior.
3. Remove old settings schema/default/migration/persistence/service fields while
   preserving the external foundation and unrelated options.
4. Remove request-log guard stats/query/command/backend marker interpretation
   while preserving generic storage/retention.
5. Remove active scripts/docs/spec/release rules and preserve changelog history.

## Preservation Contract

Use retained tests and parent backend specs as executable boundaries. The
effective provider attempt baseline, enabled transient reservation, OAuth and
`previous_response_id` recovery, total caps, strict discovery, failover,
circuits, model-route diagnostics, request/attempt logs, normal streaming,
usage/cost, cancellation, and rectifiers remain.

Removing local continuation reservations may require simplifying formulas and
tests, but it must not change configured request budgets or cross-request
circuit accounting.

## Settings and Historical Data

Serde/default/migration code drops old keys and writes only current schema.
Foundation external fields remain fully represented. No database migration
rewrites or deletes request logs; only dedicated interpretation/query/UI owners
are removed across this backend child and the frontend child.

## File Ownership

Allowed: exact backend/settings/request-log/scripts/docs/spec/AGENTS paths in
parent `inventory.md`, their focused Rust tests, and removal-only registry lines
needed to compile.

Forbidden: `src/**`, `src/generated/bindings.ts`, `infra/cli_proxy/**`,
`infra/codex_config/**`, provider sync, new `codex_retry_gateway/**`, external
runtime service/commands, startup/cleanup integration, Tauri CSP, shared task/
journal state, main, remote operations, push, and release.

## Negative Search

After removal, search active source for:

```text
codex_reasoning_guard
CodexReasoningGuard
codex_reasoning_features
codex_reasoning_continuation
continuation_repair
request_logs_codex_reasoning_guard_stats
```

Only historical changelog and archived task/research references are allowed.
Build outputs are ignored, not edited.

## Integration Handoff

Report deleted/modified paths, retained behavior tests, negative-search output,
foundation fields verified, commit SHA, and any registry/generated/frontend
cleanup left for integration/other workers.

