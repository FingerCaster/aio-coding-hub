# Prevent Codex capacity signal leakage

## Goal

Prevent Codex from stopping on an upstream capacity condition after the gateway
has already intercepted that condition for retry or Provider failover. A
client-facing terminal response must contain no text or protocol code that lets
Codex classify the response as a model-capacity error.

## Background

- Native Codex HTTP 200 SSE capacity failures are intercepted before downstream
  commit and enter the shared retry/failover engine
  (`src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:347`).
- The built-in capacity aliases are the legacy text
  `selected model is at capacity` and protocol codes `server_is_overloaded` /
  `slow_down` (`src-tauri/src/domain/usage.rs:131`).
- When all Providers fail, the gateway returns a standard 502 envelope, but
  client-visible verbose attempts are currently sanitized only for the legacy
  text (`src-tauri/src/gateway/proxy/errors.rs:30`). Code-only evidence can
  remain, and the replacement reason `upstream capacity failure` still exposes
  capacity semantics.
- Internal gateway events and request logs must continue retaining bounded,
  redacted evidence for diagnosis.
- New-user defaults currently duplicate built-in behavior: the `502`, `503`,
  and `504` HTTP rules overlap the gateway's native 5xx retry/failover path;
  the stream capacity keyword overlaps the built-in capacity recognizer; and
  the default non-retry stream keywords have the same forwarding disposition
  as unknown terminal stream errors.

## Requirements

- R1. For an intercepted pre-commit Codex capacity failure, no client-facing
  gateway error response field may contain the legacy capacity phrase,
  `server_is_overloaded`, `slow_down`, or a replacement phrase that explicitly
  identifies capacity/overload.
- R2. Capacity-bearing attempt reasons must be replaced with a generic
  transient-upstream-failure reason that does not disclose capacity semantics.
- R3. Capacity `stream_internal_error` evidence must be removed from the
  client-facing verbose `attempts` payload regardless of whether recognition
  came from message text, matched keyword, error type, error code, or event
  type.
- R4. The top-level terminal contract remains HTTP 502 with the existing
  gateway code such as `GW_FAKE_200`; retry, Provider switching, circuit
  accounting, and backoff behavior for intercepted capacity streams must not
  change.
- R5. Persisted request logs and internal events retain the existing bounded,
  redacted capacity evidence.
- R6. Sanitization is case-insensitive for all built-in capacity aliases.
- R7. New-user retry defaults must remove redundant HTTP status-only rules for
  `502`, `503`, and `504`. Native 5xx retry/failover behavior remains active;
  these failures use native circuit accounting instead of the first configured
  retry being exempt by default.
- R8. New-user Codex stream defaults must keep stream inspection enabled but
  start with empty retry and non-retry keyword lists. Built-in capacity
  recognition remains active without a visible keyword.
- R9. Keep the default HTTP 400 + `selected model is at capacity` content rule
  because a real HTTP 400 response does not traverse the HTTP 200 SSE capacity
  recognizer.
- R10. Keep the default connect, timeout, and read transport retry selections;
  they are behavior-bearing rather than display-only defaults.
- R11. Rust and TypeScript defaults must remain identical. Existing persisted
  global policies and Provider overrides must not be migrated or rewritten;
  the cleanup applies only when constructing a new/default policy.

## Acceptance Criteria

- [x] With verbose Provider errors enabled and code-only
  `server_is_overloaded`, the serialized client response contains none of
  `server_is_overloaded`, `selected model is at capacity`, `slow_down`,
  `capacity`, or `overload` (case-insensitive), while returning 502 /
  `GW_FAKE_200`.
- [x] The same assertion passes for code-only `slow_down` and for the legacy
  capacity message/matched-keyword path.
- [x] The corresponding internal attempt/request-log evidence still contains
  the original bounded error code or message.
- [x] Unrelated non-capacity attempts retain their existing verbose diagnostic
  fields.
- [x] A newly constructed default retry policy has exactly one HTTP rule: HTTP
  400 containing `selected model is at capacity`.
- [x] A newly constructed default stream policy is enabled with empty retry and
  non-retry keyword lists, while built-in capacity frames still retry or switch
  Providers as before.
- [x] Default transport retry selections remain connect, timeout, and read.
- [x] Existing explicitly persisted retry rules/keywords and Provider
  overrides survive reads and writes unchanged.
- [x] Focused Rust unit and route tests pass, followed by formatting, Clippy,
  and the repository's relevant quality checks.

## Out Of Scope

- Adding or removing UI controls, changing retry budgets, changing Provider
  selection order, or changing the built-in capacity recognizer.
- Rewriting an SSE stream after it has already been committed downstream,
  including guard-expiry and buffer-cap cases where transport safety requires
  preserving the original stream.
- Removing diagnostic evidence from internal logs.
- Migrating or silently deleting retry rules from existing users.

## Key Decisions

- This is a cross-layer hardening and default-cleanup change; `prd.md`,
  `design.md`, and `implement.md` define the implementation contract.
- The client response uses generic transient-failure wording. It must not use
  the word `capacity` or expose known overload aliases.
- Sanitization occurs only on the cloned client-facing attempts. Internal
  attempt records remain unchanged.
- Default cleanup removes `502`/`503`/`504` HTTP rows and all prefilled stream
  keywords, retains the HTTP 400 capacity row and transport retries, and does
  not alter existing stored configurations.

## Risks

- Removing the configured `502`/`503`/`504` rows means a new user's first 5xx
  failure participates in native circuit accounting. Retry count and Provider
  failover remain governed by the existing native 5xx path, but the circuit can
  accumulate a failure one attempt earlier than under the old default policy.
