# 第六轮终审研究（历史事实：独立 Codex `gpt-5.6-sol / effort=max` 只读终审）

## 结论与边界

第六轮终审确认 7 项 P1 与 1 项 P2。问题集中在四条跨层边界：settings 字段与
autostart 副作用所有权、config import 的跨 DB/settings/Skill FS 事务串行化、配置文件与
Skill 文件的真实有界读取/编码预算、Image Gen storage stats 的句柄绑定遍历；第 8 项仅为
已完成 round-5 的文档证据矛盾。

本轮修复不得依据 Skill 内容做敏感词扫描、脱敏、过滤、自动剔除或阻断。可信根内经普通
文件、身份、路径、数量与字节预算验证的合法文件必须逐字节导出并可逐字节导入。

已读取并交叉核对：根 `AGENTS.md`、`.trellis/workflow.md`、`trellis-brainstorm`、当前子任务、
父任务全部规划/研究工件、round-5 全部归档工件、相关 cross-layer specs 与 guides，以及下列
生产入口和现有测试。以下行号均为 2026-07-17 当前 worktree 的准确锚点。

## Model And Session Boundary

本文件记录的 round-6 终审是已经发生的独立 Codex `gpt-5.6-sol / effort=max` 只读审核事实。
已发生的实现保留真实 `gpt-5.6-luna / effort=max` 模型记录；按用户最新指令，剩余执行改由唯一
一个 Orca 管理的 Codex `gpt-5.6-terra / effort=max` 终端串行完成。下一次独立只读终审及父任务
最终审核都必须针对同一冻结提交，新开彼此隔离的 Codex `gpt-5.6-sol / effort=max` 与
Pi（`grok-cpa / grok-4.5`）reviewer；协调会话汇总两份结果，禁止复用 Luna/Terra 执行会话。

## F1 · P1 · `settings_set` 非法候选先修改 OS autostart

### 精确调用链与证据

1. `src-tauri/src/commands/settings.rs:20-26` 的 `settings_set` 进入
   `src-tauri/src/app/settings_service.rs:664-668` 的 `settings_set_impl`。
2. `settings_service.rs:733-740` 在 blocking work 中读取锁外 `previous` 快照，并从该快照
   构造候选。
3. `settings_service.rs:889-894` 在候选尚未组成、尚未调用
   `settings::validate_bounds` 前调用 `app::autostart::reconcile_auto_start`；该 helper 最终在
   `src-tauri/src/app/autostart.rs:18-44` 对 OS 启用/禁用启动项。
4. 候选直到 `settings_service.rs:896-966` 才完整组成，直到 `:968-969` 才执行 settings/proxy
   校验，直到 `:1013-1031` 才持久化。
5. 因此在当前 `auto_start=false` 时提交 `autoStart=true` 与
   `circuitBreakerFailureThreshold=0`，OS 启动项可以先被启用，随后 `:968` 因
   `src-tauri/src/infra/settings/persistence.rs:470-472` 拒绝候选；canonical settings 仍为 false。

### 根因

`reconcile_auto_start` 同时被当成“计算候选值”和“执行外部副作用”，副作用发生在 settings
锁、候选校验和 durable commit 之前。其错误还在 `autostart.rs:70-87` 被降级为旧布尔值，调用方
没有精确的 commit 身份可用于失败修正或并发 owner 判定。

### 现有测试缺口

- `autostart.rs:103-138` 仅测试纯 helper 的不变值/强制同步/错误传播。
- `settings_service.rs:1292-1337` 只从手工 CAS 后测试 runtime rollback，没有走真实
  `settings_set_impl`，也没有观察非法候选是否调用 OS sync。
- `src-tauri/tests/settings_crud.rs` 的 command 测试通过
  `src-tauri/src/test_support.rs:543-663` 的重复实现，不是生产 `settings_set_impl`。

### 拟修复边界

- 先在 `settings::update` 锁内基于 latest 应用字段补丁、规范化、完整校验并 durable commit；
  commit 成功前禁止 autostart 副作用。
- settings service 与 config import 共用 autostart 串行协调器。协调器为每次 autostart 字段
  commit 分配单调 generation，并返回包含 generation、previous、committed 的精确 token。
- OS sync 失败时，只在 token 仍是当前 generation 且 canonical 字段仍等于 committed 时恢复
  previous；随后在同一协调器锁内从最新 canonical 强制收敛 OS。token 已失去所有权时不得写旧值。
- 不改变其它 settings、网关或 UI 产品语义；非法候选必须实现 settings/OS 零副作用。

## F2 · P1 · `settings_set` 用锁外旧快照覆盖并发字段 owner

### 精确调用链与证据

