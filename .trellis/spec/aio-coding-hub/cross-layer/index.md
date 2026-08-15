# AIO Coding Hub Cross-Layer Specs

Rules for contracts that cross the root application's Rust backend, generated
TypeScript bindings, frontend adapters, and React UI.

## Topics

- [Codex config contract](./codex-config-contract.md): typed config fields,
  patch semantics, raw TOML validation, generated bindings, and UI behavior.
- [Codex managed model route contract](./codex-managed-model-route-contract.md):
  stable provider/model identity, provider-scoped discovery, hash-owned profile
  files and picker catalogs, explicit reasoning/context capabilities, exact
  readable/legacy alias routing, and wire-vs-observed diagnostics.
- [Gateway failover route contract](./gateway-failover-route-contract.md):
  common provider-gate ownership, Ready-provider limits, persisted attempts,
  route hops, and UI count semantics.
- [Configured model routing contract](./configured-model-routing-contract.md):
  exact original-model matching, global/provider three-state policy, final-wire
  protocol rewrites, pre-send failover, and provider-scoped audit/cost basis.
- [CX2CC routing contract](./cx2cc-routing-contract.md): single-owner model
  mapping, reasoning presence, provider-scoped context projection, shared
  defaults, raw-versus-client usage ownership, and authenticated direct one-hop
  local gateway reentry.
- [Reasoning effort observability contract](./reasoning-effort-observability-contract.md):
  final outbound explicit fields, per-attempt send evidence, coherent
  realtime/history projection, legacy compatibility, and one shared UI badge.
- [Upstream error handling contract](./upstream-error-handling-contract.md):
  configured retry budgets, native Codex SSE recovery, terminal HTTP response
  rewriting, bounded diagnostics, and the shared segmented settings entry.
- [Provider account-usage query contract](./provider-account-usage-query-contract.md):
  one TanStack Query owner for automatic, timed, and forced manual refreshes,
  bounded NewAPI model-token/account protocols, private credential ownership,
  and validated sub2api daily-limit projection.
- [Provider deletion and attempt identity contract](./provider-deletion-and-attempt-identity-contract.md):
  persisted route cascades, retired bridge cleanup/import rejection,
  cancel/filter/invalidate cache reconciliation, and request-time provider
  identity in decision-chain displays.
- [Provider OAuth device-flow contract](./provider-oauth-device-flow-contract.md):
  bounded Codex/Grok device responses, safe polling arithmetic, flow ownership,
  cancellation, and token persistence.
- [Provider share and import contract](./provider-share-contract.md): strict
  single-provider v1-v4 compatibility reads and strict v5 exports, backend-owned
  secrets/native I/O, bounded preview capabilities, plugin snapshot binding,
  additive disabled import, and exclusion of private account identity/token data.
- [Config migration bundle contract](./config-migration-skill-bundle-contract.md):
  bounded installed/local Skill export, Base64 and filesystem validation, plus
  versioned v3 private account-credential backup and atomic restoration.
- [Image Gen trust boundary contract](./image-gen-trust-boundary-contract.md):
  DNS-pinned redirect-safe downloads, backend-owned native saving, canonical
  history paths, DB-reference validation, and asset-scope authority.
- [Settings ownership and rollback contract](./settings-ownership-rollback-contract.md):
  lock-internal field-owned RMW, changed-key patch serialization, strict model-price alias editing,
  whole-snapshot CAS, and safe rollback.
- [Reliability boundary contract](./reliability-boundaries-contract.md):
  route-draft initialization, retryable and maintenance-only startup state,
  bounded diagnostic redaction, backend-confirmed task notifications, and manual upstream review.
- [CI change-scope contract](./ci-change-scope-contract.md): fail-closed Git
  range and path classification, documentation tiers, conditional jobs, and
  the stable always-run CI gate.
- [Release operations contract](./release-operations-contract.md): explicit
  stable version selection, final-head release PR validation, immutable source
  resolution, exact candidate promotion, and post-publication verification.
- [Beta release and update channel contract](./beta-release-update-channel-contract.md):
  device-local opt-in, channel/epoch-bound one-shot updater resources, strict
  static manifests, manual Beta publication, and CAS-audited channel pointers.
- [Trellis task context archive contract](./trellis-task-context-archive-contract.md):
  exact self-reference rewriting and repository-wide context validation before archive commit.
