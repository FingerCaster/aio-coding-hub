# Design: 统一 gate 与可解释的 route 计数

## Rooted flow

Current:

```text
candidate list
  -> session binding checks circuit
     -> denied bound provider removed (no attempt row)
  -> failover common gate for remaining providers
  -> attempts_json length mislabeled as switch count
```

Target:

```text
candidate list
  -> session binding only determines reuse preference/order
  -> every candidate enters common gate
     -> denied: one skipped attempt, zero upstream calls, zero Ready budget
     -> allowed: Ready budget increments, retry engine runs
  -> route hops / transitions / attempts rendered as separate quantities
```

## Backend changes

1. In `provider_selection::resolve_session_bound_provider_id`, retain a bound provider that is still
   in the eligible candidate set even when its current circuit decision denies reuse. Return no reuse
   binding and let the later common gate own the authoritative decision.
2. Keep stale binding removal when the provider is genuinely absent from the candidate set; that is a
   different eligibility boundary.
3. Reuse `provider_checks::run_gates` to create the skipped record. Do not add a parallel skip encoder.
4. Preserve `providers_tried` increment after successful preparation so skips remain outside the
   configured Ready-provider cap.

The small timing window where circuit state changes between selection and gate is resolved by making
the later gate authoritative. This is preferable to two decisions with one silently discarded.

## Frontend changes

`buildRequestRouteMeta` already receives both `route` and `attemptCount`:

- `providerCount = hops.length`
- `transitionCount = max(providerCount - 1, 0)`
- `attemptCount` remains the number of persisted attempt rows
- retry and skipped counts continue to come from per-hop `attempts`/`skipped`

Use a compact label that exposes provider scope and transition count without calling every attempt a
switch. Keep the existing route tooltip as the per-provider source of truth.

## Compatibility

- Forced-provider eligibility and sort-mode membership remain unchanged.
- Session binding remains stored when a provider is temporarily denied, matching current behavior.
- All-unavailable caching/Retry-After logic is unchanged; only the missing skipped evidence is added.
- No response body, stream, usage, response-id or timing path changes.

## Risks And Rollback

- **Risk:** a provider becomes allowed at the later common gate. This is an intentional single-owner
  decision and must be tested at cooldown recovery boundaries.
- **Risk:** adding a skipped row changes `attempt_count` and UI labels. Update route/presentation tests
  together; do not alter historical database rows.
- **Rollback:** revert the backend selection and frontend presentation commit together. No schema or
  persisted-data migration is required.
