# 技术设计：Codex OAuth 代理与 remote compaction 一致性修复

## 设计结论

本任务保持为一个实现单元，不拆成相互独立的前后端子任务。OAuth 设置、provider 重命名、活动 proxy 投影、状态检测和关闭恢复共同修改同一组 Codex 文件；分开提交会产生可见的无效中间状态。

采用以下四项核心决策：

1. `features.remote_compaction` 是托管 provider key 的唯一来源：精确布尔值 `true` 对应 `OpenAI`，其他情况对应 `aio`。
2. 使用一个结构化 TOML provider 模块统一完成推导、冲突预检、身份收敛、活动投影和状态检查；删除各调用点自行维护的字符串判断。普通配置补丁若没有显式修改 `remote_compaction`，不得触发 provider 身份收敛。
3. 路由关闭时 live config 就是用户配置；路由开启时 manifest backup 是用户基线，live config 是从基线派生的活动投影。任何 AIO 设置页写入都先更新基线，再重新派生活动投影。
4. OAuth-only 设置变化走轻量 Codex 重投影，不调用 app-server `model/list`、bundled catalog 或 managed catalog 重建；活动投影失败时回滚设置并返回错误。

不新增数据库 migration，不改变 `aio/<profile_name_key>` 的路由要求，也不恢复已经删除的旧 gateway 功能。

## 用户可见边界

| 场景 | 是否要求 Codex 路由开启 | 配置所有权 |
| --- | --- | --- |
| 编辑普通 Codex 配置 | 否 | 路由关闭时直接持久化用户配置 |
| 使用 `aio/<profile_name_key>` | 是 | 请求必须经过 AIO gateway |
| 使用 AIO provider/base URL/auth 投影 | 是 | 活动期间由 AIO 拥有，关闭时恢复用户基线 |
| 修改 `remote_compaction` | 否 | 功能值是用户字段；启用路由时同时重建活动投影 |

路由关闭后不存在后台 proxy 同步去覆盖普通配置。路由开启期间，外部或 AIO 设置页对用户字段的修改会写回可恢复基线；对 proxy 拥有字段的外部修改被视为 drift，不静默吸收到基线。

## 统一 Provider 模型

在 `src-tauri/src/infra/codex_config/provider_projection.rs` 建立共享纯函数层，供 `codex_config` 和 `cli_proxy::codex` 调用。

核心类型：

```rust
enum CodexManagedProviderKey {
    Aio,
    OpenAi,
}

struct CodexManagedProviderContext<'a> {
    desired: CodexManagedProviderKey,
    expected_base_url: Option<&'a str>,
}

enum ProviderReconcileOutcome {
    Unchanged,
    Renamed,
    Reused,
    Deduplicated,
}
```

模块职责分为两步：

- 用 `toml_edit::DocumentMut` 解析并保留 TOML decoration，禁止用 `contains` 推断语义。
- 从根 `[features].remote_compaction` 的精确布尔值推导目标 key；注释、字符串、错误类型和其他 table 中同名字段都不生效。
- 以语义 table inventory 识别无引号、单引号、双引号、dotted key、inline table 和嵌套 table。
- `reconcile_provider_identity` 只在显式 remote compaction patch 或活动投影生成时运行，负责根 `model_provider`、source/target 重命名、冲突预检和安全合并。路由关闭时它保留 provider 的 `base_url`、wire/auth 与用户字段，只把 `name` 收敛到目标 key。
- `project_active_provider` 只为已启用的 AIO proxy 运行，在身份收敛后设置目标 provider 的活动托管字段：`name`、当前 gateway `base_url`、`wire_api = "responses"`、`requires_openai_auth = true`。
- 返回预检后的新字节和 reconcile outcome；调用方只在全部预检成功后写文件或启动 provider sync。
- 提供结构化 `is_managed_projection_applied`，检查目标 key、目标 table、当前 gateway `/v1` 地址、认证模式和 alternate 托管 table 是否残留。

