# Configured Model Routing Contract

This contract owns user-configured model and reasoning-effort rewrites across
settings, Provider persistence, versioned exchange formats, gateway failover,
request logs, cost calculation, generated bindings, and desktop UI.

## Scenario: Apply A Configured Route To The Final Wire Request

### 1. Scope / Trigger

Use this contract when changing any of the following:

- global or Provider-scoped model-routing policy fields;
- Provider selection, request preparation, plugins, URL/body finalization, or
  the transport commit boundary;
- model or reasoning-effort representation for Claude, Codex, Grok, CX2CC, or
  Gemini inference requests;
- configured-route attempts, request-log markers, response observation, or
  model pricing;
- settings, SQLite, Provider share, full config bundle, generated bindings, or
  UI controls that carry this policy.

This feature is independent from the retired Codex Provider translation
bridges. Do not restore bridge types, bridge configuration, Observer/TUI
projections, notifications, or release behavior while changing it.

### 2. Signatures

The shared policy types are:

```rust
pub struct ModelRoutingRule {
    pub source_model: String,
    pub target_model: Option<String>,
    pub reasoning_effort: Option<String>,
}

pub struct ModelRoutingPolicy {
    pub enabled: bool,
    pub rules: Vec<ModelRoutingRule>,
}

pub fn normalize_model_routing_policy_for_write(
    policy: &mut ModelRoutingPolicy,
) -> AppResult<bool>;

pub fn sanitize_model_routing_policy(policy: &mut ModelRoutingPolicy) -> bool;
```

The gateway boundary is:

```rust
configured_model_route::resolve(
    cli_key,
    method,
    original_path,
    immutable_requested_model,
    managed_model_route,
    global_policy,
    provider_policy,
    provider_id,
    provider_name,
) -> Option<ConfiguredModelRoute>;

configured_model_route::apply(route, final_path, final_query, decoded_body)
    -> Result<ConfiguredModelRouteOutcome, ConfiguredModelRouteApplyError>;
```

Persistent/versioned signatures are:

```text
settings schema 57:
  AppSettings.model_routing_policy: ModelRoutingPolicy

SQLite schema 45:
  providers.model_routing_policy_json TEXT NULL

Provider upsert:
  model_routing_policy_override: Option<ModelRoutingPolicy>
  model_routing_policy_override_specified: bool

Provider share schema 3:
  configuration.model_routing_policy_override: ModelRoutingPolicy | null

Config bundle schema 4:
  settings.model_routing_policy
  providers[].model_routing_policy_override
```

`model_routing_policy_override_specified=false` preserves the current database
value. `true + None` writes SQL `NULL`; `true + Some(policy)` writes the strict,
normalized JSON value.

### 3. Contracts

#### Policy validation and matching

- A policy has at most 128 rules. Source and target are at most 256 UTF-8
  bytes; effort is at most 64 Unicode scalar characters.
- Strict writes trim fields and reject an empty source, control characters,
  duplicate normalized source, and a rule with neither target nor effort.
- Matching is exact, case-sensitive, first-match, and one-pass against the
  immutable client model. A target is never fed back into the rule list.
- An enabled empty policy normalizes to disabled. Disabled policies may retain
  valid rules for later re-enablement but have no runtime effect.

#### Global and Provider ownership

| Provider database value | Effective behavior |
| --- | --- |
| SQL `NULL` | Inherit the global policy |
| `Some(enabled=true)` | Replace the global policy completely |
| `Some(enabled=false)` | Suppress configured routing for this Provider |

A malformed global policy is defensively read as disabled. A malformed non-NULL
Provider value is read as `Some(disabled)`, not `None`, so corruption cannot
accidentally enable the global policy. These defensive read paths continue the
original unmodified request and do not trigger failover.

#### Request eligibility and ordering

Only POST inference requests are eligible:

- Claude Messages, including the final CX2CC request;
- Codex Responses and `/responses/compact`;
- Grok Chat Completions or Responses;
- Gemini `generateContent` and `streamGenerateContent`.

Managed `aio/` requests, managed aliases, discovery, availability testing,
token counting, search/list/auxiliary endpoints, non-POST traffic, disabled
policies, and unmatched models are unchanged.

The final-wire sequence is fixed:

```text
bounds/decode and immutable model inference
  -> Provider selection and common gates
  -> Provider/built-in/CX2CC preparation
  -> request sanitizer
  -> RequestBeforeSend plugin
  -> apply configured route atomically
  -> build final URL
  -> finalize body/encoding and fingerprint
  -> transport ownership commit
  -> upstream send
```

Every attempt clears any prior `configured_model_route` marker before local
preparation. Only a successful atomic apply whose final URL also builds may
create the marker. A later Provider cannot inherit the previous Provider's
route, marker, effective model, or effort.

#### Wire mappings and atomicity

| Protocol | Model | Reasoning effort |
| --- | --- | --- |
| Claude Messages | body `model` | `output_config.effort` |
| Responses / compact / CX2CC | body `model` | `reasoning.effort` |
| Chat Completions | body `model` | top-level `reasoning_effort` |
| Gemini generate/stream | model segment in path | numeric `thinkingBudget`, otherwise `thinkingLevel` |

Gemini effort siblings are mutually exclusive. Apply changes to cloned
path/query/body state, verify every requested output, and commit all fields
together. Failure leaves the original attempt state intact and sends no bytes.
When the body is compressed, operate on `GatewayRequestBody` decoded state and
let existing finalization remove or regenerate content-encoding metadata.

