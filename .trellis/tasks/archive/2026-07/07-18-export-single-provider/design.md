# 技术设计：单供应商分享与导入

## Architecture

### Rust domain

在 `src-tauri/src/domain/providers/share.rs` 建立分享格式和数据库边界：

- `ProviderShareEnvelopeV1` 及严格嵌套 DTO 是 JSON 契约的唯一所有者。
- `export_provider_share_v1` 从一个数据库快照读取供应商、凭据、扩展和插件版本。
- `parse_provider_share_v1` 执行 8 MiB、UTF-8、固定类型、显式版本和严格字段校验。
- `preview_provider_share_v1` 只投影脱敏信息，计算冲突名称并检查插件兼容性。
- `import_provider_share_v1` 在一个 SQLite 事务内重验预览并插入基础字段、凭据和扩展值。

该模块复用 `providers::validation` 的 URL、CLI、模型、限额、备注、重置时间和超时规则，并将 `queries.rs` 中仅需的重试序列化、扩展替换和下一个排序号助手收窄为 `pub(super)`。普通 CRUD 契约不改变。

### Rust application / commands

在 `src-tauri/src/app/provider_share_service.rs` 管理短期预览状态：

- 随机 256-bit 不透明 token。
- 10 分钟 TTL、最多 8 条、估算敏感载荷合计不超过 32 MiB。
- 每项保存解析后的 v1 DTO、内容哈希、预览时最终名称、插件快照；文件来源额外保存后端私有路径。
- 确认和显式丢弃均移除 token；超时和容量淘汰也释放凭据。

在 `src-tauri/src/commands/providers/share.rs` 暴露六个命令：

1. `provider_share_copy_to_clipboard(provider_id, confirm)`
2. `provider_share_save_to_file(provider_id, confirm)`
3. `provider_share_import_preview_from_file()`
4. `provider_share_import_preview_from_content(content)`
5. `provider_share_import_confirm(preview_token, confirm)`
6. `provider_share_import_preview_discard(preview_token)`

文件选择/保存均在命令内部完成。命令注册进入统一 registry 并通过 Specta 生成 TypeScript 绑定。

### Frontend

- `src/services/providers/providerShare.ts` 是唯一 IPC 适配器，验证 ID/token，构造风险确认，并对粘贴内容的诊断参数脱敏。
- `src/query/providerShare.ts` 提供导入确认 mutation，成功后失效目标 CLI 的供应商列表。
- `ProviderShareDialog.tsx` 显示凭据警告并执行复制/保存。
- `ProviderImportDialog.tsx` 提供文件/内容分段模式、脱敏预览、可信来源警告和确认导入。
- `ProvidersView.tsx` 持有两个对话框目标；导入成功后调用 `setActiveCli(result.cli_key)` 并显示最终名称。
- `SortableProviderCard.tsx` 增加 `Share2` 操作。`source_provider_id != null` 时按钮禁用并提供 tooltip/title，后端仍独立拒绝绕过调用。

## Versioned JSON Contract

```json
{
  "type": "aio-coding-hub.provider-share",
  "schema_version": 1,
  "provider": {
    "cli_key": "codex",
    "name": "Example",
    "enabled": true,
    "configuration": {
      "base_urls": ["https://example.invalid/v1"],
      "base_url_mode": "order",
      "priority": 100,
      "cost_multiplier": 1.0,
      "claude_models": {},
      "model_mapping": { "default_model": null, "exact": {} },
      "availability_test_model": null,
      "limits": {},
      "tags": [],
      "note": "",
      "bridge_type": null,
      "stream_idle_timeout_seconds": null,
      "upstream_retry_policy_override": null
    },
    "authentication": {
      "mode": "api_key",
      "api_key": "secret"
    },
    "extensions": []
  }
}
```

OAuth 使用同一 `authentication` tagged enum，字段为 provider type、access/refresh/ID token、token URI、client ID/secret、过期时间、邮箱和刷新提前量。扩展项为 `plugin_id`、`plugin_version`、`namespace`、`values`。序列化使用结构体字段顺序、扩展排序、pretty JSON 和结尾换行，复制与保存共享同一字节函数。

受控结构全部拒绝未知字段；`values` 明确是插件拥有的开放 JSON。解析先用只含 `type`/`schema_version` 的窄头部判别，再选择 v1 读取器，便于未来增加 v2 而不放宽 v1。

## Export Flow

