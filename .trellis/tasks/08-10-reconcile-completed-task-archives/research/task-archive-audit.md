# Task Archive Audit

## 当前状态

- `08-09-orca-cleanup-release` 已归档于 `.trellis/tasks/archive/2026-08/`，提交为 `1e07c484`。
- `task.py current --source` 在新任务创建前为 none；`task.py list` 显示 10 个旧 active 条目。
- 9 个业务 task 目录为未跟踪文件；`00-join-fingercaster` 为已跟踪 onboarding 任务。

## 已有归档但仍有 active 副本

| 任务 | 正式归档 | 审计结论 |
| --- | --- | --- |
| `07-19-adapt-ai-input-account-usage` | `archive/2026-07` | active 与 archive 仅 task 完成元数据不同 |
| `08-03-circuit-breaker-probe-failback` | `archive/2026-08`, `f91946d9` 属于当前 main | 双边设计存在语义差异；archive 含后续 durable dispatch/all-open recovery 设计，需审阅 active 独有内容 |
| `08-08-account-usage-route-gate` | `archive/2026-08`, `d4775a8d` 属于当前 main | active 含归档后稳态余额阻断循环分析和独有 research 文件，必须合入 |
| `08-08-custom-account-usage-routing` | `archive/2026-08`, `9df96192` 属于当前 main | 语义内容一致，主要是完成勾选与状态差异 |
| `08-08-custom-account-usage-script` | `archive/2026-08`, `ad1b8b2d` 属于当前 main | 语义内容一致，主要是完成勾选与状态差异 |

## 代码完成但归档缺失

| 任务 | 当前 main 证据 | 归档状态 |
| --- | --- | --- |
| `07-16-codex-auto-review-route-neutral` | 当前源码与聚焦测试包含 expected auto-review neutral mapping；原始 `1d5d2ac9` 不在 main，但行为由后续 main 历史保留 | 无当前归档 |
| `08-05-transport-error-retry-backoff` | `12e565c0` 是当前 main 祖先 | 无归档提交 |
| `08-05-codex-zstd-request-body` | `a7e7675c` 是当前 main 祖先 | 分支上有 `49650389` 归档，但不是当前 main 祖先 |
| `08-05-second-group-request-reliability` | 两个子任务实现均为当前 main 祖先 | 父任务未归档 |

## Onboarding

`00-join-fingercaster` 的完成条件是理解 workflow/runtime/spec/tasks 并实际使用 finish/archive。当前开发者已有多轮归档、journal 与 spec 工作，任务已失去继续保持 active 的意义。

## 根因

并行 worktree 的业务提交和部分 archive 提交并非总是一起进入 `main`；同时主 worktree 为保护用户未跟踪内容，保留了旧 active task 副本。Trellis active 列表按目录存在性列出它们，因此已归档任务被同名 active 副本重新显示。

## 差异审阅记录（2026-08-10）

### 同名 active/archive

- `07-19-adapt-ai-input-account-usage`：`prd.md`、`design.md`、`implement.md`、`implement.jsonl`、`check.jsonl` 逐字相同；差异仅为 active `task.json` 的 planning/空完成元数据，正式 archive 保留 completed 状态。
- `08-08-custom-account-usage-routing`：`prd.md`、`design.md` 和语义条目一致；active 仅把 archive 中已完成的 checklist、状态与自引用格式退回 planning/active，未发现新的研究或决策。
- `08-08-custom-account-usage-script`：同上；active checklist 全部未勾选，内容没有 archive 之外的语义资料。
- `08-03-circuit-breaker-probe-failback`：archive（`f91946d9`，当前 `main` 历史）包含后续 durable dispatch 快照、已 dispatch 终态 deadline 持久化、all-open recovery plan 和“首个完整成功停止”的设计；active 版本仍是较早的单 probe/直接 transport 方案，并把 all-open 串行恢复删回旧约束。active 没有独有 research 文件或后续验收证据，旧语义已被 archive 后续设计明确取代，因此不回灌。
- `08-08-account-usage-route-gate`：active 独有 `research/stable-blocked-failback-loop.md` 以及 design/prd/implement 中关于连续请求、fresh Blocked 投影、reservation 不伪造消费、compaction pending 和发送前竞态的结论，已逐段合入正式 archive；archive 的 completed checklist、路径与状态仍为权威。

### 当前 main 落地证据

- `07-16-codex-auto-review-route-neutral`：原始提交 `1d5d2ac9` 不在当前 `main` 的祖先链，但后续主线提交 `f6773c15` 保留了 `src/components/home/requestLogPresentation.ts` 的 `isExpectedCodexAutoReviewModelRoute` 逻辑及 Home/Realtime/Detail 消费者；聚焦测试位于 `src/components/home/__tests__/requestLogPresentation.test.ts`、`HomeRequestLogsPanel.test.tsx` 和 `RequestLogDetailDialog.test.tsx`。
- `08-05-transport-error-retry-backoff`：集成提交 `12e565c0` 在当前 `main` 祖先链；含完整任务记录的分支提交 `51e3550f` 不在祖先链，仅作来源对照。实现覆盖 `attempt_record.rs`、`upstream_retry_policy.rs`、`send_timeout.rs`、`success_event_stream.rs`、`success_non_stream.rs` 与 `upstream_error.rs`，并更新 `gateway-attempt-budget-contract.md`。
- `08-05-codex-zstd-request-body`：集成提交 `a7e7675c` 在当前 `main` 祖先链；分支来源 `f48840a1` 与 archive bookkeeping `49650389` 均不在祖先链，未 cherry-pick。`request_body.rs`、`body_reader.rs`、`http_util.rs`、`routes.rs` 和错误码/状态覆盖实现有界多编码解码及明文规范化，相关 Rust 单测随源码保留。
- `08-05-second-group-request-reliability`：两个子任务的 main 集成提交 `12e565c0`/`a7e7675c` 均已进入当前祖先链；本次按 transport 子任务、zstd 子任务、父任务顺序归档，不改变父子 PRD 或业务代码。

