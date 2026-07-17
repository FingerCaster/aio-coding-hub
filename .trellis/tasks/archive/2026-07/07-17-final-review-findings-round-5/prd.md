# 关闭第五轮终审 findings

## Goal

关闭 Max round-5 的三项 P1 与三项 P2，并以生产入口回归证明 Skill 顶层可信根、
settings CAS 副作用、方案 A common gate、OAuth capability 日志、Grok continuation 与父任务
证据均达到下一次独立 Max 终审的前置条件。

## Requirements

### R1. Skill 顶层可信根

- SSOT 与各 CLI local Skill 的顶层目录必须从已打开的可信父根句柄相对枚举、no-follow
  打开，并校验枚举项与已打开对象 identity；不得通过 `is_dir` 后再 `canonicalize` 将路径
  解析结果升级为新权威根。
- `SKILL.md` 与 source metadata 必须从同一次捕获的 Skill 目录句柄打开并读取，解析同一份
  已捕获 file bytes；文件树导出也必须锚定该句柄，不能重新按路径读取。
- SSOT 与 local 生产导出入口都必须拒绝顶层 symlink/junction，并在枚举、打开、解析或遍历
  间发生替换时 fail closed；补确定性替换竞态回归。
- 合法 Skill 根内满足既有数量、路径、单文件和总量边界的任意字节必须逐字节导出并可
  round-trip。严禁敏感词扫描、内容过滤、脱敏、自动剔除或按内容阻断；现有用户提示不变。

### R2. Settings 副作用与 CAS 所有权

- config import 不得在 settings CAS 成功前改变 autostart；副作用必须延后到成功提交后，
  或由可靠、可验证的 rollback token 管理。
- `SETTINGS_CONCURRENT_UPDATE` 必须保留并发 winner 的 canonical settings、autostart 与 runtime，
  不能以 `committed_settings=None` 遗漏恢复，也不能用旧 runtime 覆盖 winner。
- settings service 只有在 owned-fields rollback 明确成功时才能恢复旧 runtime；CAS/owned
  rollback 被并发 owner 击败或自身失败时，应保持现状或从当前 canonical winner 重同步。
- 使用真实 import/service 生产路径的确定性交错测试覆盖 CAS 冲突、runtime sync 失败、
  owned rollback 失败与并发 owner winner。

### R3. 方案 A gate 顺序

- provider authoritative common gate 必须在 ready-provider cap 的消费和上限判断之前执行。
- gate skip 必须留下 skipped attempt/route 并 `continue`；只有 `Ready` 才消费或检查
  `providers_tried` 上限。
- 生产 router 回归固定 `cap=2`，候选顺序为 `Ready, Ready, circuit-open/cooldown`；第三项必须
  仍被 gate 评估并留下 skipped attempt/route。
- 不得提前移除 `DeniedByCircuit`，也不得改变现行完整 503、retry、attempt 与 route 语义。

### R4. OAuth capability sanitizer

- IPC 错误进入字符串格式化前，先对数组/对象按敏感键递归脱敏；至少覆盖 `flowId`、
  `flow_id` 与 capability-like key，且不依赖值中出现特殊 marker。
- 字符串错误仍须安全脱敏；对象/JSON 错误不得因 `String(error)` 或 regex 形状而泄漏。
- 用随机、无特殊标记的 capability 覆盖 string/object 错误和 poll/cancel 真实调用日志。

### R5. Grok continuation 生产回归

- 新增实际 Grok router 双请求测试：首次携带 `previous_response_id`，上游返回对应 400/404；
  内部只重试一次，第二次请求已移除该字段。
- 最终响应的 usage 与 response-id 必须正确，且测试覆盖/断言现行 TTFB 与 20 MiB 非 SSE
  聚合边界没有回归。
- 不恢复已删除的 FinalSuperset、reasoning guard 或 continuation-repair 产品面；仅验证现有
  Grok/Codex 内部 rectifier 的生产路径。

### R6. 父任务证据一致性

- 修正父 `prd.md` 与 `implement.md` 中“仅 1-7 已归档”“round-4 in progress”“1-9 清单未完成”
  等矛盾，真实记录子任务 1-9 已完成归档。
- 将 round-5 记录为当前修复子任务，以及下一次独立 Max 终审的前置；父 `task.json` 全程
  保持 `in_progress`，不得归档父任务或自行启动 Max 终审。

### R7. 范围、镜像与仓库约束

- 系统搜索同类别生产入口，并仅在修复同一信任/所有权契约所必需的范围内修改。
- 更新相关 executable cross-layer specs，且 `.trellis/spec/aio-coding-hub/cross-layer` 与
  `src/templates/markdown/spec/cross-layer` 对应文件逐字节一致。
- 禁止子代理、其他终端和所有 remote/fetch/pull/push/`gh` 操作；`upstream` push URL 保持
  `DISABLED`。

## Acceptance Criteria

- [x] SSOT/local Skill 顶层 symlink、junction 与替换竞态在生产入口 fail closed；合法根内任意
      字节逐字节导出并 round-trip，未新增任何内容扫描或过滤。
- [x] config import/settings service 的确定性交错回归证明 CAS winner 及其 runtime/autostart 不被
      loser rollback 覆盖，且 runtime sync failure 路径满足 owned rollback 契约。
- [x] `cap=2` 的生产 router 回归证明第三个 circuit-open/cooldown 候选仍留下 skipped attempt/route。
- [x] OAuth sanitizer 对随机 capability 的 string/object error 与真实 poll/cancel 日志均不泄漏。
- [x] 实际 Grok router 双请求回归证明仅一次 rectifier retry、第二次无
      `previous_response_id`、最终 usage/response-id 正确，并保持 TTFB/20 MiB 边界。
- [x] 父任务工件准确记录 1-9 已完成、round-5 当前进行中；父状态始终为 `in_progress`。
- [x] 聚焦测试、完整 Rust lib/integration、前端、bindings 二次生成零漂移、typecheck/lint/
      format/build、all-target Clippy、`git diff --check`、`task.py validate --all`、
      `pnpm check:precommit:full` 与 `pnpm check:prepush` 全部通过。
- [x] 无 Unix target 时不联网安装，记录限制并完成 cfg/API 静态审计。
- [x] 工作提交后仅归档 round-5，记录 journal 并提交；最终证据包含三个 hash、clean/预期
      `git status`、父状态与 `upstream` push URL，且未启动 Max 终审。

## Out Of Scope

- 恢复已删除的 FinalSuperset、reasoning guard、外部 retry gateway 或其他产品面。
- 依据 Skill 内容做敏感词判断、脱敏、过滤、自动剔除或阻断。
- 合并 main、归档父任务、访问任何 remote、推送、创建 PR 或启动下一次 Max 终审。
