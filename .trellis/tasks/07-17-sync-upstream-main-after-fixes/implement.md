# Implementation Plan: 最后同步 upstream/main

## 进入门槛

- [ ] 证明子任务 1、2、3、4 按固定顺序分别验收、提交并归档。
- [ ] 证明当前 HEAD 包含四个独立提交且工作树干净。
- [ ] 只启动子任务 5；不派生子代理。
- [ ] 在以上检查全部通过之前，不读取、检查、fetch 或 merge 项目 remote `upstream`。

## 固定 remote 与同步输入

- [ ] 进入门槛通过后，检查 local remote 配置：`origin` 是常规目标，`upstream` 未启用
      push；不得修改 upstream 以允许推送。
- [ ] 若使用 `gh`，设置/确认默认仓库为 `FingerCaster/aio-coding-hub`，并对仓库操作
      显式传 `-R FingerCaster/aio-coding-hub`。
- [ ] 记录同步前 HEAD。
- [ ] 仅 fetch `upstream/main`，立即解析并记录其不可变 commit SHA。
- [ ] 记录该 SHA 与同步前 HEAD 的 merge-base；之后所有分析和合并只使用固定 SHA。

## Drift 审阅与决策门

- [ ] 分别审阅 merge-base 到 fork HEAD、merge-base 到 upstream SHA 的文件和行为变更。
- [ ] 标记 fork 特有产品面、前四项修复和现有网关契约的重叠区域。
- [ ] 对固定 upstream SHA 执行真实 merge，保留前四项提交历史。
- [ ] 带入全部不冲突变更，不通过挑选文件或 cherry-pick 静默丢失 upstream 内容。
- [ ] 若出现 fork 产品语义冲突，立即停止解析和提交，向主会话提供文件、提交、两侧
      行为、影响与选项；获得用户明确决定后才继续。
- [ ] 即使文本自动合并，也检查是否恢复 reasoning guard/continuation repair，或覆盖
      子任务 1–4 和 gateway contract。

## 验证

- [ ] 重跑子任务 1 的 failover/route/presentation 与完整 Rust 回归。
- [ ] 重跑子任务 2 的 Query 乱序并发和无路由副作用回归。
- [ ] 重跑子任务 3 的 NewAPI fixtures；真实 `muyuan` 仅按其授权与脱敏要求做最小只读
      复核。
- [ ] 重跑子任务 4 的大资源 round trip 与全部文件安全负例。
- [ ] 运行 upstream 变更范围要求的额外测试。
- [ ] `pnpm build`
- [ ] `pnpm check:precommit:full`
- [ ] `pnpm check:prepush`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`
- [ ] `git diff --check`
- [ ] 验证记录的 upstream SHA 是当前 HEAD 的祖先，且前四个子任务提交仍在历史中。
- [ ] 验证 `origin` 默认、`upstream` fetch-only，且没有发生 upstream push。

## 退出门槛

- [ ] 汇总固定 SHA、drift、冲突决策和全部测试证据。
- [ ] 只在全部验证通过后提交 merge 结果，完成 Trellis check/spec judgment 并归档
      子任务 5。
- [ ] 子任务 5 归档前不得开始父任务集成验收。

## 失败与回滚

- [ ] 未提交 merge 验证失败时保留证据并 abort merge，不重写前四项提交。
- [ ] 已提交 merge 需要回滚时整体 revert merge commit，不 reset/rebase 历史。
- [ ] 产品语义冲突未决时保持暂停并等待用户，不自行选边。
