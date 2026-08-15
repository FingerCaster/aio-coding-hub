# Normalize CX2CC client-visible usage

## Goal

Prevent Claude Code from double-counting cached prompt tokens in CX2CC
responses. OpenAI Responses reports inclusive `input_tokens`, while Anthropic
clients expect non-cached `input_tokens` to be mutually exclusive with cache
read and cache creation counters.

## Background

- The CX2CC bridge translates OpenAI Responses JSON and SSE into Anthropic
  Messages JSON and SSE.
- OpenAI cache details can report cached reads and cache writes inside
  `input_tokens_details` or `prompt_tokens_details`; those details are part of
  the inclusive OpenAI input total.
- Anthropic-style top-level cache fields may already accompany an exclusive
  input total and therefore are not evidence that subtraction is required.
- Provider usage remains authoritative for quota, cost, and request logs;
  client-visible normalization must not overwrite that raw accounting view.

## Requirements

1. For CX2CC OpenAI Responses usage, subtract cache read and cache creation
   counts from client-visible `input_tokens` only when the corresponding value
   came from a recognized OpenAI detail object.
2. Use saturating unsigned arithmetic so malformed or excessive cache values
   cannot underflow.
3. Treat a detail-level cache creation aggregate as the amount to subtract.
   Preserve 5-minute and 1-hour breakdown fields, but do not add them to an
   aggregate and subtract the same cache creation twice.
4. Preserve existing parsing and client projection of cache counters, including
   top-level Anthropic-style fields. Top-level cache fields alone must not
   trigger input normalization.
5. Preserve the complete pre-translation provider metrics for quota, cost,
   event fields, and request-log metric columns in both non-streaming and
   streaming response paths. Keep translated client usage as the request-log
   `usage_json` projection.
6. For JSON-to-SSE synthesis, emit input/cache usage only in Anthropic
   `message_start` and output usage only in `message_delta`; true upstream SSE
   retains its existing event placement.
7. Keep client-visible terminal detection based on the translated response and
   do not alter response-fixer, continuation repair, retry, or failover
   behavior.
8. Document the raw/provider and client-visible usage views in the directly
   related CX2CC cross-layer specification.

## Acceptance Criteria

- [x] A cached-read detail changes client-visible usage from inclusive input to
      non-cached input while preserving `cache_read_input_tokens`.
- [x] A cache-write detail changes client-visible input and preserves aggregate
      and 5-minute/1-hour breakdown fields without double subtraction.
- [x] Cache values greater than input saturate client-visible input at zero.
- [x] Usage with no cache details is unchanged.
- [x] Anthropic-style top-level cache fields do not cause a second subtraction.
- [x] Non-streaming Anthropic JSON and synthesized Anthropic SSE expose the
      normalized client view.
- [x] True streaming Anthropic SSE exposes the same normalized client view.
- [x] Synthesized SSE does not repeat input/cache usage in `message_delta`.
- [x] Quota, cost, events, and log metric columns receive the inclusive
      pre-translation provider input and its cache counters for both
      non-streaming and streaming responses, while `usage_json` retains the
      client-visible projection.
- [x] Existing CX2CC continuation repair behavior is unchanged.
- [x] Focused Rust tests, `cargo fmt --check`, relevant Clippy, and
      `git diff --check` pass.

## Out of Scope

- Changing provider billing formulas, quota policy, or persisted schemas.
- Normalizing non-CX2CC protocols or ambiguous top-level cache-only payloads.
- Changing request translation, model routing, continuation repair, retry, or
  failover behavior.
