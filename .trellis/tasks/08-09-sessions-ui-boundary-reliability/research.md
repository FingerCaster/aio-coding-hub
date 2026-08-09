# 研究摘要

- `cliSessions.ts` 使用 page size 50、`maxPages=10`、`fromEnd=true`，只有 next page；候选 `e57acb54` 增加 previous/next 与真实窗口边界。
- `memoryDiagnostics.ts` 每个 query 重置 200k 节点预算并最终排序；候选 `5d4906c5` 使用共享预算、query cap 和有界 top 20。
- `gptImageAdapter.ts:179-208` 下载所有 URL 条目；候选 `9a280136` 在任何下载前限制到 `min(n,10)`。
- `FormField.tsx` 对直接 ReactNode 生成无实际 control id 的 `<label>`；候选 `9e83772c` 引入 control/group 合同。
- `CodeEditor.tsx:29-48` 永久缓存 rejected Promise；候选 `5b13683b` 清缓存并提供错误重试。
- `AppLayout.tsx:47-50` 未为 banner/outlet 建剩余高度 flex wrapper；候选 `d12dbfe3` 修复并有小视口回归。
