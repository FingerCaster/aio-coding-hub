# 实施清单

## A. 修复实现

1. [x] 将 `CODEX_RESPONSES_PATH_CONTRACTS` 简化为不携带 TTFB floor 的共享 path 列表，并同步
   eligibility/path 测试引用。
2. [x] 从 `success_event_stream.rs` 删除 500 ms 常量、最小 TTFB 计算、编译期断言、
   `WallClockTimeout` 分支和对应失败投影。
3. [x] 将 `collect_bounded_final_wire` 保持为 20 MiB 有界、逐 read idle-timeout 的收集循环；不改
   decode/bridge/fixer/plugin/validator/commit 顺序。

## B. 回归测试

4. [x] 增加有限慢流 fixture，验证合法 Codex SSE 持续推进超过旧 500 ms 后完整返回并通过严格
   validator。
5. [x] 更新 idle timeout 用例，验证真实 idle gap 仍在配置 budget 准确失败。
6. [x] 保留共享 path eligibility、first-event timeout、20 MiB 和严格终态测试覆盖。

## C. 规范与验证

7. [x] 更新 `upstream-error-handling-contract.md`，删除 500 ms/TTFB floor 合同，写入 per-read idle
   timeout、20 MiB 与取消/关闭边界。
8. [x] 运行 `pnpm tauri:fmt`。
9. [x] 运行聚焦 Rust 测试：
   `pnpm tauri:test --lib bounded_final_wire` 及无限重试 eligibility/validator 相关用例。
10. [x] 运行 `pnpm tauri:check`、`pnpm tauri:clippy`、`git diff --check`；按实际改动补充相关完整
    library tests。
11. [x] 使用 `trellis-check` 对照 PRD、upstream error handling 与 gateway attempt budget 合同复核。
12. [ ] 使用 `trellis-update-spec` 确认规范更新完整，提交并归档任务。

## 启动前检查

- [x] 根因已有代码、本机诊断和历史需求三方证据。
- [x] 无开放产品问题；修复边界由原始 R15/AC15 决定。
- [x] `prd.md`、`design.md`、`implement.md` 已收敛。
- [x] 用户已明确要求基于 `main` worktree 实施修复。