- [Usage insights contract](./usage-insights-contract.md): folder identity and
  filtering, estimated development time, provider metric trends, generated
  bindings, query-key ownership, and Home/Usage UI behavior.

## Pre-Development Checklist

When changing a Codex `config.toml` field:

1. Read [Codex config contract](./codex-config-contract.md).
2. Trace both read and write paths through Rust, generated bindings, the
   frontend adapter, and the consuming UI.
3. Decide separately how structured patches and full raw TOML saves handle
   unset, invalid, and future values.
4. Search for every complete `CodexConfigState` fixture before regenerating
   bindings.

When changing Codex provider models, managed profiles, or alias routing:

1. Read [Codex managed model route contract](./codex-managed-model-route-contract.md).
2. Trace provider/model UUID identity through DB, IPC, generated bindings,
   query keys, profile files, gateway selection, attempts, logs, and UI.
3. Verify ordinary non-managed routing remains unchanged and test all four
   raw-response observation paths before changing warning semantics.
4. Keep filesystem ownership hash-based and fail closed on unsafe Codex-home
   resolution or provider identity drift.
5. For provider-model capability changes, trace the configured flag, effort
   set/default, and context through schema migration, IPC, adapter/query, UI,
   Profile creation gate, Profile-set hash, catalog rebuild, and compensation.

When changing provider account-usage fetching:

1. Read [Provider account-usage query contract](./provider-account-usage-query-contract.md).
2. Decide whether the change affects query ownership, the remote adapter
   protocol, or both; apply every relevant scenario in that contract.
3. For query changes, trace automatic, timed, and manual entry points through
   the same query key, options, cache owner, and component state.
4. Test uncancellable IPC Promises with deliberately reversed completion order.
5. For NewAPI changes, trace the explicit billing/account mode, private versus
   model-key credential loading, Base URL normalization, same-origin endpoints,
   redirect policy, authentication headers, bounded bodies, exact success and
   signed identity validation, field/unit normalization, IPC, and display.
6. For sub2api changes, distinguish account balance from the exact `1d`
   periodic window and fail closed on malformed or duplicate known windows.
7. Confirm the display/query pipeline remains side-effect free; only the
   explicit account-usage route gate may consume its normalized completion for
   fail-open routing. Fixtures/specs contain no upstream body/message,
   credential, PII, live host, token name, or actual account amount.

When changing configured model routing:

1. Read [Configured model routing contract](./configured-model-routing-contract.md).
2. Trace the immutable client model through per-Provider policy resolution,
   final-wire application, attempt isolation, request logs, and cost basis.
3. Preserve the ordering after sanitizer and `RequestBeforeSend`, but before
   URL/fingerprint/body finalization and transport commit.
4. Verify route-application failure switches Provider without transport retry,
   health/circuit/account/session mutation, or a second client request.
5. Recheck settings 57, SQLite 45, Provider share v5, config bundle v4, and
   generated TypeScript bindings together.

When changing CX2CC routing, reasoning, model presets, context projection,
usage translation/accounting, or local gateway reentry:

1. Read [CX2CC routing contract](./cx2cc-routing-contract.md).
2. Trace the original model through the four-slot mapper, first-hop route
   isolation, authenticated second-hop isolation, final wire model, and cost
   marker.
3. Trace reasoning presence through inbound IR and Responses output without
   consulting the legacy persisted effort field.
4. Validate provider UUID/model identity, discovered-source trust, mixed and
   unknown context behavior, and terminal environment omission together.
5. Verify nonce issue/consume order, header stripping, fingerprint order,
   direct/no-proxy/no-redirect transport, and ordinary self-loop rejection.
6. Trace raw provider usage and client-protocol usage as separate values through
   non-stream, real SSE, synthesized SSE, logs, costs, realtime events, and the
   provider ledger. Only nested OpenAI detail buckets may reduce client input.

When changing reasoning-effort transformation, attempt evidence, request-log
projection, or display:

1. Read [Reasoning effort observability contract](./reasoning-effort-observability-contract.md).
2. Trace the final path and semantic body through every protocol transformation
   to the transport boundary; never substitute original request intent.
3. Keep effort and send evidence on each attempt, then apply the shared
   last-success/last-sent selector to both realtime and historical logs.
