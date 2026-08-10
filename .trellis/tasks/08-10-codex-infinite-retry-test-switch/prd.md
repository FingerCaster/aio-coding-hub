# Codex 测试无限重试开关

## Goal

为 Codex 网关提供一个显式的测试开关。开启后，对符合条件但尚未成功的请求持续发起后续请求，不因常规重试次数耗尽或熔断而提前向 Codex 返回失败；用于稳定复现和验证长时间重试、续写及恢复场景。

该能力不得改变默认生产行为。

## Background

- 用户要求提供“无限循环、不熔断、不成功就一直不返回 Codex、一直请求”的能力，并允许将其设计为测试开关。
- 该能力是独立的网关测试模式，与 Codex 降智拦截 / reasoning guard 无关。
- 当前仓库已存在通用重试、Provider failover、熔断、超时与 Codex 流终态处理；本功能只为显式测试模式增加完整 Provider 轮次的外层循环。
- 本任务仅处于规划阶段，尚未批准实现。

## Repository Evidence

- 通用 `UpstreamRetryPolicy` 只为匹配的 HTTP、transport 和提交前 Codex 流错误提供有限 `max_retries`，并与 Provider attempt budget、backoff 和 circuit accounting 协作，见 `.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md` 与 `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md`。
- retry engine 超过 `provider_max_attempts` 后退出，只有一次性的 `allow_next_retry_beyond_max_attempts` 可以临时越界，见 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs:36-43`；普通系统失败还可能经 circuit 状态把同 Provider retry 改写为 switch，见 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_record.rs:258-273`。
- failover orchestrator 当前只遍历一次 Provider 列表，随后提交 `all_providers_unavailable` 或 `all_providers_failed`，见 `src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs:305-403`。
- HTTP 200 不等于 Codex 成功；Responses SSE 可能以错误终态、非法终态或无成功终态 EOF 结束。相邻规划任务 `08-10-codex-stream-terminal-firewall` 正在设计共享的结构化终态安全边界，两项工作不得并发修改同一分类契约后再盲合并。
- 普通 retry 的 `upstream_retry_policy.backoff_ms` 默认 `100ms`、上限 `60_000ms`，只适用于匹配规则，见 `src-tauri/src/infra/settings/types.rs:170-189`、`src-tauri/src/infra/settings/defaults.rs:110-115`；不能充当整轮失败后的固定测试间隔。
- 网关的 retry/circuit/Codex 设置使用持久化 `AppSettings`、迁移/校验、生成绑定和设置 UI；当前没有可复用的 session-only 测试开关所有者。
- 完整响应现有共享上限 `MAX_NON_SSE_BODY_BYTES` 为 20 MiB，见 `src-tauri/src/gateway/proxy/handler/failover_loop/context.rs:17`。
- request-end 仅序列化最近 100 次尝试，但活动 failover state 仍持有无界 `Vec<FailoverAttempt>`，见 `src-tauri/src/gateway/proxy/request_end.rs:13,337-348` 与 `src-tauri/src/gateway/proxy/handler/failover_loop/context.rs:407`；无限模式必须在运行期间就改为有界状态。
- `ActiveRequestRegistry` 已提供注册、活动快照和所有终止路径移除能力，见 `src-tauri/src/gateway/active_requests.rs:48-126`，可扩展为测试模式活动数量的权威来源。

## Requirements

