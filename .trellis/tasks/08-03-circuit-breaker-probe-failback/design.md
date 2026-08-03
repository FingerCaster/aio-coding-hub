# 熔断试探解除与供应商回切策略 - 技术设计

## 1. 设计目标

本设计实现 PRD R1-R11，并保持两条既有契约：

1. session binding 只拥有稳定 provider 偏好；公共 gate 仍是 circuit/cooldown/limit 的唯一权威放行点。
2. probe 不增加请求 attempt budget，不改变 strict、health-neutral、强制路由和普通同 provider 重试语义。

核心分层如下：

```text
请求分类 / 最新路由 / session 状态
        -> ProbePlanner 生成 request-scoped probe intent
        -> provider candidate 排序（候选仍完整保留）
        -> 公共 gate 原子领取 provider-scoped lease
        -> 发包前保留 session trigger
        -> 进入真实 transport send 边界时提交 dispatch + trigger reservation
        -> 同 provider 重试链共用 lease
        -> 完整终态提交 success/failure
        -> circuit、session binding、日志分别更新
```

“发现候选”“允许发 probe”“确认恢复”“更新会话绑定”是四个独立步骤，不能由一个布尔值隐式完成。

## 2. 组件边界

### 2.1 CircuitBreaker

继续由 `src-tauri/src/shared/circuit_breaker.rs` 拥有 provider 健康状态，并新增：

- provider 级 probe 最短间隔与自然兜底截止时间；
- 运行时 single-flight lease；
- 一次成功恢复与 5 分钟观察期；
- generation 校验和迟到终态拒绝；
- 重启后的 fail-closed 恢复。

`should_allow` 不再在 `open_until` 到期后无条件把 provider 放入可并发通过的半开状态。普通 `OPEN` provider 仍被拒绝；只有携带匹配 `ProbeIntent` 的公共 gate 调用可以原子领取 lease。

### 2.2 ProbePlanner

在 `src-tauri/src/gateway/proxy/handler/` 下增加小型 request-scoped 规划模块，输入：

- 请求分类（正常生成、compact、strict、health-neutral 等）；
- 当前全局回切策略；
- session 稳定 provider 与 compaction generation；
- session 保存的 route snapshot；
- 当前 CLI 最新 route snapshot；
- provider circuit snapshots；
- 当前时间。

输出至多一个 `ProbeIntent` 或一个已恢复 provider 的直接回切决定。Planner 不领取 lease、不调用上游，也不删除任何候选。

### 2.3 SessionManager

扩展 `src-tauri/src/gateway/session_manager.rs` 的 `SessionBinding`：

```rust
struct SessionBinding {
    provider_id: i64,
    sort_mode_id: Option<i64>,
    provider_order: Option<Vec<i64>>,
    route_fingerprint: Option<RouteFingerprint>,
    completed_compaction_generation: u64,
    consumed_compaction_generation: u64,
    last_compaction_fingerprint: Option<[u8; 32]>,
    trigger_reservation: Option<SessionTriggerReservationState>,
    // existing expiry fields...
}
```

- Claude/remote compact 成功后递增 `completed_compaction_generation`。
- Codex 正常请求中的 compaction item 使用有界规范化哈希去重；只有新 fingerprint 递增 generation。
- `completed > consumed` 表示自然回切机会待消费。
- 规划或 gate 通过后只创建带 owner 的短期 reservation，不推进 consumed generation，也不确认新的 route fingerprint。
- 只有进入目标高优先级 provider 的真实 transport send 边界时，才以 reservation owner + expected generation/fingerprint 提交消费。
- auth/model/plugin/request-build 等发包前错误释放 reservation；已经进入 send 后的连接错误、取消或超时属于真实尝试，必须消费。
- session TTL、清除 CLI binding、淘汰和过期逻辑同时清理这些字段，不另建无限增长 map。

