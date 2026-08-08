# 可配置模型路由核心技术设计

## 设计目标

在 Codex Provider 转译删除完成后，为真实推理请求提供全局策略和 Provider 三态覆盖。路由只改写最终上游 wire request，不改变客户端原始请求身份、Provider 选择、账户余额门控、健康/熔断、Session/failback 或 transport retry 的所有权。

实现基线为 `main@8757d32c` 以及删除子任务完成后的 SQLite 44。外部提交 `e2a2996265a92baf9d363e8ee8e6370a817f2d62` 只提供产品契约；发送链、持久化、日志和 UI 按当前仓库重写。

## 策略契约

```rust
ModelRoutingPolicy {
    enabled: bool,
    rules: Vec<ModelRoutingRule>,
}

ModelRoutingRule {
    source_model: String,
    target_model: Option<String>,
    reasoning_effort: Option<String>,
}
```

限制与规范化：

- 最多 128 条规则；source/target 各最多 256 UTF-8 字节；effort 最多 64 个 Unicode 字符。
- 写入时 trim 三个文本字段，拒绝空 source、控制字符、规范化后重复 source，以及 target/effort 同时为空的规则。
- 匹配只使用中间件在任何 Provider 转换前推导的客户端原始模型，区分大小写且只匹配第一条。target 不会再次进入规则表，避免级联。
- `reasoning_effort` 是有界自由文本。各上游协议负责把它转换到自己的 wire 字段，不在共享策略层维护易过时的枚举。
- disabled policy 可保留已经规范化的规则供用户重新启用，但运行时完全忽略规则；enabled 且清洗后无有效规则的策略强制转为 disabled。

### Provider 三态

| 持久化值 | UI 状态 | 生效策略 |
| --- | --- | --- |
| `NULL` | 继承全局 | 使用全局策略 |
| `Some(enabled=true)` | 使用专属规则 | 完整替换全局，不做合并或未匹配回退 |
| `Some(enabled=false)` | 禁用路由 | 当前 Provider 不执行配置路由 |

策略在每个 Provider 候选上独立解析。上一 Provider 的匹配、marker 或失败不得带入下一候选。

## 严格写入与防御性读取

- settings 更新、Provider upsert、复制/分享/导入使用同一后端严格规范化函数；前端校验只改善交互，不能成为安全边界。
- 全局设置 JSON 无法解析、字段来自未来版本或清洗后无有效规则：返回 disabled 全局策略，记录有界诊断。
- Provider override 的非 NULL JSON 无法解析：返回 `Some(disabled)`，明确抑制全局策略，不能错误解码成 `None`。
- 防御性读取不记录原始 JSON、模型全文或请求体；诊断只含作用域、Provider id、错误分类和是否发生清洗。
- 上述损坏策略路径继续发送原始未改写请求，不计为应用失败，也不触发 Provider failover。

## 请求分类与数据流

```text
bounded decode / request-body state
  -> infer immutable original client model + request intent
  -> provider resolution / account projection / failback planner
  -> common account/circuit/auth gates
  -> provider preparation / CX2CC / built-in transforms
  -> request sanitizer
  -> RequestBeforeSend plugin
  -> clear previous attempt route marker
  -> resolve this Provider's effective policy using original model
  -> atomically apply target + effort to cloned path/query/body
  -> verify every requested output in final wire request
  -> sync active model + write provider-scoped marker
  -> build final URL / finalize encoded body / fingerprint
  -> dispatch ownership transport commit
  -> upstream
  -> attempt log / final response / cost
```

只处理 POST 的真实推理端点：Claude Messages、Claude CX2CC 的最终 Responses、普通 Codex Responses/compact、Grok Chat/Responses、Gemini generateContent/streamGenerateContent。排除 managed `aio/`、模型发现、可用性探测、token counting、搜索、列表、辅助端点和非 POST 流量。分类应复用显式 request intent 和现有端点解析器，不能只用“JSON 中存在 model”猜测。

## 原子 wire 改写

路由器接收 immutable original model、最终协议分类以及 `PreparedProvider` 的 path/query/body 快照。它先克隆可变状态，在克隆上用结构化 JSON/URL API完成全部改写和验证，再一次性提交：

- Claude Messages：`model` 与 `output_config.effort`。
- Responses，包括普通 Codex、compact、Grok Responses 和 CX2CC 最终请求：`model` 与 `reasoning.effort`。
- Chat Completions：`model` 与顶层 `reasoning_effort`。
- Gemini：结构化替换 path 中的 model 段；effort 可解析为十进制整数时写 `generationConfig.thinkingConfig.thinkingBudget`，否则写 `thinkingLevel`，并删除另一个 sibling。

规则要求 target 和 effort 时，两者必须都能写入并从最终状态验证；任何一项失败都不提交部分状态。target 与当前最终值相同也可成功，只要最终验证一致。压缩请求通过 `GatewayRequestBody` 克隆/替换 decoded body，再由既有 finalize 维护编码头和 body state。

当前 `attempt_executor` 在插件之前构造 URL。为了支持 Gemini path/query 改写，URL 构造必须移动到配置路由成功之后；fingerprint、body finalize 和 `commit_at_transport_boundary` 都继续位于其后。配置路由失败时不能出现上游 fingerprint 或 transport commit。

## 应用失败与 Provider failover

有效 enabled 策略精确匹配后，如果最终协议不支持所请求字段、JSON/path 无法解析、序列化失败或最终值无法验证，产生专用发送前结果：

- attempt outcome：`configured_model_route_apply_failed`
- public error code：`GW_CONFIGURED_MODEL_ROUTE_APPLY_FAILED`
- 最终 HTTP：在该错误成为全候选终局时使用 502

