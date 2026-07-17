# Image Gen Trust Boundary Contract

## Scenario: Change Image Download, Save, History Paths, Or Asset Scope

### 1. Scope / Trigger

Use this contract when changing Image Gen remote-image fetching, renderer IPC,
native save dialogs, task history files, cleanup/delete behavior, storage-root
settings, or Tauri asset-protocol scope. These paths cross remote network,
renderer, native UI, SQLite, and local filesystem trust boundaries.

### 2. Signatures

The Rust-owned remote fetch and save commands are:

```rust
pub(crate) async fn fetch_image(
    url: &str,
    timeout_secs: Option<u32>,
) -> Result<ImageGenFetchedImage, String>;

pub(crate) async fn image_gen_save_image(
    window: WebviewWindow,
    suggested_filename: String,
    mime: String,
    data_b64: String,
) -> Result<bool, String>;
```

The filesystem operations receive the trusted settings-derived root:

```rust
pub(crate) fn canonical_storage_root(storage_dir: &Path) -> AppResult<PathBuf>;
pub(crate) fn tasks_list(
    db: &db::Db,
    storage_root: &Path,
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>>;
pub(crate) fn read_image(db: &db::Db, storage_dir: &Path, reference: &str)
    -> AppResult<ImageGenFetchedImage>;
pub(crate) fn task_delete(db: &db::Db, storage_root: &Path, id: &str) -> AppResult<()>;
pub(crate) fn tasks_clear(db: &db::Db, storage_root: &Path) -> AppResult<u32>;
pub(crate) fn storage_cleanup(
    db: &db::Db,
    storage_root: &Path,
    keep_count: u32,
) -> AppResult<u32>;
```

The frontend save adapter accepts only `suggestedFilename`, `mime`, and
`dataB64`. It never accepts or returns a renderer-selected destination path.

### 3. Contracts

- Remote images use a dedicated client with automatic redirects disabled and
  proxy bypassed. Each redirect is resolved explicitly, with at most five
  followed redirects.
- Validate every hop before connecting: scheme is HTTPS, credentials are
  absent, connection port is 443, and the host exists. Reject every IP literal,
  including a public literal. Every DNS answer must be globally routable;
  reject private, loopback, link-local, unspecified, broadcast, multicast,
  CGNAT, benchmark, documentation, reserved, IPv4-mapped non-global, and the
  corresponding IPv6 non-global ranges.
- After validation, pin all accepted DNS `SocketAddr` values into that hop's
  reqwest client with `resolve_to_addrs`. The connection must not perform a
  second unconstrained DNS lookup. A redirect creates a newly validated and
  newly pinned client for its target.
- Image response bytes are capped at 32 MiB. Non-success diagnostic bodies are
  capped at 8 KiB before the 512-character excerpt is formed. A successful
  response must have an `image/*` content type.
- Multipart requests validate every field and file before any Base64 decode or
  request construction. Allow at most 32 files and 64 text fields; bound field
  names, values, filenames, MIME values, and use checked derived decoded sizes.
  Decoded files total at most 64 MiB and text fields total at most 8 MiB.
- Network and console diagnostics never contain URL credentials, query, or
  fragment. Normalize reqwest errors to operation and safe error category;
  neither IPC errors nor logs may reproduce reqwest's credential-bearing URL.
- Image saving is one backend-owned action: validate MIME, matching extension,
  basename-only suggested filename, derived Base64 cap, and decoded 64 MiB cap;
  then open the native save dialog; then validate the just-authorized result's
  extension and write those already-validated bytes. Dialog cancellation
  returns `false` and writes nothing.
- SQLite `dir`, `images_json`, and `ref_images_json` values are untrusted.
  Listing canonicalizes the current settings-derived storage root and validates
  every selected row in full. A task directory must be a non-symlink/reparse
  direct child of that root whose basename exactly equals the validated task
  id; one invalid row fails the request instead of being silently skipped.
- Read access requires both conditions: the canonical file is strictly below
  the trusted root, and a validated single-component filename in that task's
  DB metadata references the same canonical file. List results expose only an
  opaque `<task-id>/<filename>` reference. The renderer resolves it through
  `image_gen_read_image`, which re-queries and revalidates the whole row before
  returning bounded Base64; it never passes the value to `convertFileSrc`.
- Delete, clear, and cleanup validate every selected task directory before the
  first recursive deletion. On validation or filesystem failure, DB rows are
  not deleted first. A DB path alone never grants read, deletion, or scope.
- Image Gen grants no Tauri asset-protocol filesystem scope at startup or when
  the storage root changes. This is deliberate: Tauri 2 `forbid_directory`
  cannot be reversed by a later allow, so allow/forbid root switching cannot
  provide transactional rollback. Backend reads are the sole projection; an
  invalid or failed root change therefore cannot retain or expand old scope.

### 4. Validation & Error Matrix

