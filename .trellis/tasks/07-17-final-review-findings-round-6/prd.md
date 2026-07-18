# 修复第六轮最终审核发现

## Goal

关闭第六轮及后续复审中属于本任务的 F1-F23，使 settings/autostart、config import、配置资源预算
与 Image Gen storage stats 在并发、竞态和恶意文件系统对象下保持单一 canonical owner，并恢复
round-5 归档后的证据一致性，为下一次双独立只读审核建立完整前置条件。F24 Trellis template-hash
观察项按用户决定不属于本任务。

## Background

- 当前分支为 `FingerCaster/sequential-task-acceptance`；父任务
  `07-17-sequential-task-acceptance` 保持 `in_progress`。
- round-5 已归档至
  `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5`。父工件的事实投影和 journal
  session 8 Testing 均已有未提交修正；当前 Terra 会话只核对并保留这些既有 dirty 内容，不改写
  journal、`.pi` 或其它用户路径。
- 已核实的调用链、根因、测试缺口和准确行号保存在
  `research/final-review-round-6.md`；本 PRD 以 F1-F8 标识对应终审 finding。
- 用户最新指令已授权本轮直接实现并验证；子任务保持 `in_progress`，完成全部必修实现和门禁后
  停止在交付，不等待额外激活指令。
- 用户已明确禁止继续处理 F24 Trellis template-hash 问题；相关现有 dirty 文件保持原样，不在
  本任务中修复、重算、测试、清理或据此给出通过结论。

## Model And Session Boundary

- 已发生的本子任务实现保留真实 `gpt-5.6-luna / effort=max` 模型记录；剩余 F23、Docker/Linux
  验证、完整门禁与收尾只使用一个 Orca 管理的 Codex `gpt-5.6-terra / effort=max` 终端串行执行。
- 产生本节 F9-F15 findings 的已发生 Round 6 独立只读终审使用新开独立 Codex
  `gpt-5.6-sol / effort=max` 会话。
- 本子任务完成后的下一次独立只读终审和父任务最终审核，都必须针对同一冻结提交新开一对彼此隔离的
  reviewer：Codex `gpt-5.6-sol / effort=max` 与 Pi（`grok-cpa / grok-4.5`）。两者可并行，
  但不得互相交换结果；只可审核冻结提交，不得修改 tracked 文件、任务状态、分支或 remote。协调会话
  在收齐两份结果后去重、核实证据并汇总结论；不得复用历史 Luna 或当前 Terra 执行会话。
- 本文件保留已发生 round-6 终审的历史事实，不把历史终审改写为未来审核模型。

## Requirements

### R1 · P1 · settings 提交前不得产生 autostart 副作用（F1）

- `settings_set` 必须在共享 settings 锁内基于 latest 合并字段补丁、规范化、完整校验并持久化，
  durable commit 成功后才能改变 OS autostart。
- 非法候选必须同时保持 canonical settings 与 OS autostart 不变。
- autostart 失败恢复必须受精确 committed token/CAS 约束；失去所有权时保留并收敛到最新 winner，
  不得盲目恢复旧值。
- 证据锚点：`settings_service.rs:740,889,968,1013`，`app/autostart.rs:18,70`。

### R2 · P1 · `SettingsUpdate` 是字段补丁而非旧快照替换（F2）

- 普通 `settings_set` 只修改其明确拥有的字段；所有 fallback、敏感字符串 preserve 和跨字段校验
  都基于锁内 latest。
- 普通入口不得拥有 Image Gen、Grok、circuit notice、Codex session completion 或 rectifier 专属
  字段；前端普通保存 payload 同样不得携带专属字段。
- 共享 helper/owned token 必须使用显式字段清单，未来新增 `AppSettings` 字段不得自动落入普通
  owner。
- 确定性真实 writer barrier 必须证明 circuit notice、Codex completion、完整 rectifier group、
  Image Gen roots 与 Grok preferences 不会被并发普通 save 覆盖。
- 证据锚点：`settings_service.rs:426-476,664-970,1161,1206,1221`，
  `settings/persistence.rs:563`。

### R3 · P1 · 所有 autostart writer 共享串行所有权（F3）

