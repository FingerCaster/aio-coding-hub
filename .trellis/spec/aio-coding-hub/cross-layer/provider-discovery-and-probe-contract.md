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

## 5. Examples

Good: discover candidates for saved provider 7, choose a concrete remote ID,
then confirm a one-time probe while preserving provider/model UUIDs.
Boundary: discovery fails after the user types a model; keep the draft and
allow manual confirmation. Bad: write discovery output directly into the
managed picker catalog or refresh expired OAuth tokens during suggestion load.

## 6. Tests

- `app::provider_model_discovery::tests`: descriptors, stored credentials,
  bounded/no-redirect parsing, OAuth read-only behavior and dynamic version.
- `wsl::provider_model_discovery::tests`: manifest/distro/version fallback.
- `domain::provider_availability::tests`: bounded overrides, defaults and
  actual Claude/Codex/Grok/Gemini request shapes.
- ProviderTestDialog, ProvidersView, query/providers and modelDiscovery service
  tests: candidate preservation, stale completion, locate, no cache mutation,
  IPC argument forwarding and diagnostic credential redaction.

## 7. Wrong / Correct

Wrong: infer that suggestions own persisted catalog rows, use an unbounded
shell version command, or make a probe while the dialog is cancelled.
Correct: keep discovery read-only and bounded, use the existing version launch
path, and call availability only after confirmation through its action guard.
