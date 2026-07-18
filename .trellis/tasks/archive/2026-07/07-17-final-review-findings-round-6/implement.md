# Implementation Plan: 第六轮终审 findings

## Model And Session Boundary

- 已发生的本子任务实现保留真实 `gpt-5.6-luna / effort=max` 模型记录。
- 剩余 F23、Docker/Linux 验证、完整门禁与收尾只使用一个 Orca 管理的 Codex
  `gpt-5.6-terra / effort=max` 终端串行执行；不得启动第二个并发执行终端。
- 产生 F9-F15 的已发生 Round 6 独立只读终审使用新开独立 Codex
  `gpt-5.6-sol / effort=max` 会话。
- 下一次独立只读终审和父任务最终审核都必须针对同一冻结提交新开一对彼此隔离的 reviewer：
  Codex `gpt-5.6-sol / effort=max` 与 Pi（`grok-cpa / grok-4.5`）。两者可并行但不得交换结果，
  且仅可读审核，不能修改 tracked 文件、任务状态、分支或 remote；协调会话统一去重、核实证据并汇总。
  不得复用历史 Luna 或当前 Terra 执行会话。

## Planning And Context Gate

- [x] 完整保存 F1-F8 的调用链、根因、测试缺口与最小修复边界到
      `research/final-review-round-6.md`。
- [x] 完成 converged `prd.md`、技术 `design.md` 与本执行计划。
- [x] 为 inline 后续执行整理 `implement.jsonl` / `check.jsonl` 的真实 spec/research 条目，不保留
      seed-only 状态。
- [x] 用户最新指令授权实现后进入 in_progress；旧 planning-only 措辞已覆盖。
- [x] 进入实现后加载 `trellis-before-dev`，重读 research/PRD/design/implement 与 cross-layer contracts。

## Implementation Order

### 1. Shared bounded file-handle reads

- [x] 在 `src-tauri/src/shared/fs.rs` 建立跨平台 no-follow regular-file opener 与
      `limit + 1` handle reader；Unix 在 fstat 前使用 nonblocking open，Windows 验证 disk/
      non-reparse/non-directory handle。
- [x] 保持 optional NotFound/error 映射与所有现有 call sites 的返回契约；不得重新按路径读取。
- [x] 将 `config_migrate/skill_fs.rs::SkillFileCollector` 改为直接有界读取已完成 relative-open、
      identity 与 single-link 校验的同一 handle。
- [x] 增加 metadata 后增长、open 后路径替换、exact limit/limit+1、任意二进制 round-trip 与
      Unix symlink/directory/FIFO 拒绝测试，以及 Windows reparse/junction cfg 测试；Windows-only
      分支由对应平台门禁执行。

### 2. Settings owned patch and autostart coordinator

- [x] 在 `app/autostart.rs` 建立共享 owner mutex、单调 generation、`AutoStartCommitToken` 和
      commit/correction/rollback/final-convergence API；settings 与 config import production writer 接入。
- [x] 固定锁序 `CONFIG_IMPORT -> AUTO_START -> SETTINGS_WRITE`，settings closure 内不获取 autostart lock。
- [x] 删除 `write_settings_snapshot` 整份替换与排除字段 owner equality；实现
      `apply_settings_update_owned_patch` 与逐字段 `SettingsServiceOwnedToken`。
- [x] fallback/trim/password preserve/retry sanitize/bounds/proxy validation 进入 `settings::update`
      锁内；OS autostart 仅 durable commit 后执行。
- [x] 从普通 `SettingsUpdate`、generated bindings、`services/settings/settings.ts` 与 persistence model
      移除 rectifier 专属字段；专属 writer 保留。
- [x] production `settings_set` 路径改走 owned patch + autostart coordinator；test helper 调用真实 generic path。
- [x] 组合原子 rollback：`rollback_owned_with_auto_start_token` 在 AUTO_START→SETTINGS 内同时校验
      generation / auto_start token / ordinary owned token；禁止先回滚 auto_start 再比较 committed_token。
- [x] preferred_port repair 仅在 canonical 仍等于本次 committed 值时条件写入；成功只更新
      `committed_token.preferred_port`，loser 保留原 token（不整表 from_settings(winner)）。
