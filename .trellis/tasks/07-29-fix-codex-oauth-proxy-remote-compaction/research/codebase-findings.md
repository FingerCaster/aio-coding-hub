# Codex OAuth 代理与 remote compaction 代码调研

## 结论

本次问题不是单一开关组件故障，而是三条契约没有使用同一份状态定义：

1. OAuth 设置保存会同步已启用的 CLI proxy，但同步失败只折叠为 `false` 并记录日志，设置命令仍返回成功。
2. 前端在设置命令结束后又同步等待完整 Codex 刷新，其中模型目录子进程有 20 秒超时；该刷新与认证模式变化无关。
3. `remote_compaction` 的结构化补丁要求 provider 为 `OpenAI`，但 CLI proxy 的写入、恢复和状态检测各自维护 provider 规则；同时 provider 重命名没有目标冲突预检。

因此修复必须同时统一 provider 投影、配置所有权事务和前端缓存刷新，不能只缩短一个 timeout 或隐藏“修复”按钮。

## OAuth 开关链路

### 前端

- `src/pages/cli-manager/useCliManagerPageDataModel.ts:378` 的 `persistCommonSettings` 调用 settings mutation，成功后只返回 `updated.settings`，没有处理 `updated.runtime.cli_proxy_synced`。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts:440` 的 `refreshCodex()` 会刷新配置、原始 TOML、CLI 信息，并继续等待模型目录刷新。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts:501` 的 `persistCodexOauthCompatibleProxyMode` 在 settings mutation 成功后无条件 `await refreshCodex()`。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs:15` 和 `:22` 的 app-server 与 bundled catalog timeout 均为 20 秒。

结果是 OAuth 开关的前端 pending 时间包含无关的 `model/list` 或 bundled catalog 子进程时间。切换标签会卸载或重建局部 UI，但不会证明后端投影已经成功，因而呈现“回来又能点、像没生效”。

### 后端

- `src-tauri/src/app/settings_service.rs:493` 的 runtime plan 把 OAuth 兼容模式变化归入 `cli_proxy_sync_required`。
- `src-tauri/src/app/settings_service.rs:1133` 的 `sync_cli_proxy_for_settings` 调用 `cli_proxy::sync_enabled`，任何失败行或 blocking 错误都只返回 `false`。
- `src-tauri/src/app/settings_service.rs:1449` 和 `:1690` 保存 `cli_proxy_synced`，但不会因 Codex 活动投影同步失败而回滚设置或返回错误。
- `src-tauri/src/infra/cli_proxy/mod.rs:1327` 的 `sync_enabled` 即使当前 Codex 投影已经匹配，也会在 `:1403` 同步 managed model catalog；重新投影后还会在 `:1486` 再同步目录。

这造成部分成功：`settings.json` 可以已经是新 OAuth 模式，而活动 `config.toml`/`auth.json` 仍是旧投影，前端却只看到 settings 命令成功。

## remote compaction provider 链路

### Provider 重命名

- `src-tauri/src/infra/codex_config/patching.rs:392` 的 `rename_model_provider_table` 基于文本行，只识别精确的无引号 base table header，且不预检目标 provider。
- `src-tauri/src/infra/codex_config/patching.rs:814` 在 `remote_compaction` 开启时设置根 `model_provider = "OpenAI"` 并把 `aio` 重命名为 `OpenAI`，关闭时反向处理。
- 当 `[model_providers.OpenAI]` 已存在时，当前逻辑可能产生重复 table 或把后续 `name` 更新到错误 table；TOML 解析或 provider sync 随后报错。
- 单引号、双引号、dotted key 和嵌套 table 的检测、重命名与冲突语义没有统一覆盖。

### Proxy 投影和状态

- `src-tauri/src/infra/cli_proxy/codex.rs:725` 的配置构建器在 `:742` 和 `:748` 无条件写 `aio`，没有从 `features.remote_compaction` 推导 provider。
- 同文件 `:794` 的状态检查同时接受 `aio` 和 `OpenAI`，但没有验证 provider 是否与 `remote_compaction` 的目标一致，且主要使用字符串包含判断。
- `src-tauri/src/infra/cli_proxy/codex.rs:274` 的 merge restore 只把 `[model_providers.aio]` 视为活动投影，未对 `OpenAI` 使用同一所有权规则。
- `src-tauri/src/infra/cli_proxy/mod.rs:1060` 的启用/修复和 `:1327` 的同步都会重新执行上述配置构建器，所以合法 `OpenAI` 状态可能在后续同步中被写回 `aio`。

历史提交 `5686f9d3` 曾包含从 `remote_compaction` 推导 provider 的投影助手；提交 `93a08f15` 移除旧 gateway 集成时同时删掉了通用投影路径，当前构建器回到硬编码 `aio`。可以借鉴旧实现的单一推导方向，但不能恢复其字符串 fallback 或已删除的 gateway 功能。

## 配置所有权

- `src-tauri/src/infra/codex_config/mod.rs:50` 可在 proxy 已启用时更新 manifest 指向的 backup。
- `src-tauri/src/infra/codex_config/mod.rs:289` 的原始 TOML 保存把提交内容同时写入 backup 和活动 `config.toml`。
- `src-tauri/src/infra/codex_config/mod.rs:314` 的结构化保存先以活动文件为输入生成 `next`，再把同一 `next` 写入 backup；remote compaction 时还把它交给 provider sync。

活动文件含有 AIO 拥有的 `model_provider`、provider base URL、认证策略、`model_catalog_json` 和 Windows sandbox 投影。把活动文件整体写进 backup 会污染可恢复基线，关闭 proxy 后可能残留活动字段或丢失启用前值。

现有 `codex_managed_profiles::lock_profile_lifecycle()` 已被 Codex proxy 启用和同步路径使用，可以作为 Codex 配置、proxy 和 managed catalog 事务的共享串行化边界，避免另建互不知情的锁。

## 前端状态缓存

- `src/query/cliManager.ts:200` 的 Codex 结构化 mutation、`:217` 的原始 TOML mutation 和 `:234` 的 provider sync mutation只失效 Codex 配置查询，没有失效 `cliProxyKeys.statusAll()`。
- `src/ui/Sidebar.tsx:297` 只根据缓存中的 `enabled && applied_to_current_gateway === false` 显示“修复”。

本机只读抽样显示当前配置为 `remote_compaction = true`、根 provider 为 `OpenAI`，其 provider base URL 与启用 manifest 的 loopback `/v1` 地址相同。以当前源码的宽松静态条件判断，该配置应被接受。因此截图还可能包含旧版本二进制、查询缓存或时序因素；规划必须同时修正状态语义和 mutation 后失效，不能把截图直接归因于当前静态谓词的单一分支。

## 既有安全边界

- `src-tauri/src/infra/codex_provider_sync.rs` 已对 rollout JSONL、SQLite threads 和 global state 做预检、快照、事务更新与失败回滚。
- `src-tauri/src/infra/codex_config/mod.rs:199` 目前仅让结构化 `remote_compaction` 变化触发 provider sync；普通配置补丁不应扩大为全量 provider sync。
- `src-tauri/src/infra/cli_proxy/mod.rs:1060` 的 Codex 启用路径和 `:1327` 的同步路径已有 target/manifest 快照及 managed catalog 回滚骨架。
- `aio/<profile_name_key>` 只有 Codex CLI proxy 开启时才会进入 AIO gateway。普通 Codex 配置读写不依赖该开关。

## 测试缺口

- `src-tauri/src/infra/codex_config/tests.rs:798` 起只覆盖简单的 `aio` 与 `OpenAI` 往返，没有已有目标、双 provider、引号、dotted/nested 或逐字节不变的冲突测试。
- `src-tauri/src/infra/cli_proxy/tests.rs` 有 OAuth 与 status 基础测试，但没有 `remote_compaction = true` 下启用、修复、重绑、同步、关闭的完整回归。
- `src-tauri/tests/cli_proxy_startup_recovery.rs` 没有覆盖 proxy 开启期间通过设置页保存普通字段后再关闭的恢复结果。
- `src/query/__tests__/cliManager.test.tsx` 没有断言 remote/raw/provider sync 会失效 proxy status。
- `src/pages/__tests__/CliManagerPage.test.tsx` 和 `CodexTab.test.tsx` 没有证明 OAuth 开关不等待模型目录，也没有覆盖后端同步失败后的 pending 收敛。