`SessionTriggerReservationState` 只存在于当前进程内，包含 request owner、trigger kind、预期 compaction generation、旧/新 route fingerprint 和过期时间。同一 session 同时最多一个回切 reservation；RAII guard 在未提交时释放它。它解决并发请求重复使用同一压缩代次或管理员路由变更的问题，但不把“保留”误当成“已经试探”。

### 2.4 Failover Loop

`provider_selection` 负责取得最新 active route 与稳定 session 信息；`failover_loop/prepare/provider_checks.rs` 负责原子 gate；attempt executor 与 response finalizer 负责 lease 生命周期。

- 一个请求只有一个 `probe_candidate_provider_id`。
- 同 provider 的 OAuth、continuation repair、配置型重试和普通重试共用同一 lease。
- 切换到其他 provider 前必须先以最终失败完成或放弃 lease。
- `providers_tried` 仍只在 provider 经过 gate 并成为 Ready 后增加；probe cooldown/in-flight skip 不消耗 Ready-provider 数量。

### 2.5 Settings 与日志

- Settings 使用现有 `AppSettings -> SettingsUpdate -> runtime sync` 所有权链路。
- Probe 诊断复用 `FailoverAttempt`/`attempts_json`，不创建第二套日志表。
- 请求详情 UI 读取结构化 probe 字段，不从自由文本 `reason` 反向解析状态。

## 3. Circuit 数据模型

### 3.1 持久状态

在 `ProviderHealth` / `CircuitPersistedState` 增加：

```rust
probe_reference_at: Option<i64>,
next_probe_at: Option<i64>,
natural_probe_due_at: Option<i64>,
recovery_guard_until: Option<i64>,
state_revision: u64,
```

含义：

- `probe_reference_at`：最近一次进入保护或实际 probe dispatch/final failure 的基准时间。
- `next_probe_at`：最短间隔 gate；任何 trigger 均不能绕过。
- `natural_probe_due_at`：自然模式 provider 全局兜底时间。
- `recovery_guard_until`：一次成功恢复后的 5 分钟观察截止时间。
- `state_revision`：保证 buffered writer 和迟到更新不会覆盖较新持久状态。

`open_until` 保留，但只代表最长等待 trigger。`half_open_success_count` 为兼容旧 schema 暂时保留并始终写 0；新逻辑不再累计 3 次成功。

持久 `state` 只需要稳定表达 `CLOSED` / `OPEN`。运行时有 lease 时，snapshot/event 可投影为 `HALF_OPEN`，但数据库不依赖一个无法持久化 owner 的 `HALF_OPEN` 状态。加载旧 `HALF_OPEN` 行时归一化为 `OPEN`，清零旧成功数并按保护状态重新计算 probe 时间。

### 3.2 运行时 Lease

每个 provider 最多一个：

```rust
struct ProbeLeaseState {
    generation: u64,
    owner_trace_id: String,
    trigger: ProbeTrigger,
    acquired_at: i64,
    dispatched_at: Option<i64>,
    expires_at: i64,
}

struct ProbeLeaseToken {
    provider_id: i64,
    generation: u64,
    owner_trace_id: String,
}
```

规则：

- `try_acquire_probe` 在 circuit mutex 内同时检查 `OPEN`、`next_probe_at`、无有效 lease 和 intent provider 匹配。
- token 必须同时匹配 provider、generation、owner 才能 dispatch/finish。
- `mark_probe_dispatched` 在真正发送第一个上游请求前调用，并持久化新的 reference/deadlines，使进程随后崩溃也不会立即重复 probe。
- retry index 变化不创建新 generation。
- success/failure/abandon 只接受当前 token；旧 token 返回 stale 结果且零状态写入。已 dispatch 的最终 failure/abandon 会按终态时刻重新计算 deadlines；ownership 在释放锁后同步 upsert 新 revision，避免长流结束后崩溃重启沿用发包时的过早 deadline。同步写失败时当前进程把 provider 保持为不可试探并重试持久化，同时记录明确诊断。
- RAII guard 在未显式完成时执行 abandon。活动 lease 根据请求超时与流活动刷新；超时关闭、任务 drop 和应用重启均不得永久阻塞。未持久化 owner 在重启后自然消失，持久 `OPEN` 与 deadlines 继续生效。