- 普通 `settings_set`、whole config import 及其 rollback/correction 必须使用同一 autostart 串行
  协议和 commit generation/token。
- 任一副作用完成后必须按最新 canonical `auto_start` 最终收敛；较旧调用不得在较新 winner
  之后留下相反 OS 状态。
- A import true 与 B settings false 的确定性交错最终必须为 canonical=false/OS=false；同时覆盖
  OS sync 失败、token loser 与 rollback loser。
- 证据锚点：`config_migrate/mod.rs:368,389,398-425`。

### R4 · P1 · config import destructive lifecycle 必须串行（F4）

- 进程级 import lock 必须覆盖 canonical/runtime capture、DB transaction、Skill FS guard、settings/
  autostart commit、DB commit 或完整 rollback，以及 guard finish。
- 用户确认、64 MiB 文件读取、UTF-8/JSON 解析和不依赖当前状态的纯预检不得无谓放入锁内。
- stage/backup 使用随机唯一 import token，不再依赖秒级 ID；一个 import 的 rollback/finish 只能处理
  自己记录的对象。
- 重复成功导入、失败后立即重试、成功/失败组合与真实并发导入必须保持 DB/settings/Skill FS/
  CLI runtime 同属最终 winner，无残留或跨 import 删除。
- 证据锚点：`commands/config_migrate.rs:62-85`，`config_migrate/mod.rs:320,356,430,455`，
  `rollback.rs:42,206`。

### R5 · P1 · 8 MiB/64 MiB 必须是读取硬上限（F5）

- Skill export 与共享配置 reader 必须从同一 no-follow、类型/身份已验证的普通文件 handle 读取，
  最多读取 `limit + 1` 字节后立即判定超限。
- metadata 后增长、路径替换、symlink/reparse 与 FIFO/socket/device 等特殊对象不得造成完整分配、
  越权读取或阻塞。
- 合法文件在既有路径、数量和字节边界内的任意字节必须原样 round-trip。
- 证据锚点：`skill_fs.rs:215-268`，`shared/fs.rs:105-145`，
  `commands/config_migrate.rs:20-33`。

### R6 · P1 · 成功 export 必须可被同版本 import 接受（F6）

- export 与 import 共享 64 MiB 配置级 encoded budget；不得只限制各 Skill decoded total。
- 超预算 export 必须在覆盖目标文件前明确失败并保留旧目标字节。
- 任何成功 export 的完整文件必须通过 production bounded reader/JSON parser，并可恢复原 bundle；
  Base64/JSON schema、v1/v2 兼容和单 Skill 8 MiB decoded budget保持不变。
- 不得通过 Skill 内容检查、过滤、静默丢弃、截断或脱敏来满足配置级预算。
- 证据锚点：`commands/config_migrate.rs:38-56`，`config_migrate/mod.rs:20-26`。

### R7 · P1 · Image Gen storage stats 必须句柄绑定且资源有界（F7）

- storage stats 从已验证 task directory handle 相对枚举；拒绝 symlink、Windows junction/reparse、
  特殊 entry、多链接/身份变化，并以 visited identity 阻断循环或别名。
- 遍历必须有明确深度、entry 数与可表示字节预算，且单个恶意 entry 不能造成无限递归、栈溢出、
  外部目录扫描或 blocking command 挂死。
- 按现有 history API 契约，结构/身份/预算异常使整次 stats 请求 fail closed，不得静默跳过后返回
  低估值；合法 nested tree 继续精确统计。
- Windows junction 与 Unix symlink/special/race 回归必须走真实 production stats 入口。
- 证据锚点：`history.rs:2030-2077`，`image_gen_service.rs:186-196`。

### R8 · P2 · 纠正 round-5 归档证据（F8）

- 父 PRD/design/implement/research 只纠正事实：round-5 已归档、round-6 当前修复、round-6 后按
  双独立审核规则进行只读 review；旧 active 路径改为 archive 路径并勾选已完成项。
- journal session 8 的 Testing 已有基于其 Summary 与归档 implement 实际门禁的未提交修正；当前
  会话只核对并保留该既有 diff，不覆盖它、不新增 session，也不把它写成当前轮新运行的测试。
