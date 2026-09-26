# W1/W2 原生后端接入契约

更新时间：2026-09-26。W1/W2 持有 domain/native_cli、infra/native_cli、app/native_cli_service.rs、commands/native_cli.rs；中央注册、设置与迁移由协调者接线。

## Settings 与数据库（协调者）

AppSettings 新字段 `#[serde(default)] pub pi_omp_native_targets: Vec<crate::domain::native_cli::NativeTargetSelection>`，默认空。字段归原生目标选择命令所有，不加入普通 SettingsUpdate/Patch；按 client 替换已选项，其他字段及另一 client 保留。

NativeTargetSelection 为 camelCase `{ client: "pi" | "omp", mode: "default" | "custom" | "profile", agentDir: string|null, profile: string|null }`。default 明确选择默认 profile，但遵循进程目录变量；profile 仅 OMP 且必须有效命名，忽略默认目录 override；custom 必须显式绝对 agent 目录且不能兼带 profile。普通原生 api 为任意非空字符串，绝不限制为网关四协议。OMP 默认模式会排除由 `OMP_PROFILE` / `PI_PROFILE` 继承的 profile-derived `PI_CODING_AGENT_DIR`，但保留真正的自定义目录变量；显式命名选择优先，绝不猜测 shell 活动 profile。OMP XDG 仅影响原生 data/state/cache；模型文件仍属于选定 agent 目录。

迁移 SQL（请在下一 migration 执行，业务服务不 CREATE TABLE）：

```sql
CREATE TABLE native_cli_provider_profiles (
  profile_uuid TEXT PRIMARY KEY NOT NULL,
  client TEXT NOT NULL CHECK (client IN ('pi','omp')),
  target_id TEXT NOT NULL,
  native_key TEXT NOT NULL,
  display_name TEXT NOT NULL,
  node_json TEXT NOT NULL,
  node_digest TEXT NOT NULL,
  revision TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(client, target_id, native_key)
);
```

完整节点可能包含用户原生字面凭证，沿用已有私有 AIO DB，不建立第二 DB，不进入 share 或日志。列表仅返回摘要；明确 read_for_edit 才返回 node。协调者已接线 v47 迁移与 settings owner。新增依赖 `serde_yaml = "0.9"`、`yaml-rust2 = "0.10"`，Windows windows-sys 使用 `Win32_Security_Authorization`；复用 serde_json、sha2、rusqlite、shared/fs 与 shared/uuid。

## IPC（均为 async Tauri + Specta）

- `native_cli_targets_list(app, client: NativeClient) -> Vec<NativeTarget>`：默认、选中自定义、已存在 OMP profile；只做有界目录枚举。
- `native_cli_target_validate(app, selection: NativeTargetSelection) -> NativeTarget`：不保存，不写原生文件。
- `native_cli_target_select(app, selection: NativeTargetSelection) -> NativeTarget`：验证后仅持久化本 client 的选择。
- `native_cli_providers_list(app, db_state, target_id: String) -> NativeProvidersList`：显式节点同步档案；解析失败保留档案并显示 unknown，不假装 removed。
- `native_cli_provider_read_for_edit(app, db_state, target_id: String, native_key: String) -> NativeProviderEdit`。
- `native_cli_provider_save(app, db_state, input: NativeProviderSaveInput) -> NativeMutationResult`。
- `native_cli_provider_apply/remove(app, db_state, input: NativeProviderActionInput) -> NativeMutationResult`。
- `native_cli_provider_delete(app, db_state, input: NativeProviderDeleteInput) -> NativeMutationResult`：显式 removeFromNative 才同时移除；仍存在时拒绝只删档案。

输入输出均 camelCase；domain/native_cli/mod.rs 是最终可生成定义。列表携带 target、revision、parseStatus、issue、providers；摘要携带 profileUuid、nativeKey、displayName、state(present/archived/unknown)、managed、nodeDigest、profileRevision、api、modelCount、apiKeyConfigured。禁止把 membership 状态叫当前默认。

Save 输入 targetId/nativeKey/displayName/expectedRevision/expectedNodeDigest/expectedProfileRevision/apply，以及互斥 node（完整原文对象）或 patch（字段路径数组）。`NativeFieldPatch {path: string[], value: JSON|null}`：null 表示删除该字段；非 null 仅改对应路径，保留其他未知字段；设置原生 null 值需传含 null 的父对象或完整 raw node。Action 输入 targetId/nativeKey/expectedRevision/expectedNodeDigest/expectedProfileRevision；Delete 在 input 中增加 removeFromNative。Archived/Unknown 摘要的 nodeDigest 为 null，profileRevision 始终独立；修改已在原生文件中的节点必须 apply=true，否则下次显式同步会覆盖未应用草稿。

## 入口生成者复用 API（W4/W5）

`app::native_cli_service::resolve_target(app, target_id) -> AppResult<NativeTarget>` 只在当前 selected 受控目标里查找，不接受任意文件路径；切换选择后旧目标预览失效。

`infra::native_cli::with_target_lock(&target, |session| -> AppResult<T> { ... })`：闭包持有该规范化文件目标的进程锁；原生 CRUD 和入口生成必须共用。

