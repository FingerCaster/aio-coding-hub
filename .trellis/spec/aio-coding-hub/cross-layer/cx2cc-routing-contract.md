# CX2CC Routing Contract

This contract owns CX2CC model selection, request reasoning presence, terminal
context projection, and the one-hop reentry into the local Codex gateway. These
rules apply whenever a provider has `bridge_type = "cx2cc"`.

## Scenario: CX2CC Is the Single Protocol-Routing Owner

### 1. Scope / Trigger

- Trigger: changing CX2CC provider editing, protocol translation, configured
  model routing, reasoning fields, provider model capabilities, terminal launch
  settings, gateway self-loop validation, or the shared CX2CC default model.
- The CX2CC mapper is the only component that selects the final Responses model.
  Generic configured-model mapping must not rewrite either the first hop or its
  authenticated local-gateway second hop.
- Ordinary provider eligibility, enablement, credentials, health, circuit,
  limits, retry, and failover gates remain active. CX2CC does not bypass them.

### 2. Signatures

The implementation boundaries are:

```rust
fn map_claude_to_openai(
    source_model: &str,
    models: &ClaudeModels,
    settings: &Cx2ccSettings,
) -> String;

enum IRReasoningConfig {
    Absent,
    Disabled,
    Enabled(Option<String>),
    Adaptive(Option<String>),
    Effort(String),
}

fn resolve_context_window_projection(
    db: &Db,
    candidates: &[ProviderModelContextWindowCandidate],
) -> AppResult<ProviderModelContextWindowProjection>;

fn configured_model_route_for_request<T>(
    cx2cc_active: bool,
    trusted_internal_reentry: bool,
    resolve: impl FnOnce() -> Option<T>,
) -> Option<T>;

InternalReentryRegistry::issue(bridge_provider_id, origin_trace_id) -> Option<String>;
InternalReentryRegistry::consume(nonce, cli_key, method, path, query)
    -> Option<TrustedInternalReentry>;
```

The shared new/blank default is exposed by both layers:

```rust
pub const DEFAULT_CX2CC_FALLBACK_MODEL: &str = "gpt-5.6-sol";
```

```typescript
export const CX2CC_PROVIDER_DEFAULT_MODEL = "gpt-5.6-sol";
```

Tests for defaults must reference these constants instead of repeating model
literals.

### 3. Contracts

#### Model ownership and UI

- The four runtime mapper slots are `opus_model`, `sonnet_model`,
  `haiku_model`, and `main_model`. `reasoning_model` is not a CX2CC mapper slot
  and must not be projected as one.
- A configured slot wins; otherwise the matching global fallback wins. The
  translated Responses body model is authoritative for cost and wire identity.
- The selectable CX2CC preset catalog includes `gpt-5.6-sol`, `gpt-5.6-terra`,
  `gpt-5.6-luna`, `gpt-5.5`, and `gpt-5.4`. Manual and unknown historical
  values remain editable, but a family-only GPT-5.6 alias is not a preset.
- `ClaudeModels` may carry `main_context_window`, `haiku_context_window`,
  `sonnet_context_window`, and `opus_context_window`. These four fields are
  CX2CC-only, each requires a non-empty explicit model in the same slot, and
  each integer must be within `1024..=10000000`. There is no reasoning context
  slot.
- Changing a slot's model clears that slot's context. Changing the default
  preset clears all four contexts, and leaving CX2CC clears all four before
  save. Provider duplication and config backup copy every slot without
  collapsing equal model names.
- The provider editor hides the generic configured-model routing section for a
  CX2CC bridge. Ordinary providers retain that section.

#### Reasoning presence

- `output_config.effort` keeps its exact string and has precedence over an
  enabled/adaptive `thinking` object.
- `thinking.type = "disabled"` maps to Responses
  `reasoning.effort = "none"`.
- Enabled/adaptive without an effort remains an explicit state but does not
  invent an effort.
- An absent reasoning configuration remains absent. The legacy persisted
  `cx2cc_model_reasoning_effort` field is schema-compatible but never supplies a
  runtime fallback.
- Unknown future effort strings are preserved through the typed bridge. Do not
  coerce Codex-only `ultra` into the Responses effort catalog.
