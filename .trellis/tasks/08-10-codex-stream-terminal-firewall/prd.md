# Codex 流终态错误安全处理

## Goal

为最终交给 Codex 客户端的 Responses SSE（包括原生和已归一化的桥接输出）建立稳定的
终态错误安全边界：供应商新增或变形的错误文案不应直接穿透到客户端，也不应因此把所有
错误盲目重试或切换供应商。分类、重试/切换和客户端展示必须彼此解耦，并保留足够的内部
诊断证据。

## Confirmed Facts

- 当前原生 Codex 流容量识别只把 `selected model is at capacity`、
  `server_is_overloaded` 和 `slow_down` 作为内置容量信号；识别入口为
  `src-tauri/src/domain/usage.rs:171-187`。
- 原生 Codex 流在提交前会调用终态错误分类；未命中重试关键词、也未命中内置容量识别的
  终态帧会返回 `StartStreaming`，随后进入 `forwarded_after_commit`，见
  `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:336-360`。
- 已复现的上游帧包含 `service_unavailable_error`、`server_error` 和
  `Our servers are currently overloaded. Please try again later.`；该文案已经由网关提交给
  Codex，客户端显示 `stream disconnected before completion` 及原文。
- 现有契约要求：重试只在提交前发生；提交后不能拼接不同 Provider 的输出，且应保留原流完整性。
  相关契约见 `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:43-68,103-106`。
- 现有路由测试明确覆盖未知终态错误的转发语义；本任务需要重新定义该语义时，必须同步更新测试，
  不能只扩大一个字符串匹配表。
- Codex Responses 路径已经通过独立 relay 把上游 chunk 发送给下游，见
  `src-tauri/src/gateway/streams/usage_tee.rs:754-907`；因此提交后替换终态帧在架构上可实现，
  但当前 relay 是任意 chunk 透传。安全替换必须增加有界的完整 SSE 帧缓冲，处理跨 chunk、同 chunk
  多帧、CRLF、尾部残帧和下游断开，不能直接做字符串替换。
- 当前 usage tracker 在 chunk 返回下游前读取原始帧。若客户端投影层放置不当，会让内部日志只看到
  改写后的错误。因此设计必须显式区分原始诊断输入和客户端可见输出。
- 规划期间两次检索官方 OpenAI Responses 流事件文档均因当前网关上游 502 失败；具体兜底 SSE
  payload 在实施前仍需以官方协议或当前 Codex 客户端源码验证，不能凭经验固定字段。
- 当前设置已有 `retry_keywords` 与 `non_retry_keywords`；后者的旧语义是“不重试后继续原流”。
  进一步的调用链检查显示，前置 `retry_keywords` 分类只在
  `is_native_codex_responses_event_stream_path(...)` 分支执行；非原生/桥接流的终态帧走
  `FinalizeAsEmptyBody`，不会使用这两个关键词。因此当前字段不是通用的第三方 HTTP 200 SSE
  错误重试机制，保留两套旧列表会让新的全量拦截模型更难理解。
- “全量拦截/改写”与“重试”是两个不同边界：前者约束客户端可见投影，后者只决定提交前是否
  消耗共享预算并切换供应商。不能把所有被拦截的终态都按 `retry_keywords` 重试。
- 第三方供应商确实可能以 HTTP 200 传输 SSE、再在终态事件或 `data` 中表达错误；但其事件
  类型、字段和桥接阶段并不统一，不能假设 Codex 关键词表可以覆盖这些错误。需要先由供应商
  适配/结构化分类归一化，再进入同一安全投影边界。

## Product Decisions

- 已提交可见输出后，默认丢弃终态错误帧并结束连接，不伪造新的终态帧；客户端可显示自身的
  通用流中断提示，原始证据留在内部日志。
- 公开配置只保留 `passthrough_keywords` 透传例外；结构化分类负责重试决策，不因“已拦截”
  自动重试。启用拦截时，硬性非重试类别优先于任何旧兼容词，容量信号不能通过透传例外放行。
- 旧字段保留一版后端双读并逐步退场：`non_retry_keywords` 迁移为受限透传例外，
  `retry_keywords` 仅作为 unknown 的提交前兼容覆盖，不再作为新的 UI 配置入口。
- 提交前的 unknown 和硬性非重试终态统一返回现有 `502 + GW_FAKE_200` 标准网关
  error envelope；在协议 fixture 未验证前不伪造 Codex SSE 终态帧。
- `stream_internal_errors.enabled` 是全量流终态处理的总开关。关闭时不做终态拦截、结构化
  重试、Provider 切换或客户端改写，capacity 与其他终态一样按原流透传；内部 tracker
  可以继续被动记录 evidence。该决定明确替换当前“关闭重试仍硬拦截 capacity”的旧行为。
- 默认迁移策略已确认：新安装或字段缺失时 `enabled=true`；已有配置明确保存的 `false`
  必须保留，不因升级自动打开。
