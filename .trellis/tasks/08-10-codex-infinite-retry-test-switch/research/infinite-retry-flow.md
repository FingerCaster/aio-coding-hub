# 无限重试链路研究

## 结论

- 当前 `failover_loop::run` 只消费一次 Provider 列表；单轮结束后直接构造
  `all_providers_unavailable` 或 `all_providers_failed`。无限重试应放在这一层之外，不能把
  某个 Provider 的 `RetryEngine` 改成无限预算。
- Provider 计划现在由 handler middleware 一次性生成。要满足“轮内固定、轮间刷新”，需把
  Provider 选择、会话偏好、候选上限及 Provider 配置读取提取为可复用的 round planner。
- 现有 `provider_health_neutral` 只阻止部分 health 写入，仍会经过 circuit gate、读取
  snapshot 或参与 probe/failback；本功能需要显式的 health mode，完整旁路 circuit/cooldown
  的读取、租约、probe 和写入。
- SSE 成功路径会在确认完整终态之前构造下游 `Body`；非流路径已经完整缓冲，但成功判断仍
  主要依赖 HTTP success。测试模式需在 bridge、response fixer、response plugin 后对
  final-wire bytes 做完整协议校验。
- 当前 request-end 只在序列化时截取最近 100 个 attempt，运行期仍持有无界
  `Vec<FailoverAttempt>`。无限模式必须从源头使用固定容量 ring buffer，并把总数独立为
  溢出安全计数器。
- `ActiveRequestRegistry` 是活动请求权威来源，适合承载测试模式标记、轮次和阶段，从现有
  snapshot 计算 UI 活动数量，无需增加第二套注册表。
- `request_logs` 的 token/cost 目前按一次最终 Provider 响应计算。无限模式跨 Provider
  累计用量时，不能把所有成本归到最终 Provider；需要独立的有界 Provider usage/cost
  breakdown，并让总成本与 Provider 维度查询使用同一份已计价数据。

## 关键代码锚点

### 请求分类与 Provider 计划

- `src-tauri/src/gateway/proxy/handler/mod.rs`
  - middleware 在 Provider resolution 后才注册 active request。
  - 空 Provider 当前会在注册前被短路，测试模式需允许空计划进入 round orchestrator。
- `src-tauri/src/gateway/proxy/handler/middleware/codex_request_classifier.rs`
  - 已识别 `thread_source=system`，但结果没有独立 typed flag。
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs`
  - 负责首次 Provider 选择、无 Provider 终态和 circuit/probe failback 计划。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`
  - `select_providers_with_session_binding` 读取当前 active mode 和 Provider 排序。
  - `resolve_session_bound_provider_id` 会调用 `circuit.should_allow`，测试模式必须旁路。

### Failover 与 health

- `src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs`
  - 当前只有一次 Provider traversal；结束即 finalize。
- `src-tauri/src/gateway/proxy/handler/failover_loop/context.rs`
  - `MAX_NON_SSE_BODY_BYTES` 为 20 MiB。
  - attempt state 是无界 `Vec<FailoverAttempt>`。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs`
  - account usage、circuit、Provider limits 共用 gate；测试模式只能跳过 circuit 子 gate。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_record.rs`
  - 普通失败会写 circuit 并可能把 retry 改成 switch。

### 响应 final-wire

- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs`
  - 最终流顺序为 decode / observer / bridge / response fixer / response plugin / usage relay。
  - 当前只探测前缀，之后直接向下游流式发送。
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs`
  - 已有 20 MiB 有界读取，并在 bridge/fixer/plugin 后解析 usage、记录 success 和绑定 session。
  - 测试模式需把 final-wire protocol success 校验插在 success/circuit/session commit 之前。
- `.trellis/tasks/08-10-codex-stream-terminal-firewall/design.md`
  - 相邻任务已经规划结构化 Codex SSE 终态提取与分类。该任务应先落地；无限模式复用并扩展
    同一 parser，不复制分类规则。

### 日志、用量和 UI

- `src-tauri/src/gateway/active_requests.rs`
  - register/touch/attempt/finish 已覆盖活动请求生命周期。
- `src-tauri/src/gateway/proxy/request_end.rs`
  - request completion 已区分 client usage 与 log usage metrics，可扩展累计账本。
- `src-tauri/src/gateway/proxy/logging.rs`
  - request log JSON 有统一大小上限；新增 summary 应走同一边界。
- `src-tauri/src/infra/request_logs.rs`
  - 成本当前由最终 Provider、最终 model 和父行 token 列计算。
- `src/components/cli-manager/tabs/CodexTab.tsx`
  - Codex 持久设置 UI 入口。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`
  - 已有 settings patch 和 active request snapshot 数据流可复用。

## 实现前置依赖

1. 先完成并合入 `08-10-codex-stream-terminal-firewall`，确认共享 final-wire parser 的实际
   API、settings schema 版本和迁移基线。
2. 本任务从合入后的基线分配下一个可用 settings/DB schema 版本；规划文档不硬编码版本号。
3. 若两项任务的终态相关文件仍有未合并改动，停止实施并先完成串行集成，不能并发复制
   parser 后再解决语义冲突。
