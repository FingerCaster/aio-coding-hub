# 修正第八轮最终审核归档事实

## Goal

关闭冻结提交 `29133ac0d71fe7d41d7ffd481570507df7f9a416` 的唯一有效 P2：已归档的
upstream 同步任务把“所有 fork 产品语义冲突均先暂停并在用户明确选择后处理”标记为
已完成，但可复现的只读审计只能证明最终合并结果，不能证明历史上曾取得用户决定。

本任务只恢复可证实的任务事实，并把父任务对 Round 7 与 Round 8 的状态投影改为真实、
可审计的顺序；不重做或改写既有 upstream merge、产品行为或运行时代码。

## Confirmed Facts

- Round 7 的实现与归档提交分别为 `8bbc619a` 与 `29133ac0`；其在
  `07-17-sync-upstream-main-after-fixes` 的 PRD 第 72 行和 implement 第 26-27 行把
  “取得用户明确决定”标记为完成。
- `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2/research/`
  `upstream-merge-conflict-decision-audit.md:15-16` 明确说明其不会捏造记录中不存在的
  历史用户决定。该审计只证明最终 blob/行为取舍，不能倒推出当时的暂停或用户选择。
- 真实 merge `9e5da346` 及固定 upstream SHA 的祖先关系已经成立，且不是本任务的修复对象。
- 父任务的当前任务地图仍把已归档的 Round 7 写作 `planning`；这不是运行时缺陷，但在
  创建下一顺序子任务时必须更正为反映已归档状态和本轮 P2 的真实投影。
- 用户已授权持续自动执行；本轮实现、检查、提交、归档和最终审核只能使用单个新开的
  Orca-managed Codex `gpt-5.6-sol / effort=max` 串行会话。最终审核必须是另一个新的只读
  Sol max 会话；不得启动 Claude、Pi 或其他审核 agent。

## Requirements

### R1. 只修正无证据的历史用户决定断言

- 在已归档 upstream 同步任务中，仅将直接声称已获得用户明确决定的两个对应记录恢复为
  未完成：PRD Acceptance Criteria 第 72 行，以及 implement 决策门第 26-27 行。
- 每个未勾选项必须保留简洁的证据说明，引用上述只读审计，明确最终 merge/行为证据仍被
  保留，但历史用户决定未在记录中得到证明。
- 不修改其他已证实的验收项、任务日期、提交 SHA、archive `task.json` 状态、merge commit
  或任何产品源码。不得将“没有未解决冲突”误写成“已获得历史用户决定”。

### R2. 维护父任务的顺序事实投影

- 父任务必须把 Round 7 记录为已归档但终审发现本 P2，并新增 Round 8 为当前待完成的顺序
  子任务；不得将 Round 8 或父任务提前标记为已验收/完成。
- 父任务的执行计划必须区分“Round 7 实现及归档已完成”与“最终 Sol gate 仍等待 Round 8
  修正和新的只读审核”。
- 只修改与 Round 7/8 状态映射直接相关的父任务 Markdown 与 task child link；不重写
  更早子任务的历史叙述或验收事实。

### R3. 验证与边界

- 运行 `python .trellis/scripts/task.py validate --all`、`git diff --check`，并验证本轮产品
  源码没有变化。
- 完成提交和 archive 后，由新的 Sol max 只读审核员复核本轮差异和归档事实；只报告可证实的
  P0-P2。审核通过后，协调会话才可进行父任务最终收口；不得自行合并 `main`。
- 任何无法由现有任务工件、Git 对象或当前会话决定证实的历史事件必须保留为未证明，不能
  通过猜测、补写用户决定或扩大调查来关闭。

## Acceptance Criteria

- [x] 已归档 upstream 同步任务中关于“用户明确选择”的 PRD 与 implement 记录均不再被错误
      标为完成，并有准确的只读审计说明。
- [x] 真实 merge、固定 upstream SHA 祖先关系、已证实的复归/门禁记录和产品源码保持不变。
- [x] 父任务任务地图和执行 checklist 真实反映：Round 7 已归档且产生本 P2，Round 8 为唯一
      后续子任务，父任务仍未完成。
- [x] `task.py validate --all` 和 `git diff --check` 通过，且本轮受控 diff 只包含 Trellis
      任务/研究工件。
- [ ] 完成提交与 archive 后，新的 Sol max 只读审核没有 P0-P2；随后才进行父任务最终收口，
      且不自行合并 `main`。

## Out of Scope

- fetch、merge、rebase、revert、push、修改 remote，或修复任何 upstream 自身问题。
- 修改既有 merge 决策、产品功能、Rust/TypeScript 源码、测试、构建配置或运行时行为。
- 捏造、补写或推断不存在的用户决定，或改写历史提交、日期、SHA、task.json 完成时间。
- 修改用户既有 dirty 内容：`.trellis/.template-hashes.json`、`.trellis/.version`、`.pi/`、
  `.trellis/scripts/tests/test_template_hashes.py`。
- 启动 Claude、Pi 或其他 agent 作为实现或审核者，或在未获用户确认时合并 `main`。