1. `settings_service.rs:740` 读取锁外完整快照，随后 `:896-966` 重新构造完整
   `AppSettings`。
2. `settings_service.rs:426-449` 的 `write_settings_snapshot` 虽在
   `settings::update` 内写入，却只暂存 `image_gen_storage_dir`、
   `image_gen_storage_roots`、`grok_proxy_preferences` 三项，然后以 `*latest = next_settings`
   替换其余整份快照。
3. 真正的共享锁内 RMW 在 `settings/persistence.rs:563-575`，但当前调用在进入锁前已把大部分
   字段冻结为旧值。
4. 专属生产 writer 位于：rectifier `settings_service.rs:1161-1203`，circuit notice
   `:1206-1219`，Codex session completion `:1221-1234`。它们在普通 save 的锁外读取与锁内
   整份替换之间提交时会被覆盖。
5. 前端 `src/services/settings/settings.ts` 当前也把多个 rectifier 字段映射进普通
   `SettingsUpdate`；`src/pages/settings/settingsPersistenceModel.ts:81-108` 会将其中一部分放入
   普通 settings 保存快照，因此仅修 Rust 比较函数不足以建立清晰所有权。

### 根因

代码把 `SettingsUpdate` 当成“重建 AppSettings 所需的全量快照”，而不是入口拥有字段的补丁。
`settings_service_owned_equal` 同样用“排除 3 字段后的全对象相等”表示 owner，错误地把专属
writer 字段和未来新增字段纳入普通 settings owner。

### 现有测试缺口

- persistence 层测试证明两个直接 `settings::update` 会串行，但没有让真实
  `settings_set_impl` 与真实专属 writer 交错。
- 当前 runtime rollback 测试以 `log_retention_days` 模拟 winner，未覆盖 circuit notice、
  Codex completion 或完整 rectifier group。
- 前端测试仍断言普通 `settings_set` 会发送 rectifier 字段，缺少“普通 payload 不含专属
  owner 字段”的负断言。

### 明确字段所有权

普通 `settings_set` 只拥有以下字段；`schema_version` 仅在同一锁内升级为当前版本：

- UI/网关基础：`preferred_port`、`show_home_heatmap`、`show_home_usage`、
  `home_usage_period`、`gateway_listen_mode`、`gateway_custom_listen_address`、`auto_start`、
  `start_minimized`、`tray_enabled`、`enable_cli_proxy_startup_recovery`。
- retention/timeout/failover：`log_retention_days`、`request_log_retention_days`、
  `provider_cooldown_seconds`、`provider_base_url_ping_cache_ttl_seconds`、
  `upstream_first_byte_timeout_seconds`、`upstream_stream_idle_timeout_seconds`、
  `upstream_request_timeout_non_streaming_seconds`、`failover_max_attempts_per_provider`、
  `failover_max_providers_to_try`、`upstream_retry_policy`、
  `circuit_breaker_failure_threshold`、`circuit_breaker_open_duration_minutes`。
- 通用开关：`enable_cache_anomaly_monitor`、`enable_debug_log`、
  `enable_task_complete_notify`、`enable_notification_sound`、`update_releases_url`。
- WSL/Codex：`wsl_auto_config`、`wsl_target_cli`、`cli_priority_order`、
  `wsl_host_address_mode`、`wsl_custom_host_address`、`codex_home_mode`、
  `codex_home_override`、`codex_oauth_compatible_proxy_mode`、`codex_provider_test_model`。
- CX2CC：四个 `cx2cc_fallback_model_*`、`cx2cc_model_reasoning_effort`、
  `cx2cc_service_tier`、`cx2cc_disable_response_storage`、
  `cx2cc_enable_reasoning_to_thinking`、`cx2cc_drop_stop_sequences`、`cx2cc_clean_schema`、
  `cx2cc_filter_batch_tool`。
- upstream proxy：`upstream_proxy_enabled`、`upstream_proxy_url`、
  `upstream_proxy_username`、`upstream_proxy_password`。

明确排除的专属 owner：

- Image Gen writer：`image_gen_storage_dir`、`image_gen_storage_roots`。
- Grok writer：`grok_proxy_preferences`。
- circuit notice writer：`enable_circuit_breaker_notice`。
- Codex completion writer：`enable_codex_session_id_completion`。
- rectifier writer：`verbose_provider_error`、`intercept_anthropic_warmup_requests`、
  `enable_thinking_signature_rectifier`、`enable_thinking_budget_rectifier`、
  `enable_billing_header_rectifier`、`enable_claude_metadata_user_id_injection`、
  `enable_response_fixer`、`response_fixer_fix_encoding`、
  `response_fixer_fix_sse_format`、`response_fixer_fix_truncated_json`、
  `response_fixer_max_json_depth`、`response_fixer_max_fix_size`。

