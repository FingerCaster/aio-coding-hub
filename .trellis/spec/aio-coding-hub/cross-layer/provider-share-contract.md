# Provider Share And Import Contract

This contract owns single-provider sharing across the Rust provider domain,
Tauri commands, generated TypeScript bindings, frontend adapters, and provider
UI. It is intentionally separate from whole-application config migration:
importing a provider is an additive, disabled-by-default operation.

## Scenario: Share Or Import One Provider

### 1. Scope / Trigger

Apply this contract when changing any of the following:

- the `aio-coding-hub.provider-share` JSON schema or serialization;
- provider credentials, configuration fields, bridge behavior, or extension
  values included in a share;
- the six `provider_share_*` Tauri commands or their generated bindings;
- preview token storage, file binding, plugin compatibility, or import
  transaction behavior;
- the provider card share action or provider-page import flow.

The contract owners are:

- `src-tauri/src/domain/providers/share.rs` for schema, validation, export,
  preview projection, and transactional import;
- `src-tauri/src/app/provider_share_service.rs` for sensitive preview state;
- `src-tauri/src/commands/providers/share.rs` for native file and clipboard
  authorization;
- `src/services/providers/providerShare.ts` for the typed IPC adapter;
- `src/query/providerShare.ts` and the provider dialogs for cache/UI behavior.

### 2. Signatures

The generated command surface is:

```typescript
providerShareCopyToClipboard(
  providerId: number,
  confirm: RiskyIpcConfirm | null,
): Promise<Result<boolean, string>>;

providerShareSaveToFile(
  providerId: number,
  confirm: RiskyIpcConfirm | null,
): Promise<Result<boolean, string>>;

providerShareImportPreviewFromFile(): Promise<
  Result<ProviderShareImportPreview | null, string>
>;

providerShareImportPreviewFromContent(
  content: string,
): Promise<Result<ProviderShareImportPreview, string>>;

providerShareImportConfirm(
  previewToken: string,
  confirm: RiskyIpcConfirm | null,
): Promise<Result<ProviderSummary, string>>;

providerShareImportPreviewDiscard(
  previewToken: string,
): Promise<Result<boolean, string>>;
```

Copy, save, and confirm require a `RiskyIpcConfirm` whose action exactly
matches the command. Export resources are `provider:<provider_id>:share`; import
resources are `provider-share-preview:<preview_token>`.

The domain boundary is:

```rust
parse_provider_share(bytes: &[u8]) -> AppResult<ProviderShareEnvelopeV2>
serialize_provider_share_v2(envelope: &ProviderShareEnvelopeV2) -> AppResult<Vec<u8>>
export_provider_share_v2(db: &Db, provider_id: i64) -> AppResult<ProviderShareEnvelopeV2>
preview_provider_share(db: &Db, envelope: &ProviderShareEnvelopeV2)
    -> AppResult<ProviderSharePreviewDraft>
import_provider_share(
    db: &Db,
    envelope: &ProviderShareEnvelopeV2,
    expected_final_name: &str,
    expected_extensions: &[ProviderShareExtensionPreview],
) -> AppResult<ProviderSummary>
```

`parse_provider_share` is the only version-dispatch boundary. It strictly
parses v1 or v2, converts v1 retry policies to canonical v2, and returns only
`ProviderShareEnvelopeV2` to preview/import services.

No schema migration is involved. Import writes one `providers` row plus its
credential and extension fields in one SQLite transaction. It must not write
default-route, sort-mode-template, circuit, usage-window, request-log, or model
discovery state.

### 3. Contracts

#### Versioned JSON

The top-level discriminator is exact:

```json
{
  "type": "aio-coding-hub.provider-share",
  "schema_version": 2,
  "provider": {
    "cli_key": "codex",
    "name": "Example",
    "enabled": true,
    "configuration": {
      "base_urls": ["https://example.invalid/v1"],
      "base_url_mode": "order",
      "priority": 100,
      "cost_multiplier": 1.0,
      "claude_models": {
        "main_model": null,
        "reasoning_model": null,
        "haiku_model": null,
        "sonnet_model": null,
        "opus_model": null
      },
      "model_mapping": { "default_model": null, "exact": {} },
      "availability_test_model": null,
      "limits": {
        "limit_5h_usd": null,
        "limit_daily_usd": null,
        "daily_reset_mode": "fixed",
        "daily_reset_time": "00:00:00",
        "limit_weekly_usd": null,
        "limit_monthly_usd": null,
        "limit_total_usd": null
      },
      "tags": [],
      "note": "",
      "bridge_type": null,
      "stream_idle_timeout_seconds": null,
      "upstream_retry_policy_override": null
    },
    "authentication": { "mode": "api_key", "api_key": "<secret>" },
    "extensions": []
  }
}
```

