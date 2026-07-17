# Round-4 break-loop retrospective

## Root cause clusters

1. Filesystem checks treated a canonical path as durable authority. History, persist and Skill export then
   reopened a name after validation, so the consumer was not bound to the object that had been checked.
2. Settings serialization was locked, but several owners constructed a complete snapshot before acquiring the
   lock. Rollback repeated the same stale whole-snapshot write and could overwrite a later owner.
3. Frontend hydration budgets measured decoded length only after Base64 had crossed IPC. The visible concurrency
   limit bounded Promise count, not native read/encode allocation.
4. Failure and diagnostic layers bounded successful payloads more carefully than failure payloads/capabilities.
   Parser errors and IPC args were treated as harmless metadata even though they can embed raw input or bearer IDs.
5. Trellis validation detected missing JSONL targets but archive did not transform self-references before moving a
   task, so a valid active task could create invalid archived state.

## Failed or incomplete approaches discovered during implementation

- Path canonicalization plus a second `open(path)` was insufficient even with a final containment check. The fix
  had to carry expected identity into handle-relative open and read the same handle.
- Merely changing settings writers to `update` was insufficient for external-side-effect rollback. Rollback also
  needed a committed-field comparison/CAS and had to skip runtime/autostart restoration when CAS lost.
- Keeping the TypeScript hydration counter while adding a batch IPC would still leave the first allocation guard
  after IPC. The authoritative reservation moved into Rust before `read_to_end`/Base64.
- Parsing structured upstream errors and truncating them could still preserve a complete secret in the first 512
  characters. The failure body is now capped and discarded; only a fixed status summary crosses IPC.
- The first archive validator refactor accidentally placed the single-task validation branch after an early
  function return. The unit test plus `task.py validate --all` caught the control-flow regression before archive.
- Initial focused frontend expectations still asserted one IPC call per thumbnail and raw upstream error text.
  Tests were updated to assert one backend-budgeted batch and status-only failure diagnostics.

## Evidence and prevention

- Deterministic hooks run precisely after enumeration/validation and before relative open/create/read. Tests swap
  same-name hardlinks, directories and junctions; outside sentinels remain unchanged.
- Skill tests prove both halves of the product decision: root-owned `SYNTHETIC_SECRET`-looking bytes round-trip
  byte-for-byte, while root-outside bytes never enter the bundle.
- A real `grok_config::set` writer is interleaved with Image Gen root mutation and a post-commit concurrent Grok
  update. Field-owned RMW preserves the root; CAS rollback preserves the newer Grok value.
- Rust batch hydration counts actual read starts and proves aggregate rejection happens before the next read.
- JSON and multipart production transports receive an oversized synthetic-secret error body and return only the
  fixed bounded summary. Frontend tests capture task persistence and console logs.
- Archive unit coverage rewrites only self references, leaves unrelated spec paths unchanged and then runs the same
  all-manifest validator used by the command.
- Break-loop spec-sync audit found `src/templates/markdown/spec` present but no existing `cross-layer` mirror.
  The complete project cross-layer spec set is therefore mirrored under the new
  `src/templates/markdown/spec/cross-layer/` directory so the index retains valid relative links. Final
  validation compares SHA-256 for every round-4-modified contract and the index, including settings/archive.
