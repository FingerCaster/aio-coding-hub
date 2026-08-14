# Upstream Reasoning Effort Log Evidence

## Compared Revisions

- Fork base / Beta 5 source: `08205248bf245fa42deab9e1a968678a94436e1d`
- Upstream evidence commit: `6007d7a0`（包含于核对时的 `upstream/main`）

## Confirmed Upstream Semantics

- upstream 在最终 outbound body/path 上按协议读取显式 effort，而不是从输入设置或模型名推断。
- upstream attempt 同时保存 `reasoning_effort` 与 `upstream_sent`。
- 请求最终 effort 选择最后成功 attempt；无成功时选择最后实际发送的 attempt。未发送的路由/准备失败不能覆盖它。
- RequestLog summary/detail、实时 request/attempt events 和前端 badge 使用该统一字段。
- Claude Messages 读取 `output_config.effort`；CX2CC 桥接后的 Responses 请求读取 `reasoning.effort`，因此无需 CX2CC 专用日志分支。

## Fork Gap At Beta 5

- Rust RequestLog / attempt / event 链路没有通用 `reasoning_effort` 字段。
- 前端仅从 Codex `special_settings_json.codex_reasoning_effort` 推导详情展示。
- Claude 与 CX2CC 的历史列表和实时 trace 因此看不到最终实际发送的思考强度。

## Adaptation Boundary

- 只适配 reasoning-effort 可观测性，不 cherry-pick `6007d7a0` 的其他价格、目录、选择或编辑器变化。
- 不采用 thinking-effort rectifier 作为透传证据；它是收到特定 400 后移除冲突字段的独立兼容机制。
- 不改变 CX2CC 模型映射或思考透传，本任务只观察最终 wire request。
