# 熔断试探解除与供应商回切策略

## Goal

为供应商熔断后的恢复建立由真实业务流量驱动、全局单飞且可观察的试探机制，并提供自然回切与积极回切两种策略。系统既要避免高优先级供应商被会话黏性永久卡在熔断状态，也要控制失败试探对用户延迟、流式输出和提示缓存的影响。

## Background

- 当前 circuit 状态包含 `CLOSED`、`OPEN`、`HALF_OPEN`。`OPEN` 到期仅在请求触达公共 gate 时转为 `HALF_OPEN`；当前半开允许并发请求通过，并要求连续 3 次成功才关闭。
- 当前没有 probe owner、lease 或 single-flight。已有 Provider 可用性测试独立直连，且会把部分 400/404/429 响应判为可用，不能用于自动解除 circuit。
- 会话成功响应会绑定实际 provider，后续轮次优先复用它。临时 circuit/cooldown deny 不会移除候选或清除绑定，公共 gate 仍须记录 skipped attempt，且 skipped 不消耗 Ready-provider 预算。
- 网关中的“高优先级”来自当前排序模式或默认路由的 provider 顺序，不来自 `providers.priority` 字段。
- Claude 已有严格 `/compact` 请求识别；Codex 存在 `/v1/responses/compact` 与 `input[].type=compaction` 语义；Grok/Gemini 当前没有经过验证的统一压缩完成信号。
- 流式响应一旦开始交付客户端，后续中断不能安全拼接另一个 provider 的回答；只有响应尚未提交时的失败才能在同一轮无痕故障转移。

## Requirements

### R1. Probe 范围与候选

- 首期只使用真实用户的正常模型生成请求承担 probe，不创建后台合成探测。流式和非流式生成均可参与。
- token 统计、模型列表/发现、预热/ping、压缩摘要生成请求、内部修复或辅助路径、health-neutral/strict 路径、强制 provider 请求和单候选请求不得取得跨 provider probe 资格。
- 即使所有活跃会话均绑定低优先级 P2，只要仍有符合该路由的正常生成流量，进入可试探窗口的 P1 就必须获得机会，不能要求新建对话或先让 P2 失败。
- 正常回切路径中，只要路由仍有可用的 `CLOSED` provider，一条逻辑用户请求最多 probe 一个 `OPEN` provider，并从当前 CLI 最新生效路由中选择位于稳定 provider 之前、顺序最靠前的合格候选。probe 失败后继续使用已 `CLOSED` 且通过公共 gate 的正常 provider，不得再启动第二个 `OPEN` provider probe。
- 全路由恢复路径中，若请求开始规划时当前 eligible route 的全部 provider 都是 `OPEN`/`HALF_OPEN`，同一请求必须按最新路由顺序串行争取多个 provider 的 probe，直到一个完整成功或候选耗尽。cooldown 未到或已有 lease 的候选零调用跳过；实际 probe 失败后才继续下一个 `OPEN` provider。任何时刻仍只允许当前请求持有一个 probe lease，且每个 provider 的全局 single-flight 不变。
- 全路由恢复在第一个完整成功处立即停止、关闭该 provider circuit 并绑定真实 provider；所有候选都被 gate 拒绝时返回 503，实际 probe 均失败时保留现有最终上游错误契约。它沿用现有 Ready-provider、provider retry 与总 attempt budget，不增加隐藏重试或无限遍历。

### R2. Single-Flight 与时间门限

- 同一 provider 全局同时最多一个在途 probe。未取得 lease 的并发请求不得等待或跟随，应继续使用稳定 provider。
- P1 进入 `OPEN` 或实际 probe 失败后，`next_probe_at = event_at + provider_cooldown_seconds`。默认 30 秒；配置为 `0` 时串行下一轮可立即试探，但并发仍受 single-flight 限制。
- probe timing 使用独立状态，不得通过写入普通 provider cooldown 实现；成功 probe 后不能因限频状态再次被公共 gate 拒绝。
- `circuit_breaker_open_duration_minutes` 保留为“最长熔断等待（可提前试探）”。到期仅形成一个 probe trigger，不得直接 `CLOSED`，也不得无 single-flight 地放开 `HALF_OPEN` 并发流量。
- 触发条件与短间隔同时满足才可实际 probe。每轮资格检查不等于每轮上游调用。