- [x] OS correction 成功后返回当前 generation token（g2）；generation 用 checked_add fail-closed。
- [x] 非法候选保持 `SEC_INVALID_INPUT`，不误标 `SETTINGS_RECOVERY_REQUIRED`。
- [x] 回归：非法 autoStart+threshold=0 零 OS 调用；真实 production owner paths barrier（circuit/
      Codex/rectifier/Image Gen/Grok）；runtime failure 恢复 ordinary+autostart；OS correction token
      世代；generation overflow；preferred_port token 成功/loser 语义；stale token loser。

### 3. Config import serialization and autostart ownership

- [x] 纯 preflight（`prepare_config_import`）与 destructive apply 分离；确认/64 MiB 读取/JSON parse
      保持在 import mutex 外。
- [x] 增加进程级 `CONFIG_IMPORT_LOCK`，覆盖 capture→commit/finish 或完整 rollback。
- [x] whole-settings CAS + autostart 接入共享 coordinator；`WholeSettingsCommitResult` 区分
      ConcurrentUpdate / Committed / CommitNeedsRollback / Failed；CAS 失败路径先 drop(tx)。
- [x] whole-snapshot token-aware rollback：`rollback_whole_settings_with_auto_start_token`；
      concurrent winner 不得当作本次 committed snapshot 做 whole-rollback。
- [x] stage/backup 改为随机 >=128-bit import token + create-new retry；`finish` 返回可审计错误。
- [x] SkillFsImportGuard::rollback 聚合 live-root/cleanup 错误；live-root 失败升级
      `SKILL_FS_RECOVERY_REQUIRED` 并在持锁路径传播。
- [x] 双线程 import barrier（AtomicBool 确定性等待，非 sleep-only）；import true → settings false
      最终 winner 收敛。
- [x] failure→success / success→failure / 同秒重复 + Skill FS 残留清理矩阵；每轮逐项核对
      DB/settings/Skill FS/CLI runtime 与 stage/backup 残留。

### 4. Config-level encoded export budget

- [x] 定义共享 `CONFIG_BUNDLE_ENCODED_MAX_BYTES`，import reader 与 export serializer 共用。
- [x] capped pretty-JSON writer + production `config_export_to_path`/atomic writer。
- [x] 小预算 exact/overflow；六个含 SKILL.md、decoded<=8MiB 的合法 Skill 走 production export path
      超 64MiB 保留 sentinel。
- [x] 真实 DB/磁盘 Skill 通过 production export → bounded import reader → `config_import`，逐字节
      验证 Skill FS 与 DB payload。

### 5. Image Gen handle-relative bounded stats

- [x] 用 `ValidatedTaskDir.task_handle` 做 handle-relative stats；Unix openat/no-follow+NONBLOCK，Windows
      handle enumeration + reparse 拒绝。
- [x] 枚举阶段 `charge_stats_entry` 即时执行全局 entry budget（不先收集无界 Vec）。
- [x] identity 比较、visited set、depth=64、entries=100000、checked bytes<=i64::MAX。
- [x] 回归：junction/reparse fail closed、nested tree、depth>64 fail closed、entry 预算路径与合法
      多文件统计；Unix FIFO cfg 测试已写。
- [x] 真实 production stats 入口触发第100001 entry、`i64::MAX+1` byte hook 与 enumerate→open
      identity race；不以 200 文件成功替代 overflow 证据。

### 6. Executable specs, bindings and factual evidence

- [x] 更新 settings ownership、config migration Skill bundle、Image Gen trust boundary 合约；同步
       `src/templates/markdown/spec/cross-layer` 镜像并逐字节一致。
- [x] 生成 `src/generated/bindings.ts`，更新普通 settings 服务/persistence model 与相关前端测试。
- [x] 纠正父 PRD/design/implement/research 的 round-5 archived、round-6 current、archive path 与
       task chain；父 `task.json.status` 保持 `in_progress`。
- [x] journal session 8 Testing 已有既有 dirty 修正；当前 Terra 会话仅依据 Summary 与 archived
      implement 核对并保留该 diff，不改写 journal、不新增 session，也不把它当作本轮测试证据。

