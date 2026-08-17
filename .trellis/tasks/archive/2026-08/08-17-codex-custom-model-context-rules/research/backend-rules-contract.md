# Research: 后端规则 schema、迁移与目录事务契约

- Query: 为 Codex 自定义模型上下文规则确定后端数据结构、schema 64 迁移、managed catalog 原子事务、Codex home 切换、目录升级目标消失、错误码、回滚顺序和测试入口。
- Scope: internal（只读 `origin` worktree；未访问 `upstream`，未做网络检索）
- Date: 2026-08-17

## Findings

### 1. 结论摘要

1. `AppSettings` 升到 schema 65，用一个经过规范化的
   `Vec<CodexModelContextRule>` 替换
   `codex_gpt56_372k_context_enabled`。新 canonical JSON 不再序列化旧字段，也不增加规则集总开关。
2. 规则结构建议固定为 `{ model_id: String, context_window: i64, enabled: bool }`。
   `i64` 直接复用 Provider capability 的
   `MODEL_CONTEXT_WINDOW_MIN_TOKENS..=MODEL_CONTEXT_WINDOW_MAX_TOKENS`，无需第三套数值边界。
3. 所有写入先执行同一个严格 normalizer：Unicode `trim`、UTF-8 字节长度、控制字符、
   `aio/` 保留前缀、token 边界、全局唯一和 128 条上限校验，最后按 `model_id` 的 UTF-8
   字节序排序。禁用规则也执行全部静态校验，但不检查基础目录是否存在目标。
4. schema 64 的迁移必须读取**原始 JSON**中的旧布尔值，不能只依赖 typed
   `AppSettings`。现有 63 -> 64 migration 会把旧 typed `true` 清为 `false`
   （`src-tauri/src/infra/settings/migration.rs:1627-1646`）；若新迁移排在它后面读取 typed
   值，会把已启用用户错误迁为空集合。
5. 专属 IPC 只提供整集合替换：
   `settings_codex_model_context_rules_set(rules) -> SettingsView`。普通
   `SettingsUpdate` / `SettingsPatch` 不拥有该字段；`SettingsView` 只读暴露 canonical rules。
6. `ManagedCatalogPolicy` 改为携带 canonical rule vector。生成条件为
   `!profiles.is_empty() || rules.iter().any(|rule| rule.enabled)`；规则先投影到基础模型，再追加
   `aio/*` Profile，后者的 context 仍只来自 provider-model capability。
7. owner metadata 升到 v3，至少持有 canonical 全规则集 hash、enabled projection、
   Codex-home identity、原始 catalog binding、base fingerprint 和 Profile hash。继续读取 v1/v2
   只为安全恢复；v2 目录在第一次成功 reconcile 时重建为 v3。
8. 保留现有提交骨架：激活/刷新为 `proxy baseline repair -> generated catalog -> live config`；
   停用为 `proxy baseline repair -> live config restore -> generated catalog delete`；回滚逐目标
   CAS 并按逆序全部尝试。规则 settings 是 catalog 事务外层的第一个 durable commit，失败时先
   回滚 catalog，再 CAS 恢复仍由本事务拥有的规则集合。
9. 任一已提交的 enabled rule 在 Codex CLI/用户基础目录升级后不再恰好命中一个条目时，
   background reconcile 返回 typed target-cardinality 错误并保持 settings、generated、live config
   和 proxy backup 的提交前字节。不得自动禁用/删除规则，也不得部分重建；用户通过禁用或删除
   后再次整批提交恢复。此前因目标缺失而被拒绝的规则从未进入 canonical，未来目录出现同名模型
   也不会自动激活。
10. 只要存在 enabled rule，普通设置和配置导入都继续阻止 Codex home 变化。只有禁用规则时
    允许切换，规则保留并在新 home 重新启用时完整验证。`FollowCodexHome` 的环境路径漂移也要
    fail closed，不能静默把全局 generated catalog 认领到新 home。
11. 便携导出必须从 JSON object 中**删除**规则键；写空数组仍不满足“导出不包含规则”。导入必须
    在反序列化 `AppSettings` 前从 raw settings JSON 删除新规则键和 legacy 开关键，否则恶意类型/
    超限载荷会在“忽略输入”之前使导入失败。

### 2. Files Found

