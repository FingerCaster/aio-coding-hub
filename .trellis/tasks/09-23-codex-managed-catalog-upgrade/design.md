# 受管模型目录升级设计

## Boundaries

升级是现有受管目录对账的一个显式入口，不是第二套目录生成器。

- 生成、owner metadata、基础来源选择、规则投影、`aio/*` 克隆和三文件事务继续留在 `src-tauri/src/infra/codex_model_catalog/managed.rs`。
- 规则停用继续走 `settings_codex_model_context_rules_set_sync` 已有的规范化、设置 CAS 和目录补偿。禁止先写设置再让 `sync_current_locked` 单独报成功。
- 自动调用点保持 `CatalogReconcileIntent::Background` 的指纹短路。只有升级命令使用新的强制重读意图。
- 候选列表命令继续只读，不触发升级。

## Command

新增一个专用 Tauri 命令，返回结构化结果，不把无效规则编码进错误字符串。

```rust
enum CodexManagedCatalogUpgradeRequest {
    Apply,
    DisableInvalidAndApply { model_ids: Vec<String> },
}

struct CodexManagedCatalogInvalidRule {
    model_id: String,
    context_window: i64,
    code: CodexManagedCatalogInvalidRuleCode, // target_missing | invalid_window
}

enum CodexManagedCatalogUpgradeResult {
    Inactive,
    Applied { settings: SettingsView },
    Blocked { invalid_rules: Vec<CodexManagedCatalogInvalidRule> },
}
```

`Apply` 与 `DisableInvalidAndApply` 都在 Profile 生命周期锁内强制重读当前基础：

1. 解析原始绑定。绝对用户路径只读该文件；否则运行当前 Codex `debug models --bundled`。
2. 先完成基础结构校验。JSON、模型数量、模型对象、slug、重复 slug、Profile 所需的可见模板和 `aio/*` 别名冲突仍是硬错误，直接返回现有错误码，不返回 `Blocked`。
3. 基础结构有效后，收集全部启用规则里的可修复目标：slug 缺失，或两个上下文字段不是非负整数。按规则的规范顺序返回，不在第一条停止。
4. `Apply` 遇到非空集合时返回 `Blocked`，写盘次数为零。
5. `DisableInvalidAndApply` 把请求中的 `model_ids` 当成集合与第 3 步的集合比较。不相等则返回冲突错误，不写盘。相等时只把这些规则的 `enabled` 改为 `false`，随后用剩余策略执行现有 prepare/apply。设置提交失败或目录 apply 失败时，沿用规则命令的反向补偿。
6. 强制重读后的生成字节、绑定和 owner 都与现状一致时，返回 `Applied` 且不重复写三个文件。
7. 没有启用规则且没有受管 Profile 时返回 `Inactive`，不创建目录。禁用动作使两者都为空时，走现有恢复绑定和删除生成文件的路径，结果仍是 `Applied`。

`Blocked` 不是启动失败。启动、代理、配置保存和能力更新继续调用现有 `sync_current_locked`；目标缺失仍在写盘前返回 `CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING` 或 `CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID`，并保持 `ReadingSettings` 降级。

## UI

入口放在 Codex 页的模型上下文规则区域，因为启动横幅已经导航到 `tab=codex&focus=model-context-rules`。

- 存在启用规则或受管 Profile 时显示「升级受管模型目录」。两类都不存在时不显示。
- 按钮与规则保存、Codex 配置保存共用现有 Codex 配置 mutation scope，避免并行写同一目录。
- 点击发送 `Apply`。`Applied` 后提示新启动 Codex 会话才会读取，并刷新设置、目录和上下文候选查询。
- `Blocked` 时在同一区域列出模型 ID、token 值和原因，并显示「禁用无效规则并升级」。该按钮提交刚才返回的完整 ID 集合。
- 启动状态为失败且错误包含 `CODEX_MODEL_CONTEXT_RULE_TARGET_` 时，进入 Codex 页自动发送一次 `Apply`，用返回的 `Blocked` 展示同一选择。失败原因不是规则目标时，只显示现有启动错误，不显示禁用按钮。
- 降级状态下命令仍可执行。成功后不自动调用「重试启动」。

## Compatibility

- 用户目录继续是基础。升级不把原始绑定改成 bundled，也不改用户文件本身。
- 自动指纹短路保持不变，避免每次启动都运行 Codex。
- 显式禁用是「启动不得静默停用规则」的唯一例外，并且只发生在 ID 集合被重新校验之后。
- 已运行的 Codex 进程不在升级范围内。

## Spec Follow-up

实现时更新 `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md`：记录升级命令、强制重读与指纹短路的区别、可修复规则集合，以及显式禁用的事务边界。启动失败和最后一份有效目录的现有句子保持不变。

## Rollback

目录阶段失败时，只回滚仍等于本次 after-bytes 的 backup、生成文件和 live config。规则阶段只在设置仍等于本次提交值时恢复旧规则。补偿失败继续返回 `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED` 或 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`，不能被升级成功结果盖住。