- 父 `task.json.status` 全程保持 `in_progress`；不得归档父任务或虚构新验证证据。
- 证据锚点：父 `prd.md:14,58,135`、`design.md:5,19,36`、`implement.md:55,92`、
  `research/integration-evidence-summary.md:47`、`journal-1.md:257,272`。

### R9 · 范围、镜像与仓库约束

- 只修改满足 F1-F8 所需的 production、测试、前端 ownership、生成 bindings、相关 executable
  specs/模板镜像及事实文档；不恢复已删除产品面。
- `.trellis/spec/aio-coding-hub/cross-layer` 的相关契约与
  `src/templates/markdown/spec/cross-layer` 对应文件必须逐字节一致。
- 不访问任何 remote，不执行 fetch/pull/push/`gh`；`upstream` push URL 保持 `DISABLED`。
- 不启动并发子代理；主会话只协调一个 Orca Terra 执行终端，提交、归档和 journal 严格串行。
- 不修改或验证 Trellis template-hash 生成/基线逻辑；不得触碰现有
  `.trellis/.template-hashes.json`、`.trellis/.version` 或
  `.trellis/scripts/tests/test_template_hashes.py` dirty 状态。

## Acceptance Criteria

- [x] AC1：非法 `settings_set(autoStart=true, circuitBreakerFailureThreshold=0)` 返回校验错误，
      canonical/OS 均保持 false，autostart sync 调用数为 0。
- [x] AC2：真实 settings writer barrier 证明普通 save 只修改显式 owned patch，全部专属 writer
      字段与未拥有字段保持并发 winner 值；普通前端 payload 不含 rectifier 专属字段。
- [x] AC3：settings/config import 共用 autostart generation/token 协议；post-CAS A/B、OS failure、
      correction/rollback loser 后 canonical 与 OS 最终一致且为最新 winner。
- [x] AC4：进程级 import 锁阻止 capture→rollback/finish 交错；重复、失败和并发生产路径回归证明
      DB/settings/Skill FS/runtime 一致且随机 stage/backup 无碰撞或残留。
- [x] AC5：shared fs 与 Skill handle reader 在 metadata 后增长时最多消费 limit+1，拒绝 link/reparse/
      special，路径替换不读取新对象；全部合法任意字节 round-trip 回归继续通过。
- [x] AC6：六个各自合法 Skill 造成的 >64 MiB export 在覆盖前失败并保留 sentinel；每个成功
      export 都能由 production import reader 接受并逐字节恢复。
- [x] AC7：Image Gen stats 对 Unix symlink loop、Windows junction/reparse、special、identity race、
      深度/entry/byte 超限快速 fail closed；正常 nested/multi-root 统计准确。
- [x] AC8：父任务与 journal 的 round-5 事实一致，所有 active/archive 路径准确；父状态仍为
      `in_progress`。
- [ ] AC9：聚焦 Rust/前端测试、完整 Rust lib+integration、完整前端、bindings 二次零漂移、
      typecheck、lint、format、build、all-target Clippy、`check:precommit:full`、`check:prepush`、
      Trellis manifests/镜像/task validation 与 `git diff --check` 全部通过。
- [ ] AC10：按工作提交 → 仅归档 round-6 → journal 纠正/记录提交的顺序完成；父任务不归档，
      `upstream` push URL 为 `DISABLED`，且无 remote 操作。

## Out Of Scope

- Skill 内容敏感词判断、扫描、脱敏、过滤、自动剔除、截断或按内容阻断。
- 修改配置 bundle schema、提高/降低现有单 Skill 8 MiB decoded budget，或迁移现有 Skill 内容。
- 把普通 settings writer 变成所有 `AppSettings` 字段的 owner。
- 更改 Image Gen storage view DTO 或将恶意 entry 静默忽略。
- 修复、重算或验证 F24 Trellis template-hash / safe-commit 机制；该观察项仅保留为历史记录。
- 访问 remote、合并 main、创建 PR、归档父任务或开始下一次双独立只读终审。

## Follow-up Findings F9-F15

