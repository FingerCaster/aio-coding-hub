# 过时 Codex 字段的写入边界

## Boundaries

只改 AIO 写入或提供给用户选择的 Codex 配置面：

- `src-tauri/src/infra/cli_proxy/codex.rs`
- `src-tauri/src/infra/codex_config/provider_projection.rs`
- `src-tauri/src/infra/codex_config/parsing.rs`
- `src-tauri/src/infra/codex_config/patching.rs`
- `src-tauri/src/infra/codex_config/types.rs`
- `src-tauri/src/infra/wsl/config_codex.rs`
- `src-tauri/src/infra/wsl/status.rs`
- `src-tauri/src/infra/wsl/manifest.rs`
- `src-tauri/src/infra/wsl/provider_model_discovery.rs`
- `src/components/cli-manager/tabs/CodexTab.tsx`
- 对应测试和 `src/generated/bindings.ts`
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`

不读取、不写入本机 Codex home。测试 home 只来自现有测试夹具的临时目录。

## Data flow

`preferred_auth_method`：

1. 路由投影从基线重建 live config。
2. 投影去掉根字段 `preferred_auth_method`，不论取值。
3. 该字段不再属于 `is_proxy_owned_path`。用户删除进入三方合并后的基线。
4. WSL 配置脚本删除已有的根字段，并且不再补写。
5. 关闭路由时不再把这个键从备份写回。

`approval_policy`：

- 解析器继续接受已有的 `on-failure`，这样当前文件能显示。
- 设置页的新选项只保留官方当前值：`untrusted`、`on-request`、`never`。
- 当前值已经是 `on-failure` 时，额外显示这一项，标签标明它已弃用。用户不改它时，保存其他字段不改写这一行。

`features.exec_policy` 和 `features.apply_patch_freeform`：

- 从 `CodexConfigState`、`CodexConfigPatch`、解析、补丁写入和生成绑定删除。
- 补丁写入不再把这两个 key 放进 feature 顺序或写入列表。
- 文件里已有的行不在无关保存中删除。

## Compatibility

- `auth.json` 的占位 `OPENAI_API_KEY` 和 `auth_mode = "apikey"` 保持现有所有权。
- 仍在官方参考中的 provider、catalog、`windows.sandbox` 和 `forced_login_method` 保持现有投影。
- `features.remote_compaction` 继续只作为旧文件的读取信号，本任务不改选择规则。
- 路由开启期间的 live 投影，以及关闭时写回的 live 文件，都不保留 `preferred_auth_method`。

## Rollback

配置写失败继续走现有 backup/live 回滚。若需要撤回，恢复投影和 WSL 脚本即可；不要额外写回本机 Codex home。