- 兼容边界已确认：settings schema 升级到 59，旧字段只双读兼容一个版本；Provider share
  升级为 v4，继续读取 v1-v3，旧客户端遇到 v4 必须明确拒绝而不能静默丢字段。

## Requirements

- R1. 所有受支持的 Codex final-wire 终态 SSE 帧（原生或已归一化桥接输出中的 `error`、
  `response.error`、`response.failed`、`response.incomplete` 及对应 `data.type`）必须经过
  统一的安全处理入口；新增供应商文案不应默认原样透传。
- R2. 终态错误的语义分类与 retry/failover 决策分离；不能因为启用安全处理就对参数、策略、认证、
  配额等错误盲目重试或跨 Provider 扩散。
- R3. 可确认的瞬态/容量错误继续复用现有共享 retry budget、backoff、circuit 和 Provider failover；
  不增加独立等待或计数；该动作只在全量流终态处理开启时生效。
- R4. 不可安全分类的终态错误必须使用稳定、脱敏的客户端错误投影；供应商原始 message/code/type
  仅保留在有界、脱敏的内部请求日志和诊断中。
- R5. 提交前终态错误可以转换为标准网关终态；已经提交可见输出后不得改变 HTTP 状态、拼接其他尝试的
  输出或破坏 SSE 顺序，默认丢弃终态错误帧并结束下游流，不伪造新的终态帧。
- R6. 现有用户的 retry 规则、Provider override、日志字段、非 Codex final-wire 路径和未归一化
  bridge 路径不得被无意迁移；配置默认值和新策略的启用方式必须明确且可回滚。
- R7. 增加覆盖当前复现帧、结构化分类边界、提交前/提交后行为、客户端脱敏和内部证据保留的回归测试。
- R8. 内置结构化分类负责所有受支持终态帧的瞬态/策略/未知分流，不因供应商更换一条文案就要求
  用户新增匹配词；未知终态默认拦截并隐藏，不盲目重试。
- R9. 公开配置目标模型只保留一个 `passthrough_keywords` 列表，负责“禁止重试且允许客户端透传”
  的显式例外（例如 `high-risk cyber` / policy 拒绝）；它不改变重试预算，也不能在拦截开启时
  让容量信号透传。
- R10. 现有 `retry_keywords` / `non_retry_keywords` 的 UI、持久化兼容和历史用户行为必须在规划中
  明确：不能直接删除字段导致旧设置丢失，也不能无提示地把旧规则变成更宽的透传规则。
- R11. 新模型的公开配置应避免同时暴露“重试关键词”和“不重试关键词”两套相互竞争的文本
  规则；结构化分类负责重试决策，文本配置仅表达明确的客户端透传例外，并可按供应商/错误
  类别限定作用范围。
- R12. 迁移必须可审计且可回滚：已有 `non_retry_keywords` 只能在受限透传语义下迁移，已有
  `retry_keywords` 只能作为短期兼容覆盖；新建或保存的配置不得继续生成两套旧字段。
- R13. `enabled=false` 必须完整旁路新增的终态动作层：不消耗 stream-internal retry budget、
  不切换 Provider、不改写或丢弃终态帧，并以 `disabled_passthrough` 记录被动观察结果。
- R14. settings schema 59 必须区分字段缺失与显式 `false`；Provider share v4 必须保持旧版本
  读取、当前版本严格导出和未知字段拒绝，不能静默丢失透传例外。

## Out Of Scope

- 不重做 Claude/Gemini 原生协议的流终态契约；只有已经归一化为 Codex Responses SSE 的桥接
  输出进入本任务边界。
- 不改变最终 HTTP 4xx/5xx rewrite 规则的匹配语义、优先级或保存所有权。
- 不为每个供应商维护一套公开文案词典；供应商差异通过结构化适配和内部别名处理。
- 在协议 fixture 未验证前，不新增凭经验伪造的 Codex SSE 终态 payload。

## Acceptance Criteria

- [ ] 当前 `service_unavailable_error + server_error + overloaded message` 不再把供应商原文直接交给 Codex。
- [ ] 上述拦截只在 `enabled=true` 时发生；`enabled=false` 时 capacity 和其他终态均保持原流，
  且不触发 stream-internal retry/failover。
- [ ] 可靠的瞬态错误仍按现有预算执行重试/切换；参数、策略、认证和未知错误不会被无条件重试。
- [ ] `enabled=true` 时客户端响应不包含供应商敏感或容量信号；无论开关状态，内部日志仍保留
  限长、脱敏证据。
- [ ] 提交前和已提交两条路径均有明确、可测试的终态语义，且不跨尝试拼接可见内容。
- [ ] 现有非目标 SSE、未归一化 bridge 请求、Codex continuation、fake-200、probe 和普通成功流
  回归测试不退化。
- [ ] Rust/前端配置投影（如有变更）保持一致，并通过格式化、类型检查和相关路由测试。
- [ ] settings schema 59 与 Provider share v4 的迁移、导入、导出和拒绝路径均有回归覆盖。
