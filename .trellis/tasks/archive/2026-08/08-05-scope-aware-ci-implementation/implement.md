# Implementation Plan

## Ordered Work

- [x] 新增 CI scope JSON 策略。
- [x] 实现策略校验、路径分类、NUL name-status 解析、事件 range 和 fail-closed CLI。
- [x] 实现覆盖策略、路径、控制面、R/C/D、PR/push 与错误场景的无依赖自测。
- [x] 在当前 `ci.yml` 增加 change-scope/docs-contract/ci-gate，并为现有 full jobs 加范围条件。
- [x] 增加 workflow 结构合同自测与可执行 gate 结果矩阵，并接入 package/CI 自测。
- [x] 增加 package script、cross-layer spec 与 index。
- [x] 运行针对性 Node 检查，交由 validation 子任务执行全量质量门禁。

## Constraints

- 不复制参考 workflow 的 `workflow_dispatch`、候选发布、TUI 或 provider benchmark。
- 不删减、改弱或重命名当前 full jobs 和检查步骤。
- 不操作远端、主 worktree 或参考仓库。
