# Upstream merge conflict decision audit

## Method and limits

This is a post-hoc, read-only audit of local Git objects. No fetch or merge was run.

- Merge: `9e5da3461e2db200a488cef17ac85ecd52c0d6e2`
- Fork parent: `4499c71d17e3d51544e57fdebabb1831b9676d37`
- Upstream parent: `419086fb36a4976e30d384add2fec086d99e648c`
- Merge base: `057c06821b5159fda202bce5cfbf1ef3afb410f9`
- Evidence: `git show --remerge-diff`, parent/final blob IDs, final source/spec/tests.

The archived implementation says “31 text conflicts” without defining whether that means files, index
entries, or hunks. Reproducible `remerge-diff` evidence contains **30 files with conflict markers and
47 marker groups**. This table covers all 30 files; it does not invent a 31st item or claim a historical
user decision that is absent from the record.

“保留 fork” means the final file blob equals the fork parent blob. “融合” means it equals neither
parent and therefore contains a reconstructed combination or follow-up resolution. No conflict file's
final blob equals the upstream parent verbatim.

## Per-file decisions

| File | Result | Reconstructed behavior decision | Evidence |
| --- | --- | --- | --- |
| `.gitignore` | 融合 | 保留 fork 忽略项并纳入 upstream 新生成物规则。 | final blob differs from both parents |
| `.release-please-manifest.json` | 保留 fork | 保留 fork 版本线，不采用 upstream 的较低发布版本。 | final = fork blob `4f608466` |
| `CHANGELOG.md` | 融合 | 保留 fork 已发布记录，同时加入 upstream Grok/Image Gen 发行说明。 | final differs from both |
| `package.json` | 融合 | 保留 fork 版本/脚本门禁并带入 upstream 前端依赖与功能脚本。 | final differs from both |
| `scripts/check-plugin-api-contract.mjs` | 保留 fork | 保留 fork 更严格的 plugin API contract 检查。 | final = fork blob `39057326` |
| `scripts/check-pnpm-audit.mjs` | 融合 | 保留 fork audit 门禁语义并兼容 upstream 依赖图变化。 | final differs from both |
| `src-tauri/Cargo.lock` | 融合 | 合并 fork 依赖锁与 upstream Grok/Image Gen Rust 依赖。 | final differs from both |
| `src-tauri/Cargo.toml` | 融合 | 保留 fork crate 配置并加入 upstream 功能所需依赖/target。 | final differs from both |
| `src-tauri/src/app/settings_service.rs` | 融合 | 保留 fork settings 写入契约并加入 upstream Grok/Image Gen 字段。 | final source and settings tests |
| `src-tauri/src/commands/cli_manager.rs` | 融合 | 保留 fork CLI 管理行为并加入 Grok CLI 状态/操作。 | final source and CLI tests |
| `src-tauri/src/domain/provider_availability.rs` | 融合 | 保留 fork provider gate/attempt 语义，同时兼容 upstream provider 扩展。 | final source; failover regressions |
| `src-tauri/src/domain/providers/tests.rs` | 融合 | 保留 fork provider 行为断言并加入 upstream 新字段/类型 fixtures。 | final test blob |
| `src-tauri/src/gateway/proxy/handler/provider_selection.rs` | 保留 fork | 保留 fork request-scoped provider selection/gate 行为。 | final = fork blob `d80d0ac3` |
| `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs` | 保留 fork | 保留 fork selection/gate 回归，不采用会削弱 fork 行为的 upstream 版本。 | final = fork blob `c70dd4f7` |
| `src-tauri/src/infra/settings/defaults.rs` | 融合 | 保留 fork 默认值/版本线并加入 upstream Grok/Image Gen defaults。 | final differs from both |
| `src-tauri/src/infra/settings/migration.rs` | 融合 | 保留 fork migration 链并接入 upstream 新设置迁移。 | final migration tests |
| `src-tauri/src/infra/settings/types.rs` | 融合 | 保留 fork settings schema 并加入 upstream Grok/Image Gen 类型。 | final source/bindings |
| `src-tauri/src/lib.rs` | 融合 | 保留 fork startup/command 注册并加入 upstream 功能模块。 | final registry/build |
| `src-tauri/tauri.conf.json` | 融合 | 保留 FingerCaster updater/pubkey/版本面，加入 upstream Image Gen 所需 CSP/资源配置但不扩 filesystem scope。 | final config and Image Gen spec |
| `src/__tests__/msw-default-settings.test.ts` | 融合 | 保留 fork 默认设置断言并加入 upstream 新字段。 | final test |
| `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx` | 保留 fork | 保留 fork Codex tab 行为与已移除产品面的断言。 | final = fork blob `5f709f1d` |
| `src/pages/cli-manager/useCliManagerPageDataModel.ts` | 融合 | 保留 fork Codex 数据模型并加入 Grok CLI 管理数据。 | final source/tests |
| `src/pages/providers/SortableProviderCard.tsx` | 融合 | 保留 fork 账户展示/刷新行为并兼容 upstream OAuth/Grok 展示。 | final source/provider tests |
| `src/pages/providers/useProviderEditorForm.ts` | 融合 | 保留 fork provider form 字段并加入 upstream OAuth/Grok 字段。 | final source/dialog tests |
| `src/services/__tests__/desktopBridge.contract.test.ts` | 融合 | 保留 fork bridge contract 并加入 upstream Image Gen/CLI command coverage。 | final contract test |
| `src/services/cli/__tests__/cliManager.service.test.ts` | 融合 | 保留 fork CLI adapter 断言并加入 Grok command fixtures。 | final test |
| `src/services/gateway/requestLogs.ts` | 融合 | 保留 fork route/attempt 投影语义并兼容 upstream log shape。 | final source/log tests |
| `src/services/providers/providers.ts` | 融合 | 保留 fork account usage/provider adapter，并加入 upstream OAuth/Grok IPC。 | final source/generated bindings |
| `src/test/msw/handlers.ts` | 融合 | 保留 fork MSW 行为并加入 upstream 新 command handlers。 | final test handler |
| `src/test/msw/state.ts` | 融合 | 保留 fork mock state 并加入 upstream Grok/Image Gen/settings state。 | final mock state |

## Product-semantics conclusion

The final tree demonstrably preserved fork-specific provider gate/selection, plugin checks, Codex tab,
release/updater identity, account-query fixes, Skill bundle fixes, and removed reasoning/continuation
surfaces, while integrating upstream Grok, Image Gen, CLI, usage and OAuth functionality. The audit found
no unresolved conflict that now requires a new product-semantics choice; it only closes missing evidence.
