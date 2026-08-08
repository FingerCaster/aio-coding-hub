# 删除 Codex Provider 转译

## Goal

完整移除已经不再需要的 Codex Provider 协议转译能力，同时安全清理已有转译配置，保留普通 Codex Provider、来源 Provider、Claude CX2CC 和历史请求日志。

## Background

- 本任务是父任务 `08-08-configurable-model-routing` 的第一个独立交付物。
- 删除对象为 `codex_to_openai_chat`、`codex_to_openai_responses` 和 legacy `codex_to_anthropic_messages` 三类 bridge；`cx2cc` 不在范围内。
- 账户用量会话 `019fda50-f0a6-7650-adb2-6b5d2457ebb5` 修改了 Provider 契约、编辑器、配置迁移和绑定，其产品改动及后续重复余额回切抑制修复已形成最后产品基线 `8757d32c`。本次规划 HEAD `534f878d` 在其上只增加 Trellis journal；原会话工作树仍停在 `c2e6b065`，不再作为实现 HEAD。
- 后续模型路由子任务依赖本任务完成，不能再依赖被删除的 Codex bridge 模型映射或转换链路。

## Requirements

### D1. 删除全部 Codex 转译入口和运行时

移除 Codex Provider 编辑器中的“转译”标签页及其表单状态，删除三类 Codex bridge 常量、校验/准备分支、模型映射和请求/响应（含流式）协议转换注册。不得误删普通 Codex Responses/compact 请求支持、插件协议桥接能力或 Claude CX2CC。

### D2. 安全迁移已有转译 Provider

通过幂等 SQLite 43->44 迁移删除三类 Codex bridge Provider 记录；不复制其来源 Provider 的 URL/凭据，不转换成普通 Codex Provider。依赖记录按数据库外键和显式清理规则删除，来源 Provider、普通 Provider、CX2CC 及历史 `request_logs` 必须保留。历史 Provider 名称继续从 `attempts_json` 快照读取，不要求为已删除 bridge 建立墓碑 Provider。

### D3. 移除持久化和交换格式中的 Codex 专属分支

Provider CRUD、复制、分享、完整配置导入导出、可用性探测、模型目录、用量统计和生成绑定不得再创建或解释三类 Codex bridge。活动 Provider DTO/runtime 不再暴露 `model_mapping`；SQLite 物理列和旧 Provider 分享 v1/v2/完整配置格式中的同名字段只可作为惰性兼容壳保留并统一清空或忽略，不能恢复运行时语义。通用 `source_provider_id` / `bridge_type` 结构仅在 CX2CC、插件 bridge 或其他仍受支持的所有者需要时保留。

### D4. 明确旧备份兼容行为

含三类 Codex bridge Provider 的旧完整配置包必须在清空当前配置前整包拒绝，返回稳定、明确的“不再支持 Codex 转译”错误并保持当前配置不变。不得静默跳过相关 Provider，也不得以成功结果造成部分恢复。

### D5. 依赖顺序

本任务以已经包含账户用量产品提交及后续回切抑制修复的 `8757d32c` 为最低基线；实现前须重新记录实际 HEAD、schema 版本和 dirty paths，并保留所有不属于本任务的改动。本任务验收完成后，`08-08-configurable-model-routing-core` 才能进入实现。

## Acceptance Criteria

- [ ] 桌面端不再显示或提交 Codex“转译”模式，生成绑定不再暴露 Codex 专属 bridge 选项。
- [ ] 网关不再注册或执行三类 Codex bridge 的请求、响应及流式转换；相关死代码和测试夹具已清理。
- [ ] 从含三类旧记录的数据库升级会删除这些 Provider 及活动引用，保留历史请求日志、来源 Provider、普通 Codex Provider 和 CX2CC；迁移重复执行结果一致。
- [ ] 分享、复制和完整配置导入导出不能重新创建已删除的 Codex 转译 Provider，旧备份行为有自动化测试和明确错误/结果语义。
- [ ] 普通 Codex Responses/compact、Claude CX2CC、插件 bridge、Provider 删除及账户用量 gate 的相关回归测试通过。
- [ ] 实现前重新记录最新 `main`、最后产品行为基线和原会话工作树 SHA，并更新任何新出现的重叠点。

## Out Of Scope

- 删除或重构 Claude CX2CC。
- 删除通用插件协议桥接框架。
- 将旧 Codex bridge 自动转换成普通 Provider 或复制来源凭据。
- 实现可配置模型路由；由依赖本任务的子任务负责。
