# 执行计划：已完成任务记录收口

## 前置门

- [x] 用户审阅 PRD、设计和执行清单并批准实施。
- [x] `implement.jsonl` 与 `check.jsonl` 使用真实 spec/research 条目并通过校验。
- [x] `task.py start 08-10-reconcile-completed-task-archives` 后才执行移动、删除和归档写入。

## 1. 快照与差异审阅

- [x] 记录 `git status`、staged 集合、`task.py list`、分支/stash 数量和受保护用户路径哈希。
- [x] 逐个比较 5 组 active/archive；将语义结论记录到 research，不以行数相等代替内容审阅。

## 2. 收口已有归档的重复记录

- [x] 把 `08-08-account-usage-route-gate` 的稳态阻断研究及相关结论合入正式归档。
- [x] 审阅 `08-03-circuit-breaker-probe-failback` 的双边设计差异，保留后续权威内容与未被取代的 active 证据。
- [x] 对 5 个精确 active 绝对路径做仓库边界验证后逐项移除；每次移除后复核正式归档仍完整。

## 3. 归档已完成但未收口的任务

- [x] 为 `07-16` 与 08-05 三个任务记录当前 `main` 的行为/提交证据。
- [x] 依次 `task.py archive --no-commit`：`08-05-transport-error-retry-backoff`、`08-05-codex-zstd-request-body`、`08-05-second-group-request-reliability`、`07-16-codex-auto-review-route-neutral`、`00-join-fingercaster`。
- [x] 复核父子引用、completed 状态、归档路径和 task list 唯一性。

## 4. 规范与验证

- [x] 更新 task archive contract 的同名 active shadow 防复发条款。
- [x] 运行所有受影响 task context 校验、全仓 context validation、spec link、`git diff --check`。
- [x] 运行 `trellis-check` 全范围审阅并修复记录丢失、路径错误或意外差异。
- [x] 复核用户脏文件、分支、stash 和 `origin/main` 基线不变。

## 5. 提交与完成

- [x] 按 Phase 3.4 提交计划取得用户确认，只提交本任务范围。
- [ ] 运行 finish-work，归档本清理任务并写入 journal；不推送。

## 验证命令

```powershell
python ./.trellis/scripts/task.py list
python ./.trellis/scripts/task.py validate <affected-task>
node scripts/check-spec-links.mjs
git diff --check
git status --short
```

## 回滚点

- 任一 active 副本含未归并资料：停止删除，先补入正式归档。
- 任一 archive 命令失败：停止后续移动，保留当前 Git diff 供恢复。
- 用户路径集合或哈希变化：停止并恢复本任务引起的变化，不触碰原始用户内容。