#### Failure, audit, and cost

A valid matched rule that cannot be applied produces:

```text
attempt outcome: configured_model_route_apply_failed
public code:     GW_CONFIGURED_MODEL_ROUTE_APPLY_FAILED
terminal HTTP:   502 only when route-apply failure owns final priority
```

The current Provider retry loop ends and the outer loop continues with the
next candidate. This is not a transport retry or Provider health failure: do
not call upstream, update circuit counts, mutate account-usage blocked/recovery
state, bind a Session, or commit dispatch ownership. If an earlier Provider
actually failed upstream, existing final-error priority remains authoritative.
The gateway never creates another client request after the response completes.

`requested_model` always records the immutable client model. A successful
Provider-scoped marker records provider ID/name, policy source, source, target,
effective model, effort, pricing CLI/model, and separate model/effort applied
flags. Pricing may use that target only when the marker belongs to the final
Provider. Missing target pricing is unknown and must not fall back to source
pricing. An effort-only route retains the actual final model as its cost basis.

#### Version compatibility

- Settings 56 -> 57 adds a disabled global policy.
- SQLite 44 -> 45 adds the nullable Provider column; old rows inherit global.
- Provider share v3 preserves inherit/enabled/disabled. V1/v2 readers reject an
  injected routing field, then canonicalize legitimate legacy payloads to
  `None`; new exports are strict v3.
- Config bundle v4 preserves global and Provider policies. V1-v3 import clears
  both, even if a crafted payload injects them.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| More than 128 rules | Strict write rejects with `SEC_INVALID_INPUT` |
| Empty/oversized/control source or duplicate source | Strict write rejects |
| Empty target and effort | Strict write rejects |
| Malformed global persisted value | Disable global route; continue original request |
| Malformed non-NULL Provider value | Explicit Provider disabled; do not inherit global |
| Disabled/no match/ineligible request | No route outcome or marker; existing behavior |
| Valid match and full apply | Commit final wire state and current Provider marker |
| Valid match but JSON/path/output verification fails | Pre-send route failure; switch Provider |
| Every eligible candidate route-fails | HTTP 502 and dedicated error code |
| Route failure follows a real upstream failure | Preserve existing upstream final priority |
| Configured target lacks price | Cost unknown; never use source price |
| Provider share v1/v2 contains routing override | Reject as cross-version schema input |
| Config bundle v1-v3 contains routing fields | Clear fields before import |

Diagnostics contain only stable classifications and bounded identity fields;
never include request bodies, credentials, raw corrupt JSON, or unbounded model
text.

### 5. Good / Base / Bad Cases

- Good: a gzipped Codex request names `source`; `RequestBeforeSend` changes the
  body to another model; the configured rule still matches immutable `source`,
  overwrites the final model/effort, retains unrelated plugin fields, sends an
  identity body, logs `source`, and marks the final Provider.
- Good: Provider A route application fails before send; Provider B resolves its
  own policy and succeeds. A has zero upstream calls and unchanged circuit,
  account, and Session state.
- Base: a Provider with SQL `NULL` inherits global policy; an unmatched source,
  disabled policy, auxiliary endpoint, or managed request passes through.
- Bad: mutate the request before `RequestBeforeSend`, apply rules to a previous
  rewrite instead of the client model, or cascade target through another rule.
- Bad: retain Provider A's marker when Provider B has no route, retry a local
  apply failure on A, or estimate target usage with source pricing.

### 6. Tests Required

- Settings/domain tests assert trim and all 128/256/64 boundaries, duplicates,
  disabled-empty normalization, exact case, no cascade, and malformed-read
  policy for global and Provider scopes.
- Migration tests assert settings 56 -> 57 and SQLite 44 -> 45, including
  fresh-install baseline, idempotence, existing-row `NULL`, and corrupted JSON.
- Provider tests assert preserve/set/clear specified semantics and local
  duplicate state. Share tests cover strict v1/v2/v3 dispatch and all three v3
  override states. Bundle tests cover v4 round trip and v1-v3 stripping.
- Protocol unit tests assert every model/effort field and Gemini sibling rule,
  plus atomic failure with unchanged input.
- Route integration tests assert gzip normalization, plugin-before-route order,
  zero upstream on apply failure, next-Provider success, all-route-failed 502,
  mixed upstream priority, marker replacement, original audit, and no circuit,
  account, retry, dispatch, or Session contamination.
- Request-log tests assert final Provider ownership, effort-only observation,
  target pricing, and unknown cost without source fallback. Frontend tests assert
  marker validation, expected/unexpected presentation, global editing, Provider
  three-state editing, reset, duplicate, and save payloads.
- Run generated bindings, frontend unit/type/lint, Rust fmt/check/clippy/tests,
  and `git diff --check` before commit.

### 7. Wrong vs Correct

#### Wrong

```rust
// Matches a plugin or bridge rewrite and can cascade across attempts.
let route = resolve(body_model, merged_global_and_provider_rules);
apply_in_place(&mut prepared.body, route)?;
record_provider_failure(provider_id);
```

#### Correct

```rust
// Match once on client identity; Provider policy replaces or suppresses global.
let route = resolve(immutable_requested_model, effective_provider_policy);
let outcome = apply(&route, &final_path, final_query.as_deref(), &decoded_body)?;
// Commit cloned state only after all outputs verify; local failure switches
// Provider without transport or health side effects.
commit_final_wire_state(outcome);
```

The immutable requested model is the audit identity. The verified final wire
model is the upstream and pricing identity. They must never be conflated.
