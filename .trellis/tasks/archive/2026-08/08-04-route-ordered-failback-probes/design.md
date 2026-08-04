# 按路由顺序串行回切所有高优先级供应商 - 技术设计

## Root Cause

`ProbePlannerDecision::{DirectClosed, Probe}` 只携带一个 `provider_id`。
session preference 又会把绑定的 `pX` 旋转到候选列表首位；随后
`move_provider_to_front(p1)` 只能形成 `[p1, pX, p2, ...]`。因此第一个回切目标
失败后，健康的当前供应商会先成功并终止 failover loop，后续高优先级目标永久
得不到机会。

多会话还有一个独立缺口：Provider 全局 single-flight 的 winner 成功后只更新自己
的 session binding。并发 loser 已经得到 `in_flight` 并继续当前供应商；Provider
随后变为无自然 deadline 的健康 `CLOSED`，旧 planner 会把它永久判为
`not_triggered`。因此 loser 不是再等一个 60 秒，而是可能一直不再回切。

另一个边界是 `RequestDispatchIntent` 的 session trigger reservation：当前 gate 拒绝
目标时会直接 `release_unclaimed_reservation()`，而 gate 允许时又在本地准备完成前
把 reservation 移给单个 ownership。多目标链中，这会让前序零调用 skip 吃掉本应
由后续真实请求消费的触发机会。

## Planner Model

把单目标 decision 收敛为有序计划：

```rust
struct PlannedFailbackTarget {
    provider_id: i64,
    dispatch: PlannedDispatch,
}

enum PlannedDispatch {
    Direct,
    Probe(ProbeTrigger),
}

enum ProbePlannerDecision {
    Stay {
        confirm_route: bool,
        not_triggered_provider_ids: Vec<i64>,
    },
    Dispatch {
        targets: Vec<PlannedFailbackTarget>,
        reservation_trigger: ProbeTrigger,
        not_triggered_provider_ids: Vec<i64>,
    },
}
```

名称可按现有模块风格微调，但必须保留三个信息：目标顺序、每个目标是否需要
probe/对应 trigger、所有未触发观察。

### 规划算法

1. 在 `ordered_candidates` 的最新路由中定位稳定绑定，取其前缀；没有有效绑定时
   保留既有 new-unbound/all-open 分支。
2. 请求不合格或前缀为空时返回 `Stay`。
3. route-change、compaction、aggressive 为显式触发：扫描完整前缀，`CLOSED` 生成
   `Direct`，`OPEN/HALF_OPEN` 生成携带同一显式 trigger 的 `Probe`。
4. 普通 natural timer 为逐目标判断：
   - `natural_probe_due_at <= now`：`CLOSED` 为 `Direct`，其他状态为
     `Probe(NaturalMaxWait)`；
   - 否则 `OPEN/HALF_OPEN` 且既有 `open_until <= now`：
     `Probe(MaxOpenWait)`；
   - 否则加入 `not_triggered_provider_ids` 并继续扫描后续候选。
5. 有目标时返回完整 `Dispatch`；无目标时返回 `Stay`。旧测试中“最高 OPEN 未到期
   阻止后续到期 OPEN”的断言改为“记录前者 not-triggered，规划后者”。

## Provider Order Restoration

resolution 不再只旋转一个 Provider。新增稳定分区式排序 helper：

```text
session-preferred input: [pX, p1, p2, ..., p(X-1), tail...]
planned target ids:      [p1, p2, ..., p(X-1)]
dispatch order:          [p1, p2, ..., p(X-1), pX, tail...]
```

helper 按 planner 给出的 target ID 顺序先放目标，再按输入相对顺序追加非目标，
忽略不存在/重复 ID。这样既恢复最新路由前缀，又保留当前供应商和低优先级 tail 的
既有 fallback 语义。未到期、仅产生 planner observation 的候选不是 target，不会被
错误地提前发包。

## Dispatch Intent

把 `RequestDispatchIntent` 从“一个全局 probe trigger + 附加 ID”改为逐目标描述：

```rust
struct DispatchTarget {
    provider_id: i64,
    probe_trigger: Option<ProbeTrigger>,
}
```

- `targets_provider(id)` 继续供公共 gate 判断。
- `probe_trigger_for(id)` 只给计划为 probe 的目标返回 trigger；计划为 direct 的
  `CLOSED` 目标走普通 `should_allow`，不伪造 probe metadata。
