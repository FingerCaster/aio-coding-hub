# 长会话与前端边界可靠性

## Goal

移植六个互不改变产品架构的前端可靠性修复：长会话双向窗口、内存诊断共享预算、图片响应 fanout 上限、FormField 标签合同、CodeEditor 加载恢复和 AppLayout 剩余高度。

## Evidence

- `src/query/cliSessions.ts:83-107` 设置 `maxPages=10` 但只有 `getNextPageParam`。
- `memoryDiagnostics.ts` 为每个 query 重置 200k 节点预算；`gptImageAdapter.ts` 遍历并下载上游返回的全部 `data[]`。
- `FormField` 允许未注入 id 的任意 ReactNode 却始终输出 `<label htmlFor>`。
- `CodeEditor` 永久缓存 rejected dynamic import promise。
- `AppLayout` 的 startup banner 与 Outlet 未使用明确的剩余高度容器。
- 候选参考：`e57acb54`、`5d4906c5`、`9a280136`、`9e83772c`、`5b13683b`、`d12dbfe3`。

## Requirements

- `R1`：会话消息十页缓存支持 previous/next 双向取回，并显示真实窗口起止而非误称完整会话边界。
- `R2`：一次内存诊断共享 200k 节点及有界 query 数量，只维护 top 20，不全量排序无界集合。
- `R3`：图片响应在任何 URL 下载前限制到 `min(requested n, 10)` 个合法候选。
- `R4`：FormField 明确区分单 control 与 composite group；label/hint 关系可被辅助技术解析。
- `R5`：CodeEditor import 失败清除缓存并展示可重试错误，不让失败 Promise 污染整个进程生命周期。
- `R6`：AppLayout 在横幅存在时仍让 Outlet 占据剩余高度并可滚动，不裁切设置页底部。

## Acceptance Criteria

- [ ] 每项修复都有聚焦组件/服务测试，长会话覆盖页面淘汰后双向恢复。
- [ ] 图片 URL 响应超限时下载调用次数受限，且 `b64_json`/URL 混合响应保持正确。
- [ ] FormField 所有生产调用点通过类型检查并符合 control/group 合同。
- [ ] CodeEditor 首次失败后可重试成功；错误 UI 不与编辑器重叠。
- [ ] AppLayout 在小视口和 startup error banner 下可访问全部内容。
- [ ] typecheck、lint、Vitest、Vite build 和 diff check 通过。
