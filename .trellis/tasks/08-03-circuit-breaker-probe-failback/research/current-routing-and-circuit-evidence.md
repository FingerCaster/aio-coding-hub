# 当前路由与熔断恢复证据

## 熔断状态机

- `src-tauri/src/shared/circuit_breaker/types.rs:11-15`：半开恢复固定需要 3 次连续成功；默认失败阈值为 5，默认打开 30 分钟。
- `src-tauri/src/shared/circuit_breaker.rs:203-247`：`should_allow` 在请求触达时才把已到期的 `OPEN` 转成 `HALF_OPEN`；`HALF_OPEN` 本身允许请求通过。
- `src-tauri/src/shared/circuit_breaker.rs:286-326`：半开成功累计到 3 次才关闭。
- `src-tauri/src/shared/circuit_breaker.rs:398-421`：半开任意失败立即重新打开。
- 状态中没有 probe owner、lease、generation 或 in-flight 标记；并发调用 `should_allow` 都能通过半开状态。

## 会话复用与候选顺序

- `src-tauri/src/gateway/proxy/failover.rs:51-82`：请求体历史数组长度大于 1 时才启用 provider 复用。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs:15-73`：会话会固定排序模式和首次捕获的 provider 顺序。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs:120-157`：熔断仅阻止 bound provider 成为复用 preference，不从候选集移除；后续公共 gate 是权威判断者。
- `src-tauri/src/gateway/proxy/handler/provider_order.rs:36-68`：复用时把 bound provider 旋转到候选首位。
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs:1400-1432` 与 `src-tauri/src/gateway/streams/finalize.rs:67-82`：成功响应更新熔断状态并把会话绑定到实际成功 provider。
- `src-tauri/src/domain/providers/queries.rs:707-814`：gateway 使用排序模式或默认路由的 `sort_order` 形成候选顺序，不读取 `providers.priority`。

## 公共 Gate 与诊断约束

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs:41-110`：每个候选统一经过 circuit 与 provider-limit gate；拒绝会形成 skipped attempt。
- `.trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md`：session binding 只拥有复用 preference/order，公共 gate 是唯一临时 deny owner；skipped 不消耗 Ready-provider budget。
- 历史产品决定 `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-3/research/provider-selection-product-decision.md` 明确选择保留上述 common-gate 行为。

## 现有可用性测试

- `src-tauri/src/domain/provider_availability.rs:1-16`：Provider 页已有轻量直连可用性测试，连接超时 8 秒、总请求超时 15 秒。
- `src-tauri/src/domain/provider_availability.rs:260-389`：探测会针对 CLI 类型构造最小生成请求，可能产生真实上游用量。
- `src-tauri/src/domain/provider_availability.rs:445-454`：除鉴权失败外，所有 `<500` 状态都被视为“可用”，包括 400、404、429。
- `src-tauri/src/domain/provider_availability.rs:500-631`：该命令独立直连 provider，不经过 gateway circuit gate，也不记录 circuit success/failure。

## 直接推论

1. 现有低优先级会话若持续成功，高优先级 provider 即使 `open_until` 已过，也可能长期没有请求触达，因而停留在持久化 `OPEN` 或未获得足够半开成功。
2. 积极回切不能只检查 snapshot 是否“到期”；必须先定义并完成可信恢复判定。
3. 直接复用手动可用性测试会把 429/模型错误当成恢复成功，且引入额外请求成本，因此需要专门收紧语义后才可用于自动恢复。
4. 若用真实业务请求试探，必须增加单飞控制，并明确半开成功但尚未达到 3 次时是否允许更新会话绑定。
