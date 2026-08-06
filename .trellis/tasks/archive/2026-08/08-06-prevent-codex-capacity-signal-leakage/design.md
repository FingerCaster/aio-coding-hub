# Design

## Client Capacity-Signal Sanitization

Keep capacity recognition and internal evidence unchanged. Before serializing
gateway error attempts to the client:

1. Inspect every string-bearing field of `stream_internal_error` that can carry
   a recognized capacity signal: event type, error type, error code, message,
   and matched keyword.
2. Match the legacy phrase and built-in codes case-insensitively.
3. Remove the complete `stream_internal_error` evidence object from the cloned
   client attempt when a capacity signal is found.
4. Replace any capacity-bearing attempt reason with a neutral transient
   upstream failure reason. Do not use `capacity` or `overload` in the
   replacement.

The original attempt remains unchanged for internal events and persisted
request logs.

## Default Policy Cleanup

Update the Rust source-of-truth defaults and their TypeScript mirror together:

- HTTP rules: retain only HTTP 400 containing
  `selected model is at capacity`.
- Stream policy: remain enabled with empty retry and non-retry keyword lists.
- Transport retries: retain connect, timeout, and read.
- Retry budget/backoff/circuit toggle: retain current values.

Do not add a schema migration. Explicit stored global policies and Provider
overrides retain their current arrays. New settings and missing-field fallback
use the cleaned defaults.

## Validation

- Unit-test client sanitization for message, matched keyword, error type, event
  type, `server_is_overloaded`, and `slow_down` cases.
- Route-test a terminal code-only capacity response with verbose errors enabled:
  the HTTP response contains no capacity signal while its request log retains
  evidence.
- Assert Rust and TypeScript default-policy shapes.
- Preserve a non-capacity verbose-attempt control case.
