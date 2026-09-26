# Pi / OMP Native Configuration And Gateway Contract

## 1. Scope / Trigger

Apply when changing Pi/OMP target resolution, model documents, native archives,
gateway capability declarations, independent generated entries, or protocol routing.
Implementation evidence belongs to the active task; a contract requirement is not
by itself proof that a platform or scenario passed a runtime test.

## 2. Interfaces And Ownership

- `domain/native_cli` defines native targets, node membership and edit DTOs.
- `infra/native_cli::with_target_lock` owns the native document transaction.
  `snapshot`, `patch` and `compensate` share canonical target identity and revisions.
- `native_cli_service` owns native CRUD and only the selected client element of
  `AppSettings.pi_omp_native_targets`; ordinary settings patches never own it.
- `domain/native_gateway` owns explicit model declarations, capability intersections,
  import previews and exact generated-node manifests.
- `native_gateway_service` coordinates catalog preview, publication and withdrawal.
- `GatewayProtocol` distinguishes `anthropic-messages`, `openai-completions`,
  `openai-responses` and `google-generative-ai`; `CliKey` retains `pi` or `omp`.
- The command registry is the shared runtime and generated-bindings authority.

IPC names below omit injected app/DB state. DTO properties use camelCase.

| Commands | Caller input → output |
| --- | --- |
| `native_cli_targets_list` | `client` → `NativeTarget[]` |
| `native_cli_target_validate`, `native_cli_target_select` | `selection` → `NativeTarget` |
| `native_cli_providers_list` | `targetId` → `NativeProvidersList` |
| `native_cli_provider_read_for_edit` | `targetId, nativeKey` → `NativeProviderEdit` |
| `native_cli_provider_save` | `NativeProviderSaveInput` → `NativeMutationResult` |
| `native_cli_provider_apply`, `native_cli_provider_remove` | `NativeProviderActionInput` → `NativeMutationResult` |
| `native_cli_provider_delete` | action fields plus `removeFromNative` → `NativeMutationResult` |
| `native_gateway_models_get` | `providerId, providerUuid` → `GatewayModelsSnapshot` |
| `native_gateway_models_set` | `providerId, providerUuid, expectedRevision, models` → `GatewayModelsSnapshot` |
| `native_gateway_catalog_preview` | `targetId` → `GatewayCatalogPreview` |
| `native_gateway_apply`, `native_gateway_remove` | `{targetId, expectedRevision, catalogRevision}` → `GatewayMutationResult` |
| `native_gateway_import_preview` | `targetId, nativeKey` → `GatewayImportPreview` |
| `native_gateway_import_confirm` | `{targetId, nativeKey, expectedRevision, credentials}` → `ProviderSummary[]` |
| `cli_manager_pi_info_get`, `cli_manager_omp_info_get` | none → `SimpleCliInfo` |
| `native_channel_catalog_preview` | `targetId` → `ChannelCatalog` |
| `native_channel_models_get` | `targetId, providerId, providerUuid, protocol` → `GatewayModelsSnapshot` |
| `native_channel_models_set` | previous fields plus `expectedRevision, models` → `GatewayModelsSnapshot` |
| `native_channel_preview`, `native_channel_apply` | `ChannelLifecycleInput` → `ChannelPreview` / `ChannelMutationResult` |

`NativeTargetSelection` is `{client: pi|omp, mode: default|custom|profile,
agentDir: string|null, profile: string|null}`. Native action inputs bind
`targetId`, `nativeKey`, `expectedRevision`, `expectedNodeDigest` and
`expectedProfileRevision`; save additionally supplies `displayName`, `node` or
field `patch`, and `apply`. Import credentials explicitly bind `groupId` to
the newly supplied `apiKey`. Never derive them from the native node.

SQLite schema v47 adds nullable `providers.gateway_protocol`,
`native_cli_provider_profiles` (unique client/target/key),
`native_gateway_model_specs` (provider FK with cascade; provider/model primary key),
and `native_gateway_manifests` (target/protocol primary key; unique target/native
key). Deleting an upstream removes its model declarations, not native archives or
generated-file ownership. Existing providers retain null protocol semantics.

## 3. Contracts

### CLI updates: check automatically, install only after confirmation

