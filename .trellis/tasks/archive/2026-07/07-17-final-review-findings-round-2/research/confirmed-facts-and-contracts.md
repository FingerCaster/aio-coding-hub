# Confirmed facts and executable contracts

## Scope and sources

Read-only evidence collected 2026-07-17 from current HEAD `bb95d18a`, baseline `2e43ee23`, the eight
review findings, current code/tests/specs, archived task artifacts, and local Git objects. No remote was
accessed and no secret-bearing file was copied into this artifact.

## Finding matrix

| Finding | Confirmed implementation fact | Contract selected |
| --- | --- | --- |
| F1 | `history.rs` writes predictable names with `std::fs::write`; stored-file canonical check is post-write. | Exclusive `root/id` creation + `create_new`; rollback only newly created dir; external inode unchanged. |
| F2 | `decode_skill_files_for_write` uses case-sensitive `HashSet<PathBuf>`; markers are removed/overwritten after payload writes. | Payload and generated markers share a platform-aware duplicate/ancestor graph; Windows case aliases conflict. |
| F3 | `is_disallowed_ip` handles mapped IPv4 but passes compatible `::/96` values to generic IPv6 table. | Reject compatible/reserved `::/96`; mapped addresses retain IPv4 classification; all DNS answers global. |
| F4 | nonpositive token expiry becomes `None`; device expiry accepts 0/max; Grok pending/slow_down collapse. | Bounded checked expiry; invalid success response never persists; explicit slow_down drives +5s subsequent interval. |
| F5 | row `dir` is absolute, but validation always supplies current settings root. | Settings v52 owns a canonical historical-root allowlist; DB paths remain untrusted candidates and old rows remain operable. |
| F6 | MIME parser is invoked in form construction after decode. | Invoke the same MIME parser during all-file preflight before first decode. |
| F7 | NewAPI research is pre-fix; parent map says five children; merge evidence lacks decisions. | New redacted live check, reproducible merge decision audit, real seven-child map, no secrets. |
| F8 | SQL sort has two keys but cursor/filter has one. | Versioned opaque composite cursor and keyset seek, explicit invalid/legacy rejection. |

## Merge evidence discrepancy

- Merge commit: `9e5da3461e2db200a488cef17ac85ecd52c0d6e2`.
- Parents: fork `4499c71d17e3d51544e57fdebabb1831b9676d37`, upstream
  `419086fb36a4976e30d384add2fec086d99e648c`; merge-base
  `057c06821b5159fda202bce5cfbf1ef3afb410f9`.
- `git show --remerge-diff` identifies 30 files containing conflict markers and 47 marker groups.
- Archived implement text says “31 text conflicts” but does not define file/hunk/index-entry counting and contains
  no table. The final audit must preserve this discrepancy and use reproducible file-level evidence; it must not
  claim a missing user decision.

## Live validation privacy contract

Allowed output: timestamp, method count, HTTP status class, JSON field path/type, public unit enum, finite boolean,
formula equality boolean, cache-header presence boolean, final pass/fail. Forbidden output: token/API key, base URL
or host, URL query, complete body, account/user identifiers, token name, PII, actual monetary/quota values.

## Test assertions

- Effects: invalid input leaves filesystem/DB/settings/provider tokens unchanged.
- Ordering: every preflight failure occurs before decode/write/persist.
- Pagination: concatenated IDs equal the sorted source set exactly once across pages.
- Migration: existing settings gain an empty roots list; the first controlled switch registers the previous current
  root before changing it, without moving bytes. Malformed/out-of-allowlist DB rows fail closed when accessed.
- Cross-layer: Rust DTO, generated binding, service wrapper, controller state and tests change together.