`circuit` 锁内只做内存状态转换并产出带 revision 的持久化快照，不等待网络，也不获取 session 锁或数据库连接。普通状态更新可以交给 buffered writer；`mark_probe_dispatched` 是崩溃一致性边界：释放 `circuit` 锁后，dispatch coordinator 必须在 send future 的首次 poll 内、poll 真实 transport 之前同步 upsert 该快照。数据库写入失败时使用 owner/generation/revision 做受保护补偿，恢复 circuit dispatch 状态与 session trigger reservation，并保持零网络调用。

### 3.3 状态转换

```text
CLOSED
  -- failure threshold --> OPEN
      set probe_reference_at=now
      set next_probe_at=now+provider_cooldown
      set natural_probe_due_at=now+natural_max_wait
      set open_until=now+open_duration

OPEN
  -- eligible intent + lease acquired --> OPEN + probe_in_flight
  -- mark dispatched --> persist fresh deadlines, remain OPEN
  -- complete success --> CLOSED + recovery_guard_until=now+300s
  -- complete failure/abandon after dispatch --> OPEN + reset deadlines + durable upsert

CLOSED + recovery guard
  -- counted failure before guard expiry --> OPEN immediately
  -- neutral/non-counted failure --> remain CLOSED
  -- lazy expiry --> ordinary CLOSED threshold behavior
```

`OPEN -> CLOSED` 只接受当前 `ProbeLeaseToken` 的完整成功。普通请求在 `CLOSED` 时获准、但在 recovery guard 已因另一请求失败而重新 `OPEN` 后才迟到成功时，只能完成自身响应，不能改写 circuit；这防止并发旧成功覆盖新的保护状态。

配置热更新时，`CircuitBreakerConfig` 同时接收 open duration、provider cooldown 和 natural max wait；对 `OPEN` 行以 `probe_reference_at` 重算绝对截止时间。把 cooldown 改为 0 只影响后续资格检查，不绕过现有在途 lease。

## 4. Trigger 与路由算法

### 4.1 ProbeTrigger

稳定枚举用于判断与日志：

```text
new_unbound_session
route_changed
natural_compaction
natural_max_wait
aggressive_turn
max_open_wait
```

若同一请求满足多个原因，采用上述从显式到兜底的固定优先级记录一个主 trigger；资格结果不因日志优先级改变。

### 4.2 最新路由快照

`RouteFingerprint` 由当前 `active_sort_mode_id` 与有序 enabled provider IDs 计算，不包含密钥、名称或其他配置。每轮读取最新 active route：

1. 稳定 provider 已不在最新 eligible set：清除旧绑定并按最新 route 正常选择。
2. route fingerprint 变化：生成一次 `route_changed` 机会；下一轮按新顺序处理。
3. route 未变化：根据自然/积极策略决定是否检查稳定 provider 之前的候选。

route change 机会先按旧/新 fingerprint 与 request owner 保留，在实际向新优先 provider dispatch 后提交；若在 cooldown/single-flight 或发包前准备阶段未发包，释放 reservation 并保留到后续请求。实际尝试失败并回落后，视为管理员新顺序已经被尝试，后续再按 circuit 与所选策略恢复。

### 4.3 自然模式