- native_cli_check_latest_version accepts only the typed Pi/OMP client. It reads local package metadata or the isolated OMP version and official stable-version/checksum metadata. It must never start a package manager or download a binary.
- native_cli_update accepts only the client and an opaque, short-lived, single-use backend plan ID. The plan pins the version, install method and directory shown in the confirmation dialog. Reprobe before execution and reject changes; serialize native installations across clients.
- Page mount, foreground polling, manual check, query retry and window focus may only invoke the check command. Only the explicit confirmed user handler invokes update; cancelled/unmounted confirmations perform no installation. UI state and backend locks both reject duplicate execution.
- Compare semantic versions; unknown versions are not up-to-date and a newer installed version must not be downgraded. Do not migrate an unknown installation to a different package manager or prefix.
- Preserve npm prefix / Bun root and package identity. Invoke Node/npm or Bun with argument arrays and a pinned version, never shell interpolation. Bound duration and output; terminate the process tree on timeout and bound output-pipe draining.
- Standalone OMP uses a fixed official Release URL, platform/architecture asset and SHA256SUMS manifest. Stream to a limited temporary file, verify hash and isolated program version, recheck the target, then atomically activate. Never execute a remote installer script or rewrite agent configuration/auth.
- Report installation failures and the actual post-install executable/version. A successful package-manager exit alone cannot establish successful active-version replacement. Windows may add the confirmed bin directory to the user PATH; no system-wide elevation is attempted.
- Network-backed tests install into temporary prefixes/directories and must not call PATH mutation or upgrade the developer's real CLI.

### Native documents

Targets resolve only controlled default, explicit absolute custom, or OMP profile
directories. Canonical target identities prevent profile/client mix-ups. An OMP
legacy JSON fallback is read-only. Higher-priority YAML files are authoritative;
creation or removal changing that priority invalidates the earlier target.

Native `api` remains an extensible string. The four gateway protocols do not limit
ordinary native configuration. Unknown fields survive node-scoped edits. Malformed,
ambiguous, oversized or unsafe files are errors/read-only states, never an empty
document authorized for replacement. Reject duplicate keys and unsupported YAML
anchors, aliases, tags, merge semantics or multiple documents.

All writers use the same target lock, re-read the latest bounded document, check
document revision and exact node digest, preserve unrelated nodes, retain private
raw backups and atomically replace their own temporary output. Compensation checks
owned node digests and preserves later external edits. A process-local mutex does
not lock an external editor; report conflicts and retain recovery evidence.

List projections contain summaries, never raw credentials or headers. Explicit
edit reads may expose the chosen node. Native expressions are data and are never
executed. Native auth stores, sessions, MCP and SQLite databases are outside
AIO native-management write ownership. Provider/gateway writers do not own
defaults; the explicit OMP settings writer below has separate, scoped ownership.
Package version discovery first
reads bounded known-package metadata. Standalone OMP binaries have no package
metadata and their Windows file version belongs to Bun, not OMP. Only native
binary files may fall back to an isolated --version probe: clear inherited
environment, use temporary HOME/AppData/XDG/agent/cache/cwd, null stdin, hide the
Windows console, bound runtime to five seconds and each output stream to 4 KiB.
Accept only the OMP-prefixed version line; never execute script shims, read normal
agent profiles, or fall back to Bun's PE version. Temporary state is cleaned up.

### OMP settings and Agent management

- `omp_settings_read(targetId)` returns only whitelisted settings, field metadata,
  safe model suggestions, and Agent summaries. `omp_settings_save(input)` accepts
  `targetId`, `expectedRevision`, and typed path patches; null removes an explicit
  setting. Model roles and per-Agent records are patched by literal member key,
  including names containing dots. Never treat these as arbitrary dotted paths.
- Only the currently selected OMP target is accepted. Recheck selection under the
  shared native target lock. Settings resolve `config.yml > config.yaml > settings.json`;
  legacy JSON is read-only. Bind revision to selected path and exact bytes, reread
  before replacement, keep private raw backups, and reuse atomic staging. Malformed,
  nonmapping, duplicate-key, oversized, graph/tag/multidocument YAML and link targets
  must fail closed. Parsing is shared with model documents, but provider validation
  does not apply to settings or Agent frontmatter.
- Preserve unknown settings and credentials semantically on disk; never return them
  in settings projections. Native auth/session/project files and models.yml are not
  changed by settings saves. Comments/layout may normalize with exact originals
  retained in private backups. Reading or default previews create no files.
- Default model is `modelRoles.default`, not retired defaultProvider/defaultModel.
  Model pickers must display a concrete configured model and the inheritance
  path when resolvable from the current draft, including Agent role references.
  Deriving this preview never creates settings patches or starts native CLI/auth
  discovery. Native automatic selection, fallback lists, fuzzy selectors and
  parent-session inheritance remain explicitly unresolved at runtime; never
  promote the first catalog entry or global default into a claimed live model.
  Check literal catalog IDs before splitting thinking suffixes, bound alias
  recursion, and keep long selectors inside their grid column.
  AIO entry import/publication still cannot change defaults. Settings UI explicitly
  distinguishes persisted global/Profile values from project/env/runtime overrides;
  setting modelRoleStorage=project never redirects this editor's writes.
