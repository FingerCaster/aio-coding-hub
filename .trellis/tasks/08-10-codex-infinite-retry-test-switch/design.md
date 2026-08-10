# 技术设计

## 1. 设计目标与依赖顺序

本功能是 Codex 用户 Responses 请求的独立测试执行模式。它不扩大单个 Provider 的重试
预算，而是在一次完整 Provider traversal 全部失败后，再等待固定间隔并生成下一轮计划。
普通模式继续使用现有单轮 failover 和 circuit 行为。

实现顺序固定为：

1. 先完成并合入 `08-10-codex-stream-terminal-firewall`，以其结构化 SSE 帧解析和终态分类
   为共享基线。
2. 本任务只扩展共享 parser 的“完整 final-wire 是否成功”能力，不复制另一套 SSE/JSON
   终态规则。
3. settings schema 和需要的 DB migration 均从实施时已合入基线分配下一个可用版本，不能
   沿用相邻任务规划中的版本号。

若前置任务尚未落地，或同一终态文件仍存在未集成修改，本任务不得开始业务实现。

## 2. 请求状态机

```text
入口校验 / 网关鉴权 / 请求插件与安全策略
                  |
                  v
        计算 typed eligibility + 请求级配置快照
                  |
           +------+------+
           |             |
       普通模式       无限测试模式
           |             |
    现有单轮路径      RoundStart
                         |
                 读取最新 Provider 计划
                         |
            +------------+------------+
            |                         |
         空计划                 按计划遍历 Provider
            |                         |
            |             Provider-local retry / failover
            |                         |
            |                 +-------+-------+
            |                 |               |
            |           final-wire 成功   本次失败/跳过
            |                 |               |
            |             CommitOnce     下一个 Provider
            |                                 |
            +--------------- RoundFailed <---+
                                  |
                      可取消固定间隔 / cooperative yield
                                  |
                              下一 RoundStart
```

终止条件只有：已验证成功、客户端取消/断开、网关关闭，以及进入模式前或明确属于本地信任
边界的拒绝。Provider、transport、timeout、HTTP 状态、Provider-local preparation 和
final-wire 协议失败都不能形成客户端失败终态。

## 3. Eligibility 与信任边界

### 3.1 Typed request facts

在 `RequestContext` 增加明确字段，不从日志 JSON 或 URL 文本反推：

- `is_codex_system_request`
- `codex_infinite_retry: Option<InfiniteRetryRequestConfig>`
- `provider_health_mode: ProviderHealthMode`

`codex_request_classifier` 负责写入 `is_codex_system_request`。无限模式 eligibility 由后端
单一函数判断，条件全部满足才返回配置快照：

- `cli_key == "codex"`；
- 用户发起的 Responses generation；
- 非 compact、非 model discovery、非 Provider test/warmup、非 token count；
- 非 `thread_source=system`；
- 已通过请求结构、网关鉴权、入口插件与安全策略；
- 持久化测试开关在该请求开始时为 `true`。

Provider resolution 的“无 Provider”短路必须在 eligibility 后执行。普通请求仍立即返回
all-unavailable；符合无限模式的请求携带空 round plan 进入 orchestrator。进入模式后的 round
plan/config 临时读取失败也记为失败轮并按间隔重试；不能因为管理面短暂写入或 SQLite busy
向客户端提交失败。

### 3.2 本地终止与可重试失败

- 入口前置的非法请求、网关鉴权、request plugin/security rejection 立即返回。
- 已进入模式后，Provider 凭据准备、目标校验、OAuth 刷新、transport、所有 HTTP 状态、
  响应读取/超时/超限、bridge/fixer 失败和 final-wire 协议失败都记为本 Provider 失败，
  继续本轮或下一轮。
- response plugin 明确返回安全策略 `Blocked` 时仍属于本地信任边界，立即终止；普通插件
  执行错误视为未得到成功响应并进入后续 Provider。两者必须使用 typed outcome 区分，
  不能靠错误文案判断。
- 客户端取消/断开和网关关闭只终止，不补发此前缓存的失败响应。

## 4. 两级快照模型

### 4.1 请求级不可变快照

`InfiniteRetryRequestConfig` 在请求开始时创建，至少包含：

