# 替换过时 Codex 实验功能并增加 Provider name

## Goal

Codex 设置页不再暴露当前 Codex 已忽略的实验开关。`OpenAI` / `aio` 的受管理 provider 身份改由 AIO Provider 区的 Provider name 显式切换，旧配置的当前生效身份保持不变。

## Background

Features 区现在有 `remote_compaction`、`fast_mode`、`responses_websockets_v2`、`multi_agent`。AIO 用精确布尔值 `[features].remote_compaction = true` 选择 provider key `OpenAI`，其他情况选择 `aio`（`src-tauri/src/infra/codex_config/provider_projection.rs:202`）。切换会弹出取消 / 仅更新配置 / 同步会话记录，并可能迁移历史里的 provider 身份。

2026-09-25 对照 OpenAI Codex `main` 的 `codex-rs/features/src/lib.rs` 与 config schema：

- `remote_compaction` 已不在 registry。`remote_compaction_v2` 是 `Removed`，解析时跳过。提交 `3dc1e2a5`（[#44255](https://github.com/openai/codex/commit/3dc1e2a58406dc69db5812539adfee7d89fa9ef7)）退役了该 toggle；支持的 provider 始终走 streamed remote compaction v2。
- `responses_websockets` 与 `responses_websockets_v2` 都是 `Removed`。
- `fast_mode` 是 `Stable`，`default_enabled: true`。`service_tier` 的现行值是 `default` / `priority` / `flex`；legacy `fast` 仍可用，但是独立字段。提交 `7abf2a3b` 要求显式 `flex` 在关闭 fast mode 时保留。
- `multi_agent` 仍是 `Feature::Collab` 的 canonical key，`Stable`，默认开启。`multi_agent_v2` 是另一个默认关闭的后端，不是替换。

用户确认：删除 `remote_compaction` 开关，不换成 `remote_compaction_v2`，身份切换完整搬到 Provider name。`fast_mode` 只写 `features.fast_mode`。

## Requirements

- R1：Features 删除 `remote_compaction` 和 `responses_websockets_v2`。结构化 patch 与状态不再包含这两个字段。其他设置保存不得因为重写 `[features]` 而删除用户 TOML 里已有的 `responses_websockets_v2`。
- R2：保留 `fast_mode` 与 `multi_agent`。`fast_mode` 只写 `[features].fast_mode`：开启写 `true`，关闭写 `false`，不设置、不清空 `service_tier`。开关只看 `features.fast_mode`；未设置时仍显示关闭，与当前 AIO 一致。不得因为 Codex `default_enabled: true` 把未设置显示成开启，也不得删除 key，否则 Codex 会按默认开启。`multi_agent` 写入语义不变，文案不再称它为实验性功能。
- R3：不新增 `multi_agent_v2`、`remote_compaction_v2` 或其他 Codex 实验开关。不改 Features 区以外的控件，包括已 `Removed` 的 `personality`。
- R4：AIO Provider 区第一项增加 Provider name。标签用 “Provider name”。控件是与现有枚举设置一致的二选一，只能选 `OpenAI` 或 `aio`，没有自定义输入。
- R5：Provider name 是新的身份写入入口，继续复用现有身份收敛、冲突失败关闭、路由 backup/live 投影，以及取消 / 仅更新配置 / 同步会话记录对话框。取消零写入。仅更新配置不扫历史。同步会话记录沿用现有 history sync。普通配置保存不改 provider 身份。
- R6：读取生效身份时，只要 `[features].remote_compaction` 仍是精确布尔值，就继续由它决定：`true` 为 `OpenAI`，`false` 为 `aio`。flag 不存在时，根 `model_provider` 精确为 `OpenAI` 才是 OpenAI，否则是 `aio`。这样旧配置的当前生效值不变。成功的 Provider name 保存写入对应 `model_provider`，删除 `remote_compaction`，之后不再写回该 flag。未改控件时，不重写第三方 `model_provider`。

## Acceptance Criteria

- [ ] AC1：Features 不再出现 `remote_compaction` 或 `responses_websockets_v2`。`fast_mode` 与 `multi_agent` 仍在，且 `multi_agent` 文案不含“实验性”。
- [ ] AC2：打开 `fast_mode` 只写入 `fast_mode = true`。关闭写入 `fast_mode = false`。两种操作都不修改已有 `service_tier = "flex"`、`"priority"` 或 `"fast"`。`features.fast_mode` 缺失时开关显示关闭。
- [ ] AC3：AIO Provider 区第一项是 Provider name，只能在 `OpenAI` 与 `aio` 间切换。取消零写入；仅更新配置切换身份且不扫历史；同步会话记录沿用现有 history sync。
- [ ] AC4：路由开启时切换 Provider name，活动投影改到目标 key，gateway URL 不写进用户 baseline。路由关闭后再打开，用户选择的 key 仍在。`aio` 与 `OpenAI` 冲突时，任何文件变更前失败。
- [ ] AC5：`remote_compaction = true` 且根 provider 还不是 `OpenAI` 时，界面显示 OpenAI。`remote_compaction = false` 时显示 aio，即使根 provider 已是 `OpenAI`。一次成功的 Provider name 保存后，配置不再保留 `remote_compaction`。
- [ ] AC6：不含 provider 身份变化的结构化补丁不重命名 provider，不触发 history sync，也不删除已有 `responses_websockets_v2`。根 `model_provider` 为其他名字且用户未改 Provider name 时，该值保持原样。

## Out of Scope

- 自定义 provider 名称，或 `OpenAI` / `aio` 以外的第三个选项。
- 新增 Codex 实验菜单里的其他 feature，包括 `multi_agent_v2`。
- 改变 history sync 的内存、进程检查或回滚语义。
- 批量清理无关的 removed feature key，或在无关保存时删除 `responses_websockets_v2`。
- Cx2cc 的 `service_tier`，以及 Features 区以外的控件，包括 `personality`。
- 重命名现有 `CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT` 错误码。