### 保护边界

- 本次不处理并发出现的 `.trellis/tasks/08-10-beta-release-channel/` 及其后续子任务
  `.trellis/tasks/08-10-beta-release-pipeline/`、`.trellis/tasks/08-10-beta-updater-core/`、
  `.trellis/tasks/08-10-beta-update-ui/`，也不触碰 `AGENTS.md`、`.orca/`、HTML、分支、stash 或其他用户任务目录。

## 执行结果

- 已按精确路径移除 5 个 active shadow：`07-19`、`08-03`、`08-08-account-usage-route-gate`、`08-08-custom-account-usage-routing`、`08-08-custom-account-usage-script`；每个 archive 唯一且 `completed`。
- 已按 `08-05-transport-error-retry-backoff` → `08-05-codex-zstd-request-body` → `08-05-second-group-request-reliability` → `07-16-codex-auto-review-route-neutral` → `00-join-fingercaster` 顺序运行 `task.py archive --no-commit`。每步成功改写自引用并执行全仓校验。
- 归档操作结束时（并发 beta 子任务尚未出现），`task.py list` 仅显示并发的
  `08-10-beta-release-channel` 与当前清理任务；10 个旧条目均不再显示。当时全仓校验通过
  136 manifests；`git diff --check` 与 `node scripts/check-spec-links.mjs` 通过。
- 最终受影响任务校验的 warning 计数为：07-19 2 条（account-usage spec 超过 32 KiB）、08-03 2 条（gateway-failover spec 超过 32 KiB）、08-08 gate 4 条（两个大 spec 分别被 implement/check 引用）、07-16 1 条（记录的历史分支已不存在），其余受影响任务为 0。全仓 136 manifests 共 70 条 warning，还包含既有 workflow/research 超限及历史 manifests 引用代码路径；没有 JSON 解析、路径缺失或 validation error。

## 收口快照（2026-08-10）

- `git rev-parse HEAD`：`444b92ac377efdf24d3e45f162f6f43194d6863f`；`git rev-parse origin/main`：`99272e4b2beffc52f483efef2e3985d9867d8051`；本任务未 fetch、merge、commit 或 push。
- `git diff --cached --name-only` 为空；当前 tracked diff 9 个路径，其中 8 个属于本任务，`AGENTS.md` 为既有用户修改。
- `git branch` 共 44 个本地 ref，`git stash list` 共 5 项；本任务没有分支或 stash 操作。
- 受保护文件对象：`git hash-object --no-filters AGENTS.md`=`a26728bca8e531197db7f33f25695b67a0eebea3`，`analysis-codex-retry-gateway-2026-07-07.html`=`4027c96f036ab7349ef830f6a242cae168da5081`；既有 `AGENTS.md` diff 对象为 `c5683faee9fedeafbd0a57d33b8d167c7e4fcf80`。
- 当前并发 beta 工作已扩展为 4 个 active 任务（根任务加
  `08-10-beta-release-pipeline`、`08-10-beta-updater-core`、`08-10-beta-update-ui` 三个子任务），
  加上本清理任务共 5 个；10 个旧条目仍全部不显示。上述 4 个 beta 目录及 `.orca/`、HTML 均未进入本任务候选集合。

## 质量门结果

- 通过：`PYTHONPATH=.trellis/scripts python -m unittest discover -s .trellis/scripts/tests -p test_task_archive_context.py`（3 tests）、`task.py validate --all`（当前 142 manifests；并发 beta 新增 6 个 manifest）、`node scripts/check-spec-links.mjs`、`git diff --check`、`pnpm lint`、`pnpm typecheck`、`pnpm tauri:fmt`。
- `pnpm check:generated-bindings` 未通过，但失败点是 Windows `link.exe` 的既有 `LNK1140` PDB 限制；本次没有生成源码差异，且此前发布质量记录已有相同环境限制。
- 归档单测输出中的 malformed JSON/missing target 错误是故意的负例断言，测试最终结果为 `OK`，不是本仓库 manifest 错误。
- `trellis-check` 发现初版 PowerShell 合同示例仅校验规范化父目录，可能接受
  `placeholder/../<approved-name>`；已补为“预审 allowlist + 单一 basename + 规范化父目录”三重检查，
  并验证 traversal、beta 根任务和 `..` 均被拒绝。

## 并发保护更新（2026-08-10）

归档操作完成后，另一工作会话为 beta 发布新增了三个独立 active 子任务：
`08-10-beta-release-pipeline`、`08-10-beta-updater-core`、`08-10-beta-update-ui`。当前
`task.py list` 的 5 个条目为 beta 根任务、这三个子任务和本清理任务；当前
`task.py validate --all` 为 142 manifests。三个子任务及其根目录均只读保留，未被本任务比较、移动、
删除、暂存或提交；136 manifests / 2 active tasks 是归档操作完成时的历史快照。