`patching.rs` 继续负责其他配置字段的局部、格式友好更新；它不再自行重命名 provider。只有 `CodexConfigPatch.features_remote_compaction.is_some()` 时，才把中间字节交给 provider 模块做身份收敛。其他普通 patch 保持根 provider 和全部 provider table 不变。

## Provider 冲突矩阵

托管字段为 `name`、`base_url`、`wire_api`、`requires_openai_auth`。其余 provider scalar、array、inline table 和嵌套 subtree 均视为用户字段。

| 当前状态 | 处理 |
| --- | --- |
| 只有 source provider，target 不存在 | 把整个 source subtree 移到 target，只更新 provider `name`，保留地址、其他字段和 decoration |
| 只有 target，且身份与当前 AIO provider 兼容 | 复用 target，身份收敛幂等；仅在活动投影阶段补齐托管字段 |
| source 与 target 同时存在，地址和托管字段兼容 | 递归合并用户字段；只复制单侧字段，双方同路径值必须语义相等，然后删除 source |
| target 地址不是当前 AIO loopback，或托管字段值不兼容 | 在任何写入前返回冲突错误 |
| source/target 同一用户字段路径值不同，或 table/value 类型冲突 | 在任何写入前返回冲突错误 |
| TOML 已有语法级重复 table | 解析阶段拒绝，不尝试猜测或修复 |

稳定错误码使用 `CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT`。错误消息只列出有界字段路径，不包含 API key、token 或完整 URL，并提示用户先重命名/处理已有 `OpenAI` provider。

关闭 `remote_compaction` 时使用同一矩阵反向收敛到 `aio`。不能因为方向相反就覆盖用户已有 `aio` provider。

冲突预检中的预期 AIO 地址按上下文取得：proxy 开启时使用 manifest/current gateway；proxy 关闭时优先使用可证明属于 AIO 的 source provider，并与 planned loopback origin 交叉校验。target 已存在但没有 source、manifest 或 planned origin 能证明其归属时，不自动认领该 target，按冲突失败关闭。target 不存在时允许保留 source 地址的纯身份重命名。

## 配置基线与活动投影

### 所有权表

| 字段 | 用户基线 | 活动投影 |
| --- | --- | --- |
| `features.remote_compaction` 及其他普通设置 | 用户拥有 | 从基线保留 |
| 根 `model_provider` | 用户拥有原始值 | AIO 根据 remote compaction 投影为 `aio`/`OpenAI` |
| 目标 provider 托管字段 | 用户拥有原始 provider | AIO 临时投影到 gateway `/v1` |
| provider 未知字段/嵌套 subtree | 用户拥有 | 原样保留，冲突时失败关闭 |
| `preferred_auth_method` | 用户拥有原始值 | 按 OAuth-compatible 模式投影 |
| `model_catalog_json` | 用户拥有原始值 | 有 managed profiles 时由 AIO 投影 |
| `[windows].sandbox` | 用户拥有原始值 | Windows 活动路由可注入 |
| `auth.json` 认证键 | 用户拥有原始值 | 非 OAuth-compatible 路径按既有 placeholder 契约投影 |

### 关闭路由时

```text
当前 config.toml
  -> 验证/结构化 patch
  -> provider 冲突预检
  -> 普通补丁原子写；remote 变化走既有 provider sync 事务
```

没有 enabled manifest 时不更新 proxy backup，不运行后台投影。下次开启路由时由现有 enable 流程捕获最新文件作为基线。

### 开启路由时

```text
manifest backup（用户基线）
  -> 应用用户 patch
  -> provider 冲突预检/收敛
  -> derive live projection（provider/auth/catalog/sandbox）
  -> 快照 backup + live + manifest + provider-sync targets
  -> 原子提交并校验 status
  -> 任一步失败按快照恢复
```

结构化设置直接作用于 backup 基线，不再以活动文件作为 canonical 输入。普通字段同时出现在新基线和新活动投影中，关闭路由后仍存在。

