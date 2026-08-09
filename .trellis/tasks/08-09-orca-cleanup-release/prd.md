# 清理 Orca 工作区并发布版本

## Goal

安全清理已完成且不再需要的 Orca 终端与 Git worktree，然后从 `origin` 对应的
`FingerCaster/aio-coding-hub` 仓库发布稳定版 `0.60.40`，并持续验证发布流水线与最终制品状态。

## Background

- 用户要求按“先清理、后发布”的顺序执行。
- 当前发布基线为 `0.60.39`：`.release-please-manifest.json` 与根 `package.json`
  均记录该版本。
- 发布入口为 `.github/workflows/release.yml`，由 release-please 或显式
  `workflow_dispatch` 创建/复用草稿 Release，并以解析后的不可变提交 SHA 构建、组装、
  校验和发布制品。
- 当前 Git 主 worktree 位于 `D:/UGit/aio-coding-hub-fork`，分支为 `main`，存在用户已有的
  未提交改动；这些改动不得因清理或发布而丢失、覆盖或误纳入发布提交。
- `origin/main` 当前为已打 `aio-coding-hub-v0.60.39` tag 的
  `a8c525cdaadce77dd4b00363962e501bc5fae491`；本地 `main` 在该提交之上领先 25 个已提交
  变更，尚未推送。
- 当前 Git 注册了主 worktree 及 8 个 Orca 子 worktree。8 个子 worktree 均无未提交内容，
  其中的 Agent 均已结束，对应 Trellis 任务均已归档为 `completed`；分支提交大多通过
  cherry-pick/整合进入本地 `main`，所以原提交哈希不一定是 `main` 的祖先。
- 历史会话确认：清理应通过 Orca CLI 完成，不能用原始目录删除或绕过 Orca 元数据；
  `orca worktree rm` 只移除 checkout/Orca 注册，本次保留对应 Git 分支作为恢复点。
- GitHub 上现有 release-please PR #30 错误地把版本从 `0.60.39` 降到 `0.60.6`。即使其
  检查通过也不得合并；发布前必须刷新或重建为正确的下一版本 PR。
- 当前 `.github/workflows/release.yml` 的手动 tag 校验只接受稳定三段式版本号，不支持
  prerelease tag；Beta 发布频道仍属于未实现的独立功能。
- 本仓库的 GitHub 发布、Actions、PR 与 Release 操作默认且仅针对 `origin`
  (`FingerCaster/aio-coding-hub`)；除非用户另行要求，不检查或操作 `upstream`。

## Requirements

- **R1 清理候选判定**：仅将同时满足下列条件的子 worktree 判为可清理：Agent 已结束、
  对应任务已归档完成、工作树无未提交或未跟踪内容，并且已有祖先关系、patch 等价性或
  明确的整合记录证明成果已进入待发布 `main`；原分支必须保留作为恢复点。
- **R2 保留边界**：保留主 worktree、当前会话终端、仍在运行/等待用户决策的终端、脏工作树、
  以及缺少完成/整合证据的 worktree；不删除分支，不清理 stash。
- **R3 Orca 一致性**：先使用 Orca CLI 停止候选 worktree 的终端，再使用 Orca CLI 移除
  worktree，并在每批操作后同时核对 Orca 列表与 `git worktree list`。若出现 stale PTY，
  仅使用版本匹配指南允许的 Orca 恢复路径，不直接删除目录。
- **R4 发布源完整性**：发布前确认本地 `main`、`origin/main`、待发布提交和发布 PR/Tag 的关系，
  不从脏工作树构建或发布，不将当前未提交 Trellis/用户文件意外带入版本。
- **R5 发布方式**：沿用仓库现有 release-please 与 `.github/workflows/release.yml` 流程；所有
  `gh` 操作显式指定 `FingerCaster/aio-coding-hub`，并按仓库规则设置默认仓库。
- **R6 发布验证**：监控目标 Actions run 到终态，确认构建、候选制品组装、不可变来源校验、
  Release 发布及适用的 Homebrew 发布任务均成功；失败时停止扩散并报告可恢复点。
- **R7 版本一致性**：最终 Git tag、GitHub Release、版本清单、应用配置与发布制品必须指向
  同一版本和同一不可变提交。
- **R8 错误 PR 隔离**：PR #30 在版本仍为 `0.60.6` 时不得合并；需通过标准流程刷新为正确
  版本，或关闭后重建等价的 release-please PR，并重新等待必需检查通过。
- **R9 发布目标**：本次发布稳定版 `0.60.40`。不复用或扩展尚未实现的 Beta 频道；主
  worktree 中等待 Beta 讨论的已结束终端可作为无用终端关闭，Beta 需求留待后续独立任务。

## Acceptance Criteria

- [x] 每个被清理 worktree 都有清理前的干净状态、合入状态和 Agent 结束状态证据。
- [x] 清理后 Orca 与 Git 均不再列出已移除 worktree，且所有保留项仍可用。
- [x] 主 worktree 的用户既有未提交改动在清理和发布前后保持不丢失、不被改写。
- [x] 稳定版 `0.60.40` 由 `origin` 仓库的标准发布流程触发，且发布源为明确的不可变提交 SHA。
- [x] 目标 GitHub Actions 发布 run 成功结束；GitHub Release 已发布且制品齐全。
- [x] 发布后的版本、tag、Release target、制品来源与 `main` 历史相互一致。
- [x] Orca 主 worktree 状态注释更新为最终发布结果，保留当前会话终端。

## Out Of Scope

- 不清理或修改 `upstream`。
- 不删除本地分支或 stash。
- 不顺带实现 Beta 发布频道设计；若本次选择稳定版，仅发布现有稳定流程支持的下一版本。
- 不修复发布前检查发现但与本次清理/发布无关的代码缺陷；此类问题单独报告。