### R3. Probe 尝试链与终态

- 一个 probe lease 覆盖该用户请求在目标 provider 上的完整同 provider 重试链。继续使用既有全局/provider 重试配置、退避、attempt budget、熔断计数与跨 provider failover，不增加 probe 专用重试。
- 重试链中任意一次完整成功即视为 probe 成功；全部失败后，正常回切路径转到 `CLOSED` 备用 provider，全路由恢复路径则释放当前 lease 并按路由顺序争取下一个 `OPEN` provider。
- 一次完整成功即可解除熔断，不再要求 3 次半开成功。非流式必须完整读取并通过现有成功分类；流式必须正常到达可信完成终态。仅有 2xx、响应头、首包或部分内容均不算成功。
- 超时、取消、fake-200、非成功状态、无法分类的终态、流读取失败或流中断均不得解除熔断，并按现有归因决定是否记录 circuit failure。
- 流式 probe 在响应提交客户端前失败时可在同一轮切到 P2；已经输出可见内容后失败时不得拼接 P2，本轮以流错误结束，P1 重新 `OPEN`，会话不绑定 P1，下一轮再使用稳定 provider。

### R4. 恢复观察期

- probe 完整成功后 P1 立即恢复并进入 5 分钟恢复观察期。
- 观察期内任意一次按现有规则应计入 circuit 的失败立即重新 `OPEN`；明确 health-neutral 或配置为不计 circuit 的错误不触发。
- 连续稳定 5 分钟后自动回到普通 `circuit_breaker_failure_threshold` 与 5 分钟滑动失败窗口语义，不新增用户配置。

### R5. 自然回切

- 自然模式以“当前 session 压缩成功后的下一轮正常生成请求”为优先回切边界。生成压缩摘要的请求本身继续使用当前稳定 provider。
- 没有有效稳定绑定的新 session 仍按最新路由自然选择；若首选 P1 为 `OPEN`，在满足短间隔后即可争取 probe，不必等待 300 秒兜底。压缩边界只约束已有稳定绑定的 session。
- 只有压缩请求的完整成功终态才能递增 session 的 compaction generation。下一轮只有真正进入目标高优先级 provider 的 transport send 边界时才原子消费该代次；仅规划、取得 probe lease、限频或 single-flight 竞争失败、以及发包前准备失败均不得丢失。
- 自然模式增加 provider 全局最大等待兜底 `natural_probe_max_wait_seconds`，默认 300 秒。无压缩、无可靠协议信号或识别失败时，下一条合格真实请求在到期后申请 probe；多个 session 不各自创建兜底计时。
- 实际 probe 成功或失败都会重置 provider 最大等待计时。压缩触发可提前申请，但不能绕过 `next_probe_at`。
- 自然 probe 失败后消费本次压缩代次；后续只能由新的成功压缩提前触发，或等待新的 300 秒兜底，不得按 30 秒连续追试而退化为积极模式。
- A、B 均绑定 P2 时，A 成功恢复 P1 后仅 A 切回 P1；B 在自身压缩前继续 P2。P1 已全局恢复后，300 秒兜底不强制迁移其他自然模式 session。

### R6. 积极回切

- “下一轮”指同一个用户 session 内的下一次正常生成请求，不是用户新建对话。
- 每一轮都按最新路由检查当前稳定 provider 之前的候选：高优先级 provider 已 `CLOSED` 时直接切换；仍 `OPEN` 时仅在满足时间门限并取得 lease 后 probe；否则继续稳定 provider。
- A 恢复 P1 后，其他仍绑定 P2 的 session 在各自下一轮直接切到已恢复的 P1，无需重复 probe。

### R7. 路由、会话与公共 Gate