- `enabled=true` 和 `retry_interval_ms`；
- 原始请求 body、headers、path、method、session identity 和 request token；
- 首字节、stream idle、non-stream response timeout 的现有解析结果；
- eligibility facts 与客户端期望的 stream/non-stream 形态。

关闭开关或修改间隔只影响后续请求。单次 timeout 为 `0` 时继续表示禁用；不增加跨轮总
timeout。

### 4.2 轮次级不可变计划

提取 `build_provider_round_plan`，每个 RoundStart 从当前数据库和 settings canonical state
重新读取：

- enabled Provider 列表、active sort mode、排序和候选上限；
- 当前 session route/preference，但不读取 circuit 可用性；
- model route、forced/managed route 的当前合法候选规则；
- Provider base URL、auth/OAuth、bridge、Provider override 和请求修正配置；
- 该轮适用的 Provider-local retry/failover 配置。

生成后得到不可变 `ProviderRoundPlan`，轮中管理员修改不改变它。下一轮重新生成，因此新增、
禁用、改序或修改 Provider 配置从下一轮生效。请求 body、session identity、测试开关、间隔
和 timeout 不随轮刷新。

请求级 `RequestDispatchIntent` 只创建一次。第一笔真实 transport send 消耗 reservation，
空轮、gate skip 或后续轮不得重新制造 compaction/failback reservation。Provider 成功绑定
只在 final-wire 验证成功之后提交；失败轮不得绑定。

## 5. Circuit-neutral health mode

新增显式枚举，而不是复用含义不同的 `provider_health_neutral` bool：

```rust
enum ProviderHealthMode {
    Normal,
    PassiveSystemRequest,
    InfiniteRetryTest,
}
```

`InfiniteRetryTest` 的完整合同是：

- session reuse 可偏好仍在本轮候选集内的绑定 Provider，但不调用
  `circuit.should_allow`；
- 跳过 `plan_request_failback`、all-open recovery、probe planner、probe lease 和 circuit
  dispatch ownership；
- common provider gate 仍执行 account-usage、Provider rate/usage limits、凭据和并发规则，
  仅旁路 circuit/cooldown 子 gate；
- success/failure/timeout/协议失败均不调用 circuit record、cooldown、probe complete 或
  recovery epoch 更新；
- attempt/event 中 `health_mode="infinite_retry_test"`，circuit/probe 字段为 null，不伪造
  CLOSED snapshot；
- 普通模式和现有 system health-neutral 请求继续保持原语义。

实现应在选择、gate、attempt record 和 success finalize 四个集中入口传递该枚举，避免分散
的测试开关条件遗漏某个 circuit 读写点。

## 6. Round orchestrator 与有界状态

### 6.1 普通路径不变

保留现有 `failover_loop::run` 的普通分支和全部终态测试。无限模式进入独立的外层
`run_infinite_rounds`，内部复用单轮 Provider executor；不要把 `RetryEngine` 的
`provider_max_attempts` 设成无穷。

单轮 executor 返回 typed outcome：

```text
ValidatedSuccess(buffered response)
RoundExhausted(summary)
LocalTerminal(response)
Cancelled(reason)
```

测试模式中，普通 retry policy 决定同 Provider 的 local retry/backoff；一旦该 Provider
不再 local retry，任何原本会形成 request-level `Abort` 的上游结果都降为“切换下一候选”。
只有本轮所有候选均失败/跳过后才执行一次固定 round interval。

### 6.2 计数与诊断

新增 `InfiniteRetryLedger`，整个请求共享：

- `u64` saturating round/attempt/failed-round/empty-round 计数及 `overflowed` 标记；
- 固定容量 `VecDeque<InfiniteRetryAttemptSummary>`，只保留最近 100 条脱敏摘要；该类型只含
  round/attempt、Provider id/限长名称、状态、固定 outcome/error/reason code 和 duration，不含
  base URL、响应正文或任意上游 message；
- 固定枚举的 failure-category counters；
- 最多 100 个 Provider summary bucket，超出后合并到显式 `other_providers` bucket；
- 当前 phase、round start、最后活动时间和最终 stop reason；
- 独立的 usage/cost ledger，见第 9 节。

