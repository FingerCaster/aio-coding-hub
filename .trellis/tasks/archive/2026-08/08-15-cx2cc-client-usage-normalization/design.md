# Design: CX2CC client-visible usage normalization

## Boundary And Data Flow

The bridge has two usage views with different consumers:

```text
OpenAI Responses provider body/SSE
  |-- raw provider metrics --> quota, cost, events, request-log metric columns
  `-- CX2CC IR --> source-aware normalization --> Anthropic JSON/SSE client
                                             `--> request-log usage_json
```

Normalization belongs in the OpenAI Responses outbound parser because that is
the boundary that knows whether a cache value came from an OpenAI detail
object. The generic Anthropic renderer continues to serialize the IR without
guessing provider semantics.

## Client-Visible Normalization

Parse cache counters with their current compatibility aliases, but separately
identify inclusive OpenAI evidence only under these detail containers:

- `input_tokens_details`
- `prompt_tokens_details`

Recognized read evidence is `cached_tokens`. Recognized cache creation evidence
uses the existing detail-level cache creation/write aliases. Compute:

```text
client_input = provider_input
  saturating_sub(detail_cache_read_or_zero)
  saturating_sub(detail_cache_creation_aggregate_or_zero)
```

The cache creation amount is a single aggregate selected from detail aliases.
The 5-minute and 1-hour fields remain output metadata. They are not added to a
present aggregate for subtraction. Top-level or `cache_creation` fields remain
parseable and visible but do not independently prove inclusive OpenAI input
semantics.

## Raw Provider Preservation

### Non-streaming

Before translating a CX2CC response, parse provider usage directly from the
OpenAI Responses bytes using OpenAI semantics. Continue translating a separate
copy into client JSON/SSE. At request finalization, use raw provider usage for
the existing `usage_metrics` channel and translated usage for the existing
`usage`/`usage_json` channel. If raw provider usage is unavailable, the metric
channel falls back to translated metrics.

When synthesizing Anthropic SSE from provider JSON, `message_start` owns the
normalized input and all cache counters. `message_delta` owns output only. This
avoids repeating the same input/cache values across both Anthropic events.

### Streaming

The existing upstream observer sits before `BridgeStream` and already consumes
the original SSE bytes. For CX2CC, initialize its shared usage tracker with
OpenAI semantics and mark the finalization context to prefer that raw usage.
The downstream usage tracker remains after translation and continues to own
client-visible completion/error/empty-response evidence. Only the accounting
metric payload switches to the raw upstream extraction; `usage_json` remains
the translated client projection.

Stream request-end construction carries both channels into persistence:
`usage_metrics` supplies event/cost/token columns and `usage` supplies
`usage_json`. Infinite buffered CX2CC streams use a bridge-before observer and
send the same raw provider selection into the retry ledger, while retaining
the collected post-bridge bytes as the client projection.

This keeps stream topology, continuation repair, response fixing, plugins, and
relay behavior unchanged.

Shared cross-layer specification changes are intentionally owned by the parent
integration worktree to avoid duplicate `.trellis/spec` edits.

## Compatibility And Failure Behavior

- Invalid, negative, fractional, or non-numeric cache values retain existing
  ignore behavior.
- Excess cache counts saturate at zero and never fail response translation.
- Top-level cache-only Anthropic-compatible payloads keep their original input
  count.
- If pre-translation provider usage cannot be extracted, persistence falls back
  to the existing translated-response usage rather than dropping statistics.
- No database or configuration migration is required.

## Rollback

Revert the parser normalization and raw-usage selection together. Reverting
only raw preservation would make billing/log views consume the normalized
client count; reverting only normalization would restore Claude Code's cached
token double count.
