# Implementation Plan

1. 在 gateway 最终发送边界增加协议感知的 effort 提取器，并把 effort / upstream-sent 证据写入 attempt 与实时 attempt event。
2. 建立复用的最终 effort 选择语义，贯通 request final event、attempts JSON、RequestLog summary/detail 查询及生成绑定。
3. 将前端日志 adapter、历史列表、实时 trace 和详情摘要接入统一 badge，同时保留 Codex 旧记录回退且避免重复。
4. 补齐 Claude、CX2CC、Codex、旧日志兼容与 UI 测试；更新跨层规范并运行完整质量门禁。
5. 创建 origin PR、等待 CI、合并 main，按 immutable merge SHA 发布 `aio-coding-hub-v0.60.41-beta.6` 并验证资产、更新通道和稳定版不变。
