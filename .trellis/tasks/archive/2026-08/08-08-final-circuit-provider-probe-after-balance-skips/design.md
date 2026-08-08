# 余额跳过后终局熔断 Provider 试探设计

## 根因

`provider_resolution::plan_request_failback` 会在进入共同 Provider gate 前读取账户用量投影，并把已知余额阻断 Provider id 交给 `probe_planner`。稳定绑定因熔断失效时，planner 进入 `NewUnboundSession` 分支；当前分支没有使用 `blocked_provider_ids`：

1. 候选顺序为 P1(balance blocked, circuit closed)、P2(balance blocked, circuit closed)、P3(circuit open)。
2. P1/P2 的 closed 状态使 `all_open=false`。
3. planner 只给 P1 建立 direct dispatch target。
4. failover 的账户 gate 正确跳过 P1/P2；P3 因没有 probe intent 被 circuit gate 跳过。
5. 请求以无可用 Provider 结束，尽管 P3 是唯一未被账户状态阻断的候选。

## 所有权与算法

修复只修改纯函数 `plan_probe_with_account_usage` 的 unbound 分支，不改变账户 gate、circuit lease 或 transport commit：

- 从 route-order candidates 中计算未被账户用量阻断的 `available_candidates`。
- 如果没有未阻断候选，返回 `Stay`；共同 failover loop 仍按原顺序记录所有余额 gate 跳过。
- `all_open` 只依据未阻断候选判断。
- 若未阻断候选全部 Open/HalfOpen，恢复目标覆盖到最后一个未阻断候选；目标前缀保留中间的余额阻断候选，使 failover attempt 仍按原顺序产生 gate 证据，并允许多个熔断候选在 probe cooldown/in-flight/失败时继续推进。
- 若未阻断候选存在 Closed，目标前缀只覆盖到第一个未阻断候选，维持新会话优先使用首个实际可发送候选的既有行为。
- 每个目标仍通过 `planned_target` 决定 Direct/Probe；余额 gate 位于 circuit gate 前，因此余额阻断目标不会获得 probe lease 或网络发送。

## 不变量

- 不绕过账户余额 gate，不修改 blocked/recovery epoch。
- 不改变 forced/managed/辅助请求的 `request_eligible` 判定。
- 不改变 Natural/Aggressive 策略、Session reservation、Ready-provider 预算或 transport retry。
- probe 继续由 `try_acquire_probe` 的 cooldown、single-flight lease 与持久化边界保护。
- 目标顺序与 Provider route 顺序一致；余额跳过证据位于终局 probe 之前。

## 验证

- planner 单测：两个 blocked closed + 最后 open，应得到 direct(P1)、direct(P2)、probe(P3)。
- planner 单测：所有候选 blocked，应 `Stay`；不得制造 probe。
- gateway E2E：P1/P2 零余额且零上游调用，P3 open 后通过 `new_unbound_session` probe 成功；attempt 顺序、probe 字段、circuit 关闭和单次网络调用均正确。
- 既有 all-open cooldown/in-flight、多 open 候选、forced/managed、账户 gate 与 ready budget 测试不得回归。

## 回滚

变更只影响纯 planner 和测试，可独立回滚；SQLite、settings、Provider DTO、账户 runtime 和公开错误码均不变。