OAuth authentication replaces `api_key` with `provider_type`, `access_token`,
`refresh_token`, `id_token`, `token_uri`, `client_id`, `client_secret`,
`expires_at`, `email`, and `refresh_lead_seconds`. A v1 retry override contains
`enabled`, `status_codes`, `transport_errors`, `max_retries`, `backoff_ms`, and
`counts_toward_circuit_breaker`. The v2 equivalent replaces `status_codes`
with `http_rules`; each rule contains `enabled`, `status_code`,
`body_contains`, and `description`. Each extension contains `plugin_id`,
`plugin_version`, `namespace`, and plugin-owned open JSON `values`.

All controlled v1 and v2 objects use `deny_unknown_fields`; only extension
`values` is open. Each version rejects fields owned by the other version, as
well as unknown discriminators, versions, fields, enum values, invalid UTF-8,
and invalid provider fields. A future format gets a new explicit version
reader rather than weakening either existing reader.

New serialization is v2 pretty JSON with one trailing newline and
deterministic extension ordering by `(plugin_id, namespace)`. Copy and save
call the same v2 serializer and enforce the 8 MiB encoded limit. There is no
v1 export path. The default filename is
`aio-coding-hub-provider-<cli>-<sanitized-name>.json`, uses a cross-platform
240-byte budget, and contains no timestamp.

#### Secret And Native-I/O Boundary

The full envelope and credentials stay in Rust memory. Export commands write
directly to the native clipboard or to a user-authorized file; neither command
returns JSON to React. File saving uses an atomic write. Clipboard cleanup runs
after 60 seconds and clears only when the current clipboard still exactly
matches the exported content.

Content preview may send pasted JSON into the command argument, but adapter
diagnostics must replace it with `[REDACTED]` and may record only its UTF-8 byte
length. Preview responses contain only:

```typescript
type ProviderShareImportPreview = {
  previewToken: string;
  cliKey: string;
  sourceName: string;
  finalName: string;
  sourceEnabled: boolean;
  importEnabled: false;
  authMode: ProviderAuthMode;
  credentialStatus:
    | "configured"
    | "needs_api_key"
    | "not_required"
    | "available"
    | "refreshable"
    | "needs_login";
  extensionCount: number;
  extensions: ProviderShareExtensionPreview[];
  canImport: boolean;
};
```

Do not add raw JSON, credentials, full Base URLs, local file paths, or secret
expiry details to this DTO. Sensitive envelope, preview, and pending-import
types must not derive `Debug`.

#### Preview Capability And Import

A preview token is a random 32-byte value encoded as 64 hexadecimal
characters. The backend owns the parsed envelope for 10 minutes, with limits
of 8 entries and 32 MiB aggregate sensitive bytes. Each service has at most one
cleanup worker, which actively releases expired entries even without later
requests. Confirm and discard remove the token immediately; confirm consumes
the token even when the subsequent import fails.

File previews additionally bind the server-private path to the exact SHA-256
content digest. Confirm must re-read the file and compare the digest. Inside the
import transaction it must recompute the collision-free name and the complete
extension compatibility projection; either snapshot changing makes the preview
stale.

Import always inserts a disabled provider at the end of that CLI's order. A
case-insensitive, trim-insensitive name collision resolves to `名称 副本`, then
`名称 副本 2`, and so on. Import does not make network requests and does not add
the provider to routes or sorting templates. Empty API keys and unusable OAuth
access tokens remain recoverable because the provider is disabled; the preview
reports the required follow-up action.

Providers with `source_provider_id != null` cannot be exported. Import has no
source-provider ID field and rejects bridge values that imply an external
provider. A Claude `cx2cc` provider with no source provider is the only
standalone bridge form allowed.

#### Plugin Compatibility

`core.provider-account-usage` may recreate its internal owner through the
existing domain helper, but its namespace must still be exact. Every other
extension must satisfy all of these at preview and confirm time:

- the plugin row exists and has status `installed`, `enabled`, `disabled`, or
  `update_available`;
- `plugins.current_version`, share `plugin_version`, and parsed
  `manifest.version` are exactly equal;
- database `plugin_id`, share `plugin_id`, and parsed `manifest.id` are exactly
  equal;
- the manifest declares capability `provider.extensionValues` and contributes
  the same namespace for the target CLI.

