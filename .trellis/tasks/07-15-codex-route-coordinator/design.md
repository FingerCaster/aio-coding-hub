# Design: Chain-aware Codex route coordinator

## Foundation Inputs

Consume foundation `CodexRouteMode`, origin wrappers, operation generation,
Provider Sync plan/result DTOs, and a route-transition store trait. Use fakes in
this branch; the runtime child supplies the durable store and process status at
integration.

## Owned Paths

```text
src-tauri/src/infra/cli_proxy/**
src-tauri/src/infra/codex_config/**
src-tauri/src/infra/codex_provider_sync.rs
src-tauri/src/infra/codex_provider_sync/**
src-tauri/src/app/cli_proxy_service.rs
src-tauri/src/commands/cli_proxy.rs
src-tauri/tests/cli_proxy_*.rs
src-tauri/tests/codex_provider_sync.rs
focused test-support additions required only by these tests
```

Shared startup, cleanup, registry, generated, and external runtime files remain
integration-owned.

## Canonical and Live Representations

The manifest stores one canonical unproxied snapshot plus enabled/route
metadata. A projection function accepts canonical bytes, route mode, AIO origin,
external origin when guarded, auth mode, platform, and effective provider. It
returns validated live config bytes without mutating the canonical route.

Structured user config edits update canonical semantics first, then project the
current live route. Managed raw TOML edits follow the same path or fail closed.
Config hashes and route generation support transition verification and startup
repair.

## Provider Mode

Effective provider is `OpenAI` only when canonical
`features.remote_compaction=true`; otherwise `aio`. Provider Sync continues to
own sessions, SQLite, and global state. The coordinator adds a read-only plan,
snapshots route/manifest inputs, prepares canonical/live bytes, then invokes the
existing transaction once with the routed target config.

The plan shown by the frontend is advisory. The mutation recomputes current and
target provider, Codex process state, canonical hash, and generation before
accepting a confirmation.

## Route Operations

Expose narrow primitives:

```text
plan_external_enable
apply_guarded_route
apply_direct_aio_route
restore_unproxied_route
verify_route
reconcile_pending_route
```

External lifecycle ordering stays outside this module. Each primitive prepares
and verifies a transition journal through the foundation trait and returns a
typed result/rollback outcome.

## Compatibility

- Preserve existing backup path safety, atomic writes, invalid config behavior,
  OAuth-compatible auth rules, MCP re-sync contract, Codex Home rebind, Windows
  sandbox, and incomplete-enable recovery.
- Do not alter Claude/Gemini apply/status/restore paths.
- Keep manual Provider Sync and its user-visible errors.

## Forbidden Paths

Do not edit new `infra/codex_retry_gateway/**`, runtime service/bridge,
failover/local-guard modules, backend guard settings/statistics, `src/**`,
generated bindings, startup/cleanup/registries, Tauri config, task/journal
state, main, or remotes. Contract defects go to the coordinator.

## Integration Handoff

Report route API signatures, commit SHA, path audit, focused test output, and
which startup/external lifecycle calls the integration coordinator must order.