## Focused Validation

- [x] `app::autostart::` (12 passed，含 whole/correction/owned+whole rollback overflow + OS correction token)
- [x] `app::settings_service::` (8 passed，含真实 owner barrier / SEC_INVALID_INPUT / port token)
- [x] `config_migrate` (49 passed，含 production export sentinel + reader round-trip + import barrier)
- [x] `domain::image_gen` (57 passed，含 depth/reparse/nested/100001-entry/i64 overflow/identity-race stats)
- [x] `shared::fs::` (12 passed)
- [x] `--test settings_crud` (22 passed，含 settings persistence failure -> `SETTINGS_RECOVERY_REQUIRED` rollback)
- [x] 前端 settings 聚焦 vitest (53 passed)
- [x] 完整前端 unit suite (`pnpm test:unit`: 287 files / 2491 tests passed)

## Full Validation Gates

- [x] 全 Rust library / integration：`pnpm check:prepush` 完整运行 `cargo test --locked`，library 2132 passed / 3 ignored / 0 failed，25 个 integration targets 全部通过
- [x] Rust 竞争事实：首次独立 `cargo test --lib` 为 2132 passed / 3 ignored；`cargo test --tests` 聚合曾因既有临时目录竞争使 1 个 `infra::cli_proxy` 用例失败，该用例单独重跑通过，之后 25 个 integration targets 逐个串行通过；未将聚合失败伪装为通过，也未扩大修改范围
- [x] 全前端 `pnpm test:unit`：287 files / 2491 tests passed
- [x] bindings 二次零漂移：`pnpm check:generated-bindings` 第二次运行通过
- [x] `pnpm typecheck`
- [x] `pnpm lint`
- [x] `pnpm format:check`（`.prettierignore` 保留 `.pi`，注释为 Local agent runtime files）
- [x] `pnpm build` / `pnpm tauri:fmt`
- [x] `pnpm tauri:check`
- [x] `pnpm tauri:clippy` (`-D warnings`)
- [x] `pnpm check:precommit:full`（13/13）/ `pnpm check:prepush`（15/15）
- [x] cross-layer spec/template 9 个文件逐字节一致（逐文件长度与 SHA-256 已核对）
- [x] `task.py validate 07-17-final-review-findings-round-6`
- [x] `task.py validate --all`（22 manifests）
- [x] `git diff --check`（仅 CRLF 提示）
- [x] 最终本地 status / task 状态核对：当前子任务与父任务均保持 `in_progress`，无 remote/upstream 操作

Trellis metadata audit：项目/CLI/npm 版本均为 `0.6.7`，`trellis update --dry-run --skip-all` 未写入文件；dry-run 如实报告 6 个现有 Trellis 模板漂移，hash 文件另含运行时 session/`__pycache__` 的动态差异，按用户指令保留 metadata，未覆盖或清理。

## Commit, Archive And Journal Order

1. [ ] 全门禁通过后，先提交一组或按边界拆分的工作 commits：production、tests、frontend、bindings、
       executable specs/templates、当前子任务工件与父事实纠正。不得包含 journal 或 archive move。
2. [ ] PowerShell commit 前动态解析 node/pnpm 目录补入 `PATH`，不硬编码机器路径；不得 amend/push。
3. [ ] 工作 commits 完成且工作树符合预期后，仅执行
       `task.py archive 07-17-final-review-findings-round-6`；确认父状态仍为 `in_progress`，并让 archive
       阶段的 manifests rewrite/validate 与 bookkeeping commit 完成。
4. [ ] archive 后由协调会话处理含既有 journal session 8 Testing 修正的独立 journal commit；本
        会话不改写该 diff，也不调用 `add_session.py` 伪造或补造 session。
5. [ ] 最终核对工作、归档、journal commits 顺序，round-6 archived、父任务 in_progress、工作树
       clean/预期、upstream push URL `DISABLED`；冻结提交后再启动下一次双独立 read-only review。

## Stop And Rollback Rules

