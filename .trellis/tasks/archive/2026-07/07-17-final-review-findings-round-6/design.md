# Design: Round 6 所有权、预算与句柄收敛

## 1. 总体边界

本轮不新增产品能力，只收紧四条既有数据流：

```text
SettingsUpdate
  -> AUTO_START owner lock（仅含 auto_start 的 writer）
  -> settings::update 锁内 owned patch + validate + durable commit
  -> OS autostart / gateway/runtime side effects
  -> token-checked rollback 或 latest-canonical convergence

config bundle bytes（确认、bounded read、JSON parse 在锁外）
  -> CONFIG_IMPORT lock
  -> capture -> DB tx + Skill FS guard + whole-settings/autostart commit
  -> commit + guard.finish  或  complete rollback

trusted Skill/config file
  -> no-follow open + handle type/identity
  -> take(limit + 1)
  -> unchanged Base64/JSON bytes

validated Image Gen task handle
  -> relative no-follow enumeration/open
  -> identity/visited + depth/entry/byte budgets
  -> exact total 或整次 fail closed
```

全局锁序固定为：

```text
CONFIG_IMPORT_LOCK -> AUTO_START_LOCK -> SETTINGS_WRITE_LOCK
```

## Model And Session Boundary

- 已发生的实现保留真实 `gpt-5.6-luna / effort=max` 模型记录。
- 剩余 F23、Docker/Linux 验证、完整门禁与收尾只由一个 Orca 管理的 Codex
  `gpt-5.6-terra / effort=max` 终端串行执行；不得并发创建第二个执行终端。
- 产生本节 F9-F15 findings 的已发生 Round 6 独立只读终审使用新开独立 Codex
  `gpt-5.6-sol / effort=max` 会话。
- 下一次独立只读终审以及父任务最终审核都必须针对同一冻结提交新开一对彼此隔离的 reviewer：
  Codex `gpt-5.6-sol / effort=max` 与 Pi（`grok-cpa / grok-4.5`）。两者可并行但不得交换结果，
  且仅可读审核，不能修改 tracked 文件、任务状态、分支或 remote；协调会话统一去重、核实证据并汇总。
  不得复用历史 Luna 或当前 Terra 执行会话。
- 已发生 round-6 终审的模型事实按历史记录保留，不用未来审核规则改写。

普通 settings writer 不获取 import lock；不涉及 `auto_start` 的专属 writer只获取 settings lock。
任何代码都不得在持有 settings lock 时获取 autostart lock，避免反向等待。

## 2. Settings 字段补丁与所有权

### 2.1 锁内 patch helper

删除 `write_settings_snapshot` 的“整份替换后保留三字段”模型，建立显式
`apply_settings_update_owned_patch(latest, update)`。该 helper 只在 `settings::update` closure 内调用：

1. 从 `latest` 解析所有 `Option` fallback；`Preserve` password 也读取 latest。
2. 对字符串 trim、retry policy sanitize 和默认值规范化。
3. 只赋值普通入口 owned fields。
4. 把 schema version 提升到当前值。
5. 对合并后的完整 latest 运行 `settings::validate_bounds` 与 proxy validation。
6. durable write 成功后返回 `previous`、`committed` 与显式 owned token。

owned token 采用专门结构/比较函数逐字段列举，不再通过 clone 整对象后“清空少数字段”实现。
未来新增 `AppSettings` 字段不会因 serde 全对象比较自动加入普通 owner。

### 2.2 Owner 表

| Writer | 唯一/合法 ownership |
| --- | --- |
| 普通 `settings_set` | PRD R2 中列出的 UI/网关、retention/timeout/failover、通用开关、WSL/Codex、CX2CC、upstream proxy 字段 |
| Image Gen storage | `image_gen_storage_dir`, `image_gen_storage_roots` |
| Grok config/CLI proxy | `grok_proxy_preferences` |
| circuit notice | `enable_circuit_breaker_notice` |
| Codex completion | `enable_codex_session_id_completion` |
| gateway rectifier | 12 个 rectifier/response-fixer 字段 |
| config import | whole-snapshot CAS；不改变其它 writer 随后提交的 winner 语义 |
| gateway effective-port repair | 条件拥有 `preferred_port`；仍通过锁内 update/CAS 与普通 writer last-commit-wins |

