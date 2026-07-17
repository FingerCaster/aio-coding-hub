# Implementation Plan: 第四轮终审关闭

## Entry And Convergence

- [x] 完整执行 start 检查：工作树干净、父任务 `in_progress`、`upstream` push URL 为 `DISABLED`。
- [x] 创建复杂子任务并关联父任务；完整记录九项 finding、约束、设计、计划与真实 JSONL。
- [x] 运行 `task.py validate` 完成 PRD convergence，并执行用户已授权的 `task.py start`。
- [x] 完整执行 `trellis-before-dev`：读取父/子工件、packages、guides、backend/frontend/cross-layer index，
      以及 Image Gen、OAuth、config migration、settings/IPC 具体 specs。

## Strict Serial Implementation

1. [x] 构建/复用跨 Unix/Windows 的 handle-relative open/create/identity primitive 与确定性 barrier hook。
2. [x] 修复 history read identity，覆盖同名 hardlink/path swap，证明外部字节不可读。
3. [x] 修复 history persist image/thumb/ref 相对创建，覆盖 symlink/junction rebind、外部无变化和 DB rollback。
4. [x] 修复 Skill export 根句柄递归、同句柄 metadata/read、目录 containment 与 hardlink 策略；回归同时
       证明 root 内任意字节逐字节 round-trip、root 外字节在文件/目录/junction/hardlink 替换下均不可导出，
       且生产实现不新增任何内容敏感过滤。
5. [x] 系统搜索生产 `settings::write`，迁移所有字段 owner 到锁内 RMW 与 CAS rollback，运行真实 writer barrier。
6. [x] 增加后端 budgeted batch hydrate、生成 bindings、接入前端，验证 read/Base64/IPC 前预算和有限并发。
7. [x] 为失败响应增加 8 KiB cap 与 512 字符安全摘要，TS 二次防御，覆盖 JSON/multipart/超限/secret。
8. [x] 修复 corrupt history 固定分类日志，捕获 console 参数验证 secret-free。
9. [x] 修复 OAuth `flowId`/`flow_id` 两层 sanitizer，系统审计 capability keys，覆盖 poll/cancel 生产失败。
10. [x] 修正 round-3 两条 JSONL，并为 archive 自动重写/全量 validate 增加机制与测试。
11. [x] 系统回归方案 A common gate、原四项问题、fixed upstream merge 行为和所有同类别生产入口。

## Required Loop And Spec Update

- [x] 实现完成后完整执行 `trellis-break-loop`，按 root cause/failed approach/new evidence 复盘并修正。
- [x] 完整执行 `trellis-update-spec`，把句柄绑定、settings owner/CAS、hydrate pre-IPC budget、错误/log
      redaction、archive JSONL rewrite/validate 写成现有 specs 中可执行且可测试的契约。

## Focused Validation

- [x] History read/persist Rust 单元与 integration，包含 `cfg(unix)` 和 `cfg(windows)` deterministic barriers。
- [x] Config migration Skill export 单元/integration，包含文件/目录替换、Windows junction/hardlink、
      root 内 synthetic sensitive-looking bytes 原样 round-trip 与 root 外字节零导出。
- [x] Settings persistence、grok config、CLI proxy、settings service 真实生产 writer 并发/rollback 测试。
- [x] Backend hydrate command + frontend persistence/controller/service 测试，验证超限后不启动额外 read。
- [x] Transport/adapter/controller JSON、multipart、超限和 synthetic-secret 测试。
- [x] Corrupt history console 捕获与 OAuth poll/cancel generated IPC/sanitizer 测试。
- [x] Trellis archive rewrite/validate 机制测试及 `python .trellis/scripts/task.py validate --all`。

## Full Gates

- [x] `pnpm tauri:gen-types` 连续运行两次且第二次 `git diff` 零漂移；`pnpm check:generated-bindings`。
- [x] 全部前端测试、typecheck、lint、format、build。
- [x] 全 Rust lib 与 integration suites（`--locked`）。
- [x] 本机仅有 Windows MSVC target，WSL 仅 docker-desktop；未联网安装 Unix 工具链，Windows features/条件编译已检查。
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`。
- [x] `git diff --check`、`pnpm check:precommit:full`、`pnpm check:prepush`。
- [x] 任一失败修复后重跑相关聚焦测试与受影响完整门禁，直至全部通过。

## Phase 3.4 And Finish

- [x] 提交前确认 diff 仅含 round-4 工作、父任务仍 `in_progress`、无 remote 操作、upstream push `DISABLED`。
- [x] 从当前 PowerShell 动态解析 `node`/`pnpm` 所在目录补 `PATH`，完成 Phase 3.4 工作提交。
- [x] 仅归档 round-4（父任务不归档），记录工作提交 hash；完成 archive/journal commits。
- [x] 最终报告工作/归档/journal hashes、完整门禁摘要、`git status`、父任务状态与 upstream push URL。

## Stop Rule

不请求开始、提交或归档确认；不访问 remote、不启用子代理、不合并 main。只有准备合并 main 时才询问用户。
Skill 导出只修复 authority 越界，不新增内容过滤、敏感词扫描、自动剔除或内容拦截。