- `.trellis/tasks/08-17-codex-custom-model-context-rules/prd.md`：当前规则产品边界；R13-R21 已确定全量原子替换、禁用态、迁移和便携配置语义（`prd.md:29-37`）。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/design.md`：直接前身的 catalog policy、direct/proxy binding 和专属 settings 事务设计。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/catalog-lifecycle-main.md`：direct-mode ownership metadata 与三文件事务的前置调研。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/runtime-dataflow.md`：Codex 完整目录、启动快照、上下文字段及 lifecycle 入口调研。
- `.trellis/tasks/archive/2026-07/07-19-instant-error-retry-rules/research/rule-model-and-migration.md`：已有规则集合严格迁移、数量/长度限制和“不把无效规则扩大为有效匹配”的先例。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md`：完整 catalog、Profile capability、base source、owner hash 和补偿合同；旧 372K 场景从 `:507` 开始。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md`：settings field ownership、owned-token CAS、并发 winner 和外部副作用补偿合同。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`：structured/raw config、proxy baseline 和 provider/history 同步合同。
- `src-tauri/src/infra/settings/defaults.rs`：当前 schema 64、规则数量常量先例和旧默认开关（`:5`, `:22`, `:68-72`）。
- `src-tauri/src/infra/settings/types.rs`：`AppSettings` 当前旧布尔字段及序列化结构（`:550-580`, `:677-696`）。
- `src-tauri/src/infra/settings/migration.rs`：typed migration 链、规则 normalizer 先例和旧开关迁移（`:456-577`, `:1627-1744`）。
- `src-tauri/src/infra/settings/persistence.rs`：raw JSON 解析、canonical serialization、repair 持久化和 settings atomic write/CAS（`:193-218`, `:221-261`, `:536-697`）。
- `src-tauri/src/app/settings_service.rs`：普通 owner 的 compare-only 旧策略字段、Codex-home guard、专属 372K settings/catalog transaction（`:344-524`, `:1036-1053`, `:1588-1719`）。
- `src-tauri/src/commands/settings.rs` / `src-tauri/src/commands/registry.rs`：专属 settings IPC 和命令注册入口。
- `src-tauri/src/infra/codex_model_catalog/managed.rs`：核心 prepare/apply/rollback、base guard、owner metadata、目录生成和 exact overlay（`:97-429`, `:431-928`, `:1435-1734`, `:1846-1985`）。
- `src-tauri/src/infra/codex_model_catalog/mod.rs` / `protocol.rs`：当前 picker DTO 与 `model/list` 解析；DTO 不含基础 context 值（`mod.rs:33-57`, `protocol.rs:203-284`）。
- `src-tauri/src/infra/codex_config/mod.rs`：structured/raw save 共享 catalog reconciler，且 live config/proxy backup 具有独立回滚 token（`:268-386`, `:550-776`）。
- `src-tauri/src/infra/config_migrate/mod.rs`：导出、raw bundle settings 解析、home rebind、catalog apply 和多层补偿顺序（`:405-412`, `:561-582`, `:680-1026`）。
- `src-tauri/src/infra/cli_proxy/codex.rs` / `mod.rs`：Codex home rebind、代理启停/离线恢复及 catalog lifecycle 调用。
- `src-tauri/src/app/startup_settings.rs`：启动时统一 catalog reconcile 与外层错误码（`:59-76`）。
- `src-tauri/src/domain/codex_managed_profiles.rs` / `provider_models.rs`：Profile lifecycle lock、Profile/capability 更新入口和共享 context 边界（`provider_models.rs:22-24`, `:429-444`）。
- `src-tauri/src/infra/codex_paths.rs`：三种 Codex-home 模式及外部 `CODEX_HOME` 解析（`:55-125`）。
- `src-tauri/src/shared/error.rs` / `shared/fs.rs`：`AppError` 的稳定 `CODE: message` IPC 表达和原子文件替换（`error.rs:18-39`, `fs.rs:395-423`）。

### 3. Existing Code Patterns

- settings 全局 schema 当前为 64，旧布尔是 `AppSettings` 的 persisted field
  （`src-tauri/src/infra/settings/defaults.rs:5,68`；`types.rs:573-580`）。
- `settings::update` 在共享 write lock 内重新读取、变更并原子替换；`compare_and_swap` 比较完整
  canonical JSON（`src-tauri/src/infra/settings/persistence.rs:660-697`）。规则专属 writer 应沿用
  owned-field token，不能调用 stale whole-snapshot `settings::write`。
- 普通 settings owner 当前故意不应用旧 372K 字段，只把它放进 compare-only token
  （`src-tauri/src/app/settings_service.rs:383-386,483-525`）。新 rules vector 应替换这个 compare-only
  guard，仍不得进入普通 update/patch payload。
- 现有专属事务在 profile lifecycle lock 下先 prepare，再 durable commit setting，再 apply catalog，
  最后 canonical reread；失败时 files-first、setting-second 补偿并保留并发 winner
  （`src-tauri/src/app/settings_service.rs:1588-1709`）。
- catalog plan 在 apply 前重检 ownership、base guard、live config 和 generated snapshot
  （`src-tauri/src/infra/codex_model_catalog/managed.rs:145-175`）。
- 激活写入顺序是 baseline、generated、config；停用顺序是 baseline、config、generated
  （`managed.rs:178-241`）。回滚按方向逆序且每个目标独立尝试（`managed.rs:350-417`）。
