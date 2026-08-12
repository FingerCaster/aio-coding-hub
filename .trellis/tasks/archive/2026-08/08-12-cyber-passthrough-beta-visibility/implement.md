# 实施计划

1. [x] 用户确认 schema 61 及以下空列表的一次性补回取舍，并批准本 PRD / design。
2. [x] 从最新 `origin/main` 创建独立 Orca worktree，激活本任务并加载 backend/cross-layer 规范。
3. [x] 实现 schema 62 迁移，复用共享 Cyber 默认常量，保持当前 schema 显式空列表语义。
4. [x] 补齐 Rust 迁移、反序列化、导入和幂等回归测试。
5. [x] 对齐前端默认/组件测试和跨层 upstream error settings spec；无必要不改 UI 生产逻辑。
6. [x] 运行聚焦与全量质量门禁，交由独立 Trellis check 代理复审并修复确认的问题。
7. [x] 提交、推送修复分支，创建 PR，等待 CI 全绿后合入 `origin/main`。
8. [x] 重新读取远端版本状态，以合入后的不可变 SHA 发布下一公开 Beta。
9. [x] 验证 Release、资产、签名、manifest、Beta pointer 和 stable 隔离；报告安装/验证方式。
10. [x] 清理已完成的独立 worktree，完成 Trellis 归档和开发日志。