- R1：提供一个明确可控、默认关闭的测试模式开关。
- R2：开关关闭时，现有 Codex 重试、熔断、超时、失败切换和响应行为保持不变。
- R3：开关开启时，单个 Provider 内仍执行现有尝试预算、路由和 failover 规则；一轮内全部候选 Provider 均未成功时，不进入现有 all-failed/all-unavailable 客户端终态，而是开始下一完整轮次，直至达到定义明确的成功条件。
- R4：在尚未成功期间，不向下游 Codex 客户端提交最终失败响应。
- R5：测试模式必须可被自动化测试稳定启用和关闭，且不得依赖修改生产默认值。
- R6：测试模式的每轮尝试必须保留足够的可观测信息，以判断它仍在推进并定位停止原因，同时不得泄漏受保护的推理或原始敏感载荷。
- R7：成功必须由完整、合法的 Codex 协议成功结果确认，不能仅根据 HTTP 2xx、响应头、首字节或任意非终态帧判断。
  - 应在协议桥接与响应变换后的 final-wire Codex Responses 语义上判定；Responses SSE 必须出现唯一、有效的 `response.completed` 成功终态，并合法结束。`response.failed`、`response.error`、`response.incomplete`、终态前 EOF/断流、读取/空闲超时、重复/冲突终态、完成后的未知语义帧或无法解析终态均不算成功。
  - 非流式响应必须是 HTTP 成功且包含合法、完整的 Codex 成功响应对象；HTTP 2xx 内嵌错误、失败或 incomplete 状态不算成功。
  - 未成功轮次的响应头、正文和 SSE 帧不得提前提交给下游；最终只交付成功轮次的一份完整响应。
- R8：测试模式必须使用独立的固定整轮重试间隔 `retry_interval_ms`，默认 `1000ms`、合法范围 `0..=60_000ms`；仅在一轮内所有候选 Provider 均未成功后等待一次，再开始下一完整轮次，`0` 表示不等待。Provider 之间继续使用当前规则，不额外施加该间隔；该设置不能改变成功判定，也不能被普通模式误用。
- R9：每个完整轮次必须保留当前 Provider 排序、候选限制、Provider-local retry、路由、凭据处理和 failover 规则；无限模式只接管全轮失败后的终态，不把单个 Provider 改成无限重试对象。
- R10：测试模式不得读取或写入 circuit/cooldown 作为 Provider 跳过或失败副作用；已有 open/cooldown 不阻止本模式发出请求，本模式失败也不推进 failure count、open/probe/cooldown 状态。普通模式的 circuit/cooldown 行为必须保持不变。
- R11：通过持久化 `AppSettings` 提供默认关闭的 Codex 全局测试开关及 `retry_interval_ms`，并在 `CLI 管理 -> Codex` 提供明确标记的配置入口；保存后只影响新请求，每个请求在开始时获取不可变配置快照。设置默认值、校验、迁移、patch ownership、生成绑定和前端状态必须一致。
- R12：测试模式只适用于用户发起的 Codex Responses 生成请求，包括流式与非流式；必须显式排除 compaction、模型发现、Provider 测试/预热、Token 计数及 `thread_source=system` 内部请求。被排除请求继续执行现有重试、熔断、成功判定与返回规则。
- R13：测试模式下的流式上游响应必须先完整暂存并通过协议成功校验；确认唯一、有效的 `response.completed` 后，才向客户端提交成功轮次的响应头与完整 SSE。确认前不得发送首字节、增量 Token、失败终态或其他中间帧；该测试模式允许牺牲实时流式延迟。
- R14：测试模式下每次上游尝试最多暂存 20 MiB 完整响应数据，与现有 `MAX_NON_SSE_BODY_BYTES` 对齐；超过上限时必须在下游提交前丢弃本次全部数据、记录有界诊断并将本次尝试视为失败。已丢弃数据必须在下一次尝试前释放，不得通过无界内存或无界磁盘维持无限循环。
- R15：测试模式不得施加覆盖所有 Provider 轮次的累计请求超时；每次上游调用仍必须使用请求启动时现有配置所解析出的首字节、流空闲和非流式响应超时。单次超时属于可重试失败；配置为 `0` 时保持关闭语义，不为测试模式引入隐式超时。
- R16：无限循环必须可被请求级取消与网关关闭信号协作终止；终止信号必须能打断整轮间隔等待并尽快取消尚未提交的上游工作，且不得在终止时补发此前失败响应。关闭持久化测试开关不得追溯终止已启动请求；MVP 不提供额外的全局强停 UI。
- R17：每个完整 Provider 轮次开始时必须从当前配置重新生成该轮调用计划，包括 Provider 清单、启停状态、顺序、候选上限、路由与 Provider 配置；该计划在轮内保持不可变，配置变更只从下一轮生效。原始客户端请求与会话身份保持请求级快照，现有 Provider-local 合法请求修正继续沿用当前规则；测试模式开关和整轮重试间隔也保持请求级快照，每次尝试内部的凭据/OAuth 刷新继续沿用当前规则。
- R18：无限模式必须使用有界诊断状态而不是无限增长的 attempt `Vec` 或日志行：每个逻辑请求只对应一条 request log，维护溢出安全的累计轮次/尝试计数、按 Provider 与失败类别汇总的有界计数，以及最近 100 次脱敏尝试摘要。运行中活动状态落库最多每秒一次；终止时记录最终汇总、最近摘要与停止原因。不得持久化失败响应正文或让 retained diagnostics 随轮次线性增长。
- R19：下游成功响应必须原样保留最终成功轮次的 usage 语义，不得注入或累计此前失败轮次的用量。内部 quota/cost/log 统计必须累计所有上游实际报告的用量，并能区分累计已知消耗、按 Provider 汇总与最终成功轮次用量；没有 usage 证据的尝试必须计为未知，不能估算或默认为零。所有累计运算必须溢出安全且保持有界存储。
- R20：请求一旦通过本地入口校验并进入测试无限模式，每轮重新选择得到零个可调用 Provider 时也必须形成一个可观测的空失败轮次，按同一 `retry_interval_ms` 等待后重新读取配置。请求合法性、网关鉴权、插件/安全策略等在进入测试模式前产生的本地拒绝不得被吞掉或循环。
- R21：MVP 不得为测试无限模式新增并发准入、排队或超额拒绝语义；所有符合条件的请求按现有网关并发模型独立运行。`CLI 管理 -> Codex` 必须明确警告持续上游调用与约 `活动请求数 × 20 MiB` 的最坏暂存风险，并显示当前活动无限重试请求数；活动计数必须在成功、取消、断开、异常退出和网关关闭时可靠归零。

