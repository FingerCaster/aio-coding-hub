# 多会话单飞回切回归

## Goal

用真实 Gateway 路由并发证明 Provider single-flight winner 成功后，任意数量 follower
session 在下一次请求中直接收敛，而不是再次等待自然回切期限。

## Requirements

- 使用共享 circuit/session/router 和可阻塞计数 upstream，确定性保持 winner probe 在途。
- follower 数量由集合驱动，至少三个；不得写成只验证两个固定 session 的特例。
- 第一波 followers 必须观察 `in_flight`、对恢复目标零调用并继续各自当前 Provider。
- winner 可信成功后，第二波 follower 请求必须 direct 命中恢复目标、无 probe metadata，
  更新各自 binding，且当前 Provider 调用数不再增长。
- 增加失败 winner 反例，证明没有 recovery epoch 时 follower 不会错误 direct 收敛。

## Acceptance Criteria

- [x] 第一波对目标 Provider 只有 1 次网络调用且为 probe；每个 follower 当前请求成功。
- [x] 第二波目标调用总数为 `1 + follower_count`，每个 follower route 只有 direct 恢复目标，
      所有 binding 收敛。
- [x] 测试有超时边界，不依赖 sleep 猜测 lease 是否已取得。
- [x] `cargo test --lib route_ordered_failback` 能真实匹配并执行新增测试。

## Out Of Scope

- 修改生产逻辑、设置默认值或并行探测多个 Provider。
