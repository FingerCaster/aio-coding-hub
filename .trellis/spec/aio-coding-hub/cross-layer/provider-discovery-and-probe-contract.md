# Provider Discovery And Probe Contract

## 1. Scope / Trigger

Applies to read-only model suggestions, their generated IPC/UI entry, the
Codex discovery client version, and explicit provider availability probes.
Persisted provider-model catalogs and managed profiles keep their separate
[managed route ownership contract](./codex-managed-model-route-contract.md).

## 2. Signatures

- `provider_models_discover(input: ProviderModelDiscoveryInput)` returns a
  tagged `ready | empty | unsupported | error` result.
- Input uses `providerId, cliKey, authMode, baseUrls, baseUrlMode, apiKey,
  sourceProviderId, bridgeType`. A saved provider may supply backend-held
  credentials; a secret is never returned to the frontend.
- `provider_test_availability(providerId, model?, prompt?)` keeps its existing
  response DTO. Missing optional overrides preserve old callers.
- `cli_manager::codex_discovery_version(app, deadline)` returns a bounded version
  observation or None, consumed only by discovery identity construction.
- `native_channel_models_discover(targetId, providerId, providerUuid, protocol)`
  returns a target/provider/revision-bound read-only suggestion set plus the
  existing tagged discovery outcome. Its query cache is independent of saved
  model declarations so saving a draft does not trigger or await another fetch.
- `ChannelModelDiscovery` contains `targetId, providerId, providerUuid, protocol,
  revision, models: ModelCapabilitySuggestion[], discovery`. Suggestions contain
  `modelId` and nullable `displayName/input/contextWindow/maxTokens/supportsTools/
  reasoning/reasoningEfforts/defaultReasoningEffort/thinkingMode/supportsDisplay/
  requiresEffort/nativeThinking`, plus `sources` (`upstream`, `configured`, `upstream_conflict`,
  `routing_confirmation`). Neither DTO contains credentials or raw responses.

## 3. Contracts

- Discovery uses the existing upstream proxy client with redirects disabled,
  a 15-second request timeout, and an 8 MiB response bound. Unknown, malformed
  or paginated catalogs fail closed; returned IDs are concrete, trimmed and
  deduplicated. Empty valid catalogs are distinct from failures.
- API-key and supported Codex/Grok OAuth descriptors choose the protocol,
  endpoint and auth headers. Discovery does not refresh or persist OAuth
  tokens. Expired/missing authorization is reported through typed results.
- Native Codex version probing preserves the existing Windows launch wrapper.
  WSL selection requires a valid applied manifest and unambiguous distro;
  invalid, stale or ambiguous evidence uses the supported fallback identity.
  Process output/time bounds and process-tree termination remain mandatory.
  The discovery query version, version header and User-Agent agree; ordinary
  inference identity is not changed by a discovery observation.
- `useProviderModelsDiscoverMutation` has no catalog write or invalidation.
  Dialog candidates combine configured values, existing saved catalog rows
  and discovered suggestions. `aio/` aliases and stars are excluded.
  Completion from a closed/replaced dialog cannot overwrite the next draft.
- Discovery errors preserve the user's typed model and existing candidates.
  The view maps failures to fixed messages, and IPC diagnostic arguments are
  redacted. Credentials, raw upstream error bodies and account tokens must
  never become candidate/error text.
- Native suggestions retain only allowlisted explicit capability fields from
  the same bounded response; unknown fields stay unknown. Configured, non-stale
  provider model capabilities take precedence. Duplicate conflicting rows do
  not widen capabilities. A parallel-tool flag is not ordinary tool support.
- Native discovery snapshots bind provider UUID, protocol, URL, credential/account,
  capability declarations and global routing. URLs and API keys are read from
  one SQLite snapshot; locks and transactions are released during the fetch.
  The source and selected native target are rechecked before returning.
- Reuse gateway rule matching to identify model/effort rewrites. Such candidates
  require explicit capability confirmation instead of inheriting metadata for
  a different upstream model. Do not infer alias-to-model identity by name.
- Native editors auto-fill only untouched drafts. Preserve cleared/manual fields,
  saved declarations and JSON drafts; isolate remounted targets by target, provider
  UUID and protocol. Pi/OMP thinking dropdowns must serialize the validated native
  schema; no guessed effort template or invalid OMP mode is permitted.
- Probe model priority is explicit override, saved availability model, then
  CLI-specific existing fallback (including Codex config). Empty overrides
  select the old defaults; the default prompt is `hi`. Model validation and
  the 4096-character prompt bound run before credential/network work.
- Probe confirmation uses the existing per-provider action guard. Cancellation
  invokes no availability request. Neither suggestions nor a one-time probe
  changes `availability_test_model`, catalog UUIDs or profile ownership.
