# Implementation Plan: 最后同步 upstream/main

## 进入门槛

- [x] 证明子任务 1、2、3、4 按固定顺序分别验收、提交并归档。
- [x] 证明当前 HEAD 包含四个独立提交且工作树干净。
- [x] 只启动子任务 5；不派生子代理。
- [x] 在以上检查全部通过之前，不读取、检查、fetch 或 merge 项目 remote `upstream`。

## 固定 remote 与同步输入

- [x] 进入门槛通过后，检查 local remote 配置：`origin` 是常规目标，`upstream` 未启用
      push；不得修改 upstream 以允许推送。
- [x] 若使用 `gh`，设置/确认默认仓库为 `FingerCaster/aio-coding-hub`，并对仓库操作
      显式传 `-R FingerCaster/aio-coding-hub`。
- [x] 记录同步前 HEAD。
- [x] 仅 fetch `upstream/main`，立即解析并记录其不可变 commit SHA。
- [x] 记录该 SHA 与同步前 HEAD 的 merge-base；之后所有分析和合并只使用固定 SHA。

## Drift 审阅与决策门

- [x] 分别审阅 merge-base 到 fork HEAD、merge-base 到 upstream SHA 的文件和行为变更。
- [x] 标记 fork 特有产品面、前四项修复和现有网关契约的重叠区域。
- [x] 对固定 upstream SHA 执行真实 merge，保留前四项提交历史。
- [x] 带入全部不冲突变更，不通过挑选文件或 cherry-pick 静默丢失 upstream 内容。
- [x] 若出现 fork 产品语义冲突，立即停止解析和提交，向主会话提供文件、提交、两侧
      行为、影响与选项；获得用户明确决定后才继续。
- [x] 即使文本自动合并，也检查是否恢复 reasoning guard/continuation repair，或覆盖
      子任务 1–4 和 gateway contract。

## 验证

- [x] 重跑子任务 1 的 failover/route/presentation 与完整 Rust 回归。
- [x] 重跑子任务 2 的 Query 乱序并发和无路由副作用回归。
- [x] 重跑子任务 3 的 NewAPI fixtures；真实 `muyuan` 仅按其授权与脱敏要求做最小只读
      复核。
- [x] 重跑子任务 4 的大资源 round trip 与全部文件安全负例。
- [x] 运行 upstream 变更范围要求的额外测试。
- [x] `pnpm build`
- [x] `pnpm check:precommit:full`
- [x] `pnpm check:prepush`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`
- [x] `git diff --check`
- [x] 验证记录的 upstream SHA 是当前 HEAD 的祖先，且前四个子任务提交仍在历史中。
- [x] 验证 `origin` 默认、`upstream` fetch-only，且没有发生 upstream push。

## 退出门槛

- [x] 汇总固定 SHA、drift、冲突决策和全部测试证据。
- [x] 只在全部验证通过后提交 merge 结果，完成 Trellis check/spec judgment 并归档
      子任务 5。
- [x] 子任务 5 归档前不得开始父任务集成验收。

## 失败与回滚

- [x] 未提交 merge 验证失败时保留证据并 abort merge，不重写前四项提交。
- [x] 已提交 merge 需要回滚时整体 revert merge commit，不 reset/rebase 历史。
- [x] 产品语义冲突未决时保持暂停并等待用户，不自行选边。

## 完成证据（2026-07-17）

- 固定输入：pre-merge HEAD `4499c71d17e3d51544e57fdebabb1831b9676d37`，
  upstream SHA `419086fb36a4976e30d384add2fec086d99e648c`，merge-base
  `057c06821b5159fda202bce5cfbf1ef3afb410f9`；merge-base 到 upstream 共 6 个提交。
- 真实 merge commit：`9e5da3461e2db200a488cef17ac85ecd52c0d6e2`；两个 parent 依次为
  pre-merge HEAD 和固定 upstream SHA。固定 upstream SHA、merge-base、四项子任务的实现提交
  `9c4d875c`、`ebb0cfe2`、`82e82e7b`、`86680415` 及对应归档提交
  `3abbcdaa`、`8cd8956e`、`b8eb2555`、`4499c71d` 均为 merge commit 祖先。
- 冲突处理：审阅 71 个同路径 overlap，人工解析 31 个文本冲突；保留 fork provider gate、
  request-scoped attempt budget、Codex config、Query/NewAPI/Skill bundle 契约、版本 `0.60.26`、
  FingerCaster updater/pubkey，并带入固定 upstream SHA 的 Grok、Image Gen、CLI、usage、OAuth、
  audit/plugin/support-matrix 等全部 6 个提交。
- Image Gen 信任边界：下载逐跳 no-redirect、公开地址校验、DNS pinning、重定向和响应体上限；
  保存由 Rust native dialog 授权并写入；历史读取/清理/删除和 asset scope 以 canonical storage
  root 为唯一权限根，DB 路径仅作不可信候选。契约记录于
  `.trellis/spec/aio-coding-hub/cross-layer/image-gen-trust-boundary-contract.md` 并已更新索引。
- 聚焦回归：Image Gen Rust 38/38、frontend 207/207；Grok Rust 81/81、frontend 142/142；
  provider availability 17/17；failover Rust 16/16、frontend 44/44；Query/provider frontend
  103/103；NewAPI 27/27；config migrate 26/26；Codex library 44/44、integration 5/5、
  frontend 41/41。
- 完整门槛：`pnpm check:generated-bindings`、`pnpm check:plugin-api-contract`、
  `pnpm audit:deps`（435 packages，high=0，critical=0）、support-matrix check、
  `pnpm check:precommit:full`（13/13）、`pnpm check:prepush`（15/15）、`pnpm build`
  （3544 modules）、完整 locked Cargo test（library 2040 passed、3 ignored，全部 integration
  suites 通过）、locked all-target clippy `-D warnings`、`git diff --check` 与 cached diff check
  均通过。
- Remote/负向审计：`origin` fetch/push 均为 `https://github.com/FingerCaster/aio-coding-hub.git`；
  `upstream` fetch 为 dyndynjyxa 仓库且 push URL 为 `DISABLED`。未 fetch、push、调用 `gh`、
  修改 remote 或合并到 main；活动产品代码不存在 `DeniedByCircuit`、
  `session_bound_provider_circuit_denied`、`codex_retry_gateway` 或已移除的 continuation repair /
  reasoning guard 产品面。
- 第二轮只读证据补全：`git show --remerge-diff` 可复现 30 个冲突文件、47 个 marker groups，
  与本文件原“31 个文本冲突”口径无法一一对应。逐文件 blob/行为决策表见
  `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2/research/upstream-merge-conflict-decision-audit.md`；
  该表不杜撰历史用户决定，也未重新 fetch/merge。
