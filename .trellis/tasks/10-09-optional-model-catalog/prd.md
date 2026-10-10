# 把 model_catalog_json 托管做成可开关功能

## Goal

用户可以自己打开或关闭 AIO 对 Codex 根配置 `model_catalog_json` 的本地文件托管。打开时保持现在的生成、锁定和升级。关闭时该字段交还给用户，不再是必填受管项。

## Background

现状见 [current-ownership.md](research/current-ownership.md)。`model_catalog_url` 不能代替这个文件，见 [provider-catalog-url.md](research/provider-catalog-url.md)。

- 有任一受管 Profile，或任一启用的模型上下文规则时，AIO 生成 `cli-proxy/codex/managed-model-catalog.json`，并把根 `model_catalog_json` 指过去（`src-tauri/src/infra/codex_model_catalog/managed.rs:1101`）。
- Codex 路由启用时，该根键不可改。不同的提交值返回 `CODEX_PROXY_OWNED_FIELD_EDIT`（`src-tauri/src/infra/codex_config/provider_projection.rs:646-652`、`provider_projection.rs:715-724`）。活动目录期间外部改绑定返回 `CODEX_MANAGED_MODEL_CONFIG_DRIFT`（`managed.rs:1402-1405`、`managed.rs:1991-1995`）。
- 两类所有者都没有时，现有流程恢复原来的路径或删除该键，并只删除 AIO 拥有的生成文件。
- 目录在 Codex 启动时读取。已经打开的会话不会热更新。

## Requirements

- R1. 提供一个持久化开关，让用户选择开启或关闭根 `model_catalog_json` 的本地文件托管。
- R2. 开启且仍有受管 Profile 或启用的上下文规则时，继续生成受管文件并把根字段指过去。保持现有所有权校验、失败回滚和「升级受管模型目录」。
- R3. 开启但两类所有者都没有时，不安装新的受管目录。这与现在的空所有者行为相同。
- R4. 不使用 `model_providers.*.model_catalog_url` 代替这个文件。
- R5. 关闭时恢复托管前的根字段：原来有绝对路径就写回该路径，原来没有这个键就删除它。只删除 AIO 拥有的生成文件。受管 Profile 和上下文规则数据保留，但不再投影进目录。
- R6. 关闭后，保存 Codex 配置可以修改 `model_catalog_json`。该键不再返回 `CODEX_PROXY_OWNED_FIELD_EDIT`，AIO 也不再因为外部修改它而报 `CODEX_MANAGED_MODEL_CONFIG_DRIFT`。
- R7. 缺少该开关的现有安装只推导一次：已有受管 Profile、启用的上下文规则，或根字段已经指向 AIO 生成文件时，持久化为开启；否则持久化为关闭。之后只随用户的选择改变。关闭期间新增 Profile 或规则不会自动重新开启。
- R8. `model_provider` 与 `preferred_auth_method` 的现有托管不因这个开关改变。
- R9. 切换只影响之后新启动的 Codex 会话，不改写已经运行的 Codex 进程。

## Acceptance Criteria

- [ ] AC1. Codex 页能看到托管开/关，选择会保存下来，并说明需要新启动 Codex 会话。
- [ ] AC2. 开启且存在受管 Profile 或启用规则时，目录、上下文投影、`aio/*` 条目和升级入口仍按现在的规则工作。开启但两类所有者都为空时，不创建受管目录。
- [ ] AC3. 关闭后，根字段恢复为原来的用户路径或缺失状态，AIO 生成文件被删除。Profile 与规则记录仍在。保存配置可以修改 `model_catalog_json`，不再返回 `CODEX_PROXY_OWNED_FIELD_EDIT` 或 `CODEX_MANAGED_MODEL_CONFIG_DRIFT`。
- [ ] AC4. 缺少开关的旧设置：有 Profile、启用规则或现有生成文件绑定时保存为开启；三者都没有时保存为关闭。随后把开关关掉再新增 Profile，根字段保持用户可编辑，不会重新指向生成文件。
- [ ] AC5. 切换不重启、不改写已经运行的 Codex 进程。

## Out of Scope

- 不改变 `model_provider`、`preferred_auth_method`、Windows sandbox 或受管 provider 表的所有权。
- 不改变受管 Profile 文件格式，不删除已有 Profile 或上下文规则数据。
- 不热更新已经打开的 Codex 会话。
- 不改 Codex 远端 `/models` 协议，不替换 Codex 安装文件。
- 不把托管改成 `model_catalog_url`。
