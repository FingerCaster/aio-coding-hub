# 多会话回切收敛状态

## Goal

让 Provider 全局 single-flight probe 的成功结果被任意数量仍绑定低优先级 Provider 的
session 独立观察，并在各自下一次合格请求上收敛到恢复 Provider。

## Requirements

- 只有可信、已 dispatch 且 lease 仍有效的 probe success 发布恢复 epoch；失败、stale、
  abandon 和普通 success 不发布。
- recovery epoch 与 session baseline 仅存在于进程内；不得增加数据库字段、设置或生成
  绑定。重启时 session bindings 同时清空。
- session 第一次建立路由 binding 时捕获当前全局 epoch，并在 binding 存活期间保持；
  滑动 TTL、同 Provider 成功和 in-flight loser 不得推进它。
- natural planner 逐候选比较 Provider recovery epoch 与 session baseline。较新的健康
  `CLOSED` 候选走 direct；`OPEN/HALF_OPEN` 继续走原 probe 资格和 single-flight。
- direct follower 收敛仍经过公共 gate、串行 failover、Ready/attempt budget，且不携带
  `selection_method=circuit_probe`、probe trigger 或 generation。
- 支持任意数量 session 和任意长度 Provider 前缀，不使用一次性消费标志或固定 ID。

## Acceptance Criteria

- [x] circuit 单测证明 applied probe success 单调推进 epoch，失败/stale 不推进。
- [x] session 单测证明创建时基线固定，同 Provider refresh 和滑动 TTL 不会吞掉恢复事实。
- [x] planner 单测证明 newer recovered `CLOSED` 为 direct，旧 epoch/健康无恢复信号仍为
      `not_triggered`，并能与任意长度 due probe 目标按路由顺序混合。
- [x] 现有 all-open、natural max wait、route-change、compaction、预算和完整 Rust 测试通过。

## Out Of Scope

- 后台批量重绑 session、取消在途请求、持久化 session 或 recovery epoch。
- 修改自然回切等待时间、熔断阈值或 Provider 排序。