```text
卡片分享 -> 风险警告 -> 复制或保存命令
          -> DB 单快照读取 -> 拒绝 source_provider_id
          -> 严格 DTO -> 8 MiB 有界序列化
          -> 剪贴板写入 / 后端保存对话框 + 原子写入
```

- API Key/OAuth 凭据仅存在于 Rust 内存、剪贴板或目标文件。
- 保存取消返回 `false`；不创建文件。
- 剪贴板清理任务等待 60 秒，在 blocking 线程读取；只有内容字节一致时调用 `clear`。读取/清理失败只记录不含内容的状态。
- 默认文件名按 Unicode 字符处理，替换控制字符和 `<>:"/\\|?*`，去掉尾部点/空格，并为扩展名预留长度。

## Preview And Binding

```text
文件选择/文本 -> 有界读取 -> 严格解析 -> 配置验证
              -> 名称冲突计算 + 插件检查 + 认证状态投影
              -> 后端缓存敏感 DTO -> 返回 token + 脱敏预览
```

预览返回：token、CLI、原名、预计最终名、来源启用/实际禁用、认证模式/状态、扩展兼容列表和修复提示。它不返回路径、URL 或任何凭据。

文本变化时 UI 立即丢弃 token。文件确认时后端重新无跟随读取并比较 SHA-256；确认事务重新计算最终名称并重新查询插件，任何差异返回 `PROVIDER_SHARE_PREVIEW_STALE`。token 是单次使用，失败也要求重新预览。

## Atomic Import

确认顺序：

1. 验证服务端风险确认，取出单次 token。
2. 文件来源重读并核对哈希。
3. 开启 SQLite transaction。
4. 重算同 CLI 可用名称，必须等于预览名称。
5. 重验所有非内置插件的状态、精确版本和 namespace；按现有函数补齐内置账户用量拥有者。
6. 使用现有 provider 验证器规范化字段；拒绝任何外部来源转译。
7. 插入 provider，强制 `enabled = 0`，分配末尾 `sort_order`，写完整 OAuth/API Key 字段。
8. 写扩展值并提交；提交后读取 `ProviderSummary` 作为脱敏结果。

不写 `default_route_providers`、`sort_mode_providers` 或运行状态表，因此不会进入调用链。导入路径不调用 OAuth 刷新、账户查询或其他网络适配器。

## Plugin Compatibility

- `core.provider-account-usage`：忽略文件中的安装状态，通过既有事务 helper 创建内部拥有者，然后写值。
- 其他插件：目标必须有可安装状态（installed/enabled/disabled/update_available）、`current_version` 精确等于文件版本，manifest 能严格解析，且 provider contribution 中存在相同 `extensionNamespace`。
- available、uninstalled、incompatible、quarantined、缺失、版本不同或 namespace 不存在均阻断整个导入。
- 采用精确版本是因为当前 extension values 没有自己的 schema/version；未来若插件协议增加声明，可在新分享 schema 中放宽。

## Authentication Status

- API Key 非空：`configured`；空：`needs_api_key`。
- 独立 `cx2cc`：`not_required`。
- OAuth access token 非空且未过期：`available`。
- access token 不可用但 refresh token、token URI 和 client ID 齐备：`refreshable`。
- 其他 OAuth：`needs_login`。

这些状态只基于本地字段，不解析 token 内容、不联网，也不把具体过期时间返回 UI。

## Failure And Rollback

- 所有解析、字段校验、名称和插件检查在 commit 前完成；SQLite 错误自动 rollback。
- 保存使用同目录临时文件 + fsync + 原子替换；序列化超限或写入失败不覆盖既有目标。
- 错误只包含稳定错误码和字段/插件标识，不包含凭据、原 JSON、完整 URL 或文件路径。
- 本功能无需数据库 migration；回滚代码即可恢复旧 UI/IPC，已导入供应商只是普通禁用供应商。

## Verification Strategy

- Rust：严格 schema negatives、8 MiB 边界、确定性序列化/文件名、全字段往返、引用拒绝、空密钥/过期 OAuth、插件兼容、名称预览失效、事务原子性、文件哈希变化、预览 token TTL/单次/容量、剪贴板条件判断纯函数。
- Frontend service：真实参数与脱敏日志参数分离、风险确认资源、预览/result decoder。
- React：入口位置、引用型禁用、分享警告、文件/内容模式、内容变化失效、脱敏预览、确认后 CLI 切换和输入清理。
- 全量门禁：生成绑定、Rust fmt/check/tests、前端 typecheck/lint/focused tests。