本节追加当前独立终审对历史 F1-F8 实现提出的 follow-up。历史 F1-F8 的完成勾选仅保留为上一轮
实现记录，不作为本节 findings 的关闭证据。

### F9 · P1 · ordinary settings rollback 不得吸收 preferred-port winner

coordinator 返回到 ordinary committed/previous token 构造之间，gateway preferred-port repair
可能提交 B 的端口。A 的 token 必须直接来自 A 自己锁内 durable commit 结果；runtime sync 失败时
旧 rollback 只能按 token ownership 判定，必须保留 B winner，不得执行旧 runtime rollback。

### F10 · P1 · import early failure 不得删除旧 SSOT Skills root

Skill FS guard 必须显式区分候选路径、已创建 stage、已备份 live root、已激活新 root。stage 写入
失败或 live root rename 到 backup 前失败时，rollback 只能删除本 import 创建的 stage，旧 root
sentinel 字节必须保留且无残留。

### F11 · P1 · local Skill 中途写入失败必须清理半成品

local target 在第一次 mkdir/write 前登记本 import ownership，或由 writer 自身提供明确 cleanup。
失败时必须清理新建半成品；预先存在且不属于本 import 的目录和字节不得被删除。

### F12 · P1 · Unix Skill export FIFO race 必须 fail closed

Unix handle-relative child open 在类型验证前加入 NONBLOCK，同时保留 NOFOLLOW/CLOEXEC、identity
和 single-link 检查。枚举后 regular file 被替换为 FIFO 的 production export 必须用外部 watchdog
证明在时限内返回错误，不能只在同步返回后检查 elapsed。

### F13 · P2 · settings finalize/restore 双失败必须可恢复

canonical -> backup 成功、tmp -> canonical 失败后，必须在 restore 前捕获旧 backup ownership。restore
成功时清理 writer-owned tmp；只有没有旧 canonical/backup 且 finalize 失败时才保留 tmp 作为唯一
恢复副本。finalize 与 restore 都失败时聚合错误并返回 SETTINGS_RECOVERY_REQUIRED，保留至少一份
durable settings bytes，不声称 canonical 可用。

### F14 · P2 · Image Gen timeout 必须是真正 bounded test

storage_stats production entry 对恶意 FIFO/special/race 必须使用可终止的独立子进程 watchdog 或
等价有界机制。线程方案不得污染后续测试；Windows 无法执行 Unix FIFO 时保留 cfg 门控并如实记录
未运行限制。

### F15 · P2 · round-5 journal 证据必须一致

只依据当前 session Summary 与归档 round-5 implement 中已有真实门禁记录，替换 journal 中的
Validation was not recorded；不得虚构本轮新测试。同步当前 task/父 task 工件中的事实与执行顺序，
父 task 始终保持 in_progress。

## Follow-up Acceptance

- [x] F9：真实 coordinator-return barrier 保留 B preferred_port，旧 runtime rollback 不执行。
- [x] F10：stage/backup-rename early failpoint 保留旧 SSOT sentinel 且无残留。
- [x] F11：真实 local multi-file 中途写失败清理半成品且保留预存目录/字节。
- [x] F12：Unix export FIFO race 在外部 watchdog 时限内 fail closed，保持 no-follow identity 检查。
      Docker/Linux production watchdog 实际通过；未以 Windows cfg 或替代测试代替该证据。
- [x] F13：settings_crud 覆盖 finalize-only restore-success tmp 清理与 finalize+restore 双失败的
  durable copy、canonical、backup、tmp 状态和错误分类。
- [x] F14：production storage stats FIFO/special/race bounded regression；Docker/Linux production
      watchdog 实际通过；未以 Windows cfg 或替代测试代替该证据。
- [x] F15：journal 的既有 dirty Testing 修正与当前/父 task 工件事实一致，父状态为
      `in_progress`；本会话未覆盖该 diff，未新增 session。

## Follow-up Findings F16-F23

本节保留当前独立 Codex `gpt-5.6-sol / effort=max` 只读审核确认且属于本任务的八项新 finding。
F1-F15 的历史完成记录继续保留，但不能作为 F16-F23 的关闭证据；本轮必须以新的 production barrier、失败交错、
bounded watchdog 或跨层契约验证重新证明。

