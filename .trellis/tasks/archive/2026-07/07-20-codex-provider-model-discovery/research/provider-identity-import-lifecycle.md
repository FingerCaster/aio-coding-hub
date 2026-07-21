# Provider 身份、配置迁移与 Codex Profile 生命周期调研

## 调研范围

本文只回答本任务中的四个问题：

1. 当前数据库 `provider_id` 是否能作为跨机器、跨完整导入后的模型路由身份；
2. 若新增不可变 UUID，应覆盖哪些数据和迁移路径；
3. 完整配置导入、单供应商分享、供应商复制各自应如何处理身份与重复项；
4. `$CODEX_HOME/*.config.toml` 形式的用户级 Codex profile 是否已被现有迁移覆盖，以及同名文件如何避免覆盖用户数据。

## 结论摘要

- 当前 `providers.id` 是 SQLite `AUTOINCREMENT` 内部主键，不是稳定外部身份。完整导入会删除现有供应商，再以不指定 `id` 的方式插入新行；旧 ID 仅在本次事务的临时映射中用于回填引用。因此，同库重复导入和跨机器导入都不能依赖数值 ID 保持不变。
- AIO 模型别名若要写入 Codex profile，必须使用新的不可变 `provider_uuid`，例如 `aio/<provider_uuid>/<encoded-model-id>`；网关收到别名后用 UUID 查找当前数据库行，再走对应供应商的强制路由。数值 `provider_id` 继续作为数据库内部主键和外键即可。
- 身份语义应按操作区分：普通编辑保留 UUID；完整配置导出/导入保留 UUID；“复制供应商”和“单供应商分享导入”均生成新 UUID。后两者当前就是新增副本语义，不应悄悄变成跨机器更新同一实体。
- 现有完整配置包不包含 Codex profile 定义或任意 `$CODEX_HOME/*.config.toml` 文件。导入回滚虽会暂存 Codex 主 `config.toml`，但那只是 MCP/提示词运行时收敛的事务备份，不是 profile 的导出载荷。
- MVP 应把受管 profile 定义存入数据库并纳入完整配置包，profile 文件仅作为可重建的派生状态。目标文件已存在但无法证明由 AIO 管理时必须阻止写入并报告冲突，不能覆盖、自动认领或删除。

## 代码证据

### 1. 数值 provider ID 不稳定

- 新装表结构将 `providers.id` 定义为 `INTEGER PRIMARY KEY AUTOINCREMENT`，唯一业务约束只有 `(cli_key, name)`，没有不可变外部键：`src-tauri/src/infra/db/migrations/baseline_v25.rs:22-40`。
- 对外 `ProviderSummary` 也只暴露数值 `id`，没有 UUID 或稳定 key：`src-tauri/src/domain/providers/types.rs:287-324`。
- 完整导出确实把当时的数值 ID 写入 `ProviderExport.id`，并同时保存 `source_provider_id`：`src-tauri/src/infra/config_migrate/mod.rs:68-118`；查询从数据库读取这些字段：`src-tauri/src/infra/config_migrate/export.rs:52-91`、`src-tauri/src/infra/config_migrate/export.rs:107-151`。
- 但完整导入先执行 `DELETE FROM providers`，且没有重置或承诺 SQLite sequence：`src-tauri/src/infra/config_migrate/import.rs:692-715`。
- 重新插入供应商的 SQL 列表不含 `id`，随后用 `last_insert_rowid()` 取得新 ID：`src-tauri/src/infra/config_migrate/import.rs:95-181`。
- 旧 ID 只进入本次事务的 `old exported ID -> new imported ID` 临时表，用来回填 `source_provider_id`；并不会恢复为原数值：`src-tauri/src/infra/config_migrate/import.rs:227-268`。
- 排序模式导出主动使用 `(provider_cli_key, name)` 而非数值 ID 连接供应商：`src-tauri/src/infra/config_migrate/mod.rs:127-139`。这也是项目现有代码已经把 ID 视作导入后会变化的证据。

结论：即使某次向全新数据库导入时恰好得到相同数字，也只是插入顺序碰巧一致，不构成身份合同。把 `aio/<provider_id>/...` 写入长期存在的 Codex profile 会在恢复配置后静默指向错误供应商或失效。

### 2. 完整配置导入是替换，不是逐项合并

