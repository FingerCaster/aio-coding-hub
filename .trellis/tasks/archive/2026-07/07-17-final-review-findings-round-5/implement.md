# Implementation Plan: 第五轮终审 findings

## Planning And Context Gate

- [x] 将 Max round-5 原始结论完整写入 `prd.md`、`design.md`、本计划与
      `research/final-review-round-5.md`。
- [x] 运行 `task.py validate 07-17-final-review-findings-round-5`，通过后直接 `task.py start`。
- [x] 执行 trellis-before-dev：重读任务工件、packages、guides index、aio-coding-hub backend/
      cross-layer index 与涉及的具体 contracts，再编辑业务代码。

## Implementation

- [x] 盘点 config export 的 SSOT/local 顶层 Skill 入口，建立可信父句柄相对枚举/no-follow
      打开/identity 校验，共用捕获 handle bytes 完成 metadata 解析与文件导出。
- [x] 增加 SSOT/local 顶层 symlink、Windows junction/reparse point（按平台 cfg）与确定性替换
      竞态生产入口测试；保留任意 bytes round-trip 和全部既有预算负例。
- [x] 将 config import autostart 副作用移到 CAS 成功后，令 rollback 返回明确 owner 恢复结果；
      修正 settings service 仅在 owned rollback 成功后恢复旧 runtime，否则按 canonical winner 同步。
- [x] 增加真实 import/service barrier 交错测试，覆盖 `SETTINGS_CONCURRENT_UPDATE`、runtime sync
      failure、rollback failure 与并发 owner winner。
- [x] 调整 failover loop 为 authoritative gate 先于 ready cap；补 `cap=2`、Ready/Ready/circuit
      skipped 的生产 router attempt/route 回归，保留 `DeniedByCircuit`。
- [x] 在 `generatedIpc.ts` stringify 前递归按敏感键脱敏；补随机 capability 的 string/object
      error 与真实 poll/cancel 日志测试。
- [x] 新增真实 Grok router 双请求 continuation 回归，断言单次 retry、第二次移除字段、最终
      usage/response-id、TTFB 与 20 MiB 边界。
- [x] 系统搜索同类别入口并做最小必要修复；不得恢复 FinalSuperset/reasoning-guard 产品面。
- [x] 修正父 `prd.md`/`implement.md` 为 1-9 已归档、round-5 当前修复和下一次 Max 终审前置；
      父 `task.json` 保持 `in_progress`。
- [x] 更新 executable specs，并同步 cross-layer 模板镜像至逐字节一致。

## Focused Validation

- [x] 运行 config migration/export/Skill filesystem 聚焦 Rust 测试（含平台特定入口）。
- [x] 运行 settings service、config import、autostart 与 runtime rollback 聚焦测试。
- [x] 运行 failover production router/attempt-route 聚焦测试。
- [x] 运行 generated IPC/OAuth poll/cancel 前端测试。
- [x] 运行 Grok production router continuation、usage/response-id、TTFB/body-limit 聚焦测试。

## Full Gates

- [x] 完整 Rust library 与 integration tests。
- [x] 完整前端测试。
- [x] bindings 生成两次并验证第二次零漂移。
- [x] typecheck、lint、format、build 与 all-target Clippy。
- [x] `git diff --check`、`task.py validate --all`。
- [x] `pnpm check:precommit:full` 与 `pnpm check:prepush`。
- [x] 若本机没有 Unix target，不联网安装；记录限制并审计 Unix cfg/no-follow API。

## Commit, Archive And Evidence

- [x] 确认 `upstream` push URL 为 `DISABLED`、父状态为 `in_progress`，且未执行 remote 操作。
- [x] 从当前 PowerShell 动态解析 `node`/`pnpm` 所在目录补 `PATH`，提交工作改动。
- [x] 仅归档 `07-17-final-review-findings-round-5`，不得归档父任务或启动 Max 终审；提交归档。
- [x] 用 `add_session.py` 记录 journal（含工作与归档 hash、测试和平台限制），提交 journal。
- [x] 最终核对三个 hash、测试摘要、`git status`、父状态与 `upstream` push URL，输出
      `ROUND5_READY_FOR_MAX_REVIEW`。

## Stop And Rollback Rules

- 任一边界测试或全量门禁失败即修复并重跑，不弱化测试、不越过失败门禁。
- 发现 Skill 内容扫描/过滤、旧 runtime 覆盖 winner、gate skip 被 cap 隐藏或多次 Grok rectifier
  retry 时停止提交并回到相应实现步骤。
- 不访问 remote，不合并 main，不归档父任务，不自行启动下一次 Max 终审。
