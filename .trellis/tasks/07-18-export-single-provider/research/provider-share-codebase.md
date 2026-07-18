# 单供应商分享代码调研

## UI 与前端数据流

- `src/pages/providers/ProvidersView.tsx:227` 是供应商页右上角操作区；`ProvidersViewProps` 已包含 `setActiveCli`，可在导入后切换到文件所属 CLI。
- `src/pages/providers/SortableProviderCard.tsx:561` 是卡片次级操作区；组件已接受动作回调与 loading 状态，适合增加 `Share2` 图标的“分享”按钮。
- `src/query/providers.ts:241` 与 `src/services/providers/providers.ts:244` 展示现有 Service -> TanStack Query -> View 分层。新增分享 IPC 应继续由 service 负责参数验证/日志脱敏，由 query mutation 负责缓存失效。
- `src/ui/Dialog.tsx`、`src/ui/Textarea.tsx`、`src/ui/Button.tsx` 可组成导入/分享对话框；项目未提供通用 Tabs，文件/内容模式可使用带 `aria-pressed` 的紧凑分段按钮。
- `src/pages/providers/ProvidersView.tsx:37` 当前未解构 `setActiveCli`，实现导入后切换时需要补上。

## 凭据与供应商字段

- `src-tauri/src/domain/providers/types.rs:285` 的 `ProviderSummary` 只含认证摘要；明文 API Key/OAuth token 不能从卡片 DTO 拼装。
- `src-tauri/src/infra/config_migrate/export.rs:44` 的全局导出 SQL可确认现有 OAuth 存储列，但它还包含模型缓存/瞬时状态且不含全部新字段，不能直接作为单供应商格式。
- `src-tauri/src/app/provider_service.rs:263` 的复制路径覆盖基础字段、模型、限额、标签、备注、转译字段、流超时、重试覆盖和扩展值，可用于核对分享字段完整性。
- `src-tauri/src/domain/providers/queries.rs:1050` 的普通 upsert 创建路径在事务内写基础字段和扩展值，但直接 API Key 为空时会拒绝，且不接受完整 OAuth 凭据；单供应商导入需要专用的事务写入函数，同时复用同模块验证器。
- `src-tauri/src/domain/providers/validation.rs` 已集中管理 CLI、URL、限额、备注、重置时间、模型和流超时边界，分享导入应调用这些函数而非复制规则。

## 引用、路由与网络副作用

- `src-tauri/src/domain/providers/queries.rs:1123` 要求引用型转译项的源供应商存在、启用、CLI 匹配且自身不是转译项；本机 ID 无法跨环境安全恢复。
- `src-tauri/src/domain/providers/queries.rs:1203` 说明 Codex 转译类型必须有 `source_provider_id`，因此单供应商导入只能接受无需外部来源的 `cx2cc`。
- `src/pages/providers/providerEditorSubmitModel.ts:96` 与 `:174` 允许 Claude `cx2cc` 在 `source_provider_id = null` 时跟随当前 AIO Codex 网关。
- `src-tauri/src/domain/providers/queries.rs:718`、`:1310` 说明新建供应商不会自动进入默认路由；但 `:2091` 的 OAuth 刷新查询和 `src/query/providers.ts:467` 的账户用量查询会对启用项产生网络请求，所以导入必须强制 `enabled = false`。

## 插件扩展

- `src-tauri/src/infra/db/migrations/v34_to_v35.rs:12` 定义扩展值主键为 `(provider_id, plugin_id, namespace)`，并通过外键归属于插件。
- `src-tauri/src/domain/plugin_contributions.rs:63` 的 provider contribution 声明 `extensionNamespace`，可用于确认目标插件仍拥有对应 namespace。
- `src-tauri/src/domain/plugins.rs:15` 与 `src-tauri/src/infra/plugins/repository.rs:83` 提供 manifest、当前版本和状态证据；当前扩展值没有独立 schema version，因此 v1 以插件版本精确一致作为非内置扩展兼容门槛。
- `src-tauri/src/domain/provider_account_usage.rs:203` 已能在同一事务中自动补齐 `core.provider-account-usage` 内部拥有者，应直接复用。

## 文件、剪贴板与 IPC

- `src-tauri/src/commands/image_gen.rs:157` 展示“后端打开保存对话框 -> 后端写入”的现有安全模式。
- `src-tauri/src/shared/fs.rs:310` 和 `:387` 提供无跟随的有界读取与原子写入；错误映射需要隐藏本机路径。
- `tauri-plugin-clipboard-manager` 的 Rust API提供 `write_text`、`read_text` 与 `clear`；其文档警告 `read_text` 不能在主线程执行，60 秒条件清理应放到阻塞任务。
- `src-tauri/src/shared/ipc_confirm.rs` 与 `src/services/ipcConfirm.ts` 提供服务端可验证的风险确认载荷，分享导出和确认导入都应使用。
- `src/services/generatedIpc.ts` 会记录调用诊断参数；粘贴内容必须只把 `[REDACTED]` 与字节长度传给该层，真实文本只进入生成命令调用闭包。

## 设计结论

1. 新增严格 `ProviderShareEnvelopeV1`，受控对象全部 `deny_unknown_fields`，插件 `values` 保持开放 JSON。
2. 导出、解析、预览和落库由 Rust 持有；前端只接触用户主动粘贴的文本和脱敏预览。
3. 后端管理短期、单次预览令牌，缓存解析后的凭据；文件确认时重读并比较 SHA-256，事务内重算名称和插件兼容性。
4. 单次文件/内容上限 8 MiB；预览缓存同时限制 TTL、条目数和估算总字节。
5. 保存复用原子写入，复制后异步条件清空；所有成功返回均为布尔值或脱敏结果。