### 拟修复边界

- 建立一个显式 `apply_settings_update_owned_patch(latest, update)` 共享 helper；所有 `Option`
  fallback、password preserve、规范化和跨字段校验均在 `settings::update` 锁内基于 latest 完成。
- 从普通 Rust DTO/生成绑定/前端普通 request builder 中移除 rectifier 专属字段；现有 UI 控件
  改走已有 `settings_gateway_rectifier_set`，不得通过普通 save 间接重写该组。
- owned equality/rollback 只比较上述显式普通字段 token，未来新增字段默认不属于普通入口。
- 真实 writer barrier：A 在真实 `settings_set_impl` 进入 locked RMW 前暂停；B 依次通过真实
  circuit/Codex/rectifier writer 提交；A 恢复后所有 B 字段必须保留。

## F3 · P1 · config import 的 post-CAS autostart 覆盖并发 winner

### 精确调用链与证据

1. `src-tauri/src/infra/config_migrate/mod.rs:284-320` 解析 import 并捕获旧状态。
2. `mod.rs:365-368` 在 test hook 后执行 whole-settings CAS。
3. CAS 成功后，`:388-394` 无条件以旧 `previous_settings.auto_start` 与 import desired 调用
   autostart；只有 OS sync 失败导致 effective 值变化时，`:398-425` 才做第二次 CAS/收敛。
4. 因而 A import 提交 true 后暂停，B settings writer 提交 false 并关闭 OS，A 恢复仍会把 OS
   打开；由于 A 的 OS call 成功，不进入第二 CAS，最终 canonical=false/OS=true。

### 根因与测试缺口

config import 与普通 settings writer 共享 canonical 字段，却不共享 autostart commit/副作用
所有权。现有 `config_import_cas_loser_preserves_winner_without_autostart_side_effect`
（`config_migrate/tests.rs:230-260`）只覆盖首次 CAS 之前的 loser，没有 post-CAS barrier。

### 拟修复边界

- whole import CAS 也必须在 F1 的 autostart 协调器内登记 token；不得在协调器外按旧参数调用
  OS。
- 锁序固定为 `CONFIG_IMPORT_LOCK -> AUTO_START_LOCK -> SETTINGS_WRITE_LOCK`。普通 settings
  不获取 import lock；任何路径都不得持有 settings lock 再等待 autostart lock。
- 协调器每次副作用后按最新 canonical 收敛；A/B 交错的最终 winner 由最后成功提交的
  autostart generation 决定，旧 token 只能观察并同步 winner，不能恢复旧值。
- 新增真实 A/B barrier：A 在 commit 后/OS sync 前暂停，B 必须在共享协议上等待；A 完成后
  B 提交 false，最终 canonical 与 OS 均为 false。另测副作用失败和 loser rollback。

## F4 · P1 · 并发 config import loser 可回滚成功 winner 的 Skill FS

### 精确调用链与证据

1. `src-tauri/src/commands/config_migrate.rs:62-85` 在确认并读取 bundle 后直接调用
   `config_migrate::config_import`，没有进程级 import mutex。
2. `config_migrate/mod.rs:320` 捕获 CLI runtime，`:324-363` 开始 DB transaction 并接管 Skill
   FS，`:430` 在 runtime sync 失败时先 `drop(tx)` 再调用 rollback。
3. `rollback.rs:42-62` 的 `SkillFsImportGuard::rollback` 无 owner 身份检查：删除
   `imported_local_dirs`、当前 SSOT root，再把自己的 backup 恢复。
4. A 在 `drop(tx)` 后暂停、B 完成合法 import 并 `guard.finish()` 后，A 恢复可删除 B 的当前
   Skill 根并恢复 A backup，造成 DB/settings/FS 分裂。
5. `rollback.rs:206-260` 用秒级 `now_unix_seconds()` 生成 stage/backup 名，同秒并发或残留目录
   会碰撞。

### 现有测试缺口

- 只有单 import success/failure 与 settings CAS loser 测试；没有两个真实 import 并发交错。
- 没有证明第二个 import 在第一个 capture→rollback/finish 全生命周期内被阻塞。
- 没有同秒重复 import、失败后立即重试、随机 stage/backup 唯一性与残留清理断言。

### 拟修复边界

- 选择进程级 `CONFIG_IMPORT_LOCK` 主方案。64 MiB 文件 no-follow 有界读取、UTF-8/JSON 解析、
  schema/纯 payload 预检与用户确认均在锁外；锁在第一次读取 canonical settings、capture CLI
  runtime、打开 DB transaction 或改动 Skill FS 之前获取。
