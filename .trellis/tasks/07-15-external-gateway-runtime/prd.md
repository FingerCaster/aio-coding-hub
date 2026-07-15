# Build external gateway runtime backend

## Parent Contract

This is a child of `07-15-external-codex-retry-gateway`. The parent `prd.md`,
`design.md`, and `research.md` are authoritative for product behavior. This
child owns runtime infrastructure only and starts from the parent's reviewed
foundation commit.

## Goal

Implement the AIO-owned backend that resolves and installs approved official
gateway source, verifies Node.js, manages one fully owned loopback process,
supervises health/recovery primitives, and exposes the external management page
through a protected AIO loopback bridge.

## Requirements

### R1. Official immutable source store

- Resolve only full official `nonononull/codex-retry-gateway` main/ancestor
  commits, download by canonical SHA, validate bounded archives, persist source
  fingerprints, and atomically promote immutable source directories.
- Support verified offline cache use and same-SHA repair. Do not accept custom
  repository/download URLs or package external source with AIO.

### R2. Verified Node and process ownership

- Discover or validate an absolute Node executable and require a bounded direct
  version probe of Node 18+ before execution.
- Spawn without a shell string and record PID/start identity, executable,
  source/config/state paths, commit, listener, AIO upstream, and instance nonce.
- Reuse, stop, or update only when every available identity and health field
  matches. Never kill a foreign/stale PID or port occupant.

### R3. Managed state, port, and health

- Parse/write bounded versioned AIO ownership state under the feature root and
  reconstruct paths under that trusted root.
- Prefer persisted port then `4610`; select another loopback port for a foreign
  occupant and persist only after full verification.
- Provide process start/stop/probe and supervisor recovery primitives with
  bounded backoff/storm protection. The parent integration service decides
  Codex route changes.

### R4. Protected management bridge

- Serve iframe/browser sessions from an AIO-only loopback bridge with expiring
  session authorization, an explicit method/path allowlist, streaming bounds,
  and per-request managed-instance validation.
- Proxy external-owned UI/status/config/log/analytics/import/export/probe
  behavior while rejecting changes to AIO-owned listener, upstream, and health
  fields.
- Never forward external whole-file restore. Delegate restore/exit through the
  foundation lifecycle callback and keep raw external compatibility state from
  containing a usable restore backup.

### R5. Parallel ownership boundary

- Add new runtime modules and their tests. Do not edit Codex CLI proxy/config/
  Provider Sync, existing local-guard modules, frontend code, generated
  bindings, startup/cleanup registries, or main.
- Consume the foundation DTO/trait contracts. Report required contract changes
  to the integration coordinator instead of editing shared foundation files.

## Acceptance Criteria

- [ ] Valid official source installs and revalidates; all invalid ancestry,
      redirect, archive, cache, and offline cases fail without executable state.
- [ ] Automatic/manual Node paths and process ownership pass success, timeout,
      old-version, stale-PID, foreign-listener, crash, and stop-mismatch tests.
- [ ] Port fallback and state recovery persist only verified identities.
- [ ] Bridge authentication/allowlist/config protection/restore interception
      tests pass, including process disappearance during a request.
- [ ] Worker branch modifies only declared paths, passes focused Rust tests, and
      reports its commit and residual integration requirements without touching
      main, pushing, or releasing.

## Out of Scope

- Host Codex route/config projection or Provider Sync.
- Frontend UI, Tauri CSP, generated bindings, command registry, startup/exit
  wiring, or local reasoning-guard deletion.
- Merging into integration/main, packaging, or release.
