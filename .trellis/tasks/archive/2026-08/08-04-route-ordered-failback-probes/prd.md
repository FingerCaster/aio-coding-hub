# 按路由顺序串行回切所有高优先级供应商

## Goal

修复已有稳定会话只回切第一个高优先级供应商的问题。对于最新有效路由
`p1 -> p2 -> ... -> p(X-1) -> pX -> ... -> pN`，当会话当前稳定使用 `pX`
并触发回切时，本次请求必须按最新路由顺序处理 `pX` 之前的任意数量候选，
成功即停止；全部失败或跳过后才继续 `pX`。

## Background

- 会话绑定会把当前供应商旋转到请求候选列表首位。例如最新路由
  `[p1, p2, p3]` 绑定 `p3` 后，执行列表变成 `[p3, p1, p2]`。
- 当前 planner 只返回一个 `provider_id`，resolution 只把该目标移到首位，
  因而得到 `[p1, p3, p2]`。`p1` 失败后会先回到 `p3`，不会触达 `p2`。
- failover loop、公共 gate、Provider 级 probe lease 和
  `additional_probe_provider_ids` 已能串行处理多个目标；缺口在规划结果、
  顺序恢复、逐目标 trigger 以及 session trigger reservation 的所有权。

## Requirements

### R1. 任意长度的路由前缀

- 不得针对 `p1`、`p2` 或固定数量写特例。
- 以每次请求的最新有效路由为准，定位当前稳定供应商 `pX`，把其前面的完整
  路由前缀作为高优先级候选集合。
- 规划器必须扫描完整前缀并输出有序目标列表，而不是只返回第一个候选。

### R2. 逐候选触发资格

- 路由变化、成功压缩边界和积极回切触发时，按路由顺序处理完整高优先级前缀。
- 自然最大等待场景中，每个候选按自己的 circuit snapshot 独立判断；只有自身
  `natural_probe_due_at` 或既有最长 OPEN 等待已经到期的候选获得本次尝试资格。
- 前面的候选未到期时记录结构化 `probe_result="not_triggered"`，但必须继续检查
  后面的候选，不能再以“最高优先级未到期”为理由阻塞整个前缀。
- 计划时为 `CLOSED` 的候选走普通 direct dispatch；`OPEN/HALF_OPEN` 候选通过
  Provider 全局 single-flight probe lease。不得把 `CLOSED` 请求伪装成 probe。

### R3. 串行执行与停止条件

- 实际调用顺序必须为规划目标的最新路由顺序，之后才是当前稳定供应商及原有
  其余 fallback；不得因 session 绑定旋转而把 `pX` 插到两个回切目标之间。
- 某候选完整成功后立即停止，关闭其 circuit（若为 probe）并把 session 绑定到
  真实成功供应商；不得继续访问后续候选。
- 候选实际失败、公共 gate 拒绝、probe cooldown、已有 in-flight lease 或发包前
  准备失败时，继续下一个规划目标。
- 所有规划目标失败或跳过后，才继续当前稳定供应商；当前供应商成功时保持或
  重新确认其绑定。
- 继续沿用流式响应已提交后不可拼接其他供应商的现有终态规则。

### R4. Reservation、预算与公共 Gate

- route-change/compaction session trigger reservation 由本次有序链中第一个真正越过
  transport send 边界的目标消费且只消费一次。
- gate skip、cooldown、in-flight、Ready 上限判断或其他发包前退出不得提前丢弃
  reservation；若本次请求没有任何规划目标发包，reservation 应释放并允许后续请求重试。
- 每个 `OPEN/HALF_OPEN` 目标继续独立申请 Provider 级 single-flight lease；同一时刻
  本请求最多持有一个实际执行中的 probe lease。
- skipped 候选不消耗 `failover_max_providers_to_try`；Ready 候选继续消耗现有上限，
  不增加隐藏尝试、同 Provider 重试或总 attempt budget。
- 所有候选仍必须经过公共 gate，并保留稳定的 skipped attempt、reason、trigger 和
  probe result 观察数据。

### R5. 兼容性

