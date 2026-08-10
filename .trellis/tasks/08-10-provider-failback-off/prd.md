# 新增关闭自动回切策略

## Goal

为 Provider 回切增加可持久化的“关闭自动回切”策略，使用户可以让已有稳定会话继续复用当前 Provider，同时不关闭正常故障转移、新会话选路、管理员显式路由变更或全路由恢复能力。

## Background

- 当前 `provider_failback_strategy` 只接受 `natural` / `aggressive`，默认 `natural`；设置界面也只展示“自然回切”和“积极回切”。
- 自然回切不是关闭：压缩完成、Provider 恢复 epoch、余额恢复 epoch、自然最长等待和最长熔断等待都可能让已有会话重新尝试更高优先级 Provider。
- 原始回切任务的用户表述是“可以先做成两个策略”，且当时 PRD 将第三种策略列为范围外；这属于首期范围收敛，不是永久禁止第三种策略。
- 回切 planner 与正常 failover 共用候选和公共 gate。关闭自动回切不能通过禁用 circuit、移除候选或绕过公共 gate 实现，否则会破坏当前 Provider 失败后的恢复能力。

## Requirements

### R1. 设置契约

- `ProviderFailbackStrategy` 新增序列化值 `disabled`，现有 `natural` / `aggressive` 值和默认 `natural` 保持不变。
- `disabled` 必须通过现有 settings patch、持久化、读取、热更新和生成 TypeScript bindings 完整往返。
- 该变更扩展现有字段的枚举值，不新增设置字段，不修改现有默认值。

### R2. 已绑定会话的关闭语义

- 当请求有有效稳定 Provider 绑定、有效路由未变化且策略为 `disabled` 时，planner 必须保持当前绑定，不创建回切 dispatch intent。
- 以下自动来源均不得触发回切：Claude/Codex 压缩信号、积极轮次、circuit recovery epoch、账户余额恢复 epoch、`natural_probe_due_at`、`open_until` 到期以及其他会话正在执行的高优先级 probe。
- 关闭模式是明确的“不评估自动回切”，不得为高优先级候选生成 `probe_result="not_triggered"` 观察记录，避免每条请求出现无动作的伪 attempt/hop。

### R3. 必须保留的路由行为

- 没有有效绑定的新会话仍按最新路由选择 Provider；首选或全路由均熔断时仍可沿用现有真实流量 single-flight probe / 全路由恢复。
- 管理员修改有效 Provider 路由后，`route_changed` 仍是显式意图；已有会话下一条合格请求继续按最新路由处理完整高优先级前缀。
- 当前绑定 Provider 被公共 gate 拒绝或真实请求失败时，现有串行故障转移、重试预算、Provider 上限和最终错误行为保持不变。
- 强制 Provider、单候选、health-neutral/strict、模型发现及其他本来不具备跨 Provider probe 资格的路径保持原契约。

### R4. 模式切换与兼容

- 切换到 `disabled` 不清空 Provider deadlines、recovery epochs、session compaction generation 或 route fingerprint；它只影响后续请求是否消费这些状态。
- 从 `disabled` 切回 `natural` / `aggressive` 后，后续请求按新策略和届时仍有效的状态正常规划。
- 旧配置缺少该值时继续默认 `natural`；未知值继续按现有 `#[serde(other)]` 规则回落到 `natural`。
- 旧版本程序不认识 `disabled` 时会按现有兼容规则回落为 `natural`，因此版本回退后不能保证继续关闭自动回切；该限制需在设计与交付说明中明确。

### R5. 设置界面

- “回切策略”增加第三个单选项“关闭自动回切”，持久值为 `disabled`。
- 文案必须明确：关闭后已有稳定会话不会主动回到更高优先级 Provider，但当前 Provider 失败后的故障转移和新会话选路仍然有效。
- “自然模式最长回切等待”只在 `natural` 选中时显示；`disabled` 与 `aggressive` 均隐藏该输入。

### R6. 回归范围

- `natural` 和 `aggressive` 的 planner 顺序、触发类型、观察记录、默认设置与 UI 持久化不得回归。
- 不新增 probe trigger、attempt 类型、数据库列或后台探测机制。

## Acceptance Criteria

- [x] **AC1 / R1、R5**：设置页可选择“关闭自动回切”，保存并重新读取为 `disabled`；生成绑定包含 `disabled`，默认仍为 `natural`。
- [x] **AC2 / R2**：已有稳定绑定且路由未变化时，即使同时存在压缩、恢复 epoch、余额恢复、到期 deadline 或在途 probe，`disabled` 仍返回无观察记录的 `Stay`。
- [x] **AC3 / R3**：`disabled` 下 route change 仍按完整高优先级前缀生成 `RouteChanged` direct/probe 计划。
- [x] **AC4 / R3**：`disabled` 下无绑定请求仍执行现有首选/全 Open 恢复规划；正常故障转移预算和公共 gate 行为不变。
- [x] **AC5 / R4、R6**：切回 `natural` / `aggressive` 后无需状态迁移即可恢复原行为，现有两种策略的 focused tests 继续通过。
- [x] **AC6 / R5**：关闭模式隐藏自然最长等待输入，界面文案不把“关闭自动回切”误写为关闭故障转移或熔断恢复。
- [x] **AC7 / R6**：Rust focused/full tests、前端 focused tests、typecheck、lint、generated bindings、format 和 Clippy 按任务检查计划通过。

## Out of Scope

- 关闭 circuit breaker、正常故障转移、全路由恢复或 Provider 健康计数。
- 后台定时/合成健康探测。
- CLI、模型、路由或单 Provider 级覆盖策略。
- 选择关闭模式时清除既有 session/circuit 状态。
- 新增数据库或 settings 字段迁移。
