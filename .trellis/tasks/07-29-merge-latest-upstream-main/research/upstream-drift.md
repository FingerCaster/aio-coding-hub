# Upstream Drift Research: 2026-07-29

## Fixed Inputs

- Pre-task local main: `099cf90d8b05c5fd1f39cb4f0fafd624b131da66`
- Pinned origin main: `1a551cbee35960fbb954e475a13b2d8d55d709df`
- Pinned upstream main: `4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`
- Fork/upstream merge base: `419086fb36a4976e30d384add2fec086d99e648c`
- Divergence from the merge base: 285 fork-only commits and 7
  upstream-only commits.
- Upstream drift size: 75 files, 5,651 insertions, 2,699 deletions.

The remotes were fetched explicitly. `origin` fetch/push both resolve to
`https://github.com/FingerCaster/aio-coding-hub.git`; `upstream` fetch resolves
to `https://github.com/dyndynjyxa/aio-coding-hub.git` and its push URL is
`DISABLED`.

## Upstream Commits

| SHA | Intent | Principal surfaces |
| --- | --- | --- |
| `de09d645` | Do not show unknown Bundle/runtime mode | About settings UI/tests |
| `7bd1812f` | Restore Claude OAuth via claude.ai authorize endpoint | Claude adapter, token exchange, upstream identity |
| `c9326c0a` | Usage folder ranking and estimated development time | Rust usage domain, bindings, home/usage query/service/UI |
| `84564a5b` | Refresh displayed OAuth expiry after token refresh | Provider editor action context, provider query/cache |
| `7cc1d8ac` | Isolate client ChatGPT account headers | Shared auth cleanup and Codex ChatGPT request preparation |
| `d27efdb8` | Provider latency/TTFB/output-rate metric trends | Rust usage metrics, bindings, query/service/UI/chart |
| `4f02ba3d` | Release upstream 0.60.16 | Version sources and changelog |

## Merge Preview

Merging the pinned origin input into pre-task local main produces no textual
conflict. A merge preview between pinned origin and pinned upstream reports 11
textual conflicts:

1. `.release-please-manifest.json`
2. `CHANGELOG.md`
3. `package.json`
4. `pnpm-lock.yaml`
5. `src-tauri/Cargo.lock`
6. `src-tauri/Cargo.toml`
7. `src-tauri/src/gateway/oauth/token_exchange.rs`
8. `src-tauri/src/gateway/util.rs`
9. `src-tauri/tauri.conf.json`
10. `src/pages/providers/__tests__/providerEditorOAuthActions.test.ts`
11. `src/query/__tests__/providers.test.tsx`

The release conflicts are caused by fork `0.60.30` versus upstream `0.60.16`.
The package conflict also combines fork plugin scripts/security overrides with
upstream dependency upgrades. The four code/test conflicts are compatible
behavior additions rather than mutually exclusive product choices:

- Fork token exchange adds bounded, redacted failures and precise
  authorization-code versus refresh-token `invalid_grant` classification.
  Upstream adds the Anthropic token-request user agent required by the restored
  Claude OAuth flow. Both must remain.
- Fork gateway util adds UTF-8-safe, 256-byte provider model sanitization.
  Upstream removes client-supplied `chatgpt-account-id` in the shared auth
  cleanup. Both must remain.
- Fork provider editor/query tests cover model catalogs and account-usage
  ownership. Upstream tests cover OAuth status/expiry cache updates. Both
  suites must remain.

## Semantic Overlap To Audit

- `claude.rs` and `upstream_identity.rs` auto-merge around the conflicted token
  exchange file. The final call chain must select the new authorize endpoint
  and Anthropic identity while preserving secret-free fork diagnostics.
- `codex_chatgpt.rs` auto-merges around the conflicted shared header cleanup.
  The final request must discard the client's account ID and inject only the
  selected provider account ID.
- Provider editor/query production files auto-merge around conflicted tests.
  OAuth refresh must update only the intended provider OAuth cache and leave
  model catalogs, account usage, availability, routing, and circuits alone.
- Both usage features span Rust DTOs, generated bindings, query keys, services,
  and UI. Generated bindings and all relevant tests must be checked together.
- `pnpm-workspace.yaml` auto-merges, but the result must retain fork package
  workspaces/security overrides and upstream's compatible override additions.

## Scope Boundary

The merge owns only fixed-SHA ancestry, conflict reconciliation, auto-merge
semantic auditing, regression validation, and final local-main integration.
Any defect that exists unchanged on `4f02ba3d` without the fork merge is an
upstream-origin finding and must not be repaired in this merge commit.
