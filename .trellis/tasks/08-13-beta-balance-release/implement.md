# 执行计划

## 1. 规划与集成

- [x] 激活任务并确认 `origin/main`、余额提交、updater worktree 三方基线。
- [x] cherry-pick 余额修复，提取 updater 修复和相关 spec，记录冲突及解决理由；两次 cherry-pick 均无文本冲突。
- [x] 检查最终源码只包含两项用户请求及必要任务记录。

## 2. 质量门禁

- [x] 余额用量 focused Rust 测试（48 + 3 + 16）与 updater focused tests（19）。
- [x] Rust fmt/check/Clippy，前端 typecheck/lint/generated bindings。
- [x] release source/contract/promotion/channel/signing/support-matrix/Homebrew/CI selftests。
- [x] diff/spec/task validation；独立复审 findings-first 未发现 P1/P2，受影响 Rust focused 与跨层 selftests 均通过。

## 3. 集成发布

- [ ] 提交并推送集成分支，创建 PR 到 `main`。
- [ ] 等 required checks 通过后合并，记录合入后的 40 位 SHA。
- [ ] 从该 SHA 运行 Beta 发布 workflow，选择严格递增版本。
- [ ] 核对公开 Release flags、14 assets、signatures、manifest、pointer/state、Stable isolation。
- [ ] 执行 Windows updater 与余额刷新 smoke，记录结果。

## 4. 收尾

- [x] 更新相关跨层 spec 和任务记录。
- [ ] 归档任务、清理集成/updater/余额 worktree 与临时产物，保留主工作区既有改动。