### F16 · P1 · settings 成功路径仍返回并同步旧快照

生产证据锚点：`src-tauri/src/app/settings_service.rs:1163` 的 coordinator 返回后，
`settings_service.rs:1436` 仍按旧 `committed_settings` 同步 runtime，`:1512` 仍返回旧
`SettingsView`；`src/pages/settings/useSettingsPersistRunner.ts:207` 把返回值当作持久快照。
gateway repair 入口 `src-tauri/src/app/gateway_control.rs:36` 可在 owner 释放后提交新的
`preferred_port` 或普通设置。

根因是成功路径只保护了 runtime 失败时的 rollback，未在 coordinator 成功返回和 runtime/response
收敛之间重新确认 canonical winner。A 的旧快照既可污染 runtime，也可通过前端下一次保存重写 B。

设计要求：把 durable commit、autostart correction、真实 gateway rebind 和返回 `SettingsView`
放入同一个 winner-aware convergence 协议；成功或失败都只能根据当前 canonical 快照同步副作用并
返回，旧 A snapshot 只能作为本次 owned token，不能作为成功返回值。不得只改返回值而留下旧 runtime。

回归要求：在真实 `settings_set` production barrier 中让 A 完成 commit 后暂停，B 经
`gateway_control` 写入 preferred port/普通字段，恢复 A 并完成真实 rebind；断言 canonical、runtime、
返回 view 和下一次 persist payload 都是 B winner，且旧 A runtime sync 不再发生。覆盖 successful
return 与 runtime rebind failure 两条路径。

### F17 · P1 · 失败 config import 遗留未拥有的 imported settings

生产证据锚点：`src-tauri/src/infra/config_migrate/mod.rs:470` whole commit S1，普通 writer B
随后提交 S2，A 在 `:549/:566` 失败；`src-tauri/src/app/autostart.rs:698` 因 generation 变化跳过
settings restore，而 `rollback.rs:372` 把该路径当作正常 winner。现有
`src-tauri/src/infra/config_migrate/tests.rs:528` 只有成功 import 覆盖。

根因是 import rollback 只有 whole-snapshot winner/loser 判断，没有按字段记录 A 对 imported
settings 的 ownership。generation 变化保护了 B winner，却同时让 A 仍拥有的 Image Gen/Grok/
rectifier 等 imported fields 留在 S1，造成失败 import 的部分残留。

设计要求：保存 A imported snapshot 与每个字段/owner token；rollback 只在字段当前仍等于 A imported
值时恢复 A 的 previous 字段，B 已提交或明确拥有的字段保持 S2。不得整表覆盖，也不得因任一
并发 writer 而整表放弃恢复。

回归要求：真实 import 在 whole commit 后由 B 分别写普通、Image Gen、Grok、rectifier、circuit 和
Codex 专属字段，再触发 A DB/runtime failure；断言 B fields 全部保留，A 仍拥有且未被 B 改写的
imported fields 恢复，DB/Skill FS/runtime 与最终 canonical 一致，并覆盖 generation loser。

### F18 · P1 · autostart correction 吸收专属 writer B

生产证据锚点：A whole-CAS 后 B 写入专属字段，OS sync 失败时
`src-tauri/src/app/autostart.rs:540` 只改 `auto_start`，`:562` 返回含 B 的完整 `settings_after`，
随后 `:724` 的 whole rollback 把该完整值当作 A expected 并恢复 S0。

根因是 correction 返回值把并发 canonical snapshot 与 A 的 whole commit token 混为一体；后续
rollback 没有区分 A-owned whole fields 与 B 专属字段，故能吸收并覆盖 B。

设计要求：autostart correction 只返回新的 auto-start generation/effective value；whole rollback
使用 A 原始 committed snapshot/token 和 per-owner field CAS，禁止把 correction reread 的完整
snapshot 当作 A expected。token/rollback 必须在专属 writer B 存在时保留 B。