普通 `SettingsUpdate` 删除 rectifier 专属字段。serde 对旧客户端多余 JSON 字段保持兼容忽略；生成
bindings 与 `src/services/settings/settings.ts` 的普通映射同步收窄。设置页若继续展示 rectifier
控件，保存时由现有 `settings_gateway_rectifier_set` 负责：页面层按 owner group 分拆 pending keys，
从最新 settings cache 组成完整 rectifier DTO，分别结算成功/失败的 keys，不伪装跨命令原子性。

### 2.3 真实 writer barrier

在测试构建中把一次性 barrier 放在真实 `settings_set_impl` 完成纯输入准备、即将进入
`settings::update` 之前。A 暂停时，B 分别调用真实 circuit、Codex、rectifier production writer，
并通过 Image Gen/Grok production writer 或其真实锁内入口写专属值。A 恢复后断言：

- A 的普通字段已提交；
- B 的全部专属字段逐项保持；
- A 返回的 `SettingsView` 与重新读取的 canonical 一致；
- 普通前端 generated command args 不出现 rectifier 字段。

测试不得再依赖 `test_support.rs` 中重复实现的伪 command path；该 helper 改为调用真实 service，
或把回归放在能构造真实 `DbInitState` 的 service test 中。

## 3. 共享 autostart owner 协议

### 3.1 Coordinator 与 token

`app/autostart.rs` 拥有进程级 mutex 和状态：

```rust
struct AutoStartOwnerState { generation: u64 }
struct AutoStartCommitToken {
    generation: u64,
    previous: bool,
    committed: bool,
}
```

所有会修改 canonical `auto_start` 的生产路径都通过 coordinator：普通 settings commit、whole
config import CAS、它们的 correction/rollback。协调器在持锁时执行 settings commit closure，只有
commit 成功才递增 generation 并产生 token。generation 解决布尔值 ABA：较新 writer 即使提交相同
布尔值，旧 token 也不再拥有 rollback 权。

### 3.2 Commit 后副作用与失败恢复

1. commit closure 在 settings lock 内合并/校验/持久化；失败时 generation 和 OS 均不变。
2. commit 成功后仍持 `AUTO_START_LOCK`，读取 canonical target，并对 OS 执行 sync。
3. OS sync 成功后再次以 canonical 为最终值返回。
4. OS sync 失败时，仅当 `state.generation == token.generation` 且
   `latest.auto_start == token.committed`，才在 `settings::update` 中只恢复 auto_start previous；
   其它字段保持已提交值。
5. correction 本身递增 generation。随后从最新 canonical 强制 sync；若 token 已失权则跳过写入，
   只同步 winner。
6. correction persistence 或最终 OS sync 失败返回组合错误，canonical 仍是唯一权威；禁止盲写旧
   snapshot。

保持当前 best-effort 产品语义：单纯 OS enable/disable 失败且 previous correction 与最终收敛成功
时，其它普通/import 字段仍可成功提交，返回的 `auto_start` 是实际 effective canonical。只有无法
建立 canonical/OS 收敛时整次操作报错。

### 3.3 延后 runtime 失败时的 rollback

gateway/runtime sync 发生在 autostart commit 后。若之后失败，rollback 重新获取
`AUTO_START_LOCK`，使用原 token 与普通 settings owned token：

- 两个 token 均仍拥有时，只恢复本次普通字段，并把 OS 收敛到恢复后的 canonical；
- 任一 token 被较新 owner 击败时，不恢复旧 runtime/OS，读取并同步 canonical winner；
- whole import rollback 继续用 whole-snapshot CAS，但其 autostart 部分必须由 coordinator 执行。

