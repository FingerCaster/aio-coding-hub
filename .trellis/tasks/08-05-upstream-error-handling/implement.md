# 执行计划

1. [ ] 固化参考实现与当前 fork 的逐层差异，记录不可整体移植的共享文件和必须保留的行为。
2. [ ] 完成并启动子任务 `upstream-error-response-rewrite`，实现、测试、检查并原子提交。
3. [ ] 完成并启动子任务 `codex-stream-internal-error-retry`，实现、测试、检查并原子提交。
4. [ ] 启动父任务，整合统一入口、共享编辑体验、日志投影与生成绑定，消除跨子任务合同偏差。
5. [ ] 运行 focused Rust tests：设置读写/迁移、规则 matcher/envelope、failover 路由、SSE guard/classifier、日志投影、Provider 分享/备份。
6. [ ] 运行完整 Rust 检查：`pnpm tauri:fmt`、`cargo check --locked`、`cargo test --locked`、`cargo clippy --all-targets --locked -- -D warnings`。
7. [ ] 运行前端与跨层检查：generated bindings、Vitest、typecheck、lint、build、cross-layer contracts。
8. [ ] 启动本地应用前端，使用浏览器验证桌面和移动视口、两个模式、规则弹窗、保存/启停、Provider 覆盖和日志徽标。
9. [ ] 读取 `trellis-check` 与 `trellis-update-spec`，执行全范围检查并更新 backend/cross-layer 合同。
10. [ ] 形成必要的父任务整合提交，记录 SHA、测试证据、有意差异和残余风险。
11. [ ] 归档两个子任务和父任务，记录 Trellis session；不 merge、push 或发布。

## 风险文件与审查点

- `success_event_stream.rs`：必须保留 fork 的 probe 终态和 session convergence；guard 只能在下游提交前改变 failover。
- `upstream_error.rs` / `finalize.rs`：正文只能消费一次，rewrite 候选不得逃逸到后续成功或不同失败。
- `routes.rs`：现有大量 circuit/failback 路由测试必须继续通过；新增测试使用 paused time。
- settings migration/persistence：当前 schema 版本高于参考功能基线，新增迁移必须基于当前版本顺延且幂等。
- `GeneralTab.tsx`：保持设置页密度，避免新增卡片嵌套和移动端操作溢出。

