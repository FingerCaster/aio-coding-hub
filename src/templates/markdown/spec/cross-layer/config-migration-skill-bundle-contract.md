# Config Migration Skill Bundle Contract

## Scenario: Change Installed Or Local Skill Payload Migration

### 1. Scope / Trigger

Use this contract when changing how a configuration bundle exports installed
or local Skill files, serializes their bytes as Base64, decodes an imported
payload, or writes the decoded files to disk.

The complete boundary is:

```text
installed/local Skill directory
  -> bounded recursive export
  -> SkillFileExport { relative_path, content_base64 }
  -> ConfigBundle JSON
  -> 64 MiB bounded import read
  -> Base64 and decoded-byte validation
  -> validated target-directory write
```

This contract does not change Skill installation, synchronization, or runtime
budgets outside configuration migration.

### 2. Signatures

The Skill filesystem entry points in
`src-tauri/src/infra/config_migrate/skill_fs.rs` are:

```rust
pub(super) fn export_skill_dir_files(
    dir: &Path,
    skip_source_marker: bool,
) -> AppResult<Vec<SkillFileExport>>;

pub(super) fn write_skill_files_to_dir(
    dir: &Path,
    files: &[SkillFileExport],
    source_metadata: Option<&SkillSourceMetadataFile>,
) -> AppResult<()>;
```

The serialized file payload remains:

```rust
pub struct SkillFileExport {
    pub relative_path: String,
    pub content_base64: String,
}
```

The configuration import command in
`src-tauri/src/commands/config_migrate.rs` owns the bounded bundle read:

```rust
fn read_config_import_bundle_with_max_len(
    file_path: &str,
    max_len: usize,
) -> Result<config_migrate::ConfigBundle, String>;

fn read_config_import_bundle(
    file_path: &str,
) -> Result<config_migrate::ConfigBundle, String>;
```

`read_config_import_bundle` passes
`config_migrate::CONFIG_IMPORT_FILE_MAX_BYTES` to the bounded helper before
UTF-8 and JSON parsing. Do not introduce an unbounded alternate reader.

### 3. Contracts

- The configuration bundle schema, `relative_path` representation, and
  standard Base64 field format remain unchanged.
- `CONFIG_SKILL_TOTAL_MAX_BYTES` is `8 * 1024 * 1024` and
  `CONFIG_SKILL_FILE_MAX_BYTES` explicitly equals that shared constant.
  `CONFIG_SKILL_FILE_BASE64_MAX_BYTES` is derived from the raw single-file
  limit with `CONFIG_SKILL_FILE_MAX_BYTES.div_ceil(3) * 4`; it is not an
  independent magic number.
- One Skill contains at most 256 exported files and at most 8 MiB of decoded
  file bytes in total. A relative path contains at most 512 characters.
- Source metadata remains bounded at 64 KiB and `SKILL.md` remains bounded at
  256 KiB. The complete imported configuration file remains bounded at
  64 MiB before JSON parsing.
- A necessary binary file larger than 1 MiB and no larger than 8 MiB must be
  carried completely. Do not skip it, truncate it, replace it, or branch on
  its extension. A file of exactly 8 MiB is valid only when the Skill's other
  exported file bytes do not make the total exceed 8 MiB.
- Export bounds each file read by the shared single-file limit, then uses
  checked addition to enforce the decoded total before adding the encoded
  file to the bundle.
- Import validates file count, relative paths, duplicate paths, derived
  Base64 length, decoded single-file length, and checked decoded total before
  creating the target directory or writing any file. Import orchestration
  validates local source metadata completeness before calling the writer; the
  writer receives typed metadata only after that validation.
- Paths must remain UTF-8, non-empty, relative, component-safe, and within the
  512-character limit. Traversal and rooted or absolute paths are rejected.
- Payload paths and the generated `.aio-coding-hub.managed` and
  `.aio-coding-hub.source.json` marker paths form one preflight conflict graph.
  Exact, ancestor, or descendant collisions with either marker are rejected;
  the writer never silently removes or overwrites a payload marker.
- Path comparison follows the target platform: Windows normalizes every UTF-8
  component with stable lowercase comparison, while non-Windows keeps
  case-sensitive components. The same comparison identifies `SKILL.md`, so a
  Windows alias such as `skill.MD` receives the 256 KiB budget and cannot share
  an address with `SKILL.md`. Results are independent of payload order.