### 3.4 并发 winner 语义

A import true 在 commit→OS 期间持有 owner lock，B settings false 只能等待。A 完成后 B 获锁、提交
false 并关闭 OS，最终 winner 是 B。若 A 在后续 runtime rollback 时恢复，B 已增加 generation，A
只能按 B 的 canonical 收敛。所有 OS call 在测试 hook 中记录 target 序列，最终一项必须等于最新
canonical。

## 4. Config import 全生命周期串行化

### 4.1 锁外准备

`commands/config_migrate.rs` 保持以下顺序，均不持 `CONFIG_IMPORT_LOCK`：

1. trim/validate `file_path`；
2. `RiskyIpcConfirm::require`；
3. ensure DB ready；
4. no-follow 64 MiB bounded read、UTF-8 与 JSON parse；
5. schema、路径、Base64 length 和其它不读取当前 canonical 状态的纯 payload preflight。

这样用户确认、慢磁盘读取和大 JSON parse 不阻塞另一个已经进入 destructive lifecycle 的 import。

### 4.2 锁域

纯 preflight 产出 `PreparedConfigImport` 后获取进程级 mutex。锁内从第一次 state capture 开始：

```text
previous settings + CLI runtime capture
  -> DB transaction / current DB snapshot
  -> randomized Skill stage + backup + activation guard
  -> whole-settings CAS through autostart coordinator
  -> CLI runtime sync
  -> DB commit
  -> SkillFsImportGuard.finish + resident/tray finalization
```

任一失败在释放锁前完成 settings/autostart owner-aware rollback、Skill guard rollback、DB/runtime
恢复。成功 `finish` 后才允许下一 import capture。`SkillFsImportGuard::finish` 改为返回结果或明确只把
backup cleanup 作为可审计残留错误；不能静默掩盖会影响当前 root ownership 的失败。

### 4.3 唯一 token 与文件系统 guard

用 `rand::RngCore` 生成至少 128 bit token，结合 PID 形成不可预测目录名；用 `create_dir`/create-new
碰撞重试，不再先 `remove_dir_if_exists` 一个秒级共享名字。guard 精确保存本 import 的 stage、backup、
activated root 与 local backup paths。全局锁阻止 live import 竞争，随机 token 处理同秒重复与崩溃
残留；不需要用“删除当前 root 再猜 owner”的无身份并发协议。

### 4.4 回归矩阵

- success→success 同秒重复：第二次完整替换第一轮，无目录碰撞/残留。
- failure→success：第一轮在 `drop(tx)` 后 barrier 暂停 rollback，第二轮必须尚未 capture；释放后 A
  完整 rollback，B 才完成，最终 DB/settings/FS/runtime 全为 B。
- success→failure：失败 import 恢复到刚完成的 success，而非更早备份。
- CAS loser、runtime sync failure、DB commit failure：每种错误都在释放锁前完成补偿。

## 5. 真正有界的文件读取

### 5.1 共享 handle reader

在 `shared/fs.rs` 分离两层：

```text
open_regular_file_no_follow(path) -> Option<File>
read_open_file_with_max_len(&mut File, max_len) -> Vec<u8>
```

第二层 capacity 最多为 `min(handle_metadata_len, max_len)`，用 checked `max_len + 1` 和
`Read::take`；第 `max_len+1` 字节出现即报 too-large。它从不调用 `read_to_end` 于无限 reader，也不
重新按路径打开。

Unix open 带 `O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK`，随后 `fstat` 必须为 regular file；这使 FIFO
不会在类型检查前阻塞。Windows 以 `FILE_FLAG_OPEN_REPARSE_POINT` 打开，handle attributes 必须是
非 directory/非 reparse，`GetFileType` 必须为 disk。NotFound 保持 optional helper 的 `None` 语义；
其它对象返回显式错误。

### 5.2 Skill collector 复用

