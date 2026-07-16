# Integration research summary

## Evidence classes

- **User observations** are retained verbatim in child research but are not treated as code facts.
- **Verified facts** come from baseline code/tests/history, read-only SQLite request logs, or the
  explicitly authorized redacted `muyuan` GETs.
- **Implementation-stage validation** is limited to deterministic regressions and one final redacted
  live check where specified.

## Confirmed roots

| Area | Confirmed root | Not the root |
| --- | --- | --- |
| Failover 503 | A session-bound provider denied by circuit/cooldown is removed before the common gate, so it is absent from attempts; UI labels raw attempt rows as switches. | Provider attempt ceiling: persisted value is 5, not 2. The observed 503 had no currently eligible third provider. |
| Manual balance refresh | Automatic query and manual direct fetch own independent writes to the same cache; an older automatic response can overwrite a newer manual result. | No client HTTP cache exists in the Rust command; no cache response headers were observed from `muyuan`. |
| NewAPI | `/api/user/self` returned HTTP 200 with `success=false` authentication error, while the parser ignored `success` and reported missing `quota`; the provider model key is not a user access token. | A renamed or nested quota field in the observed response. |
| Config export | The exporter recursively reads all local Skill files with a 1 MiB per-file cap introduced by boundary hardening and aborts the whole bundle on the first oversized file. | PNG decoding, Windows path parsing, or total bundle size in the reported case. |

## NewAPI read-only validation

- Provider: `muyuan`, direct API-key provider, one key, NewAPI adapter, timed refresh 300 seconds.
- No API Key, response body, PII, endpoint host or actual balance was printed or persisted.
- `/api/user/self`: HTTP 200, 64-byte JSON with only `success:boolean` and `message:string`;
  `success=false`; message classification was invalid/unauthorized credential, not quota/user shape.
- Public `/api/status`: HTTP 200, `quota_per_unit` present and numeric (`500000`), display type `USD`.
- `/api/usage/token/`: HTTP 500 application error on this deployment; it is not a usable primary path.
- `/v1/dashboard/billing/subscription`: HTTP 200 with numeric `hard_limit_usd` and access metadata.
- `/v1/dashboard/billing/usage`: HTTP 200 with numeric `total_usage`.
- A finite balance is computable using the documented NewAPI contract
  `hard_limit_usd - total_usage / 100`. No cache headers (`Cache-Control`, `Age`, `ETag`,
  `Last-Modified`, `Expires`) were present on the inspected account responses.

## Gateway boundary after the predecessor

- `07-15-external-codex-retry-gateway` deliberately removed the local reasoning-guard and
  continuation-repair product surfaces; this task must not restore them.
- The retained generic gateway contracts are the failover attempt-budget spec, OAuth and
  `previous_response_id` recovery reservations, strict model discovery, request logging, usage,
  response-id/TTFB passthrough, cancellation and `MAX_NON_SSE_BODY_BYTES = 20 * 1024 * 1024`.
- Child 1 is therefore an accounting/observability correction at provider selection and UI route
  presentation, not a response transformation feature.

## Fixed execution policy

- Baseline: local `main@2e43ee23572e69e34ce2c4cfb60481b58acf9298`.
- Order: child 1 -> 2 -> 3 -> 4 -> 5 -> parent integration acceptance.
- One task active at a time; no concurrent subagents.
- No `upstream` access before child 5. Child 5 keeps `origin` as normal target and `upstream`
  fetch-only, imports non-conflicting changes, and pauses on fork behavior conflicts.

## Remaining dynamic checks (not root-cause hypotheses)

- Child 1: exact route test with a blocked session-bound provider plus other skipped candidates.
- Child 2: deferred old/new promises proving old automatic completion cannot overwrite manual data.
- Child 3: after fixtures pass, repeat only the minimal redacted `muyuan` billing reads.
- Child 4: synthetic binary `>1 MiB && <=8 MiB` export/import round trip and `>8 MiB` rejection.

No unresolved product decision blocks implementation.
