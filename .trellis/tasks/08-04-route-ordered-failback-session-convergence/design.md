# 多会话回切收敛状态 - 技术设计

## State Ownership

- `CircuitBreaker` 拥有全局单调 epoch 计数；每个 `ProviderHealth`/`CircuitSnapshot` 只记录
  自己最近一次 applied probe success 的 epoch。
- `SessionBinding` 拥有创建时的 `recovery_epoch_baseline`，并通过
  `SessionRoutingSnapshot` 只读暴露给请求 planner。
- Provider selection 在新 session binding 建立时读取 circuit 当前 epoch，避免在响应
  成功时才取值而漏掉与首请求并发发生的恢复。

## Planner Rule

natural 模式扫描高优先级前缀时，若候选为 `CLOSED` 且 recovery epoch 新于 session
baseline，则把它加入 `Direct` 目标。该规则只增加 eligibility，不改变 target-first
排序、gate、reservation、Ready 上限或成功绑定逻辑。

## Concurrency Invariants

- epoch 只增不减；同一 probe lease 最多发布一次。
- baseline 不是消费游标，不因某个 follower 成功而影响其他 session。
- loser 的当前请求可在 winner 完成前落到旧 Provider；同 Provider success 不推进 baseline。
- 任何 direct follower 失败都会走现有 circuit failure 记账，阻止健康事实继续误用。

## Compatibility

所有新增字段均为内存状态。持久化 circuit 初始化 epoch 为 0；进程重启时 session map 也
为空，因此新 binding 从当前 0 基线正常开始，不需要 schema migration。
