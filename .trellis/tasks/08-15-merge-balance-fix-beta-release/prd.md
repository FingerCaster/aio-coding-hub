# 合并余额修复并发布新版 Beta

## Goal

合并余额刷新修复、Codex 流错误设置 UI 拆分、CX2CC 嵌套首字节超时预算修复与 Claude Code 客户端 usage 归一化，完成 beta 发布门禁并发布新版 beta；仅清理已合并、无未提交改动且不再使用的 Orca worktree。

## Requirements

- Integrate only the completed Sub2API balance-refresh implementation from
  `FingerCaster/fix-sub2api-balance-window-refresh`; do not import its journal
  commit or overwrite unrelated user changes in the main worktree.
- Move Codex stream-internal-error controls out of the general settings surface
  into the Codex tab and keep provider-level controls scoped to Codex providers.
- Preserve the inner AIO gateway's ownership of response-header and first-event
  timeouts for authenticated CX2CC reentry, without weakening ordinary upstream
  timeout enforcement or Codex continuation-repair safety.
- Normalize OpenAI Responses usage only at the Anthropic client projection so
  `input_tokens`, cache-read tokens, and cache-creation tokens are mutually
  exclusive for Claude Code context accounting. Preserve the inclusive raw
  provider usage for quota, cost, and logs, and do not re-normalize upstream
  values that already use Anthropic top-level cache semantics.
- Validate the integrated Rust behavior and the repository release gates that
  protect generated bindings, formatting, type safety, lint, and tests.
- Publish the next prerelease through the repository's existing beta release
  contract, using `origin` only and an immutable commit SHA after the tag is
  resolved or created.
- Remove Orca-managed worktrees only when they are no longer in use, have no
  uncommitted changes, and contain no unique unmerged commit.
- Preserve every pre-existing dirty or untracked file in the main worktree.

## Acceptance Criteria

- [ ] The Sub2API manual balance-refresh fix, Codex-only stream settings UI,
      authenticated CX2CC nested-TTFB behavior, and client-visible usage
      normalization are present on
      `FingerCaster/beta8-balance-cx2cc-integration` with focused and full checks green.
- [ ] Cached-read and cache-creation usage cannot be counted a second time in
      Claude Code's visible `input_tokens`, while provider quota/cost/log usage
      retains the original inclusive totals in streaming and non-streaming paths.
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
- Usage normalization is an independent child task because the observed
  approximately half-window auto-compaction can be caused by cached tokens being
  exposed both inside OpenAI's inclusive input total and in Anthropic cache fields.
- The source branch's `104f27e6` journal commit is explicitly out of scope.
