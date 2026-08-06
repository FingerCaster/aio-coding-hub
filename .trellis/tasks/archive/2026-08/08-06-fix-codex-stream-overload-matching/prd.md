# 修复 Codex 流过载错误匹配

## Goal

修复原生 Codex Responses HTTP 200 SSE 过载错误未被 AIO 网关拦截的问题，避免
上游 `server_is_overloaded` / `slow_down` 错误原样转发后由 Codex 二次显示为
`Selected model is at capacity. Please try a different model.`。

## Background

- Codex 0.146.1 在收到 `error.code = "server_is_overloaded"` 或 `"slow_down"`
  时，会在客户端映射为 `ServerOverloaded` 并显示 capacity 文案；该文案不是
  AIO 入站 SSE 的原始文本。
- AIO 的流错误匹配目前只检查 `selected model is at capacity`，位置为
  `src-tauri/src/domain/usage.rs:126-145`。
- 当前有效分类开关是父级 `upstream_retry_policy.enabled` 与子级
  `stream_internal_errors.enabled` 的逻辑与，位置为
  `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:336-345`。
- 现有前置检查在未命中可重试关键词且未识别 capacity 时会进入
  `StartStreaming`，随后日志记录为 `forwarded_after_commit`，位置为
  `success_event_stream.rs:347-360`。
- 最终 HTTP 错误重写规则不处理 HTTP 200 SSE 内部错误；修复边界属于 Codex
  流错误分类与 Provider failover，不改变 HTTP rewrite 规则。

## Requirements

- **R1 统一容量识别**：原生 Codex Responses SSE 的终态错误中，只要已知错误
  字段包含 `selected model is at capacity`、`server_is_overloaded` 或
  `slow_down`，均视为容量错误；匹配大小写不敏感，不能依赖 Codex 最终展示文案。
  协议错误码是内置容量别名，不要求用户配置关键词。
- **R2 保持配置语义**：父级重试关闭时，容量错误仍不得原样转发给 Codex；按
  既有语义直接进入 Provider 切换/失败流程。父级和子级均启用时，内置容量错误码
  直接进入现有流错误 retry/failover 引擎，并复用共享 retry budget/backoff；
  用户关键词继续处理第三方 Provider 的自定义错误文案。
- **R3 证据与安全**：请求日志继续只保存结构化、限长、脱敏的错误证据，不保存
  原始 SSE；新增识别字段不得改变现有脱敏和日志 schema。
- **R4 非目标保持不变**：非容量的 unknown/non-retryable 流错误、已提交响应、
  guard 到期、buffer cap、桥接请求和最终 HTTP rewrite 行为保持现状。
- **R5 回归覆盖**：补充 domain 分类测试和 buffered native stream 路由测试，至少
  覆盖三种容量信号、大小写变化、顶层 `error.code` 与 `response.error.code`、
  父级关闭时的 pre-commit 拦截，以及非容量错误仍可转发。
- **R6 无新增 UI**：不新增开关、规则或输入项；现有“Codex 200 流内部错误”开关
  继续控制处理，现有关键词编辑器只承担自定义错误匹配。

## Acceptance Criteria

- [x] `server_is_overloaded` 和 `slow_down` 在原生 Codex SSE 终态中被识别为容量错误。
- [x] `selected model is at capacity` 的现有匹配行为和正向关键词优先级不回归。
- [x] 父级重试关闭、子级流错误打开时，容量 SSE 在下游提交前被拦截，不再返回
      原始 HTTP 200 capacity 流；日志保留 `classification = disabled` 和结构化证据。
- [x] 父级与子级开启并命中默认/配置关键词时，继续产生 retry/failover 行为，不
      增加额外 backoff 或消耗独立预算。
- [x] 父级与子级开启时，内置 `server_is_overloaded` / `slow_down` 无需用户关键词
      也进入现有 retry/failover 行为。
- [x] 非容量终态错误仍保持既有 unknown/non-retryable 转发语义，且所有相关测试通过。
- [x] 前端界面、设置 schema、generated bindings 与持久化数据均无变化。

## Out Of Scope

- 不修改 Codex 客户端、其错误展示文案或上游服务行为。
- 不修改最终 HTTP 4xx/5xx response rewrite 规则。
- 不改变用户已有关键词、Provider override、retry budget、circuit breaker 或日志字段结构。
- 不新增或修改前端配置界面。

## Key Decisions

- 以 Codex 协议错误码作为稳定容量信号，文本只作为兼容性补充；这样能覆盖 Codex
  将上游错误转换为固定 UI 文案之前的真实 SSE 数据。
- 容量拦截边界与重试开关解耦：关闭重试只改变“重试还是切换/失败”，不重新允许
  容量 SSE 穿透到 Codex。
- `server_is_overloaded` / `slow_down` 是系统内置协议别名；现有开关决定启用重试
  还是直接切换，用户不需要在界面了解或维护这些内部错误码。

## Risks / Deferred Items

- `service_unavailable_error` 类型本身不单独视为容量，因为它可能表示其他服务错误；
  只有明确的 `server_is_overloaded` / `slow_down` code 或既有 capacity 文本触发
  容量拦截。
- 回归测试必须分别构造顶层 `error.code` 与 `response.error.code`，避免只修复单一
  envelope。
