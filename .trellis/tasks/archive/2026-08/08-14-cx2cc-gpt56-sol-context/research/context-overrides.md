# CX2CC 自定义上下文设计核对

CX2CC 的可信模型能力身份是 `(provider_id, provider_uuid, remote_model_id)`，同名模型
在不同来源 Provider 上可能有不同容量。全局裸 model-ID map 会抹掉这个身份并绕过
既有 Mixed/Unknown 保护，因此不采用。

上下文与 CX2CC Provider 的四个真实 mapper 槽位放在同一个 `ClaudeModels` JSON：
main/haiku/sonnet/opus 各有 `Option<u64>`，不为未使用的 reasoning_model 增加字段。
context 必须伴随同槽显式 model，且普通 Claude Provider 后端拒绝。这样配置随
Provider create/update/duplicate/share/config bundle 保持同一生命周期。

terminal 不能先按 model ID 去重，而应逐槽解析：有显式 context 则使用；否则查
provider-scoped catalog。任一槽 Unknown 则整体 Unknown，全部已知但不同取最小值。
一个 Claude 进程只有一个 MAX_CONTEXT/AUTO_COMPACT 值，因此最终注入同一个保守最小
窗口，不改变 family aliases，也不写 `ANTHROPIC_MODEL`。

