# Design: model-token NewAPI billing adapter

## Protocol flow

For a configured NewAPI provider, normalize the base URL to its deployment root and use one bounded
HTTP client:

```text
public GET /api/status --------------------------> display unit
Bearer GET /v1/dashboard/billing/subscription -> total + expiry + payment method
Bearer GET /v1/dashboard/billing/usage          -> used amount in cents-of-display-unit
                                                  normalize -> ProviderAccountUsageResult
```

Status and subscription may run together. Usage date parameters depend on subscription compatibility:
use month start when `has_payment_method`, otherwise the established 100-day window, ending today.

## Parsing contract

- Require successful HTTP status and bounded valid JSON for each endpoint.
- Recognize application errors before success fields:
  - `success == false` -> sanitized auth/query failure;
  - root `error` -> query failure;
  - absent or non-finite required numeric fields -> invalid response.
- For `quota_display_type == USD`:
  - `total = hard_limit_usd`
  - `used = total_usage / 100`
  - `balance = total - used`
  - `unit = USD`
- Other documented display types may be mapped only when their source semantics are explicitly
  implemented and tested. Unknown values fail closed rather than inheriting USD.
- `access_until <= 0` means no expiry; positive values use existing timestamp/status logic.

The endpoint's field names are compatibility names. Do not additionally divide billing values by
`quota_per_unit`; NewAPI already performs that conversion before producing billing fields.

## Compatibility

- Keep sub2api request and parser unchanged.
- Keep current DTO fields so generated TypeScript bindings need change only if a new diagnostic field
  is genuinely required.
- Preserve the stored `newApiUserId` value without using it to authenticate the model-token billing
  path. Do not silently delete existing user configuration in this bug fix.
- `/api/usage/token/` is not used as a required fallback because live evidence returned 500.

## Security

- API Key remains loaded only in Rust immediately before requests and is redacted from every error.
- Enforce a small explicit response-body cap on status/subscription/usage reads.
- Require all derived endpoints to retain the normalized base URL's origin; no response-provided URL
  or redirect target becomes a credential destination.
- Tests and research use synthetic values only.

## Risks And Rollback

- **Extra calls/latency:** two authenticated reads plus status can fail independently. Fail the whole
  snapshot rather than mix stale/partial values; timed refresh remains bounded by existing interval.
- **Deployment variance:** older NewAPI variants may shape billing errors differently. Preserve raw
  bodies only in memory and classify conservatively.
- **Unit ambiguity:** non-USD must not be labeled USD. Unknown status is a visible failure.
- Rollback is confined to the NewAPI adapter/parser and fixtures; no DB migration or credential change.
