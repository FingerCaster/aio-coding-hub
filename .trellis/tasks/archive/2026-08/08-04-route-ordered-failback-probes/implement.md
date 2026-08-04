# 按路由顺序串行回切所有高优先级供应商 - 执行计划

1. 更新 `probe_planner.rs` 的 decision/target model，按最新路由完整扫描高优先级前缀，
   为每个候选独立计算 direct/probe trigger 和 not-triggered observation。
2. 重写 planner 单测，覆盖任意长度、前序未到期但后序到期、显式触发完整前缀、
   CLOSED/OPEN 混合及无目标 stay；删除固化“最高候选阻塞后续候选”的旧断言。
3. 在 `provider_resolution.rs` 使用有序目标列表稳定重排 Provider，把全部目标置于当前
   稳定供应商之前；保留 all-open unbound recovery 与不合格请求路径。
4. 将 `RequestDispatchIntent` 改为逐目标 trigger，并把 session trigger reservation
   延迟到首个实际 transport send 消费；gate/preparation skip 保留给后续目标。
5. 调整 `provider_gate.rs` 使用 `probe_trigger_for(provider_id)`，移除 gate deny 对整个
   request reservation 的提前释放；补 dispatch/gate 单测覆盖消费一次、前序 skip 保留、
   全链零调用释放和每目标 claim 单次。
6. 在 `gateway/routes.rs` 复用 counting upstream harness 增加路由级回归：
   - `p1` 失败、`p2` 成功、当前 `p3`；
   - 五供应商动态序列及成功短路；
   - cooldown/in-flight/not-triggered 后继续后序目标；
   - CLOSED/OPEN 混合；
   - 全部高优先级失败后回到当前 Provider；
   - Ready-provider 上限边界。
7. 更新 `gateway-failover-route-contract.md`：普通绑定会话回切改为任意长度有序前缀，
   明确逐候选自然期限、reservation 和预算契约；保留无绑定 all-open 特例说明。
8. 运行定向测试后执行完整门禁，检查没有生成绑定漂移或前端 route/count 回归。
9. 在 circuit breaker 中增加仅进程内的单调 recovery epoch；仅可信 probe success
   发布 Provider epoch，并补 success/failure/stale/restart 初始化单测。
10. session 第一次建立路由 binding 时捕获全局 recovery baseline，通过 routing snapshot
    传给 planner；滑动 TTL、同 Provider 成功和 loser 请求不得推进 baseline。
11. natural planner 把 baseline 之后恢复的 `CLOSED` 高优先级候选规划为 direct target，
    保持任意长度路由顺序、公共 gate、预算和无 probe metadata 契约。
12. 增加 winner + 任意数量 follower 的真实并发路由回归，证明第一波单飞、第二波各
    session direct 收敛，以及失败 winner 不发布恢复事实。

## Validation

在 `src-tauri` 工作目录运行：

```text
cargo test --lib probe_planner -- --nocapture
cargo test --lib dispatch -- --nocapture
cargo test --lib route_ordered_failback -- --nocapture
cargo test --lib
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
```

在仓库根目录运行：

```text
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm check:precommit:full
python ./.trellis/scripts/task.py validate --all
git diff --check
```

若聚合门禁仅因未被本任务修改的既有格式漂移失败，记录具体文件和对应底层命令结果；
本任务涉及的 Rust 格式、Clippy、完整 library suite 和定向 route tests 不得跳过。

## Rollback Point

若实现需要并行 probe、绕过公共 gate、提高 Ready/attempt budget，或无法保证
session trigger 只在真实 transport send 消费，则停止并返回设计阶段。

## Child Task Ownership

- `08-04-route-ordered-failback-planner-resolution`: only
  `probe_planner.rs` and `provider_resolution.rs`.
- `08-04-route-ordered-failback-dispatch-gate`: only `dispatch.rs` and
  `provider_gate.rs`.
- `08-04-route-ordered-failback-route-tests`: only `gateway/routes.rs`.
- `08-04-route-ordered-failback-contract`: only
  `gateway-failover-route-contract.md`.
- `08-04-route-ordered-failback-session-convergence`: circuit/session/planner production state
  propagation and focused unit tests; it must not edit `gateway/routes.rs`.
- `08-04-route-ordered-failback-session-concurrency-tests`: only the shared route-test harness and
  multi-session route regressions in `gateway/routes.rs`.
- The parent/coordinator owns integration fixes, validation, release metadata,
  commit, push, and release publication after all children complete.
