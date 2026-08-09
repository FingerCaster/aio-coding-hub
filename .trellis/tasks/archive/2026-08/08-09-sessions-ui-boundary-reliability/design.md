# 技术设计

## 子边界

这些修复共享前端质量门但不共享业务状态。每项保持局部 helper/组件所有权，不引入统一“可靠性”抽象。

- Infinite Query 通过对页码/`fromEnd` 语义对称计算 previous/next，并让 UI 从实际 pages 推导窗口边界。
- 内存诊断把预算对象提升到单次 report 作用域，使用有界 top-N 插入。
- 图片解析接收请求数量上限并在遍历/下载前截断候选。
- FormField 用 discriminated props 表达 control/group。
- CodeEditor loader 在 rejection 时仅清除自己缓存，并由组件状态提供 retry。
- AppLayout 使用稳定 flex column 和 `min-h-0` outlet wrapper。

## 兼容性

不改变 IPC schema、page size、maxPages、图片生成 API 或视觉设计 token。FormField 允许必要的调用点机械迁移，但不顺带改版页面。
