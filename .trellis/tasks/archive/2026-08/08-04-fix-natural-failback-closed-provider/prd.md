# 修复自然回切健康高优先级供应商

## Goal

修复自然回切只对 `OPEN/HALF_OPEN` 高优先级供应商生效的缺口。高优先级供应商发生可计失败并使会话稳定在低优先级供应商后，即使其熔断器仍为或重新显示为 `CLOSED`，也必须在 `natural_probe_max_wait_seconds` 到期后的下一条合格真实请求中获得一次回切机会。

## Requirements

- 高优先级供应商在 `CLOSED` 状态记录可计失败时，建立或重置 Provider 全局自然回切期限；期限以最近一次可计失败为基准。
- 同一供应商随后完整成功时，清除未消费的自然回切期限；普通健康且没有待处理期限的 `CLOSED` 供应商仍保持自然会话黏性。
- 自然模式中，稳定供应商之前的 `CLOSED` 候选若存在且已到达自然回切期限，下一条合格真实请求直接尝试该候选，不创建 circuit probe lease。
- 候选为 `OPEN/HALF_OPEN` 时继续使用现有 Provider 全局 single-flight probe；不得削弱 cooldown、公共 gate、Ready-provider 和 attempt budget 契约。
- 到期回切失败后按失败时刻重新等待完整的自然最大等待时间，不得退化成每轮或每个 cooldown 周期连续尝试。
- 期限未到时继续记录结构化 `probe_result="not_triggered"` 观察；详情中的恢复时间应包含 `CLOSED` 候选的自然期限。
- 配置热更新和进程重启后继续保留相同保护语义，不新增数据库列或后台探测。
- 积极回切、压缩触发、路由变更、强制 Provider、health-neutral/strict/单候选请求语义保持不变。
- 设置页文案要明确：最大等待覆盖高优先级 Provider 故障后的探测或直接回切，而不只是 `OPEN` 熔断探测。

## Acceptance Criteria

- [x] `CLOSED` P1 第一次可计失败后仍为 `CLOSED`，但 snapshot 持有 `natural_probe_due_at = failure_at + configured_wait`。
- [x] P1 在期限前再次可计失败时，以最新失败时间重置期限；P1 完整成功时清除期限。
- [x] 自然模式、P2 稳定绑定、P1 `CLOSED` 且期限未到时继续 P2，并记录 `not_triggered`。
- [x] 同一场景期限到达后直接把 P1 放到本次请求前面；P1 成功后会话绑定 P1，P1 失败时仍可回退 P2。
- [x] P1 回切失败后，后续请求在新的完整等待期到达前不再次尝试 P1。
- [x] `CLOSED` P1 没有待处理自然期限时仍保持 P2，不把自然模式变成积极模式。
- [x] `OPEN/HALF_OPEN` 的 single-flight probe、cooldown/in-flight skip 和全路由恢复测试保持通过。
- [x] 热更新自然最大等待值会按最近失败基准重算 `CLOSED`/`OPEN` 待处理期限；持久状态重载后行为一致。
- [x] 定向 Rust 测试、完整 Rust library 测试、格式检查及相关前端测试通过。

## Out Of Scope

- 后台合成健康检查。
- 改变 circuit failure threshold、普通 retry 次数或 Provider 排序。
- 让从未发生过失败的健康高优先级供应商按计时器强制迁移所有自然会话。