现有代码不能再用 `attempts.len()` 推导全局 attempt index。内部使用 saturating `u64`；投影
到现有 `u32` event 字段时 clamp 并带 `counter_overflowed=true`，持久 JSON 使用十进制字符串
避免 TypeScript safe-integer 丢精度。

每轮创建新的 round-local failed Provider set、retry counters、gate counters 和不可变计划；
全局 ledger 不重置。失败响应 bytes 在记账后立即 drop，再进入下一 attempt。

`InfiniteRetryAttemptSummary` 可在 request-end 投影为现有 attempt JSON 所需的兼容子集，但运行
期不保留完整 `FailoverAttempt`。为 attempt ring、Provider buckets 和 activity metadata 分配固定
序列化预算，总和必须小于现有 256 KiB request-log JSON 上限；最长 Unicode 名称和 100+100
边界下仍应输出有效摘要，不能最后被通用日志层替换成空数组/截断占位。

### 6.3 间隔与取消

- `retry_interval_ms > 0`：使用 `tokio::select!` 等待 timer、请求取消或 gateway shutdown。
- `retry_interval_ms == 0`：每轮至少执行一次 `tokio::task::yield_now()`，并在 yield 前后检查
  取消/关闭，防止空计划或同步准备失败形成 busy loop。
- 取消 token 同时包围 transport future、响应 body 读取和 round sleep；触发后不得再创建
  下一 attempt。

## 7. Final-wire 有界验证与单次提交

### 7.1 共同处理顺序

测试模式下，成功候选必须经过现有 final-wire 变换后才校验：

```text
upstream body
  -> content decoding / Gemini OAuth adapter
  -> protocol bridge
  -> response fixer
  -> response plugin after-hook / chunk transform
  -> 20 MiB bounded final-wire buffer
  -> shared Codex Responses validator
```

response plugin 的安全 `Blocked` 是本地终止；普通 transform error 是本次失败。校验前不调用
circuit success、session success binding、request success log，也不构造下游 response body。

### 7.2 SSE

共享 validator 对完整 final-wire SSE 执行严格状态机：

- 支持 chunk 分割、LF/CRLF、多帧和合法注释；
- 只允许一个有效 `response.completed`，且它必须与响应身份/状态一致；
- `[DONE]` 只允许在唯一 completed 之后按共享协议合同出现；
- `response.failed/error/incomplete`、缺 terminal、EOF、重复/冲突 terminal、completed 后未知
  semantic frame、malformed/unparseable frame 均为失败；
- 只消费 final-wire bytes，不能用 bridge 前的原始响应判成功。

成功后按原顺序回放完整 buffer；测试模式不提供实时 token。失败 buffer 完全丢弃，任何 header
和 body byte 都不得到达客户端。

### 7.3 非流

HTTP 必须是 success，body 必须是唯一合法的完整 Codex Responses success object，且 status
为 completed。2xx 内嵌 error、failed、incomplete、空 body、额外冲突对象或非法 JSON 均失败。
所有非 2xx HTTP 状态直接失败，但仍在 20 MiB 上限内读取结构化 usage/evidence 后丢弃。

### 7.4 Buffer 与 replay

- 每个 attempt 的 upstream-read bytes 与 final-wire payload 都受
  `MAX_NON_SSE_BODY_BYTES`（20 MiB）约束；两者是独立计数，但实现只允许一个完整 payload buffer
  owner，transform 必须消费旧 buffer 或按 chunk 输出，不能同时长期保留两份 20 MiB body。
- 已知 `Content-Length` 超限可发送前拒绝；未知长度在追加下一 chunk 前检查，超限后立即取消
  body、清空 buffer 并记 `response_too_large`。
- 每次 bridge/fixer/plugin 变换后再次检查输出上限；替换 buffer 时先释放不再需要的上一份
  payload。若现有 non-stream/plugin API 会复制完整 body，应先改成 ownership-taking 或有界输出
  helper；不能用磁盘或第二个长期完整副本绕过上限，也不能把失败 body 放进 diagnostic ledger。
- 成功 replay 复用成功轮的安全响应头，移除 hop-by-hop、旧 `Content-Length` 和失败 attempt
  header，重新设置最终长度/stream 语义，并只添加一次 `x-trace-id`。
