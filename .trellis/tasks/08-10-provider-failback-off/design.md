# 新增关闭自动回切策略 - 技术设计

## 1. 边界与数据流

现有数据流保持不变：

```text
settings.json provider_failback_strategy
  -> AppSettings / SettingsUpdate
  -> GatewayRuntimeSettings
  -> ProbePlannerInput.strategy
  -> provider resolution / common gates / failover loop
```

只扩展 `ProviderFailbackStrategy` 和 planner 分支，不增加新的设置所有者、IPC 字段、数据库状态或 trigger 类型。前端继续使用生成的 `AppSettings["provider_failback_strategy"]`，避免手写重复 union。

## 2. 枚举与兼容

在 `ProviderFailbackStrategy` 中增加 `Disabled`，使用现有 `snake_case` 序列化为 `disabled`。`Natural` 继续同时承担 `Default` 与 `#[serde(other)]`，因此：

- 现有配置与缺失字段继续得到 `natural`；
- 新版本能稳定往返 `disabled`；
- 未知未来值继续 fail-safe 到 `natural`；
- 旧版本读取 `disabled` 会回落为 `natural`，回退版本后自动回切可能重新启用。

现有字段已经由 settings schema 54 引入。本次没有新增字段或默认初始化需求，因此不提升 schema 版本；生成 bindings 反映枚举扩展即可。

## 3. Planner 决策顺序

`plan_probe_with_account_usage` 保持以下固定顺序：

1. 计算稳定绑定之前的完整高优先级前缀；空前缀和不合格请求沿用现有 `Stay`。
2. 无有效绑定时沿用现有 `NewUnboundSession` / all-open 恢复规划，不读取 `disabled` 作为阻断条件。
3. `route_changed` 继续作为最高优先级显式 trigger，按完整前缀生成 direct/probe targets。
4. 若有稳定绑定、路由未变化且策略为 `Disabled`，立即返回 `Stay { confirm_route: false, not_triggered_provider_ids: [] }`。
5. 只有非 `Disabled` 才继续处理 compaction、`AggressiveTurn`、recovery epochs、账户余额恢复、in-flight follower、自然 deadline 与 `open_until`。

早返回必须放在无绑定和 route-change 分支之后、所有自动 trigger 之前。放得更早会误伤新会话/管理员显式路由；放得更晚会让 compaction 或自然兜底绕过关闭设置。

关闭模式返回空 `not_triggered_provider_ids`：用户已经要求不评估自动回切，这些候选不是“等待触发”的失败尝试，不应写入请求日志或形成展示 route hop。

## 4. 状态与切换

模式开关不写 session 或 circuit 状态：

- `natural_probe_due_at`、`open_until` 和 recovery epochs 继续由健康状态机维护；
- compaction generation 和 route fingerprint 继续由 session owner 维护；
- `disabled` 仅阻止 planner 在普通已绑定请求中消费这些来源；
- 重新启用其他策略后，planner 使用当时仍有效的状态，不需要补偿迁移或全局扫描。

这样可避免设置更新同时获取 settings、session、circuit 锁，也保持现有 owned-field CAS/回滚契约。

## 5. UI

在现有 `RadioGroup` 增加：

- label：`关闭自动回切`
- value：`disabled`
- description：说明稳定会话保持当前 Provider，故障转移和新会话选路仍有效

`onChange` 必须显式接受三个合法值，不能继续把所有非 `aggressive` 值压回 `natural`。自然等待输入继续使用 `providerFailbackStrategy === "natural"` 条件，无需新增布局。

## 6. 测试策略

### Rust planner

- 关闭模式忽略 compaction、circuit/account recovery、自然/open deadlines 和 in-flight 状态，并返回无观察的 `Stay`。
- 关闭模式仍执行 `RouteChanged` 完整前缀计划。
- 关闭模式仍执行无绑定/all-open 计划。
- 现有 natural/aggressive 单元测试全量回归。

### Settings 与 bindings

- 增加 `disabled` JSON round-trip/default 保护测试，或在现有 settings CRUD/persistence 边界覆盖等价契约。
- 运行绑定生成并提交 `src/generated/bindings.ts` 的枚举变化。

### Frontend

- 选择“关闭自动回切”会更新本地状态并只提交 `provider_failback_strategy: "disabled"`。
- `disabled` 处于选中状态且隐藏自然最长等待输入。
- 保留现有积极/自然持久化测试。

## 7. 风险与回滚

- **分支位置错误**：可能关闭无绑定/all-open 恢复或让 compaction 绕过关闭设置；由三类 planner 测试锁定。
- **伪观察日志**：若返回 higher IDs，会在每轮请求制造无动作 skipped attempt；关闭分支必须返回空列表。
- **旧版本回退**：旧 binary 会把 `disabled` 解释为 `natural`。代码回退可直接进行，但用户需在兼容版本中重新确认策略；本任务不尝试跨版本保留未知枚举。
- **绑定漂移**：生成绑定必须由 Specta 工具更新，不能手写独立 union。
