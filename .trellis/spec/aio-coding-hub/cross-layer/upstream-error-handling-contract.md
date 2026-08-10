# Upstream Error Handling Contract

## Scenario: Change Upstream Retry Or Final Error Presentation

### 1. Scope / Trigger

Use this contract when changing configured upstream retry rules, native Codex
or normalized Codex Responses SSE terminal handling, final upstream HTTP error
rewriting, their settings, or their request-log projections. Retry, stream
firewall, and rewrite share one product entry, but remain separate execution
phases. The stream firewall applies to the final Codex Responses wire format;
non-Codex and unnormalized bridge streams keep their existing behavior.

### 2. Signatures

- Retry settings save patch:
  `{ upstream_retry_policy: UpstreamRetryPolicy, stream_internal_error_guard_ms: u32 }`.
- Final HTTP rewrite save patch:
  `{ upstream_error_response_rules: UpstreamErrorResponseRule[] }`.
- Stream policy runtime shape:
  `{ enabled: boolean, passthrough_keywords: string[], legacy_retry_keywords: string[] }`.
  `legacy_retry_keywords` is hidden migration state for one compatibility
  version; it is not a public editing surface.
- Classifier:
  `classify_codex_stream_internal_error(event, data, enabled,
  passthrough_keywords, legacy_retry_keywords, allow_legacy_retry,
  disposition) -> Option<StreamInternalErrorEvidence>`.
- Post-commit filter:
  `CodexTerminalFirewall::ingest(&mut self, bytes) -> TerminalFirewallOutput`
  and `finish(&mut self) -> TerminalFirewallOutput`.
- Sanitized pre-commit terminal failure: HTTP `502` with
  `error_code = "GW_FAKE_200"`; request logs retain bounded, redacted
  `stream_internal_error` evidence.

### 3. Contracts

- The settings UI exposes one `Upstream Error Handling` section with segmented
  `Retry Rules` and `Final HTTP Error Rewrite` modes. Retry edits persist
  `upstream_retry_policy` and `stream_internal_error_guard_ms` in one save;
  rewrite edits persist only `upstream_error_response_rules`. A rule never
  combines retry and rewrite actions.
- Resolve the effective retry policy, including a complete Provider override,
  before reserving attempts. HTTP, transport, and retryable pre-commit Codex
  stream classifications share that policy's `max_retries`, `backoff_ms`,
  circuit accounting, cooldown, and failover path; they do not add independent
  budgets.
- New retry-policy defaults expose only the behavior-bearing HTTP 400 capacity
  content rule. Native 5xx handling does not require visible `502` / `503` /
  `504` status-only rows. Stream terminal handling defaults to enabled with
  `high-risk cyber` as its sole passthrough exception. An explicitly persisted
  empty list stays empty. Connect, timeout, and read transport retries remain
  selected by default.
- `stream_internal_errors.enabled` is the sole master switch for every new
  terminal action. Missing/new values default to `true`; an explicit persisted
  `false` is preserved. `true` enables classification, pre-commit retry or
  Provider switch, client filtering, and passthrough exceptions. `false`
  performs no interception, retry, switch, rewrite, or frame drop, including
  for capacity. The raw tracker may still record passive evidence with
  `disabled_passthrough`.
- A configured same-Provider retry waits once through the common backoff helper
  after the final decision is `RetrySameProvider`. Provider switches, aborts,
  and circuit-open rewrites add no wait.
- Terminal handling applies to the final Codex Responses SSE emitted by
  `/v1/responses`, `/responses`, and `/v1/codex/responses`, including a bridge
  already normalized to that wire format and third-party HTTP 200 responses
  containing terminal SSE errors. Recognize only terminal `error`,
  `response.error`, `response.failed`, and `response.incomplete` event/data
  types and extract evidence only from known envelope fields.
- Metadata does not commit the downstream response. The guard starts at the
  first real text, refusal, reasoning summary, tool arguments, or concrete
  output. The setting is `0..=5000` ms with default `500`; the buffered prefix
  is capped at 1 MiB per request.
- Classification priority is hard non-retry (`auth`, `invalid_request`,
  `quota`, `policy`), then `transient_capacity` / `transient_provider`, then a
  valid passthrough exception, then the hidden legacy override, then `unknown`.
  Built-in transient fixtures include capacity/overload aliases,
  `server_is_overloaded`, `slow_down`, `service_unavailable_error`, and
  structured `server_error`. A passthrough keyword is a post-commit projection
  exception only: it may expose an explicitly matched policy/unknown terminal
  frame, but never overrides capacity or sensitive `auth`, `quota`, or
  `invalid_request` categories. The hidden legacy list may upgrade only a
  pre-commit `unknown`; it cannot override a hard category and cannot act
  post-commit.
- Before downstream commit, transient classifications discard the buffered
  prefix and use the shared retry/failover path only when the stream master
  switch is enabled. Hard and unknown classifications return the standard
  sanitized `502/GW_FAKE_200` envelope without fabricating an SSE terminal
  frame, consuming a stream retry, opening a circuit, or switching Provider.
- After downstream commit, the raw tracker consumes bytes before the firewall.
  The firewall buffers at most one complete SSE frame (1 MiB), preserves exact
  normal-frame bytes and LF/CRLF boundaries, and supports split chunks and
  multiple frames per chunk. By default it drops the complete terminal error
  frame and all later bytes, then ends downstream. A valid passthrough exception
  forwards that complete terminal frame unchanged, then ends. Malformed JSON,
  invalid UTF-8, partial EOF tails, and oversized frames fail closed. Forward
  `response.completed` at most once and preserve its order with `[DONE]`.
