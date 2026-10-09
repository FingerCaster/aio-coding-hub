# 清理过时的 Codex 配置字段

## Goal

Codex 路由和设置页不再写入官方已经删除或弃用的配置，同时不改动这台机器上现有的 Codex `config.toml` 或 `auth.json`。

## Background

官方配置参考是 [Codex config reference](https://developers.openai.com/codex/config-reference)，对照记录在 `research/official-config-comparison.md`。

`preferred_auth_method` 已在 Codex 0.35.0 从 `config.toml` 删除。当前仓库的本机投影和 WSL 配置仍会把它写回。工作区里已有一版未提交草稿，放在 worktree `E:/OrcaProject/aio-coding-hub-fork/codex-retire-outdated-config` 的分支 `FingerCaster/codex-retire-outdated-config`。

`approval_policy = "on-failure"` 在官方参考里已弃用，设置页仍把它作为推荐选项写出。`features.exec_policy` 和 `features.apply_patch_freeform` 不在当前官方参考里，设置页也不再展示，但结构化补丁仍能写入。

`auth.json` 的 `auth_mode = "apikey"` 与 `OPENAI_API_KEY` 仍是当前凭证格式。`model_provider`、`wire_api`、`requires_openai_auth`、`model_catalog_json`、`windows.sandbox`、`forced_login_method` 仍在官方参考中。

## Requirements

- R1. 本机 Codex 路由投影和 WSL Codex 配置不得插入 `preferred_auth_method`。投影、WSL 配置和关闭恢复都去掉这个根键，不论取值。
- R2. `preferred_auth_method` 不再作为代理托管字段。用户删除它后，这次删除进入基线，后续同步不得把 `apikey` 写回。
- R3. 设置页不再推荐或新选 `approval_policy = "on-failure"`。已有该值时控件仍能显示它，避免当前值变成空白。用户不改这个控件时，不因为其他设置保存而改写该行。
- R4. 结构化配置读写不再暴露 `features.exec_policy` 和 `features.apply_patch_freeform`。无关保存不主动删除文件里已经存在的这两行。
- R5. 实现、测试和检查都不得打开或写入本机 Codex home 的 `config.toml` 与 `auth.json`。测试只使用临时目录。

## Acceptance Criteria

- [ ] AC1. 基线含任意 `preferred_auth_method` 时，普通模式和 OAuth 兼容模式投影结果都不含该行。
- [ ] AC2. WSL Codex 配置脚本不再插入 `preferred_auth_method`，并删除已有的根键。关闭时也不把该键写回。
- [ ] AC3. 设置页文案和选项不再把 `on-failure` 当作推荐新值。现有 `on-failure` 仍能被读出并显示。
- [ ] AC4. 结构化补丁类型和生成绑定不再包含 `features_exec_policy` 与 `features_apply_patch_freeform`。
- [ ] AC5. 验证命令只使用临时 Codex home。任务记录里没有本机 Codex `config.toml` 被读取或写入的证据。

## Out of Scope

- 不改 `auth.json` 的 `auth_mode = "apikey"` 和 `OPENAI_API_KEY`。
- 不删除仍在官方参考中的 `model_provider`、`wire_api`、`requires_openai_auth`、`model_catalog_json`、`windows.sandbox`、`forced_login_method`。
- 不改变 `features.remote_compaction` 的旧值读取，也不在无关保存时删除用户已有的 `features.responses_websockets_v2`。
- 不启动应用，不对本机 Codex home 做修复、同步或配置写回。