- 保留无有效绑定且全路由 `OPEN/HALF_OPEN` 时的既有 all-open recovery 行为。
- 强制 Provider、单候选、模型发现、health-neutral、warmup、token 统计、压缩请求
  本身和 managed model route 的资格不变。
- 不新增设置、数据库字段、后台合成探测或并行 probe。

### R6. 多会话 single-flight 后的收敛

- 多个 session 同时命中同一 Provider 的自然回切时，仍只能有一个请求取得
  Provider 全局 probe lease 并执行探测；其他请求必须记录 `in_flight` 且本次可继续
  自己当前的稳定供应商。
- winner 只有在可信完整响应后才发布 Provider 恢复事实。失败、stale、取消或未越过
  transport send 的 lease 不得发布恢复事实。
- 恢复事实不能是全局一次性消费标志。任意数量仍绑定在低优先级供应商的 session
  都必须能独立观察同一次恢复，并在各自下一次合格请求中把该 Provider 作为
  `CLOSED` direct 目标按路由顺序处理。
- follower 不得重新等待 `natural_probe_max_wait_seconds`，不得为已恢复的 Provider
  再申请 probe lease，也不得被另一个 follower 的收敛动作吞掉机会。
- 已在途的 follower 请求不强制中断或改写；如果它在 winner 完成前已经因
  `in_flight` 继续到当前供应商，则从下一次合格请求开始收敛。
- recovery 状态只需覆盖同一进程内仍存活的 session binding，不新增持久化 schema；
  进程重启后 session binding 本身清空，普通最新路由自然从高优先级供应商开始。

## Acceptance Criteria

- [x] 路由 `[p1, p2, p3]`、当前 `p3`、`p1/p2` 均到期时，`p1` 失败后实际访问
      `p2`；`p2` 成功后不访问 `p3`，并绑定 `p2`。
- [x] 至少一个五供应商回归证明目标数量是动态的：按 `p1 -> p2 -> p3 -> p4`
      顺序处理后才允许回到当前 `p5`，不存在固定长度截断。
- [x] 前序候选 cooldown、in-flight 或自然期限未到时产生零网络调用及对应结构化
      skip/not-triggered 观察，后续到期候选仍可尝试并成功。
- [x] `CLOSED` direct 与 `OPEN/HALF_OPEN` probe 混合时仍严格按路由顺序执行，
      每个实际 probe 使用自己的 trigger/lease，direct attempt 不标记为 probe。
- [x] 中间候选成功后，后续高优先级候选和原稳定供应商调用数均为零。
- [x] 所有高优先级目标失败或跳过后才调用当前供应商；当前供应商成功时最终响应、
      route 和 session binding 均指向当前供应商。
- [x] Ready-provider 上限仍按 Ready 数量生效，skipped 不占名额；达到上限后不发起
      额外网络请求。
- [x] compaction/route-change reservation 在前序 gate/pre-send skip 后仍由第一个真实
      transport send 消费；整条链零调用时不消费。
- [x] planner、resolution/dispatch、route integration 回归覆盖任意长度和上述边界；
      完整 Rust library、format、Clippy、generated bindings、typecheck 和 lint 通过。
- [x] 至少三个 follower session 与一个 winner 并发命中同一到期 Provider 时，第一波
      只有 winner 对该 Provider 发起一次 probe；followers 的本次请求可继续旧供应商。
- [x] winner 可信成功后，每个仍绑定低优先级供应商的 follower 在自己的下一次合格
      请求中 direct 访问恢复 Provider、更新绑定且不携带 probe metadata，不再等待 60 秒。
- [x] winner 失败或 stale 时不发布恢复事实；followers 后续仍遵循原自然期限和公共
      gate，不能把失败误判为健康收敛。

## Out Of Scope

- 并行探测多个供应商或后台健康检查。
- 修改回切设置默认值、熔断阈值、Provider cooldown 或普通 retry 策略。
- 改变路由优先级来源、Provider 排序配置或前端设置界面。
- 让自然模式中从未出现恢复信号且期限未到的健康候选强制抢占会话。
- winner 成功后主动中断在途 follower、后台批量改写全部 session binding，或为每个
  follower 合成一次探测请求。