- `omp_agent_read(targetId, fileName)` is an explicit, uncached source read;
  `omp_agent_save(input)` binds target, validated plain `.md` filename, revision and
  complete Markdown content. Limit to the selected agentDir/agents directory, validate
  strict frontmatter and required name/description, preserve unknown frontmatter and
  prompt source. Reject traversal, reserved filenames, directory links and conflicting
  user Agent names. Bundled files are never edited; per-Agent settings provide overrides.
- Bundled Agent/model/field metadata is versioned reference data, not active-session
  discovery or proof of authenticated availability. Local user Agent definitions win
  over bundled definitions; source-only/project/plugin agents must be clearly labeled
  and never discovered by executing extensions. Native thinking suggestions use
  efforts/levels/minLevel/maxLevel, not invented fields.
- Frontend drafts are target-scoped, kept through background refetch and conflicts,
  and saved only from explicit handlers with retry disabled. Reset means deletion.
  Confirm discarding dirty drafts on explicit reload/target switch. Old target responses
  cannot replace the new target's form. Agent source is never query-cached or logged.
- Verification uses temporary HOME/agent/cwd and a cleared inherited environment
  for real OMP config probes. It must not edit a developer's installed configuration.

### Independent gateway entries

Native archives and AIO upstream snapshots are separate. Import is an explicit,
revision-bound preview/confirm operation with user-supplied AIO credentials; newly
imported providers are disabled. Unsupported native transport overrides must be
reported, never silently discarded or interpreted as executable credentials.

Models publish only explicit capabilities. Intersections use supported input kinds,
capacity minima and comparable thinking/tool declarations, never model-name guesses.
Provider identity and model revisions protect stale UI writes. Candidate admission
requires the same real client, protocol, declared model and coverage of every
still-published capability snapshot. Routing/configuration drift invalidates stale
declarations or publication; incompatible candidates consume no attempts or health
penalty. The existing model-routing policy remains the sole model mapper.

Generated nodes use a whitelist, local protocol-specific URL and placeholder key.
OMP entries explicitly set `auth: apiKey`. Published node keys are exact manifest
ownership, never a prefix-based license to overwrite user nodes. Publication uses
durable intent, node CAS and manifest finalization; interrupted intent is reconciled
using recorded digests. Withdrawal removes only still-owned nodes. Native defaults
and existing sessions are not changed. Listener readiness, node presence and
observed traffic are separate facts.

### Gateway execution

Native routes use `/{cli}/_protocol/{protocol}/...` and the existing common provider
selection, attempts, deadlines, cancellation, logging and accounting chain. Preserve
real client identity even for Responses. Protocol compatibility does not authorize
Codex/Claude-specific rewrites or legacy whole-client proxy lifecycle operations.
Native inbound credentials are stripped and the chosen upstream credential is
injected according to protocol. Pi and OMP may use different inbound auth headers
for the same Anthropic wire protocol.

Pi/OMP persisted input buckets are mutually exclusive. At the native protocol
usage boundary, OpenAI deducts cache reads and writes from inclusive input; Google
deducts reads; Anthropic keeps its additive input. Normalize once, preserve unknown
input as `None`, keep total/cache details and protocol evidence, and prove parity
between stream/non-stream parsing, persisted SQL aggregates and pricing. Never
relabel the source client to choose token accounting semantics.

### Referenced AIO source channels

`domain/native_channels` owns four sources and five protocol groups: Claude
Messages, Codex Responses, Grok Chat/Responses and Gemini Google API. Schema v48
adds `native_channel_bindings` (unique target/source/protocol and target/native
key) and `native_channel_model_specs` (provider/consumer/protocol/model, provider
FK cascade). Existing target/protocol manifests keep their semantics. Binding
IDs are SHA-256 of target:source:protocol and contain no credential data.

`ChannelLifecycleInput` carries target/document/catalog revisions, incremental
`selections` and explicit `removeBindingIds`. Missing selections preserve their
bindings. The shared publication engine keys mutations by native key and scopes
reconciliation to the correct ownership table. Both tables protect ordinary CRUD
and native-to-AIO import from mutating or re-importing generated nodes. Preview
never writes; apply rechecks source catalog and selected target inside existing
locks, intent persistence, atomic patch, finalization and compensation. Remove-only
operations stay possible without a running listener or compatible current catalog.

Routes `/{consumer}/_aio/channel/{bindingId}/...` resolve an applied binding before
protocol dispatch. A server-owned typed extension holds the source identity;
client headers cannot authorize a source. Allow only supported inference paths.
Whitelist published models and current source candidates with version-bound,
explicit capabilities. Recheck before every send/retry. Candidate membership,
source configuration, model policy, deletion, disabled state and Gemini admission
must not be bypassed by old publication or a forced provider header.

