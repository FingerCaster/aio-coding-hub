# 收口并归档已完成 Trellis 任务

## Goal

让 `.trellis/tasks/` 只保留真实未完成任务：保全已完成任务的独有记录，清除会遮蔽正式归档的 active 副本，并把已进入 `main` 但未归档的任务及 onboarding 任务完整归档。

## Background

- `08-09-orca-cleanup-release` 已在提交 `1e07c484` 归档，当前任务列表中的残留来自更早的并行 worktree/task 生命周期未统一收口。
- 5 个 active 目录已有同名正式归档：`07-19-adapt-ai-input-account-usage`、`08-03-circuit-breaker-probe-failback`、`08-08-account-usage-route-gate`、`08-08-custom-account-usage-routing`、`08-08-custom-account-usage-script`。
- 4 个业务任务已由当前 `main` 的代码与测试证明完成，但没有当前分支可见的完整归档：`07-16-codex-auto-review-route-neutral`、`08-05-second-group-request-reliability`、`08-05-transport-error-retry-backoff`、`08-05-codex-zstd-request-body`。
- `00-join-fingercaster` 是 2026-07-14 创建的 onboarding 任务；开发者已经完成 Trellis 工作流、spec、task、archive 与 journal 的实际使用，满足其完成意图。
- `.trellis/tasks/08-10-beta-release-channel/` 及其并发子任务
  `08-10-beta-release-pipeline`、`08-10-beta-updater-core`、`08-10-beta-update-ui` 属于后续 beta
  发布工作，必须保持 active 且不得纳入本次归档。
- active 与 archive 并非都可直接二选一：`08-08-account-usage-route-gate` active 副本包含归档后补充的稳态余额阻断故障分析；`08-03-circuit-breaker-probe-failback` 两边设计记录存在差异。
- 上述 active 业务目录当前均为未跟踪文件。`AGENTS.md`、`.orca/`、HTML 和其他非本任务路径属于用户或其他工作，不得纳入。

## Requirements

- `R1`：逐项比较同名 active/archive 内容；已有归档目录是状态与路径权威，但 active 独有且未被后续内容取代的研究、决策和验收证据必须先合入归档。
- `R2`：`08-08-account-usage-route-gate` 必须保留 `research/stable-blocked-failback-loop.md` 及相关稳态门控结论；`08-03-circuit-breaker-probe-failback` 必须完成语义差异审阅，不能用整目录覆盖。
- `R3`：确认无独有信息后才移除 5 个已归档任务的 active 副本；删除目标必须解析为仓库内 `.trellis/tasks/<exact-name>`，不得使用宽泛 glob。
- `R4`：将 4 个代码已完成任务与 `00-join-fingercaster` 归档。业务任务记录应注明当前 `main` 的落地证据；08-05 子任务先于父任务归档。
- `R5`：归档操作使用 `task.py archive --no-commit`，避免在当前清理任务的 Phase 3.4 之前产生交错自动提交；最终由一个受控工作提交收口。
- `R6`：不修改业务代码、不恢复或删除分支/stash、不推送、不改变发布结果；保留所有任务以外的用户脏文件和未跟踪文件。
- `R7`：更新现有 Trellis task archive contract，记录“同名 active 副本可遮蔽正式归档，删除前必须做语义差异审阅”的可执行防复发约束；不修改 Trellis 脚本实现。

## Acceptance Criteria

- [x] 5 个既有归档目录均保持 `completed`，active 同名目录全部消失，且 active 独有研究/决策已进入对应归档。
- [x] `07-16`、08-05 父任务及两个子任务、`00-join-fingercaster` 均有唯一归档目录，状态为 `completed`，无同名 active 目录。
- [x] `task.py list` 在当前清理任务完成前只显示本任务和上述 4 个并发 beta 任务；本任务归档后不再显示上述 10 个旧条目。
- [x] 所有受影响 `implement.jsonl` / `check.jsonl` 自引用均指向归档路径，且全仓 context validation 通过。
- [x] `git diff --check` 通过；最终差异只包含任务归档/研究、archive contract、本任务和 journal，不包含业务代码或用户文件。
- [x] `AGENTS.md`、`.orca/`、其他用户未跟踪文件、分支、stash 和 `origin/main` 均保持不变。
- [ ] 完成 `trellis-check`、spec 评估、受控提交、本任务归档和 session journal。

## Out Of Scope

- 修改 `task.py list/archive` 的重复名检测或自动合并行为。
- 重新实现、重测或改动已发布的业务功能。
- 清理本地分支、stash、Orca runtime handle 或其他仓库。
- 推送本地 Trellis 记录提交。
