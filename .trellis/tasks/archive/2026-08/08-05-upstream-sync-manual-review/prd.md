# 上游同步人工审查

## Goal

让定时或手动同步只生成可审查 PR，任何情况下都不由 workflow 直接更新或自动合并 `main`。

## Requirements

- workflow 不持有目标分支写权限，不运行直接 push 或自动 merge。
- 快进与分叉场景都创建/更新同步 PR。
- 冲突或未知合并状态失败闭合。
- 策略检查可在本地和 CI 重复执行。

## Acceptance Criteria

- [x] 静态策略自检通过，并有负例证明危险配置会被拒绝。
- [x] workflow 仍支持 schedule 与 workflow_dispatch。
- [x] 同步 PR 保持打开等待人工审查。

## Out Of Scope

- 实际执行 upstream 合并、修改远端 `main` 或自动批准 PR。
