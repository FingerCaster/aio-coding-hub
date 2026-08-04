# 修复自然回切健康高优先级供应商 - 技术设计

## Root Cause

自然规划器当前只从稳定供应商之前寻找 `OPEN/HALF_OPEN` 候选。`CircuitBreaker` 也只在进入 `OPEN` 时写入 `natural_probe_due_at`；低于 failure threshold 的失败虽然会立即切换 Provider 并让 session 绑定低优先级供应商，但高优先级供应商仍为 `CLOSED`，没有自然期限。结果是没有压缩或路由变化的会话可以永久停留在低优先级供应商。

## State Model

复用现有持久字段，不迁移 schema：

- `probe_reference_at`：在 `CLOSED` 且存在待处理自然回切时，表示最近一次可计失败时间；在 `OPEN` 时继续表示现有 probe deadline 基准。
- `natural_probe_due_at`：在 `CLOSED` 和 `OPEN` 中都可表示自然回切期限。
- `next_probe_at` / `open_until`：仍只属于 `OPEN` probe，不在 `CLOSED` 直接回切中设置。

状态规则：

```text
CLOSED + counted failure below threshold
  -> remain CLOSED
  -> reference = failure_at
  -> natural_due = failure_at + max_wait

CLOSED + another counted failure
  -> reset reference/natural_due from latest failure

CLOSED + complete success
  -> clear failures and pending natural deadline

CLOSED + natural_due reached + eligible lower-bound session request
  -> direct normal dispatch to higher Provider
  -> success clears deadline and binds this session
  -> failure rearms deadline and may fall back normally

OPEN/HALF_OPEN
  -> retain existing probe lease and terminal semantics
```

## Planner Changes

自然模式的 deadline candidate 可为：

1. `OPEN/HALF_OPEN` 候选；或
2. 带有待处理 `natural_probe_due_at` 的 `CLOSED` 候选。

到期后根据状态返回：

- `CLOSED` -> `DirectClosed { trigger: NaturalMaxWait }`
- `OPEN/HALF_OPEN` -> `Probe { trigger: NaturalMaxWait }`

没有待处理期限的 `CLOSED` 候选仍返回 `Stay`，保留压缩边界与会话缓存语义。Planner 只改排序意图，实际请求继续经过公共 gate。

## Configuration And Persistence

现有数据库字段已经允许 `CLOSED` 行保存 reference/deadline，无需 migration。旧版本持久行若有 failure timestamps 但没有 reference/deadline，加载时以最新一条持久化失败时间为 reference 补齐期限；只有无法取得该时间时才回退到 `updated_at`。`update_config` 对所有具有 pending natural deadline 的状态按 reference 重算 `natural_probe_due_at`；`OPEN` 行同时继续重算 `next_probe_at` 与 `open_until`。

## Observability

期限未到仍写 `probe_result="not_triggered"`，现有 `not_triggered_probe_observation` 会把 `natural_probe_due_at` 投影到 `circuit_recover_at_unix`。到期后的 `CLOSED` 路径是直接真实请求，不伪装成 circuit probe。

## Compatibility

- 不把 circuit threshold 加入单请求 attempt budget。
- 不给 `CLOSED` 直接回切创建 probe lease，也不改变 `OPEN -> CLOSED` 必须由当前 probe token 完成的契约。
- Provider 成功会取消旧失败带来的回切机会，避免健康请求后仍触发过期迁移。
- 最近失败重置期限，保证回切失败后不会按 30 秒 cooldown 连续追试。