- Installed `skill_key` values are directory authority and use one shared
  import/rollback/export validator: exactly one portable `Component::Normal`.
  Separators, `.`/`..`, root/prefix, drive, UNC, and colon forms are rejected
  before staging creation. File paths also reject portable file/directory
  ancestor conflicts (`a` with `a/b`) in either input order.
- Recursive export opens the canonical Skill root once, enumerates directories
  from that handle, and opens every child relative/no-follow. Type, identity,
  size and bytes come from the same child handle; identity changes, hard links,
  symlink escapes, Windows junction/reparse points and special files fail closed.
  Visited identities may skip a symlink directory cycle, but no content is read
  through the symlink path.
- Export authority is not content policy. Every byte from a regular single-link
  file proven inside the Skill root is exported byte-for-byte, including
  credential-looking or `SYNTHETIC_SECRET` test content. Do not add sensitive-word
  scanning, filtering, omission, redaction or content blocking.
- Input and security validation failure is explicit and all-or-nothing with
  respect to validation: export does not return a partial file list, and
  import completes validation before creating the target directory or writing
  files. This does not promise directory-level transactional rollback if a
  filesystem I/O failure occurs after writing begins.
- Each atomic file write creates a randomized same-directory temporary file
  with `create_new`. Temporary cleanup removes only that writer-owned file;
  legal payload names such as `a.aio-tmp` or
  `.aio-coding-hub.source.json.aio-tmp` are never reserved or overwritten.
- `SKILL.md` uses its 256 KiB budget on export/import/restore. Source metadata
  is serialized, checked for completeness and the 64 KiB cap, and held in the
  prepared payload before any ordinary file write.
- Schema v1 continues to preserve legacy Skill state. Schema v2 continues to
  require and restore the complete installed/local Skill payload.

### 4. Validation & Error Matrix

| Boundary / input | Required result |
| --- | --- |
| Export file `> 1 MiB` and `< 8 MiB`, total within 8 MiB | Include every byte and encode with standard Base64 |
| Export file exactly 8 MiB, no other payload bytes | Accept |
| Export file 8 MiB + 1 | Reject the export explicitly |
| Export files individually valid, decoded total 8 MiB + 1 | Reject before returning a bundle payload |
| Export contains 257 files | Reject with `too many skill files` |
| Export encounters a symlink outside the Skill root | Reject with the symlink-escape error |
| Export encounters a directory cycle | Stop at the already visited canonical directory |
| Export encounters a special file or a non-UTF-8 path | Reject explicitly |
| Enumerated file is replaced by a same-name outside hardlink | Reject on relative-open identity/link-count check; export no outside bytes |
| Enumerated directory is replaced by a symlink/junction | Reject on relative no-follow open/identity check; do not traverse outside |
| Root-owned file contains sensitive-looking arbitrary bytes | Round-trip every byte; perform no content filtering |
| Import contains 257 files | Reject before target-directory creation |
| Import path is duplicate, empty, traversal, rooted/absolute, or over 512 characters | Reject before target-directory creation |
| Installed `skill_key` traverses, is absolute, drive/UNC, or contains a separator | Reject before staging/DB activation; preserve old state |
| Paths contain `a` and `a/b` in either order | Reject before target-directory creation |
| Payload equals or nests below either generated marker path | Reject before target-directory creation |
| Windows payload contains `SKILL.md` and `skill.MD` in either order | Reject as a duplicate before target-directory creation |
| Windows payload contains only oversized `skill.MD` | Reject with the dedicated 256 KiB `SKILL.md` budget |
| Non-Windows payload contains `SKILL.md` and `skill.MD` | Treat as distinct case-sensitive paths; each retains its applicable budget |
| Base64 text exceeds the raw-limit-derived cap | Reject before decoding or target-directory creation |
| Base64 text is within the cap but decodes to 8 MiB + 1 | Reject on decoded size before target-directory creation |
| Decoded files are individually valid but total 8 MiB + 1 | Reject before target-directory creation |
| Local source metadata is absent | Accept `None` |
| Local source metadata is complete and within 64 KiB | Preserve the typed metadata |
| Local source metadata is partial, invalid, or oversized | Reject before activating imported Skill state |
| `SKILL.md` exceeds 256 KiB | Reject its dedicated bounded read |
| Config import file exceeds 64 MiB | Reject before UTF-8 or JSON parsing |
| Payload contains names resembling the atomic temporary suffix | Preserve every byte regardless of input order |
| v1 bundle omits full Skill payload | Preserve legacy installed/local state |
| v2 bundle omits installed or local payload | Reject as invalid input |

