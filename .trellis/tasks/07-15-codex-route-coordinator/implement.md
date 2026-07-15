# Implementation Plan: Chain-aware Codex route coordinator

## 1. Foundation and Baseline

- [ ] Verify foundation SHA, read parent/child specs, and record allowed paths.
- [ ] Run current CLI-proxy, Codex config, provider-sync, Home-rebind, OAuth mode,
      MCP sync, and startup-recovery focused tests.
- [ ] Confirm required route/store DTOs exist; ask rather than edit shared files.

## 2. Route Model and Canonical Backup

- [ ] Add exhaustive Codex route metadata and typed AIO/external origins.
- [ ] Refactor Codex backup/live config handling so canonical bytes never learn
      a temporary routed URL.
- [ ] Add route projection/verification for unproxied, direct AIO, and guarded,
      including OAuth-compatible auth and current provider identity.
- [ ] Preserve Claude/Gemini paths and all existing backup/path safeguards.

## 3. Chain-Aware CLI-Proxy Service

- [ ] Make Codex status report desired enabled, effective origin, route mode,
      and applied truthfully while keeping public compatibility where required.
- [ ] Add combined-enable, gateway-only direct route, explicit unproxied restore,
      crash/update bypass, and pending-transition reconciliation primitives.
- [ ] Keep MCP resync and custom Codex Home behavior on successful commits.

## 4. Provider Sync Enable Plan

- [ ] Add pure current/target/change-required/process-precondition projection.
- [ ] Recompute under lifecycle lock and reject stale generation/config hash.
- [ ] Invoke existing Provider Sync once for provider-changing route apply and
      preserve manual sync result/error behavior.
- [ ] Test cancel/no-call at frontend-contract level through command fakes and
      backend stale/running/check/sync failures here.

## 5. `remote_compaction`

- [ ] Apply canonical feature/provider change and current route projection as
      one provider-sync transaction.
- [ ] Update compatibility callback only after prepared snapshots and verify it
      before commit.
- [ ] Cover on/off in all parent-required route modes and injected failures at
      backup, provider sync, live write, compatibility update, and verification.

## 6. Focused Validation and Handoff

- [ ] Run formatting plus route/config/provider-sync integration tests and
      retained Claude/Gemini/OAuth/MCP/Home tests.
- [ ] Run `git diff --check` and ownership audit.
- [ ] Commit child-owned changes and send one `worker_done` with commit, paths,
      tests, API/glue notes, and residual risks.
- [ ] Do not merge, push, release, edit shared Trellis state, or touch main.

