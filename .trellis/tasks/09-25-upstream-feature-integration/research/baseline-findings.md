# 本次整合外的基线发现

## F1：Codex OAuth status 旧 apikey drift 测试夹具过期

- 用例：`infra::cli_proxy::tests::codex_oauth_compatible_status_reports_drift_when_old_apikey_preference_remains`。
- 未修改 fork 起点：`270b808c678af99c5d1ee4535071acae0adbb465`。
- 隔离方式：使用 `git archive` 将该 SHA 导出到隔离目录（现位于已被 Vitest 排除的 .codex-temp/upstream-integration-baseline-270b808c），执行单个原始测试；没有覆盖当前工作区文件，也没有更改远程。
- 复现命令：`node scripts/tauri-test.mjs --lib codex_oauth_compatible_status_reports_drift_when_old_apikey_preference_remains -- --test-threads=1`。
- 基线与整合分支相同结果：期望 `Some(false)`，实际 `Some(true)`；基线在原始 tests.rs:1855 失败，运行 0.02s。
- 代码证据：夹具调用 `write_codex_proxy_files`，再由 `build_codex_config_toml` 生成配置；当前 builder 不再生成该用例名称声称的旧 root `preferred_auth_method = "apikey"`。用例未显式注入旧字段。
- 本次没有修改相关 Codex projection/status 实现或该测试。此项为 fork 原有测试夹具问题，不属于本次整合引入，也不标称为 upstream 来源缺陷。
- 范围处理：按 integration-only 约束保留原样，记录为独立后续修复候选；最终 Rust 回归明确排除这一个已独立复现的用例。不能把排除后的结果表述为无条件完整测试全绿。
- 后续授权后建议：让该用例显式写入旧 apikey 字段，再验证 status 拒绝旧字段、sync 能清除旧字段及普通/OAuth 两种投影。

## F2：既有 tauri.conf.json 的 Prettier 格式问题

- 全仓 pnpm format:check 仅报告 src-tauri/tauri.conf.json 格式不符合 Prettier。
- git diff 270b808c678af99c5d1ee4535071acae0adbb465 --exit-code -- src-tauri/tauri.conf.json 返回 0；基线与工作区 canonical blob hash 均为 6a3415f3c8b472e226b373e53e06ca5dd2312519，该文件本次未修改。
- 本次涉及的全部 20 个 TypeScript/TSX/MJS 文件单独执行 prettier --check 均通过；Rust cargo fmt --check 通过。
- 按 integration-only 范围不改动既有发布配置文件的格式。该项另列为基线问题，不把全仓格式 gate 标为通过。

## 本次整合中已修复的回归

- 两项移植 SQL 夹具未写 fork 的 canonical provider_uuid：补齐 UUID，不改 schema 校验。
- Grok Responses 旧透明输入断言与本次明确批准的 input normalization 冲突：更新为规范化输入断言，同时保留 response、header、日志与单次 attempt 断言。
- legacy literal-star 规则必须保留精确匹配优先级；严格新写验证不能删除防御读数据，补充 matcher 与回归。

未发现并修复任何固定 upstream 独立缺陷。本文件中的 F1 明确属于既有 fork 基线。
