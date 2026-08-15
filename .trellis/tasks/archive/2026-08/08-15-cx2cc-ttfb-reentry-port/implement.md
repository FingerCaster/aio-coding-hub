# 实施计划

## 有序步骤

1. [x] 读取 `AGENTS.md`、Trellis workflow、aio-coding-hub backend/cross-layer specs、
   旧三个提交及 `08-14-cx2cc-aio-nested-ttfb-budget` 归档设计；确认父任务不可见，改为
   独立 task 并在 notes 保留外部父引用。
2. [x] 在最终 typed intent match + `SelfLoop` 确认后生成私有 attempt target；让 send/header
   与 SSE first-event 从同一个 `AttemptTiming` 投影 effective timeout。
3. [x] 保持 nonce 注入顺序、入站消费与现有 no-proxy/no-redirect client，不重写最新
   proxy/self-loop 架构。
4. [x] 补 owner 矩阵、ordinary/explicit source 与 delayed local hop wall-clock 上界测试。
5. [x] 运行并保留真实 Provider 524、proxy bypass、`response.completed` 可见/单次输出、
   final-wire wall-clock cap 聚焦回归；不修改 continuation repair。
6. [x] 更新 `gateway-attempt-budget-contract.md`、`cx2cc-routing-contract.md` 与 backend index。
7. [x] 运行 focused Rust、fmt、clippy、`pnpm check:generated-bindings`、spec links
   和 `task.py validate`；检查 `git diff --check`、未解决冲突与无关 dirty paths。
8. [x] 按仓库 commit 门禁显式列出本会话文件，确保 node/pnpm 可见后提交；不 push、不建 PR、
   不操作 upstream，并更新 Orca comment（SHA、验证、残余风险）。

## 预定验证

```powershell
Push-Location src-tauri
cargo test internal_codex_reentry --lib
cargo test attempt_executor::tests --lib
cargo test delegated_sse_first_event_budget_skips_outer_deadline --lib
cargo test mock_runtime_router_timeout_stub_returns_bad_gateway_and_emits_request_log --lib
cargo test internal_reentry_client_bypasses_proxy_and_does_not_follow_redirects --lib
cargo test codex_responses_buffers_created_event_until_completion --lib
cargo test bounded_final_wire --lib
cargo test final_wire_wall_clock_cap_precedes_every_supported_path_ttfb_floor --lib
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
Pop-Location
pnpm check:generated-bindings
pnpm check:spec-links
python .trellis/scripts/task.py validate 08-15-cx2cc-ttfb-reentry-port
git diff --check
```

若仓库脚本名称或 Windows 工具链不同，记录实际等价命令与环境缺口，不放宽测试门禁。

## 实际证据

- `cargo test --locked --lib attempt_executor::tests`：7 passed，覆盖 typed owner、header/SSE
  双投影、普通/显式 source 保留预算与 header wall-clock 上界。
- `cargo test --locked --lib delegated_sse_first_event_budget_skips_outer_deadline`：1 passed，
  直接证明委托预算跳过 SSE 首事件 probe，而普通预算在同一延迟流上超时。
- 真实 Provider 524、direct no-proxy/no-redirect、`response.completed` 可见且单次、延迟
  compact route、final-wire cap 三项聚焦回归均通过。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、
  `pnpm check:spec-links`、task validate 与 `git diff --check` 通过。
- `pnpm check:generated-bindings` 的 Rust exporter 成功；worktree 未安装 `node_modules`，最终
  `prettier` 步骤不可用。临时 exporter 输出已恢复，generated bindings 无差异。

未新增完整 outer Gateway -> inner Gateway 的 nested route E2E；当前证据由 typed owner 投影、
header wall-clock、SSE probe 边界和真实 Provider 524 route 组合覆盖，保留此项为残余风险。