- 已有 session 的回切比较采用每次请求时当前 CLI 最新生效的排序模式及 provider 顺序，而不是 session 首次保存的顺序。路由新增或上移的 provider 可成为已有 session 的回切候选。
- 当前排序模式或有序 provider 集与 session 保存快照发生变化时，视为管理员显式路由变更：已有 session 下一轮即按最新顺序处理。更高优先级 provider 已 `CLOSED` 时直接切换；为 `OPEN` 时仍须满足短间隔并取得 probe lease，不再等待自然压缩。
- 最新顺序只用于发现更高优先级候选；没有回切机会时继续复用稳定 provider，不得无条件重排所有正常请求。
- 已移除、禁用、不再属于当前路由或未通过公共 gate 的 provider 不得因旧 session 绑定重新承接流量。
- 只有实际成功完成请求的 provider 才能更新 session 绑定。probe 失败、取消、部分流输出或被 gate 跳过均不得覆盖稳定绑定。
- 保持公共 gate 权威性、skipped attempt 可观察性、Ready-provider 上限及现有重试/故障转移预算契约。

### R8. 压缩协议分类

- Claude 使用现有严格 `/compact` system prompt 分类，并只在流式/非流式成功终态提交 compaction generation。
- Codex remote compaction 使用严格方法/路径分类；后续正常请求中明确的 `input[].type=compaction` 可作为已压缩上下文信号。普通文本中的同名内容不得触发。
- Grok/Gemini 首期不猜测压缩完成，直接使用自然模式 300 秒兜底。
- 不得根据消息数、body 大小、token 变化或模糊文本推断压缩。

### R9. 状态并发与恢复

- probe lease 必须携带 provider-scoped generation/owner，并覆盖请求重试及流式终态。超时、取消、panic/drop 或进程重启不得永久留下 `probe_in_flight`。
- 只有当前 generation 的终态可以提交成功、失败、`next_probe_at` 与恢复观察期；迟到的旧请求不得覆盖新状态。
- recovery guard 内 counted failure 重新 `OPEN` 后，之前从 `CLOSED` 状态放行的普通在途请求即使随后成功，也不得关闭 circuit；`OPEN -> CLOSED` 只能由当前 probe token 的完整成功完成。
- provider 的 circuit、`next_probe_at`、最大等待基准和恢复观察截止时间应在重启后保持保护语义；在途 owner 不持久化，重启后按无在途请求安全恢复。
- session compaction generation 与现有 session binding 使用相同生命周期；session 过期或绑定清除时一并清理。

### R10. 设置与界面

- 新增全局 `provider_failback_strategy`，取值 `natural` / `aggressive`，默认 `natural`；新增全局 `natural_probe_max_wait_seconds`，默认 `300`。
- 首期不增加按 CLI、路由、模型或 provider 的覆盖层级。运行时每轮读取当前全局策略，设置修改只影响后续请求。
- 旧配置缺少字段时迁移/默认到自然模式与 300 秒，设置读写和失败回滚沿用现有 `AppSettings` 所有权契约。
- 在“熔断与重试”区域提供策略选择和自然最大等待输入，并把既有熔断时长文案改为“最长熔断等待（可提前试探）”。界面需说明回切触发与 `provider_cooldown_seconds` 最短实际 probe 间隔的区别。

### R11. 可观察性

- probe 进入现有 attempt/route 日志和请求详情链路，稳定区分 `probe_started`、`probe_success`、`probe_failed`、`probe_cooldown`、`probe_in_flight` 及不满足策略条件等结果。
- 日志应呈现例如“P1 probe 失败 -> P2 成功”的完整路由，不把 probe 内部提示写入用户聊天响应。
- 不因本功能新增请求正文、响应正文、凭据或其他敏感数据记录。

## Acceptance Criteria