- 任一 focused/full gate 失败即停止提交，修复后重跑受影响 focused gate 与全部 full gates；不得
  弱化、跳过测试或用内容过滤降低资源用量。
- 发现锁序反转、autostart 旁路 writer、旧 token 覆盖 winner、import lock 提前释放、读取超过
  limit+1、stats 跳过恶意 entry 或 export 覆盖 sentinel 时回退对应模块设计。
- 不访问 remote，不合并 main，不归档父任务；本实现回合保持子任务 `in_progress`，完成实现与验证
  后停止在交付。

## Follow-up Implementation F9-F15

历史 F1-F8 的 [x] 仅记录前一轮实现；本节必须重新以 production regression 和当前门禁证明。

### Regression-first order

- [x] F9 regression：真实 settings_set_impl barrier 位于 coordinator return/token construction
      窗口，B 走 preferred-port repair，注入 runtime failure，断言 B winner 与 runtime sync 序列。
- [x] F10 regression：真实 apply_skill_fs_import 的 stage-write 与 live-root pre-backup-rename
      failpoint，断言旧 SSOT sentinel 保留、stage/backup 无残留。
- [x] F11 regression：真实 local multi-file writer 在第一份文件后失败，断言新目录全清理、预存
      local directory/bytes 不变。
- [x] F12 regression：Unix config export 枚举后替换 FIFO 的独立 child + external watchdog test 在
      Docker/Linux production run 中通过；Windows cfg 不作为该通过结论的替代证据。
- [x] F13 regression：settings_crud finalize-only restore-success tmp cleanup 与 finalize+restore
      double-failure durable copy state 已加入。
- [x] F14 regression：Unix production storage_stats FIFO/race child watchdog 在 Docker/Linux production
      run 中通过；Windows cfg 不作为该通过结论的替代证据。
- [x] F15 evidence：既有 journal Testing dirty diff 与 task facts 已核对一致；当前 Terra 会话不改写
      journal、不新增 session，也不把历史门禁写成本轮验证。

### Follow-up production fixes

- [x] F9 ordinary committed/previous token 仅来自 A 的 locked durable commit；coordinator 自身
      auto_start correction 不吸收其他 canonical writer。
- [x] F10 guard 显式记录 candidate/stage/backed-up/activated ownership，rollback 只删除本 import
      拥有对象。
- [x] F11 local target 在首次写入前登记 ownership，失败时由 guard 清理。
- [x] F12 Skill relative open 保留 NOFOLLOW/CLOEXEC/identity/single-link 并加入 NONBLOCK；Unix
      external-watchdog production regression 已在 Docker/Linux 实际通过。
- [x] F13 捕获 restore 前 had_backup；finalize/restore 双错误聚合，保留 durable bytes 且只清理
      writer-owned tmp。
- [x] F14 生产实现已具备 NONBLOCK/handle-relative stats；Unix watchdog production regression 已在
      Docker/Linux 实际通过。
- [x] F15 当前 task/父 task 事实修正，既有 journal dirty diff 已保留并核对。

### Follow-up focused validation

- [x] settings service F9：1 passed。
- [x] config migration F10/F11：3 passed。
- [x] settings_crud F13：24 passed，其中 finalize-only restore-success 与 finalize+restore
      double-failure 两个 production regressions 均通过。
- [x] config migration F12 Unix watchdog regression：Docker/Linux production child watchdog 1 passed。
- [x] Image Gen F14 Unix watchdog regression：Docker/Linux production child watchdog 1 passed。

### Docker/Linux Unix watchdog verification (passed)

- 本机 `docker images --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.Size}}'` 列出的 10 个本地
  Linux 镜像均经 `docker run --rm --entrypoint sh <image> -lc 'command -v cargo >/dev/null 2>&1 &&
  rustc --version && cargo --version'` 探测为不含可用 Rust toolchain。
- Docker Desktop/代理恢复后，`docker pull rust:1.90-bookworm` 成功，得到
  `sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f`。