- 一旦成功 response 交给 axum，后续客户端断开按正常 downstream abort 结束，不重新发起
  Provider 请求。

## 8. Timeout 与停止语义

首字节、stream idle、non-stream response timeout 使用请求级现有快照并逐 attempt 重置。
它们只结束当前 attempt，不结束整个无限请求。`0` 保持禁用，因此单个 Provider 调用可以按
用户配置无限等待；这是显式测试风险，不增加隐藏 watchdog。

stop reason 至少包括：

- `succeeded`
- `client_cancelled`
- `client_disconnected`
- `gateway_shutdown`
- `local_security_rejected`
- `local_unrecoverable_error`

关闭持久开关不属于 stop signal。MVP 不增加“停止全部”控制面。

## 9. Usage、cost 与 request log

### 9.1 客户端与内部用量分离

- 成功 replay 的 bytes 不改写 usage，客户端只看到最终成功 attempt 的原始 usage。
- `RequestCompletion.usage` 和 client-facing usage metrics 使用最终成功 attempt。
- `RequestCompletion.log_usage_metrics` 使用整个请求的累计已知 usage；每个 token 字段分别保存
  known sum 和 missing count。完全无证据时父列为 null；部分 attempt 缺字段时父列保存已知部分，
  summary 标记 `complete=false`，不能按 0 推断一次 attempt 没有消耗。
- 每个有结构化 usage 的失败/成功 attempt 在 buffer 丢弃前进入账本；transport/no-body/无法
  解析 usage 只增加 `unknown_usage_attempts`。

### 9.2 有界 Provider usage ledger

每个 Provider bucket 保存：Provider id/限长 name、attempt count、usage-known/unknown count、各 token
字段的 saturating sum、已计价 cost、未定价计数和 overflow 标记。最多保留 100 个明确
Provider；额外 Provider 合并到 `other_providers`，但 scalar known totals 和已计算总成本继续
累计。任何溢出 clamp 到持久层可表示范围并标记，不能 wrap。

计价应抽取现有 `request_logs` price alias、effective cost basis、priority 和 multiplier 逻辑为
共享 calculator。每次收到 usage 时使用该 attempt 的 Provider/model/config 快照计价，再把
scalar cost 加入总量；不能在结束时拿最终 Provider 的 multiplier 给所有 attempt 计价。
价格缺失保持 unpriced，不估算。

### 9.3 持久化模型

- 每个 trace 仍只有一条父 `request_logs` 记录；运行期最多每秒 upsert 一次，终止时强制一次
  final upsert。
- 父行 token 列和 `cost_usd_femto` 保存累计已知总量；`usage_json` 继续保存最终成功 wire
  usage，避免破坏 replay/export 语义。当前日志 adapter 的 `usage` 会覆盖
  `log_usage_metrics`，因此需把“用于父列的 metrics”与“最终 raw usage JSON”拆成两个 typed
  输入；普通请求继续保持现有优先级，只有无限模式使用分离投影。
- `activity_details_json` 保存版本化、限长的 infinite summary：round/attempt、phase、stop
  reason、usage totals、最近 100 attempts 和 Provider buckets；不保存响应 body。
- 为避免把跨 Provider 总成本归给最终 Provider，新增 implementation-time DB migration 的
  Provider usage/cost 子记录（父 trace + Provider 为稳定键），由同一节流 upsert 批次更新。
  Provider quota/cost 和 provider-scoped usage 查询对无限模式读取子记录；普通父行继续走
  现有查询，不能双计。
- 超过 100 个明确 Provider 后，父行总量仍准确，子记录新增停止并写 overflow bucket；UI/日志
  显示 attribution incomplete，不把 overflow 成本伪归到最终 Provider。
- client cancel/网关关闭也要持久化已经观察到的累计用量与 cost；逻辑请求失败状态不能让
  已发生的上游消耗消失。

`RequestAbortGuard` 持有共享 ledger snapshot handle。future 被 drop 时，guard 释放 active
registry 并异步写最后一个有界 snapshot；它不能复制当前最多 20 MiB response buffer。

## 10. 活动请求与 UI

扩展 `ActiveRequestStart` / `ActiveRequestSnapshotItem`：