- `ConfigBundle` 当前 schema 为 3，载荷包含 settings、providers、排序模式、工作区、MCP、技能及图像生成配置，没有 profile 集合：`src-tauri/src/infra/config_migrate/mod.rs:17-66`。
- 导入在同一事务中清空主要配置表后重建：`src-tauri/src/infra/config_migrate/mod.rs:483-515`、`src-tauri/src/infra/config_migrate/import.rs:692-715`。
- UI 也明确承诺“导入将覆盖当前所有配置”，而非合并：`src/pages/settings/SettingsDialogs.tsx:109-118`。
- 当前导入结果只有各类记录数量，没有跳过项或文件冲突报告：`src-tauri/src/infra/config_migrate/mod.rs:218-228`。

因此，完整配置导入不应沿用分享导入的“重名追加副本”规则。新 bundle 中 UUID 重复、格式错误或 profile 文件冲突应在清库前 fail closed；合法 UUID 则原样保留。

### 3. 单供应商分享是新增副本语义

- 分享 envelope 的 provider 只包含 CLI、名称、启用状态、配置、认证和扩展，不包含数据库 ID：`src-tauri/src/domain/providers/share.rs:41-77`。
- 分享导出明确拒绝依赖另一个 `source_provider_id` 的供应商，因为单文件无法闭包该引用：`src-tauri/src/domain/providers/share.rs:1063-1079`。
- 导入遇到同 CLI 同名供应商时，会选择 `名称 副本`、`名称 副本 2` 等新名称：`src-tauri/src/domain/providers/share.rs:1157-1206`；测试也验证了重复导入得到副本名：`src-tauri/src/domain/providers/share.rs:2058-2068`。
- 分享导入执行新的 `INSERT` 并读取 `last_insert_rowid()`：`src-tauri/src/domain/providers/share.rs:1556-1601`、`src-tauri/src/domain/providers/share.rs:1684-1714`。
- 无论分享内容原本是否启用，导入 SQL 都强制写入 `enabled = 0`：`src-tauri/src/domain/providers/share.rs:1611-1617`；前端同样说明“新增一个默认禁用的供应商，不会覆盖现有配置”：`src/pages/providers/ProviderImportDialog.tsx:183-202`。

因此，分享包不应携带可直接恢复为本地身份的 `provider_uuid`。若未来为溯源需要携带，可命名为非身份字段（例如 `source_provider_uuid`），但导入仍必须生成新的本地 UUID；否则重复导入会在“覆盖、报冲突、条件性换 UUID”之间产生不可预测行为。

### 4. 本机复制同样是新增实体

- `provider_duplicate` 读取原供应商后，以 `provider_id: None` 调用创建路径并生成“副本”名称：`src-tauri/src/app/provider_service.rs:269-333`。
- 名称分配规则同样使用 `副本`、`副本 2`：`src-tauri/src/app/provider_service.rs:49-70`。

因此，复制必须生成新 UUID。若复制后沿用原 UUID，两个可独立编辑、启停和删除的数据库行会争用同一个外部模型路由身份。

### 5. 现有配置迁移没有覆盖用户级 profile 文件

- Codex 路径适配器只定义主文件 `$CODEX_HOME/config.toml`、`auth.json` 等固定路径，没有枚举 `*.config.toml` profile：`src-tauri/src/infra/codex_paths.rs:107-120`。
- 配置迁移的运行时备份只捕获 prompt target/manifest 与 MCP target/manifest：`src-tauri/src/infra/config_migrate/rollback.rs:249-264`。
- 对 Codex 而言，MCP target 明确只是 `$CODEX_HOME/config.toml`：`src-tauri/src/infra/mcp_sync/paths.rs:17-38`。
- 成功导入后的运行时同步也只同步 prompts、MCP 和 skills：`src-tauri/src/infra/config_migrate/rollback.rs:309-325`。
- 完整配置包确实能携带技能文件，但这是显式的 `installed_skills` / `local_skills` 文件载荷，不代表 Codex home 下的任意文件都会被迁移：`src-tauri/src/infra/config_migrate/mod.rs:58-65`、`src-tauri/src/infra/config_migrate/export.rs:521-578`。

因此，现状下即使供应商配置跨机器恢复成功，`grok.config.toml` 之类的 profile 也不会随之迁移；反过来，用户手工复制旧 profile 时若其中嵌入数值 ID，也会失效。

## MVP 推荐设计

### A. 两层身份

保留两种身份，各自只承担一种职责：

