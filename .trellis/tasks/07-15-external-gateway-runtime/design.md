# Design: External gateway runtime backend

## Foundation Inputs

The integration foundation supplies stable DTOs and narrow traits for source
selection, route callbacks, public status, and command signatures. Runtime code
must not know how Codex TOML or Provider Sync works. It receives a verified AIO
origin and calls a coordinator-owned disable/recovery callback when bridge or
process lifecycle needs a route change.

## Owned Modules

```text
src-tauri/src/infra/codex_retry_gateway/
  mod.rs
  source.rs
  node.rs
  managed_state.rs
  config.rs
  process.rs
  bridge.rs

src-tauri/src/app/codex_retry_gateway_service.rs
src-tauri/src/commands/codex_retry_gateway.rs
focused unit/integration test modules and fixtures
```

The worker may split files when size warrants it, but all parsing from unknown
JSON/HTTP/process state has one typed owner at the infrastructure boundary.

## Source Store

- Compile the repository owner/name and consume only exact 40-hex SHAs.
- Resolve official main and ancestry with distinct not-found, transient, and
  rate-limit errors.
- Reuse/factor current bounded GitHub ZIP and package hashing helpers.
- Stage under the AIO feature root, reject traversal/symlink/reparse/duplicate
  paths and archive limits, validate layout plus Node syntax, fingerprint, then
  atomic-rename to `sources/<sha>`.
- A source manifest binds repository, commit, verified main, verification time,
  layout version, and fingerprints. Cached execution revalidates the manifest
  and files.

## Node and Process

Node resolution returns an executable, version, and source classification. A
manual selection replaces the prior one only after full validation. Launch uses
an exact executable plus argument vector and controlled cwd/PATH.

Process identity combines the child/PID record, OS start identity where
available, executable/source/config/state paths, source SHA, instance nonce,
listener, and health `process_id`/upstream. Stop/reuse requires the complete
predicate; partial matches are diagnostic conflicts.

The runtime writes external config and compatibility state but omits a valid
restore backup. It exposes start, probe, stop, update-candidate verification,
and supervisor primitives. It does not change Codex routing directly.

## Bridge

Use a separate loopback server from both AIO and the external proxy. A launch
URL establishes an expiring HttpOnly SameSite session, then redirects to the
external UI path. Every request revalidates current runtime generation and
process identity.

Allow known external paths only. Config POST accepts external-owned behavior
fields but requires listener/upstream/health values to equal the managed state.
Restore invokes a trait callback and is never forwarded. Status may overlay
AIO-authoritative runtime/commit fields supplied through the foundation status
projection.

## Concurrency and Failure

Network staging can occur outside the shared route critical section, but final
process switch methods require an operation ID/generation and reject stale
commit attempts. Supervisor methods use bounded timeouts/backoff and report
state; the integration coordinator orders direct-AIO fallback before restart.

## File Ownership

Allowed: new `codex_retry_gateway` infrastructure, new runtime service/command
files, and child-owned tests/fixtures.

Forbidden: existing `infra/cli_proxy`, `infra/codex_config`, provider sync,
gateway failover/local guard, `src/**`, `src/generated/bindings.ts`, command/app
registries, startup/cleanup, `tauri.conf.json`, parent/other child task state,
main operations, push, and release.

## Integration Handoff

Return commit SHA, exact modified paths, source/Node/process/bridge test output,
public contract mismatches, and startup/route glue still required. Do not merge
another branch or edit shared glue to make the worker branch globally complete.