- base 是用户绝对 catalog 或 installed Codex bundled catalog，用户源以 path + bytes hash 做 guard，
  bundled 源以 executable/runtime/version/length/mtime descriptor 做 guard
  （`managed.rs:431-536,1331-1432`）。
- 当前生成器先验证根对象、`models` 数组和全局 slug 唯一性，再应用旧 policy、再追加
  `aio/*` entries（`managed.rs:1501-1608`）。这是通用规则最合适的替换点。
- owner metadata/payload hash 会阻止外部目录修改和错误缓存复用
  （`managed.rs:1610-1681,1846-1985`）。
- structured/raw save 已经持有 lifecycle lock，并通过 proposed config 走同一个 catalog reconciler；
  catalog 回滚、live config 回滚和 proxy backup/manifest 回滚各自使用 committed token
  （`src-tauri/src/infra/codex_config/mod.rs:268-386,550-776`）。
- config import 锁序是 `CONFIG_IMPORT -> profile lifecycle -> updater`，应用顺序为 staged DB/Skill FS、
  settings、home rebind、catalog、runtime、DB commit；后半段失败先回滚 catalog 与 rebind，再恢复
  DB/settings/Skill FS/runtime（`src-tauri/src/infra/config_migrate/mod.rs:680-684,804-1026`）。
- Provider capability 已有唯一后端 context 边界 `1_024..=10_000_000`，且数据库/业务验证均引用
  该常量（`src-tauri/src/domain/provider_models.rs:22-24,429-444`）。

### 4. Proposed Persisted Schema

```rust
pub const SCHEMA_VERSION: u32 = 65;
pub(super) const SCHEMA_VERSION_ADD_CODEX_MODEL_CONTEXT_RULES: u32 = 65;
pub const MAX_CODEX_MODEL_CONTEXT_RULES: usize = 128;
pub const MAX_CODEX_MODEL_CONTEXT_MODEL_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct CodexModelContextRule {
    pub model_id: String,
    pub context_window: i64,
    pub enabled: bool,
}

#[serde(default)]
pub struct AppSettings {
    // ...
    pub codex_model_context_rules: Vec<CodexModelContextRule>,
    // remove codex_gpt56_372k_context_enabled
}
```

Canonical settings JSON 示例：

```json
{
  "schema_version": 65,
  "codex_model_context_rules": [
    {"model_id":"gpt-5.6-luna","context_window":372000,"enabled":true},
    {"model_id":"gpt-5.6-sol","context_window":372000,"enabled":true},
    {"model_id":"gpt-5.6-terra","context_window":372000,"enabled":true}
  ]
}
```

字段选择理由：

- `context_window` 与 Codex catalog/provider capability 命名一致；只有一个值，投影时双写
  `context_window` 和 `max_context_window`。
- 用 `i64` 让负数/越界值走统一 typed validation；JSON 浮点数会在 serde 边界直接被拒绝。
- 不持久化规则 ID、priority、family、pattern 或全局 enabled。模型 ID 是唯一 key，删除就是从集合移除。
- `#[serde(deny_unknown_fields)]` 防止拼错字段被静默忽略；当前规则是 canonical device policy，读取
  malformed 规则应 fail closed，不能像显示性偏好一样 lossy drop。

严格 normalizer 的顺序应固定：

1. 若长度大于 128，立即返回 `CODEX_MODEL_CONTEXT_RULE_LIMIT`。
2. 每条 `model_id = model_id.trim().to_string()`；Rust `String::len()` 即 UTF-8 bytes。
3. 校验 `1..=256` bytes、`!chars().any(char::is_control)`。
4. 精确、区分大小写地拒绝 `model_id.starts_with("aio/")`；不要扩大成大小写不敏感规则。
5. 使用 `crate::provider_models::{MODEL_CONTEXT_WINDOW_MIN_TOKENS,
   MODEL_CONTEXT_WINDOW_MAX_TOKENS}` 校验唯一 token 值。
6. 在 enabled/disabled 的合并集合中用规范化 `model_id` 检查唯一；空白归一后重复也必须失败。
7. 用 `sort_by(|a, b| a.model_id.as_bytes().cmp(b.model_id.as_bytes()))` 排序。不要依赖
   `HashMap` iteration 生成 persisted bytes 或 hash。

`settings::write_unlocked`、migration、专属 command 和 catalog policy constructor 都调用同一个
normalizer；写入口严格报错，不能复用会静默丢弃规则的 sanitizer。

### 5. Schema 64 -> 65 Migration

目标映射固定为：

```text
schema 64 + legacy absent/false -> []
schema 64 + legacy true ->
  gpt-5.6-luna  / 372000 / enabled
  gpt-5.6-sol   / 372000 / enabled
  gpt-5.6-terra / 372000 / enabled
```

建议把原始 JSON 信息显式送进 migration，而不是给 `AppSettings` 保留一个隐藏 legacy field：

