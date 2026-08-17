# Full-Scope Check Brief

Review and fix the entire diff from `origin/main`/`HEAD`, not only the latest
follow-up edits. Work only in
`D:\\OrcaProjects\\aio-coding-hub-fork\\codex-gpt56-372k-context`.
Do not commit, push, merge, package, or publish. Other agents may have already
edited the shared worktree; preserve their changes.

Read `prd.md`, `design.md`, `implement.md`, `check.jsonl`, every referenced
spec/research file, the root package spec indexes and their Quality Check
sections. This is the final Phase 2.2 pass for package `aio-coding-hub`, layers
`backend` and `cross-layer`.

Audit at minimum:

- Decimal policy is exactly `372000`; `380928` occurs only in negative tests
  or explicitly superseded research notes.
- Only `gpt-5.6-sol`, `gpt-5.6-terra`, and `gpt-5.6-luna` have both catalog
  windows rewritten. Missing or duplicated target slugs share the dedicated
  error; non-target duplicates retain the base-catalog error.
- Settings schema 64, default false, migration, read-only projection,
  generated bindings, ordinary-writer non-ownership, export/import isolation,
  dedicated transaction, CAS compensation, and Codex-home locking are coherent.
- Managed catalog metadata v2, source binding, hashes, prepared/apply guards,
  direct/proxy/offline/startup/CLI-fingerprint lifecycle, raw/structured config
  saves, and zero/nonzero managed Profile behavior preserve user data.
- Every committed config/generated/backup/manifest target rolls back
  independently by its own token; concurrent drift is preserved and recovery
  errors cannot be downgraded.
- UI/query/service behavior is backend-confirmed, pending-safe, read-only-safe,
  invalidates all affected queries, displays exact `372,000`, promises only new
  sessions, and blocks every Codex-home control while active or writing.
- No installed Codex binary/file is modified, no global context/auto-compact
  override is introduced, and 95% upstream semantics remain unchanged.

Known validation already passed before the last Rust-only follow-ups:

- `pnpm check:generated-bindings`
- `pnpm typecheck`
- `pnpm lint`
- `pnpm test:unit` (308 files, 2876 tests)
- release source/contract/overlay/promotion/channel/signing/support-matrix/
  Homebrew/CI-scope self-tests
- real Codex 0.147.0 smoke: `272000 -> 372000 -> 272000`

Run at least focused checks plus `pnpm typecheck`, `pnpm lint`, Rust fmt/check,
and `git diff --check`. Fix every confirmed finding directly, add regression
tests where needed, and report remaining risks explicitly.
