# 多会话回切收敛状态 - 执行计划

1. 给 circuit runtime state、snapshot 和 breaker 增加 global/provider recovery epoch；只在
   `complete_probe_success` applied 分支发布，补原子性与反例单测。
2. 给 session binding/routing snapshot 增加 baseline；新增带 epoch 的新 binding API，保留
   现有测试便利 API，并让生产 provider selection 使用新 API。
3. 把 session baseline 传入 `ProbePlannerInput`，为 newer recovered `CLOSED` 候选生成
   direct target；更新所有 snapshot fixture 和 planner/resolution 单测。
4. 运行 circuit、session、planner、resolution 定向测试以及 rustfmt、Clippy、完整 lib suite。

## File Ownership

- `src-tauri/src/shared/circuit_breaker.rs`
- `src-tauri/src/shared/circuit_breaker/types.rs`
- `src-tauri/src/shared/circuit_breaker/tests.rs`
- `src-tauri/src/gateway/session_manager.rs`
- `src-tauri/src/gateway/session_manager/tests.rs`
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`
- `src-tauri/src/gateway/proxy/handler/provider_selection/probe_planner.rs`
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs`

不得修改 `src-tauri/src/gateway/routes.rs`，该文件由并发测试子任务独占。