```rust
fn migrate_codex_model_context_rules(
    settings: &mut AppSettings,
    schema_version_present: bool,
    original_schema_version: u32,
    raw: &serde_json::Value,
) -> AppResult<bool>;
```

契约：

- 只在 `schema_version_present && original_schema_version == 64` 时消费旧键；更老 schema 没有该
  产品字段，迁移为空。schema 缺失沿用历史默认 false。
- schema 64 的旧键 absent 视为 false；存在但不是 JSON bool 时返回 `SEC_INVALID_INPUT`，保留原文件，
  不把损坏的 true 状态猜成 false。
- 先捕获 `original_schema_version` 和 raw legacy bool，再运行会改写 typed settings 的 migration 链；
  或把 raw-aware migration 作为链的最后一步，但绝不能在 `migrate_add_codex_gpt56_372k_context`
  清值后读取 typed bool。
- preset 通过同一个 normalizer 排序。schema 65 canonical serialization 只写新 vector，旧键自然删除。
- schema 65 重读跳过 legacy mapping；第二次读取后 settings bytes、vector 和 catalog policy hash 必须不变。
- owner metadata v2 继续可验证；settings 成功迁移后，统一 reconciler 从等价三条 rules 重建 v3 metadata，
  不能先删除/解绑旧 v2 generated catalog。

一个现有风险必须在实现时处理：`read_unlocked` 对 migration repair 的 `write_unlocked` 错误使用
`let _ =` 忽略（`src-tauri/src/infra/settings/persistence.rs:251-260`）。旧迁移大多只改偏好，但本次
迁移可能恢复一个 enabled catalog policy。建议让 repair 持久化失败直接返回
`SETTINGS_PERSISTENCE_FAILED` / `SETTINGS_RECOVERY_REQUIRED`，禁止 startup 在只有 cache、没有可靠
durable schema 65 的情况下继续应用规则。至少要有 fault-injection 测试证明失败时不报告迁移成功。

### 6. IPC And Read Model

最小写 API：

```rust
#[tauri::command]
#[specta::specta]
async fn settings_codex_model_context_rules_set(
    app: tauri::AppHandle,
    rules: Vec<CodexModelContextRule>,
) -> Result<SettingsView, String>;
```

- 后端只接收完整集合；不提供 add/edit/toggle/delete 单项命令。
- 返回 canonical `SettingsView`，其中包含已经 trim、排序并确认持久化的规则；前端不能用 request
  draft 代替 response。
- `SettingsUpdate`、`SettingsPatch` 不加 rules 字段。`SettingsServiceOwnedToken` 加一份
  `codex_model_context_rules` compare-only guard，`apply_to` 不写它。
- IPC 继续使用项目统一 `AppError -> "CODE: message"`，无需在本任务另造错误 envelope
  （`src-tauri/src/shared/error.rs:18-39,79-82`）。

候选模型不应直接复用当前 `cli_manager_codex_model_catalog_get`：它先执行有写入能力的 reconcile，
而且 `model/list` DTO 不带 base context；当升级后 enabled target 消失时，该 GET 反而会失败，用户
无法获得修复信息（`src-tauri/src/commands/cli_manager.rs:34-41`；
`src-tauri/src/infra/codex_model_catalog/mod.rs:33-57`）。建议增加只读接口：

```rust
struct CodexModelContextCandidate {
    model_id: String,             // base models[*].slug，非 model/list alias
    display_name: String,
    base_context_window: Option<i64>,
    base_max_context_window: Option<i64>,
}

struct CodexModelContextCandidatesState {
    status: CodexModelCatalogStatus,
    issue: Option<CodexModelCatalogIssue>,
    snapshot: CodexModelCatalogSnapshot,
    models: Vec<CodexModelContextCandidate>,
}

cli_manager_codex_model_context_candidates_get()
    -> CodexModelContextCandidatesState
```

该 GET 在 lifecycle lock 下只检查 current ownership 并读取**原始 base source**，不调用
`sync_current_locked`、不创建目录、不写 config。建议过滤 literal `aio/`，保留 context 字段为
`None` 的可见 slug 供搜索；候选只是提示，SET 事务仍重新读取同一 base 并做 authoritative validation。

