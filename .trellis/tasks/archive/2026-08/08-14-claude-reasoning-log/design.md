# Claude 思考强度调用日志设计

## Data Flow

```text
最终 outbound path/body
  -> protocol-aware effort extractor
  -> provider attempt（effort + upstream_sent）
  -> request attempts JSON / realtime attempt event
  -> final effort selector（last success, else last sent）
  -> RequestLog summary/detail + request event
  -> TypeScript adapter / trace projection
  -> shared reasoning-effort badge
```

## Backend Ownership

- 提取器属于 gateway attempt 发送边界，只读取最终 wire body，不读取原始客户端 body，也不猜测模型默认能力。
- `upstream_sent` 必须在 transport 真正开始后置为 true；若当前代码已有等价发送证据，复用该证据而不是再建状态机。
- attempt 是权威数据。RequestLog 查询和实时 request finalization 使用同一选择函数，避免历史与实时口径漂移。
- attempts JSON 新字段使用 serde default 兼容 Beta 5 及更早记录；优先不增加数据库列。

## Frontend Ownership

- 使用共享 badge 组件承载文案、tooltip 和紧凑尺寸。
- 历史行与实时卡片消费标准化后的 `reasoning_effort`；详情摘要优先显示最终观测值。
- Codex `special_settings_json` 保留用于请求开始阶段或旧记录的兼容回退，但最终观测值出现后不得同时渲染第二个强度。

## Protocol Semantics

| Path / mode | Explicit source | Deliberately ignored |
| --- | --- | --- |
| `/responses` | `reasoning.effort` | 推断出的模型默认 effort |
| `/chat/completions` | `reasoning_effort` | 数值 budget |
| `/messages` | `output_config.effort` | `thinking.type`、`budget_tokens` |
| Gemini generateContent | `thinkingLevel` | `thinkingBudget`、countTokens |

未知但非空的显式字符串保持原样，以兼容未来 provider 值；UI 不维护封闭枚举。

## Compatibility And Failure Semantics

- 旧 attempt 缺少新字段时反序列化为 `None` / `false`，查询仍可用。
- 最后一个未发送的准备失败不能覆盖更早已经发送的 attempt。
- 成功 attempt 优先于后续仅用于诊断的记录；无成功时才采用最后已发送 attempt。
- 没有显式 effort 时不展示 badge。尤其 Claude 只有 enabled/adaptive thinking 或 token budget 时保持空值。
- 不改变 CX2CC 四槽模型映射、通用 provider eligibility 或思考参数透传。

## Validation

- 纯提取器表驱动测试。
- attempt 选择与旧 JSON 兼容测试。
- failover/request-log 查询、事件投影和 CX2CC final-wire 集成测试。
- React 组件及 adapter 测试覆盖最终值优先、Codex 回退与无重复。
- bindings check、前后端定向测试和完整 pre-push gate。
