# 实施清单：单供应商分享与导入

## Ordered Checklist

1. 在 provider domain 新增严格 v1 DTO、确定性有界序列化、文件名清理、有界解析和脱敏预览类型。
2. 实现单供应商 DB 快照导出，覆盖全部配置/凭据/扩展及插件版本，拒绝 `source_provider_id != null`。
3. 实现事务化导入：复用 provider 验证器、强制禁用、冲突自动命名、OAuth/API Key 完整写入、扩展兼容重验和账户用量拥有者补齐。
4. 新增后端预览状态，落实随机 token、TTL、单次使用、条目/字节上限、文件哈希绑定和显式丢弃。
5. 新增 provider share Tauri commands：后端文件选择/保存、原子写入、剪贴板条件清理、文件/文本预览、确认和丢弃。
6. 注册命令与 managed state，生成并检查 TypeScript IPC bindings。
7. 新增前端 service/query 适配器，确保粘贴内容不进入诊断参数，成功后失效目标 CLI 列表。
8. 新增分享和导入对话框；接入卡片“分享”和页面“导入”，处理引用型禁用、loading、toast、预览失效、输入清理与成功后 CLI 切换。
9. 补齐 Rust、service/query 和 React 测试；审计所有错误、日志和 IPC 返回结构不泄露秘密。
10. 运行格式化、生成绑定检查、focused tests、`pnpm typecheck`、`pnpm lint`、`pnpm tauri:check`，最后执行 Trellis full-scope check。

## Validation Commands

```powershell
pnpm tauri:gen-types
pnpm check:generated-bindings
pnpm exec vitest run src/services/providers/__tests__/providerShare.test.ts src/pages/providers/__tests__/ProviderShareDialog.test.tsx src/pages/providers/__tests__/ProviderImportDialog.test.tsx src/pages/providers/__tests__/SortableProviderCard.test.tsx src/pages/providers/__tests__/ProvidersView.test.tsx
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:test
python ./.trellis/scripts/task.py validate 07-18-export-single-provider
```

若完整 Rust 测试耗时超出当前迭代，至少先运行 provider share 模块测试与 `cargo check --locked`，随后在最终质量门禁补跑 `pnpm tauri:test`。

## Risky Files And Review Gates

- `src-tauri/src/domain/providers/queries.rs`：仅放宽 sibling helper 可见性，不改变普通 CRUD 行为；任何共享重构都要跑原 provider tests。
- `src-tauri/src/domain/providers/share.rs`：凭据与 schema 的唯一所有者；必须逐字段对照数据库和 `ProviderUpsertParams`。
- `src-tauri/src/app/provider_share_service.rs`：敏感内存生命周期与并发边界；不得日志化 Debug DTO。
- `src-tauri/src/commands/providers/share.rs`：文件/剪贴板权限边界；返回值必须脱敏。
- `src/generated/bindings.ts`：只通过生成脚本更新。
- `src/services/providers/providerShare.ts`：`invokeGeneratedIpc.args` 禁止放原始粘贴内容。
- `ProvidersView.tsx` / 对话框：确认内容变化会丢弃预览，关闭路径清空文本。

## Rollback Points

- 完成 Rust format/DB tests 后再接前端，避免 UI 掩盖格式或原子性问题。
- 生成绑定后先检查仅新增预期命令/类型，再继续 service 层。
- 若普通 provider upsert 测试回归，撤回共享 helper 重构并把分享导入保持为独立事务函数。
- 若插件兼容无法在事务内证明，保持精确版本 fail-closed，不降级为忽略或部分导入。

## Before `task.py start`

- [x] PRD convergence pass 完成，无阻塞 open question。
- [x] 代码调研已写入 `research/provider-share-codebase.md`。
- [x] `design.md` 与 `implement.md` 完整。
- [x] `implement.jsonl` 与 `check.jsonl` 已加入真实规范/研究条目。
- [x] `task.py validate` 通过。