Do not infer compatibility from semver while extension values have no separate
schema version. Missing or changed compatibility blocks the whole transaction;
partial extension import is forbidden.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Missing, mismatched, invalid, or expired risky confirmation | `SEC_CONFIRM_*`; perform no clipboard, file, or DB mutation |
| Empty, oversized, non-UTF-8, malformed, unknown-field, invalid-field, or unsupported-version content | `SEC_INVALID_INPUT`; create no preview/import row |
| Export target does not exist | `DB_NOT_FOUND` |
| Export target references another provider | `PROVIDER_SHARE_REFERENCED_PROVIDER`; write no output |
| Exported extension has no installed owner version | `PROVIDER_SHARE_EXTENSION_UNAVAILABLE` |
| Preview token malformed | `SEC_INVALID_INPUT` |
| Preview token expired, consumed, or absent | `PROVIDER_SHARE_PREVIEW_EXPIRED` |
| Preview file digest, final name, or complete plugin projection changed | `PROVIDER_SHARE_PREVIEW_STALE`; require a new preview |
| Preview still contains any incompatible extension | `PROVIDER_SHARE_EXTENSION_INCOMPATIBLE`; roll back all writes |
| Clipboard, native dialog, serialization, or atomic file operation fails | redacted `SYSTEM_ERROR`; do not include content or path |

Errors may identify a field, plugin ID, namespace, or stable error code. They
must not contain credentials, pasted JSON, full URLs, native paths, or plugin
extension values.

### 5. Good/Base/Bad Cases

- Good: a complete API-key provider with compatible extensions exports to the
  same bytes through copy/save, previews without secrets, and imports as a
  disabled provider with all fields and extension values preserved.
- Good: importing an existing name selects the deterministic next copy name,
  rechecks that name in the transaction, and leaves the original provider and
  every route/template untouched.
- Base: an empty API key previews as `needs_api_key`; an expired OAuth access
  token with usable refresh material previews as `refreshable`. Both may import
  disabled without any remote call.
- Base: a standalone Claude `cx2cc` provider previews as `not_required` and
  round-trips without a source-provider ID.
- Bad: unknown or cross-version v1/v2 fields, a future schema version, an
  external provider bridge, a changed preview file, an expired token, or
  plugin manifest/version drift fails before commit and leaves provider counts
  unchanged.
- Bad: cancelling file selection or save returns `null`/`false` and causes no
  filesystem, clipboard, or database side effect.

### 6. Tests Required

- Domain tests: strict schema negatives, UTF-8 and 8 MiB boundaries,
  deterministic reserialization, cross-platform filename byte bounds, all
  configuration/credential/extension round-trips, referenced-provider refusal,
  standalone `cx2cc`, collision naming, disabled/no-route import, and rollback.
- Plugin tests: missing/unavailable owner, exact version mismatch, manifest
  ID/version mismatch, missing capability/namespace/target CLI, built-in owner
  recreation, and a compatibility change between preview and confirm. Assert
  provider counts and extension rows are unchanged on every failure.
- Preview-service tests: 64-hex token validation, single use, explicit discard,
  TTL expiry without a follow-up request, one-worker cleanup, entry/byte
  eviction, file digest changes, and sensitive DTOs without `Debug`.
- Command tests: conditional clipboard equality, atomic-write failure without
  path/content leakage, and native-dialog cancellation behavior where it can be
  isolated from the desktop shell.
- Frontend adapter tests: generated-result decoding, CLI/token narrowing,
  `canImport` consistency, risky confirmation action/resource, and
  `[REDACTED]` diagnostic arguments for pasted content.
- React/query tests: card/page entry placement, referenced share disabled,
  warning/confirmation flows, file and content modes, late preview suppression,
  discard on edit/mode switch/close/unmount, target-CLI invalidation, and CLI
  switch after success.
- Final gate: regenerate bindings, then run focused tests, `pnpm typecheck`,
  `pnpm lint`, `pnpm tauri:fmt`, `pnpm tauri:check`, and full
  `pnpm tauri:test`.

### 7. Wrong vs Correct

#### Wrong

```typescript
// Leaks credentials into renderer state and lets the UI choose a write path.
const json = await commands.exportProvider(providerId);
await save(json, rendererSelectedPath);
```

```rust
// Trusts a point-in-time preview after target state may have changed.
let preview = preview_provider_share(db, &share)?;
cache.insert(token, (share, preview.final_name));
// Later: insert without rechecking name, file digest, or plugin state.
```

#### Correct

```typescript
// Rust owns plaintext and native authorization; React receives only success.
await providerShareSaveToFile(providerId);
```

```rust
// A single-use capability binds content and previewed target state.
let pending = share_service.take_preview(&preview_token)?;
pending.verify_and_import(db)?; // digest + name + full plugin projection + transaction
```

