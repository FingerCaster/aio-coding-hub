# Design: Merge upstream main 4f02ba3d

## Repository Topology

| Item | Value |
| --- | --- |
| Pre-task local main | `099cf90d8b05c5fd1f39cb4f0fafd624b131da66` |
| Fork remote input | `origin/main@1a551cbee35960fbb954e475a13b2d8d55d709df` |
| Upstream input | `upstream/main@4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee` |
| Fork/upstream merge base | `419086fb36a4976e30d384add2fec086d99e648c` |
| Divergence | 285 fork-only commits, 7 upstream-only commits |
| Isolated branch | `FingerCaster/merge-upstream-2026-07-29` |
| Isolated worktree | `D:/UGit/aio-coding-hub-fork-upstream-merge-main-20260729` |

The worktree starts from the activated-task commit descended from the pre-task
local main. It first merges the fixed origin SHA, then merges the fixed upstream
SHA. This makes the final branch a descendant of local `main`, allowing the
final main update without rewriting history.

## Upstream Change Set

1. `de09d645`: hide unknown Bundle/runtime mode in About UI.
2. `7bd1812f`: restore Claude OAuth through `claude.ai` and use the required
   Anthropic token-exchange identity.
3. `c9326c0a`: add usage folder ranking and development-time estimation.
4. `84564a5b`: update cached OAuth expiry/status after token refresh.
5. `7cc1d8ac`: strip client `chatgpt-account-id` and inject only the selected
   provider account identity.
6. `d27efdb8`: add provider latency/TTFB/output-rate trend metrics.
7. `4f02ba3d`: upstream `0.60.16` release metadata.

## Merge Sequence

1. Create the isolated worktree from the activated local task commit.
2. Merge `origin/main@1a551cbe` with `--no-ff`; validate that local-only task
   history and all remote fork commits are ancestors.
3. Merge `upstream/main@4f02ba3d` with `--no-ff --no-commit`.
4. Resolve and stage each conflict, then audit both conflict and auto-merged
   files before committing the upstream merge.
5. Run focused and full validation, archive task metadata, and create any
   required bookkeeping commit on the isolated branch.
6. Re-fetch neither pinned input during validation. If `main` moves, merge the
   new local main into the isolated branch and rerun affected gates.
7. Protect the main worktree with a named stash, fast-forward or merge the
   validated branch into `main`, reapply the stash, and verify restoration.

## Predicted Conflict Matrix

| Paths | Resolution contract |
| --- | --- |
| `.release-please-manifest.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json` | Keep fork version `0.60.30`; retain unrelated upstream dependency entries if any. |
| `CHANGELOG.md` | Keep the fork changelog. Do not add a second, incompatible upstream `0.60.16` release section. |
| `package.json` | Keep fork scripts and plugin tooling; import upstream React, React Router, Vitest, and related compatible dependency updates; keep version `0.60.30`. |
| `pnpm-lock.yaml`, `pnpm-workspace.yaml` | Preserve fork workspace packages and security overrides, add upstream compatible overrides, then regenerate/validate the frozen lockfile from the resolved manifests. |
| `src-tauri/src/gateway/oauth/token_exchange.rs` | Preserve fork bounded/redacted token responses, safe errors, invalid-grant classification, and privacy tests; add upstream Anthropic token request user-agent helper and tests. |
| `src-tauri/src/gateway/util.rs` | Preserve fork UTF-8-safe 256-byte model sanitization and tests; remove client `chatgpt-account-id` in the shared auth cleanup and retain upstream isolation tests. |
| `src/pages/providers/__tests__/providerEditorOAuthActions.test.ts` | Retain fork model-catalog/editor action coverage and add upstream OAuth-status expiry cache assertions. |
| `src/query/__tests__/providers.test.tsx` | Retain fork model-catalog and account-usage cache contracts and add upstream OAuth-status cache write/invalidation coverage. |

## Auto-Merge Semantic Audit

- Verify `claude.rs`, `upstream_identity.rs`, and the resolved token exchange
  jointly use the `claude.ai` authorize route without leaking token URI,
  client identity, tokens, response bodies, or remote error text.
- Verify `codex_chatgpt.rs` injects the selected provider account ID only after
  the shared cleanup removes any client-supplied ID.
- Verify provider OAuth refresh writes the new expiry through the existing
  editor/query ownership model without invalidating model catalogs, account
  usage, availability, routing, or circuit state.
- Verify the two upstream usage features coexist with fork account-usage and
  model-routing work across Rust DTOs, generated bindings, query keys,
  services, and UI tabs.
- Verify generated bindings match Rust source and no hand-edited incompatible
  binding survives.
- Verify upstream package upgrades do not remove fork plugin scripts,
  workspace packages, dependency security overrides, or hook scripts.

## Worktree Preservation

- Before final main integration, record `git status --porcelain=v2`, hashes or
  absence of every dirty path, and the named stash object ID.
- Use `git stash push --include-untracked` only in the main worktree after all
  task-owned files are committed on the integration branch.
- Apply rather than immediately drop the stash. Verify modified-file hashes,
  deleted-path absence, untracked-path presence/hashes, and staged-state
  restoration before considering the main update complete.
- Keep the stash until the task is fully verified, providing a recovery point
  if restoration is later questioned.

## Rollback

- Before the upstream merge commit, abort only the merge in the isolated
  worktree; local main remains untouched.
- After the merge commit but before main integration, leave the branch and
  worktree intact for inspection rather than resetting main.
- During final integration, the named stash is the recovery source for user
  changes. If the validated branch no longer descends from local main, update
  it in the isolated worktree and rerun validation instead of forcing main.
- A merge-origin defect discovered later is reverted as a whole merge commit;
  upstream-origin defects are tracked separately.
