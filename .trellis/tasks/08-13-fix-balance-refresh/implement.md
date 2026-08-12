# 执行计划

1. [x] 在独立 worktree 基于当前 beta 分支复核任务归档、账户用量契约和现有实现，记录可复现的用户时序。
2. [x] 运行现有 Rust runtime、前端 query 与 Provider card focused tests；确认本地 force/query 生命周期已覆盖该回归。
3. [x] 按根因在共享账户用量 HTTP 请求边界增加缓存绕过，保持适配器与路由副作用隔离；生成绑定无变化。
4. [x] 增加/补强回归测试：请求头 helper、NewAPI 三请求、custom Request，以及既有 zero-to-positive、in-flight tail、连续点击、target replacement 和 side-effect isolation。
5. [x] 执行 focused tests，再运行 `pnpm typecheck`、`pnpm lint`、相关 Rust fmt/check/test/clippy、`pnpm check:generated-bindings` 和 `git diff --check`。
6. [x] 通过 Trellis quality check，提交代码并记录修复与剩余风险。

## 风险与回滚点

- runtime 调度状态机和异步测试是主要风险点；本次复核证明其契约未需改动，回滚点为三个适配器请求边界的缓存头注入。
- 任何生成绑定差异都必须与 Rust 命令签名逐项核对，不能提交无关生成文件。

## 开始前门槛

- `prd.md`、`design.md`、本文件已完成并审阅。
- `implement.jsonl` 与 `check.jsonl` 已引用账户用量契约及本任务研究材料。