- Route-panel locate clears name/tag filters and pending scroll restoration,
  scrolls the real `data-provider-id` card once, and disables missing rows.
  It does not mutate route order or save a route draft.

## 4. Validation / Error Matrix

| Input or event | Result |
| --- | --- |
| Valid read-only catalog | Ready candidates; no database/catalog mutation |
| Valid empty catalog | Empty suggestions; manual model entry remains available |
| Redirect, oversized body, pagination, invalid schema | Typed failure, no follow-up redirect/page |
| Unsupported bridge or OAuth descriptor | Unsupported; no accidental inference call |
| OAuth token missing/expired | Typed auth/config failure; no refresh/write |
| WSL manifest ambiguous/stale | Supported fallback version |
| Probe blank model/prompt | Existing model fallback / `hi` |
| Probe wildcard/control model or oversized prompt | Reject before request |
| Discovery completes after dialog replacement | Ignore stale completion |
| Native provider UUID/protocol changed | Reject before using the source |
| Native snapshot, credential/account or global routing changes during fetch | `NATIVE_GATEWAY_MODELS_CONFLICT`; keep draft |
| Native source is blocked, including unverified Gemini OAuth | `NATIVE_CHANNEL_SOURCE_BLOCKED`; no discovery request |
| Native model has a model/effort rewrite or conflicting metadata | Keep candidate; clear only unedited automatic capability fields and require confirmation |
| Saved declaration or manual/cleared/JSON field | Preserve it when suggestions arrive or refresh |

## 5. Examples

Good: discover candidates for saved provider 7, choose a concrete remote ID,
then confirm a one-time probe while preserving provider/model UUIDs.
For native drafts: a catalog returning only `low/high` produces only those
automatic effort mappings; other Pi levels stay null. A missing output capacity
or tool flag stays unfilled unless an exact bundled catalog match supplies a default; otherwise saving requires explicit supplementation.
Boundary: discovery fails after the user types a model; keep the draft and
allow manual confirmation. Bad: write discovery output directly into the
managed picker catalog or refresh expired OAuth tokens during suggestion load.

## 6. Tests

- `app::provider_model_discovery::tests`: descriptors, stored credentials,
  bounded/no-redirect parsing, OAuth read-only behavior and dynamic version.
- `provider_model_discovery::metadata` and `native_channel_discovery`: explicit
  metadata, conflicts, read-only snapshots, configured/stale capability selection,
  UUID/protocol/credential checks, eligibility and shared route matching.
- NativeChannelDiscovery and nativeChannels service tests: automatic fields,
  Pi/OMP dropdown serialization, manual/cleared/JSON preservation, refresh failure,
  model/target identity replacement and response revision checks.
- `wsl::provider_model_discovery::tests`: manifest/distro/version fallback.
- `domain::provider_availability::tests`: bounded overrides, defaults and
  actual Claude/Codex/Grok/Gemini request shapes.
- ProviderTestDialog, ProvidersView, query/providers and modelDiscovery service
  tests: candidate preservation, stale completion, locate, no cache mutation,
  IPC argument forwarding and diagnostic credential redaction.

## 7. Wrong / Correct

Wrong: infer that suggestions own persisted catalog rows, use an unbounded
shell version command, or make a probe while the dialog is cancelled.
Also wrong: infer tools from parallel-tool support, map a routed alias by name,
or insert an OMP `thought` mode / guessed low-medium-high template.
Correct: keep discovery read-only and bounded, use the existing version launch
path, and call availability only after confirmation through its action guard.

## 8. Native catalog defaults and batch selection

- Versioned Pi/OMP catalog defaults may enrich only existing upstream or non-stale saved candidates, by exact consumer/source-provider/auth/protocol/model identity. Never publish the whole bundled catalog as available models; never use prefix/alias guesses or cross-protocol borrowing.
- Retain source versions, commits, input digests, reproducible generator and distributed licenses. A documented native-catalog omission (e.g. OMP tools support) must never be applied to arbitrary upstream JSON.
- Explicit configured/upstream capabilities take precedence. Preserve native non-identity thinking mappings and narrow them by explicit supported efforts. Conflicts and routed aliases bypass defaults. A catalog capacity is an editable suggestion, not a live upstream guarantee.
- Additional sources are configured_candidate, pi_catalog:<version>:<provider> and omp_catalog:<version>:<provider>. nativeThinking contains the validated consumer-specific mapping and default; it is not parsed from arbitrary upstream JSON.
- Batch picker supports search, select visible, clear and deduplicated append; hidden selections survive filtering. Complete drafts may collapse, incomplete drafts remain editable. Outer import retains explicit model selection and publish preview.
- Refresh updates untouched automatic fields but preserves saved/manual/cleared/JSON values and all existing identity/revision protections. OMP defaults prefer explicit values, then medium/low/first supported effort; Pi preserves its native map without changing session defaults.