`SkillFileCollector::push_file` 已接收经 parent handle relative-open、entry identity、single-link
验证的 `File`。它直接调用 shared handle reader，并保留 `SKILL.md` 256 KiB、source metadata
64 KiB、普通文件/Skill total 8 MiB 的现有选择。测试 hook 位于 handle metadata 与 bounded read
之间，用追加写制造确定性增长。

### 5.3 竞态语义

- open 前 symlink/reparse：拒绝；
- open 后路径被 rename/替换：只读取已捕获原 handle，绝不读新路径对象；
- 同一已打开文件增长：最多读取 limit+1 后拒绝；
- special/device：在 read 前拒绝；
- 合法二进制、零字节与敏感外观任意 bytes：不解释内容，逐字节返回。

## 6. 配置级 encoded budget

`config_migrate` 定义一个 SSOT：

```rust
CONFIG_BUNDLE_ENCODED_MAX_BYTES = 64 * 1024 * 1024
```

import path 与 export pretty serializer 同时引用它。export 使用实现 `std::io::Write` 的 capped Vec
writer；writer 在下一次 write 会超过 budget 时返回可识别 overflow，不让 serialized buffer 超过
limit+1。完成序列化后才调用 `shared::fs::write_file_atomic` 覆盖目标，因此 overflow/serialization
失败保留原目标。

预算计算对象是最终 pretty JSON bytes，天然包含 Base64、字段名、转义、metadata 与换行开销；不
依赖不精确的 raw×4/3 估算。ConfigBundle schema 与每个 Skill decoded budget不变。

性质测试分两层：小 max 的 serializer 单元边界；生产 64 MiB 常量下以六个合法大 Skill 触发真实
command 拒绝并检查 sentinel。任一成功写出的 export 再由同一 production bounded reader 解析。

## 7. Image Gen stats 的 handle-relative walker

### 7.1 Authority 与 entry 模型

`validate_task_dir` 已返回持有 `root_handle`、`task_handle`、root/task identity 的
`ValidatedTaskDir`。新 walker 直接以 `task_handle` 为根，不再调用 path-based `read_dir`。

每个 entry 的流程：

1. parent handle 相对 no-follow 枚举/lstat，记录 name、类型与 identity；
2. 增加全局 entry 计数并检查深度；
3. symlink/reparse/special 立即 `SEC_INVALID_INPUT`；
4. regular file 相对 no-follow 打开，比较 handle identity、普通类型与 single-link，再从 handle
   metadata 取得 size；
5. directory 相对 no-follow 打开，比较 identity；visited 新增成功后递归，否则按结构别名/循环
   fail closed；
6. checked 累加 size，超过 `i64::MAX` 拒绝。

Windows 可扩展现有 `windows_directory_entries` 以保留 reparse attributes，并以专用只读 relative
open 获取 handle；Unix 复用 `rustix::fs::Dir::read_from`、`statat(SYMLINK_NOFOLLOW)`、
`openat(NO_FOLLOW|DIRECTORY)` 模式。walker 使用显式栈或每层先检查 depth，避免无界栈增长。

### 7.2 预算与 API 语义

- max depth：64；
- 一次 `storage_stats_with_roots` 的所有 task 合计 max entries：100,000；
- max bytes：`i64::MAX`，与现有 `ImageGenStorageView.total_bytes` 可表示范围一致；使用 checked sum，
  不再 saturating。

预算和结构异常使整次请求失败；不跳过恶意 entry。普通 transient enumerate/open/identity error 也
终止请求，因为返回部分 total 会违反现有“无效 selected row 整体失败”契约。正常 task_count 与
current dir 字段不变。

### 7.3 回归

- Unix `loop -> .` symlink 与 Windows junction 指向 task/root/outside：在 timeout 内返回错误；
- socket/FIFO/其它 special；
- depth 65、100001 entries、模拟 byte overflow；
- 枚举后将 entry 替换为 link/junction/不同 identity；
- 合法 nested files、多个 roots 与共享总预算的精确统计。

