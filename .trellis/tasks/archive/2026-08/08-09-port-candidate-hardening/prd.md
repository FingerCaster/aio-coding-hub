# 迁移候选仓库安全与可靠性修复

## Goal

基于 `KNaiFen/aio-coding-hub` 已验证实现，在当前 fork 的路由、账户用量、模型路由和发布契约上适配仍缺失的安全、数据一致性与可靠性修复，并通过独立 Orca worktree、多终端并发开发后集成到一个可验证分支。

## Background

- 集成基线为 `origin/main@a8c525cdaadce77dd4b00363962e501bc5fae491`（`0.60.39`）。
- 当前主工作区存在用户所有的未提交修改；实施必须在 Orca 管理的独立 worktree 中完成，不得改写或提交主工作区内容。
- 候选仓库最新审计实现与 fork 已存在大量架构分歧。所有改动按行为边界适配，不整体合并候选 `main`，也不默认直接 cherry-pick。
- 用户明确要求本批不实现非回环网关 Bearer 鉴权。

## Requirements

- `R1`：Provider 全局禁用必须压过默认路由、自定义排序方案、Session 复用和重试；已开始的请求在每次真正发送前重新检查 Provider 状态。保留当前账户用量门控、熔断、探测回切、模型路由和 Provider 专用路由行为。
- `R2`：在 Provider 发送前拒绝直接地址、解析后地址或别名指向当前 AIO 网关的自环目标；保留现有系统代理自环保护和请求递归标记。
- `R3`：普通设置写入使用共享串行边界，并仅基于调用方实际变更字段构造持久化输入，防止不相交并发修改互相覆盖。
- `R4`：模型价格别名编辑读取失败必须显式失败并阻止保存，不能把读取错误投影为空配置；前后端 schema 版本保持一致。
- `R5`：数据重置使用跨重启 durable maintenance marker；异常退出后必须在数据库、日志和后台任务启动前完成或明确阻断恢复，同时保留当前网关生命周期锁和 DB reset guard。
- `R6`：升级受影响 Rust 依赖并删除 `RUSTSEC-2026-0194`、`RUSTSEC-2026-0195` 审计豁免，CI 恢复普通 fail-closed `cargo audit`。
- `R7`：长会话消息窗口支持向前和向后恢复被 `maxPages` 淘汰的页面，并准确展示已加载窗口边界。
- `R8`：发布流程对同一发布目标使用统一并发键；更新器签名私钥不进入 job 级环境；正式发布只能晋升与目标 SHA 精确匹配的不可变候选制品。保留现有 release tag 解析与 immutable checkout SHA 契约。
- `R9`：移植低风险前端边界修复：内存诊断共享预算、图片响应下载数量上限、FormField control/group 标签合同、CodeEditor 动态导入失败恢复、启动横幅后的剩余高度布局。
- `R10`：插件运行时硬化已由用户确认纳入：移植本地包 preview/install 内容绑定、已记录版本不可变、Hook 上下文预算与字段合同、Header fail-open 语义、完整调用 deadline、主动 idle recycle、配置/Storage 原子性和 fail-closed 日志持久化；不得恢复用户正在删除的 SDK/脚手架文件。
- `R11`：每个交付项必须包含候选实现对照、当前 fork 语义适配说明、聚焦回归测试及与风险相称的完整检查。
- `R12`：各实现子任务在独立 Orca worktree 中提交；集成 worktree 只接收已验证提交，并执行跨子任务冲突审查和联合质量门。

## Out Of Scope

- 非回环/LAN 网关 Bearer 鉴权、Token 展示/轮换及 WSL Token 同步。
- 整体合并或 rebase `KNaiFen/main`，以及整体 cherry-pick `ab76a307`。
- Observer、TUI、托盘新产品面、Responses continuity cache、对方账户用量运行时。
- Durable Usage Ledger、每日用量汇总、完整 filesystem recovery journal、Provider Sync 整体重构。
- cloud-only CI、ARM-only Homebrew 假设和整份候选依赖锁文件替换。

## Acceptance Criteria

- [ ] `R1-R10` 中最终确认纳入的行为均有独立子任务、实现提交和聚焦测试证据。
- [ ] 鉴权与其他明确排除项没有进入产品代码或配置差异。
- [ ] Provider 路由改动不回归账户用量门控、熔断探测回切、可配置模型路由、Session 绑定和 Provider 专用路由。
- [ ] 设置、价格别名和数据重置均有失败优先及并发/恢复测试。
- [ ] Release workflow 通过静态合同测试，保留现有 tag/SHA 安全约束。
- [ ] 插件项必须通过安装完整性、运行时失败策略、资源上限和字段兼容测试；不得无意恢复主工作区删除的包文件。
- [ ] 集成分支通过 `git diff --check`、聚焦测试、类型检查、Lint、generated bindings 检查、Rust format/Clippy/test/audit 中所有受影响且本机可执行的质量门；平台限定检查需明确记录。
- [ ] 最终差异仅包含本任务及 Trellis 交付文件，不包含当前主工作区的既有未提交修改。
