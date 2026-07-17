# 实施计划

## 1. 写前配置迁移安全

- [x] 添加共享 `skill_key` 单组件校验，接入 import/rollback/export 相关入口。
- [x] 将 Skill 文件、专用预算、metadata 序列化与路径冲突检查收敛为全输入预检。
- [x] 保证 staging/目标创建和 local 注册晚于所有安全验证；补路径、顺序、残留、状态与预算回归。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked`。
- 回滚点：仅撤销本节新增实现/测试，保留父任务和既有 8 MiB 修复。

## 2. Image Gen 网络、历史、scope 与 multipart

- [x] 完整实现 IP literal/global-route、URL 安全诊断和 reqwest 错误归一化。
- [x] 让 tasks list/read 以当前 canonical root 校验 DB 行，并移除前端 `convertFileSrc` 路径能力。
- [x] 按 Tauri 2 scope 真实语义采用零 Image Gen asset scope，root 切换不授予新旧根 renderer 权限。
- [x] multipart 全条目 decode 前完成数量、元数据、派生与聚合预算。
- [x] 补 SSRF、DB tamper/list/render、旧根撤权、正常历史、secret 与 multipart 副作用测试。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml image_gen --lib --locked` 及对应 Vitest。
- 回滚点：网络、历史投影、scope、multipart 四个小节分别可回到其聚焦测试通过的前一状态。

## 3. Grok OAuth、网关日志与 NewAPI

- [x] Grok device/token 响应改为 bounded reader，验证结构/字段并安全 clamp interval。
- [x] 网关持久化前清除 401/403 正文并保持内存 failover 分类；补 `SYNTHETIC_SECRET` 回归。
- [x] NewAPI 拒绝 credentialed/invalid URL、禁 redirect，三个端点分别执行 body cap 和全有或全无。
- [x] 保持 sub2api 不变，运行 Grok OAuth、gateway logging/failover、provider usage 聚焦测试。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked`。
- 回滚点：每个协议域独立回滚；不得通过恢复正文日志、放宽 URL 或 body cap 解决失败。

## 4. 跨层同步与自审

- [x] 同步 bindings、TS adapter/UI 与测试；更新 config/Image Gen/provider/gateway 最相关 spec。
- [x] 审阅所有 diff：验证数据源、helper 复用、Windows 行为、日志 secret、scope 回滚与测试非同义反复。
- [x] 运行所有新增 Rust/TS 聚焦测试及对应 Vitest。
- [x] 运行 `pnpm check:generated-bindings` 与 `pnpm build`。
- [x] 运行 `pnpm check:precommit:full` 与 `pnpm check:prepush`。
- [x] 独立运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked`。
- [x] 运行 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`。
- [x] 运行 `git diff --check` 与 `python ./.trellis/scripts/task.py validate 07-17-final-review-security-boundaries`。

## 5. 提交与归档

- [ ] 动态从 `Get-Command node`/`Get-Command pnpm` 补 PATH，提交代码与任务材料；不 amend、不 push。
- [ ] 仅归档 `07-17-final-review-security-boundaries` 并提交归档 bookkeeping。
- [ ] 确认父任务仍为 `in_progress`，输出 `IMPLEMENTATION_COMPLETE`、代码提交哈希、归档哈希、
      测试摘要与最终 `git status`。
