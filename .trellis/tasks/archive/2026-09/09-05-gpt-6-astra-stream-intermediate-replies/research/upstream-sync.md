# 上游同步评估

## 快照与基线

- 上游副本：`E:\MyWork\aio-coding-hub-upstream`
- 上游 `main`：`420e9958`（2026-09-25，`revert(plugins): withdraw Codex model consistency feature`）
- 当前 fork `HEAD`：`28d17ddb`（2026-09-25，`feat(codex): select the managed provider by name`）
- 共同祖先：`4f02ba3d`（2026-07-27，`aio-coding-hub 0.60.16`）
- 上游共同祖先之后有 30 个提交；当前 fork 同期有 518 个提交。
- 当前 fork 版本为 `0.60.41`、数据库 schema 为 46；上游快照版本为 `0.60.19`、schema 为 39。

从 `HEAD` 直接模拟合并 `upstream/main` 会产生 123 个冲突文件，不能作为一次性同步方案。

## 可直接移植

以下提交在独立临时 worktree 中对当前 `HEAD` 执行 `git cherry-pick --no-commit` 时无冲突，已按顺序选择性应用到当前工作区：

| 提交 | 内容 | 影响 |
| --- | --- | --- |
| `cda19b25` | 请求日志卡片在未报告 cache write 时仍显示 0，并修正 TTL 展示测试 | 前端局部 UI，低风险 |
| `3b19a24b` | Windows 资产预览 CSP 增加 `http://asset.localhost` / `https://asset.localhost`，附契约测试 | Tauri 配置与测试，低风险 |
| `f273d301` | macOS 通知音改用 `/usr/bin/afplay` 子进程，rodio 限制到非 macOS | 平台依赖与新模块，需 macOS 构建验证 |

## 适合单独审查后手工移植

- `def1060c`：Claude 代理重启时刷新 direct-config 备份。只在 `cli_proxy/mod.rs` 与 fork 冲突；`claude.rs` 和回归测试可作为移植参考。
- `3758d8e8`：依赖审计在 npm bulk 端点失败时回退 OSV，并保持 fail-closed。当前 fork 的审计脚本已发生本地改动，建议移植函数和测试，不直接覆盖脚本。
- `ab83c23f`：费用聚合改用 SQLite `TOTAL()` / `f64`，防止 femto 成本超过 `i64` 溢出。当前 fork 的 `request_logs.rs`、provider limit 和 usage stats 已重写，应先统一数值契约再移植。
- `85253db0`：请求日志费用刷新修复；与 fork 的 cost/log 查询重构相撞，需要验证现有刷新语义后再取 patch。
- `3bd9bb59`：OAuth 登录、刷新和额度查询统一使用上游代理；与 fork 的 OAuth/provider projection 改动相撞，属于独立兼容性任务。
- `9234280f`、`37319565`：供应商调用顺序默认显示/卡片定位。当前 provider UI 已重构，功能可取但需要按现有组件重做。
- `b3343335`、`b34fe58a`、`867a0db3`：供应商探测模型/提示词选择。fork 已有 provider 级模型覆盖，但没有直接沿用上游对话框，建议作为产品功能单独确认。

## 不建议直接融合

- `9e2d84c8`、`bcb63382`、`48563377`、`537dd7a8`：上游统一 model policy/discovery/routing 系列。fork 使用自己的 managed Codex catalog、provider routing 和 schema 迁移，文件和数据模型均不兼容。
- `a09cbb05`、`d0f85364`、`8bf194e0`：上游 Codex catalog 事件、残留竞态和动态 OAuth 模型发现。fork 已有更晚的 managed catalog/ownership/recovery 实现；需要按 fork 契约抽取行为，不能按提交顺序套用。
- `6007d7a0`：思考等级和价格别名的大型跨层改动。fork 已有 reasoning capability、managed catalog 和价格逻辑，直接移植会覆盖本地决策。
- `e2d03792`：CCH v0.9.2 整流器对齐，涉及 54 个文件并与 stream firewall、failover loop、settings schema 冲突。它可能提供独立的响应整流器参考，但不是 Astra phased-message 的直接修复，需单独设计和回归。
- `b9e6c890`：上游撤销 v39->v40 数据库迁移；fork 已在 schema 46，不能移植。
- `eee73cce`：上游把 `js-yaml` 升到 4.3.1、`nanoid` 升到 3.3.17；fork 已分别在 4.3.2、3.3.18，已被本地依赖修复覆盖。
- `7725effd`、`0a4a0c89`、`87fa66d0`：上游 release/changelog 元数据，不能覆盖 fork 的 0.60.41 发布线。
- `82e3825e` 与 `420e9958`：插件 response-validation/provider-failover 功能随后完整回滚；从 `87fa66d0` 到 `420e9958` 的树差异为零，应忽略这对提交。
- `9bce1dbf`：仅 README Star History 链接，按 fork README 现状手工决定即可。

## 与 Astra 流问题的关系

上游没有提交直接放宽或修复 fork 当前的 Codex phased-message strict validator。`e2d03792` 主要是通用整流器和 fake-200/输入输出归一化，不能替代对 `phase: commentary` / `final_answer` 的协议契约和 fixture 设计。Astra 修复仍应沿当前任务已记录的 strict/live/CX2CC 边界单独验证。

## 本次移植验证

- `pnpm exec vitest run src/components/home/__tests__/HomeRequestLogsPanel.test.tsx src/services/desktop/__tests__/assetUrl.test.ts`：2 个文件、29 个测试通过。
- `pnpm typecheck`、`pnpm lint`、`pnpm check:spec-links`、`pnpm check:generated-bindings`、`pnpm tauri:fmt`：通过。
- `cargo check --locked`、`cargo test --manifest-path src-tauri/Cargo.toml --lib notification_sound`：Windows host 通过；后者在 Windows cfg 下运行 1 个通知音频测试。
- `git diff --check`：通过；新增文件内容与对应 upstream 提交一致，没有额外上游改动。
- 当前 Rust toolchain 只安装 `x86_64-pc-windows-msvc`，未安装 `x86_64-apple-darwin`，因此 macOS `afplay` cfg 分支尚未实际编译或运行；这不是 Windows 测试覆盖范围内的结论。

## 结论

不建议合并整个 `upstream/main`。首批候选 `cda19b25`、`3b19a24b`、`f273d301` 已完成选择性移植并通过当前环境可执行的检查；第二批可按需求分别建立 `def1060c`、`3758d8e8`、`ab83c23f` 的小任务并手工适配。未选择的冲突提交仍保持不变。