## Scenario: Account-Usage Secrets In A Single-Provider Share

### 1. Scope / Trigger

Use this scenario when account-usage configuration, private NewAPI account
credentials, the built-in account-usage extension, or provider duplication
changes. Single-provider share, whole-config backup, and local duplication have
different credential policies and must not be collapsed into one serializer.

### 2. Signatures

```rust
pub(crate) fn sanitize_account_usage_extension_value(
    values: &serde_json::Value,
) -> serde_json::Value;

pub(crate) fn export_provider_share_v2(
    db: &Db,
    provider_id: i64,
) -> AppResult<ProviderShareEnvelopeV2>;

pub(crate) fn import_provider_share(
    db: &Db,
    envelope: &ProviderShareEnvelopeV2,
    expected_final_name: &str,
    expected_extensions: &[ProviderShareExtensionPreview],
) -> AppResult<ProviderSummary>;
```

The `aio-coding-hub.provider-share` v1 compatibility reader and v2 export
schema contain no User ID or account access-token field. Their built-in
extension value may contain only
`adapterKind`, `newApiQueryMode`, `timedRefreshEnabled`, and
`refreshIntervalSeconds`.

### 3. Contracts

- Normalize the exact built-in identity
  `core.provider-account-usage/accountUsage` on v2 share export, strict v1/v2
  parse, preview normalization, and import. Historical `newApiUserId`, account
  token, and unknown fields are removed through the shared sanitizer.
- Preserve explicit `newApiQueryMode: "account"`. Do not downgrade it to
  billing just because share excludes the credentials.
- Never read `provider_account_usage_credentials` while building a
  single-provider share. The envelope, preview DTO, generated bindings,
  renderer, clipboard diagnostics, and file diagnostics contain neither User
  ID nor account access token.
- Import creates the provider disabled and without a private account credential
  row. Account mode therefore projects an explicit credentials-required state
  and sends no request until the recipient supplies their own credentials.
- Local provider duplication is not share/import. It copies User ID and token
  inside the backend provider transaction so the local duplicate retains query
  capability without exposing plaintext to React.
- Whole-config export/import is also not share/import; schema v3 intentionally
  includes private account credentials under its separately warned backup
  contract.
- Built-in owner recreation and exact-namespace validation remain mandatory.
  No account-specific exception may weaken the rest of the plugin compatibility
  projection or the disabled/no-route import posture.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Built-in extension has private or unknown fields | Strip them and preserve only canonical config |
| Built-in plugin ID uses another namespace | Reject as incompatible/invalid |
| Share source is in account mode with credentials | Share mode/config only; omit both private fields |
| Imported account-mode provider | Disabled, no credentials, explicit configuration required |
| Imported provider is refreshed before credentials are set | Send no account request |
| Local duplicate is requested | Copy private credentials in the same backend transaction |
| Whole-config v3 export is requested | Follow config-migration contract, not share policy |

### 5. Good / Base / Bad Cases

- Good: an account-mode provider exports a v2 share whose canonical extension
  keeps account mode but contains no account identity or token; the recipient
  imports it disabled and sees a credentials-required state.
- Base: billing or sub2api providers retain their canonical account-usage
  extension settings with no new share fields.
- Good: local duplication preserves synthetic credentials while the returned
  `ProviderSummary` exposes only User ID and token-configured boolean.
- Bad: reuse the whole-config `ProviderExport` credential snapshot as a
  single-provider envelope.
- Bad: remove account mode during normalization and silently query billing with
  the recipient's model key.

### 6. Tests Required

- Seed synthetic User ID/token credentials and assert serialized share bytes,
  preview DTOs, adapter diagnostics, and summaries contain no token and no
  source account identity.
- Assert both export and import normalization remove historical private keys
  from extension values while preserving canonical mode and refresh settings.
- Import an account-mode share and assert disabled state, no private credential
  row, no route/template writes, and configuration-required account usage.
- Duplicate the same local provider and assert private credentials are copied
  transactionally while no frontend response or log contains the token.
- Keep strict schema, built-in namespace, preview capability, rollback, plugin
  compatibility, and full credential/config/extension share tests green.

### 7. Wrong vs Correct

#### Wrong

```rust
share.provider.account_credentials =
    load_account_usage_credentials(conn, provider_id)?;
```

#### Correct

```rust
extension.values = sanitize_account_usage_extension_value(&extension.values);
// Single-provider share never reads the private credential table.
```

The user-selected account mode is portable configuration; the sender's account
identity and system token are not portable share data.