处理规则：

1. 记录当前 Provider 的安全失败分类，不包含请求体、凭据或完整模型值。
2. 立即结束当前 Provider 的 retry loop，同一 Provider 不做 transport retry。
3. 不加入健康失败集合，不更新熔断、不写账户 `blocked_provider_ids`/recovery epoch、不提交 dispatch ownership。
4. outer Provider loop 继续下一候选，重新经过其共同 gate 并解析其独立策略。
5. pending dispatch ownership 以未提交状态放弃；既有 Session reservation 只按 planner 原生命周期留给后续候选或在请求结束释放。

如果后续 Provider 成功，客户端只收到该正常响应，失败 attempt 保留审计。如果至少一个 Provider 已实际发送上游后失败，最终响应继续遵循既有 all-providers-failed 优先级；只有所有通过前置 gate 的可用候选都因配置路由无法应用而耗尽时，才返回上述路由错误。响应完成后不会自动创建新的客户端请求。

## Marker、日志与成本

成功应用后写入 Provider-scoped marker：Provider id、original source model、effective model、effort、policy source（global/provider）、实际 pricing CLI 和分别是否应用 model/effort。marker 必须绑定当前 Provider；每个新 attempt 开始先清除旧 marker，只有原子提交成功后重建。

- `requested_model` 始终保存客户端原始模型。
- 每个 `FailoverAttempt` 保存自己的路由摘要或应用失败 outcome，最终 special settings marker 只描述最终 Provider。
- `effective_cost_basis` 优先使用与 final Provider id 匹配的 configured route target/pricing CLI，其次使用 CX2CC cost basis，最后才是原始 requested model。
- configured route 命中但 target 价格不存在时成本为 unknown，禁止回退 source model；CX2CC 的 token usage 语义修正仍可独立保留。
- 没有 target、仅改 effort 的规则继续按最终未变模型计价。
- 下一 Provider 无路由或策略损坏时，上一 Provider marker 必须已清除，防止错误计价和 UI 泄漏。

## 持久化与交换格式

### Settings 56 到 57

- `AppSettings` 增加全局 `model_routing_policy`，迁移默认 disabled。
- settings 写入使用严格规范化；读取使用防御性清洗。
- 完整配置 v4 可携带该字段，并使用独立的“模型路由最低 bundle 版本 = 4”能力常量；v1-v3 预检必须清除此字段，不能通过手工注入绕过版本能力。

### SQLite 44 到 45

- `providers` 增加 nullable `model_routing_policy_json`，迁移使用幂等列检查并同步新安装 baseline。
- 所有 Provider gateway/summary/query projection 同步读取该列；upsert 使用 `override_specified` 区分保留、清除和设置。
- 本机复制完整复制三态 override；旧 Provider 默认 NULL 继承全局。
- 损坏非 NULL 值读取为显式 disabled，不自动写回；用户下次保存时写入规范化值。

### Provider 分享 v3

- 新增严格 v3，并在 configuration 中加入 nullable policy override；新导出只写 v3。
- v1/v2 继续由各自严格结构解析，再转换为内部 v3 canonical，override 固定为 NULL；删除子任务留下的 legacy `model_mapping` 只被丢弃。
- v3 round-trip 保留 inherit/enabled/disabled 三态；未来字段和版本继续拒绝。

## 桌面端

- 全局设置复用一个无嵌套卡片的规则编辑器：启用开关、紧凑规则表、添加按钮和图标删除操作。
- Provider 编辑器使用明确的三段选择控件“继承全局 / 使用专属规则 / 禁用路由”；只有专属规则状态显示可编辑规则，禁用状态可保留已规范化草稿。
- 请求日志展示 original -> effective model、effort 和 policy source；无路由时不添加装饰。成本未知必须显示未知，不能显示 source 估算值。
- 表单创建、编辑、重置、复制、分享预览和账户用量测试状态不能互相清空。
- Rust 绑定统一重新生成，前端服务类型不得手写漂移。

## 发布与回滚

- 功能默认 disabled，数据库/settings 变更均为加性；删除子任务先独立验收，再启用本实现。
- 代码回滚可停止读取新增字段并保留数据；Provider 分享 v3 会被旧应用作为未来版本拒绝，避免静默丢失。
- 若迁移版本、完整配置 v4 发布边界或发送链所有权在实现前变化，暂停并更新设计，不能自动占用新版本。
- 路由模块应集中且无网络副作用，便于关闭全局策略或回退调用点；不得通过回滚恢复已删除的 Codex 转译。

## 测试矩阵

- 策略：trim、限制、重复、控制字符、精确大小写、无级联、disabled、三态 replace/suppress、损坏全局/override。
- 协议：Claude、CX2CC、Responses/compact、Grok Chat/Responses、Gemini 两种 endpoint 与数值/文本 effort。
- 顺序：built-in/CX2CC/sanitizer/plugin 在前，路由在后，URL/fingerprint/transport commit 在路由之后。
- 原子性：model-only、effort-only、两者成功；任一失败均零部分提交、零上游发送。
- failover：下一 Provider 重解析、全部路由失败、混合上游失败、余额 blocked、恢复回切、forced/unbound、reservation 与 abort。
- 请求体：identity、gzip/zstd/brotli 等当前支持状态在无修改/有修改时保持一致。
- 日志成本：final Provider marker、attempt 隔离、仅 effort、target 有价/无价、CX2CC pricing CLI/usage 组合。
- 持久化/UI：settings 56->57、SQLite 44->45、share v1/v2/v3、bundle v1-v4、复制、三态表单和生成绑定。
