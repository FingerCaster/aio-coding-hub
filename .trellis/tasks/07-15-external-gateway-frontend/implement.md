# Implementation Plan: External gateway frontend

## 1. Foundation and Baseline

- [ ] Verify foundation SHA and generated contracts; read parent/child artifacts
      and frontend conventions.
- [ ] Record allowed paths and run current CodexTab, CLI proxy, routes,
      request-log, service/query, and settings tests.
- [ ] Ask the coordinator if a generated contract is missing; do not edit it.

## 2. Service and Query State

- [ ] Add command service adapters and query/mutation keys for plan/status,
      enable, update check/apply, SHA validation, Node override, retry,
      uninstall, and details session.
- [ ] Add generation-aware event/query reconciliation and status presentation
      projection with focused tests.
- [ ] Extend MSW state/handlers and typed frontend fixtures against foundation
      bindings.

## 3. Consolidated Confirmations

- [ ] Implement one conditional enable confirmation with exact Provider Sync
      transition/session impact and all required acceptance flags.
- [ ] Test cancel/no mutate, stale plan, first download, unreviewed source,
      implicit CLI proxy, Provider Sync, WSL, and combined rows.
- [ ] Reuse existing Provider Sync running/process-check result messages.
- [ ] Add separate update-apply and uninstall confirmations.

## 4. Codex Gateway Surface

- [ ] Replace local guard configuration/statistics with compact external status
      and actions.
- [ ] Link gateway-only and sidebar CLI-proxy lifecycle semantics.
- [ ] Render full SHA/trust/license, Node, port, route phase, recovery errors,
      retry/update/uninstall, and persistent WSL warning.
- [ ] Verify responsive wrapping and stable control dimensions.

## 5. Details Route and CSP

- [ ] Add lazy route and AIO-owned controls around a bridge-session iframe.
- [ ] Add loopback-only frame CSP and minimum sandbox attributes.
- [ ] Test navigation/unload/window hide does not call lifecycle mutations;
      explicit actions and fallback browser behavior do.

## 6. Remove Legacy Frontend

- [ ] Remove all parent-inventory local guard UI, stats query/service/parser,
      settings adapter/fixture, badge/detail/card code and guard-only tests.
- [ ] Retain route mismatch, Codex system, service-tier, generic logs, manual
      Provider Sync, and unrelated Codex settings/tests.
- [ ] Run frontend negative searches excluding archived history.

## 7. Focused Validation and Handoff

- [ ] Run focused Vitest suites, typecheck, lint for owned files, CSP contract
      tests, and `git diff --check`.
- [ ] Audit ownership, commit child changes, and send one `worker_done` with
      commit/path/tests/assumptions/risks.
- [ ] Do not edit generated/Rust/shared Trellis state, merge, push, release, or
      touch main.