原始 TOML 编辑器在路由开启时采用三方语义：旧基线、由旧基线生成的预期活动投影、用户提交的 TOML。用户字段变更合并入新基线；若提交改动了 provider base URL、根 provider、认证/catalog/sandbox 等活动拥有字段，则返回 `CODEX_PROXY_OWNED_FIELD_EDIT`，提示先关闭路由，而不是静默忽略或污染 backup。之后再从新基线生成活动投影。

关闭路由时，把活动文件中可证明是用户拥有的外部修改合并到基线，再恢复基线拥有的 provider/auth/catalog 字段。活动 owned 字段与预期不一致只作为 drift 处理，不覆盖基线；最终不得残留仅由活动投影创建的 `aio` 或 `OpenAI` table。

已被旧版本污染且无法从 manifest/backup 证明原值的字段不做猜测性迁移。可证明为当前活动投影的字段按上述规则去投影；存在歧义时失败关闭并保留文件，避免第二次数据损失。

## 事务与锁顺序

复用 `codex_managed_profiles::lock_profile_lifecycle()` 串行化以下操作：

- Codex proxy 启用、修复、同步、重绑和关闭。
- 结构化与原始 Codex 配置保存。
- provider key sync 以及 managed model catalog 根配置更新。

若调用路径同时需要异步 gateway lifecycle lock，固定顺序为：

1. gateway lifecycle lock；
2. blocking worker 内的 Codex profile/config lifecycle lock；
3. 文件快照和 SQLite transaction。

禁止在持有 Codex sync lock 时反向等待 gateway lifecycle lock。测试 hook 应覆盖并发保存与 proxy sync，证明最终 backup/live 是同一 generation。

仅 `sync_history = true` 的 `remote_compaction` provider sync 使用现有 Codex App 运行检查、rollout/SQLite/global state 预检和回滚。`sync_history = false` 在这些历史迁移预检之前分支，只提交配置/backup/live 事务；配置文件自身的 symlink、大小、TOML 和原子写校验继续执行。新增的 backup/live 快照包裹在它外层：backup 写失败时不启动 provider sync；provider sync 失败时恢复 backup；成功后不再执行可能失败的无关工作。

## OAuth-only 快速同步

`SettingsRuntimePlan` 区分三种原因：gateway rebind、Codex home 变化、OAuth-compatible 变化。新增 Codex-only 轻量同步入口，只有第三种原因时执行：

1. 检查 Codex proxy manifest；未启用则立即成功，不修改文件。
2. 获取既有 Codex lifecycle lock并快照 config/auth/manifest。
3. 从 canonical backup 和当前 provider 规则重新生成 config/auth 活动投影。
4. 原子写入并用共享状态检查器校验。
5. 失败恢复快照并返回带稳定错误码的失败结果。

这条路径明确不调用：

- `codex_model_catalog::managed::sync_current_locked`；
- Codex app-server `model/list`；
- `debug models --bundled`；
- provider model discovery 或网络请求。

gateway origin 或 Codex home 变化仍使用完整 `sync_enabled`，因为这些变化确实可能要求重绑目录和 managed catalog。

`sync_cli_proxy_for_settings` 改为返回可区分成功/失败的结果。OAuth-compatible 已提交但 Codex 活动重投影失败时，复用 settings service 的 owned-field CAS rollback：恢复旧 OAuth 设置，再按旧设置恢复活动投影；若 rollback 失去并发所有权，则同步最新 canonical winner，不覆盖较新的设置。最终 command 返回 `CODEX_OAUTH_PROXY_SYNC_FAILED`，不得以 `cli_proxy_synced = false` 的成功响应结束。

## 前端交互与缓存

