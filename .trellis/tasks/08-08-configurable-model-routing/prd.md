# 可配置模型路由融合调研

## Goal

评估并规划将 KNaiFen/aio-coding-hub 的可配置模型路由能力融合到本项目当前网关的可行路径，形成可供后续实现使用的、代码库有证据支撑的范围与依赖结论。

本任务当前只做调研和规划材料，不修改产品代码、不 cherry-pick 外部提交、不启动实现阶段。

## Background

- 目标能力来自 [commit e2a29962](https://github.com/KNaiFen/aio-coding-hub/commit/e2a29962)，其最终合入上下文是 [PR #14](https://github.com/KNaiFen/aio-coding-hub/pull/14)。原提交之后仍有原生构建和数据库迁移幂等性修复，因此不能把原始提交视为最终交付物。
- 本次规划快照的 `main` HEAD 为 `534f878d`；它相对 `8757d32c` 只新增 Trellis journal。最后产品行为基线仍为 `8757d32c`，其中包含会话 `019fda50-f0a6-7650-adb2-6b5d2457ebb5` 的产品提交 `708d2965`、`ef8892be` 及后续的重复余额回切抑制修复。对应工作树 `D:/OrcaProjects/aio-coding-hub-fork/account-usage-routing-build` 已干净且仍指向 `c2e6b065`，因此实现以启动时最新 `main` 为准，工作树只用于追溯原会话边界。
- 该会话及后续修复新增了账户用量运行时、余额门控和基于 `blocked_provider_ids` 的已知 Blocked 高优先级回切抑制，覆盖 Provider 查询/分享/导入导出、共同发送前 gate、failback/session 和 Provider 编辑器，现已成为模型路由的直接实现基线。当前主工作树存在与本调研无关的未提交改动，实现前必须按实际 dirty paths 重新审计并完整保留。
- `trellis mem` 无法读取给定会话 ID（当前构建不支持 OpenCode 1.2+ SQLite 会话）；本次使用 Orca 终端只读输出和 Git 对象核对会话改动，不把缺失的历史对话当作不存在。

## Deliverables

- `08-08-remove-codex-provider-translation`：独立删除 Codex Provider 转译、迁移旧记录并验证普通 Codex/Claude CX2CC 回归。
- `08-08-configurable-model-routing-core`：在删除子任务完成后，以启动时包含账户路由产品基线的最新 `main` 独立实现可配置模型路由。
- 父任务只维护共同需求、依赖和最终联合验收，不直接承载产品代码；子任务的先后依赖必须同时写入各自制品。

## Requirements

### R1. 还原目标能力契约

记录全局策略、Provider 级继承/覆盖/禁用、精确模型匹配、reasoning effort 重写、协议端点范围、managed `aio/` 排除、日志与计价语义，并区分原始提交与后续修复。

### R2. 对齐账户路由最终基线

以当前 `main` 已完成的账户用量/余额门控及回切抑制代码为交叉基线，列出共享文件、数据结构、生命周期和测试的实现顺序；不得退回会话工作树的旧 HEAD，也不得依赖任何未提交内容。

### R3. 形成当前仓库的实现边界

明确哪些逻辑可以复用，哪些必须按当前请求体压缩、插件、故障转移和配置迁移架构重写，哪些目标分支功能（通知、Observer/TUI、版本发布）暂不纳入 MVP。

### R4. 定义持久化和兼容策略

当前删除子任务使用 SQLite 43->44 清理旧 Codex bridge；随后模型路由子任务使用 SQLite 44->45 新增 Provider policy override，settings 56->57 新增全局 policy。迁移必须幂等；Provider 分享使用严格 v3 承载 override，v1/v2 只作兼容读取；完整配置继续使用 v4 并通过功能最低版本门槛承载全局和 Provider 策略。旧配置默认禁用全局策略并由 Provider 继承。实现开始前仍需复查版本未被其他改动占用。

### R5. 定义跨层验收矩阵

提出覆盖网关协议、Provider 级策略、插件顺序、故障转移、managed/辅助请求排除、压缩请求、日志计价、UI 和迁移的自动化测试范围，并标明目标提交已有测试缺口。

### R6. 路由改写失败采用 Provider 级失败关闭

规则已经匹配，但当前 Provider 的最终模型或 reasoning effort 无法完整写入 wire request 时，不向该 Provider 发送请求，记录稳定、可审计的发送前路由失败，并继续同一用户请求的下一候选 Provider。该失败没有上游调用，不得改变 Provider 健康、熔断、余额门控或 transport retry 状态；下一 Provider 必须按自己的 override/global policy 重新解析和改写。只有所有候选都无法完成路由时，才向客户端返回明确错误；本功能不在响应后自动创建新的客户端请求。

### R7. MVP 只覆盖当前已有桌面端链路

本次融合只实现全局设置、Provider 覆盖设置、桌面请求日志和成本计算。目标提交依赖但当前仓库不存在的 Observer/TUI 投影不纳入本任务；通知和发布改动也继续排除。未来需要 Observer/TUI 时必须单独立项，不能作为本任务的隐式依赖。

### R8. Provider 专属策略完整替换全局策略

Provider 未设置 override 时继承全局策略；设置 enabled override 后只使用该 Provider 的完整规则集，不从全局补齐未匹配的 source model；设置 disabled override 时对该 Provider 完全关闭可配置模型路由。UI 必须明确显示“继承全局 / 使用专属规则 / 禁用路由”三种状态，避免隐藏继承。

### R9. 完整删除 Codex Provider 协议转译

删除 Codex Provider 的整个“转译”能力，而不只是其中的模型映射：移除桌面端“转译”标签页、Codex bridge 类型、来源 Provider 引用、上游端点选择、默认/精确模型映射、请求/响应及流式协议转换，以及相关活动配置分支。普通 Codex Provider 保留；Claude 的 CX2CC 协议转换不在删除范围。用户已确认通过升级迁移自动删除已有 Codex 转译 Provider，不能静默留下无法编辑或无法运行的记录。

### R10. 升级时删除旧 Codex 转译 Provider 并保留历史日志

SQLite 升级迁移自动删除 `codex_to_openai_chat`、`codex_to_openai_responses` 和 legacy `codex_to_anthropic_messages` Provider 记录；不尝试转换为普通 Codex Provider，不复制来源 Provider 的 URL 或凭据。依赖这些记录的活动路由/扩展配置按既有外键规则清理，历史请求日志必须保留。迁移不得删除普通 Codex Provider、来源 Provider 或 Claude CX2CC Provider，并且重复执行必须幂等。

### R11. 旧完整配置包必须原子拒绝

完整配置导入在清空当前配置前预检 Provider；若旧配置包包含任一已删除的 Codex bridge 类型，整包拒绝并返回稳定、明确的“不再支持 Codex 转译”错误，当前配置保持不变。不得静默跳过相关 Provider，也不得以成功结果造成部分恢复。

### R12. 损坏策略禁用受影响范围

无法解析或经防御性清洗后失效的全局策略按 disabled 处理；损坏的 Provider override 按显式 disabled override 处理，必须抑制全局策略而不是回退继承。两者均记录不含敏感数据的诊断并继续发送未改写请求。只有有效规则已经匹配、但最终 wire request 无法完整应用其目标值时，才执行 R6 的 Provider 级失败关闭。

## Acceptance Criteria

- [x] `research/feasibility.md` 包含可追溯的提交、分支和文件证据，并明确当前 `main` 与原会话工作树的基线差异及实现前复查步骤。
- [x] 调研明确“直接 cherry-pick”与“按当前架构移植”的差异、主要冲突层和推荐 MVP 边界。
- [x] 调研明确账户用量门控、回切抑制与模型路由的文件交叉点、依赖顺序和不变量，且不依赖旧工作树或未提交代码。
- [x] 调研给出 settings/SQLite 版本兼容方案及旧配置的安全默认值。
- [x] 调研给出可执行的跨层测试矩阵和回滚/重审条件。
- [x] 已确定路由改写采用 Provider 级失败关闭，并继续同一请求的下一候选 Provider。
- [x] 已确定 MVP 排除 Observer/TUI，只覆盖当前已有桌面设置、日志和成本链路。
- [x] 已确定 Provider 专属策略完整替换全局策略，不与全局规则叠加。
- [x] 已确定完整删除 Codex Provider 协议转译，且明确保留普通 Codex Provider 与 Claude CX2CC。
- [x] 已确定升级时自动删除旧 Codex 转译 Provider、清理活动引用并保留历史请求日志，且不得复制来源凭据或影响普通 Provider/CX2CC。
- [x] 已确定含旧 Codex 转译 Provider 的完整配置包在预检阶段整包拒绝，当前配置保持不变。
- [x] 已确定损坏策略只禁用受影响范围并继续原请求，且损坏 Provider override 不得意外继承全局。
- [x] 父任务及两个子任务均已形成可评审的 `prd.md`、`design.md` 和 `implement.md`，依赖与联合验收有明确所有者。
- [x] 本任务阶段没有产品源码变更；在用户批准前不运行 `task.py start`，不进入实现阶段。

## Out Of Scope

- 直接合并或 cherry-pick 外部提交。
- 账户用量、余额门控和回切抑制本身的实现；本文只记录其已合入基线、依赖和交叉约束。
- 目标提交中独立的任务通知、Observer/TUI、发布版本号和 CI/release 改动。
- 在本任务获得实现批准前修改 `src/`、`src-tauri/` 或生成绑定。
