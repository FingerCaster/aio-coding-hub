# 供应商界面支持导入导出单个供应商

## Goal

让用户能够从供应商界面安全地分享、备份和迁移单个供应商的完整配置与凭据，并在目标环境中以禁用的新供应商导入，不影响任何既有供应商或其他应用配置。

## Background

- 供应商卡片已有“测试、复制、删除”等次级操作区，页面右上角已有刷新与添加操作区，分别适合放置“分享”和“导入”入口（`src/pages/providers/SortableProviderCard.tsx:561`、`src/pages/providers/ProvidersView.tsx:227`）。
- 前端 `ProviderSummary` 只暴露 `api_key_configured`，不暴露明文 API Key 或 OAuth token；完整导出必须由 Rust 后端读取凭据并直接写入剪贴板或用户授权文件，不能把明文导出内容返回 React（`src-tauri/src/domain/providers/types.rs:285`、`src-tauri/src/domain/providers/types.rs:319`）。
- 现有全局配置导出包含 API Key、OAuth access/refresh/ID token 与 client secret，但现有全局导入会清空供应商及其他配置表，不能复用为单供应商导入（`src-tauri/src/infra/config_migrate/mod.rs:44`、`src-tauri/src/infra/config_migrate/mod.rs:65`、`src-tauri/src/infra/config_migrate/mod.rs:454`、`src-tauri/src/infra/config_migrate/import.rs:659`）。
- 现有供应商复制覆盖基础配置、模型映射、限额、转译关系、流空闲超时、上游重试覆盖及插件扩展值，是“完整供应商配置”字段集的重要依据（`src-tauri/src/app/provider_service.rs:263`）。
- 引用型转译供应商依赖目标机器上的具体源供应商 ID、CLI 和启用状态，无法作为独立单供应商安全迁移；无 `source_provider_id` 的 Claude `cx2cc` 则只依赖当前 AIO Codex 网关，可以独立迁移（`src-tauri/src/domain/providers/queries.rs:1123`、`src/pages/providers/providerEditorSubmitModel.ts:96`、`src/pages/providers/providerEditorSubmitModel.ts:174`）。
- 新供应商不会自动进入默认路由，但启用的 OAuth 供应商会被后台刷新，启用且配置账户用量扩展的卡片也会自动请求远端，因此导入项必须强制禁用（`src-tauri/src/domain/providers/queries.rs:718`、`src-tauri/src/domain/providers/queries.rs:1310`、`src-tauri/src/domain/providers/queries.rs:2091`、`src/query/providers.ts:467`）。
- 插件扩展值以 `plugin_id + namespace` 归属于插件；内置账户用量扩展可自动补齐内部拥有者，其他扩展在目标环境可能缺少对应插件（`src-tauri/src/domain/providers/queries.rs:149`、`src-tauri/src/domain/provider_account_usage.rs:192`）。
- `supported_models_json`、本机 ID、时间戳、路由、熔断、用量窗口和日志属于可重建或运行时状态，不应进入单供应商分享文件（`src-tauri/src/domain/providers/queries.rs:1560`、`src-tauri/src/domain/providers/types.rs:306`）。

## Requirements

### R1. 入口与交互

- 每张供应商卡片提供“分享”操作；供应商页面右上角提供“导入”操作。
- 每次打开分享操作都明确警告文件包含完整 API Key、OAuth token 与 client secret，并提供“复制内容”和“保存到本地”；取消不改动剪贴板或文件。
- 导入同时支持选择 JSON 文件和粘贴 JSON 内容，两种入口共用同一解析、预览、校验、冲突处理和落库逻辑。
- 文件与内容导入都采用“校验预览 -> 用户确认 -> 写入”流程；未确认时数据库保持不变。

### R2. 导出安全与输出

- 复制和保存必须调用同一序列化器，产生字节完全一致的版本化 JSON。
- 明文分享 JSON 不得出现在前端状态、IPC 返回值、日志或错误信息中；保存由后端打开保存对话框并原子写入，复制由后端直接写入系统剪贴板。
- 复制成功后 60 秒，后端仅在剪贴板仍等于本次分享 JSON 时清空；用户已复制其他内容时不得修改，并在成功反馈中说明此行为。
- 默认文件名为 `aio-coding-hub-provider-<CLI>-<清理后的供应商名称>.json`，不含时间戳；保留可读 Unicode，替换跨平台非法字符，清理尾部点/空格并限制长度，空名称回退为 `provider`，保存对话框允许修改。

### R3. v1 分享契约

- 顶层固定类型标识为 `aio-coding-hub.provider-share`，并包含严格的 `schema_version: 1`。
- v1 的顶层与所有受控嵌套对象都拒绝未知字段、未知枚举、未知类型和未知/未来版本；插件扩展的 `values` 是插件拥有的开放 JSON 值。
- 后续演进必须增加显式版本读取器并保留仍受支持的旧版本，不以忽略未知字段模拟兼容。
- 文件只包含被选中的一个供应商。
- 文件包含全部可迁移配置：CLI、名称、来源启用状态、优先级、Base URL 与模式、认证模式和完整凭据、Claude 模型、Codex 模型映射与可用性测试模型、成本倍率、各周期限额与重置规则、标签、备注、转译类型、流空闲超时、上游重试覆盖和全部插件扩展值。
- OAuth 凭据包括 provider type、access/refresh/ID token、token URI、client ID/secret、过期时间、邮箱和刷新提前量。
- 文件排除本机或运行状态：数据库 ID、创建/更新时间、展示顺序、默认路由与排序模板关系、熔断状态、用量窗口、请求日志、模型发现缓存、OAuth 最近刷新时间及瞬时错误/诊断。

### R4. 严格、脱敏的导入预览