### 5. Good / Base / Bad Cases

- Good: a synthetic nested `assets/fixture.png` payload is larger than 1 MiB
  but smaller than 8 MiB; export and import reproduce every synthetic byte.
- Good: one synthetic 8 MiB file and no other payload bytes completes a
  bounded round trip.
- Good: input order does not change duplicate/ancestor/marker conflict results.
- Base: small `SKILL.md` and text assets keep the existing schema, paths, and
  Base64 representation.
- Base: installed and local Skill payloads continue to round trip under schema
  v2, while schema v1 keeps its legacy preservation behavior.
- Bad: accept an 8 MiB file plus any non-empty companion file because each
  file is individually legal; the decoded Skill total still exceeds 8 MiB.
- Bad: skip a large resource by extension and emit a bundle that cannot
  reconstruct the source Skill.
- Bad: create the target directory, write early files, and only then discover
  a duplicate path, oversized decoded file, or invalid metadata.
- Bad: accept a payload marker and later delete/overwrite it while generating
  local metadata, or compare paths with raw `PathBuf` on Windows.
- Bad: describe `write_skill_files_to_dir` as a directory transaction that
  rolls back every file after an I/O failure; the helper guarantees
  validation-before-write ordering, not transactional filesystem rollback.

### 6. Tests Required

Keep focused regressions in
`src-tauri/src/infra/config_migrate/tests.rs` for:

- `1 MiB + 1` export acceptance and nested synthetic binary byte-for-byte
  round trip.
- Exactly 8 MiB acceptance and 8 MiB + 1 export/import rejection.
- Multiple individually legal files whose decoded total exceeds 8 MiB.
- Export and import rejection at 257 files.
- Base64 text above the derived cap and decoded bytes above the raw cap; both
  must prove the target directory does not exist after failure. The decoded
  case must first prove its encoded length did not exceed the precheck cap.
- Duplicate, traversal, rooted/absolute, overlong, and non-UTF-8 paths.
- Symlink escape, symlink directory cycles, and special files on platforms
  that expose those filesystem types.
- Deterministic post-enumeration file/directory replacement barriers, including
  Windows hardlink/junction, plus a root-owned sensitive-looking binary round trip.
- v1/v2 compatibility, installed/local restoration, dedicated metadata and
  `SKILL.md` bounds, and the 64 MiB import-file read boundary.
- Import/rollback `skill_key` traversal and Windows drive/UNC forms, proving no
  escaped path, staging residue, or partial DB/Skill activation.
- File/directory ancestor conflicts in both orders and metadata serialization
  overflow before target creation or ordinary file writes.
- Generated marker collisions, Windows case aliases in both orders, platform-
  specific `SKILL.md` alias budgets, and explicit non-Windows case behavior.
- Writer order tests for `a.aio-tmp` plus `a`, and a real
  `export_skill_dir_files` to `write_skill_files_to_dir` byte-for-byte round trip
  containing temporary-like names.

Run at least:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
pnpm tauri:fmt
pnpm tauri:clippy
git diff --check
```

Run the full Rust library suite when production config-migration code or a
shared filesystem helper changes.

### 7. Wrong vs Correct

#### Wrong

```rust
const CONFIG_SKILL_FILE_MAX_BYTES: usize = 1024 * 1024;
const CONFIG_SKILL_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;

// Or: silently omit a large file and continue exporting.
if bytes.len() > CONFIG_SKILL_FILE_MAX_BYTES {
    return Ok(());
}
```

This creates an unsupported 1 MiB/8 MiB asymmetry and can produce an
incomplete bundle. Raising only export or only import would instead create a
bundle that one side accepts and the other rejects.

#### Correct

```rust
const CONFIG_SKILL_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;
const CONFIG_SKILL_FILE_MAX_BYTES: usize = CONFIG_SKILL_TOTAL_MAX_BYTES;
const CONFIG_SKILL_FILE_BASE64_MAX_BYTES: usize =
    CONFIG_SKILL_FILE_MAX_BYTES.div_ceil(3) * 4;
```

Use the shared raw limit on both export and import, keep the decoded total at
8 MiB, validate the complete synthetic payload before writing, and require a
bounded byte-for-byte round trip rather than omission or truncation.

Path correctness uses one normalized component graph for payload paths,
generated markers, duplicate/ancestor checks, and `SKILL.md` classification;
do not bolt marker deletion or case handling onto the write phase.