- Guard expiry and prefix-cap release commit one attempt only; the post-commit
  firewall still protects subsequent terminal frames. Never splice output from
  another Provider after commit. Preserve the existing 20 MiB aggregate guard,
  TTFB accounting, response ID, usage, completion, continuation repair, and
  downstream-abort behavior.
- If all Providers fail on retryable pre-commit stream errors, return the
  existing standard HTTP 502 / `GW_FAKE_200` terminal envelope instead of the
  original HTTP 200 stream. Client-facing top-level messages, attempt reasons,
  and verbose stream evidence contain no upstream terminal message, code, type,
  or matched keyword. Internal events/request logs retain bounded, redacted
  evidence.
- Settings schema is `59`. Reads merge old `non_retry_keywords` into
  `passthrough_keywords` and old `retry_keywords` into hidden
  `legacy_retry_keywords`; canonical writes emit neither old field. When both
  the canonical passthrough field and its legacy alias are missing, reads use
  the `high-risk cyber` default; either field being explicitly present,
  including as an empty list, preserves the persisted choice. Provider shares
  read v1-v3 and export strict v4. Unknown/future versions and old field names
  inside v4 are explicitly rejected so older clients cannot silently discard
  the new semantics.
- The persisted global `AppSettings.upstream_retry_policy` decoder is a
  compatibility boundary: additive future policy and stream-policy fields are
  ignored while known valid fields are retained, and malformed content falls
  back to the default policy. The direct `UpstreamRetryPolicy` decoder used by
  Provider overrides and strict share/import wire formats remains strict;
  global forward compatibility must not weaken those boundaries.
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
| Master switch disabled; any terminal class | Return the original bytes unchanged; no stream retry, Provider switch, rewrite, or drop |
| Transient/capacity before commit; master enabled | Discard buffer and use shared retry/backoff/circuit/failover state |
| Hard or unknown before commit; master enabled | Return sanitized `502/GW_FAKE_200`; passthrough words do not apply and no fake SSE, retry, or Provider switch |
| Legacy keyword matches a pre-commit unknown | Mark `legacy_retry_override` and use shared transient routing; never override hard errors |
| Capacity also matches passthrough keyword | Capacity wins; never forward the terminal frame |
| Terminal error after commit | Drop the complete terminal frame and end; retain raw evidence internally |
| Valid post-commit passthrough exception | Forward the complete original terminal frame once, then end |
| Partial, malformed, invalid UTF-8, or >1 MiB frame | Fail closed without forwarding unclassified tail bytes |
| Normalized bridge emits Codex terminal frame | Apply the same classifier/firewall as native final wire |
| Non-Codex or unnormalized bridge stream | Preserve existing protocol behavior |
| All Providers end in retryable capacity streams | Return standard 502 / fake-200 envelope; keep evidence internal and remove every capacity text/code signal from client diagnostics |
| Final HTTP rewrite parsing fails | Preserve the existing terminal HTTP response |
| Provider has an explicit complete override | Use only that override; do not append global rules |
| Provider share v4 has an old/unknown field or future version | Reject explicitly; never import a lossy partial policy |

### 5. Good / Base / Bad Cases

- Good: an early capacity text or protocol-code frame retries or switches, a
  later Provider succeeds, and no terminal evidence reaches the client.
- Base: visible output commits, a split CRLF terminal frame arrives later, the
  raw tracker records bounded evidence, and the client receives only the prior
  frames before clean downstream termination.
- Bad: a disabled firewall still intercepts capacity, a hard error is upgraded
  by a legacy word, an unknown tail leaks on parse failure, or a normalized
  bridge bypasses the final-wire firewall.

### 6. Tests Required

- Persistence tests assert missing master switch defaults `true`, explicit old
  `false` survives schema 59 migration, both old keyword fields migrate to the
  correct new field, canonical writes omit old names, and invalid writes remain
  bounded and strict.
- Provider-share tests read v1-v3, export deterministic strict v4, round-trip
  passthrough/legacy fields, and explicitly reject future versions, v4 old field
  aliases, and unknown nested fields.
- Route tests cover final rewrite, intermediate failure then success,
  multi-Provider failure, transport exclusion, protocol envelopes, safe headers,
  client/attempt status separation, probe finalization, and fake-200.
- Codex route tests cover native and normalized bridge final-wire gating, event
  and data types, evidence redaction, structured hard/transient/unknown priority,
  capacity aliases across known envelopes, pre-commit retry/failover, hard and
  unknown sanitized 502, master-disabled raw capacity/unknown passthrough, and
  third-party HTTP 200 embedded terminal frames.
- Relay tests assert raw-tracker-before-filter ordering, exact split/multi-frame
  and LF/CRLF bytes, complete-frame passthrough, default drop, malformed/
  oversized/partial fail-closed behavior, downstream abort, one completed frame,
  `[DONE]` order, usage, response ID, 20 MiB limit, and no content duplication.
- Default-policy tests keep Rust and TypeScript aligned: only the HTTP 400
  capacity content rule is prefilled, passthrough and hidden legacy lists are
  empty, the stream master switch is enabled, and the three transport retry
  kinds remain selected.
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
// Wrong: capacity has a hidden interception path outside the master switch.
if !stream_policy.enabled && is_capacity(frame) {
    return provider_failure(frame);
}

// Correct: disabled means a complete bypass for every terminal class.
if !stream_policy.enabled {
    return start_streaming(original_bytes);
}
return classify_then_apply_terminal_action(frame);
```

```rust
// Wrong: parse chunks independently and leak a split or malformed terminal tail.
relay(chunk_without_complete_frame_check);

// Correct: track raw bytes first, then filter bounded complete SSE frames.
raw_tracker.ingest(chunk);
let visible = terminal_firewall.ingest(chunk);
relay(visible.bytes);
```