### 7. Managed Catalog Policy And Metadata v3

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedCatalogPolicy {
    pub(crate) model_context_rules: Vec<CodexModelContextRule>, // always canonical
}
```

`ManagedCatalogPolicy::new/from_settings` 必须验证 canonical invariants；不要继续 `Copy`。核心派生：

```text
enabled_rules = rules where enabled == true
needs_generated_catalog = !profiles.is_empty() || !enabled_rules.is_empty()
```

生成算法：

1. 完整解析 base root/`models`，沿用 4 MiB、1,000 base model 和全局 slug 唯一限制。
2. 预先构造 enabled target set。遍历 base 时仍要求每项为 object、`slug` 非空且 <=256 bytes。
3. 若重复 slug 属于 enabled target，返回 target-cardinality code；其他重复仍返回
   `CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID`。
4. 对每条 enabled rule 要求该 slug 恰好出现一次，且原 `context_window` 与
   `max_context_window` 都是 JSON 非负整数。任一条失败时不产出 bytes。
5. 对命中 object 只覆盖两个字段为同一个 rule value。不要检查值比 base 高还是低，也不要改
   `effective_context_window_percent`、`auto_compact_token_limit`、reasoning 或未知字段。
6. 所有规则投影完成后追加 `aio/*` Profile。R18 已禁止规则命中 `aio/*`，Profile context 继续来自
   `ProviderModelCapabilities`。
7. disabled rule 完全不参与 base target lookup。若没有 Profile 且全部 disabled，不读取 bundled/user
   base，直接准备恢复 original binding/删除 owned generated file。

metadata 建议从 v2 升到 v3：

```json
{
  "schema_version": 3,
  "managed_by": "aio-coding-hub",
  "profile_set_sha256": "...",
  "base_source_fingerprint": "...",
  "original_catalog_path": null,
  "codex_home_key": "...",
  "model_context_policy_version": 1,
  "model_context_rule_set_sha256": "...",
  "model_context_enabled_rules": [
    {"model_id":"gpt-5.6-sol","context_window":372000}
  ],
  "model_context_enabled_rules_sha256": "...",
  "projection_sha256": "...",
  "payload_sha256": "..."
}
```

- `rule_set_sha256` 对完整 canonical rules（含 `enabled` 和 disabled rules）做 compact JSON hash；
  `enabled_rules_sha256` 对排序后的 `{model_id, context_window}` 投影做 hash。这样相同集合不同输入
  顺序得到相同 settings bytes、owner hash 和 generated bytes。
- `projection_sha256` 纳入 owner schema、Profile hash、base fingerprint、original binding、
  `codex_home_key`、两个 rule hash 和 policy version。
- metadata 嵌入 enabled projection，使 `validate_owned_catalog` 能独立验证 metadata 结构；payload hash
  仍覆盖去除 metadata 后的完整 catalog 及上述 ownership 字段。
- `codex_home_key` 应复用项目已有的 path identity 语义：可 canonicalize 时 canonicalize，统一斜杠，
  Windows ASCII lowercase，非 UTF-8 fail closed（相似实现见
  `src-tauri/src/infra/config_migrate/provider_local_state.rs:271-286`）。
- v1/v2 只作为 legacy reader。v1 无 original binding，direct active 仍 recovery-required；v2 有 binding，
  但没有 home key。迁移 v2 active policy 时必须看到当前 live config 已绑定 generated path才可认领当前
  home；否则返回 home-drift/recovery error，不能在 `original_catalog_path == None` 时静默采用新 home。

### 8. Prepare / Apply / Rollback Contract

统一 reconciler 的输入必须显式、不可变：`profiles + canonical policy + intent`。所有入口在
`codex_managed_profiles::lock_profile_lifecycle()` 下调用它；config import 使用已经持锁的 `_locked`
路径，不能递归获取锁。

Prepare（首次 durable write 前）：

1. 解析 ownership context（proxy applied / proxy restored direct / direct）及 resolved home key。
2. snapshot proxy baseline/backup、live config、generated file、original binding。
3. 验证现有 owner metadata；选择 user absolute base 或 installed bundled descriptor。
4. 严格验证 rules；仅当需要 generated 时加载一次 base，验证所有 enabled targets 并生成完整 bytes。
5. 计算 deterministic v3 metadata/hash 和 proposed config bytes。
6. 返回 plan；不创建目录、不写 temp/backup/config/settings，不改变 DB。

现有 `managed_catalog_path` 在 prepare 中执行 `create_dir_all` 和 `canonicalize`
（`src-tauri/src/infra/codex_model_catalog/managed.rs:960-976`），与 PRD R4 和现有 spec 的
“Prepare is side-effect free”（`codex-managed-model-route-contract.md:570-571`）不一致。应把它拆成
纯 path resolution；父目录创建留给 apply 中的 atomic writer，并增加“prepare 后目录仍不存在”测试。

Apply 在第一笔写入前重新检查：

- ownership context 和 home key 未变；
- user base bytes/fingerprint 或 bundled executable descriptor 未变；
- proxy backup、live config、generated snapshot 未变；
- 若 dedicated rules transaction 已先提交 settings，当前 rules 仍等于本 plan 的 committed token。

文件提交和回滚顺序：

| 方向 | Apply | Rollback |
| --- | --- | --- |
| 激活/刷新 | baseline repair -> generated write -> live config bind | live config -> generated -> baseline |
| 取消最后一个生成原因 | baseline repair -> live config restore -> generated delete | generated -> live config -> baseline |
| rules 改动但仍需 generated | baseline repair（如需）-> generated replace -> live config（如需） | live config -> generated -> baseline |

每个 committed target 保存 `{before_snapshot, committed_after}`；rollback 仅当 current 等于
`committed_after` 时恢复 `before_snapshot`，一个目标漂移/失败不能阻止其余目标尝试。任一目标未恢复，
统一返回 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`。

专属 rules SET 的外层顺序：

1. lifecycle lock；normalize candidate；load profiles；prepare candidate plan（零写入）。
2. `settings::update` 仅替换 rules vector，记录 `previous_rules` 和 `committed_rules`。
3. `plan.apply`。内部 partial failure 已先按上表逆序回滚 files。
4. canonical `settings::read` 确认完整 vector 相等；成功才返回 `SettingsView`。
5. apply/confirmation 失败：先 `AppliedManagedCatalog::rollback`（若已完整 apply），再
   `settings::update` CAS：只有 latest rules 等于 `committed_rules` 才恢复 `previous_rules`。
6. CAS 输给 newer canonical 或任一 catalog rollback 报错时，读取 latest rules 并执行一次
   `sync_current_locked` 收敛 winner。收敛失败提升为
   `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED`；绝不能返回普通原始错误掩盖恢复失败。

### 9. Codex Home Contract

规则存储是设备 settings-owned，目录绑定是当前 resolved Codex-home owned；不按路径维护多份规则 map。

- `rules.iter().any(|r| r.enabled)` 时，普通 `settings_set/settings_patch` 改
  `codex_home_mode` 或 `codex_home_override` 必须在 settings/DB/file 首次写入前返回
  `CODEX_MODEL_CONTEXT_RULES_HOME_CHANGE_BLOCKED`。保留现有 lock 顺序：home intent 先拿 Profile
  lifecycle lock，再进入 autostart/settings（`src-tauri/src/app/settings_service.rs:1736-1779`）。
- 全部 rules disabled 时允许 home change；rules vector 原样保留。新 home 不因 disabled rules 生成
  catalog，未来显式启用才按该 home 的 authoritative base 验证。
- config import 先恢复 canonical pre-import rules，再判断 home guard；incoming rules 永远不能通过
  “先清空规则再换 home”绕过。
- `FollowCodexHome` 模式下外部环境变量可能在 settings 不变时改变 resolved home
  （`src-tauri/src/infra/codex_paths.rs:85-119`）。v3 metadata 的 `codex_home_key` 必须检测该漂移：
  enabled rules 下返回 `CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT`、零写入，不把新 home config 绑定到
  旧 global generated file，也不清理旧 home。恢复路径是切回原 home、成功禁用规则，再切换。
- Proxy rebind 与 config import `_locked` rebind 继续沿用现有 committed tokens。enabled rules 的
  settings-driven home change在 rebind preparation前就失败；disabled-only 情况按现有顺序 rebind，
  外层 import 再用 preserved rules 做一次 catalog plan/apply。

### 10. Catalog Upgrade / Target Disappearance

Codex executable descriptor、用户 catalog bytes/path、Profile set、rules 和 home key任一改变都使 v3
projection cache miss并重新生成。对已 canonical enabled rule：

```text
base upgrade detected
  -> parse one new full base snapshot
  -> every enabled target cardinality == 1 and fields valid?
     yes: prepare complete replacement, drift recheck, atomic apply
     no:  return typed validation error before first write
          keep canonical rules + last committed generated/config/backup bytes
```

明确语义：

- 不自动删除/禁用规则；这会绕过 R21 的显式整集合提交。
- 不从旧 generated catalog 补回已消失条目到新 base，也不部分应用仍存在的规则。
- last-known-good generated catalog 保持绑定；startup 外层可继续报
  `CODEX_STARTUP_MODEL_CATALOG_RECONCILE_FAILED`，inner cause 保留 rule target code
  （`src-tauri/src/app/startup_settings.rs:59-76`）。这不是 recovery-required：没有 committed target
  丢失，只是 canonical policy无法针对新 base前进。
- 用户禁用/删除缺失规则后，disabled target不再检查；零 Profile/零 enabled 时可直接恢复 original
  binding并删除 generated，有 Profile 时从新 base重建仅 Profile catalog。
- 若原已接受的 enabled target以后重新出现，background reconcile可恢复到新 base，因为该规则始终
  是 canonical enabled intent。与之不同，曾在 SET 时因缺失而被拒绝的 candidate从未持久化，未来
  出现不会自动激活。

### 11. Portable Config Import / Export

导出：

```rust
let mut exported = serde_json::to_value(&settings::read(app)?)?;
let object = exported.as_object_mut().ok_or(...)?;
object.remove("codex_model_context_rules");
object.remove("codex_gpt56_372k_context_enabled");
```

不要只 clone `AppSettings` 后设 `rules = []`；R20 要求 key 不存在，且空数组仍暴露了 policy shape。

导入 prepare 必须在 `serde_json::from_value::<AppSettings>` 之前：

1. parse bundle settings 为 mutable JSON object；
2. remove `codex_model_context_rules` 和 `codex_gpt56_372k_context_enabled`，无论值是什么类型/大小；
3. 才反序列化和迁移 ordinary settings；
4. 获取 import/profile lifecycle locks 后，从 `previous_settings` clone canonical rules到
   `settings_to_write`（防止 preparation 与 commit 间规则 winner 漂移，whole-settings CAS仍决定输赢）；
5. 若 canonical 有 enabled rule 且 home changed，在 staged DB/Skill FS/settings/file 首次 mutation前失败。

后半段沿用现有顺序：settings/autostart commit -> applied home rebind -> catalog apply -> runtime sync ->
DB commit。失败补偿顺序固定为：catalog rollback -> home rebind rollback -> abort staged DB并通过现有
import rollback恢复 settings/autostart、Skill FS、runtime。所有分支都要尝试独立 token，恢复失败分别
提升 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`、`CLI_PROXY_REBIND_RECOVERY_REQUIRED` 或
`CONFIG_IMPORT_RECOVERY_REQUIRED`。

### 12. Error Code Matrix

建议新增并稳定映射：

| Code | Condition | Mutation |
| --- | --- | --- |
| `CODEX_MODEL_CONTEXT_RULE_LIMIT` | 输入超过 128 条 | zero write |
| `CODEX_MODEL_CONTEXT_RULE_MODEL_INVALID` | trim 后空、>256 UTF-8 bytes、含控制字符 | zero write |
| `CODEX_MODEL_CONTEXT_RULE_RESERVED_TARGET` | literal `aio/` prefix | zero write |
| `CODEX_MODEL_CONTEXT_RULE_VALUE_INVALID` | token 不在共享 `1_024..=10_000_000` 整数范围 | zero write |
| `CODEX_MODEL_CONTEXT_RULE_DUPLICATE` | 规范化后同一 case-sensitive ID 重复（含 enabled/disabled） | zero write |
| `CODEX_MODEL_CONTEXT_RULE_TARGET_NOT_UNIQUE` | 任一 enabled ID 在同一 base 中命中 0 或 >1 次 | zero write |
| `CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID` | 唯一命中项的两个 window 字段不是 JSON 非负整数 | zero write |
| `CODEX_MODEL_CONTEXT_RULES_HOME_CHANGE_BLOCKED` | settings/import 请求在 enabled rules 下改 home | zero write |
| `CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT` | FollowCodexHome 或外部状态令 persisted owner home 与 current home 不同 | zero write |
| `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED` | dedicated SET 的 file/settings rollback或 winner reconcile失败 | partial state possible; manual recovery |

继续复用 managed catalog 通用码：base unavailable/invalid/drift、CLI missing、bundled timeout/invalid、
catalog modified、config drift/write failed、catalog write failed、catalog recovery required。structured/raw
config save仍可用外层 `CODEX_CONFIG_MANAGED_CATALOG_SYNC_FAILED` / `_RECOVERY_REQUIRED` 包装 inner rule
cause；startup保留 `CODEX_STARTUP_MODEL_CATALOG_RECONCILE_FAILED` 并记录 inner code。

`TARGET_NOT_UNIQUE` 的 message应包含经过安全长度限制的精确 `model_id` 和 observed count，不能复用
旧 `CODEX_GPT56_372K_MODELS_MISSING`；新后端不得残留 GPT family/372000 special policy。

### 13. Test Entry Points

#### Settings schema / migration

- `src-tauri/src/infra/settings/migration.rs`：64 false/true映射、排序、幂等、legacy wrong-type、schema 65
  preserve、128/129、byte/control/aio、token min/max/out-of-range、全局 duplicate。
- `src-tauri/src/infra/settings/persistence.rs` 与 `src-tauri/tests/settings_crud.rs`：canonical key移除、第二次
  read bytes相同、repair persistence/finalize/recovery failpoints不得产生 transient enabled success。
- `src-tauri/src/app/settings_service.rs`：ordinary owner preserve、整集合专属 SET、settings-commit 后 apply
  failure、confirmation failure、vector CAS winner、home writer/lifecycle serialization。

#### Catalog generator / transaction

- `src-tauri/src/infra/codex_model_catalog/managed.rs` 现有测试入口从 `:2174` 开始；把
  `gpt56_policy_*` 泛化为多规则 exact overlay、disabled target、high/equal/low value、missing/duplicate/
  invalid target、unknown-field和 `aio/*` preservation、顺序稳定 hash、v2 -> v3 metadata。
- 复用 `managed.rs:2923-3151` 的 apply/rollback trace和 fault injection；增加纯 prepare不创建目录、
  settings token drift before apply、home key drift。
- 增加 user base bytes升级后 target消失/重现，以及 bundled descriptor变化后 target消失；失败时对
  settings/generated/live/backup做 byte-exact断言。

#### Cross-entry reconciliation

- `src-tauri/src/infra/codex_config/tests.rs` 和 `src-tauri/tests/codex_provider_sync.rs`：structured/raw
  proposed source、missing target zero commit、live config与proxy backup独立 rollback、history failure后
  catalog回滚。
- `src-tauri/src/infra/config_migrate/tests.rs`：export key absent；import missing/empty/different/malformed/
  oversized rule payload全部 ignored；canonical rules保留；enabled home block；disabled-only home rebind；
  runtime/DB commit failure按 catalog -> rebind -> import state顺序补偿。
- `src-tauri/src/infra/cli_proxy/tests.rs`：enable/sync/disable/exit/offline/startup、零/多个 Profile、规则关闭但
  Profile保留、Profile/capability update、FollowCodexHome drift。
- `src-tauri/src/app/startup_settings.rs`：target missing保留last-known-good并记录outer + inner code。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs` / candidate API tests：base `slug`/display/context解析、
  suggestions unavailable仍可读取 canonical rules、candidate GET零 sync/零 write。

建议定向命令（实现后按实际 test module filter微调）：

```powershell
Push-Location src-tauri
cargo test --lib settings::migration::tests
cargo test --lib codex_model_catalog::managed::tests
cargo test --lib app::settings_service::tests
cargo test --lib infra::codex_config::tests
cargo test --lib infra::config_migrate::tests
cargo test --lib infra::cli_proxy::tests
cargo test --test settings_crud
cargo test --test codex_config_toml_raw
cargo test --test codex_provider_sync
Pop-Location
pnpm check:generated-bindings
pnpm exec vitest run src/constants/__tests__/crossLayerContracts.test.ts
```

全量门仍需 Rust fmt/check/clippy/full test、generated bindings、前端 typecheck/lint/test和Linux CI。
真实 Codex集成只能作为额外 smoke：隔离 `CODEX_HOME`，规则 SET 后运行不带 `--bundled` 的模型
读取，验证双字段；CI核心语义必须用绝对 user catalog fixture，不能依赖开发机安装 Codex。

### 14. External References

- 本次遵守任务约束，未访问网络、Git `upstream` remote或外部 worktree。
- 复用已归档调研固定的外部证据：OpenAI Codex `rust-v0.147.0`，commit
  `be6e8eac029b183056b7e4402879f15d2c85f61b`；其 bundled JSON使用十进制 `272000`，
  `model_catalog_json`在新进程启动时加载。来源记录在
  `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/runtime-dataflow.md`，
  本次未重新外部验证。

### 15. Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md:201-245`：完整
  catalog/Profile capability/base source/三文件 transaction。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md:507-703`：待本任务
  替换的专属 372K policy合同；通用规则应保留其 lifecycle/rollback部分，删除 family-specific部分。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md`：exclusive settings
  owner、owned token、CAS winner和recovery-required升级规则。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`：raw/structured proposed base与
  proxy baseline三方合并。
- `.trellis/tasks/08-17-codex-custom-model-context-rules/prd.md:17-38`：当前任务权威产品要求。

## Caveats / Not Found

- PRD同时使用“设备/Codex-home本地策略”，但没有要求按多个home分别存储规则。本研究按现有
  `AppSettings`形态解释为“设备上一个canonical集合，激活绑定当前home”；disabled rules换home后保留。
  若产品要每个home各一套规则，需要全新的path-keyed schema和切换UI，不能从本方案隐式推导。
- v2 owner metadata没有 Codex-home identity。正常active direct配置会绑定generated path，可作为一次性
  v3认领证据；若用户在升级应用前同时改变外部 `CODEX_HOME`，旧metadata无法证明原home。应fail
  closed，不应猜测迁移。
- 当前 bundled cache fingerprint依赖executable descriptor而非直接catalog hash
  （`managed.rs:1392-1425`）。对静态bundled资源足够，但不能宣称检测同descriptor下的外部动态变化。
- 当前普通 `model/list`协议不返回base context字段，因此“基础值 -> 目标值”需要新增只读base
  candidate parser；不能从active generated catalog或 `CodexModelCatalogState.models`伪造基础值。
- 现有 prepare创建managed catalog目录，以及settings repair持久化错误被忽略，均与本任务更强的
  zero-side-effect/durable-migration合同冲突；实现时必须显式修复并加故障注入测试。
- 进程内lifecycle lock和桌面single-instance不能保证多个AIO进程共享同一Codex home时强串行化。
  文件snapshot/owner hash只能在检测到漂移后fail closed；没有新增OS级文件锁时不要承诺跨进程事务。
- WSL Codex目录仍未进入当前managed catalog管线；本研究不把Windows侧generated path视为Linux
  Codex可用路径，WSL支持需独立任务。
