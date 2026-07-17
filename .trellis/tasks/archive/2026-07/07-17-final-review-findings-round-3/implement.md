# Implementation Plan: 第三轮终审关闭

## Entry

- [x] 创建唯一 round-3 子任务并挂到父任务。
- [x] 完整读取 `trellis-before-dev`、适用 spec index/具体契约及共享 guides。
- [x] 本地只读核对网关冲突两侧与 10 条悬空 JSONL。
- [x] 校验 planning artifacts 并执行已授权的 `task.py start`。

## Strict Serial Work

1. [x] 修复 NAT64/6to4 SSRF 分类并运行 transport 聚焦测试。
2. [x] 绑定 Device OAuth flow 所有权并运行 OAuth 聚焦测试/bindings。
3. [x] 加固 storage root/task/file 逐组件与稳定身份边界，运行路径负例。
4. [x] 修复 shared atomic temp 名及 Skill round-trip 回归。
5. [x] 增加原子 settings update 并覆盖并发 barrier 交错。
6. [x] 用 transaction 覆盖 persist INSERT 与最终验证。
7. [x] 实现历史缩略图优先、原图/参考图按需加载及并发/字节上限。
8. [x] 固定 Image Gen path 错误与端点枚举/脱敏日志。
9. [x] 修正 archive JSONL 并增加 active/archive 全量引用存在性校验。
10. [x] 为用户决策 A 增加/复核 common-gate skipped、继续候选、完整 503 诊断回归。
11. [x] 完整读取并执行 `trellis-check`，修复所有发现后重复检查。

## Focused Validation

- [x] `cargo test --manifest-path src-tauri/Cargo.toml image_gen --lib --locked`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml oauth --lib --locked`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked`
- [x] settings persistence/service 对应聚焦测试。
- [x] Image Gen controller/persistence/detail/service Vitest。
- [x] Trellis task validation 与全 JSONL 引用检查。

## Full Gates

- [x] `pnpm tauri:gen-types` 与 `pnpm check:generated-bindings`
- [x] `pnpm build`
- [x] `pnpm check:precommit:full`
- [x] `pnpm check:prepush`
- [x] locked Rust lib 与全部 integration suites
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`
- [x] bindings/typecheck/diff-check、`git diff --check`、Trellis full-scope check
- [x] 确认无 remote 操作、无秘密泄漏、upstream push `DISABLED`。

## Stop Rule

所有实现与门禁完成前，不提交、不 finish/archive round-3，不归档父任务。
