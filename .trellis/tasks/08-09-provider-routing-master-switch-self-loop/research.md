# 研究摘要

- 基线：`origin/main@a8c525cdaadce77dd4b00363962e501bc5fae491`。
- `domain/providers/queries.rs:1108-1114` 的 custom route SQL 缺少 `p.enabled = 1`；默认 route 与 source provider 已有 enabled 过滤。
- `provider_selection/tests.rs:315-355` 明确保留 disabled Provider 于 custom route，需按新总开关契约更新。
- 发送链位于 `failover_loop/attempt/attempt_executor.rs`；当前 fork 同时包含账户用量 gate、探测回切、模型路由和 provider-specific route，均需回归。
- `gateway/http_client.rs` 已拒绝配置/system proxy 指向 gateway，并有递归 Header；缺少 Provider target URL/DNS 自环校验。
- 候选 `73a15d6d` 增加查询过滤、runtime `ProviderEnableGate` 和 retry 前检查；候选 `ecd82606` 增加有界 DNS target validator。
- 禁止带入候选 `7fc78235` 的鉴权及 Provider 专用路由删除。