| Boundary / input | Required result |
| --- | --- |
| HTTPS public host resolving only to public addresses | Pin resolved addresses and perform one no-redirect hop |
| URL contains username/password, non-HTTPS scheme, or non-443 port | Reject before DNS/connect |
| Any IP literal, including globally routable | Reject before DNS/connect |
| Any DNS answer is non-global, including CGNAT, benchmark, reserved, documentation, or mapped non-global | Reject before connect; do not try another answer |
| Redirect is relative and public | Resolve against current URL, revalidate, repin, and follow within five-hop limit |
| Redirect targets HTTP, credentials, private DNS/IP, invalid port, or a loop | Reject without contacting the target |
| Error body exceeds 8 KiB | Stop bounded read; do not include the full body in the error |
| URL query/fragment contains a token or reqwest formats the URL in an error | Omit it from console, request logs, and IPC errors while retaining safe operation/category diagnostics |
| Multipart count, metadata, derived decoded size, or checked aggregate exceeds its cap | Reject every entry before Base64 decode, form construction, or request send |
| Image body exceeds 32 MiB or content type is not `image/*` | Reject without returning Base64 data |
| Suggested filename traverses, contains separators, mismatches MIME, or exceeds 128 chars | Reject before showing a dialog |
| Base64 exceeds derived cap, is invalid, or decodes above 64 MiB | Reject before showing a dialog or writing |
| Native dialog is cancelled | Return `false`; create no file |
| Dialog result extension mismatches MIME | Reject; write no file |
| DB task dir is outside root, nested, wrong-id, missing, symlink, or reparse escape | Reject before filesystem mutation or DB deletion |
| One row in clear/cleanup has an invalid dir | Reject the batch before deleting any selected directory |
| Requested file is inside root but absent from validated DB metadata | Reject the read |
| Stored filename contains traversal or multiple components | Reject metadata; do not normalize or silently skip it |
| DB row is tampered during list or between list and read | Fail the entire request after validation; return no renderer-consumable filesystem path |
| Startup or storage-root switch | Grant no Image Gen asset scope; old roots remain unauthorized |

### 5. Good / Base / Bad Cases

- Good: a public CDN redirects once to another public HTTPS CDN; both hosts are
  independently resolved, screened, pinned, and the bounded image is returned.
- Good: the renderer submits `image-123.png`, `image/png`, and bounded Base64;
  Rust opens the dialog and writes only the returned `.png` path.
- Base: a persisted task under `<canonical-root>/<task-id>` lists an opaque
  reference and reads through the backend normally when safe metadata names it.
- Bad: validate the first URL, let reqwest auto-follow, then inspect the final
  URL after a private redirect has already been contacted.
- Bad: let the renderer obtain an arbitrary path and pass it to a generic Rust
  write command.
- Bad: trust a DB `dir` because its basename equals the task id, feed a listed
  DB path to `convertFileSrc`, or authorize DB directories in the asset scope.
- Bad: delete valid task directories while iterating, then discover a later
  tampered row and leave DB/filesystem state partially advanced.

### 6. Tests Required

- Unit-test URL credentials, scheme, port, private IPv4/IPv6, IPv4-mapped IPv6,
  public IP literals, CGNAT, benchmark, documentation/reserved ranges, relative
  redirects, unsafe redirect targets, redirect loops/limits, content type,
  error-body cap, and image-body cap. Keep a test proving hostname DNS resolving
  to localhost is rejected before an HTTP request.
- Unit-test multipart overlong Base64, too many files/fields, metadata lengths,
  exact aggregate boundaries and checked overflow. Prove invalid input produces
  no decode/form/send side effect.
- Use `SYNTHETIC_SECRET` in query/fragment and reqwest error paths; assert it is
  absent from console/request logs and returned errors while safe diagnostics
  remain.
- Unit-test suggested filename and MIME matching, traversal/separators,
  unsupported MIME, Base64 precheck, invalid Base64, decoded limit, matching
  destination extension, write success, and cancellation behavior at the IPC
  adapter/controller boundary.
- Persist/list/read/delete a normal task through opaque references. Add negative
  DB tampering tests for outside-root task dirs, traversal filenames,
  unreferenced in-root files, symlink/reparse escape, wrong task ids, and
  tampering between list and read. Any invalid selected row fails the list.
- Prove clear/cleanup validate all selected rows before deleting a valid task,
  and prove failed validation leaves DB rows intact.
- Test that startup and successful/failed root switches grant no Image Gen
  asset scope, that an old root cannot render after switching, and normal
  history still renders through backend reads.
- Regenerate bindings, then run Image Gen Rust/frontend suites, full Rust tests,
  typecheck, lint, format checks, Clippy, and `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
let response = shared_client.get(url).send().await?; // follows redirects
validate_fetch_image_url(response.url().as_str())?;

std::fs::write(renderer_supplied_path, decoded_bytes)?;

for dir in distinct_db_dirs {
    app.asset_protocol_scope().allow_directory(dir, true)?;
}
```

Validation happens after network authority was exercised, the renderer chooses
filesystem authority, and DB content expands asset authority.

#### Correct

```text
URL hop -> validate -> resolve -> reject any unsafe IP -> pin addresses
         -> no-redirect GET -> bounded redirect handling or bounded image body

suggested filename + MIME + Base64 -> validate -> native save dialog
                                   -> validate authorized extension -> write

settings storage root -> canonical trusted root
DB task/file metadata -> validate whole row -> opaque reference
opaque reference -> re-query/revalidate -> bounded backend Base64 read
asset scope -> no Image Gen filesystem authority
```

Keep authority acquisition and validation in the same Rust-owned operation;
untrusted remote, renderer, or DB values may describe a candidate but never
grant network or filesystem authority by themselves.