- `codex_infinite_retry_test: bool`
- 当前 phase
- round/attempt 的安全投影

`ActiveRequestRegistry` 仍是唯一活动权威。CodexTab 复用现有 active snapshot query，过滤
`cli_key=codex && codex_infinite_retry_test` 计算活动数量；成功、abort guard、shutdown reconcile
和所有 local terminal 路径都必须 `finish`。

`CLI 管理 -> Codex` 增加一个紧凑、明确标为测试用途的设置区：

- `无限重试测试模式` Switch；
- `整轮重试间隔` 数字输入，范围 `0..=60000` ms；
- 当前活动无限重试请求数；
- 持续真实调用及“最坏暂存内存约为活动请求数 × 20 MiB”的风险提示。

保存继续使用 canonical partial settings mutation。活动数量来自 registry，不从 request logs
估算；即使管理员关闭开关，已运行请求仍会计数直到自然终止。

## 11. Settings 跨层合同

新增普通 settings owner 字段：

```text
codex_infinite_retry_test_enabled: bool = false
codex_infinite_retry_test_interval_ms: u32 = 1000  // 0..=60000
```

同步更新：

- Rust `AppSettings` type/default/migration/normalize/validate；
- `SettingsUpdate`、`SettingsPatch`、owned token、apply/equality/rollback；
- generated bindings；
- frontend settings clone/default/validation/patch mapping；
- CodexTab 和相关 fixtures/tests。

旧设置缺字段时迁移为 `false/1000`。非法持久值修复到默认并记录现有设置修复诊断；IPC 写入
越界 interval 返回现有 invalid-input 错误。普通 partial patch 缺字段时保留 canonical 当前值。

## 12. 兼容与回滚

- 默认关闭，因此升级后没有行为变化。
- 普通模式不经过 round planner 外层循环、完整流缓存、neutral circuit 或累计账本分支。
- 运行时回滚是关闭测试开关；只影响新请求。活动请求通过客户端取消或重启网关结束。
- 若 final-wire parser 前置任务 API 变化，只在一个 adapter 处调整，不能退回文案/substring
  成功判断。
- settings/DB migration 必须与 bindings、日志查询和 Provider limit 查询同批落地；不能只写
  新字段而让旧查询静默双计或错归 Provider。

## 13. 主要风险与控制

- **高并发内存**：无专用准入上限；靠每请求 20 MiB hard cap、失败即释放、UI 活动数和风险
  提示控制。测试应以多个并发请求验证 registry 与 buffer 独立。
- **零间隔空轮 busy loop**：强制 cooperative yield + cancellation select。
- **circuit 误写**：用 health mode 集中控制并用 spy/快照测试证明所有 read/write/probe 为零。
- **成功误判**：只依赖共享 final-wire parser；HTTP 2xx/首字节不能提交。
- **日志/成本无界**：ring buffer、Provider bucket cap、saturating counters、1 Hz upsert 和无 body
  日志。
- **跨 Provider 成本错归**：Provider 子账本与父总量分开，Provider 查询不得继续只看
  `final_provider_id`。

## 14. 验证策略

实现阶段按以下层级验证：

1. 纯单元：eligibility、state machine、round interval、saturating/ring ledger、SSE/JSON success
   validator、usage/cost accumulation。
2. 路由集成：多 Provider 多轮、空轮后动态新增、轮内配置固定/轮间刷新、所有 HTTP/transport/
   timeout/preparation failure、最终单次 replay。
3. health：预置 OPEN/cooldown 仍发出请求，前后 circuit snapshot/lease/recovery epoch 不变；普通
   模式回归保持。
4. 生命周期：attempt 中、buffer 中、sleep 中 cancel/disconnect/shutdown；active count、buffer
   释放和 final log stop reason 一致。
5. 跨层：settings migration/ownership/binding、CodexTab 保存/边界值/最长文案、active count 和
   风险提示。
6. 计费：多 Provider、缺 usage、缺价格、priority/multiplier、超大累计与取消终止；客户端 final
   usage、父总量和 Provider 子账本互不混淆。
7. 全量质量门：Rust format/check/test、frontend typecheck/lint/tests、generated bindings、DB
   migration tests、`git diff --check` 和 Trellis check。
