# 实施清单：Codex OAuth 代理与 remote compaction 一致性修复

## Ordered Checklist

1. 在 `src-tauri/src/infra/codex_config/provider_projection.rs` 建立结构化 provider key 推导、table inventory、身份收敛、活动投影、冲突预检、递归安全合并和状态检查纯函数；在 `codex_config/tests.rs` 先固定 provider 冲突矩阵。
2. 从 `patching.rs` 移除 line-based `rename_model_provider_table` 的 remote compaction 职责；只有 patch 显式包含 `features_remote_compaction` 时调用统一身份收敛并触发 provider sync，普通 patch 不改 provider。
3. 重构 `cli_proxy/codex.rs` 的 build/status/restore，使 enable、repair、sync、rebind 和 disable 都从 `remote_compaction` 得到同一目标 key，并支持 `OpenAI` 活动投影的完整恢复。
4. 复用 Codex profile lifecycle lock，把 `codex_config_set`、raw TOML 保存和 proxy lifecycle 纳入同一串行化边界；固定 gateway lock -> Codex lock -> 文件/DB transaction 的顺序。
5. 改造 proxy-enabled 配置保存：结构化 patch 作用于 manifest backup 基线后再派生 live；raw 保存进行基线/预期投影/提交 TOML 三方合并，拒绝活动拥有字段编辑。
6. 扩展 backup/live/provider-sync 事务快照和补偿，保证冲突、backup 写、live 写、rollout、SQLite、global state 任一失败时无部分提交；disable 后保留用户字段并清除活动字段。
7. 在 CLI proxy/settings service 增加 OAuth-only Codex 重投影入口，跳过 managed model catalog；把活动 Codex 同步失败提升为 command error，并复用 settings owned-field CAS rollback 恢复旧设置和投影。
8. 删除 `persistCodexOauthCompatibleProxyMode` 对完整 `refreshCodex()` 的同步等待；为 OAuth switch 补齐 pending 可见状态和成功/失败收敛。
9. 在 settings/Codex mutations 后定向失效 config/raw/proxy status 查询；保留 Sidebar 真 drift 的“修复”入口，不添加 UI 特判。
10. 补齐 Rust provider/config/proxy/settings 与 TS query/page/sidebar 回归测试，执行 focused tests 后跑全量质量门禁。

## Planned Files

- `src-tauri/src/infra/codex_config/provider_projection.rs`：新增共享 provider 语义与冲突规则。
- `src-tauri/src/infra/codex_config/mod.rs`：基线选择、锁、结构化/raw 保存事务。
- `src-tauri/src/infra/codex_config/patching.rs`：移除脆弱 provider rename，接入共享 reconcile。
- `src-tauri/src/infra/codex_config/tests.rs`：provider table、格式和幂等矩阵。
- `src-tauri/src/infra/cli_proxy/codex.rs`：provider-aware build/status/restore 与 OAuth-only 投影。
- `src-tauri/src/infra/cli_proxy/mod.rs`：共享同步入口、事务快照与 Codex lifecycle 调用。
- `src-tauri/src/infra/cli_proxy/tests.rs`：enable/repair/sync/rebind/disable 组合回归。
- `src-tauri/src/app/settings_service.rs`：同步结果错误化和 settings rollback 测试。
- `src-tauri/tests/codex_provider_sync.rs`：冲突前零写入和 provider sync rollback。
- `src-tauri/tests/cli_proxy_startup_recovery.rs`：活动期用户字段保存与关闭恢复。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`：OAuth 快速关键路径。
- `src/components/cli-manager/tabs/CodexTab.tsx`：switch pending 表达。
- `src/query/settings.ts`、`src/query/cliManager.ts`：proxy status 定向失效。
- `src/pages/__tests__/CliManagerPage.test.tsx`、`src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx`、`src/query/__tests__/settings.test.tsx`、`src/query/__tests__/cliManager.test.tsx`、`src/ui/__tests__/Sidebar.test.tsx`：前端回归。

文件列表允许实现阶段按现有模块所有权收窄，但不得把 provider 规则复制回多个文件。若不改变 IPC DTO，不生成无关 bindings diff。

## Validation Commands

```powershell
pnpm exec vitest run src/query/__tests__/settings.test.tsx src/query/__tests__/cliManager.test.tsx src/pages/__tests__/CliManagerPage.test.tsx src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx src/ui/__tests__/Sidebar.test.tsx
cd src-tauri
cargo test --lib infra::codex_config
cargo test --lib infra::cli_proxy
cargo test --lib app::settings_service
cargo test --test codex_provider_sync
cargo test --test cli_proxy_startup_recovery
cd ..
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm check:generated-bindings
pnpm tauri:test
python ./.trellis/scripts/task.py validate 07-29-fix-codex-oauth-proxy-remote-compaction
```

在 PowerShell 中分别运行目录切换后的 Rust 命令；上述代码块表达顺序，不要求把它们拼成一个不可诊断的长命令。

## Review Gates

1. Provider 纯函数测试通过后，检查所有 remote compaction 调用点只使用统一 key 推导。
2. Proxy build/status/restore 回归通过后，确认合法 `OpenAI` 状态不会出现“修复”，真实地址/auth drift 仍为 false。
3. 配置所有权事务完成后，人工对照 backup、live、manifest 和 provider-sync snapshots 的 commit/rollback 顺序，特别检查并发 lock order。
4. OAuth-only 路径完成后，以测试 hook 断言 managed catalog 调用次数为零，不使用宽松 wall-clock 断言代替调用链证明。
5. 前端完成后，以 deferred catalog promise 证明 switch mutation 已经 settled，且查询失效不延长 pending。
6. 全量测试前审计错误和日志，禁止输出 TOML、auth、rollout 内容或完整 provider URL。

## Rollback Points

- provider 结构化 helper 是第一可独立回滚点；不通过冲突矩阵时不接入写路径。
- config/proxy 事务是第二回滚点；若三方 raw merge 无法证明所有权，保持 `CODEX_PROXY_OWNED_FIELD_EDIT` fail-closed，不降级为整体覆盖 backup。
- OAuth-only 同步是第三回滚点；完整 gateway/home sync 保持原路径，避免为性能修复改变其 catalog 语义。
- 前端只在后端原子性成立后移除完整刷新，防止 UI 提前宣称成功。
- 不涉及 schema migration；代码回滚不会要求数据库降级。

## Before `task.py start`

- [x] 用户确认已有 `OpenAI` 的等价复用/冲突拒绝规则。
- [x] PRD 无阻塞 open question。
- [x] 代码调研写入 `research/codebase-findings.md`。
- [x] `design.md` 与 `implement.md` 完整。
- [x] `implement.jsonl` 与 `check.jsonl` 加入真实规范/研究条目。
- [x] `task.py validate` 和规划工件检查通过。
- [x] 用户批准进入实现。
