# 新增关闭自动回切策略 - 实施计划

1. 扩展 Rust `ProviderFailbackStrategy` 为 `disabled | aggressive | natural`，保持默认与未知值回退到 `natural`；补充设置序列化/持久化覆盖。
2. 调整 `plan_probe_with_account_usage` 的决策顺序：保留无绑定和 `route_changed`，随后为 `Disabled` 无观察早返回；补齐关闭、路由变化、无绑定/all-open focused tests。
3. 运行 `pnpm tauri:gen-types` 并格式化生成的 `src/generated/bindings.ts`，确认只出现预期枚举扩展。
4. 在 `CliManagerGeneralTab` 增加“关闭自动回切”单选项，修正三值 `onChange`，并补充持久化、选中状态、自然输入可见性与文案测试。
5. 运行 focused checks：
   - `cd src-tauri; cargo test --locked provider_selection::probe_planner`
   - `pnpm exec vitest run src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx`
   - `pnpm check:generated-bindings`
6. 运行全范围质量门禁：
   - `pnpm typecheck`
   - `pnpm lint`
   - `pnpm exec prettier --check <changed frontend/task files>`
   - `cd src-tauri; cargo fmt -- --check`
   - `cd src-tauri; cargo clippy --all-targets --locked -- -D warnings`
   - `pnpm tauri:test`
   - `pnpm test:unit`
7. 根据最终实现更新 gateway failover route spec，执行 Trellis check，修复全部发现后提交到 `FingerCaster/provider-failback-off`。

## Risky Files / Rollback Points

- `src-tauri/src/gateway/proxy/handler/provider_selection/probe_planner.rs`：早返回位置决定是否误伤 route change、新会话和全 Open 恢复。
- `src-tauri/src/infra/settings/types.rs` 与 `src/generated/bindings.ts`：必须同步，默认 `natural` 不得变化。
- `src/components/cli-manager/tabs/GeneralTab.tsx`：三值处理不能将 `disabled` 静默压回 `natural`。

若实现导致共享 failover 回归，优先回退 planner 的 `Disabled` 分支与 UI 枚举扩展；不需要数据库 downgrade。