## 8. 证据纠正

父 PRD/design/implement/research 已更新为 round-5 archived、round-6 current 与下一次双独立
read-only review 的事实，父 `task.json.status` 保持 `in_progress`。journal session 8 Testing 已有未提交修正，其内容只来自现有 Summary、round-5
archived implement 与门禁记录；当前 Terra 会话只核对并保留该 diff，不修改 journal、不重述本轮
测试，也不新增 session。

## 9. 兼容、回滚与提交形状

- Settings JSON schema 不变；旧普通 IPC payload 的多余 rectifier 字段被 serde 忽略，当前前端改走
  专属 writer。
- ConfigBundle v1/v2、Base64、合法 Skill bytes 和单 Skill budget不变；只新增最终 encoded 上限的
  对称执行。
- Image Gen DTO 与正常统计结果不变；仅不安全/超预算树从可能挂死或越界扫描变为显式错误。
- 每个新协议都有 failpoint/barrier；实现失败时按模块回退，不通过放宽测试或内容过滤绕过。
- 最终协调顺序严格为：工作/测试/spec/父事实工件提交 → 仅归档 round-6 → 含既有 journal Testing
  修正的独立 journal 提交。当前会话不执行这些仓库操作，也不为该顺序新增虚假 session；父任务不
 归档，不访问 remote。

## Follow-up Design F9-F15

历史 F1-F8 设计与门禁记录保留。本节是独立终审追加的约束，不能用历史完成勾选代替本节验证。

### F9

commit_settings_update_owned 必须保留 coordinator 内 settings::update 返回的 previous token、
committed snapshot 和 ordinary committed token。coordinator 返回后只允许把其自身 correction 的
effective auto_start 写入这份 A-owned 结果；禁止 reread canonical 构造 token。测试 hook 放在
coordinator return 与 map/token construction 之间，B 通过 production preferred-port repair 写入
新端口，随后注入 runtime failure，rollback 必须返回 ConcurrentWinner 并同步 B。

### F10/F11

SkillFsImportGuard 增加 stage path、backup ownership、activation ownership 等明确状态。rollback
顺序为清理 import-owned stage/local outputs、恢复自己的 local backups、仅删除 activated SSOT
replacement、恢复自己的 SSOT backup；candidate 或 pre-backup live root 永远不是删除对象。local
target 在 write_prepared_skill_files_to_dir 前登记，writer 中途 failpoint 验证真实 apply path 的
cleanup。

### F12/F14

Unix Skill export child open 使用 RDONLY + NOFOLLOW + CLOEXEC + NONBLOCK 后再验证 regular type；
生产 race 回归在独立测试子进程中安装 enumeration barrier，将 regular file 替换为 FIFO，由父测试
进程以 bounded try_wait/kill watchdog 负责终止挂死 child。Image Gen stats 使用同样外部 watchdog
覆盖 task-handle production entry 的 FIFO replacement；Unix 测试 cfg(unix)，Windows 不虚构通过。

### F13

settings persistence 在 finalize attempt 前捕获 had_backup。restore 成功/失败都进入统一 cleanup
分支；有旧 backup 时 finalize failure 后清理 tmp，双失败保留 backup bytes；无旧 durable owner 时
才保留 tmp。双失败错误聚合为 SETTINGS_RECOVERY_REQUIRED，并显式说明 canonical 未确认可用。
settings_crud 真实 command path 同时检查 canonical/backup/tmp 和 bytes。

### F15

现有 journal session 8 Testing 的既有 dirty 修正只依据已有 Summary 与 round-5 archived implement
的真实记录；当前 Terra 会话不改写该行、不新增 session、不声称本轮测试。当前 task 与父 task 的
round-5 archive、round-6 current、下一次双独立 read-only review 顺序保持一致。

## Follow-up Design F16-F23

历史 F1-F15 的设计、实现和验证记录保持不变。本节定义本轮新 finding 的独立设计边界，任何旧
checkbox 都不替代本节的 production regression。

