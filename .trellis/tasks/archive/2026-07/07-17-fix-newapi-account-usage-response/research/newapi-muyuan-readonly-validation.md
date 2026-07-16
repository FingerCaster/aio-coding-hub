# NewAPI `muyuan` read-only validation

## Privacy method

The user explicitly authorized read-only validation of `muyuan`. The investigation:

- read the configured Base URL and API Key only into process memory;
- sent GET requests only;
- never printed or wrote the API Key, endpoint host, full body, PII, token name or actual account
  amounts;
- recorded only HTTP status, cache-header presence, JSON paths/types, public unit metadata and boolean
  semantic relations.

## Current implementation evidence

1. `src-tauri/src/domain/provider_account_usage.rs:255-291` builds NewAPI URL
   `/api/user/self`.
2. `src-tauri/src/commands/providers/account_usage.rs:99-114` sends the provider's model API Key as
   Bearer and optionally adds stored `New-Api-User`.
3. `src-tauri/src/domain/provider_account_usage.rs:423-442` reads `quota` before checking any
   `success` flag and emits `NewAPI 响应缺少 quota 字段`.
4. Lines 445-465 divide by a hardcoded 500000 and label USD without reading deployment display
   settings.

## Actual provider shape

Safe local metadata:

- direct `codex` API-key provider, enabled;
- one non-empty single `sk-` key (no list separators; value not exposed);
- one Base URL;
- adapter `newapi`, no configured `New-Api-User`, timed refresh 300 seconds.

Read-only results:

| Endpoint | Result | Safe shape/semantic evidence |
| --- | --- | --- |
| `/api/user/self` | HTTP 200 | 64-byte object with only `message:string`, `success:boolean`; `success=false`; message categories: invalid credential + unauthorized, no quota/user fields |
| `/api/status` | HTTP 200 | numeric `quota_per_unit=500000`; `quota_display_type=USD`; currency display enabled |
| `/api/usage/token/` | HTTP 500 | application error object `success/message`; no usage data; must not map to zero |
| `/v1/dashboard/billing/subscription` | HTTP 200 | numeric hard/soft/system limits and access time; boolean payment method |
| `/v1/dashboard/billing/usage` | HTTP 200 | numeric `total_usage` |

The latter two support a finite calculation `hard_limit_usd - total_usage / 100`. No inspected account
response included `Cache-Control`, `Age`, `ETag`, `Last-Modified` or `Expires`.

## Official NewAPI source contract

Evidence was read from immutable upstream source commit
`QuantumNous/new-api@a63364d156cf2a64f1c3d1ee4923d73d5f3222a1`:

- `router/api-router.go:80-84`: `/api/user/self` is under `middleware.UserAuth()`.
- `middleware/auth.go:43-67`: UserAuth validates an account access token; lines 96-114 require and
  validate `New-Api-User` after token validation.
- `model/user.go:959-965`: `ValidateAccessToken` queries the user `access_token`, not a model token.
- `router/dashboard.go:10-21`: billing subscription/usage routes use `middleware.TokenAuth()`.
- `controller/billing.go:11-68`: subscription returns total quota in configured display units.
- `controller/billing.go:71-107`: usage returns `total_usage = displayed used amount * 100`.
- `controller/misc.go:76-81`: public status exposes quota/display-unit metadata.
- `common/constants.go:62` and `model/option.go:557-558`: 500000 is a default and can be configured,
  so blindly hardcoding it is not a general response contract.

## Falsifiable root conclusion

The observed error is not a NewAPI schema variant hiding `quota`. It is an authentication-contract
mismatch, then a parser-order error that masks `success=false`. The conclusion is falsified only if
the exact saved model token and no user header produce a successful `/api/user/self` payload, which the
redacted live request disproved. The dashboard billing path is independently proven usable on the same
provider.

## Regression matrix

| Case | Expected |
| --- | --- |
| Live-shaped `success=false/message` | auth/query failure, never missing-quota/zero |
| USD subscription + usage | exact total, used/100, balance difference, expiry |
| CNY/TOKENS/unknown display type | explicit correct unit or fail closed; never mislabeled USD |
| Error object / missing / NaN / infinity | query failure, no partial snapshot |
| `/api/usage/token/` 500 | not zero and not required for success path |
| sub2api | unchanged |
| Live `muyuan` post-fix | only redacted field/type/formula assertions |
| Account query | no routing/circuit/availability mutation |

## Risk and rollback

Multiple endpoint reads increase failure surface, so snapshots must be all-or-nothing and body-bounded.
Deployment variance is handled by explicit error classification, not field guessing. No schema change is
needed; the adapter commit can be reverted without touching provider credentials.