4. Verify legacy attempt/event defaults and regenerate TypeScript bindings.
5. Preserve CX2CC routing/thinking behavior and render one observed-first badge,
   with the existing Codex resolver used only as a compatibility fallback.

When changing provider deletion or request-log provider identity:

1. Read [Provider deletion and attempt identity contract](./provider-deletion-and-attempt-identity-contract.md).
2. Trace stable provider ID through SQLite route foreign keys, frontend query
   families, and historical attempt snapshots.
3. Keep route cache cancellation, ID filtering, and invalidation contiguous;
   test late uncancellable query completion.
4. Render request-time name plus ID without looking up the current provider or
   treating URL/name as identity.

When changing configured retries, Codex stream-internal recovery, or final
upstream response rewriting:

1. Read [Upstream error handling contract](./upstream-error-handling-contract.md).
2. Keep retry and rewrite schemas, save ownership, and execution phases separate.
3. Trace real upstream facts through retry/failover/circuit accounting before
   constructing a client-visible terminal response.
4. Prove buffered stream data and diagnostic evidence remain bounded and
   credential-safe.

When changing Codex or Grok device authorization:

1. Read [Provider OAuth device-flow contract](./provider-oauth-device-flow-contract.md).
2. Trace start and poll responses through the bounded reader, object/type and
   required-field validation, interval/expiry arithmetic, flow ownership, and
   token persistence.
3. Test pending, terminal, cancellation/replacement, and successful completion
   separately; remote bodies and tokens must not enter errors or logs.

When changing single-provider sharing or import:

1. Read [Provider share and import contract](./provider-share-contract.md).
2. Trace credentials and extension values through Rust serialization, native
   output, preview capability storage, transactional import, generated bindings,
   the frontend adapter, and dialogs without exposing plaintext to React.
3. Preserve strict version dispatch, deterministic bounded serialization,
   referenced-provider refusal, disabled additive import, and no route/template
   writes.
4. Recheck file digest, collision name, and the complete plugin compatibility
   projection at confirm time; stale previews must fail closed.
5. For built-in account usage, preserve explicit mode/refresh config while
   proving User ID and account token never enter share bytes or preview DTOs.

When changing config migration payload handling:

1. Read [Config migration Skill bundle contract](./config-migration-skill-bundle-contract.md).
2. Trace installed and local Skill files through bounded export, Base64,
   bundle reading, decoded validation, metadata validation, and filesystem
   activation.
3. Confirm the single-file raw cap, derived Base64 cap, and decoded total are
   symmetric across export and import.
4. Confirm path, duplicate, file-count, symlink, special-file, metadata,
   `SKILL.md`, and import-file limits remain enforced before partial output.
5. For account credentials, keep v2 Skill and v3 account-snapshot thresholds
   independent, sanitize extension config, and restore private credentials in
   the provider transaction with full rollback on validation failure.

When changing Image Gen network or filesystem behavior:

1. Read [Image Gen trust boundary contract](./image-gen-trust-boundary-contract.md).
2. Trace remote URL hops through DNS validation and pinned connections; do not
   rely on final-URL checks after automatic redirects.
3. Keep save-dialog authorization and file writing in one Rust command; the
   renderer supplies data and a suggested filename, never a destination path.
4. Treat task dirs and stored filenames from SQLite as untrusted candidates and
   validate them against the canonical current/historical settings-owned root
   allowlist; DB content never adds a root.
5. Confirm DB content cannot expand read/delete/cleanup or asset-scope authority.

When changing a production settings writer:

1. Read [Settings ownership and rollback contract](./settings-ownership-rollback-contract.md).
2. Name the fields owned by the writer and search every production `settings::write` call.
3. Keep read, mutation, validation and write under the shared settings lock.
4. Define a committed-field token and CAS rollback for external side effects.
5. For ordinary UI saves, serialize mutations, recompute changed keys after settlement, and encode every
   unowned `SettingsPatch` field as null/missing.
6. For model-price aliases, keep editor reads strict and blocked on failure; do not reuse runtime
   `read_fail_open` behavior for an editing surface.

When changing startup recovery, diagnostic reporting, task-complete notification,
Provider route-draft initialization, or the upstream-sync workflow:

1. Read [Reliability boundary contract](./reliability-boundaries-contract.md).
2. Preserve generation/token invalidation across asynchronous reads and events.
3. For reset maintenance, trace marker durability, fixed-target deletion, plugin initialization and IPC, generated bindings, and the frontend initial-status gate together.
4. Keep diagnostic projections bounded and fail closed at both frontend and Rust boundaries.
5. Confirm upstream synchronization and task notification against their authoritative source before acting.

When changing CI path policy, classification, conditional jobs, or the final gate:

1. Read [CI change-scope contract](./ci-change-scope-contract.md).
2. Trace event payload SHAs through merge-base/diff parsing, path policy, job
   outputs, every conditional job, and ci-gate.
3. Keep control-plane paths hard-coded to full and machine-readable or
   runtime-parsed files outside documentation-only tiers.
4. Preserve every existing full-tier check and verify selected jobs succeed
   while unselected jobs are explicitly skipped.

When changing or running stable release automation:

1. Read [Release operations contract](./release-operations-contract.md).
2. Verify the manifest and historical `Release-As:` values before selecting the
   next version; an override commit must have an empty index.
3. Treat release PR creation and artifact publication as two separate
   no-input dispatches, with final-head CI and six-file version review between
   them.
4. Resolve the tag to a 40-hex commit SHA before builds and independently
   verify tag, Release target, asset digests, and `latest.json` after publish.

When changing Beta participation, updater channel behavior, or Beta release automation:

1. Read [Beta release and update channel contract](./beta-release-update-channel-contract.md).
2. Trace the canonical channel and transition epoch through settings, endpoint,
   query key, metadata, one-shot resource, fresh check, install, and UI copy.
3. Keep release subscription (`channel`) separate from Release classification
   (`isPrerelease`), including a stable final Release on the Beta pointer.
4. Verify stable defaults, explicit opt-in, strict four-platform manifests,
   CAS pointer/pause behavior, and no changes to GitHub latest or Homebrew.

When changing Trellis task archive or context validation:

1. Read [Trellis task context archive contract](./trellis-task-context-archive-contract.md).
2. Keep path rewriting JSON-aware and limited to the archived task's exact `file` prefix.
3. Validate all active and archived manifests before archive auto-commit.

When changing usage folders, development-time estimates, or provider metrics trends:

1. Read [Usage insights contract](./usage-insights-contract.md).
2. Trace request-log/session metadata through Rust aggregation, generated
   bindings, frontend normalization, query keys, and both Home/Usage views.
3. Keep folder filters, gap thresholds, day boundaries, trend buckets, and
   provider identity rules aligned across every consumer.

## Quality Check

- Regenerate and verify `src/generated/bindings.ts` from Rust source.
- Test Rust parsing, structured patching, and full-file write safety.
- Test frontend adapter defaults and the UI's null/unknown-value behavior.
- When Rust changes touch target-gated code, run
  `cargo clippy --all-targets --locked -- -D warnings` on every affected target
  family. Host Clippy does not compile another platform's `cfg` branches; use
  the CI-equivalent Linux environment for Unix-only code before pushing from
  Windows.
- Verify unrelated patches preserve fields that they do not own.
- Run a deterministic barrier through a real production settings writer; prove
  unrelated Image Gen/Grok fields survive and CAS preserves newer owner values.
- Run focused tests, `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`, and
  `pnpm check:generated-bindings`.
- When changing usage insights, verify folder-filter parity, unknown-folder
  retention, development-time gap/hour/day arithmetic, trend metric validity,
  complete normalized query keys, generated bindings, and UI empty/error states.
- When changing CI scope routing, run the dependency-free classifier and
  workflow contract self-tests, all three documentation contracts, actionlint
  when available, and git diff --check; inspect the full job graph for retained
  checks.
- When changing or executing release automation, run release source,
  promotion, signing-scope, support-matrix, Homebrew, and CI-scope contracts;
  verify the final PR head and all six version files, then verify the published
  tag/source identity, exact asset matrix, digests, and signed updater entries.
- When changing the Beta update channel, also verify settings error recovery,
  stale writer responses, one-shot install retry, cleanup against the latest
  channel+epoch, strict raw manifest shape, stable-on-Beta labeling, UTF-8
  release assets, and pointer CAS/pause races.
- When changing gateway selection or failover, verify skipped candidates,
  Ready-provider limits, route projection, and attempt/transition labels together.