Keep `cli_key=pi|omp` for request logs, usage and client behavior. Use source identity
only where needed for pool selection, credential resolution, OAuth adapters and
source backend handling. Source providers share the existing limits/circuits/cost
identity. Session affinity uses consumer and a binding-prefixed hash of the original
session ID. Append `channel_binding` to the same request's special settings for UI
source badges; never write duplicate source request logs.

Capability declarations are consumer/protocol-specific and tied to source UUID and
configuration. Credential/name rotation alone does not invalidate capabilities.
Discovery IDs never establish tools, reasoning, input kinds or token limits.
Gemini standard API remains available. Current records contain no authoritative
Code Assist license evidence: Gemini OAuth remains blocked as qualification pending,
including tokens with an unverified enterprise claim. Do not infer entitlement
from email/project/token, erase old credentials, or relabel Gemini as Antigravity.
The June 18, 2026 personal-service migration and real-account support evidence are
separate from local protocol/OAuth mock tests.

### Portable configuration boundaries

Legacy portable bundles cannot express Pi/OMP protocol, model declarations or node
ownership. Reject export/replacement import with
`NATIVE_GATEWAY_BUNDLE_UNSUPPORTED` when a Pi/OMP upstream, gateway manifest, source
binding or channel model declaration exists, and reject incoming Pi/OMP providers
before mutation. Recheck under the
import lock and transaction. `pi_omp_native_targets` is device-local: exclude it
from exports, ignore incoming copies and preserve the locally selected targets.

## 4. Validation / Error Matrix

| Condition | Required result |
| --- | --- |
| Document is missing | Explicit creation is permitted after absence revision check |
| Parse/read/size/format validation fails | Keep archives visible as unknown; block replacement |
| Revision or owned digest changes | Reject stale write and preserve current document |
| AIO key collides with a native node | Reject publication without overwriting |
| Native target priority or identity changes | Reload target before editing |
| DB finalization fails | Compensate only still-owned nodes or retain recovery intent |
| Model/client/protocol is incompatible | No upstream attempt or circuit mutation |
| Global/provider mapping drifts | Revalidate declarations/publication before admission |
| Provider deleted/replaced while editor is open | UUID/revision check rejects stale update |
| Native CLI modifies its own isolated state | Record separately from AIO management writes |
| Legacy bundle would drop native gateway semantics | Reject before replacing stored configuration |
| Inclusive native usage includes cache tokens | Normalize once and retain real Pi/OMP identity |

## 5. Good / Base / Bad Cases

- Good: Pi keeps its native provider while a separate AIO Responses entry is published.
- Base: two same-protocol upstreams advertise compatible capabilities and share the
  existing failover policy while logs retain `cli_key=omp`.
- Bad: changing the default native provider during publication, copying native
  credential expressions into AIO keys, or routing Pi as `cli_key=codex`.
- Bad: repairing a corrupt YAML file as an empty map or restoring a whole stale file.

## 6. Required Tests

Cover strict JSONC/YAML parsing, unknown-field preservation, bounded reads, canonical
paths, private backups, target switches, exact ownership, external drift, node CAS,
DB failure compensation and pending-intent recovery. Test explicit import grouping
and rejection, capability intersections, stale provider identity and stale publication.

Exercise Pi and OMP against all four protocols with fixed real CLI versions in
isolated homes and local mock upstreams. Direct mock captures establish client
contracts; separate CLI-to-AIO tests must prove actual production gateway routing.
Include tools, thinking/images where declared, streaming/non-streaming, cancellation,
pre/post-commit errors, compatible failover, incompatible exclusion and usage/log
identity. Regress the existing four clients and Grok dual-protocol behavior.

For referenced channels additionally cover incremental multi-source publication,
exact withdrawal, Codex/Grok same-protocol coexistence, mixed Gemini pools, dynamic
source changes, per-send withdrawal, OAuth refresh, source model mapping, session
isolation and the ten real CLI/source-protocol combinations. Mock authentication
success is not live-account entitlement verification.

Run generated bindings, TypeScript, frontend tests, formatting/lint, Rust library
tests and Clippy. Describe Windows/Linux/macOS evidence accurately; path-unit tests
do not establish an untested platform runtime.

## 7. Wrong / Correct

Wrong: read models.json, mutate a provider, then write that old whole document.
Correct: acquire the canonical target lock, read latest, verify expected revision
and node digest, patch only selected nodes and retain conditional compensation.

Wrong: advertise a model from its name or select every provider in a Pi route.
Correct: publish explicit capabilities and filter candidates by client, protocol,
model and every still-published snapshot before entering the common attempt gate.
