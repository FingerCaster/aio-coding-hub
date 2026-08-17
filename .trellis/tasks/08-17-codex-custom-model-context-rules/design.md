# Design: Codex 自定义模型上下文规则

## Decision Summary

把已发布的 GPT-5.6 372K 布尔策略升级为一个设备本地、可持久化、精确匹配的规则集合。后端只认识通用规则，不保留 GPT-5.6、模型族或 `372000` 特判；原三模型快捷项仅作为前端草稿预设。

规则以 Codex 基础 catalog 中 `models[].slug` 为唯一目标键，区分大小写、逐字节精确匹配。每条规则保存一个十进制 `context_window`，生成目录时同时覆盖目标条目的 `context_window` 和 `max_context_window`。`272K`、`372K` 继续按上游十进制口径解释为 `272000`、`372000`。

规则集合继续复用现有 managed catalog 的单一事务和所有权边界。settings、generated catalog、live `config.toml`、proxy backup、Profile 与 provider capability 不建立第二条同步路径。

## Persistent Contract

### Schema 65

`AppSettings` 从 schema 64 升到 65，删除旧 canonical 字段 `codex_gpt56_372k_context_enabled`，新增：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct CodexModelContextRule {
    pub model_id: String,
    pub context_window: i64,
    pub enabled: bool,
}

pub codex_model_context_rules: Vec<CodexModelContextRule>
```

不增加规则 ID、优先级、匹配类型、模型族字段或规则集总开关。`model_id` 是集合内的唯一键；删除即从集合移除。

### Canonical normalizer

所有持久化、迁移、专属 SET 和 catalog policy 构造共享一个严格 normalizer，顺序固定为：

1. 集合最多 128 条。
2. 对 `model_id` 做 Unicode `trim`，结果必须为 1 到 256 个 UTF-8 bytes。
3. 拒绝任意 Unicode 控制字符和 literal、区分大小写的 `aio/` 前缀。
4. `context_window` 必须是 `1_024..=10_000_000` 的整数，并复用 provider capability 的 Rust/TypeScript 常量与跨层漂移测试。
5. 在启用和禁用规则的合并集合中检查规范化 ID 全局唯一。
6. 按 `model_id.as_bytes()` 确定性排序。

禁用规则执行全部静态校验，但不要求目标存在。写入口严格失败，不做静默丢弃、截断或自动修复。

### Field ownership and IPC

- `SettingsView` 只读暴露 canonical `codex_model_context_rules`。
- 普通 `SettingsUpdate` / `SettingsPatch` 不拥有也不携带规则；普通 settings owner 只把完整规则集放入 compare-only token。
- 唯一写入口为整集合专属命令：

```rust
settings_codex_model_context_rules_set(
    rules: Vec<CodexModelContextRule>,
) -> SettingsView
```

- 命令返回 trim、排序、持久化、catalog 应用和 canonical 确认后的后端状态。前端不得用请求草稿伪造成功结果。

## Migration And Portable Configuration

### Schema 64 to 65

迁移必须在 typed migration 改写旧布尔之前，从原始 settings JSON 捕获 schema 版本和旧字段：

```text
schema 64 + legacy absent/false -> []
schema 64 + legacy true ->
  gpt-5.6-luna  / 372000 / enabled
  gpt-5.6-sol   / 372000 / enabled
  gpt-5.6-terra / 372000 / enabled
```

- schema 64 的旧键存在但不是 JSON boolean 时 fail closed，不猜成 false。
- 更老 schema 和缺失 schema 沿用历史默认 false，再进入 schema 65。
- preset 走同一个 normalizer；第二次读取不得重复添加或改变 bytes/hash。
- migration repair 的原子持久化错误必须向上传播。不得在只有内存态 schema 65、磁盘仍为旧状态时继续应用 catalog policy。
- schema 65 canonical JSON 不再序列化旧键，不形成两个真源。

### Import and export

- 导出在 raw JSON object 上完全删除新规则键和旧开关键，不以空数组代替省略。
- 导入在反序列化 `AppSettings` 前删除两种键。输入中的空、不同、超限、恶意类型或伪造 disabled 值都不进入 validator，也不能成为拒绝导入的 DoS 面。
- 导入在锁内读取并保留导入前 canonical 规则，再计算 active rules、Codex home 变化与 catalog plan。
- 导入失败补偿保留并发规则 writer 的较新 winner，不用旧 whole-settings snapshot 覆盖它。

## Read-Only Base Catalog Candidates

现有 `cli_manager_codex_model_catalog_get` 会先 reconcile 派生目录，且 `model/list` DTO 不含基础窗口，不能用于规则建议和失配恢复。新增独立的只读 IPC：

```rust
pub struct CodexModelContextCandidate {
    pub model_id: String,
    pub display_name: String,
    pub hidden: bool,
    pub base_context_window: Option<i64>,
    pub base_max_context_window: Option<i64>,
}