## Technical Notes

- `retry_interval_ms=0` 表示不增加墙钟等待，但每个空轮次或完整失败轮次后仍须至少协作式 yield，并优先响应取消/关闭信号，避免无 Provider 或同步失败路径独占运行时。
- 测试模式的“所有上游失败都重试”覆盖 HTTP、transport、timeout、协议终态失败和已经进入轮次后的 Provider-local 准备失败；它不改变进入测试模式前的本地信任边界。
- 结构化 final-wire 成功验证应与 `08-10-codex-stream-terminal-firewall` 共用一个解析/分类契约。实现前必须先确定两任务的串行顺序并以已落地版本为基线；不得复制两套会漂移的终态解析器。
- 相关设置 schema 版本必须从实现时已合入基线顺延分配；相邻终态任务当前计划占用下一 schema，不能在两个规划文档中同时硬编码同一个版本。

## Acceptance Criteria

- [ ] AC1：默认配置下，现有有限重试与熔断相关测试及行为无回归。（R2）
- [ ] AC2：开启测试模式后，模拟完整 Provider 调用轮次连续失败多轮，网关按相同候选顺序重新开始下一轮，且不会向客户端返回 all-failed/all-unavailable 或重试耗尽终态。（R3、R4、R9）
- [ ] AC3：在若干失败轮次后模拟成功，客户端只收到一次符合现有协议约束的最终成功响应。（R3、R4）
- [ ] AC4：自动化测试能在同一测试进程中隔离开关状态，不污染其他测试或默认配置。（R1、R2、R5）
- [ ] AC5：诊断数据能区分普通模式与测试无限重试模式，并能观察轮次及最终停止原因，且不记录敏感响应正文。（R6）
- [ ] AC6：HTTP 200 + `response.failed/error/incomplete`、缺少 `response.completed` 的 EOF、空闲超时和不可解析终态都继续重试；只有完整协议成功才结束循环。（R7）
- [ ] AC7：经历任意多个失败轮次后，客户端看不到失败轮次的响应头、正文、SSE 或内部诊断，只收到最终成功轮次。（R4、R7）
- [ ] AC8：使用暂停时间分别验证 `0`、`1000`、`60_000ms` 边界；同一 Provider 轮次内部不增加测试间隔，整轮失败后只等待一次，成功后不再等待，普通模式的现有 backoff 行为不受影响。（R8）
- [ ] AC9：多 Provider 测试证明每轮仍遵循现有顺序、候选上限和 Provider-local retry；第二轮重新执行完整链路，而不是无限停留在最后或第一个 Provider。（R9）
- [ ] AC10：预置 open/cooldown Provider 仍会在测试模式中按序收到请求；连续多轮失败前后 circuit/cooldown 快照完全不变，关闭测试模式后原有 gate 和失败计数行为恢复。（R10）
- [ ] AC11：全新/旧设置默认关闭；UI 可启停并保存合法间隔，拒绝越界值；开关切换只影响随后创建的请求，不改变已运行请求的配置快照，普通设置写入不丢失该字段。（R11）
- [ ] AC12：覆盖各类 Codex 请求的分类测试证明，只有用户 Responses 生成请求进入测试无限重试；compaction、模型发现、Provider 测试/预热、Token 计数与 `thread_source=system` 请求在失败时仍按现有规则结束。（R12）
- [ ] AC13：上游先发送若干可见 SSE 帧后以失败、断流或非法终态结束时，客户端仍未收到响应头或正文；后续轮次成功后，客户端只收到该成功轮次完整且顺序一致的 SSE，且回放前已确认唯一有效的 `response.completed`。（R13）
- [ ] AC14：分别验证恰好 20 MiB 与超过 20 MiB 的流式/非流式响应；超限尝试不向客户端提交任何字节，诊断标记为响应超限，后续 Provider/轮次仍可成功，且连续超限轮次的 retained bytes 不累积。（R14）
- [ ] AC15：在短首字节/流空闲/非流式超时下，单次超时后继续后续 Provider 与完整轮次；累计运行时间超过任一单次超时仍不产生总超时终态。对应配置为 `0` 时，不出现测试模式额外注入的超时。（R15）
- [ ] AC16：分别在上游调用进行中和整轮间隔等待中触发客户端取消/断开与网关关闭，循环及时退出、不再发起新尝试、无失败内容下发且暂存数据被释放；关闭测试开关不影响已启动请求，只影响随后请求。（R16）
- [ ] AC17：在第一轮进行中新增、禁用、改序或修改 Provider/路由配置，证明第一轮计划不变而第二轮采用新快照；同时证明切换测试开关或修改间隔不会改变该活动请求，请求载荷与会话身份跨轮保持一致。（R17）
- [ ] AC18：以 `retry_interval_ms=0` 运行超过 100 次尝试，证明只有一条 request log、累计计数正确且不溢出、仅保留最近 100 次脱敏摘要、内存诊断大小稳定，并且活动写入频率不超过每秒一次；成功、取消与关机均记录正确停止原因。（R18）
- [ ] AC19：构造多个含 usage、缺失 usage 的失败轮次及最终成功轮次，证明客户端仅看到最终成功轮次的原始 usage；内部已知总量和按 Provider 成本仅累加实际报告值，缺失项显示未知且不会被填零或估算，超大累计值不会溢出。（R19）
- [ ] AC20：活动请求在连续空轮次期间不向客户端返回 all-unavailable，管理员随后启用/新增合法 Provider 后下一轮成功；同时非法请求、网关鉴权失败及插件/安全拒绝在发出任何上游请求前仍立即返回且不会进入循环。（R20）
- [ ] AC21：并发启动多个符合条件的请求时均进入独立循环且不因测试模式专用上限排队/失败；UI 活动数量准确变化，风险提示可见，所有正常与异常终止路径均释放活动计数和最多 20 MiB 的单请求暂存。（R21）

## Out of Scope

- 改变默认生产重试策略或默认启用无限重试。
- 移除现有普通模式的熔断与最大尝试次数保护。
- 恢复或修改 Codex 降智拦截、`reasoning_tokens` 规则或 reasoning guard 统计。
- 把进入测试模式前的本地非法请求、网关鉴权失败、插件/安全策略拒绝、客户端取消/断开或网关关闭改成无限重试。
- 改变 compaction、模型发现、Provider 测试/预热、Token 计数及 `thread_source=system` 内部请求的重试或终态语义。
- 为 MVP 增加管理员“停止全部无限重试”按钮或让关闭测试开关追溯中断活动请求。

## Notes

- 这是复杂任务；规划收敛前需要补齐 `design.md` 与 `implement.md`。