| 字段 | 用途 | 稳定性 |
| --- | --- | --- |
| `providers.id` | 本地数据库主键、内部 FK、日志与运行时状态 | 仅当前数据库实例内稳定 |
| `providers.provider_uuid` | AIO 模型别名、受管 profile、完整配置迁移中的外部身份 | 创建后不可变，跨完整导入保留 |

建议 `provider_uuid` 使用规范化的小写 UUIDv4 文本，数据库加 `UNIQUE NOT NULL` 约束。外部别名只使用 UUID，不能从名称、排序位置、Base URL 或模型 ID 推导供应商。

模型目录、profile 等表在本地仍以 `provider_id` 做 FK；只有会离开当前数据库或长期写入外部文件的引用才使用/导出 `provider_uuid`。网关解析 AIO 别名后执行 `provider_uuid -> providers.id` 查找，再进入现有强制供应商路由。

### B. 身份生命周期矩阵

| 操作 | UUID 行为 | 重名/重复行为 |
| --- | --- | --- |
| 新增供应商 | 生成新 UUID | 继续由 `(cli_key, name)` 阻止重名 |
| 编辑供应商 | 保留原 UUID，不允许客户端修改 | 名称仍可按现有约束修改 |
| 复制供应商 | 生成新 UUID | 继续自动生成“副本”名称 |
| 单供应商分享导入 | 生成新 UUID | 继续新增、自动副本名、默认禁用 |
| 完整配置导出/导入 | 导出并原样恢复 UUID | bundle 内 UUID 重复或非法时整体拒绝；不自动改 UUID、不追加副本 |
| 导入旧 schema 1-3 bundle | 每行生成新 UUID | 旧 `id -> new id` 映射继续兼容旧引用 |

完整导入是唯一保留 UUID 的跨机器路径。这使同一份 AIO 完整备份中的受管 profile 能继续使用原别名，同时不改变分享/复制的现有安全语义。

### C. 数据库与 bundle 迁移

1. 新数据库迁移增加 `provider_uuid`，为现有行生成唯一值后建立唯一、非空约束。迁移不得用当前数值 ID、名称或 URL 生成可猜且可变的身份。
2. 所有创建入口统一在领域层生成 UUID；更新入口不接受 UUID patch。这样普通创建、OAuth 创建、分享导入和复制不会遗漏。
3. `ProviderSummary` 至少增加只读 `provider_uuid`（或对前端命名为 `providerKey`）；网关 alias 解析和 profile 创建命令必须以后端查库为准。
4. 配置 bundle 升级到新 schema，并给 `ProviderExport` 增加 `provider_uuid`；新 schema 中该字段必填，旧 schema 反序列化时允许缺失并生成新值。
5. 新 schema 同时增加 `source_provider_uuid`。新包优先按 UUID 恢复 bridge/source 引用；仅旧包继续使用现有 exported numeric ID 映射及 CLI fallback。现有 fallback 会在找不到旧 ID 时选该 CLI 的首个供应商：`src-tauri/src/infra/config_migrate/import.rs:244-267`，不应成为新 schema 的身份规则。
6. 持久化模型目录若单独建表，数据库内继续 FK 到数值 `provider_id`；完整导出时嵌套在对应 provider 下或携带 `provider_uuid`。导入后重新绑定新数值 ID。

### D. Profile 应存定义，不迁移裸文件

建议新增 AIO 受管 profile 元数据，至少包含：profile 名称/文件名、所选 `provider_id`、上游 `model_id`、生成的 AIO alias、创建与更新时间。provider 删除继续按已确认产品决策由领域层阻止；数据库 FK 也应兜底限制悬空引用。

完整配置包导出 profile 定义，并在导入时通过 `provider_uuid` 重新绑定；不要把整个 `$CODEX_HOME/*.config.toml` 当任意字节复制。这样不会把用户在 profile 文件里手写的无关配置、机器路径或未来 Codex 私有字段误当成 AIO 数据。

文件生成应有独立 AIO manifest/sidecar，至少记录 `managed_by`、profile 名称、provider UUID、期望路径和最后生成内容的 hash。项目已有同类所有权先例：MCP manifest 记录 `managed_by` 与 `managed_keys`，并拒绝错误 owner：`src-tauri/src/infra/mcp_sync/manifest.rs:13-30`、`src-tauri/src/infra/mcp_sync/manifest.rs:70-100`。