- 锁一直持有到成功路径的 DB commit、`SkillFsImportGuard::finish`、runtime/tray 最终化，或
  失败路径的 settings/autostart/Skill FS/runtime rollback 全部完成。
- 即使已有全局锁，也把秒级 ID 改为随机唯一 import token，并以 create-new/retry 创建该
  token 的 stage/backup 根；guard 只清理自己记录的随机路径。
- 任一失败必须在锁内完成补偿后才允许下一 import 进入；不允许 DB 指向 B、settings 指向 A、
  Skill FS 又来自第三种状态。

## F5 · P1 · 8 MiB/64 MiB 读取并非硬上限

### 精确调用链与证据

- Skill export：`src-tauri/src/infra/config_migrate/skill_fs.rs:215-268` 已从 no-follow 且 identity
  校验的 file handle 获取 metadata，但 `:232-244` 仅按旧 metadata 预分配后执行
  `read_to_end`，文件在 metadata 后增长会无界读入，再在 `:245` 才报错。
- 通用 reader：`src-tauri/src/shared/fs.rs:105-145` 先 `path.exists()`，再以会跟随链接的
  `metadata(path)` 检查，最后 `std::fs::read(path)` 重新按路径完整分配；替换、symlink、FIFO/
  socket/device 均可跨过检查或阻塞。
- config import 在 `commands/config_migrate.rs:20-33` 使用该通用 reader，因此 64 MiB 是事后
  长度检查，不是分配/读取硬上限。

### 现有测试缺口

- shared fs 只有静态 oversized/missing 测试；没有 symlink/reparse、metadata 后增长、路径替换、
  FIFO/socket/device 与最大读取次数断言。
- Skill FS 已有 symlink、hardlink rebind、special-file 与静态 8 MiB+1 测试，但没有在同一
  handle metadata 后增长的 barrier。

### 拟修复边界

- 共享“no-follow 打开并验证普通磁盘文件”与“从已打开 handle 硬上限读取”helper。
- Unix 使用 `O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK` 打开后 `fstat` 确认 regular file；Windows 使用
  `FILE_FLAG_OPEN_REPARSE_POINT`，验证非 reparse、非 directory 且 `GetFileType` 为 disk。
- 从同一 handle 读取 `limit + 1`（`Read::take` 或等价），capacity 至多为 limit；读到第
  `limit+1` 字节立即返回 too-large。路径在 open 后被替换不改变已捕获对象。
- Skill collector 直接把已经过枚举 identity/no-follow 校验的同一 handle 交给 bounded helper，
  不重新按路径打开。
- 保持合法文件任意字节逐字节 Base64 round-trip；安全判断仅依赖对象类型、身份、路径与预算。

## F6 · P1 · export 可生成自身必然拒绝 import 的 bundle

### 精确调用链与证据

1. 单 Skill decoded total 是 8 MiB（`config_migrate/mod.rs:21-26`），但跨 Skill 没有共享 encoded
   总预算。
2. `commands/config_migrate.rs:38-56` 调用 `config_export`，随后
   `serde_json::to_string_pretty`，再直接 `std::fs::write`；未检查同模块 import 在
   `config_migrate/mod.rs:20` 定义的 64 MiB 上限。
3. 标准 Base64 约扩大 4/3，六个各自合法的接近 8 MiB Skill，加 JSON/metadata 后会超过
   64 MiB。export 成功，`read_config_import_bundle` 必然拒绝该文件。

### 现有测试缺口

- 现有 tests 只断言单 Skill raw/Base64/decoded total 对称，没有配置级 encoded budget。
- command export 没有“oversize 时旧目标保持原字节”测试，也没有“每个成功 export 都能被
  production import reader 接受”的性质测试。

### 拟修复边界

- 建立共享 `CONFIG_BUNDLE_ENCODED_MAX_BYTES = 64 MiB`，import reader 与 export serializer
  引用同一 SSOT；旧 `CONFIG_IMPORT_FILE_MAX_BYTES` 可作为兼容别名，不再独立定义数值。
- pretty JSON 通过 capped writer/serializer 写入最多 limit+1 的 Vec；超限返回明确
  `SEC_INVALID_INPUT`，不得先覆盖目标文件。
- 只有完整序列化且长度在预算内后，才以同目录随机临时文件 + atomic replace 写目标。
- 测试六个合法 Skill 造成配置级超限并保留目标 sentinel；另以真实小 bundle 完成
  export file → bounded reader → config import → Skill bytes round-trip。
- 不通过内容过滤、静默丢文件或改变 Base64/JSON schema 来缩小 bundle。