- 无稳定绑定：最新 route 的首选 `CLOSED` provider 正常使用；首选 `OPEN` 时，短间隔满足后可用 `new_unbound_session` 领取 probe。
- 有稳定绑定且 route 未变化：仅新的 compaction generation、provider 全局 `natural_probe_due_at` 或 `open_until` 到期形成机会。
- 更高 provider 已 `CLOSED`：compaction 后下一轮直接切换，不需要 lease；实际 dispatch 时消费 generation。
- 更高 provider `OPEN`：生成 intent，公共 gate 成功领取 lease并实际 dispatch 时消费 generation。
- probe 失败后不保留旧 compaction generation；下一次提前机会必须来自新压缩，否则等待新的全局兜底。
- 一个 session 的 generation 不影响其他 session。P1 全局恢复后，未压缩的其他自然 session 保持 P2。

### 4.4 积极模式

每个正常生成轮次都读取最新 route：

- 更高 provider `CLOSED`：直接排到稳定 provider 之前并在成功后绑定。
- 更高 provider `OPEN`：生成 `aggressive_turn` intent；短间隔或 in-flight 不满足时继续稳定 provider。
- P1 被其他请求恢复后，不需要重复验证，下一轮直接使用 P1。

### 4.5 公共 Gate 集成

Planner 只把 intent 对应 provider 排到本请求的 probe 位置；所有 eligible candidates 仍进入 `run_gates`。

- intent provider：circuit gate 调 `try_acquire_probe`。
- 其他 `OPEN` provider：继续产生普通 `circuit_open` skip。
- cooldown/in-flight 拒绝：产生 `probe_cooldown` / `probe_in_flight` skip，零上游调用、零 Ready-provider 消耗。
- 对 compaction/route-change 机会，lease 获取后再创建 session trigger reservation；reservation 失败时释放 lease并继续稳定路径。
- lease/reservation 获取后若 auth/model/plugin/request-build 在发包前失败：二者都释放，不消费 session trigger；保留既有错误尝试语义。
- 正常回切路径的 request-scoped `probe_slot_consumed` 阻止同一请求领取第二个 provider lease。
- 若规划时 eligible route 全部为 `OPEN`/`HALF_OPEN`，创建有序的 request-scoped all-open recovery plan。公共 gate 按顺序为每个候选争取 lease；cooldown/in-flight 直接推进，实际 probe 的完整重试链失败并释放 lease后才推进，首个完整成功终止计划。计划始终串行持有至多一个 lease，并受既有 Ready-provider 与 attempt budget 限制。

## 5. 请求与终态处理

### 5.1 同 Provider 重试

`ProbeLeaseGuard` 存放在 provider attempt context，并在 retry engine 创建后续 attempt 时复用。中间失败继续按现有 retry policy 决策；只有决定离开该 provider、最终 Abort，或成功终态才完成 lease。

同 provider 重试链中的中间失败可继续进入既有失败分类/计数，但不得更换 probe generation、释放 lease或启动第二个 probe。最终完整成功以当前 token 关闭 circuit；确定离开该 provider 时才把本 generation 提交为 probe failure 并重置 probe deadlines。

既有 `counts_toward_circuit_breaker=false` 仍只影响对应配置型重试的 circuit 计数，不改变最终 probe 成功必须完整、最终失败必须保持保护的规则。

### 5.2 Dispatch 提交边界

attempt executor 完成所有可能失败的本地准备后，把以下协调器包装进 send future；只有该 future 被首次 poll 时才执行提交。直接切向已 `CLOSED` 的高优先级 provider 时没有 probe token，但仍使用同一 reservation 提交入口：

1. 校验并提交 session trigger reservation；
2. 若本次为 probe，用当前 token 调用 `mark_probe_dispatched`，在 `circuit` 锁内写入 `dispatched_at`、新的 provider deadlines 与 revision，并取得持久化快照；
3. 释放所有 circuit/session 锁；若本次为 probe，在 poll transport 前同步 upsert dispatch 快照；
4. 同步落盘失败时，以 owner/generation/revision 补偿 circuit 与 session reservation并返回错误，保证零网络调用；成功后将 request-scoped `probe_slot_consumed` 置位；
5. 立即 poll 既有 transport `send`。

