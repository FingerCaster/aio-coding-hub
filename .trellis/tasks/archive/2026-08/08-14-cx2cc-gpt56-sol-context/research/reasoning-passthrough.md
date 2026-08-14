# CX2CC reasoning 透传核对

基线 `a62869c` 已实现以下链路：

- `inbound/anthropic.rs` 读取 `output_config.effort` 与 `thinking.type`。
- `IRReasoningConfig` 保留 Absent/Disabled/Enabled/Adaptive/Effort presence。
- `outbound/openai_responses.rs` 写入 Responses `reasoning.effort`。
- `apply_cx2cc_request_settings` 只写 service tier/store，不再读取 legacy 固定 effort。

精确语义为：显式 effort 原字符串透传；enabled/adaptive 无 effort 不造值；disabled
映射 none；budget-only 不推断等级。协议字段转换存在，effort 枚举换算不存在且不应
新增。现有 e2e/单测已覆盖核心矩阵，本任务只补未来值/非字符串边界并保持回归。