- When changing configured model routing, verify exact case-sensitive one-pass
  matching, Provider replace/suppress semantics, all supported wire protocols,
  compressed/plugin ordering, failure isolation, original-model audit, final
  Provider marker ownership, and no source-price fallback.
- When changing CX2CC, verify the four-slot mapper remains the only model owner,
  reasoning presence is preserved without legacy fallback, context is trusted
  only from discovered provider-scoped rows, authenticated local reentry skips
  second-hop mapping, the private nonce cannot traverse a proxy or redirect,
  client usage is mutually exclusive, and raw provider accounting remains
  inclusive across stream and non-stream paths.
- When changing reasoning-effort observability, verify final-wire explicit-field
  extraction, truthful per-attempt send evidence, last-success/last-sent
  selection, old JSON/event defaults, realtime/history parity, future string
  preservation, and exactly one shared badge with Codex fallback precedence.
- When changing upstream error handling, verify retry/rewrite save isolation,
  shared retry budget/backoff, pre-commit-only stream recovery, terminal-only
  HTTP rewrite, client/attempt status separation, and bounded redacted evidence.
- When changing managed Codex models, verify exact UUID lookup, one bound
  provider, readable-profile plus legacy-UUID lookup, no cross-provider
  failover, canonical/wire/observed separation, stale-mismatch clearing,
  profile/catalog no-clobber and hash ownership, proxy-time catalog restore,
  provider-scoped query generation, explicit capability validation and v41
  backfill, effort/context catalog projection, capability-update rollback, and
  ordinary-route regression coverage together.
- When changing account-usage refresh, verify forced fetches, late-result
  suppression, loading/error state, and provider/cache isolation together.
- When changing provider deletion, verify persisted cascade, failure atomicity,
  all cached route families, cross-CLI isolation, and reverse-completion races.
- When changing decision-chain provider identity, verify the same snapshot
  label in summaries and collapsed/expanded attempts, including deleted,
  unknown, invalid-ID, same-name, and same-URL cases.
- When changing the NewAPI account-usage adapter, verify the public status plus
  two Bearer billing requests, trailing `/v1` normalization, same-origin and
  no-redirect rules, exact unit/formula/expiry parsing, per-response body caps,
  exact unlimited-sentinel behavior, application-error precedence, and
  all-or-nothing failure. For account mode, separately verify public status plus
  private `user/self`, signed User ID identity, exact success, credential
  isolation, missing-credential zero-request behavior, and no fabricated total.
- For sub2api `rate_limits`, verify only one exact `1d` window projects to
  daily fields, arithmetic/timestamps are consistent, unknown windows stay
  unknown, and periodic remaining never becomes wallet balance.
- Audit account-usage diffs for credential, PII, host, upstream-message/body,
  token-name, and actual-account-value leakage, and verify routing, circuit,
  availability, order, and enablement remain untouched.
- When changing config migration payloads, verify export/import boundary
  symmetry, failure before target-directory creation or file writes, v1/v2 and
  installed/local compatibility, and file-count, total-size, Base64, path,
  symlink, cycle, special-file, metadata, and import-bundle safety negatives.
  For private account snapshots, add the v1/v2/v3 capability matrix, sanitized
  config, invalid-credential rollback, no-Debug/no-log checks, and proof that
  single-provider share remains credential-free.
- When changing Image Gen, verify no-redirect per-hop DNS pinning, private-host
  and non-global-address negatives, body/redirect caps, URL/error redaction,
  multipart decode-before-allocation budgets, backend-owned save cancellation
  and extension checks, canonical root containment, opaque DB-reference reads,
  batch validation-before-delete, and zero Image Gen asset scope.
- When changing provider device OAuth, verify bounded authorization/token
  bodies, non-empty typed fields, bounded Result expiry arithmetic, cumulative
  RFC 8628 slow-down intervals, pending/terminal flow ownership, cancellation,
  no-persistence invalid cases, and secret-free diagnostics.
- When changing provider sharing, verify copy/save byte identity, strict schema
  and size negatives, redacted IPC/UI boundaries, conditional clipboard cleanup,
  active preview expiry, single-use/discard behavior, file/name/plugin snapshot
  binding, full credential/config/extension round-trip, forced disabled import,
  and zero route/template writes. For account mode, also verify canonical config
  survives while User ID/token are excluded, imported providers require their
  own credentials, and local duplication still copies private credentials.
