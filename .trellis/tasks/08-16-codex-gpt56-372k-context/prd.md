# 为 Codex GPT-5.6 增加 372K 上下文开关并发布 Beta

## Goal

在 AIO Coding Hub 的 Codex 配置界面提供一个默认关闭的显式开关，让用户可将 Codex 0.147.0 中三个 GPT-5.6 模型的名义上下文窗口从 bundled catalog 的 272,000 token 提升到 372,000 token，并能完整恢复原始 catalog 行为；完成实现与质量门后，将合入 `origin/main` 的不可变提交发布为新的公开 Beta。

## Background

- OpenAI Codex 0.147.0 对应 tag `rust-v0.147.0`、commit `be6e8eac029b183056b7e4402879f15d2c85f61b`，权威 bundled 源文件是 `codex-rs/models-manager/models.json`。
- 原文件中的 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 均同时声明 `context_window = 272000` 和 `max_context_window = 272000`。
- 本任务跟随 Codex 上游目录的十进制标注口径：原始 272K 写作 `272000`，因此 372K 的唯一目标值是 `372000`。`380928` 不得被识别为开启状态。
- Codex 对解析后的上下文继续应用其 95% 有效窗口安全系数，所以名义值 `372000` 对应有效输入预算 `353400`。本任务保留该上游语义，不暗中改成 100%。
- 配置 `model_catalog_json` 后 Codex 在新进程启动时加载完整静态目录。当前 AIO 已有完整目录派生、未知字段保真、ownership metadata、原子写入和回滚机制，应扩展该机制而非修改 Codex 安装文件。

## Requirements

- R1. Codex 设置页新增一个符合现有设计系统的 Switch，标签明确为 GPT-5.6 372K 上下文；显示的精确 token 值为 372,000。
- R2. 功能使用单一常量语义 `372000`，与上游 `272000` 目录值采用相同十进制口径。Rust 目录生成、状态契约、前端显示和测试不得使用 `380928` 代替。
- R3. 开关默认关闭，并使用专属持久化状态。普通 settings 全量写入和通用 Codex TOML patch 不拥有该字段；切换必须通过专属事务入口执行。
- R4. 开启时从当前 Codex bundled catalog 或用户原始完整 catalog 派生目录，只对三个精确 slug 的 `context_window` 与 `max_context_window` 同时写入 `372000`。所有非目标模型、未知字段、顶层字段和 `aio/*` managed entries 必须保持原语义。
- R5. 关闭时，若没有 managed profile 继续需要派生目录，恢复用户原始 `model_catalog_json` 或 bundled 默认并移除 AIO 生成文件；若 managed profile 仍存在，只撤销 GPT-5.6 改写并保留派生目录。
- R6. 目录生成原因是 `has_managed_profiles || gpt56_372k_enabled`。代理关闭或 managed profile 数量为零时，开启的 372K 功能仍必须有效；代理启停、应用启动和 Codex CLI 更新必须重对账。
- R7. owner metadata/hash 必须覆盖 372K 策略、原始 catalog 路径/来源和 managed profile 集合。用户自定义 catalog 未完整包含三个目标 slug 时，开启操作失败且持久开关不得宣称成功。
- R8. 目录、live `config.toml`、代理 baseline/backup 和专属开关持久化必须具备有条件回滚。检测到外部漂移时失败关闭，不覆盖用户并发修改。
- R9. 不写 `model_context_window` 或 `model_auto_compact_token_limit` 来冒充此功能；已有手工值、注释、表和其他 Codex 配置必须保持。
- R10. 开关改变只保证新启动的 Codex 进程/会话加载新目录，界面以简短状态提示表达这一点。已运行进程不被伪装成热更新。
- R11. 仅支持三个已验证 canonical slug，不使用 `starts_with("gpt-5.6")` 推断未来型号，也不猜测裸 `gpt-5.6` alias。
- R12. 开关开启时禁止切换 Codex home，并要求先关闭开关完成目录恢复，避免跨 home 遗留绑定或恢复错误。
- R13. 增加目录生成/恢复、设置迁移/所有权/回滚、启动同步、前端交互及边界条件测试，并通过项目质量门。
- R14. 在独立 worktree 构建 Windows x64 MSI；通过质量门后提交功能分支、创建 PR、等待 required checks 并合入 `origin/main`。不操作 `upstream`。
- R15. 以合入后的 40 位 `origin/main` SHA 发布严格高于当前高水位 `0.60.41-beta.8` 的新 Beta。发布前重新确认 tag/Release 不存在；候选版本为 `0.60.41-beta.9`。
- R16. Beta 必须由 `release.yml` 的 `release_channel=beta` 路径构建并公开为 `draft=false`、`prerelease=true`、`make_latest=false`，包含精确 14 项官方资产；`latest-beta.json` 与 `beta-channel-state.json` 必须通过 CAS 指向该 Release，稳定版 latest/Homebrew 不得改变。

## Acceptance Criteria

- [ ] 默认状态不绑定仅为 372K 而生成的 catalog，三个目标模型继续使用原始 272000 值。
- [ ] 开启后，生成目录中三个精确 slug 的 `context_window` 和 `max_context_window` 均为 `372000`，且新 Codex 进程实际加载该目录。
- [ ] `380928` 在实现和状态判断中仅可作为负向测试值，绝不等同于本功能的 372K。
- [ ] 关闭后恢复 bundled 或用户原始 catalog；存在 managed profiles 时只保留其原有 `aio/*` 投影。
- [ ] 普通模型、未来 `gpt-5.6-*` 字符串、`aio/*` 能力、手工上下文和自动压缩配置不被改写。
- [ ] 用户 catalog 缺少任一目标 slug、catalog/config 漂移、写入失败或持久化失败时，操作返回错误并恢复所有已提交阶段。
- [ ] 代理开关、零/多 managed profiles、应用启动、CLI fingerprint 变化和关闭恢复均有回归覆盖。
- [ ] Codex home 切换在 372K 开启时被阻止，关闭并恢复后可正常切换。
- [ ] UI 在保存中禁用重复操作，失败后回到后端确认态，并提示仅新会话生效。
- [ ] 定向 Rust/前端测试、typecheck、lint、Rust fmt/check、generated bindings 检查和 Windows x64 MSI 构建通过。
- [ ] 最终记录 MSI 绝对路径、字节大小和 SHA-256。
- [ ] 功能 PR 已合入 `origin/main`，发布 source SHA 与合入 commit 完全一致且在发布全程保持不可变。
- [ ] `aio-coding-hub-v0.60.41-beta.9`（或发布前重新计算出的更高空闲 Beta）公开成功，14 项资产、签名、manifest、Release flags 和 `release-channels` 指针全部通过独立复核。

## Out Of Scope

- 修改、替换或重新发布 OpenAI Codex CLI 自身的 bundled 文件。
- 将 95% 有效窗口安全系数改为 100%，或把实际输入预算强行调整到 372000。
- 修改 provider model capability、`aio/*` 上下文、reasoning effort、价格或请求路由。
- 为未知或未来 GPT-5.6 型号做前缀匹配，或猜测没有权威映射的 alias。
- 发布稳定版、修改 GitHub latest、同步 Homebrew，或操作 `upstream`。

## Notes

- 用户已明确授权自行建任务、并行调研、直接实现、提交/合入 `origin` 并发布新的 Beta，无需再次请求规划、实施或发布确认。
- 仓库操作默认 `origin`；实现位于独立 worktree `D:\\OrcaProjects\\aio-coding-hub-fork\\codex-gpt56-372k-context`。