- Unix test module 的首次真实编译暴露 5 个 in-scope Linux 兼容性错误：两个 `libc::mkfifo`
  未声明，以及三个 `SkillFileExport` 缺少 `Debug` 的 `expect_err` 编译错误。最小修复为
  `src-tauri/Cargo.toml` / `Cargo.lock` 增加 Unix-only test `libc`，
  `src-tauri/src/infra/config_migrate/mod.rs` 为 `SkillFileExport` 派生 `Debug`，并把
  `src-tauri/src/domain/image_gen/tests.rs` 的生产 fixture 改为实际创建的 `image-1.png`。
- 修复后以当前工作树 bind mount 实际执行：

  ```powershell
  $repo = (Get-Location).Path
  $script = @'
  set -e
  export PATH=/usr/local/cargo/bin:$PATH
  command -v cargo
  cargo --version
  export DEBIAN_FRONTEND=noninteractive
  apt-get -qq update
  apt-get -o Acquire::Retries=2 install -y --fix-missing --no-install-recommends libasound2-dev librsvg2-dev patchelf libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev pkg-config
  cd /workspace/src-tauri
  CARGO_TARGET_DIR=/tmp/aio-target cargo test -q --locked --lib fifo -- --test-threads=1 --nocapture
  '@
  docker run --rm --mount "type=bind,source=$repo,target=/workspace" rust:1.90-bookworm bash -c $script
  ```

  前置检查输出 `/usr/local/cargo/bin/cargo` 与 `cargo 1.90.0 (840b83a10 2025-07-30)`；测试输出为
  `domain::image_gen::tests::storage_stats_rejects_fifo_replacement_without_hanging ... ok`、
  `infra::config_migrate::tests::config_export_fifo_replacement_fails_closed_under_external_watchdog ... ok`，
  最终 `2 passed; 0 failed`。两个 production watchdog 都通过，未以 Windows cfg、静态审计或其它
  替代测试声明 Unix 证据。

## Follow-up Implementation F16-F23

F1-F15 的 `[x]` 仅保留为历史记录；以下是当前 Round 6 新 finding 的 regression-first 与实现门禁，
完成状态必须由本轮真实命令/测试更新。

### Regression-first order

- [x] F16：真实 settings success-path barrier；B 经 gateway preferred-port/普通 writer 提交后，
      A 的 runtime、返回 `SettingsView` 与下一次 frontend persist 均必须收敛 B。
- [x] F17：真实 failed import 交错 B ordinary/Image Gen/Grok/rectifier/circuit/Codex writers，
      验证 owner-aware field rollback 保留 B 并恢复 A 未被改写 fields。
- [x] F18：真实 whole-CAS A + 专属 writer B + OS sync failure + whole rollback，验证 correction
      不把 B full snapshot 当作 A expected。
- [x] F19：真实 import tail 与 tray writer barrier，验证 tray canonical/runtime/return winner 一致。
- [x] F20：真实 local Skill absence-to-create race 与多文件中途失败，验证外部目录/字节不被删除。
- [x] F21：near-64MiB invalid Base64/path/metadata/budget payload 在 lock-attempt 和 DB write hook
      之前失败，验证无 lock/transaction/stage/FS 残留。
- [x] F22：settings command/integration/frontend finalize-only 与双失败 structured classification。
- [x] F23：已核对 dirty journal/`.pi`/用户文件与任务工件事实；本会话不覆盖、不新增 session，
      不执行 commit/archive。

### Production fixes

- [x] F16：把成功返回、gateway rebind 和 runtime failure 纳入同一 canonical winner finalizer；
      禁止从旧 committed snapshot生成 runtime/response。
- [x] F17：whole import rollback 增加 per-field/per-owner imported token，按字段 CAS，不整表覆盖或放弃。
- [x] F18：拆分 autostart correction effective token 与 A whole committed snapshot，rollback 不吸收 B。
- [x] F19：tray canonical/runtime 共享 owner coordinator，tail write 使用 token/CAS 且最终从 canonical返回。
- [x] F20：以原子 create/create-new + identity 取得 local Skill ownership，rollback 仅清理 owned object。
- [x] F21：新增 typed prepared payload，完整 path/Base64/metadata/budget preparation 全部移出 destructive lock。
- [x] F22：持久化层引入结构化 finalize/restore classification，稳定错误码贯穿 command/integration/frontend。
- [x] F23：已修正工件事实，保留用户 dirty journal 与 `.pi`，明确本会话不
      commit/archive/journal。

