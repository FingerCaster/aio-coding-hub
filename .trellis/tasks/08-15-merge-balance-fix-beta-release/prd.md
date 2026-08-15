# 合并余额修复并发布新版 Beta

## Goal

合并余额刷新修复、Codex 流错误设置 UI 拆分与 CX2CC 嵌套首字节超时预算修复，完成 beta 发布门禁并发布新版 beta；仅清理已合并、无未提交改动且不再使用的 Orca worktree。

## Requirements

- Integrate only the completed Sub2API balance-refresh implementation from
  `FingerCaster/fix-sub2api-balance-window-refresh`; do not import its journal
  commit or overwrite unrelated user changes in the main worktree.
- Move Codex stream-internal-error controls out of the general settings surface
  into the Codex tab and keep provider-level controls scoped to Codex providers.
- Preserve the inner AIO gateway's ownership of response-header and first-event
  timeouts for authenticated CX2CC reentry, without weakening ordinary upstream
  timeout enforcement or Codex continuation-repair safety.
- Validate the integrated Rust behavior and the repository release gates that
  protect generated bindings, formatting, type safety, lint, and tests.
- Publish the next prerelease through the repository's existing beta release
  contract, using `origin` only and an immutable commit SHA after the tag is
  resolved or created.
- Remove Orca-managed worktrees only when they are no longer in use, have no
  uncommitted changes, and contain no unique unmerged commit.
- Preserve every pre-existing dirty or untracked file in the main worktree.

## Acceptance Criteria

- [ ] The Sub2API manual balance-refresh fix, Codex-only stream settings UI, and
      authenticated CX2CC nested-TTFB behavior are present on
      `FingerCaster/beta8-balance-cx2cc-integration` with focused and full checks green.
- [ ] The new beta tag and GitHub prerelease point to the exact validated commit,
      and the release workflow completes successfully.
- [ ] All removed worktrees satisfy the clean-and-merged proof; dirty or unique
      worktrees are retained and reported.
- [ ] Main-worktree user changes remain byte-for-byte outside this task's files.

## Notes

- Candidate implementation commit: `33954c99`.
- UI reference commit: `926279e6`; timeout references: `27efa801`, `39f85ae5`,
  and `d7921ee5`. These are semantic references only and must not overwrite newer
  `origin/main` behavior.
- The source branch's `104f27e6` journal commit is explicitly out of scope.