pub struct CodexModelContextCandidatesState {
    pub status: CodexModelCatalogStatus,
    pub issue: Option<CodexModelCatalogIssue>,
    pub snapshot: CodexModelCatalogSnapshot,
    pub models: Vec<CodexModelContextCandidate>,
}

cli_manager_codex_model_context_candidates_get()
    -> CodexModelContextCandidatesState
```

该 GET 在 profile lifecycle lock 下检查当前 home/owner 后直接读取原始 base source：

- 不调用 `sync_current_locked`，不写 config，不创建 managed 目录或文件。
- 使用 `models[].slug`，不使用 `model/list` alias；过滤 literal `aio/`。从基础条目投影 `hidden`，未知 visibility 保守映射为 hidden。
- 缺少显示名时回退到 slug；窗口字段不可读时返回 `None`，仍可作为搜索建议。
- UI 用全量 candidate map 为 canonical/预设规则做基础值比较，只有 `hidden=false` 的条目进入可搜索建议。
- candidate 只用于提示和 base 对比。SET 事务必须重新读取权威基础目录并完成全量验证。
- GET 不可用时，手工输入与规则修复仍可工作；严格 settings 读取失败则整个编辑器进入只读保护。

`managed_catalog_path` 等路径 helper 必须拆分为纯路径计算与 commit-time 目录创建，确保所有 prepare/GET 路径零副作用。

## Managed Catalog Policy

### Policy and generation

`ManagedCatalogPolicy` 改为持有已 canonicalize 的完整规则集合，不再是 `Copy` 布尔：

```text
enabled_rules = rules where enabled == true
needs_generated_catalog = profiles non-empty || enabled_rules non-empty
```

生成顺序：

1. 读取并验证完整 base root、`models` 数组、条目 object、slug 与全局唯一性，保留现有大小/数量上限。
2. 对每条 enabled rule 要求其精确 slug 恰好命中一次，且两个基础窗口字段均为非负 JSON integer。
3. 将规则唯一 token 值同时写入两个字段；高于、等于、低于基础值均允许。
4. 不改 `effective_context_window_percent`、自动压缩、reasoning 或未知字段。
5. 完成普通规则投影后追加 `aio/*` Profile。其上下文仍只来自 provider-model capability。

任一 enabled target 缺失、重复或结构无效时不产生部分 output。全部规则 disabled 且没有 Profile 时，不读取 base 即可准备恢复原始 binding 和删除 owned generated file。

### Owner metadata v3

owner metadata 升到 v3，至少覆盖：

- owner schema 与 projection algorithm version；
- 完整 canonical rule-set hash；
- enabled `{model_id, context_window}` 投影及其 hash；
- Profile set/hash；
- base source fingerprint；
- normalized Codex-home identity；
- original catalog binding；
- payload/projection hash。

相同语义规则的不同输入顺序必须生成相同 settings bytes、owner hash 和 catalog bytes。v1/v2 只用于验证和安全恢复；不能证明 home identity 或 original binding 的旧 owner 必须 fail closed。第一次成功 reconcile 将等价旧目录重建为 v3，不先删除仍在使用的 generated catalog。

## Atomic Rule Transaction

专属整集合 SET 持有 profile lifecycle lock，并采用唯一流程：

```text
read canonical settings + profiles + current home/owner
  -> normalize and validate complete candidate rules
  -> prepare full catalog/binding plan with candidate policy (zero writes)
  -> conditionally commit complete canonical rules
  -> apply prepared baseline/generated/live-config targets
  -> reread and confirm canonical rules, owner, projection and binding
  -> return confirmed SettingsView
```

失败补偿顺序：

1. 逆序回滚所有已经应用且仍等于本事务 after-bytes 的 catalog/config/proxy 目标。
2. 仅当 canonical 规则仍等于本事务 committed token 时 CAS 恢复 previous rules。
3. 若 CAS 输给较新规则 winner，保留 winner 并按 winner 重新 reconcile。
4. 任一目标无法证明所有权或无法收敛时返回 recovery-required，不覆盖外部 writer。

激活/刷新继续使用 `proxy baseline repair -> generated -> live config`；停用继续使用 `proxy baseline repair -> live config restore -> generated delete`。所有 caller 复用同一 plan/apply/rollback 实现。

## Lifecycle State Machine

设 `E` 为 enabled rules，`P` 为 managed Profiles：

| 状态 | Generated catalog | 普通规则投影 | Codex home 变化 |
| --- | --- | --- | --- |
| `E` 空、`P` 空 | 不存在 | 无 | 允许 |
| 仅 disabled、`P` 空 | 不存在，规则仍保留 | 无 | 允许 |
| 仅 disabled、`P` 非空 | Profile 维持 | 无 | 服从 Profile 现有合同 |
| `E` 非空且兼容 | 存在 | 全量生效 | 拒绝实际变化 |
| `E` 非空但升级后不兼容 | 保留最后成功 bytes/binding | 不部分重建 | 拒绝实际变化并进入 typed failure |

以下入口都从最新 canonical rules 构造同一 policy：

- 应用启动恢复；
- structured/raw Codex config save；
- 普通 settings 与 Codex home writer；
- config import；
- CLI Proxy enable/disable/offline sync/exit restore/home rebind；
- managed Profile 创建、删除和 provider capability 更新。

任一 enabled rule 时，实际 home 变化在 settings 锁内拒绝；same-home no-op 仍通过 lifecycle lock 串行化。全部 disabled 时允许切换并原样保留规则。

先前成功提交的 enabled rule 因 CLI/base 升级失配时：

- prepare 在首次写入前失败，canonical intent、generated、live config 和 proxy backup 保持最后成功状态；
- startup 停在 `ReadingSettings`，外层为 `CODEX_STARTUP_MODEL_CATALOG_RECONCILE_FAILED`，保留 inner typed code，`can_retry=true`，不启动 Gateway 或后续 Proxy sync；
- 普通应用路由和规则编辑 IPC 不依赖 Gateway ready，用户可禁用、修正或删除后显式重试；
- 同一精确 ID 后来恢复时，下次启动或显式重试按持久 intent 自动收敛；
- 新 candidate 因缺失而被拒绝时从未进入 canonical，未来不得自行激活。

## Error Contract

前端只解析稳定 code，不解析英文 message：

- `CODEX_MODEL_CONTEXT_RULE_LIMIT`：超过 128 条。
- `CODEX_MODEL_CONTEXT_RULE_INVALID`：空/超长/控制字符/`aio/`/token 边界等静态错误。
- `CODEX_MODEL_CONTEXT_RULE_DUPLICATE`：trim 后精确 ID 重复。
- `CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING`：enabled 目标不存在。
- `CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID`：enabled 目标基数或窗口结构不安全。
- `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED`：专属事务无法恢复；保留 inner code。
- 继续复用 `CODEX_MANAGED_MODEL_*_DRIFT`、`CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID` 和 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` 表达共享所有权边界。

错误详情最多包含一个经过验证且有界的模型 ID 和阶段名，不包含完整 catalog、任意输入列表或敏感绝对路径。

## Frontend Design

在 Codex 设置页用独立 `CodexModelContextRulesSection` 替换旧 372K Switch，避免继续扩大 `CodexTab` 内部状态。常驻摘要只显示“启用 X / 共 Y 条”和“管理规则”命令；完整草稿在受控 Dialog 中编辑。Dialog 采用单层、可扫描的规则表格/列表，不嵌套卡片：

- 模型列为可搜索建议输入，同时允许手工输入精确 ID。
- token 列为数字输入，显示千位分隔后的基础值与目标值。
- 每行使用 Switch 表达 enabled，垃圾桶图标删除，均有可访问 label/tooltip。
- 命令区提供“添加规则”“添加 GPT-5.6 372K 预设”“取消”“应用更改”。预设只补入不存在的三个 ID，并报告跳过项，不提交后端。
- base 值可读时显示 `基础值 -> 目标值`；目标提高时显示非阻塞风险提示。base 未知或目标不可用时不伪造对比。
- canonical enabled rules 非空时禁用 Codex home 控件；全部 disabled 时允许切换。

草稿只从最后确认的 settings snapshot 建立。编辑、启停、删除都只改本地草稿；显式应用才发送一次整集合 SET。前端复用 provider context 常量并校验 safe integer、数量、ID bytes/control/`aio/` 与重复；后端仍是最终权威。

严格状态规则：

- settings GET 失败或只剩 stale cache 时只读，清除 pending write；candidate GET 失败不阻止手工编辑。
- pending 时禁止重复提交与冲突控制。
- 取消或本地校验失败时直接从当前最后确认 snapshot 重置草稿；SET reject、空响应或 canonical 确认不一致时必须 await 严格 settings 回读，再从回读结果重置。回读也失败时转为只读保护；任何路径都不能保留伪成功草稿。
- 成功后以命令返回值更新 canonical，再重置/失效 settings、Codex config、candidate/base catalog 和 model catalog query；通知只在确认后显示。
- startup 失配时 Banner 的“打开设置”能定位到该编辑器；保存修复不会隐式启动 runtime，用户显式点击重试。

## Data Flow

```text
SettingsView canonical rules + read-only base candidates
  -> editor confirmed snapshot + local draft
  -> normalize/validate complete draft
  -> dedicated SET under lifecycle lock
  -> prepare candidate policy from authoritative base
  -> settings token commit -> catalog apply -> canonical confirmation
  -> returned SettingsView -> query reset -> confirmed editor state
```

```text
Startup / config / proxy / profile lifecycle event
  -> read canonical rules + profiles
  -> prepare one managed catalog policy
  -> compatible: atomically converge v3 owner/generated/binding
  -> incompatible: zero writes, retain last-good state, typed retryable failure
```

## Compatibility And Rollback

- schema 64 `false/true` 映射为空集合/三条通用规则，保持已发布 Beta 9 用户意图。
- v1/v2 owner 可安全读取和恢复；只有成功确认后升级为 v3。
- 用户自定义 base catalog 的未知字段和非目标模型保持语义不变。
- 规则只影响新启动的 Codex 会话；不承诺热更新已运行进程。
- 源码回滚仅限独立 worktree。产品回滚通过禁用/删除规则触发统一恢复；若没有 Profile，恢复原始 binding 并删除 owned generated file。
- 降级到不理解 schema 65 的旧版本不保证保留规则编辑能力；发布前必须完成升级/重复启动测试，并保留用户原始 catalog 与 AIO settings 原子备份恢复路径。

## Rejected Alternatives

- 为 GPT-5.6 保留后端特判或布尔开关：形成第二真源，无法扩展任意模型。
- 支持 prefix/glob/regex：未来模型可能被静默扩大匹配，破坏精确验证。
- 分别配置两个窗口字段：引入无产品价值的非法组合。
- 复用 `model/list` 候选：读取的是运行时/派生结果，缺少权威 base 值，并会让失配恢复依赖成功 reconcile。
- 把设备规则放入便携配置：跨机器 catalog 不同，可能在导入时隐式激活不兼容策略。
- 升级失配时自动禁用或删除：丢失明确的持久用户意图。
- 修改 Codex bundled catalog 或顶层 `model_context_window`：前者不归 AIO 所有，后者会被逐模型 max clamp 且可能影响无关模型。

## Release Contract

- 只读取和操作 `origin` / `FingerCaster/aio-coding-hub`，不访问 `upstream`。
- 本地检查全部通过后生成 Windows x64 MSI，记录绝对路径、bytes 与 SHA-256。
- 功能分支推送后创建 PR，等待 final head required CI 成功，再合并并记录 40 位不可变 merge SHA。
- 发布前重新读取 Beta promotion high-water，确认候选 tag/ref/Release 全部不存在，选择严格更高的下一个 Beta。
- `release.yml` 显式使用 `release_channel=beta`、canonical tag 和 merge SHA；所有 build/promotion/publication 使用同一 SHA。
- 最终核验 public prerelease flags、14 项资产、签名、四平台 manifest、tag/source identity 和 release-channels pointer；stable latest 与 Homebrew 保持不变。
