# Upstream Error Handling Contract

## Scenario: Change Upstream Retry Or Final Error Presentation

### 1. Scope / Trigger

Use this contract when changing configured upstream retry rules, native Codex
Responses SSE recovery, final upstream HTTP error rewriting, their settings,
or their request-log projections. Retry and rewrite share one product entry,
but they remain separate persisted schemas and execute at different phases.

### 2. Signatures

- Retry settings save patch:
  `{ upstream_retry_policy: UpstreamRetryPolicy, stream_internal_error_guard_ms: u32 }`.
- Final HTTP rewrite save patch:
  `{ upstream_error_response_rules: UpstreamErrorResponseRule[] }`.
- Terminal pre-commit capacity failure: HTTP `502` with
  `error_code = "GW_FAKE_200"`; request logs retain bounded
  `stream_internal_error` evidence.

### 3. Contracts

- The settings UI exposes one `Upstream Error Handling` section with segmented
  `Retry Rules` and `Final HTTP Error Rewrite` modes. Retry edits persist
  `upstream_retry_policy` and `stream_internal_error_guard_ms` in one save;
  rewrite edits persist only `upstream_error_response_rules`. A rule never
  combines retry and rewrite actions.
- Resolve the effective retry policy, including a complete Provider override,
  before reserving attempts. HTTP, transport, and pre-commit Codex stream
  matches share that policy's `max_retries`, `backoff_ms`, and
  `counts_toward_circuit_breaker`; they do not add independent budgets.
- A configured same-Provider retry waits once through the common backoff helper
  after the final decision is `RetrySameProvider`. Provider switches, aborts,
  and circuit-open rewrites add no wait.
- Native Codex stream recovery applies only to unbridged `/v1/responses`,
  `/responses`, and `/v1/codex/responses` requests. Recognize only terminal
  `error`, `response.error`, `response.failed`, and `response.incomplete`
  event/data types and extract evidence only from known envelope fields.
- Metadata does not commit the downstream response. The guard starts at the
  first real text, refusal, reasoning summary, tool arguments, or concrete
  output. The setting is `0..=5000` ms with default `500`; the buffered prefix
  is capped at 1 MiB per request.
- Before downstream commit, a retry-keyword match may discard the buffered
  prefix and enter the configured retry engine. Positive retry keywords win
  over non-retry keywords; matching is case-insensitive literal text. A
  terminal `selected model is at capacity` match is always intercepted before
  commit even when configured retries are disabled: enabled matching may retry,
  while disabled matching switches Provider directly. At guard expiry or
  buffer cap, or after downstream commit, preserve the original SSE without
  splicing output from another attempt. Buffer-cap release is diagnostic and
  is not a Provider failure.
- If all Providers fail on retryable pre-commit stream errors, return the
  existing standard HTTP 502 / `GW_FAKE_200` terminal envelope instead of the
  original HTTP 200 capacity stream. Client-facing verbose attempts must remove
  the capacity message and matched keyword; internal events/request logs retain
  the bounded, redacted evidence.
- Final response rewriting considers only the terminal upstream HTTP 4xx/5xx
  candidate after retry, failover, quota, cooldown, and circuit decisions use
  the real upstream facts. HTTP 200 stream errors and transport errors never
  enter rewrite matching. Any read, match, or envelope-construction failure
  fails open to the existing terminal response.
- Rewrite matching supports priority, Any/All, status codes,
  case-insensitive literal keywords, CLI and Provider scope, enablement, and
  independent passthrough/override behavior for status and message. Build a
  protocol-compatible Claude, Codex/Grok, or Gemini error envelope and preserve
  safe `Retry-After` and `x-trace-id` headers. Lower priority numbers run first
  and only the first match applies. Any means status-group OR keyword-group;
  All means status-group AND keyword-group; values inside each group use OR.
  After CLI scope changes, clear known selected Providers outside that scope;
  preserve unknown/deleted IDs until the user explicitly removes them.