- [ ] **AC1 / R1-R2**：P1 `OPEN`、所有 session 绑定 P2 且 P2 持续成功时，后续正常生成流量仍能在短间隔/策略触发满足后产生一个 P1 在途 probe；并发请求不等待并继续 P2。
- [ ] **AC2 / R1**：仍有 `CLOSED` 备用时，一条请求只 probe 最新路由中最靠前的一个 `OPEN` 候选；全路由均 `OPEN` 时，同一请求按最新顺序串行 probe，cooldown/in-flight 候选零调用跳过，首个完整成功停止并绑定；全部 gate 拒绝才 503，实际 probe 均失败时保留现有最终错误。
- [ ] **AC3 / R2**：默认首次及失败后 probe 最短间隔为 30 秒；配置为 0 时串行请求可逐轮 probe，并发仍只有一个；probe timing 不污染普通 provider cooldown。
- [ ] **AC4 / R2**：`circuit_breaker_open_duration_minutes` 到期只允许下一条合格请求争取 single-flight，不自动关闭 circuit 或放开并发半开流量。
- [ ] **AC5 / R3**：probe 沿用既有同 provider 重试链，任一完整成功即可恢复；正常回切失败后按原预算转 Closed P2，全 Open 恢复失败后按原预算推进下一 Open，不产生额外 retry。
- [ ] **AC6 / R3**：2xx/响应头/首包/部分流均不能关闭 circuit；完整非流式或可信流式完成才成功。流式晚失败不拼接 P2，结束本轮并重新保护 P1。
- [ ] **AC7 / R4**：恢复后 5 分钟内一次可计 circuit 的故障立即重新 `OPEN`；不计 circuit 的错误保持中性；超过观察期后恢复普通阈值。
- [ ] **AC8 / R5**：自然模式中压缩请求保持 P2，成功后下一轮才切换或 probe；只有真实 transport dispatch 才消费 compaction generation，限频、竞争失败和发包前错误不会丢失。
- [ ] **AC9 / R5**：没有压缩信号时，P1 连续 300 秒未获实际 probe 后由下一条合格请求申请全局兜底；失败后不在 30 秒处连续追试。
- [ ] **AC10 / R5-R6**：A、B 均在 P2 且 A 恢复 P1 后，自然模式仅 A 回切、B 等自身压缩；积极模式下 B 下一轮直接回到 P1。
- [ ] **AC11 / R6-R7**：管理员调整当前路由顺序后，已有 session 下一轮按最新顺序处理：健康候选直接切换，`OPEN` 候选仅在合格 probe 成功后切换；无资格或失败时保持原绑定。
- [ ] **AC12 / R8**：Claude `/compact`、Codex compact path/item 能产生严格压缩信号；Grok/Gemini 和模糊文本/body 变化不误触发，并走 300 秒兜底。
- [ ] **AC13 / R1、R9**：token 统计、模型发现、预热、压缩生成、内部修复、strict/health-neutral、强制和单候选路径不 probe；取消、超时、重启和迟到终态不会卡住或错误提交 lease。
- [ ] **AC14 / R10**：新旧配置默认自然/300 秒，设置页可读写并持久化两项新设置，熔断时长显示为可提前试探的最长等待。
- [ ] **AC15 / R11**：请求日志和详情可区分 probe 开始、成功、失败、短间隔和在途跳过，并显示 P1->P2 链路；聊天响应和日志均不泄漏内部标记或新增敏感内容。
- [ ] **AC16 / 全部**：测试覆盖状态转换、single-flight 竞争、两种策略、压缩代次、最新路由、同 provider 重试、多个 `OPEN` 候选、全路由串行恢复、流式早/晚失败、恢复观察期、重启恢复、设置迁移和既有公共 gate/attempt budget 回归。

## Out of Scope

- 后台定时或合成健康探测请求。
- 自然/积极之外的第三种回切策略。
- 按 CLI、路由、模型或 provider 覆盖全局策略。
- 把 provider-level circuit 改造成模型级 circuit。
- 复用或改变 Provider 页手动可用性测试的成功标准。
- 为流式 probe 缓存完整回答、跨 provider 拼接流输出，或在已经输出部分内容后无痕重放。
- 改变现有普通重试次数、Ready-provider 预算、强制路由或 strict/health-neutral 请求语义。
