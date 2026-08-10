# 第二组请求可靠性与 Codex 压缩体适配设计

## Task Map

- `08-05-transport-error-retry-backoff`：修复已有策略在 transport retry 路径未生效。
- `08-05-codex-zstd-request-body`：选择性移植参考 fork 的 Codex 多编码明文规范化合同。

两项改动代码边界基本独立，可在同一独立 worktree 中分阶段实现和验证；最终父任务负责完整回归、审查与合并。

## Integration Constraints

- 基于当前本地 `main` 创建独立 worktree，不以 upstream/main 为基线。
- 参考源固定为 `KNaiFen/aio-coding-hub@5b13683b`；只移植 `13a3c6f`、`909b7a0` 的相关语义，不整仓同步，也不用官方 upstream 代替参考 fork。
- 不覆盖主 worktree 的 AGENTS.md、包删除、`.orca/`、既有 Trellis 目录等用户改动。
- 子任务提交应可独立审查；最终合并前跑合并态质量门禁。

## Risk Order

先实现传输退避的小范围共享路径修复，再实现压缩请求规范化。后者涉及 zstd/brotli 依赖、early error、错误码合同和端到端数据流，单独提交便于审查与回滚。