- 既有 `new_all_open_recovery` 改为构造任意长度的同 trigger target 列表，保持兼容。
- `claimed_provider_ids` 继续防止同一目标在一条逻辑请求内重复取得 ownership。

每个目标到达公共 gate 后独立申请 lease。前一个 probe 的终态在 failover loop 进入
下一个 Provider 前完成，因此不会同时持有多个正在执行的 probe。

## Session Trigger Reservation

reservation 的所有者保持为 request intent，直到某个目标真正到达 transport send：

- gate deny、cooldown、in-flight、Ready-limit 或本地 preparation skip 不移除它；
- ownership 在 send boundary 原子取得并提交 reservation；第一个真实 dispatch 后标记
  为已消费，后续目标不再重复提交；
- send boundary 之前 ownership 被丢弃时，reservation 仍留在 intent 给后续目标；
- 整个 request intent 销毁且从未 dispatch 时，由 reservation 的 Drop 释放 session
  侧占用，下一条请求仍可重新申请；
- 保留现有 probe dispatch 持久化失败时 trigger commit rollback 与 fail-closed 行为。

实现可使用 request-scoped 共享 reservation state 或显式归还机制，但不得让 Provider
gate 直接决定整个多目标 reservation 的终态。

## Cross-Session Recovery Convergence

Provider circuit 维护仅进程内使用的全局单调 `recovery_epoch`。只有
`complete_probe_success` 的 lease 校验、dispatch 校验和可信终态全部成功后，才分配
新 epoch 并写入该 Provider 的 `CircuitSnapshot`；probe failure、stale、abandon 和普通
`CLOSED` success 不推进 epoch。

session 第一次建立路由 binding 时记录当时的全局 epoch 作为
`recovery_epoch_baseline`。该 baseline 在 binding 的滑动 TTL 内保持不变；同一低优先级
Provider 的后续成功、一次 `in_flight` loser 请求或不合格请求都不能把它推进，否则会
吞掉尚未执行的收敛机会。session 过期/清除后，新 binding 从当前 epoch 重新开始。

natural planner 对每个高优先级候选增加一条资格：候选为 `CLOSED` 且其
`recovery_epoch > session.recovery_epoch_baseline` 时，生成 `Direct` 目标。它不申请
Provider probe lease，也不产生 probe metadata；仍经过公共 gate、Ready 上限和现有串行
failover loop。epoch 不是全局消费标志，所以任意数量 session 都能观察同一恢复。目标
成功后 session 绑定到它；目标失败则 circuit 重新记录失败/打开，后续按原自然期限执行。

epoch 不持久化。Gateway 重启会同时丢弃 session bindings，因此不会出现“恢复事实丢失、
旧 binding 仍存活”的跨重启不一致，也无需新增数据库字段或迁移。

## Data Flow

```text
latest route + session binding + circuit snapshots
  -> session recovery baseline + provider recovery epochs
  -> planner: ordered targets + per-target mode + observations
  -> stable target-first reorder
  -> existing failover loop (serial)
  -> common gate per target
       CLOSED: ordinary allow/deny
       OPEN/HALF_OPEN: per-provider single-flight acquire/skip
  -> first transport send consumes session trigger reservation
  -> failure/skip continues; complete success stops and binds actual provider
```

## Observability And Budgets

- 每个不合格 natural candidate 产生 `not_triggered` planner observation；该精确结果仍按
  现有契约从 route/provider-attempt 汇总中排除。
- gate cooldown/in-flight/OPEN deny 仍是普通 skipped attempt，保留在 route 中。
- 每个实际 probe 的 `selection_method`、`probe_trigger`、`probe_result` 和 generation 来自
  自己的 lease，不从列表首目标复制。
- `providers_tried` 仍只在 common gate/preparation Ready 后增长；有序列表不改变
  `failover_max_providers_to_try` 或每 Provider retry budget。

## Compatibility And Rollback

- 不改 circuit 持久化 schema、session schema、设置 DTO 或生成绑定。
- recovery epoch 与 session baseline 都是进程内路由状态；重启时和现有 session binding
  一起归零，不改变持久化兼容性。
- 无绑定 all-open recovery、单目标 direct failback 和普通 failover 都通过同一个新列表
  模型表达，避免维护两套执行循环。
- 若逐目标 intent 无法保持 transport-boundary reservation 原子性，停止实现并回到
  设计阶段；不得通过提前消费 trigger、绕过 gate 或新增隐藏请求规避。