- Request-log status is client-visible; attempt status remains the real
  upstream status. Rewrite audit metadata contains only bounded rule identity,
  Provider identity, before/after status, and behavior modes. Stream evidence
  contains bounded/redacted event, type, code, message, classification,
  matched keyword, disposition, and truncation state. Never persist response
  bodies, raw SSE, rule keywords, rewrite messages, Bearer values, API keys, or
  access tokens.
- A later success clears terminal rewrite candidates. Stream retries retain
  bounded early evidence in the Provider chain; final failure projects the
  last evidence. Preserve circuit probe/failback, health-neutral helper routes,
  strict auxiliary budgets, forced Provider routing, count-tokens, Codex
  continuation repair, fake-200 behavior, and Codex request-body decoding.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| HTTP failure matches retry and later succeeds | Return success; no rewrite audit |
| Final HTTP 4xx/5xx matches rewrite | Rewrite only the client envelope/status; keep upstream attempt status |
| Transport failure or HTTP 200 SSE error | Never evaluate final HTTP rewrite rules |
| Retryable Codex error before commit | Discard buffer and use the shared configured retry budget/backoff |
| Capacity error before commit with retry disabled | Do not forward SSE; switch Provider, then return gateway 502 if exhausted |
| Codex error after commit, guard expiry, or cap | Forward one original SSE stream; never splice attempts |
| All Providers end in retryable capacity streams | Return standard 502 / fake-200 envelope; keep evidence internal and remove capacity text from client diagnostics |
| Rule/evidence parsing fails | Preserve existing behavior without leaking raw content |
| Provider has an explicit complete override | Use only that override; do not append global rules |

### 5. Good / Base / Bad Cases

- Good: an early capacity frame retries or switches, a later Provider succeeds,
  and no capacity text reaches the client.
- Base: all Providers return early capacity frames; the client receives the
  standard `502/GW_FAKE_200` envelope and logs retain bounded evidence.
- Bad: disabling stream retries forwards the original capacity SSE, or verbose
  client attempts serialize the retained capacity message/matched keyword.

### 6. Tests Required

- Persistence tests cover strict writes, lossy per-rule reads, schema/database
  migration, global/Provider override intent, share/import/backup, and unrelated
  field preservation.
- Route tests cover final rewrite, intermediate failure then success,
  multi-Provider failure, transport exclusion, protocol envelopes, safe headers,
  client/attempt status separation, probe finalization, and fake-200.
- Native Codex route tests cover exact path/bridge gating, event and data types,
  evidence redaction, positive-keyword precedence, unknown/non-retry forwarding,
  pre-commit retry success, retry-disabled capacity interception, client
  diagnostic sanitization, all-Provider failure, guard expiry, 1 MiB cap, and
  downstream-commit behavior.
- Use paused time for guard and backoff boundaries. Assert same-Provider waits
  exactly once and Provider switching waits zero.
- Frontend tests cover segmented mode switching, retry/rewrite save ownership,
  observation-window validation, CLI/Provider scope filtering, dialog CRUD,
  longest legal text, mobile-width containment, and log-hit badges.
- Regenerate bindings and run full Rust tests, frontend unit tests, typecheck,
  lint, production build, Rust format, check, and strict Clippy.

### 7. Wrong vs Correct

```rust
// Wrong: rewrite changes facts before retry/failover/circuit accounting.
let visible = rewrite(upstream_response);
record_failure(visible.status());

// Correct: finish routing and accounting with upstream facts, then rewrite the
// one terminal client response candidate.
record_failure(upstream_response.status());
let visible = rewrite_terminal_candidate(upstream_response).unwrap_or_original();
```

```typescript
// Wrong: one UI rule controls both execution phases.
type Rule = { retry: boolean; rewriteStatus?: number };

// Correct: one product section, separate execution phases and save ownership.
persist({
  upstream_retry_policy: retryPolicy,
  stream_internal_error_guard_ms: guardMs,
});
persist({ upstream_error_response_rules: rewriteRules });
```

```rust
// Wrong: disabling retries opts back into forwarding an early capacity frame.
if !policy.enabled { return start_streaming(raw_capacity_sse); }

// Correct: retry enablement changes retry vs switch, never the pre-commit
// capacity interception boundary.
return provider_failure(GatewayErrorCode::Fake200, bounded_evidence);
```