- `persistCodexOauthCompatibleProxyMode` 在 settings mutation 成功后不再 `await refreshCodex()`。
- 成功后只触发 Codex config/raw TOML 和 CLI proxy status 的定向失效；这些本地读取不进入 switch 的关键 pending 路径。
- settings mutation 失败时沿用现有 toast/error formatter，并由后端已完成的 rollback 使 query refetch 回到旧值。
- OAuth switch 在 mutation pending 时保持 disabled，并显示与现有控件风格一致的小型 loading indicator；成功、后端错误和同步错误都必须结束 pending。
- `useCliManagerCodexConfigSetMutation` 在 patch 含 `features_remote_compaction` 时失效 `cliProxyKeys.statusAll()`；raw TOML 保存和手动 provider sync 每次都失效该 status。
- 不通过前端条件隐藏“修复”。Sidebar 继续只在后端 `applied_to_current_gateway === false` 时显示它，修复来自共享状态谓词和及时缓存失效。

## 状态判定

Codex `applied_to_current_gateway` 使用与写入相同的 structured projection inspector：

1. 解析 `features.remote_compaction` 并得到唯一目标 provider。
2. 根 `model_provider` 必须等于目标 key。
3. 目标 provider 必须恰有一个语义 table，`base_url` 规范化后等于当前 gateway `/v1`，wire/auth 字段符合投影。
4. alternate key 不得同时保留另一个 AIO-owned loopback provider。
5. OAuth-compatible 开启时验证其配置投影；关闭时验证 auth placeholder 契约。
6. managed profiles 存在时继续验证 catalog 所有权，但 provider key 不再由 catalog 路径反向猜测。

因此合法 `remote_compaction = true + OpenAI + 当前 loopback` 状态直接为已应用；修复、gateway 重绑、OAuth 切换和 startup sync 都保持 `OpenAI`，不会自行写回 `aio`。

## 错误与回滚

| 错误码 | 条件 | 持久化结果 |
| --- | --- | --- |
| `CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT` | target provider 地址、托管字段或用户字段冲突 | config/backup/rollout/SQLite/global state 全部不变 |
| `CODEX_PROXY_OWNED_FIELD_EDIT` | proxy 开启时 raw TOML 改动活动拥有字段 | backup/live 不变 |
| `CODEX_OAUTH_PROXY_SYNC_FAILED` | OAuth 设置已写但活动 Codex 投影失败 | CAS 恢复旧设置和旧投影；恢复失败升级为 recovery-required 错误 |
| `sync_history = true` 的 provider sync 错误 | Codex App 运行、历史文件或 DB 预检/提交失败 | 延续既有 provider sync 回滚，并额外恢复 proxy backup 快照 |

所有错误日志禁止记录 auth 内容、API key、token、完整 TOML 或 rollout body。冲突错误只报告字段路径与处理建议。

## 兼容性与回滚

- 没有 CLI proxy manifest 的用户保持现有直写行为。
- 非 Codex CLI proxy 不改变；`sync_enabled` 的通用结果结构只做兼容扩展或内部封装。
- 没有 managed profiles 时 OAuth-only 路径也不启动 catalog 子进程。
- 有 managed profiles 时 OAuth-only 路径保留现有 `model_catalog_json` 投影，不重建 catalog；完整 enable/rebind/startup sync 仍负责目录一致性。
- 代码回滚不需要 DB downgrade。新版本若遇到无法证明的旧 backup 污染会失败关闭，不进行破坏性自动清理。

## MSI 测试反馈追加设计

### Provider Sync 范围

配置 mutation 增加显式、瞬时的 `sync_history` 选项，不把该选择写入 Codex 配置。结构化开启或关闭 `remote_compaction` 时都由 UI 弹窗选择；普通结构化保存和 raw TOML 保存默认 `false`，手动 Provider Sync 固定为 `true`。无论是否同步历史，当前 config/backup/provider 身份都必须完成一致更新。

弹窗以用户点击时的目标布尔值为受控状态。提交期间开关、三个按钮、Escape 和遮罩关闭全部禁用；只有配置 mutation 返回非空成功结果才关闭。返回 `null` 或 Promise 拒绝时保持原方向，结束 pending 后允许用户重试；页面数据模型继续负责错误提示。