## F7 · P1 · Image Gen storage stats 跟随 task dir 内链接

### 精确调用链与证据

1. 生产 IPC `src-tauri/src/app/image_gen_service.rs:186-196` 的 `storage_get` 调用
   `domain::image_gen::storage_stats_with_roots`。
2. `src-tauri/src/domain/image_gen/history.rs:2030-2053` 校验 DB task dir，得到已包含
   `root_handle`/`task_handle` 与稳定 identity 的 `ValidatedTaskDir`，但随后仅把路径传给
   `dir_size_bytes`。
3. `history.rs:2062-2077` 用 path-based `read_dir` 和会跟随链接的 `entry.metadata()` 递归；
   `loop -> .` 可无限递归/栈溢出，junction 可扫描根外，单个恶意 entry 即可挂死整个 blocking
   command。

### 现有契约与测试缺口

- `image-gen-trust-boundary-contract.md` 要求无效选中 row 整次请求 fail closed；
  `storage_get` 直接透传该错误。因此安全 entry 不能静默跳过后返回虚假 total。
- 现有 `image_gen/tests.rs:678-740` 只验证正常平面目录的字节总数，`:1121-1146` 只验证多根；
  没有 task 内 symlink/junction loop、特殊 entry、深度/数量/字节预算或枚举后替换。

### 拟修复边界与错误语义

- 以已捕获 `ValidatedTaskDir.task_handle` 为根做平台相对枚举；不重新把 `dir.path` 升级为 authority。
- 每项先 no-follow/lstat，目录/普通文件再相对打开并比较枚举 identity 与 handle identity；Windows
  明确拒绝 `FILE_ATTRIBUTE_REPARSE_POINT`，Unix 明确拒绝 symlink，所有平台拒绝特殊类型与多链接
  file。
- 维护 visited `FileIdentity`，递归深度上限 64、一次 storage stats 全局 entry 上限 100,000，
  累计字节用 checked `u64` 且不得超过 `i64::MAX`（与返回 DTO 的可表示范围完全一致）。
- 任何 link/reparse/special/identity/race/budget 异常均让整个 storage stats 请求 fail closed；不跳过
  恶意 entry，也不返回低估值。正常合法 nested tree 保持精确统计。
- Windows junction、Unix symlink loop、special file、深度/entry budget、枚举后替换和普通 nested
  tree 都走真实 `storage_stats_with_roots`；测试用 timeout 证明单个恶意 entry 不会挂死。

## F8 · P2 · round-5 归档事实与父任务/journal 证据矛盾

### 精确证据

- `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5/task.json` 已是
  `status=completed`，归档 PRD/implement 的所有 AC 与门禁均已勾选。
- 父 `prd.md:14-15` 仍写子任务 10 正在修复，`:58` current map 仍为 in progress，`:135`
  round-5 AC 未勾选，且任务链尚未加入 round-6。
- 父 `design.md:5,19,36` 仍写“十个子任务”并止于 round-5。
- 父 `implement.md:55-56,92-96` 仍未勾选 child 10，并指向已不存在的 active round-5 路径。
- 父 `research/integration-evidence-summary.md:47` 仍写 child 1→5→parent。
- `.trellis/workspace/FingerCaster/journal-1.md:257` 明确记录 round-5 全门禁通过，但同一 session 的
  Testing `:272` 仍写 “Validation was not recorded”。

### 根因、缺口与修复边界

round-5 archive/journal 三阶段完成后，父事实投影与 journal Testing 模板未同步。此项不修改任何
产品行为、测试逻辑或父 `task.json.status`。

实现阶段只做事实纠正：父 PRD/design/implement/research 改为 round-5 已归档、round-6 当前修复、
下一次双独立只读终审在 round-6 后；active 路径改为 archive 路径并勾选已完成项；journal session 8
Testing 填入其 Summary 与归档 implement 已记录的实际门禁。父 task 始终保持 `in_progress`。

## 共享验证与回滚边界

- 聚焦回归必须通过真实 production entry/barrier，而非只测纯 helper。
- 影响共享 filesystem/settings helper 后必须运行完整 Rust lib 与 integration suites；前端 ownership
  调整必须运行完整前端 suite。
- bindings 生成两次，第二次 hash 必须零漂移；cross-layer specs 与
  `src/templates/markdown/spec/cross-layer` 对应镜像逐字节一致。
- 全门禁包含 typecheck、lint、format check、build、all-target Clippy、
  `check:precommit:full`、`check:prepush`、Trellis active/archive manifests/task validate 与
  `git diff --check`。
