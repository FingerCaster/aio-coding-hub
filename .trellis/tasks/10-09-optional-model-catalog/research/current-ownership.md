# model_catalog_json 当前所有权

检查日期：2026-10-09。基线：`FingerCaster/model-catalog-hosting` @ `f994a6bd`。

## 何时生成受管目录

`src-tauri/src/infra/codex_model_catalog/managed.rs:1101`：只要存在任一受管 Profile，或任一启用的模型上下文规则，`needs_generated_catalog` 就为真。没有这两类所有者时，同一准备流程不安装新目录，并恢复原来的 `model_catalog_json` 绑定或缺失状态。

生成文件位于应用数据目录的 `cli-proxy/codex/managed-model-catalog.json`。根配置被写成指向该文件。Codex 因此使用这份静态目录，不再合并安装包里的新模型。新启动的会话才会读到变更。

基础来源：原始根配置有绝对用户路径时，以该文件全文为底；否则运行当前安装的 Codex `debug models --bundled`。启用规则和 `aio/<profile_name_key>` 再投影进去。

## 字段为什么改不了

Codex 路由启用时，根键 `model_catalog_json` 与 `model_provider`、`preferred_auth_method` 同属代理拥有路径：`src-tauri/src/infra/codex_config/provider_projection.rs:646-652`。

保存配置时，`merge_raw_user_changes` 在 `provider_projection.rs:150` 调用 `validate_owned_edits`。该键的当前值与提交值语义不同就拒绝，错误为 `CODEX_PROXY_OWNED_FIELD_EDIT`，文案要求先关闭路由再编辑（`provider_projection.rs:715-724`）。

受管目录处于活动状态时，外部改掉根绑定会在 `managed.rs:1402-1405` 和 `managed.rs:1991-1995` 以 `CODEX_MANAGED_MODEL_CONFIG_DRIFT` 失败关闭。键可以缺失（`managed.rs:1964-1965` 返回 `None`），但一旦出现就必须是绝对路径字符串（`managed.rs:1967-1978`）。活动投影会把键写成生成路径，或在不需要目录时恢复原路径 / 删除该键（`managed.rs:2049-2053`）。

生成文件本身带 owner 与哈希。外部改动目录字节也会失败关闭，不会被静默覆盖成用户版本。

## 现有界面

Codex 页「模型上下文规则」只有「升级受管模型目录」和「管理规则」（`src/components/cli-manager/tabs/CodexModelContextRulesSection.tsx:336-349`）。没有单独的托管开关。`AppSettings`（`src-tauri/src/infra/settings/types.rs:560-595`）也没有对应布尔项。

## 已有契约

`.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md` 的 picker catalog lifecycle：代理启用且存在受管 Profile 时，AIO 拥有一份完整合并目录；零 Profile 时不保留生成目录。上下文规则是后来加上的第二类所有者，与 `managed.rs:1101` 一致。

`.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md` 要求设置回滚保留活动的受管 `model_catalog_json` 绑定。
