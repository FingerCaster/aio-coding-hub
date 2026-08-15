# Bug Analysis: CX2CC Client Usage Double Counting

## Bug Summary

OpenAI Responses reports inclusive `input_tokens` and nested cache detail as a
subset. CX2CC previously projected both values directly into Anthropic usage,
so Claude Code could count cached input twice and compact at roughly half of the
configured context window. The same translated usage also fed some accounting
paths, making a parser-only subtraction unsafe for quota, cost, and logs.

## Root Cause Categories

- **B - Cross-Layer Contract**: one usage value was treated as both a provider
  accounting fact and a client protocol projection.
- **D - Test Coverage Gap**: parser fixtures did not prove raw log/cost values
  and client-visible values simultaneously across stream and non-stream paths.
- **E - Implicit Assumption**: code assumed OpenAI and Anthropic input/cache
  buckets had identical exclusivity semantics.

## Why A Surface Fix Would Fail

Subtracting cache tokens in the protocol parser fixes Claude Code's visible
count, but replacing the provider usage everywhere would make Codex cost and
usage aggregation subtract cache a second time. That can undercount or saturate
accounting to zero. Fixing only real SSE would also leave non-stream and
synthesized SSE behavior inconsistent, including duplicate input/cache emission
in both start and delta events.

## Prevention Mechanisms

| Mechanism | Enforcement |
| --- | --- |
| Typed ownership | Keep raw `UsageMetrics` separate from client `UsageExtract` through request finalization |
| Boundary normalization | Subtract only nested OpenAI cache detail while translating to Anthropic |
| Provider preservation | Feed raw inclusive metrics to quota, cost, token columns, realtime events, and provider ledger |
| SSE event ownership | Start owns input/cache; delta owns output |
| Cross-path tests | Assert non-stream, real SSE, synthesized SSE, and already-Anthropic payloads |
| Paired assertions | In one integration test, assert raw persisted columns and normalized `usage_json` independently |

## Systematic Expansion

- Audit every protocol bridge that maps an inclusive total into exclusive
  buckets; do not infer semantics from field names alone.
- Audit stream observers for destructive finalization when multiple consumers
  share one tracker.
- Audit cost/statistics code before changing persisted token semantics, because
  some provider families intentionally subtract cached subsets downstream.
- Require new provider usage fields to declare whether they are totals, subsets,
  or mutually exclusive buckets.

## Knowledge Capture

- Added executable usage ownership, validation cases, tests, and wrong/correct
  examples to the CX2CC routing contract.
- Added a reusable usage projection checklist to the project guide and its
  generated template.
- Updated the cross-layer index so future CX2CC changes load this contract
  before implementation and quality review.