reservation owner 使第 1 步在正常路径上不会和其他请求竞争；probe 的第 2 步若因 stale token 失败，或第 3 步同步持久化失败，必须补偿刚提交的 session reservation并且不得调用 transport。这里的“实际 dispatch”定义为 dispatch 状态已同步落盘且请求开始 poll 真实 transport：此后即使连接建立失败、调用取消或没有收到响应头，也已形成真实上游尝试，因此本次 compaction/route-change trigger 保持已消费。实现需要为该极短提交块提供专门 API 和故障注入测试，不允许各调用点自行拼接顺序。

### 5.3 非流式成功

仅在 body 已有界完整读取、协议桥接/response fixer/fake-200 检查完成后：

1. `complete_probe_success(token)`；
2. session `bind_success` 到真实 provider；
3. 记录最终 probe attempt；
4. 返回响应。

trigger 已在第一次真实 send 时消费；成功 finalizer 不得再次消费。任何完整成功之前的错误进入现有 retry/failover，不能提前关闭 circuit。

### 5.4 流式成功与失败

`StreamFinalizeCtx` 增加可选 probe token/metadata。`success_event_stream` 收到 2xx 或首包时仅记录 `probe_started`，不调用 probe success、不更新 session binding。

- 可信 completion/正常 EOF 且无 fake-200/terminal error：finalizer 提交 success 并绑定。
- 在 response 尚未构建/提交前发现错误：沿用 failover loop，可切 P2。
- response 已提交后出现 stream error/idle timeout/fake-200/unknown terminal：finalizer 提交 failure，关闭下游流，不启动 P2。
- client abort 对 probe 采用严格规则：除非协议已经明确观察到完整可信 completion，否则不得以现有“输出过内容即可视为成功”的宽松分支关闭 circuit。

不缓存完整流，不跨 provider 拼接内容。

## 6. 压缩分类

### 6.1 Claude

复用 `ModelInferenceMiddleware::is_compact_request`。compact 请求排除 probe，并继续稳定 provider；流式/非流式完整成功 finalizer 调 `mark_compaction_completed`。失败、取消、fake-200 或未知终态零 generation。

### 6.2 Codex

- `POST /v1/responses/compact`（及现有规范化等价路径）分类为 compact 请求，成功后递增 generation。
- 普通 Responses 请求中严格 `type=compaction` item 表示当前请求已经使用压缩上下文。对该 item 做有界 canonical hash；新 fingerprint 可在同一请求生成自然 intent，重复 item 不重复触发。
- 不搜索普通文本，不把 bridge 后生成的 developer message 文本反推为 compaction。

### 6.3 Grok / Gemini

不添加启发式检测。自然模式依靠新 session、route change、300 秒 provider 全局兜底和最长 open trigger。

## 7. 持久化与迁移

### 7.1 SQLite

新增 `v41_to_v42`，向 `provider_circuit_breakers` 添加 nullable deadline/reference/guard 字段和非负 `state_revision`。同步更新：

- migrations registry、baseline、ensure；
- buffered writer insert/upsert/load；
- inert CLOSED 判定；
- provider 删除与配置导入清理测试。

upsert 使用 revision 条件，防止旧 buffered snapshot 覆盖新状态。旧行缺少新时间时以 `updated_at` 为保护基准并按当前 settings 计算；旧 `HALF_OPEN` fail closed 为 `OPEN`。

### 7.2 Settings

Settings schema 从 53 升级一版，新增 enum 和 `natural_probe_max_wait_seconds`。校验规则：

- strategy 仅 `natural` / `aggressive`；未知持久值迁移为 `natural`；
- natural max wait 为 `1..=86400` 秒，默认 300；
- 与 cooldown 的大小不做强制交叉约束，实际 eligibility 始终取短间隔 gate 与 trigger 的共同结果。

设置更新通过 `settings::update` 只拥有这两个字段，并在 durable commit 后热更新 gateway circuit config；运行时更新失败使用现有 owned-field CAS/回滚契约。