### Follow-up Acceptance Evidence

- [x] F16-F19 settings/import/tray：真实 production barriers、canonical/runtime/response 三方一致。
- [x] F20-F21 Skill：absence race、中途写失败、near-64MiB invalid preflight 均 fail closed 且无越权清理。
- [x] F22：finalize-only 普通错误、双失败 `SETTINGS_RECOVERY_REQUIRED` 的结构化分类跨 Rust/前端一致。
- [x] F23：任务事实与 dirty preservation 准确，不覆盖用户内容或新增虚假 session。
- [x] 全部本轮 focused/full/Docker Linux 验证：Docker/Linux 的 F12/F14 production child watchdog
      为 2 passed / 0 failed；Windows cfg 未作为 Unix FIFO 通过证据。

### Follow-up full validation gates

已记录的聚焦证据：F16 generic/gateway barriers 各 1 passed；F17、F18、F19、F20 各 1 passed；
F21 5 passed；F22 两个 Rust production regressions 各 1 passed，前端/跨层 45 passed。

- [x] 在 F16-F23 最终状态上重跑受影响 Rust 与前端：`config_migrate` 65/65、settings 11/11、
      Image Gen 57/57、shared fs 12/12、`settings_crud` 24/24、前端聚焦 4 files / 54 tests；
      `cargo test --manifest-path src-tauri/Cargo.toml --locked -- --test-threads=1` 为 library
      2145 passed / 3 ignored 与 25 integration targets / 125 passed，`pnpm test:unit` 为
      287 files / 2492 tests passed。
- [x] 二次 bindings 零漂移、`pnpm typecheck`、`pnpm lint`、`pnpm format:check`、`pnpm build`、
      `pnpm tauri:fmt`、`pnpm tauri:check`、`pnpm tauri:clippy`（`-D warnings`）均通过。门禁途中
      以真实错误修正 F9 regression 的 canonical-winner 断言、前端/Rust 格式与
      `GatewayConvergenceAction::Start(Box<AppSettings>)` 的 clippy 大枚举布局；随后重跑受影响测试。
- [x] `pnpm check:precommit:full` 13/13、`pnpm check:prepush` 15/15、当前 task validate（8+8）与
      `validate --all`（22 manifests）均通过；三对 cross-layer spec/template 长度和 SHA-256 一致，
      `git diff --check` 仅报告既有 CRLF 归一化提示。未运行或修复 F24 template-hash/safe-commit 专项。

### Explicitly excluded

- F24 Trellis template-hash / safe-commit 修复与验证已由用户移出范围。保持相关现有 dirty 文件原样，
  不执行生成、重算、清理或专项测试。

## Remaining Serial Handoff

1. [x] 当前唯一的 Orca 管理 Codex `gpt-5.6-terra / effort=max` 执行终端已完成 F23 事实与
   dirty preservation 核对，并只把真实状态写回任务工件。
2. [x] 在 Docker/Linux 运行 F12 config export FIFO 与 F14 Image Gen storage stats FIFO watchdog：
   两条 production child watchdog 均在外部 watchdog 下通过（2 passed / 0 failed）。
3. 在最终代码状态重跑受影响测试与完整质量门禁；任何失败先修复并重跑，不进入提交。
4. 门禁全部通过后按既定工作提交、仅归档 round-6、journal 独立提交顺序收尾。
5. 在 round-6 archive 与 journal commit 完成后的冻结提交上，并行新开 Codex
   `gpt-5.6-sol / effort=max` 和 Pi（`grok-cpa / grok-4.5`）两位只读 reviewer；两者彼此隔离，
   协调会话收齐结果后去重、核实证据并形成 round-6 审核汇总。
6. 汇总存在有效 finding 时，按正常修复、验证与提交流程关闭后再冻结新提交；汇总通过后，对父任务
   最终冻结提交以同一对新会话 reviewer 重复该流程。
7. 两次审核汇总均通过后停在合并 `main` 前，向用户请求唯一一次合并确认。
