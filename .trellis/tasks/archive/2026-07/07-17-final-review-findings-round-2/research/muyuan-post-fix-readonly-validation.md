# NewAPI `muyuan` post-fix read-only validation

## Authorization and privacy boundary

The user authorized a minimum post-fix read-only validation using the locally configured `muyuan`
provider. The validation loaded the Base URL and model API key into process memory only, issued GET
requests only, and did not print or persist the host, URL query, key/token, response body, account
identifier, PII, token name, or any account amount.

## Request contract

Validated at `2026-07-17T09:24:42.4230091+08:00`:

| Request | Authentication | Safe result |
| --- | --- | --- |
| public status | none | `2xx`; public display unit `USD`; quota-unit field numeric |
| billing subscription | Bearer model token | `2xx`; hard-limit field numeric; payment-method field boolean |
| billing usage for current month | Bearer model token | `2xx`; total-usage field numeric |

The command disabled automatic redirects and used a 15-second timeout for each request. It constructed
the same status/subscription/usage endpoint family and date-query shape used by the implementation.

## Assertions

- Request count: 3 GETs.
- All three status classes: `2xx`.
- Required numeric field types: true.
- `has_payment_method` boolean type: true.
- All source and derived numeric values finite: true.
- Formula assertion `used = total_usage / 100` and `balance = hard_limit_usd - used`: true.
- Cache-header presence across `Cache-Control/Age/ETag/Last-Modified/Expires`: false.
- Overall result: **PASS**.

Two earlier attempts in the same session also completed the three GETs but failed only while formatting
the redacted local summary (PowerShell formatting/spacing errors). They emitted no host, credential,
body, query, account value, or PII and made no remote mutation.
