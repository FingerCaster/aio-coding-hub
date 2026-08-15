# 实施清单

1. 读取并对照 `926279e6` 的相关 patch、当前 main 组件和前端规范，记录差异边界。
2. 拆出 `CodexStreamInternalErrorFields`，从 `RetryPolicyFields` 移除 Codex 专属 UI，
   保持通用字段更新时的隐藏策略。
3. 把共享 retry policy/guard 接入当前 `CodexTab` controller，增加独立设置卡、校验和
   保存；从 `GeneralTab` 移除流专属 props/控件但保留完整保存 payload。
4. 调整 Provider override，仅 Codex 渲染流专属组件，并补充相关回归测试。
5. 更新 upstream-error-handling contract；检查生成绑定/spec links 不需变更。
6. 运行聚焦 Vitest、全量前端测试、typecheck、lint、format check、build、generated
   bindings check 和 spec-link check；修复发现的问题。
7. 检查 diff/dirty paths，提交本端口改动，更新 Orca comment，记录 SHA 和验证结果。

## 预期文件

- `src/components/gateway/RetryPolicyFields.tsx` 及测试
- `src/components/gateway/CodexStreamInternalErrorFields.tsx` 及测试
- `src/components/cli-manager/tabs/{GeneralTab,CodexTab}.tsx` 及测试
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`
- `src/pages/providers/ProviderEditorDialog.tsx` 及测试
- `src/pages/__tests__/CliManagerPage.test.tsx`
- `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md`
