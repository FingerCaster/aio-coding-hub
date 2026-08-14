# Claude 思考强度调用日志

## Goal

适配 upstream 通用 reasoning_effort 日志链路，在 Claude/CX2CC 实时与历史调用日志显示最终实际发送的思考强度，并保持 Codex 现有展示兼容。

## Requirements

- 在最终 outbound path/body 已完成协议桥接、模型映射和请求设置后，提取显式的字符串思考强度：
  - Responses API：`reasoning.effort`
  - OpenAI Chat Completions：`reasoning_effort`
  - Anthropic Messages：`output_config.effort`
  - Gemini：`generationConfig.thinkingConfig.thinkingLevel`，并兼容网关包装后的 `request` body
- 提取器只接受 trim 后非空字符串，并原样保留未来新增值；不得根据 `thinking.type`、`budget_tokens`、数值 thinking budget 或模型默认值推断强度。
- 每次 provider attempt 记录该次真正准备发送的强度以及是否已经触达 upstream transport。准备阶段失败、路由跳过和其他未发送 attempt 不得被当作用户可见强度来源。
- 请求最终强度选择规则固定为：最后一次成功 attempt 优先，否则最后一次实际发往 upstream 的 attempt；旧日志和缺少字段的 attempt 必须兼容为 `null`。
- 同一最终强度必须贯通实时事件、历史 RequestLog summary/detail、生成 TypeScript 绑定和前端展示，不新增重复或相互冲突的数据来源。
- Claude 直连、CX2CC、Codex 使用同一观测链路。CX2CC 必须观察桥接后 Responses body 中实际发送的值；不得增加第二层模型路由或改变 CX2CC 思考透传语义。
- 历史日志列表、实时 trace 卡片和详情摘要显示统一的思考强度 badge。最终观测值可用时以它为准；Codex 现有 special settings 只作为尚无最终观测值时的兼容回退，且页面不得出现重复 badge。
- 不持久化完整请求 body 或敏感信息；该功能仅记录一个有界的可选字符串。优先复用 attempts JSON，除非现有架构明确要求 schema 迁移。
- 本任务只适配 upstream `6007d7a0` 的 reasoning-effort 日志语义，不顺带移植无关 upstream 功能或修复无关缺陷。

## Acceptance Criteria

- [x] Rust 单元测试覆盖四类协议、空值/非字符串/数值预算、未知未来字符串与 Gemini 包装 body。
- [x] attempt 测试证明成功优先、最后已发送失败回退，以及未发送 attempt 不污染最终值。
- [x] Claude `/messages`、CX2CC `/responses` 和 Codex 回归测试均能得到最终实际 outbound effort；只有 thinking 开关或预算时保持无 badge。
- [x] RequestLog summary/detail 与实时事件均包含可选 `reasoning_effort`，旧 attempts JSON 可正常读取。
- [x] 历史列表、实时 trace、详情摘要展示一致且无重复，Codex 既有模型/强度兼容展示不回归。
- [x] 生成绑定已更新，前端 lint/typecheck/test 与 Rust fmt/clippy/test 通过。
- [x] 独立检查代理完成跨层数据流核对，发布前完整 `pnpm check:prepush` 通过。

## Notes

- upstream 作为语义证据，fork 当前架构决定具体落点；不进行整提交 cherry-pick。
- UI 文案沿用现有“思考”展示，不把内部协议字段暴露给用户。