`sync_history = false` 不执行 Codex App 进程检测，且 change set 不进入 rollout、SQLite 和 global state 收集函数。`sync_history = true` 才执行进程检测，并迁移 rollout `session_meta` provider、SQLite 会话关联字段和 `.codex-global-state.json`。因此“仅更新配置”不是先预检/扫描后跳过，而是从调用边界完全排除历史迁移。

### 有界历史迁移

`SessionChange` 只保存目标路径。发现阶段逐行读取 JSONL，只标记确有 provider 变化的文件；提交阶段逐个文件流式改写到同目录临时文件并原子替换。change set、日志和错误均不保留 rollout body。

rollback 不再把 session/SQLite/global state 全量读入内存。provider sync 先创建磁盘备份并记录 `{ target, backup, existed }`，失败时从备份逐文件恢复，原本不存在的 sidecar 则删除。`sync_history = true` 时配置和历史属于同一事务边界，并保留双重 Codex App 进程检查；`sync_history = false` 只提交配置事务，不执行该检查。

### Catalog 基线自修复

`CodexProxyBaseline` 同时暴露实际 backup 路径。当 backup 中 `model_catalog_json` 规范化后精确等于当前 AIO generated catalog 路径时，将其识别为可证明的旧版污染：从 backup TOML 删除该绑定，以 bundled catalog 作为本次 base，并把 backup、generated catalog、live config 作为同一 prepared transaction 提交和回滚。

修复只接受这一精确等值证据。任意其他外部路径、缺失文件或无法解析内容仍沿用既有校验，不猜测用户意图；底层 `reject_generated_path_as_base` 保留为最终 fail-closed 防线。

## 验证矩阵

### Rust provider/config

- `aio -> OpenAI`、`OpenAI -> aio`、重复执行幂等。
- target 已存在且等价时复用；双 table 可安全合并时去重。
- base URL、托管字段、重叠用户字段冲突时返回稳定错误且输入字节不被写入。
- 无引号、单引号、双引号、dotted、inline、嵌套 table 和注释保留。
- route off 普通字段直写；route on 结构化字段写入基线并在 disable 后保留。
- route on raw 用户字段合并；owned 字段编辑拒绝；disable 清除活动 provider/auth/catalog/sandbox。
- backup、live config、manifest 或 provider sync 任一步故障都回滚到同一 generation。

### Rust proxy/settings

- remote true 下 enable、status、repair、sync、rebind、disable 全程使用 `OpenAI`。
- remote false 下同矩阵使用 `aio`。
- OAuth 两种模式与有/无 managed profiles 组合均不调用 catalog hook。
- OAuth 重投影成功、投影失败、settings rollback 成功、并发 winner 和 recovery failure。
- gateway/home 变化仍走完整 sync，非 Codex proxy 行为不回归。

### TypeScript/React

- OAuth handler 不调用或等待 model catalog refresh，延迟的 catalog promise 不能阻塞 switch。
- settings success/error/sync error 都结束 pending；失败显示 toast并回到旧值。
- OAuth、remote patch、raw save、provider sync 后均失效 proxy status。
- remote 开启和关闭都弹出同一范围选择，标题和提交的目标布尔值必须与切换方向一致。
- remote config-only 在 Codex App 运行时仍成功；显式 history sync 在同一条件下保持 `CODEX_PROVIDER_SYNC_PROCESS_RUNNING` 且零写入。
- status true 时 Sidebar 不显示“修复”，status false 时仍可执行真正修复。

### 全量门禁

- Rust fmt/check、相关 lib/integration tests 和完整 `pnpm tauri:test`。
- 前端 focused Vitest、`pnpm typecheck`、`pnpm lint`。
- 若 DTO 无变化，generated bindings 必须保持无 diff；若 settings 结果契约需要调整，只通过生成脚本更新并运行 bindings check。
