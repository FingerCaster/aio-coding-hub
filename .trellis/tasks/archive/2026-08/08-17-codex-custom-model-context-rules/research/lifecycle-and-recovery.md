# Research: Codex 自定义模型上下文规则的生命周期与恢复契约

- Query: 在现有 GPT-5.6 372K managed catalog 事务基础上，确定通用模型上下文规则在 Codex CLI 升级、Codex home 变化、应用启动、CLI Proxy 启停/退出、配置导入及失败补偿中的状态机、错误契约和测试矩阵。
- Scope: mixed
- Date: 2026-08-17

## Findings

### 1. 结论

1. 通用规则应继续复用现有 `prepare -> canonical intent commit -> catalog apply -> canonical confirmation` 事务，不应建立第二条目录写入路径。`ManagedCatalogPolicy` 从一个 372K 布尔值扩展为完整、不可变、已规范化的规则集合；所有调用者都把同一份 policy 交给同一 reconciler。
2. 生命周期中的 `active rules` 必须精确定义为“至少一条 `enabled=true` 的 canonical 规则”。只有禁用规则时，规则集合仍需持久保留和校验，但不参与目录派生，也不单独维持 generated catalog。
3. CLI 升级后，如果任一已启用规则在新的基础目录中缺失、重复或窗口字段结构无效，reconcile 必须在首次写入前失败。canonical 规则、generated catalog、live `config.toml` 和 proxy backup 均保持提交前状态；不得静默禁用、删除规则或生成部分 overlay。
4. 启动时遇到上述不兼容，应保持现有 fatal-but-retryable 语义：停在 `ReadingSettings`，不启动 Gateway 或后续 CLI Proxy 同步，并返回外层 `CODEX_STARTUP_MODEL_CATALOG_RECONCILE_FAILED`，其中保留内层稳定错误码。静默降级会造成 `enabled intent != applied catalog`，不可接受。
5. 无需为普通启动失败新建一个绕过事务的“强制恢复”写接口。当前非 maintenance 启动失败仍挂载正常路由，Banner 已提供“打开设置”和“重试启动”；规则编辑器只要不依赖 Gateway ready，就能禁用、修正或删除问题规则，再由用户显式重试启动。应补端到端回归证明这条恢复路径可用。
6. 任一 active rule 时，实际 Codex home 变化必须在 settings 锁内被拒绝；只有禁用规则时允许切换 home。禁用规则随设备 canonical 状态保留，不会因 home 切换自动启用。导入载荷伪造空规则或全禁用规则不能绕过该判断。
7. Proxy enable/disable/offline sync/exit restore/home rebind 必须继续把 catalog reconcile 纳入同一补偿事务。普通目录不兼容且补偿成功时返回 `CLI_PROXY_MANAGED_MODEL_SYNC_FAILED` 并恢复此前代理状态；managed catalog 已进入 recovery-required，或代理补偿失败时，提升为对应动作的 `*_RECOVERY_REQUIRED`。
8. 配置 bundle 不拥有规则。导出 JSON 中应完全不出现规则字段；导入无论携带空、不同或恶意同名字段，都必须在锁内用导入前 canonical 规则覆盖。后续 home rebind、catalog apply、runtime sync 和 DB commit 仍按现有顺序执行并统一补偿。

### 2. Files Found

