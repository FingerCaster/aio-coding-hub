# Current main 落地证据

核查基线：`main` 当前头为 `444b92ac`（2026-08-10），原始任务提交
`1d5d2ac9` 不在该祖先链，但后续主线提交 `f6773c15` 保留了该行为。

- `src/components/home/requestLogPresentation.ts` 通过
  `isExpectedCodexAutoReviewRequestedModel` 识别
  `codex-auto-review`/`codex-auto-review-*`，并把该映射标记为非严重的预期路由。
- `src/components/home/HomeRequestLogsPanel.tsx`、
  `src/components/home/RealtimeTraceCards.tsx` 和
  `src/components/home/RequestLogDetailSummaryTab.tsx` 保留请求到实际模型的可见映射。
- 聚焦回归位于
  `src/components/home/__tests__/requestLogPresentation.test.ts`、
  `src/components/home/__tests__/HomeRequestLogsPanel.test.tsx` 和
  `src/components/home/__tests__/RequestLogDetailDialog.test.tsx`，同时覆盖带 effort 后缀与普通严重 mismatch。

因此该轻量任务的 PRD 目标已由当前 `main` 的源码和测试闭环满足；本记录不改变后端路由或业务代码。
