# 移植上游低风险修复

## Goal

从本地 upstream/main 420e9958091ae460d152a508b1eb0e2110ab733b 选择性移植三个与当前 fork 无冲突的修复：cda19b25 缓存写入指标显示、3b19a24b Windows asset CSP、f273d301 macOS 通知音频隔离。保持 upstream fetch-only，禁止扩大为整合其他冲突提交。

## Requirements

- 仅从本地 fetch-only 上游快照 `420e9958091ae460d152a508b1eb0e2110ab733b` 选择性移植以下三个已验证可干净应用的提交：
  - `cda19b25`：Home 请求日志中的缓存写入指标在缺失或为零时仍显示稳定值。
  - `3b19a24b`：Windows asset protocol 的 CSP 同时允许 `http://asset.localhost` 和 `https://asset.localhost`，并补充 `convertFileSrc` URL 回归测试。
  - `f273d301`：macOS 通知音频改用 `afplay` 临时文件，其他平台继续使用 `rodio`，并保留超时、终止和回收行为。
- 保留当前 fork 的已有 provider、网关、数据库 schema、依赖版本和发布版本决策；不执行整个 `upstream/main` 合并。
- 不引入与上述三个提交无关的上游提交，也不修复 pinned upstream 本身已有的缺陷。
- 保留当前工作区中用户或其他任务已有的修改。

## Acceptance Criteria

- [x] 三个目标提交的功能和回归测试均进入当前分支，且 `git diff` 中没有无关文件。
- [x] Home 缓存写入指标覆盖存在、零值和缺失字段的显示行为。
- [x] asset CSP 和 `convertFileSrc` 测试覆盖 HTTP/HTTPS asset origin。
- [x] 通知音频的 macOS 与非 macOS cfg 分支分别可编译；macOS 分支不再链接 `rodio`，临时文件和子进程生命周期有测试覆盖。（macOS target 在当前 Windows 环境未安装，已记录限制。）
- [x] 通过与改动范围匹配的前端测试、Rust 测试、格式检查、类型检查或其环境受限说明，并运行 `git diff --check`。
- [x] 记录上游快照、选择理由和未移植提交分类，方便后续继续审计。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
