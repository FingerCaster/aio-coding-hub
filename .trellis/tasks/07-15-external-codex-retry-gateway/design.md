# Design: recommendation-only Codex reasoning guard gateway

## Final boundary

```text
AIO Codex tab -> fixed repository URL

No AIO runtime path -> external gateway
No Codex route -> external gateway
```

AIO recommends the independent project but does not install, execute, update,
embed, configure, or monitor it. This removes the privileged native integration
and its cross-layer maintenance surface.

## Backend

- Delete `infra/codex_retry_gateway`, its app service, commands, route
  coordinator tests, and startup/cleanup integration.
- Restore ordinary CLI proxy, Codex config, and Provider Sync behavior from the
  integration base.
- Remove gateway settings and generated command registrations.
- Keep schema version 49 as a no-payload migration that marks removal of the
  legacy local reasoning-guard schema.

## Frontend

- Delete the manager component, details page, query/service/hook layers, event
  constants, fixtures, and MSW handlers.
- Render one compact recommendation card directly in `CodexTab`.
- Open the fixed repository URL with `openDesktopUrl`; no caller-provided URL is
  accepted.
- Keep the card independent of Codex availability and config loading because it
  is informational only.

## Retained behavior

- Local reasoning guard code and UI remain deleted.
- Generic retry/routing/logging remains untouched.
- `approvals_reviewer` and route-neutral `codex-auto-review*` presentation stay
  intact.

## Compatibility

The managed gateway integration existed only in this unreleased integration
branch. No production data migration or process takeover is added. Unknown
pre-release test fields are tolerated by Serde and removed on a later settings
write.

## Validation

- Negative searches distinguish the fixed recommendation URL from forbidden
  runtime integration identifiers.
- UI tests assert the card label, repository identity, and exact URL call.
- Rust migration tests assert schema 48 advances to schema 49 without gateway
  fields.
- Full project gates verify restored CLI proxy and config behavior.