- 文件与文本采用相同的 UTF-8、JSON、结构和 8 MiB 编码大小上限，任何错误都在写入前失败。
- 预览只返回 CLI、原名称、预计最终名称、来源启用状态、实际导入状态、认证类型与可用性、扩展数量及逐项兼容结果等脱敏元数据。
- 预览不得返回 API Key、OAuth token/client secret、原始 JSON、完整 Base URL 或本机文件路径。
- 后端为准确内容保存短期、单次使用的不透明预览令牌；内容修改、文件修改、令牌过期，或影响预计名称/插件兼容性的目标状态变化后，确认必须失败并要求重新预览。
- 预览界面提示分享内容含有凭据，只应导入可信来源；确认导入使用服务端风险确认载荷。

### R5. 新增语义与导入结果

- 导入只新增一个供应商，不清空、不覆盖或修改其他供应商、工作区、提示词、MCP、Skill、路由、排序模板或应用设置。
- 同一 CLI 下名称冲突时，按现有规则依次选择“名称 副本”“名称 副本 2”等；比较忽略首尾空白和大小写，绝不覆盖原供应商。
- 分享文件保留来源 `enabled` 供预览，但导入始终以禁用状态新增，且不加入任何默认路由或排序模板。
- 导入成功后自动切换到文件所属 CLI，刷新该 CLI 的供应商列表，并显示实际最终名称。
- 整个数据库写入必须在一个事务内完成；凭据、基础字段和扩展值不能部分落库。

### R6. 转译供应商边界

- `source_provider_id != null` 的引用型转译供应商禁止分享：前端入口禁用并解释原因，后端复制与保存命令也必须拒绝且不得写入输出。
- 分享契约不包含外部供应商 ID；导入端拒绝手工构造的外部引用或任何必须依赖源供应商的转译类型。
- `source_provider_id` 为空、使用当前 AIO Codex 网关的 Claude `cx2cc` 供应商允许正常导入导出。

### R7. 插件扩展完整性

- 分享文件包含每项扩展的 `plugin_id`、插件版本、namespace 和完整 `values`，并保持确定性顺序。
- 非内置扩展导入要求目标环境存在可用插件、版本与分享文件精确一致，且目标 manifest 仍声明对应 namespace；缺失、不兼容、隔离或未安装时整体失败并给出非敏感问题列表。
- 在插件扩展值尚无独立 schema 版本声明前，不能用宽松 semver 推断兼容。
- 内置 `core.provider-account-usage` 沿用现有机制自动补齐内部拥有者，不要求用户安装可见插件。

### R8. 不完整凭据的可恢复性

- OAuth access token 缺失或过期时仍允许禁用导入；预览标明“可用”“可刷新”或“需要重新登录”，导入期间不发起任何远端请求。
- 直接 API Key 为空时仍允许禁用导入；预览标明“需要填写 API Key”，用户之后可编辑补充并手动启用。

### R9. 敏感数据生命周期

- 内容导入不得记录原文；服务层诊断参数只记录字节数等非敏感信息。
- 前端在用户修改已预览内容时立即使预览失效；取消、成功或失败关闭后清空文本输入并请求后端丢弃预览令牌。
- 后端预览缓存有 TTL、数量和总字节上限，确认或丢弃后立即释放保存的凭据。

## Acceptance Criteria

- [ ] AC1: 卡片“分享”和页面右上角“导入”入口可用，引用型转译卡片的分享入口禁用且有原因说明。
- [ ] AC2: 分享警告后可复制或保存；两种方式对同一数据库快照生成完全相同的严格 v1 JSON，取消无副作用。
- [ ] AC3: 导出仅含一个供应商，完整配置、凭据和插件扩展可由当前版本往返恢复，运行时字段不出现。
- [ ] AC4: 明文导出 JSON 和凭据不经过前端、IPC 返回、日志或错误；保存为后端原子写入。
- [ ] AC5: 剪贴板在 60 秒后仅按内容一致条件清空，不覆盖用户后来复制的内容。
- [ ] AC6: 文件名在 Windows、macOS、Linux 上有效，含 CLI 和可识别名称且无时间戳。
- [ ] AC7: 文件与粘贴内容共享同一 8 MiB 有界严格解析器；非法 UTF-8/JSON、未知字段、未知类型和未知版本均在写入前确定性失败。
- [ ] AC8: 两种入口都先显示无凭据预览；未确认、令牌过期、内容/文件或相关目标状态变化时不写入数据库。
- [ ] AC9: 同 CLI 同名自动使用确定性副本名称，原供应商和所有其他应用数据保持不变。
- [ ] AC10: 导入结果始终禁用且不加入路由/模板；成功后切换到对应 CLI 并显示最终名称。
- [ ] AC11: 前后端都阻止导出引用其他供应商的转译项，导入拒绝外部引用；独立 `cx2cc` 可往返。
- [ ] AC12: 扩展值完整往返；非内置插件缺失、版本不匹配或 namespace 不存在时事务前失败，内置账户用量拥有者可自动补齐。
- [ ] AC13: OAuth 凭据失效或 API Key 为空时仍可禁用导入，预览给出非敏感修复提示，且导入不联网。
- [ ] AC14: 预览令牌单次使用且有界；取消、成功和失败关闭后不继续保留文本或预览凭据。
- [ ] AC15: Rust 单元/集成测试、前端服务/组件测试、生成绑定检查、格式化、lint 和 typecheck 全部通过。

## Out of Scope

- 改变现有全局配置导入的整份快照替换语义。
- 修复与本功能无关的全局配置迁移缺陷。
- 递归打包、自动创建或跨环境匹配转译供应商引用的源供应商。
- 打包、安装或升级第三方插件；分享文件只携带扩展值与兼容性要求。