- 工作提交完成后只归档 round-6，再纠正/新增 journal Testing 并单独提交；父任务继续
  `in_progress`。不得访问 remote，`upstream` push URL 保持 `DISABLED`。

无剩余产品决策或范围阻断。

## Follow-up Findings F9-F15

以下 findings 是 round-6 独立终审在历史 F1-F8 实现之后确认的 follow-up；历史完成记录不改变
这些新增缺口。

### F9 · P1 · settings rollback 吸收并发 preferred_port winner

#### 生产证据

settings_service 的 durable commit 通过 autostart coordinator 返回后，控制流在 token 构造处
重新读取 canonical settings。该窗口允许 gateway_control 的 preferred-port repair 先提交 B；
canonical reread 会把 B 端口写入 A 的 committed token。随后 runtime sync 失败，ordinary rollback
会误认为 B 是 A 的 token 并恢复旧 previous token，覆盖 B。

#### 回归与修复边界

在真实 settings_set_impl 中加入 coordinator return 到 token construction 的 barrier；B 调用
production preferred-port repair 写入新端口，A 注入 runtime sync failure。A-owned token 必须来自
锁内 settings::update durable result，rollback 必须保留 B 且只同步 canonical winner。

### F10 · P1 · config import early failure 删除旧 SSOT root

#### 生产证据

SkillFsImportGuard 过早登记 ssot_root。stage write 失败，或 live root rename 到 backup 前失败时，
rollback 无条件 remove live root，且没有 backup 可恢复，旧 root sentinel 丢失。

#### 回归与修复边界

guard 显式记录 candidate/stage-created/live-backed-up/new-root-activated 四种状态。stage 与
pre-backup rename failpoint 必须命中真实 apply_skill_fs_import，rollback 只清理自身 stage，旧
root/bytes 保留且无 import artifact 残留。

### F11 · P1 · local Skill writer 半成品未登记

#### 生产证据

write_prepared_skill_files_to_dir 多文件中途失败时，imported_local_dirs 只在整体成功后登记；
rollback 不知道新目录，留下第一批已写文件。预存 local directory 也不能被当作新对象删除。

#### 回归与修复边界

local target 在 writer 可能 mkdir/write 前登记 ownership，真实中途 failpoint 后 guard 清理
半成品；预存目录与 sentinel bytes 逐字节保留。

### F12 · P1 · Unix export regular file -> FIFO race

#### 生产证据

Skill export 的 Unix handle-relative child open 缺 O_NONBLOCK。枚举后 regular file 替换 FIFO 时，
在类型验证前 open 可能永久阻塞，即使后续 identity/type 检查正确也无法执行。

#### 回归与修复边界

保留 NOFOLLOW/CLOEXEC、relative identity 与 single-link 检查并加入 NONBLOCK。production export
回归在独立 child 中安装 enumeration barrier 替换 FIFO，父测试进程使用真正 bounded watchdog，
超时可终止 child；Windows cfg 明确不运行 Unix FIFO。

### F13 · P2 · settings finalize 后恢复失败被吞没

#### 生产证据

canonical -> backup 成功、tmp -> canonical 失败后，restore backup -> canonical 的结果被忽略；
canonical 可能不存在，调用方只得到普通 finalize error，无法区分 SETTINGS_RECOVERY_REQUIRED。
同时 restore 成功后若在 restore 之后检查 backup ownership，会错误地跳过 tmp cleanup。

#### 回归与修复边界

restore 前捕获 had_backup/previous durable ownership。restore 成功时清理 writer-owned tmp；只有
没有旧 canonical/backup 且 finalize 失败才保留 tmp。settings_crud 真实 failpoint 覆盖 finalize-only
restore-success 和 finalize+restore double-failure，检查错误分类、canonical/backup/tmp 状态及 durable
bytes。

### F14 · P2 · Image Gen timeout 测试不是 bounded test

#### 生产证据

历史测试调用同步 storage_stats 后才比较 elapsed；若 production open 永久阻塞，测试本身永远
不会完成。storage_stats production entry 必须覆盖 FIFO/special/race 的 fail-closed 时限。

#### 回归与修复边界

用可终止独立 child + 外部 watchdog 运行真实 storage_stats_with_roots，并在 child 内把枚举后的
regular file 替换为 FIFO。Unix 测试保留 cfg(unix)；当前 Windows 环境如实列为未运行 Unix-only
覆盖，Windows junction/reparse/special 覆盖继续由 Windows cfg 执行。

### F15 · P2 · round-5 journal 证据矛盾

#### 生产/文档证据

journal session 8 Summary 已明确记录 round-5 聚焦、Rust library/integration、前端、bindings、
typecheck/lint/format/build、Clippy、git diff、manifest、precommit/full、prepush 门禁通过，但
同一 session Testing 仍为 Validation was not recorded。

