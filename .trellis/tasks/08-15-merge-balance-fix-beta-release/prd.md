# 合并余额修复并发布新版 Beta

## Goal

定位并合并刚完成的余额问题修复，完成 beta 发布门禁并发布新版 beta；仅清理已合并、无未提交改动且不再使用的 Orca worktree。

## Requirements

- Integrate only the completed Sub2API balance-refresh implementation from
  `FingerCaster/fix-sub2api-balance-window-refresh`; do not import its journal
  commit or overwrite unrelated user changes in the main worktree.
- Validate the integrated Rust behavior and the repository release gates that
  protect generated bindings, formatting, type safety, lint, and tests.
- Publish the next prerelease through the repository's existing beta release
  contract, using `origin` only and an immutable commit SHA after the tag is
  resolved or created.
- Remove Orca-managed worktrees only when they are no longer in use, have no
  uncommitted changes, and contain no unique unmerged commit.
- Preserve every pre-existing dirty or untracked file in the main worktree.

## Acceptance Criteria

- [ ] The Sub2API manual balance-refresh fix is present on
      `FingerCaster/beta-release-channel` with its focused and full checks green.
- [ ] The new beta tag and GitHub prerelease point to the exact validated commit,
      and the release workflow completes successfully.
- [ ] All removed worktrees satisfy the clean-and-merged proof; dirty or unique
      worktrees are retained and reported.
- [ ] Main-worktree user changes remain byte-for-byte outside this task's files.

## Notes

- Candidate implementation commit: `33954c99`.
- The source branch's `104f27e6` journal commit is explicitly out of scope.
