# 实施计划

## 实现

- [x] 在 `src/query/sortModes.ts` 提供规范化的 CLI 级 sort-mode provider 查询前缀，并复用它构造 exact key。
- [x] 在 `useProviderDeleteMutation` 成功路径取消 provider list、Default route 和该 CLI 的 sort-mode provider 在途查询。
- [x] 同步过滤上述已存在缓存并失效相同查询族；保持 `false`/异常路径及其他 CLI/provider 缓存不变。
- [x] 扩充 `src/query/__tests__/providers.test.tsx`，覆盖 Default、多模板、相似 provider、其他 CLI、空缓存和在途查询竞态。
- [x] 在 `ProviderChainView` 统一格式化名称与 ID，并用于摘要、折叠标题和展开详情。
- [x] 扩充 `ProviderChainView` 测试，覆盖同名同 URL、未知名称、无效 ID、损坏 raw JSON、空 URL及折叠态。
- [x] 增加 Rust provider 删除级联测试，覆盖 pool、Default、多模板、邻接 provider 保留和外键一致性。

## 聚焦验证

- [x] `pnpm exec vitest run src/query/__tests__/providers.test.tsx src/query/__tests__/sortModes.test.tsx src/components/__tests__/ProviderChainView.test.tsx src/components/home/__tests__/RequestLogDetailDialog.test.tsx`
- [x] 在 `src-tauri` 运行新增删除测试及相关 `providers_crud` / `sort_modes_crud` 测试。
- [x] `pnpm check:generated-bindings`，预期 `src/generated/bindings.ts` 无差异。

## 完整质量门禁

- [x] `pnpm check:precommit:full`
- [x] `pnpm check:prepush`
- [x] `pnpm audit:deps`
- [x] `pnpm test:e2e`
- [x] `pnpm build`
- [x] 审阅 `git diff --check`、生成文件差异及最终工作树状态。

## 提交与 MSI

- [ ] 按仓库要求为 hook 补齐当前 Node/pnpm 目录到 `PATH`，提交到独立分支；不推送、不合并 `main`。
- [x] 在无 CI/updater 私钥污染的本地环境运行 `pnpm tauri:build:win:x64`。
- [x] 只接受本次构建后生成且数量恰为一个的 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi`。
- [x] 记录 MSI 绝对路径、文件名、字节数和 SHA-256；确认构建未产生非预期受控文件差异。

## MSI 产物

- 绝对路径：`D:\OrcaProjects\aio-coding-hub-fork\fix-provider-delete-log-display\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\msi\AIO Coding Hub_0.60.32_x64_en-US.msi`
- 文件名：`AIO Coding Hub_0.60.32_x64_en-US.msi`
- 大小：`16465920` 字节
- SHA-256：`009F711EF7CD6580F517816399BC7E4A35064A14B15D543E6487A9EAB83BF281`
- 签名状态：`NotSigned`（构建环境未设置 Tauri 签名私钥）

## 风险与停止条件

- Query 缓存清理必须覆盖所有已缓存 mode，不能只处理当前模板或只做视觉过滤。
- 生成绑定校验可能在失败时留下文件差异，必须审阅而非盲目回滚。
- MSI 首次打包可能下载 WiX 工具；若普通 PowerShell 无法发现 MSVC，则使用已安装的 VS 2022 Developer 环境重试。
- 任一测试或打包失败先在本分支修复并重跑对应门禁；不得通过跳过检查、改低阈值或合并 `main` 规避。
