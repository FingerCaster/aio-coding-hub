# 可选的本地 model_catalog_json 托管

## Decision

继续用本地文件和根 `model_catalog_json`。开关决定 AIO 能否占用这个字段。`model_catalog_url` 不参与。

关闭表示完全交还该字段，并走现有“没有目录所有者”的恢复路径。已经依赖受管目录的安装默认保持开启，避免下一次新启动的 Codex 丢掉 `aio/*` 选模和上下文投影。

## Setting

新增 `codex_managed_model_catalog_enabled: Option<bool>`，设置 schema 升到 66。

- JSON 缺键时反序列化为 `None`，不能用 `serde(default)` 把它变成 `false`。那样会在第一次目录对账前把现有安装当成关闭，并拆掉已经生效的绑定。
- `None` 只在 Profile 生命周期锁内的目录准备中推导一次：有受管 Profile、有启用的上下文规则，或当前根绑定已经是 AIO 生成文件时写成 `true`，否则写成 `false`。推导结果先写入设置，然后才按该值准备目录。设置写入失败时不改目录、配置或生成文件。
- 显式 `true` 或 `false` 不再推导。用户关闭后新增 Profile 或规则，值保持 `false`。
- 新的默认设置直接写成 `Some(false)`。

设置迁移本身看不到 Profile 表和 `config.toml`，所以不能在 `migration.rs` 里单独决定旧安装的初值。

## Catalog Lifecycle

`needs_generated_catalog` 改为：开关为 `true`，并且（Profile 非空或有启用规则）。开关为 `false` 或仍是 `None` 但已推导为 `false` 时，强制走现有空所有者分支。

- 该分支恢复原始根绑定或删除该键，并只删除 AIO 拥有的生成文件。
- Profile 记录和 `codex_model_context_rules` 不删除、不改 `enabled`。
- 关闭期间，启动对账、配置保存、Profile 增删、规则保存和升级命令都不得因为规则目标无效而失败。没有目录要生成，就不读取基础目录。
- 开启且所有者非空时，保留现有基础选择、投影、哈希、三文件事务和失败回滚。规则目标无效仍在写盘前失败，启动仍停在 `ReadingSettings`。
- 开启但所有者为空时，继续不安装目录。
- 升级入口只在开关为 `true` 且所有者非空时可用。关闭时返回现有 `Inactive`，不创建目录。

开关命令与目录 apply 共用 Profile 生命周期锁和 Codex 配置 mutation。用户提交的布尔值先写入设置，再 `prepare`/`apply`。目录 apply 失败时，只有设置仍等于本次提交值才恢复旧布尔值。补偿失败返回现有 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` 或设置恢复错误，不能报告切换成功。

## Config Ownership

`model_catalog_json` 只在开关为 `true` 且 Codex 路由启用时属于代理拥有路径。`model_provider`、`preferred_auth_method`、Windows sandbox 和受管 provider 字段的判断不变。

关闭后，`merge_raw_user_changes` 接受该键的用户修改。目录已按空所有者路径解除绑定，因此不再进入活动目录的 drift 校验。

## UI

开关放在 Codex 页「模型上下文规则」区域，与「升级受管模型目录」同一组控制。

- 文案说明这是本地 `model_catalog_json` 托管，只对新启动的 Codex 会话生效。
- 设置仍是 `None` 或写入被阻断时，开关不可操作，也不显示成关闭。
- 关闭时隐藏升级按钮。
- 成功后沿用现有 toast：请新启动 Codex 会话。不重启外部进程。

## Compatibility

- 旧安装的第一次成功对账之后，开关是普通持久布尔值。删除全部 Profile 和规则不会把它从开改成关。
- 用户自己的目录文件不被改写。关闭只恢复指向它的根键，或在原来没有键时删除根键。
- 已运行的 Codex 进程继续使用启动时读到的目录。

## Spec Follow-up

更新 `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md` 的 picker catalog lifecycle：生成条件改为“托管开启且存在 Profile 或启用规则”；关闭时的恢复、字段解锁和默认推导写进同一节。零所有者恢复句子保持不变。

## Rollback

目录阶段沿用现有反向补偿：backup、生成文件、live config 只回滚仍等于本次 after-bytes 的内容。设置阶段只在布尔值仍等于本次提交时恢复。任一步补偿失败都保持恢复错误，不把开关显示成已切换。
