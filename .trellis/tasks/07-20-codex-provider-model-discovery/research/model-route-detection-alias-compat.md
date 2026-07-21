# Codex 受管模型 alias 与路由检测兼容性

## 结论

Codex 配置中始终只暴露 `model_provider = "aio"`。AIO 可按需生成 profile，并在网关内部把 AIO canonical alias 路由到具体供应商模型。

现有模型路由检测若直接比较 Codex 请求 alias 与上游响应模型，会把预期路由误报为异常。检测必须比较“本次 attempt 最终发给上游的模型”与“上游实际返回的模型”。

## 关键证据

- 请求模型提取：`src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs:31`
- 核心检测器：`src-tauri/src/gateway/model_route_mapping.rs:25`
- 非流式检测：`src-tauri/src/gateway/proxy/handler/success_non_stream.rs:1088`、`:1331`
- SSE 提前终结：`src-tauri/src/gateway/proxy/handler/success_event_stream.rs:453`
- 正常 SSE observer：`src-tauri/src/gateway/proxy/handler/success_event_stream.rs:1167`
- SSE 最终检测：`src-tauri/src/gateway/proxy/streams/usage_tee.rs:477`
- body-buffer 检测：`src-tauri/src/gateway/proxy/streams/usage_tee.rs:901`
- 成本模型选择：`src-tauri/src/infra/request_logs.rs:310`
- 前端 route resolver：`src/services/gateway/requestLogSpecialSettings.ts:215`
- 严重告警展示：`src/components/home/requestLogPresentation.ts:210`

## 三层模型语义

1. `requested_model`
   - Codex 发来的 AIO canonical alias，例如 `aio/grok-4.5`。
   - 保留 profile、入口协议和 AIO 审计语义，禁止被远端模型覆盖。

2. `requested_upstream_model_id`
   - 每个 provider attempt 经 provider 选择、bridge、`ModelMapping` 后，最终写入上游请求的模型。
   - 例如 `grok-4.5`；必须按 attempt 记录，不能只挂在整次请求上。

3. `observed_upstream_model_id`
   - 从上游原始响应中采集的 `model`。
   - 应在 bridge、fixer、plugin 改写响应前采集。

异常检测唯一有效的比较是：

```text
requested_upstream_model_id vs observed_upstream_model_id
```

不得比较 `requested_model` 与 `observed_upstream_model_id`。

## 四类检测落点

以下路径必须调用同一语义的检测逻辑，并传入当前 attempt 的 wire model：

1. 非流式响应：普通 JSON 返回后检测。
2. body-buffer：缓冲响应解析到上游模型后检测。
3. 正常 SSE：observer/usage tee 收齐模型信息后检测。
4. SSE 提前终结：提前完成路径也使用同一 attempt 上下文。

状态建议：

- 两端均存在且相同：`matched`。
- 两端均存在且不同：写入真实 `model_route_mapping`，标记异常。
- 响应没有 `model`：`unobserved`，不告警，也不宣称已验证匹配。

预期 alias 还原不得写成 `model_route_mapping`，也不得依赖前端 alias 白名单消警；否则日志、告警和后续消费者仍会把正常路由当异常。

## 受管路由日志

建议新增 provider-scoped special setting，与异常映射分离：

```json
{
  "type": "aio_managed_model_route",
  "canonicalModel": "aio/grok-4.5",
  "providerId": 17,
  "remoteModelId": "grok-4.5",
  "requestedUpstreamModel": "grok-4.5",
  "pricedModel": "grok-4.5",
  "applied": true
}
```

- `providerId` 必须是最终选中的具体供应商，避免同名模型跨供应商串用。
- `model_route_mapping` 仅表示非预期偏离。
- 前端可展示“由 AIO 路由至供应商/模型”，但不得显示严重异常样式。
- 重试或故障转移时，每个 attempt 分别记录实际 provider 和 wire model。

## 成本兼容

`requested_model` 目前隐含参与计价；引入 canonical alias 后不能直接用 alias 查远端价格。建议按以下顺序选择成本模型：

1. 与最终 provider 匹配的 `cx2cc_cost_basis`。
2. 与最终 provider 匹配的 `aio_managed_model_route.pricedModel`。
3. 旧日志兼容回退到 `request_logs.requested_model`。

计价键必须同时受 provider 约束，不能只按模型字符串匹配。

## 最小测试矩阵

| 场景 | canonical | wire | observed | 预期 |
|---|---|---|---|---|
| 受管 alias 正常路由 | `aio/grok-4.5` | `grok-4.5` | `grok-4.5` | 无异常；有受管路由记录 |
| 上游真实偏离 | `aio/grok-4.5` | `grok-4.5` | `grok-4` | 写异常映射并告警 |
| 响应无 model | `aio/grok-4.5` | `grok-4.5` | 空 | `unobserved`，不告警 |
| provider 故障转移 | 同一 alias | attempt 各自模型 | 各自响应模型 | 按 attempt 独立判断与计价 |
| 非流式 | 任意 alias | 相同 | 相同 | 不误报 |
| body-buffer | 任意 alias | 相同 | 相同 | 不误报 |
| 正常 SSE | 任意 alias | 相同 | 相同 | 不误报 |
| SSE 提前终结 | 任意 alias | 相同 | 相同或空 | 匹配或 `unobserved` |

验收还应断言 `requested_model` 始终保持 canonical alias，前端不会把 `aio_managed_model_route` 渲染成模型路由异常。
