# AIO Coding Hub Cross-Layer Specs

Rules for contracts that cross the root application's Rust backend, generated
TypeScript bindings, frontend adapters, and React UI.

## Topics

- [Codex config contract](./codex-config-contract.md): typed config fields,
  patch semantics, raw TOML validation, generated bindings, and UI behavior.
- [Codex managed model route contract](./codex-managed-model-route-contract.md):
  stable provider/model identity, provider-scoped discovery, hash-owned profile
  files and picker catalogs, exact readable/legacy alias routing, and
  wire-vs-observed diagnostics.
- [Gateway failover route contract](./gateway-failover-route-contract.md):
  common provider-gate ownership, Ready-provider limits, persisted attempts,
  route hops, and UI count semantics.
- [Provider account-usage query contract](./provider-account-usage-query-contract.md):
  one TanStack Query owner for automatic, timed, and forced manual refreshes,
  bounded NewAPI model-token/account protocols, private credential ownership,
  and validated sub2api daily-limit projection.
- [Provider OAuth device-flow contract](./provider-oauth-device-flow-contract.md):
  bounded Codex/Grok device responses, safe polling arithmetic, flow ownership,
  cancellation, and token persistence.
- [Provider share and import contract](./provider-share-contract.md): strict
  single-provider v1 compatibility reads and v2 exports, backend-owned
  secrets/native I/O, bounded preview capabilities, plugin snapshot binding,
  additive disabled import, and exclusion of private account identity/token data.
- [Config migration bundle contract](./config-migration-skill-bundle-contract.md):
  bounded installed/local Skill export, Base64 and filesystem validation, plus
  versioned v3 private account-credential backup and atomic restoration.
- [Image Gen trust boundary contract](./image-gen-trust-boundary-contract.md):
  DNS-pinned redirect-safe downloads, backend-owned native saving, canonical
  history paths, DB-reference validation, and asset-scope authority.
- [Settings ownership and rollback contract](./settings-ownership-rollback-contract.md):
  lock-internal field-owned RMW, whole-snapshot CAS, and safe rollback.
- [Trellis task context archive contract](./trellis-task-context-archive-contract.md):
  exact self-reference rewriting and repository-wide context validation before archive commit.

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
7. Confirm account usage remains display-only and that fixtures/specs contain
   no upstream body/message, credential, PII, live host, token name, or actual
   account amount.

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

When changing Trellis task archive or context validation:

1. Read [Trellis task context archive contract](./trellis-task-context-archive-contract.md).
2. Keep path rewriting JSON-aware and limited to the archived task's exact `file` prefix.
3. Validate all active and archived manifests before archive auto-commit.

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
- When changing gateway selection or failover, verify skipped candidates,
  Ready-provider limits, route projection, and attempt/transition labels together.
- When changing managed Codex models, verify exact UUID lookup, one bound
  provider, readable-profile plus legacy-UUID lookup, no cross-provider
  failover, canonical/wire/observed separation, stale-mismatch clearing,
  profile/catalog no-clobber and hash ownership, proxy-time catalog restore,
  provider-scoped query generation, and ordinary-route regression coverage
  together.
- When changing account-usage refresh, verify forced fetches, late-result
  suppression, loading/error state, and provider/cache isolation together.
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