### E. Profile 文件冲突策略

| 目标状态 | MVP 行为 |
| --- | --- |
| 文件不存在 | 原子创建文件，再原子更新 manifest |
| 文件存在，manifest 证明是同一 AIO profile，且内容 hash 未被外部改动 | 原子更新 |
| 文件存在，但无 AIO ownership | 阻止创建/导入，报告冲突路径，要求用户改 profile 名或显式移走文件 |
| 文件由另一个 AIO profile 拥有 | 阻止并列出双方 profile，不自动换名 |
| AIO 文件被用户手改，hash 不匹配 | 阻止覆盖/删除，要求用户确认如何处理 |
| 完整导入后旧 AIO profile 不再存在 | 仅当 manifest ownership 与 hash 都匹配时删除；否则保留并报告冲突 |

严格阻止已有未受管路径符合项目现有文件导入先例：本机 Skill 导入把原子 `create_dir` 视为 ownership claim，路径已存在就返回 `SKILL_IMPORT_DIR_ALREADY_EXISTS`，不会覆盖或认领：`src-tauri/src/infra/config_migrate/rollback.rs:629-653`。

对完整配置导入，建议在清空数据库前完成 profile payload 和目标路径预检；发现冲突即整体失败。若预检通过，则把 profile 文件/manifest 纳入现有 runtime backup、apply、rollback 生命周期。当前流程已在 DB commit 前同步运行时并在失败时回滚：`src-tauri/src/infra/config_migrate/mod.rs:598-643`。首版不建议“数据库导入成功但跳过若干 profile”，因为当前 `ConfigImportResult` 没有部分成功合同，且会留下 UI 显示存在但 Codex 文件未生成的分裂状态。

## 不推荐方案

- **在 alias 中使用数值 `provider_id`**：完整导入后不稳定，可能误路由而不只是报错。
- **用供应商名称作为 identity**：名称可编辑，且分享/复制会自动加后缀；不能满足不可变性。
- **单供应商分享保留 UUID 并静默更新同 UUID 行**：违反现有“新增、默认禁用、不覆盖”合同，也会让导入包含凭据的分享文件变成远程覆盖入口。
- **UUID 冲突时悄悄生成新 UUID**：会让已经写入 profile 的 alias 在不同机器上表现不同；完整导入应拒绝坏 bundle，分享导入则从一开始明确生成新 UUID。
- **扫描并打包所有 `$CODEX_HOME/*.config.toml`**：无法区分 AIO profile 与用户文件，也无法安全合并未知字段。
- **同名 profile 文件自动覆盖或自动认领**：可能破坏用户手工配置；必须先证明 ownership。

## MVP 验收与测试建议

1. 同库先创建供应商和 profile，再执行完整导出/导入：数值 ID 允许改变，但 `provider_uuid`、profile alias 和最终路由供应商必须不变。
2. 把同一 bundle 导入另一空数据库：UUID 保持一致，内部数值 ID 不参与断言。
3. 导入 schema 1-3 bundle：每个供应商获得唯一 UUID，旧 `source_provider_id` 引用仍正确重映射。
4. bundle 含非法或重复 UUID：在删除当前配置或写 profile 文件前整体失败。
5. 复制供应商和重复导入单供应商分享：每个副本都获得不同 UUID，保持默认禁用和副本命名语义。
6. 完整导入携带受管 profile：目标不存在时生成；同 ownership 可幂等更新；未受管同名文件、不同 owner、用户改写 hash 均 fail closed 且原文件不变。
7. profile 文件写入或运行时收敛失败：DB、settings、已有 profile 文件和 manifest 全部恢复到导入前状态。
8. 删除仍被 profile 引用的供应商：返回可识别错误并列出引用 profile，不产生悬空 alias。

## 最小落地建议

MVP 不需要把所有内部 `provider_id` 外键改成 UUID。最小且完整的改动是：

1. 在 `providers` 增加不可变 UUID，并在所有创建路径统一生成；
2. AIO 模型 alias 与 profile 只使用 UUID；
3. 完整配置 bundle 保留 UUID 和 profile 定义，旧 bundle 导入生成 UUID；
4. 分享/复制始终生成新 UUID；
5. profile 文件由数据库定义重建，通过 manifest 证明所有权，冲突时阻止而非覆盖。

这组规则既解决跨机器身份稳定性，也保持当前完整导入、分享导入和复制功能各自已有的产品语义。
