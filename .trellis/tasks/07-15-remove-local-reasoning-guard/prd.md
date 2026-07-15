# Remove local Codex reasoning guard

## Parent Contract

This child implements the backend/repository portion of parent requirements R1
and R12 using the exact boundary in parent `inventory.md`. It starts from the
shared foundation, whose new external settings/contracts must be preserved.

## Goal

Delete the AIO in-process reasoning-degradation guard, continuation repair,
backend statistics/settings, and active maintenance material without removing
generic retry, routing, logging, Codex configuration, or the newly introduced
external gateway foundation.

## Requirements

### R1. Delete dedicated local implementation

- Delete the four dedicated reasoning-guard/continuation Rust modules, module
  wiring, callers, buffers, outcomes, classifiers, fixtures, and guard-only
  tests listed in `inventory.md`.
- Remove owned branches only from shared failover/streaming/non-streaming/
  request context/finalization/routes code.

### R2. Preserve adjacent gateway behavior

- Retain generic transient retry/provider overrides, OAuth refresh,
  `previous_response_id`, attempt caps, provider failover/circuits, model
  discovery health neutrality, route diagnostics, request logs, streams, usage,
  cost, cancellation, and response rectifiers.
- Strengthen or retain focused regressions where removing guard code changes a
  shared execution path.

### R3. Remove backend settings and statistics

- Remove all old guard defaults/types/migrations/persistence/service mappings
  while preserving foundation external-gateway fields and unrelated settings.
- Remove dedicated request-log stats command/query/types and old guard marker
  projection. Keep generic rows and `special_settings_json`; do not migrate or
  delete historical data.

### R4. Remove active maintenance material

- Remove obsolete guard probe/validation scripts and active README/wiki/spec
  claims. Preserve historical changelog entries.
- Surgically remove only the obsolete continuation release rule from the
  protected current `AGENTS.md` baseline and preserve all other user rules.

### R5. Parallel ownership boundary

- Own old Rust guard/shared backend/settings/statistics files plus repository
  scripts/docs/spec/AGENTS. Do not edit frontend `src/**`, CLI proxy/Codex
  config/provider sync, new external runtime, generated bindings, startup/
  cleanup glue, parent/other child task state, main, or remotes.
- A removal-only command-registry change is allowed when required for Rust
  compilation; do not add new external commands there.

## Acceptance Criteria

- [ ] Parent inventory is exhausted and active-source negative searches contain
      no old guard/continuation/stats symbols outside historical task/changelog.
- [ ] Full retained Rust gateway/config/settings/request-log tests pass,
      including adjacent retry/failover/model-route/stream/usage behavior.
- [ ] Old settings disappear on canonical persistence, generic request-log rows
      remain untouched, and foundation external fields survive migration tests.
- [ ] Scripts/docs/spec/AGENTS contain no active local-guard maintenance rule;
      historical changelog and unrelated user rules remain unchanged.
- [ ] Worker stays within ownership, commits/report tests, and never touches
      frontend/generated/main/push/release.

## Out of Scope

- New external runtime, route coordinator, frontend, generated bindings,
  startup/cleanup integration, packaging, integration/main merge, and release.