#### 修复边界

只依据该 Summary 与 archive round-5 implement 的真实门禁记录替换 Testing 行；不新增本轮测试
证据，不新增 journal session。当前 child/parent 工件保持 round-5 archived -> round-6 current ->
下一次双独立 read-only review 的顺序，父 task status 保持 in_progress。

## Follow-up Validation Status

- F9/F10/F11/F13 的确定性 production regressions 已在实现阶段先加入并 focused 通过。
- F12/F14 的 Unix child watchdog regressions 已加入，但当前 x86_64-pc-windows-msvc 无法执行
  Unix FIFO；不得将 cfg 排除报告为已运行。
- F15 仅进行事实文档修正，不虚构本轮新测试。

### Docker/Linux verification record (passed)

当前 Terra 会话先用 `docker images --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.Size}}'` 列出本地
镜像，再以 `docker run --rm --entrypoint sh <image> -lc 'command -v cargo >/dev/null 2>&1 && rustc
--version && cargo --version'` 探测全部 10 个本地 Linux 镜像；没有可用 Rust toolchain。Docker
Desktop/代理恢复后，`docker pull rust:1.90-bookworm` 成功，digest 为
`sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f`。

经一次受限重试，当前工作树 bind mount 的 `rust:1.90-bookworm` 容器使用非 login `bash -c`、
`export PATH=/usr/local/cargo/bin:$PATH`、同一 CI 类依赖组以及
`CARGO_TARGET_DIR=/tmp/aio-target cargo test -q --locked --lib fifo -- --test-threads=1 --nocapture`。前置
检查确认 `/usr/local/cargo/bin/cargo` 和 `cargo 1.90.0 (840b83a10 2025-07-30)`，依赖安装也成功。
首次 Unix test module 编译暴露 5 个 Rust 错误：

```text
E0433 src/domain/image_gen/tests.rs:1656: libc::mkfifo 未链接
E0433 src/infra/config_migrate/tests.rs:2469: libc::mkfifo 未链接
E0277 src/infra/config_migrate/tests.rs:2473,3004,3032: SkillFileExport 未实现 Debug，expect_err 无法编译
```

按任务范围最小修复 Unix-only test `libc` 依赖、`SkillFileExport: Debug` 和 F14 production fixture 的
`image-1.png` 文件名后，同一真实 Docker/Linux 命令通过：Image Gen child watchdog 1 passed、config
export child watchdog 1 passed，汇总 `2 passed; 0 failed`。Windows cfg、静态审计或替代测试均未作为
Unix FIFO 通过证据。

## Follow-up Findings F16-F24

以下为当前独立 Codex `gpt-5.6-sol / effort=max` 只读审核新增的九项 finding。生产行锚点按当前
worktree 记录；F1-F15 的历史实现/验证记录不作为本节关闭证据。

### F16 · P1 · settings success path uses stale snapshot

`settings_service.rs:1163` coordinator 返回后，`settings_service.rs:1436` 仍用旧
`committed_settings` 做 runtime sync，`:1512` 返回旧 `SettingsView`；前端
`useSettingsPersistRunner.ts:207` 将响应当作持久 canonical。`gateway_control.rs:36` 在 owner
释放后仍可提交 preferred-port/普通设置。根因是成功路径没有 winner-aware finalization：runtime 与
返回值脱离了最新 canonical。修复必须把真实 gateway rebind、success response 和 failure rollback
统一收敛到最新 winner，并用 production barrier 覆盖成功返回及 rebind failure，不能只 reread response。

### F17 · P1 · failed config import leaves unowned imported fields

`config_migrate/mod.rs:470` whole commit S1 后 B 写 S2，A 在 `:549/:566` 失败；
`autostart.rs:698` generation 变化跳过 settings restore，`rollback.rs:372` 将其视作正常 winner。
这只保护了 whole winner，未记录 imported fields 的 per-owner ownership，故 Image Gen/Grok/rectifier
等未被 B 改写的 S1 字段残留。`config_migrate/tests.rs:528` 仅为成功 import。修复为字段/owner
CAS rollback，保留 B winner，同时恢复 A 仍拥有字段；加真实失败交错回归，不整表覆盖/整表放弃。

### F18 · P1 · autostart correction absorbs private writer B

whole-CAS 后 B 写专属字段，OS sync failure 进入 `autostart.rs:540`，只 correction `auto_start`，
但 `:562` 返回含 B 的完整 `settings_after`；` :724` whole rollback 把它当 A expected 恢复 S0。
根因是 correction snapshot 被误用为 whole commit token。修复拆分 correction token 与 A committed
snapshot，并用专属 writer barrier、OS failure 和 rollback 回归证明 B 不被覆盖。