### F16 · Canonical success convergence

`settings_set` 在 durable commit 后把本次 owned token 与 canonical generation 传入一个 winner-aware
success finalizer。该 finalizer 在真实 gateway rebind 前后读取 canonical，仅以当前 winner 同步
runtime，并从最终 canonical 构造 `SettingsView`。coordinator 返回与 token construction 间的
preferred-port/ordinary writer barrier 必须使 A 的 token保持 A-owned字段；成功路径也必须处理 B
winner，而不只处理 runtime failure。runtime failure 走同一 finalizer 的 owner-aware rollback，
不得使用 A 的旧完整 snapshot。

### F17 · Field-aware import rollback

whole import 保存每个 imported field 的 A token/previous value。失败时先用 shared autostart owner
判断 `auto_start` generation，再在 settings lock 内按字段 compare-and-set：当前仍为 A imported
value 的字段恢复 A previous，当前属于 B 或其它 writer 的字段保持不动。DB/Skill FS rollback
只在同一 import lock 内完成；rollback 结果必须返回 restored/concurrent-winner/failed 分类。

### F18 · Correction token separation

autostart correction 的返回类型拆分为 `effective_auto_start` 与 correction token，不再返回并发
canonical 全快照作为 whole import committed snapshot。whole rollback 使用 A capture/commit 的
expected snapshot及其 owner tokens；correction 只能更新 A 的 auto_start token，不能把 B 专属字段
吸入 expected。

### F19 · Tray owner coordinator

tray_enabled 由 canonical tray owner coordinator 统一处理。import tail 写入前必须持有/验证 import
tray token；若 B 已提交，A tail 返回 concurrent winner 并立即按 canonical 同步 resident。成功返回
只从同一 finalizer 的 canonical snapshot 生成，禁止 reread 后无条件写旧 import value。

### F20 · Atomic local root ownership

local Skill target 由父目录的原子 `create_dir` 或等价 create-new 操作取得；absence 检查仅用于诊断，
不产生 ownership。guard 记录创建后的 identity 与 path，writer 只接收该 owned handle/path；若外部
对象先创建，返回冲突且不登记。rollback 以 recorded identity 为准，仅清理 import-owned local
output，绝不依据一个后来变为存在的路径执行 `remove_dir_all`。

### F21 · Typed preflight outside destructive lock

把 payload preparation 拆成纯函数/typed object：bounded bundle bytes -> decoded/normalized path
graph -> validated metadata -> `PreparedSkillPayload`。该阶段在 import lock 和 DB transaction
之前完成所有路径、marker、Base64、decoded/file-count/total budget 检查；lock 内只执行 typed
payload 的 apply。near-limit invalid cases 要在 lock-attempt 和 DB-write hooks 前 fail closed，
同时保留 import lock 对 canonical capture 到 finish/rollback 的完整生命周期覆盖。

### F22 · Structured settings persistence classification

持久化层返回结构化 `SettingsPersistenceFailure`，至少区分 `FinalizeOnly`、`RestoreSucceeded`、
`RecoveryRequired` 和双失败聚合。command/service 将结构化分类映射到稳定错误码；frontend model
按 error code 判断只读保护。finalize-only restore-success 清理 writer-owned tmp，双失败保留
backup 或唯一 durable tmp，不声明 canonical 可用。

### F23 · Factual artifact state

任务工件把已有 journal dirty diff、已发生的历史验证和本会话未执行的提交/归档/session 分开
描述。不得伪造 session 或覆盖 journal；本会话只记录事实和待协调会话的顺序，最终 staging/commit
顺序继续是工作提交、仅 Round 6 归档、journal 独立提交。

## Withdrawn Design Note F24

F24 Trellis template-hash 观察项按用户决定移出本任务。不得实现或验证此前拟议的 manifest/safe
commit 修改，也不得触碰现有 template hash、version、测试脚本、`.pi`、journal 或其它 dirty 内容。
