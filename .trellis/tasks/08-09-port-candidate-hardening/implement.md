# 执行计划

1. [x] 创建干净 Orca 集成 worktree，基于 `origin/main@a8c525cd`。
2. [x] 建立父任务和七个独立子任务，记录鉴权及大型架构排除项。
3. [ ] 收口插件范围问题并完成 PRD convergence pass。
4. [ ] 为父/子任务配置真实 `implement.jsonl`、`check.jsonl` 上下文并验证。
5. [ ] 提交规划基线，逐个激活已批准子任务。
6. [ ] 创建 Orca Run/Tasks，在独立 child worktree 并行启动 worker。
7. [ ] 等待并处理每个 `worker_done`、question、escalation；对子任务运行独立 Trellis check。
8. [ ] 按设计顺序集成已验证提交，解决仅由本批引入的冲突。
9. [ ] 运行聚焦测试、完整前后端质量门、release/plugin 合同和 `git diff --check`。
10. [ ] 更新必要 spec、完成 Trellis 任务、提交集成结果并报告未覆盖平台风险。

## 集成质量门

- `pnpm test` 或受影响 Vitest 集合、`pnpm typecheck`、`pnpm lint`、`pnpm build`。
- `pnpm tauri:fmt`、`pnpm check:generated-bindings`、聚焦及完整 Rust tests、Clippy、`cargo audit`。
- Release/CI dependency-free contract tests、workflow 语法/actionlint（可用时）。
- `git diff --check`、提交范围检查、鉴权/Observer/TUI/Usage Ledger 等排除项搜索。

## 风险与停止条件

- Provider master switch 与当前自定义路由产品语义无法同时满足测试时停止并升级决策。
- reset maintenance 与现有启动/插件 owner 出现不可分离架构冲突时停止，不顺带搬 recovery journal。
- release candidate promotion 需要大规模替换当前 release workflow 时，缩成并发键和密钥作用域的安全子集并报告剩余项。
- 任何 worker 发现必须恢复用户删除文件才能完成时停止并向协调者询问。
