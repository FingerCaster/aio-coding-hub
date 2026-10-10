# 实施清单

1. 在 `AppSettings` 增加 `codex_managed_model_catalog_enabled: Option<bool>`，schema 升到 66。缺键保持 `None`；新默认值为 `Some(false)`。生成绑定和前端设置类型一起更新。
2. 在目录准备的 Profile 生命周期锁内推导一次 `None`。有受管 Profile、启用规则，或当前绑定已是 AIO 生成文件时持久化为 `true`，否则为 `false`。设置写入失败则不碰目录。
3. 把 `needs_generated_catalog` 收成“开关为 true 且（Profile 非空或有启用规则）”。`false` 走现有空所有者恢复：还原或删除根键，只删除 AIO 拥有的生成文件，保留 Profile 和规则。
4. 关闭时跳过基础目录读取和规则目标校验，避免启动停在 `ReadingSettings`。开启且所有者非空时保留现有失败关闭。
5. `model_catalog_json` 的代理拥有判断改为依赖这个开关。另外三个根字段和 provider 表字段保持原判断。
6. 增加切换命令。它与目录 apply 同一把锁：先写布尔值，再准备并应用目录；apply 失败时按设计回滚设置。升级命令在关闭时返回 `Inactive`。
7. 在 Codex 页模型上下文规则区域加开关。`None` 或写入阻断时不可操作。关闭时隐藏升级按钮。成功提示需要新启动 Codex 会话。
8. 更新 `codex-managed-model-route-contract.md` 的目录生命周期，记下开关、默认推导和关闭后的字段解锁。

## Validation

- `cargo test --manifest-path src-tauri/Cargo.toml --lib settings::`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_model_catalog::managed::`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_config::provider_projection::`
- `pnpm exec vitest run src/components/cli-manager/tabs/__tests__/CodexModelContextRulesSection.test.tsx`

聚焦测试要覆盖：缺键推导为开和关、显式关闭后新增 Profile 不重新绑定、关闭后配置保存可改根键、开启且有所有者时现有投影和升级仍在、apply 失败时开关回到原值。

## Rollback Points

- 推导写入设置之后、目录 apply 之前失败：设置已是推导值，目录未变。不要为了重试把已持久化的值再改回 `None`。
- 用户切换的 apply 失败：回滚本次布尔值。补偿失败时停止并返回恢复错误。
- 不手改用户的 `config.toml` 来撤销一次失败切换。