### F19 · P1 · import tail tray write overwrites runtime winner

B 通过 `settings_service.rs:539` 写 `tray_enabled` 并同步 resident；A 成功 import 在
`config_migrate/mod.rs:592` 无条件写旧 import 值，形成 canonical/runtime 分裂，并可能触发
`resident.rs:63` 错误关闭/隐藏。修复 tray owner/coordinator 的 token/CAS tail convergence，
真实 import/tray barrier 必须证明 B winner 同时支配 canonical、resident 和返回值，消除 tail TOCTOU。

### F20 · P1 · local Skill ownership is not atomic

`rollback.rs:608` `exists` 后 `:622` 登记 ownership，`skill_fs.rs:896` 再 `exists` + `create_dir_all`；
外部工具可在窗口创建同名目录，失败 `rollback.rs:124` 仍 `remove_dir_all`。修复以父目录原子
create/create-new 取得 identity 后再登记/写入，absence-to-create race 必须保留外部目录和 bytes。

### F21 · P2 · pure Skill payload preflight is under import lock/DB transaction

`mod.rs:330` 仅浅解析，`:400` 取 lock，`:425` 开 transaction，`:458` 才进入 Skill FS；路径图、
Base64、decoded budget 在 `rollback.rs:665` 才发生。修复将完整 typed payload/path/metadata/budget
preparation 移至 lock-attempt 前，锁内只消费 prepared payload；invalid near-64MiB、path conflict
和 budget regressions 必须在 lock/DB write 前失败且无残留。

### F22 · P2 · restore success is misclassified as recovery required

`persistence.rs:572` 已 restore canonical/清 tmp，`settings_service.rs:1133` 仍把所有 finalize
failure 包装成 `SETTINGS_RECOVERY_REQUIRED`；`useSettingsPersistRunner.ts:23` 误进只读保护，
`settings_crud.rs:302` 只能排除文本。修复通过结构化 persistence classification 区分
finalize-only restore-success 与 finalize+restore double-failure，command/integration/frontend
按稳定 error code 判断，不依赖字符串 contains。

### F23 · P3 · artifact timing statement is false

`implement.md:101` 声称本轮未改 journal，`:143` 要求归档后再纠正，但 `journal-1.md:270` 已有 dirty
修改，`implement.md:176/:190` 又声称完成。修复只改工件事实，不覆盖既有 dirty diff、不新增 session，
并保留工作提交 -> 仅归档 Round 6 -> journal 独立提交的最终顺序；本实现会话按用户约束不执行这些
repository mutations。

#### F23 closure fact

当前工作树的 `journal-1.md` Testing 段已有未提交修正；内容只列出 round-5 Summary 与 archived
implement 已记录的门禁。当前 Terra 会话通过只读 `git diff` 核对该既有 diff，未修改 journal、`.pi`
或其它用户路径，也未创建 session。Round 6 工件现明确：本会话不执行 commit/archive/journal 操作；
后续协调顺序仍为工作提交、仅归档 Round 6、journal 独立提交。

### F24 · P3 · template hashes include dynamic runtime paths

`.trellis/.template-hashes.json` 已包含 24 个 `.pi/.runtime/session/__pycache__` 路径，且
`safe_commit.py:49` 禁止自动 stage runtime manifest。根因是动态运行时输入混入静态 baseline。
修复需依据 Trellis manifest 语义移除动态路径、保留合理静态 baseline，不重哈希本地定制/不删除
`.pi` 或其它 dirty 内容，并以可执行重算/校验回归证明不再漂移。

### F16-F24 Regression And Acceptance Map

- F16：真实 settings success/rebind barrier，B winner 驱动 canonical/runtime/response/persist。
- F17：真实 failed import + B 多 owner writer，字段 CAS rollback 保留 B、恢复 A 仍拥有字段。
- F18：whole-CAS + private writer + OS failure + rollback，不吸收 B snapshot。
- F19：import tail/tray barrier，canonical/resident/return 同一 winner。
- F20：absence-to-create 和 local multi-file failure 保留外部对象，仅清理原子 owner。
- F21：完整 typed payload 在 import lock/DB write 前 fail，near-64MiB/path/budget 无残留。
- F22：Rust command/integration/frontend stable classification；restore success 不只读，双失败可恢复。
- F23：dirty preservation/task facts/未执行的 commit/archive/session 顺序准确；既有 journal diff
  不由当前 Terra 会话改写。
- F24：manifest static baseline and dynamic-path exclusion executable verification。