所有依赖当前选择的命令必须在获得上述锁之后调用 `app::native_cli_service::verify_selected_target(app, &target)`，再执行 snapshot/DB/文件操作；函数本身只读 settings/目标元数据，不再取 target lock。`target_select` 在旧目标锁内条件更新 settings，拒绝并发改过的旧 selection，因此运行中操作先结束再完成切换，切换后排队旧操作被拒绝。验证候选新目标时的锁在获取旧目标锁前已释放，禁止同时持两个 target lock 或重复获取同一 nonreentrant 锁。

- `session.snapshot() -> AppResult<NativeDocumentSnapshot>`：安全有界读取，revision 为完整字节摘要；内部 snapshot 不 Serialize/Debug，不直接发 IPC；`snapshot.providers()` 返回精确显式节点 map，`snapshot.revision` 可绑定预览。
- `session.patch(expected_revision, &[NativeNodePatch]) -> AppResult<NativePatchReceipt>`：先验证整批，节点 `NativeNodePatch {native_key, expected_digest: Option<String>, replacement: Option<Value>}`；None digest 要求当前缺席，Some 要求精确内容。replacement None 为删除。整批一次原子写，未知字段与其他节点保留。
- `session.compensate(&receipt) -> AppResult<()>`：仅当每个触及节点仍匹配 receipt 的 after_digest 时还原这些节点，保留其他外部变化；冲突明确报 NATIVE_COMPENSATION_CONFLICT，绝不整文件覆盖。
- `node_digest(&Value) -> String`：规范化 JSON SHA-256，供 manifest 精确记录。

NativePatchReceipt 只内部使用，包含 after_revision、changed、可选 backup_path 及节点反向 patch（旧值和预期 after digest），含敏感数据故不 Debug/Serialize。没有变更时不写文件、不建备份。默认项、auth.json、agent.db、.env 从不读取或修改。调用者的 manifest intent/finalize 必须也位于同一个 target lock 内，锁顺序固定 target → DB。

## 所有权协作

已接 W4/W5 的 `domain::native_gateway::managed_native_keys(&Db, &str) -> AppResult<Vec<String>>`，按精确 `(target_id,native_key)` manifest 查询（含 pending）；原生刷新跳过这些托管节点，native CRUD 拒绝托管 key，不能按 aio 前缀识别。普通原生档案不会导入 gateway providers 池。

安全限制：最大文件 4 MiB，重复对象/YAML key、非字符串 key、标签/锚点/别名/merge、多文档及不可识别语法只读。允许丢格式/注释，但受限权限原文备份保留恢复依据；路径 canonicalize 并拒绝文件级 symlink/reparse 及不确定目标。无法参与 AIO 锁的外部写入者仍有 final-check 与 atomic rename 间竞争窗口，此窗口不宣称为文件系统事务。

Pi 解析以真实源码 `core/model-config.ts`、`utils/json.ts` 和 W0 checkpoint 为准：只接受 BOM、`//` 行注释、尾逗号，不接受 `/* */` 块注释、单引号、裸 key；严格拒绝重复 key。OMP YAML 使用完整 tokenizer 检测图引用和标签，避免字符串引号欺骗；未知字段的 JSON-compatible 值原样保留。模型 thinking 分别验证 Pi thinkingLevelMap 与 OMP efforts/legacy range，读取时不互相翻译、不补能力、不执行 apiKey/headers 命令表达式。

备份为 `.aio-native-backups/<sha256>-<uuid>.original`，每次生成独立私有文件，不复用潜在宽权限旧备份；Unix 文件 0600、新目录 0700，Windows 写入任何字节前设置 protected owner/SYSTEM ACL。已存在原文件原子替换，新文件使用 no-replace；备份/暂存/最终替换失败保留原文件，DB 失败只条件补偿本次节点。跨 DB/文件不宣称崩溃原子事务，进程崩溃时 retained backup 是人工恢复依据。

## 验证记录

- 协调者第二轮 `cargo check` 已通过；两个未用字段已移除，无 blanket dead_code allow。
- 自有文件 `rustfmt --edition 2021 --check` 通过；2026-09-26 最终增量 `cargo test --lib native_cli` 通过：39 passed、0 failed（包含名称匹配的两项既有 gateway 用例），构建耗时 2m07s、测试耗时 1.13s，无编译警告。
- 首轮两项失败根因均为测试 fixture 持有单连接池中的唯一连接后再次调用 service；已显式释放连接再验证，保留语义断言，最终全部通过。
- 本轮显式使用 `CARGO_TARGET_DIR=<worktree>/src-tauri/target`、`CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=1`；测试二进制为 `src-tauri/target/debug/deps/aio_coding_hub_lib-25a5c65b4b903819.exe`。11 个 Rust 源文件最后修改时间为 2026-09-26 08:58:12 UTC；后续仅更新本报告。
- 隔离 fixtures 覆盖 JSONC/YAML 语法、未知字段、targets/profile、完整档案 CRUD、重复 key、外部 revision/node 冲突、备份与最终写失败、DB 故障补偿、精确 manifest 排除，以及 auth/default/.env 内容与 mtime 不变。
- 排队 save/remove 与目标选择竞争使用真实 target mutex、scoped threads 和 channel 确定顺序，验证获锁后的选择失效不会改文件/DB或生成备份。
- Windows locked destination 和 protected owner/SYSTEM ACL 实测均通过；Unix 权限用例以 cfg(unix) 保留，当前 Windows 运行不代表完成 Unix 运行验证。
- 已向协调者移交 cargo 所有权，最终全库测试、clippy 和 bindings 再生由父统一执行；本包无剩余未实现功能、无提交或推送、未改中央接线/迁移/settings文件。