- `.trellis/tasks/08-17-codex-custom-model-context-rules/prd.md` - 当前需求真源，定义精确匹配、整集合提交、启停语义、数值/数量边界以及 lifecycle 验收条件。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/design.md` - 已发布 372K 功能的事务设计和 lifecycle 接线基线。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/catalog-lifecycle-main.md` - 原始 binding、owner metadata、direct/proxy 模式和方向感知回滚的历史研究；文件头的最终决策已覆盖正文早期的 `380928` 建议，当前值为十进制 `372000`。
- `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/implementation-followups.md` - 已实现后的错误提升、启动恢复和设置所有权补充结论。
- `src-tauri/src/infra/codex_model_catalog/managed.rs` - managed catalog 的 prepare/apply、基础目录指纹、owner 校验、生成和条件回滚核心。
- `src-tauri/src/app/settings_service.rs` - 372K 专属设置事务、Home 互斥、普通 settings owner 和补偿逻辑。
- `src-tauri/src/app/startup_settings.rs` - 启动时 proxy repair 与独立 catalog reconcile。
- `src-tauri/src/app/startup_tasks.rs` - 启动阶段顺序、失败状态和 retryable 契约。
- `src-tauri/src/app/startup_state.rs` - `Failed`、`failed_stage`、`error_message` 和 `can_retry` 状态模型。
- `src-tauri/src/commands/app.rs` - `app_startup_retry` 重新进入启动 pipeline 的 IPC 入口。
- `src/App.tsx` - 非 maintenance 启动失败仍挂载普通应用路由。
- `src/components/app/AppStartupStatusBanner.tsx` - 启动失败时的错误展示、“打开设置”和“重试启动”入口。
- `src-tauri/src/infra/cli_proxy/mod.rs` - Proxy enable/disable/sync/exit restore 与 catalog reconcile 的组合事务及错误提升。
- `src-tauri/src/infra/cli_proxy/codex.rs` - Codex home rebind 与 catalog sync 的补偿事务。
- `src-tauri/src/infra/cli_proxy/tests.rs` - 372K direct/proxy/exit/offline/rebind/raw-save 回归，可泛化为通用规则测试。
- `src-tauri/src/infra/config_migrate/mod.rs` - 配置导入的锁序、settings CAS、home rebind、catalog apply、runtime/DB commit 和回滚聚合。
- `src-tauri/src/infra/config_migrate/tests.rs` - 设备本地 372K 状态、Home 互斥、导入补偿和错误码现有测试。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md` - Codex config、proxy baseline、managed catalog 共享所有权边界。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md` - settings 字段所有者、CAS token 和条件回滚要求。
- `.trellis/spec/aio-coding-hub/cross-layer/config-migration-skill-bundle-contract.md` - 配置导入锁、设备本地 managed 状态和全事务回滚要求。
- `.trellis/spec/aio-coding-hub/cross-layer/reliability-boundaries-contract.md` - startup retry、状态事件和 fail-closed 边界。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md` - `aio/*` managed profile 的单一权威关系；通用规则不得覆盖该命名空间。

### 3. 可直接复用的代码模式

#### 3.1 Prepare/apply 与方向感知文件提交

- `ManagedCatalogPlan::apply` 在写入前重新读取 ownership context，并重检基础目录 guard、live config 和 generated snapshot；prepare 后发生任何漂移都返回 `CODEX_MANAGED_MODEL_CONFIG_DRIFT`（`managed.rs:145-175`）。
- 激活/刷新顺序为 `proxy baseline repair -> generated -> config binding`；停用顺序为 `proxy baseline repair -> config binding restore -> generated delete`。因此不会先写指向不存在文件的 binding，也不会先删除仍被 binding 引用的文件（`managed.rs:178-240`）。
- 每个 apply stage 都记录 before/after bytes；显式 rollback 按方向反序执行，并继续尝试其他目标而不是在第一个失败处停止（`managed.rs:350-428`）。
- 单文件 rollback 仅当当前 bytes 仍等于本事务 after-state 时恢复 before-state，否则返回 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`，避免覆盖外部 winner（`managed.rs:1062-1097`）。

该模式应原样扩展到规则集合。规则更新失败时，先回滚已应用 catalog，再只条件回滚本事务提交的完整规则集合；若规则字段已被较新的 writer 改写，则保留新 winner 并用其重新 reconcile。

#### 3.2 CLI 指纹会强制重新读取 bundled catalog

- 用户目录指纹覆盖绝对路径和内容 hash（`managed.rs:1331-1375`）。
- bundled catalog descriptor 覆盖 executable、runtime path、version、文件长度和 mtime；其 hash 用作基础来源 fingerprint（`managed.rs:1392-1425`）。
- prepare 只有在 profile hash、source fingerprint、projection hash、policy 和 original binding 全部一致时才复用 generated bytes，否则重新调用基础目录加载和 generator（`managed.rs:738-775`）。

因此 CLI 在同一路径升级、包装运行时变化、版本变化或二进制被替换后都会重新抓取 bundled catalog。通用规则必须进入 projection hash/owner metadata；若只比较旧 372K bit 或 profile hash，会错误复用旧投影。

#### 3.3 目标验证已经位于首次写入之前

- generator 先要求 JSON root object、`models` array、合法模型数量、每个 entry 为 object、非空 slug 且全局唯一（`managed.rs:1501-1589`）。
- 旧 372K overlay 只精确匹配三个目标；目标窗口字段必须为 `u64`，任一缺失/结构错误失败，任一目标缺失也失败（`managed.rs:1684-1733`）。
- `generate_catalog` 完成后 prepare 才构造 `generated_after/config_after`；所有实际文件写入发生在随后 `plan.apply`（`managed.rs:646-795`）。

通用实现应把规则集合的纯校验和 enabled-target 验证放在相同位置：

1. 所有规则先做集合级规范化：trim 模型 ID、UTF-8 字节长度、控制字符、`aio/`、token 整数边界、128 条上限、enabled/disabled 全局唯一和确定性排序。
2. 只有 enabled 规则参与基础目录命中；每条必须精确命中一个 entry，且两个窗口字段均为可安全覆盖的整数。
3. disabled 规则不要求目标存在，但仍执行第 1 步全部结构校验。
4. 任一失败时不生成部分 output，也不提交 candidate rules。

#### 3.4 Settings 专属事务已有正确骨架

- 当前专属 372K writer 在 profile lifecycle lock 内加载 profiles、用 candidate policy prepare，然后提交 settings bit、apply catalog、读取 canonical confirmation；失败时条件恢复 bit 和文件（`settings_service.rs:1588-1710`）。
- 普通 settings writer 在实际 home 变化且 372K active 时于 settings 更新 closure 内拒绝（`settings_service.rs:1036-1053`）。
- 所有携带 Home intent 的普通 writer，即使最后是 no-op，也先取得 profile lifecycle lock，防止 disable 暂态与 home move 交错（`settings_service.rs:1736-1779`）。

通用规则 writer 应把“一个布尔字段 token”替换为“完整 canonical rule collection token”，其他时序不变。普通 settings update/patch 不拥有规则字段；配置 bundle 则是特殊 whole-settings owner，但必须显式保留设备规则。

### 4. Canonical 状态机

#### 4.1 派生状态

设：

- `R` = canonical 规则集合，包含 enabled 与 disabled 规则；
- `E` = `R` 中 enabled 规则集合；
- `P` = managed Profile 集合；
- `G` = 是否需要 AIO generated catalog。

唯一派生公式为：

```text
G = (E 非空) || (P 非空)
```

| 状态 | Canonical rules | Generated catalog | 基础模型 overlay | Home 变化 |
| --- | --- | --- | --- | --- |
| 空集合，`P` 空 | 保留空集合 | 不存在 | 无 | 允许 |
| 仅 disabled，`P` 空 | 完整保留 | 不存在 | 无 | 允许，规则仍 disabled |
| 仅 disabled，`P` 非空 | 完整保留 | 由 Profile 维持 | 无通用规则 overlay；仅有 `aio/*` | 由 Profile 现有契约决定，规则本身不阻止 |
| 至少一条 enabled，全部兼容 | 完整保留 | 存在 | 每条精确目标的两个窗口字段写同一 token | 拒绝实际变化 |
| 至少一条 enabled，CLI/base 不兼容 | enabled intent 保留 | 保留提交前 bytes/binding | 不生成部分新 overlay | 拒绝实际变化；进入 typed failure |
| 回滚无法证明所有权 | 保留最新 canonical winner | 不猜测、不强行覆盖 | 未知，需恢复 | 所有相关 writer 返回 recovery-required |

`disabled` 是规则的持久状态，不是删除。切换 home 不得清空它们；未来模型重新出现也不得把 disabled 规则自动改成 enabled。重新启用必须重新走整集合原子验证。

#### 4.2 整集合规则更新事务

建议的唯一写入流程：

```text
acquire profile lifecycle lock
  -> read canonical rules + home identity + committed profiles
  -> normalize and validate the complete candidate collection
  -> prepare catalog/binding plan from immutable candidate policy (no writes)
  -> conditionally commit the complete canonical rule collection
  -> apply prepared catalog/binding plan
  -> confirm canonical rules + owner/projection hash + effective binding
  -> return confirmed canonical collection
```

失败补偿：

```text
catalog apply/confirmation fails
  -> rollback every applied file while after-bytes still match
  -> CAS rule collection from this transaction's committed token to previous token
  -> if CAS loses, preserve newer rules and reconcile that winner
  -> if any rollback/convergence cannot prove coherence, return recovery-required
```

重要区分：

- 新增/编辑 candidate 因目标缺失而失败时，candidate 从未进入 canonical；后续 CLI 再出现该模型不能自行激活这次被拒绝的变更。
- 已经成功提交的 enabled 规则后来因 CLI 升级暂时不兼容时，canonical enabled intent 仍保留并阻断启动；外部条件恢复后，用户显式重试或下次启动可以按该既有 intent 重新收敛。

### 5. CLI 升级与目录漂移契约

#### 5.1 升级成功路径

1. source fingerprint 变化，prepare 不复用旧 generated bytes。
2. 从新 bundled/user catalog 重新读取完整基础 JSON。
3. 逐条验证所有 enabled 规则恰好命中一次；从新基础目录派生完整 catalog。
4. owner projection hash 同时覆盖规范化规则集合、profiles、source fingerprint、original binding 和投影算法版本。
5. 原子刷新 generated；binding 路径不变。非目标字段和未知字段来自新基础目录，不从旧 generated 反推。

#### 5.2 升级不兼容路径

| 条件 | 结果 | 状态变化 |
| --- | --- | --- |
| enabled 目标缺失 | typed target-missing | 零 settings/file 写入 |
| enabled 目标重复 | typed target-ambiguous 或 base-catalog-invalid | 零 settings/file 写入 |
| 任一目标不是 object，或窗口字段不是非负 JSON integer | typed catalog-invalid | 零 settings/file 写入 |
| source 在 prepare/apply 间再次变化 | `CODEX_MANAGED_MODEL_BASE_CATALOG_DRIFT` | 零提交，或回滚已提交 stage |
| generated/live config/proxy backup 在 prepare/apply 间变化 | `CODEX_MANAGED_MODEL_CONFIG_DRIFT` | 不覆盖外部 writer |
| rollback 目标不再等于事务 after-state | `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` | 保留外部 winner，要求恢复 |

建议稳定错误码分层，而不是让 UI 解析英文 message：

- `CODEX_MODEL_CONTEXT_RULES_INVALID`：集合数量、ID、token、`aio/` 等纯输入错误。
- `CODEX_MODEL_CONTEXT_RULE_DUPLICATE`：candidate 中相同规范化模型 ID 重复，包括 enabled/disabled 之间重复。
- `CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING`：enabled 规则未在基础目录命中。
- `CODEX_MODEL_CONTEXT_RULE_CATALOG_INVALID`：enabled 目标重复或目标窗口结构无效；若实现沿用全局 base 错误，也必须在错误详情中给出有界模型 ID。
- 继续复用 `CODEX_MANAGED_MODEL_*_DRIFT` 和 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` 表示所有权/补偿边界。
- 专属整集合 writer 对无法恢复的失败可提升为 `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED`，但必须保留原始 inner code。

错误消息不得回显完整 catalog、任意绝对敏感路径或无界模型列表。最多包含有界、已验证的模型 ID 和阶段名。

### 6. Startup 状态机与恢复入口

#### 6.1 现状

- settings 读取成功后先执行 incomplete proxy enable repair，再独立 reconcile managed catalog（`startup_settings.rs:10-41,59-76`）。proxy repair 是 best effort；catalog reconcile 是启动门禁。
- catalog reconcile 在 profile lifecycle lock 内调用统一 `sync_current_locked`（`startup_settings.rs:59-70`）。
- reconcile 失败会调用 `fail_startup_run(... ReadingSettings ...)` 并立即返回；Gateway、CLI Proxy sync 和 WSL 收尾均不会执行（`startup_tasks.rs:60-91,94-105`）。
- 失败状态明确保存 `failed_stage`、`error_message` 并设置 `can_retry=true`（`startup_state.rs:75-82`）；现有单测验证外层启动错误码和 inner drift code（`startup_tasks.rs:188-216`）。
- 前端只把“尚未收到任何 startup snapshot”当作全屏 loading；非 maintenance 的 `Failed` 状态仍挂载 `NormalAppRuntime`（`src/App.tsx:34-56`）。Banner 可导航到设置并调用 `app_startup_retry`（`AppStartupStatusBanner.tsx:29-94`；`commands/app.rs:190-213`）。

#### 6.2 目标契约

```text
startup reads settings
  -> best-effort incomplete proxy repair
  -> catalog reconcile with committed rules
     -> success: continue Gateway and CLI sync
     -> incompatibility: Failed(ReadingSettings), canRetry=true
        -> show outer startup code + inner rule/catalog code
        -> keep normal settings route available
        -> user edits/disables offending rule via normal atomic writer
        -> user explicitly retries startup
```

不采用以下行为：

- 不把 incompatible enabled 规则临时当 disabled；
- 不在启动时删除规则或 generated owner；
- 不继续启动 Gateway 后仅显示 warning；
- 不在失败页直接改 settings JSON 或删除 catalog 文件；
- 不因重试而重复导入、重复迁移或生成重复规则。

规则查询/写入 IPC 必须只依赖已初始化 DB、settings 和文件事务，不能要求 Gateway ready。UI 应在 startup error 的 inner code 属于规则不兼容时，将“打开设置”定位到规则编辑器；保存成功后仍由用户点击“重试启动”，避免保存动作隐式启动其他 runtime owners。

### 7. Codex Home 状态机

#### 7.1 规则

```text
active = any(canonical_rule.enabled)

actual_home_change && active     -> reject before settings commit
actual_home_change && !active    -> allow; preserve all disabled rules
same_home intent                 -> allow/no-op, but still serialize on lifecycle lock
```

检查必须使用 settings 锁内的最新 canonical rules 和规范化后的目标 home，不能相信前端快照，也不能只在事务外读取一次。rule writer 与 home writer 的正确竞态结果是：

- Home writer 先完成：rule writer 在新 home 重新 prepare；若目标不存在则 candidate 不提交。
- Rule writer 先启用至少一条规则：后到的实际 Home change 在锁内被拒绝。
- 两者都不能观察到一个“rules 已提交到旧 home、settings 已切到新 home”的中间成功状态。

#### 7.2 Config save 与 Home 的区别

Structured/raw `config.toml` save 如果修改 `model_catalog_json`，不是 Home change，但会改变规则的基础目录。它必须在相同 lifecycle lock 内用 proposed config 重新 prepare：

- enabled rules 对新基础目录全部有效：保存用户字段、更新 original binding、刷新 generated，最后确认；
- 任一 enabled 规则无效：整个 config save 和 catalog transaction 失败，live config、proxy backup、generated、owner 和 canonical rules 全部保持原值；
- 只有 disabled rules且无 profiles：允许保存用户 binding，不生成规则 catalog。

现有 `codex_372k_raw_save_post_catalog_failure_restores_live_backup_and_generated_bytes` 已覆盖这类四目标补偿，可直接泛化（`cli_proxy/tests.rs:3024-3059`）。

### 8. CLI Proxy 生命周期与错误契约

#### 8.1 Enable/disable

- Codex enable 先持有 profile lifecycle lock，完成 backup、proxy projection 和 enabled manifest，再调用 catalog reconciler；reconcile 失败会恢复 proxy projection/manifest（`cli_proxy/mod.rs:2018-2195`）。
- Codex disable 先捕获完整 lifecycle snapshot，恢复 direct targets并写 disabled manifest，再 reconcile；失败会回滚 targets、manifest 和 catalog 生命周期（`cli_proxy/mod.rs:2240-2371`）。
- 对普通 incompatibility且 rollback 成功，两条路径都返回 `CLI_PROXY_MANAGED_MODEL_SYNC_FAILED`；若 inner managed error 是 recovery-required 或 rollback 失败，则分别提升为 `CLI_PROXY_ENABLE_RECOVERY_REQUIRED` / `CLI_PROXY_DISABLE_RECOVERY_REQUIRED`（`cli_proxy/mod.rs:2174-2194,2335-2371`）。

#### 8.2 Offline sync、exit restore、home rebind

- `sync_enabled(... apply_live=false)` 仍会在 direct/ProxyRestoredDirect 状态刷新 catalog；补偿失败提升 `CLI_PROXY_SYNC_RECOVERY_REQUIRED`（`cli_proxy/mod.rs:2759-3114`）。
- `restore_enabled_keep_state` 恢复 direct config但保留 manifest enabled，随后 reconcile；active rules 下 direct config 必须继续绑定 generated，普通失败回滚到此前 proxy-applied 状态，补偿失败提升 `CLI_PROXY_RESTORE_RECOVERY_REQUIRED`（`cli_proxy/mod.rs:3126-3256`）。
- Codex home rebind 的 catalog failure普通映射为 `CLI_PROXY_MANAGED_MODEL_SYNC_FAILED`，rebind/canonical rollback失败则为 `CLI_PROXY_REBIND_RECOVERY_REQUIRED`（`cli_proxy/codex.rs:114-193,260-297`）。

通用规则替换 372K bit 后，错误提升规则无需改变。关键是所有入口都从 canonical rules 构造同一 policy，不能在 proxy 模块复制规则匹配或自行忽略 disabled 状态。

#### 8.3 目标真值表

| Proxy 操作 | Enabled rules | 预期成功状态 |
| --- | --- | --- |
| enable | 有 | proxy live config 与 effective direct backup 都保留 generated binding |
| disable | 有 | manifest disabled；direct config 仍绑定 generated |
| exit/stop restore | 有 | manifest 仍 enabled；direct config 仍绑定 generated |
| offline sync | 有 | 不应用 Gateway URL，但按最新 base/rules 刷新 generated |
| 任意操作 | 仅 disabled、无 profiles | 不因规则保留 generated；恢复用户原 binding |
| 任意操作 | 仅 disabled、有 profiles | generated 只含 Profile 投影，不应用 disabled rule |
| 任意操作遇到 CLI 不兼容 | 有 | 操作失败并恢复操作前 proxy/catalog bytes；不部分成功 |

现有 `codex_372k_lifecycle_disable_*`、`exit_restore_*`、`offline_sync_*` 和 `offline_home_rebind_*` 测试已经证明事务骨架（`cli_proxy/tests.rs:2766-3021`）。应保留这些测试并将 fixture 改为通用规则，同时增加多规则和 disabled-only 行。

### 9. Config Import 状态机

#### 9.1 锁和提交顺序

现有导入顺序为：

```text
pure bundle preflight
  -> CONFIG_IMPORT_LOCK
  -> profile lifecycle lock
  -> update-channel lock
  -> read previous settings and preserve local policy
  -> prepare optional Home rebind
  -> stage DB + Skill FS
  -> whole-settings/autostart CAS
  -> apply Home rebind
  -> prepare/apply managed catalog
  -> sync CLI runtime
  -> commit DB
  -> finish staged FS
```

证据：锁序在 `config_migrate/mod.rs:668-686`；旧 372K bit 的保留与 active+home 拒绝在 `:705-725`；catalog prepare/apply、runtime sync 和 DB commit 在 `:912-1027`。

通用规则替换时必须保持：

1. 导入前 canonical rules 是唯一可信规则源。
2. 在锁内读取 `previous_settings` 后，立即把 `settings_to_write.rules` 替换为完整 `previous_settings.rules`，再计算 `active` 和 Home change。
3. 输入 bundle 中同名字段无论内容、schema version或 enabled 状态都不进入 candidate policy。
4. active+Home change 在 DB clear、settings CAS、Home 文件写入和 Skill FS activation 前失败。
5. disabled-only+Home change 可继续；规则原样保留，不会因目标在新 home 缺失而阻断，因为 disabled 不参与目录命中。
6. catalog plan 从导入事务内 candidate profiles + 已保留规则构造，保证 provider/Profile 变化和规则 overlay 一次收敛。

#### 9.2 失败和错误优先级

- runtime sync 或 DB commit 失败时，回滚顺序为 catalog -> Home rebind -> settings/DB/Skill FS/runtime（`config_migrate/mod.rs:982-1026`）。
- Home rebind rollback 或普通 import rollback 失败优先提升 `CONFIG_IMPORT_RECOVERY_REQUIRED`；只有 catalog rollback/recovery 失败时保持 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`（`config_migrate/mod.rs:619-665`）。
- 导入规则保留不是一个可回滚的“导入字段写入”，因为导入从未拥有它。若 whole-settings rollback 输给并发规则 writer，必须保留该 writer 的规则 winner，而不是恢复导入前旧集合。

导出端必须对序列化后的原始 bundle JSON 断言规则 property 不存在，而不是仅断言值为空。导入端则应对 absent、空数组、不同数组、超限数组、`aio/*`、非法 token 和伪造 disabled 状态全部证明“被忽略且 canonical rules 不变”；是否校验被忽略字段不应成为 DoS 面，最简单契约是只把它当不拥有的输入，不为其运行规则 validator。

### 10. 测试矩阵

#### 10.1 Catalog 单元测试

| 场景 | 必须断言 |
| --- | --- |
| 1 条/多条 enabled 规则 | 每个精确目标的 `context_window` 与 `max_context_window` 同值；非目标及未知字段 byte-semantic 保留 |
| 高于、等于、低于 base 值 | 全部允许；不修改 `effective_context_window_percent` 和自动压缩字段 |
| disabled 目标缺失 | 规则校验成功，不参与 overlay |
| enabled 目标缺失/重复/窗口字段非法 | 稳定 typed error；无部分 output |
| duplicate candidate ID | enabled/disabled 任意组合均在 catalog/file write 前拒绝 |
| ID/token/count 边界 | 1024、10000000、128 条成功；越界、129 条、控制字符、超长、`aio/*` 失败 |
| 排列不同但语义相同 | canonical bytes、projection hash、generated bytes 相同 |
| CLI executable/version/mtime/length 变化 | source fingerprint 变化并重新读取 base |
| owner/generated tamper | fail closed，不覆盖外部 bytes |

扩展现有：`gpt56_policy_fails_closed_for_missing_duplicate_or_invalid_targets`、`user_base_catalog_guard_detects_prepare_apply_byte_drift`、`bundled_base_catalog_descriptor_tracks_launch_and_executable_changes`、`catalog_file_transaction_*`（`managed.rs:2332-2386,2574-2620,2923-3143`）。

#### 10.2 规则整集合事务

| 场景 | 必须断言 |
| --- | --- |
| 一次草稿含 add/edit/enable/disable/delete | 仅一次 canonical commit和一次 catalog rebuild；返回后端确认集合 |
| candidate 中一个 enabled 目标缺失 | canonical rules、generated、live config、backup 均为提交前 bytes |
| apply stage 任一失败 | 文件反向回滚；完整规则 token 条件恢复 |
| confirmation 失败 | 已应用文件和规则 token 均恢复 |
| rollback 遇到较新 rule winner | 不覆盖 winner；按 winner 重对账 |
| target 后来出现在 CLI 中 | 先前未提交 candidate 不会出现或激活 |
| no-op 提交 | 幂等 reconcile，但不伪造规则变化或额外 UI 成功态 |

扩展现有 `dedicated_372k_toggle_*` 和 settings-owner 竞态测试（`settings_service.rs:2994-3143,3254-3589`）。

#### 10.3 Startup/CLI 升级

| 场景 | 必须断言 |
| --- | --- |
| CLI 升级且所有 enabled 目标仍兼容 | 新 base 未知字段进入 generated，规则值保持；启动继续到 Gateway |
| CLI 升级移除一个 enabled 目标 | `Failed/ReadingSettings/canRetry`；outer+inner code；Gateway/CLI sync 未启动 |
| CLI 升级改变目标窗口字段类型 | 同上；canonical 与四类文件零变化 |
| 只有 missing disabled 规则 | 不阻断启动，不维持规则-only generated |
| 启动失败恢复 | Banner 打开规则设置；禁用/修正规则无需 Gateway；显式 retry 达到 Ready |
| recovery-required ownership drift | 重试持续失败直到状态被明确恢复；不自动清文件 |

保留 `catalog_reconciliation_failure_stops_startup_with_typed_retryable_state`，并增加真实 reconciler + 前端 Banner/规则编辑器联动测试（`startup_tasks.rs:188-216`；`AppStartupStatusBanner` 现有测试目录）。

#### 10.4 Codex Home 与 config save

| 场景 | 必须断言 |
| --- | --- |
| 任一 enabled + structured settings Home change | 锁内拒绝，settings/home/files 不变 |
| bundle 伪造全 disabled + 旧 canonical active + Home change | 仍拒绝 |
| 全 disabled + Home change | 允许；规则集合原样保留且不自动启用 |
| enable 与 Home writer 两种交错 | 要么新 home prepare 成功，要么 Home 被 active rule 拒绝；无错配状态 |
| raw/structured save 换 base，enabled 全兼容 | 保存与 catalog 原子收敛 |
| raw/structured save 换 base，任一 enabled 不兼容 | live/backup/generated/owner/canonical 全回滚 |

扩展 `ordinary_settings_owner_rejects_codex_home_change_while_372k_is_enabled`、`codex_home_writer_waits_for_the_372k_catalog_lifecycle` 和 raw-save rollback 测试。

#### 10.5 Proxy

对 `rules={none, disabled-only, one-enabled, many-enabled}`、`profiles={none,present}` 至少覆盖以下操作：

- enable；
- disable；
- gateway-stopped offline sync；
- exit restore keep-state；
- Home rebind（仅 rules 非 active 时允许）；
- 每个动作中的基础目录不兼容；
- 每个动作中的 proxy/catalog rollback failure。

普通不兼容必须断言动作前 config/auth/backup/manifest/generated 完全恢复并返回 `CLI_PROXY_MANAGED_MODEL_SYNC_FAILED`；补偿失败必须断言对应 `ENABLE|DISABLE|SYNC|RESTORE|REBIND_RECOVERY_REQUIRED`，不能仍报告普通失败。

#### 10.6 Config import/export

| 场景 | 必须断言 |
| --- | --- |
| export 时 canonical rules 非空 | 原始 bundle JSON 中规则 property 不存在 |
| import 不含/含空/含不同/含恶意规则字段 | canonical rules byte-semantic 不变 |
| active + Home change | 在 destructive import 前拒绝，伪造字段不能绕过 |
| disabled-only + Home change | 导入成功，rules 保留，规则不维持 generated |
| catalog prepare/apply failure | settings/Home/DB/Skill FS/runtime 与 rules 回到正确 winner |
| runtime/DB commit failure after catalog apply | catalog 先回滚；错误码按现有恢复优先级聚合 |
| concurrent rule writer during failed import rollback | 新 rule winner 保留，不被 whole-snapshot rollback 覆盖 |

扩展 `config_import_does_not_own_codex_372k_policy`、`config_import_rejects_codex_home_change_while_372k_policy_is_enabled`、`config_import_runtime_failure_rolls_back_codex_home_rebind` 和 `config_import_failure_promotes_compensation_error_codes`（`config_migrate/tests.rs:446-725`）。

### 11. External References

- OpenAI Codex Configuration Reference: `https://developers.openai.com/codex/config-file/config-reference.md`（检查日期 2026-08-17）。官方将 `model_catalog_json` 定义为启动时加载的模型目录 JSON 路径，并允许 profile 覆盖该配置。
- 官方配置参考没有定义 model catalog JSON 的内部 schema，也没有承诺文件热加载。因此本项目不能把当前 `models`/窗口字段形状当作长期上游保证；结构变化必须 fail closed，用户文案继续明确“新启动的 Codex 会话生效”。
- 本机只读探测的 `codex-cli 0.147.0` bundled catalog 顶层只有 `models`，共 8 条；`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 均存在，两个窗口字段均为 JSON `Int64 272000`。这是版本观测，不是官方稳定 schema 契约。

### 12. Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`：规则 overlay 必须继续服从 direct/proxy config ownership、原始 binding 和安全保存约束。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md`：普通 settings writer 不拥有规则；专属 writer 与 config import 只能按 committed token 条件回滚。
- `.trellis/spec/aio-coding-hub/cross-layer/config-migration-skill-bundle-contract.md`：机器本地 managed 状态不随 bundle 迁移，导入失败必须跨 DB/settings/FS/runtime 一致补偿。
- `.trellis/spec/aio-coding-hub/cross-layer/reliability-boundaries-contract.md`：startup 失败保持 retryable，不能缓存失败为 ready 或绕过门禁。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md`：`aio/*` 上下文由 provider capability/Profile 独占，通用规则拒绝该前缀。
- `.trellis/spec/guides/cross-layer-thinking-guide.md`：设计与验证必须覆盖 Rust domain、IPC、前端草稿、持久化和 lifecycle 调用方的完整数据流。

## Caveats / Not Found

1. 官方资料未公开 catalog JSON schema/version，也未承诺热更新；当前字段验证只能基于已观测版本并保持保守失败。
2. 当前任务尚无 `design.md`，所以本文建议的通用规则错误码名称仍需在设计阶段定稿；错误分层、写入前失败和 recovery-required 提升语义不应改变。
3. 当前前端启动失败页可以进入普通设置，但通用规则编辑器尚未实现，也未证明其查询/写入不依赖 Gateway ready。这是选择“阻断 + 设置内恢复”前必须补齐的关键验收测试。
4. 现有普通启动失败会挂载完整 `NormalAppRuntime`，其中部分前端后台任务仍会运行。本文只要求规则恢复入口在 Gateway 未 ready 时可用；是否进一步限制其他降级态后台任务属于 startup reliability 的独立范围。
5. 如果外部 Codex 在 AIO 启动失败期间自行启动，它可能仍读取上一次成功绑定的 generated catalog。当前事务能保证不写出半成品，但不能控制外部进程；UI 应提示先关闭 Codex 后修复并重试。
6. 本研究没有修改代码、规范或 Git 状态，也没有执行全量测试；行号对应当前工作树，后续实现后可能漂移。
