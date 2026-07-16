# Remove managed retry gateway and keep a repository recommendation

## Goal

Keep AIO Coding Hub out of the Codex reasoning-degradation interception
business. The local reasoning guard stays removed, and the attempted managed
integration of `nonononull/codex-retry-gateway` is removed as well. The Codex
tab contains only a recommendation card that opens the official repository.

## Requirements

### R1. Do not restore the local reasoning guard

- Keep the local detection, interception, continuation repair, dedicated
  settings, statistics, request-log presentation, scripts, and maintenance
  rules removed.
- Preserve generic transient retry, provider failover, model routing, normal
  request logs, streaming/non-streaming passthrough, usage, and cancellation.

### R2. Remove AIO-managed external gateway behavior

- Remove source download, commit validation, Node discovery, process ownership,
  bridge sessions, update/uninstall flows, startup reconciliation, and recovery.
- Remove external route modes from Codex CLI proxy, Provider Sync, Codex config
  transactions, settings, commands, generated bindings, tests, and mocks.
- Remove the details route, iframe CSP allowance, status events, and management
  controls.

### R3. Keep only a recommendation card

- Show a compact `Codex reasoning guard gateway recommendation` card in the
  Codex tab.
- Show the repository identity `nonononull/codex-retry-gateway`.
- Provide one `View repository` command that opens the fixed official HTTPS URL
  through AIO's validated desktop URL opener.
- The card must have no enable state, details entry, update action, status poll,
  process control, or Codex config side effect.

### R4. Preserve adjacent fork behavior

- Preserve the `approvals_reviewer` Codex setting.
- Preserve the route-neutral presentation for `codex-auto-review*` mappings.
- Preserve ordinary Codex CLI proxy, Provider Sync, remote compaction, WSL,
  Claude/Gemini proxy, plugins, and request-log behavior.

### R5. Keep settings migration coherent

- Schema 49 represents removal of the old local reasoning-guard schema.
- Do not add external gateway fields to `AppSettings`.
- Unknown fields from local pre-release test builds are ignored on read and
  disappear on the next canonical settings write.

## Acceptance Criteria

- [ ] Active runtime source contains no managed external gateway command,
      setting, route mode, process, bridge, event, query, service, or mock.
- [ ] Active runtime source contains no local reasoning-guard implementation or
      user-facing configuration/statistics surface.
- [ ] The Codex tab renders the recommendation and opens exactly
      `https://github.com/nonononull/codex-retry-gateway`.
- [ ] Generated bindings contain neither old local guard fields nor managed
      external gateway types/commands.
- [ ] Focused UI/settings tests, typecheck, lint, full pre-commit, pre-push, and
      Windows x64 packaging pass.
- [ ] The integration branch remains isolated from `main` until user approval.