回归要求：真实 whole import A、专属 production writer B、OS sync failure、随后 whole rollback
交错；断言 B 专属字段、B auto_start winner 和其 runtime 均保留，A 只恢复仍由 A 拥有的 fields。

### F19 · P1 · 成功 import 尾部 tray 写覆盖 runtime winner

生产证据锚点：B 在 `src-tauri/src/app/settings_service.rs:539` 提交新 `tray_enabled` 并同步
resident；A 成功 import 在 `src-tauri/src/infra/config_migrate/mod.rs:592` 仍无条件写旧
import 值，可能形成 canonical `false` / runtime `true`，并触发
`src-tauri/src/app/resident.rs:63` 错误关闭/隐藏。

根因是 tray 既被普通 settings writer 更新又被 import 尾部直接写入，尾部写没有共享 owner token、
没有以 latest canonical 做条件收敛，也没有对 resident runtime 使用相同 winner。

设计要求：tray_enabled 的 canonical commit 与 resident side effect 使用共享 tray owner/coordinator；
import 只能在其 token 仍拥有字段时提交旧 import 值，否则保留 B 并按 canonical 同步 resident。成功
返回前再次完成 owner-aware convergence，消除 reread 后新的 TOCTOU。

回归要求：真实 A import 与 B tray writer barrier 交错，B 在 import tail 前提交并同步 resident；断言
canonical/runtime/返回值一致为 B，A 不发送旧 tray side effect，且关闭/隐藏错误不发生。

### F20 · P1 · local Skill ownership 取得非原子

生产证据锚点：`src-tauri/src/infra/config_migrate/rollback.rs:608` 先 `exists`，`:622` 提前登记
import-owned；`src-tauri/src/infra/config_migrate/skill_fs.rs:896` 再次 `exists` + `create_dir_all`。
并发本地 Skill/外部工具可在 absence-to-create 窗口创建同名目录，失败路径
`rollback.rs:124` 会 `remove_dir_all` 删除对方。

根因是“缺失检查”和“创建/ownership 登记”分离，guard 没有通过原子根目录创建取得身份；失败
rollback 仅凭路径认为目录属于本 import。

设计要求：用同一父目录下的原子 `create_dir`/create-new 取得根目录 owner identity；创建成功后才
登记并允许写入。若已存在，拒绝本 import 或按已验证的外部对象路径返回，不得登记其 ownership。
rollback 只删除记录的创建结果/identity 匹配对象。

回归要求：在 absence-to-create barrier 中让外部工具先创建同名目录，再让真实 import 失败；断言
外部目录和 sentinel bytes 保留，import 不调用其 remove，且无半成品残留。覆盖本 import 原子创建后
的中途写失败。

### F21 · P2 · Skill payload preflight 仍在全局锁与 DB transaction 内

生产证据锚点：`src-tauri/src/infra/config_migrate/mod.rs:330` 仅浅解析，`:400` 获取 import lock，
`:425` 已写 DB transaction，`:458` 才进入 Skill FS；完整路径图、Base64、decoded budget 直到
`rollback.rs:665` 才做。

根因是 import orchestration 把与当前 canonical 状态无关的 payload validation 留在 destructive
阶段，导致 invalid bundle 能持有全局锁、打开 transaction、甚至产生 staging side effect。

设计要求：在 lock-attempt 前完成完整 typed `PreparedSkillPayload`：路径/marker/ancestor graph、
Base64 derived cap、decoded bytes、metadata 完整性和 per-file/total budget；锁内只消费 prepared
payload，不重复解析或进行无界 allocation。纯预检失败必须发生在获取 import lock 和 DB write 前。

回归要求：invalid near-64MiB bundle、Base64 cap violation、路径冲突和 decoded budget violation
在 lock-attempt hook 以及 DB-write hook 前失败；断言没有等待/持有 import lock、没有 transaction write、
没有 stage/Skill FS/DB 残留。有效 typed payload 仍可完整 import。

### F22 · P2 · F13 restore 成功仍误报 recovery-required

