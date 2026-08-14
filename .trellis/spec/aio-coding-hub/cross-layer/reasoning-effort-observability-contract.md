# Reasoning Effort Observability Contract

## Scenario: Observe The Explicit Effort Sent Upstream

### 1. Scope / Trigger

Use this contract when changing gateway request transformation, protocol
bridging, failover attempt evidence, request-log projections, generated
bindings, or the Home request-log reasoning-effort badge. The observed value is
diagnostic evidence about one concrete upstream attempt; it is not a model
default, a configured preference, or an inference from thinking being enabled.

### 2. Signatures

Each persisted and realtime attempt carries the final explicit value together
with independent send evidence:

```rust
pub struct FailoverAttempt {
    pub reasoning_effort: Option<String>,
    pub upstream_sent: bool,
    // existing provider, outcome, status, and timing fields omitted
}

#[derive(Deserialize)]
struct AttemptRow {
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    upstream_sent: bool,
}
```

The completed realtime request event and both historical request-log DTOs
project one request-level value:

```rust
pub reasoning_effort: Option<String>
```

The final selector consumes ordered attempt tuples of
`(outcome, reasoning_effort, upstream_sent)`. It returns the value, including
`None`, from the last successful attempt; if no attempt succeeded, it returns
the value from the last sent attempt. Unsent attempts do not participate.

### 3. Protocol Sources

Read only a trimmed, non-empty JSON string from the final semantic outbound
body and the final outbound path/mode:

| Final outbound protocol | Explicit field |
| --- | --- |
| OpenAI Responses, including CX2CC | `reasoning.effort` |
| OpenAI Chat Completions | `reasoning_effort` |
| Anthropic Messages | `output_config.effort` |
| Gemini generateContent | `generationConfig.thinkingConfig.thinkingLevel` |
| Wrapped Gemini OAuth generateContent | `request.generationConfig.thinkingConfig.thinkingLevel` |

Gemini countTokens has no effort observation. Preserve any future non-empty
string value, within the shared 64-character diagnostic bound, rather than
validating against a closed enum. Never derive effort from `thinking.type`,
`budget_tokens`, `thinkingBudget`, model capabilities, provider settings, or a
default catalog value.

### 4. Contracts

- Extract after protocol bridging, provider-specific request configuration, and
  request mutation have produced the semantic body for the selected attempt.
  Use the final forwarded path. Observe before content encoding can make the
  JSON body opaque; do not inspect the original client request.
- Store the extracted value on that attempt even when the transport later
  fails. Keep `upstream_sent = false` for preparation, authentication, gate,
  and connection failures that did not send the request; set it only from the
  transport boundary evidence already owned by the send path.
- A timeout or terminal response after transport dispatch remains sent
  evidence. Do not infer sending from attempt outcome, status text, or the mere
  existence of an attempt row.
- Historical JSON written before these fields existed must deserialize as
  `reasoning_effort = None` and `upstream_sent = false`. No schema migration or
  fabricated value is required.
- Realtime request completion and historical summary/detail queries use the
  same final selector. The selected attempt is authoritative even when its
  explicit effort is absent; do not backtrack to an earlier non-null attempt.
- Attempt events expose per-attempt evidence. Completed request events and
  request-log DTOs expose only the selected final value. Keep string bounding
  consistent with the existing event diagnostic limits.
- Regenerate TypeScript bindings after changing Rust DTOs. Frontend event
  normalizers must tolerate legacy payloads that omit the new fields and
  normalize them to `null` and `false`.
- Home historical rows, realtime cards, and request detail use one shared
  reasoning-effort badge. An observed value wins for every protocol. Codex may
  use its existing special-settings resolver only when an observed value is
  absent; never render the observed badge and the Codex fallback together.
- An absent observation renders no badge for Claude/CX2CC. An absent Codex
  observation renders no badge when the compatibility resolver is unknown.
- This observability path must not mutate CX2CC model routing, its four-slot
  mapping, thinking presence, effort precedence, service tier, storage setting,
  or response translation.

### 5. Data Flow

```text
final outbound path + semantic JSON body
  -> protocol-aware explicit-field extractor
  -> attempt reasoning_effort + upstream_sent
  -> attempts JSON + realtime attempt event
  -> shared last-success / last-sent selector
  -> historical summary/detail + completed request event
  -> generated binding + legacy-tolerant frontend adapter
  -> one shared badge (observed first, Codex fallback second)
```

### 6. Tests Required

- Extractor tests for Responses, Chat Completions, Messages, direct Gemini,
  wrapped Gemini OAuth, countTokens, empty/non-string fields, thinking/budget
  false positives, and a future explicit string.
- A CX2CC integration test that translates an Anthropic request through the
  real bridge, then observes the resulting `/responses` body. Keep the existing
  routing and thinking-passthrough regressions passing.
- Selector tests for a later success, an all-failed chain, a final unsent
  attempt, a selected attempt with no explicit effort, and legacy attempt JSON.
- Shared event fixture tests for per-attempt and completed-request payloads,
  including missing-field normalization on the frontend.
- Historical row, realtime card, and detail tests for observed-value priority,
  Codex fallback, unknown/absent values, and exactly one rendered badge.
- Run Rust format and targeted library tests, generated-bindings verification,
  frontend typecheck/lint/tests, and the repository pre-push gate.

### 7. Wrong vs Correct

```rust
// Wrong: client intent is not evidence about the final upstream request.
let effort = original_body.pointer("/output_config/effort");

// Correct: observe the selected attempt after every outbound transformation.
let effort = extract(final_semantic_body, final_forwarded_path, oauth_mode);
```

```ts
// Wrong: the Codex fallback duplicates a final observed badge.
return <>{observedBadge}{codexFallbackBadge}</>;

// Correct: one resolver applies explicit evidence before compatibility data.
return <ReasoningEffortBadge value={observed ?? codexFallback} />;
```
