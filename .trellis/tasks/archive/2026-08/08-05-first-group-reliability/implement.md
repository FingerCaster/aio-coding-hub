# Implementation Plan

## Ordered Work

- [x] R1：增加每 CLI 路由草稿一次性初始化状态与异步竞态测试。
- [x] R2：改造同步 workflow，新增策略检查、自测、package script 与 CI 接线。
- [x] R3：调整 Rust DB 初始化缓存，增加启动重试 helper；统一前端监听和快照同步并补测试。
- [x] R4：实现有界诊断脱敏模块，接入控制台、IPC、前端错误报告和 Rust 日志并补全边界测试。
- [x] R5：增加通知世代防护和后端活动快照复核，保留 30 秒静默期并补竞态测试。

## Validation

- 针对性 Vitest：Provider 数据模型、同步策略、启动状态、诊断边界、通知事件。
- 针对性 Rust 测试：`app_state`、启动任务和前端错误报告命令。
- 全局检查：`pnpm typecheck`、`pnpm lint`、`pnpm tauri:fmt`、`pnpm check:generated-bindings`。
- 根据仓库脚本执行完整前端测试及 `cargo test --locked`/Clippy 可行范围；Windows 无法覆盖的 Unix cfg 必须明确记录。

## Review Gates

- 每项实现后先运行其针对性测试，再进入下一项。
- 最终执行 `trellis-check` 全范围审查，核对所有受影响文件与父/子 PRD。
- 检查功能分支干净且基准未漂移，再提交并合并到 `main`。

## Rollback Points

- workflow 策略、启动可靠性、诊断脱敏和通知可靠性分别保持逻辑提交边界。
- 若主分支在开发期间前进，先审查差异再合并；禁止 reset 或覆盖用户现有改动。