生产证据锚点：`src-tauri/src/infra/settings/persistence.rs:572` 已能 restore canonical 并清理
tmp，但 `src-tauri/src/app/settings_service.rs:1133` 对所有 failed-to-finalize settings 统一包装
`SETTINGS_RECOVERY_REQUIRED`；前端 `src/pages/settings/useSettingsPersistRunner.ts:23` 因此进入只读
保护，`src-tauri/tests/settings_crud.rs:302` 只通过排除 restore failed 文本规避。

根因是错误分类只由 finalize error 文本驱动，持久化层没有结构化表达 finalize-only、restore-success
和 finalize+restore 双失败；调用层不能区分 canonical 已恢复与不可确认。

设计要求：引入结构化 settings persistence failure 分类/错误码。finalize-only 且 restore 成功返回
普通持久化错误并清理 writer-owned tmp；只有 restore 失败或双失败才返回
`SETTINGS_RECOVERY_REQUIRED`，并保留至少一份 durable bytes。command、integration、frontend
按分类判断，不依赖脆弱字符串包含。

回归要求：覆盖 finalize-only restore-success 与 finalize+restore double-failure 的 command、
integration、frontend classification；断言 canonical/backup/tmp/bytes 状态及 UI 是否进入只读保护
精确匹配。

### F23 · P3 · F15 工件时序陈述不实

生产/工件证据锚点：`implement.md:101` 声称本轮未改 journal，`:143` 要求归档后才纠正；但
`.trellis/workspace/FingerCaster/journal-1.md:270` 已有 dirty 修改，且 `implement.md:176/:190`
已声称完成。

根因是当前工件把旧会话的实际 dirty 状态、Round 6 执行顺序和历史完成记录混写，形成不可审计的
时序陈述。

设计要求：不丢弃或覆盖用户 dirty diff，不新增虚假 session；把任务工件改为准确事实，最终仍按
工作提交、仅归档 Round 6、journal 独立提交的顺序收尾。本轮用户约束明确禁止实际 commit/archive/
journal 操作，因此本会话只修正陈述与验证结果，保留待协调会话执行的后续顺序。

回归要求：检查 `git diff`/`git status` 证明 journal、`.pi` 和其它用户路径未被覆盖；task 工件
准确标注当前 active Round 6、未执行的提交/归档/session 动作，不新增 session 文件。

## Withdrawn Audit Note F24 · Trellis template hash

F24 是审核时记录的 Trellis template-hash 观察项。用户已明确该项不属于本任务，并要求停止处理
Trellis；以下证据仅作为历史记录，不构成设计、实现、测试或验收要求。

生产证据锚点：`.trellis/.template-hashes.json` 当前重算已包含 24 个动态 `.pi/.runtime/session/
__pycache__` 路径；`.trellis/scripts/common/safe_commit.py:49` 又禁止自动 stage 该 runtime
manifest。

根因是 manifest 生成/校验把机器运行时路径当作静态模板 baseline，同时 safe commit 对动态 manifest
采用拒绝策略，导致每次运行漂移且无法自洽。

不得在本任务中修改、重算、验证或清理相关文件；现有 dirty 内容按用户所有权原样保留。

## Follow-up Acceptance F16-F23

- [x] F16：真实 settings success barrier 后 canonical、runtime、返回 view 和下一次 persist 都收敛到 B winner；不残留旧 A runtime。
- [x] F17：失败 import 按字段/owner rollback，保留 B winner 并恢复 A 仍拥有字段，不整表覆盖或整表放弃。
- [x] F18：autostart correction/whole rollback 不吸收或覆盖专属 writer B，token ownership 可证明。
- [x] F19：真实 import/tray barrier 后 canonical/runtime/返回值一致为最新 tray winner，无 TOCTOU 尾写。
- [x] F20：local Skill absence-to-create race 保留外部目录/字节，rollback 只清理本 import 原子创建对象。
- [x] F21：完整 typed Skill payload 在 lock-attempt/DB write 前失败；无 lock、transaction、stage 或 FS 残留。
- [x] F22：finalize-only restore-success 是普通错误，双失败才是结构化 recovery-required；command/integration/frontend 分类一致。
- [x] F23：工件准确描述既有 dirty journal 与未执行的提交/归档/session 顺序；本会话未覆盖
      用户 diff，未新增虚假 session。
