# 同步 upstream/main 更新并处理漂移

## Goal

仅在前四个修复子任务均已验收、提交并归档后，以一个固定的
`upstream/main` 提交为输入完成语义合并：带入全部不冲突更新，保留 fork 产品行为，
并在任何产品语义冲突处暂停交由主会话和用户决策。

## Background

- 本任务是父任务固定顺序中的第 5 项，也是唯一获准访问项目 git remote `upstream`
  的子任务。
- 当前 planning 阶段没有访问、fetch 或检查 `upstream`，因此不对届时 drift、提交数或
  冲突文件作未经证实的描述。
- 常规仓库和 GitHub 操作以 `origin` / `FingerCaster/aio-coding-hub` 为默认目标。
- `upstream` 只作为 fetch 来源，必须保持 fetch-only；本任务不授权向任何 remote 推送。
- 详细门控、冲突分类和回滚流程见 `research/upstream-sync-policy.md`。

## Requirements

### R1. 不可绕过的进入门槛

- 子任务 1、2、3、4 必须依次达到验收标准、形成各自提交并完成 Trellis 归档。
- 开始前验证提交/归档顺序为 `1 -> 2 -> 3 -> 4`，且工作树干净。
- 任一前置任务仍为 planning/active、验收失败、未提交、未归档或存在未归属变更时，
  本任务不得启动，也不得访问 `upstream`。

### R2. Remote 权限边界

- 只有进入门槛通过且本任务已启动后，才允许读取 remote 配置并 fetch
  `upstream/main`。
- 继续将 `origin` 和显式仓库 `FingerCaster/aio-coding-hub` 用于常规 GitHub 操作；
  不依赖含两个 remote 时的隐式 `gh` 仓库解析。
- `upstream` 只允许 fetch；不得恢复或启用 push URL，不得向 upstream 推送。
- 对 origin 的任何 push 也不属于本任务自动授权范围；如后续另有授权，仍必须显式以
  `origin` / `FingerCaster/aio-coding-hub` 为目标。

### R3. 固定同步输入

- fetch 完成后立即解析并记录 `upstream/main` 的不可变 commit SHA、同步前 HEAD 与
  merge-base。
- 后续分析、合并和验收都针对该 SHA，不以可能继续移动的远端分支名作为隐含输入。
- 使用真实 merge 保留前四个子任务提交，不 rebase、压平或重写其历史。
- 合并后必须证明该 upstream SHA 是结果 HEAD 的祖先，确保全部不冲突变更已带入。

### R4. 产品语义冲突决策门

- 除文本冲突外，还必须审阅可干净合并但改变 fork 产品行为的语义冲突。
- 若 upstream 变更与 fork 特有产品行为、功能、安全边界或前四项修复冲突，必须停止
  解析/提交，并向主会话提供：
  - 具体文件和相关提交；
  - fork 当前行为与 upstream 行为；
  - 对用户流程、兼容性和测试的影响；
  - 可行选项及各自代价。
- 未取得用户决定前不得静默采用 ours、theirs、混合方案或删除 fork 功能。
- 普通文本冲突若只涉及格式或可证明等价的重构，可按证据解决，但仍需记录。

### R5. 合并后验收

- 重跑子任务 1–4 的全部聚焦回归，确认 upstream 未覆盖根因修复。
- 运行仓库完整前端/Rust 质量门槛及受 upstream drift 影响的额外测试。
- 检查已由前置任务删除的 reasoning guard、continuation-repair 产品面没有被恢复；
  gateway usage、response-id、TTFB、取消、模型发现和 20 MiB 非 SSE 上限保持契约。
- 本任务验收、提交并归档后，才能开始父任务最终集成验收。

## Acceptance Criteria

- [x] 访问 `upstream` 前已有子任务 1–4 按顺序验收、提交、归档和干净工作树证据。
- [x] 记录同步前 HEAD、merge-base 和唯一的 `upstream/main` 不可变 SHA。
- [x] 结果历史保留前四项提交，且记录的 upstream SHA 是结果 HEAD 的祖先。
- [x] 全部不冲突 upstream 变更均被带入；没有静默丢弃提交或文件。
- [x] 所有 fork 产品语义冲突均先暂停并在用户明确选择后处理。
- [x] `origin` 仍是默认仓库目标，`upstream` 仍为 fetch-only，未向 upstream 推送或
      恢复 push URL。
- [x] 子任务 1–4 聚焦回归、完整 Rust/前端门槛及 upstream 影响测试全部通过。
- [x] 子任务 5 已独立提交并归档，之后才进入父任务集成验收。

## Out of Scope

- 在前四项归档前查看 upstream drift、fetch、merge 或预判冲突。
- 向 upstream 推送、修改 upstream 仓库、创建 upstream PR/issue/release。
- 借同步重写前四项历史或顺带改变 fork 产品策略。
- 在用户未决策时自行选择涉及 fork 行为的冲突侧。
- 在本 planning 终端执行任何 remote 操作或业务代码合并。