## 8. 可观察性

扩展 `FailoverAttempt` 的可选结构化字段，避免解析 `reason`：

```rust
probe: Option<bool>,
probe_trigger: Option<&'static str>,
probe_result: Option<&'static str>,
probe_generation: Option<u64>,
```

稳定值包含 `started`、`success`、`failed`、`cooldown`、`in_flight`、`not_triggered`。实际 probe attempt 的 `selection_method` 使用 `circuit_probe`；gate skip 继续 `outcome=skipped`。

- `attempts_json` 保存完整字段；无需新增请求日志表。
- route projection 保留 provider hop/attempt 数语义；请求详情展示 probe 标签、trigger 与结果。
- 直播 attempt event 可先发 started，request final event/持久日志必须包含最终结果。
- probe metadata 不进入上游请求、下游响应或聊天正文。

## 9. 并发与锁顺序

- circuit health/lease 在同一个短时 mutex 临界区内原子判断。
- 不在 circuit mutex 内获取 session mutex、数据库连接或执行网络 I/O。
- 顺序为：规划只读 session/route -> circuit acquire -> session trigger reserve -> 完成本地 request build -> 专用 dispatch 提交块 -> transport send。任何发包前失败都按相反顺序释放 reservation/lease。
- dispatch 提交块不得持有嵌套的 circuit/session mutex。session reservation 的 owner 保证其提交无竞争；若 circuit token 在随后校验中已 stale，或 dispatch 快照同步 upsert 失败，专用 API 必须补偿恢复刚提交的 session trigger，并在零网络调用状态结束。
- success/failure finalizer 先提交 circuit token，再独立更新 session；任何 stale token 都不能绑定 probe provider。
- 已 dispatch 的 failure/abandon finalizer 在 circuit/token 锁释放后同步持久化终态 deadlines；数据库 I/O 不得发生在 circuit、session 或 ownership mutex 内。
- 设置锁与 circuit mutex 不嵌套：settings durable commit 完成后调用 runtime config update。

## 10. 兼容与回滚

- 默认自然模式尽量保持稳定 session 的既有 provider；行为变化仅是 circuit 可以被受控 probe 提前恢复。
- 新 SQLite 列为加法迁移，旧数据可确定性归一化；迁移前仍应遵循现有数据库备份/版本兼容流程。数据库一旦把 `user_version` 升到 42，当前只支持到 41 的旧二进制会拒绝打开，不能把“代码回退”误写成可直接降级；发布回滚必须恢复迁移前备份，或先提供明确的 schema downgrade 工具。
- feature 实现不拆成独立 child tasks：circuit token、failover terminal 与 session trigger 是一个原子契约，分支并行实现容易造成中间状态不可测试。实施按状态机 -> 路由/终态 -> 设置/UI 的依赖顺序推进。
- 需要回退行为时可先切回自然策略并提高自然最大等待，但不得恢复旧的无 single-flight 半开并发语义。

## 11. 关键风险

- **流式过早成功**：必须审计所有 initial-2xx/first-chunk 成功调用，probe 只能在 terminal finalizer 关闭。
- **lease 泄漏**：所有 return、retry、abort、stream drop 和 panic-safe guard 路径都要覆盖。
- **双重或过早消费 trigger**：compaction generation 与 route fingerprint 必须先 reservation、再在真实 dispatch 边界提交；cooldown、in-flight 和发包前错误都必须保留机会。
- **公共 gate 旁路**：不能在 provider selection 直接把 `OPEN` 当 Ready；lease acquisition 属于公共 gate。
- **attempt budget 漂移**：probe lease 覆盖已有 retry，不得把 circuit threshold 或 probe 次数加入 provider attempt 上限。
- **设置回滚覆盖并发写**：新增字段必须遵循 owned-field token，不能整份 settings snapshot 回写。