- Non-reasoning CX2CC settings such as `service_tier`, response storage, and
  stream compatibility retain their existing ownership.

#### Context projection

- An explicit valid Provider context window wins for its own slot. Slots
  without an explicit window fall back independently to provider-catalog
  resolution; preserve all four slots until after that per-slot decision.
- Candidate identity is exactly `provider_id + provider_uuid +
  remote_model_id`. Numeric ID or model name alone is insufficient.
- A confirmed window requires a fresh provider catalog, a `source =
  "discovered"` model row, `stale = false`, `capabilities_configured = true`,
  and a valid non-null `context_window`.
- All candidates with the same confirmed value return `Exact`. Different
  confirmed values return `Mixed` with the minimum as the conservative process
  limit. Any unknown candidate returns `Unknown`.
- Provider-catalog manual/custom rows, stale rows, unconfigured or missing
  capabilities, identity changes, and unavailable catalogs remain unknown even
  if a local row contains a numeric context value. This does not invalidate an
  explicit Provider slot context that already passed write validation.
- Once every slot is known, equal values are exact and differing values use the
  minimum as the conservative process limit. Any unresolved slot makes the
  terminal projection unknown. Equal model names in different slots do not
  merge distinct explicit context values.
- Terminal launch injects `ANTHROPIC_DEFAULT_OPUS_MODEL`,
  `ANTHROPIC_DEFAULT_SONNET_MODEL`, and `ANTHROPIC_DEFAULT_HAIKU_MODEL` using
  the non-Claude aliases `aio-cx2cc-opus`, `aio-cx2cc-sonnet`, and
  `aio-cx2cc-haiku`. For `Exact` or `Mixed`, it also injects numeric
  `CLAUDE_CODE_MAX_CONTEXT_TOKENS` and
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW`. For `Unknown`, the three family aliases
  remain injected, but neither context-window variable is injected. CX2CC
  terminal launch never injects `ANTHROPIC_MODEL`.

#### Authenticated local reentry

- Only the typed `source = current AIO Codex gateway` intent may authorize a
  local self-loop exception. A hostname, localhost address, or `source_id =
  None` alone is not authority.
- The private header is `x-aio-internal-reentry-nonce`. It carries a random
  256-bit, one-time nonce with a short TTL and bounded registry capacity.
- Authorization binds the bridge provider and origin trace, and matches exactly
  Codex `POST /v1/responses` with no query and a one-hop budget.
- The ingress removes the private header before all other request processing,
  then consumes it once. Invalid method/path/query, expiry, forgery, or replay
  fails closed and burns an issued nonce when present.
- The nonce is added only after the upstream request fingerprint is emitted.
  The authorized hop uses the reusable direct client with `no_proxy()` and
  `redirect(Policy::none())`; ordinary provider requests use ordinary clients.
- A trusted second hop skips generic configured-model route resolution so the
  mapper-selected model remains unchanged. Ordinary self-loop rejection remains
  enabled for every other request.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| CX2CC first hop reaches configured-model resolver | Return no configured route; do not call the resolver closure |
| Authenticated local second hop reaches configured-model resolver | Return no configured route; preserve translated model |
| Ordinary provider request | Resolve and apply configured route normally |
| Reasoning absent with legacy setting present | Emit no `reasoning.effort` |
| `thinking.type = disabled` | Emit `reasoning.effort = none` |
| Enabled/adaptive without effort | Preserve explicit state; emit no invented effort |
| Unknown effort string | Preserve the exact value |
| Ordinary Claude provider submits any context window | `SEC_INVALID_INPUT`; contexts are CX2CC-only |
| CX2CC context has no explicit model in the same slot | `SEC_INVALID_INPUT`; do not borrow another slot or fallback model |
| CX2CC context is outside `1024..=10000000` or is not an integer | `SEC_INVALID_INPUT` |
| Valid explicit slot context exists | Use it for that slot without catalog lookup |
| Candidate is manual/custom | `Unknown(custom_model)`; inject no context variables |
| Any stale/unconfigured/missing/unknown candidate | `Unknown`; do not claim an exact or mixed capacity |
| Confirmed candidate windows differ | `Mixed(minimum)` |
| Forged, replayed, expired, or wrong-contract nonce | Untrusted request; ordinary recursion guard applies |
| Authorized local origin returns 3xx | Return the 3xx; never follow it with the private header |
| Proxy environment variables are set | Authorized reentry connects directly; proxy receives no request |
| Default changes in one layer only | Cross-layer/default-contract tests fail |

### 5. Good / Base / Bad Cases

- Good: a Sonnet request maps once to `gpt-5.6-sol`, enters the current local
  Codex split with a one-time nonce, skips second-hop configured mapping, and
  sends `gpt-5.6-sol` to the selected provider.
- Good: all active provider/model candidates report 1,050,000 tokens; terminal
  launch injects that exact numeric window.
- Good: confirmed candidate windows differ; terminal launch uses the minimum
  and records a mixed projection.
- Good: two slots select the same model with different explicit windows; each
  retains its own value and the terminal process uses the smaller window.
- Base: an ordinary Codex or Claude provider continues to use configured model
  routing, normal proxy behavior, redirects allowed by its own policy, and all
  existing failover gates.
- Bad: mark CX2CC as a managed route to suppress generic routing.
- Bad: infer context from a model family/version string, copy one slot's
  context to another, or trust a manual model row as discovered capability
  evidence.
- Bad: allow all localhost/self-loop targets, reuse an ordinary proxied client,
  follow redirects, or keep the private nonce header after ingress.
- Bad: inject a persisted default effort when the caller omitted reasoning.

### 6. Tests Required

- Mapper tests cover all four slots, configured overrides, runtime fallbacks,
  and shared-default constant ownership.
- Protocol tests cover absent, disabled, enabled, adaptive, known effort,
  unknown effort, precedence, and the legacy-field non-fallback behavior from
  Anthropic input through the final Responses body.
- Route tests prove CX2CC first hop and trusted second hop do not evaluate the
  configured resolver, while an ordinary request still resolves it.
- Context tests cover exact identity, equal candidates, mixed minimum,
  custom/manual, stale model, stale catalog, unconfigured capability, null
  window, missing model, missing/replaced provider identity, and empty
  candidates. They also cover four explicit slot overrides, same-model
  different-window preservation, pairing/range rejection, config/share/local
  duplicate round-trips, and v1-v4 share imports mapping contexts to `None`.
  Terminal tests assert exact/mixed env injection, unknown env omission, and no
  `ANTHROPIC_MODEL`.
- Reentry tests cover issue/consume once, forgery, replay, expiry, capacity
  eviction, wrong CLI/method/path/query, ingress header removal, typed target
  matching, direct proxy bypass, and redirect refusal.
- UI tests cover every GPT-5.6 preset, manual/historical preservation, CX2CC
  generic-route hiding, ordinary-provider visibility, four context controls,
  per-slot/default model clearing, leaving-CX2CC clearing, and removal of the
  fixed thinking control. New production UI code uses typed fields without
  `any` or type assertions.
- Run frontend coverage shards, generated-binding checks, full Rust library
  tests, Rust fmt/check/Clippy, and release contract self-tests before Beta.

### 7. Wrong vs Correct

#### Wrong

```rust
let route = configured_model_route::resolve(requested_model);
let model = route.target_model.unwrap_or_else(|| cx2cc_map(requested_model));

let effort = request_effort.or(settings.cx2cc_model_reasoning_effort);

let context = lookup_by_model_name(remote_model_id)
    .or(Some(DEFAULT_CONTEXT_WINDOW));

let client = ordinary_proxy_client();
client.post(local_gateway).header(INTERNAL_REENTRY_HEADER, nonce).send().await?;
```

#### Correct

```rust
let configured_route = configured_model_route_for_request(
    cx2cc_active,
    trusted_internal_reentry.is_some(),
    || configured_model_route::resolve(/* ordinary request facts */),
);

let reasoning = parse_reasoning_presence(request_body);
let responses_body = cx2cc_translate_once(request_body, reasoning);

let projection = resolve_context_window_projection(
    db,
    &stable_provider_model_candidates,
)?;

emit_fingerprint_without_private_nonce();
let nonce = internal_reentry_registry.issue(bridge_provider_id, trace_id)?;
direct_no_proxy_no_redirect_client
    .post(local_gateway)
    .header(INTERNAL_REENTRY_HEADER, nonce)
    .send()
    .await?;
```
