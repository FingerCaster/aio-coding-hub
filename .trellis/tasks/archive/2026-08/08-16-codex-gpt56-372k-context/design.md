# Design: Codex GPT-5.6 372K context catalog policy

## Decision Summary

将 372K 作为 AIO 拥有的 catalog 派生策略，而不是修改 Codex 安装目录或复用全局 `model_context_window`。策略由专属持久开关表达；现有 managed catalog 管线扩展为可由 managed profiles 和 372K 策略共同驱动。

目标常量为 `372_000`，与上游 bundled catalog 的 `272_000` 使用相同十进制口径。开启时三个目标条目的 `context_window` 与 `max_context_window` 都改为 `372000`；`effective_context_window_percent`、`auto_compact_token_limit` 及其他字段保持基础目录原值。

## Boundaries

### Persistent intent

- `AppSettings` 增加默认 `false` 的 372K 策略字段，schema 从 63 升至 64，并提供显式迁移。
- 字段可出现在只读 settings view 中，但从普通 `SettingsUpdate`/`SettingsPatch` 的 ownership 中排除，防止通用保存绕过 catalog 事务。
- 新增专属 Tauri command 执行 enable/disable。命令返回后端确认状态，前端不做不可恢复的乐观提交。

### Catalog policy

- 在 `codex_model_catalog::managed` 中定义三个 canonical slug 和 `GPT56_LONG_CONTEXT_TOKENS: u64 = 372_000`。
- `needs_generated_catalog = !profiles.is_empty() || enabled`。
- `generate_catalog` 先验证完整基础目录，再在 enabled 时精确定位三个目标条目并同时改写两个窗口字段，随后按现有逻辑追加 `aio/*` entries。
- 三个目标任何一个缺失、重复或字段结构不合法时返回稳定错误，不生成部分开启目录。
- owner metadata/payload hash 增加策略版本、enabled 状态和原始 catalog binding，从而保证重复生成稳定且外部修改可检测。

### Binding lifecycle

- 代理开启时继续以 proxy baseline/backup 为原始配置来源，复用现有三文件事务和恢复规则。
- 代理关闭时，以 live `config.toml` 为事务目标；首次绑定前记录其原始 `model_catalog_json`，后续从受验证 owner metadata 恢复该来源。
- 需要生成目录时绑定 AIO 路径；两个生成原因都消失时恢复原始 binding 并删除 AIO 文件。
- structured/raw Codex config 保存必须保留已激活的 AIO binding，同时把用户对其他根键的修改写入正确的原始配置/baseline。
- 应用启动、CLI proxy 生命周期、managed profile 增删/能力更新和 CLI fingerprint 变化复用统一的 `sync_current_locked` 入口。

### Dedicated toggle transaction

事务在现有 profile lifecycle lock 下串行化，并与 settings writer 的并发令牌规则兼容：

1. 读取当前 settings、profiles、live config、proxy baseline 和 generated file 快照。
2. 针对目标 enabled 值完整 prepare catalog/binding 计划，不先产生可见写入。
3. 持久化专属 settings intent，并保存可条件恢复的前态/提交态。
4. 应用 catalog/binding 计划；任一步失败时仅在文件仍等于本事务提交态时恢复 settings 和已写文件。
5. 返回重新读取的确认状态；启动同步负责进程崩溃后的最终对账。

实现可调整步骤 3/4 的先后，但必须证明失败注入覆盖每个提交点，且不能出现“开关为 true、目录仍被 272000 clamp”的成功返回。

### Codex home boundary

Codex home 决定 live config 和原始 catalog 的所有权根。为避免一次设置变更跨两个 home 搬运 binding，372K 开启时 settings 服务拒绝改变 `codex_home_mode`/`codex_home_override`，错误提示要求先关闭 372K。关闭事务完成恢复后，现有 home 切换流程保持不变。

## Data Flow

```text
CodexTab Switch
  -> dedicated query mutation / Tauri command
  -> settings + profile lifecycle lock
  -> load persisted intent, profiles, proxy state, live config, base catalog
  -> prepare full derived catalog and binding plan
  -> conditional settings persistence + managed plan apply
  -> confirmed state + query invalidation
  -> newly launched Codex reads model_catalog_json
```

Startup and lifecycle reconciliation:

```text
App/proxy/profile/capability lifecycle event
  -> load persisted 372K intent + managed profiles
  -> sync_current_locked
  -> regenerate only when policy/profile/base fingerprint changed
  -> validate binding and atomically converge files
```

## Compatibility

- Default `false` preserves existing installs and bundled dynamic behavior unless managed profiles already require a static catalog.
- Existing user catalog remains the base and all unknown JSON fields are preserved. Enabling fails closed if it cannot safely support all three target slugs.
- Downgrade caveat follows existing settings canonicalization behavior; schema migration and rollback tests must make the new default explicit.
- Current Codex processes retain their startup snapshot. Only newly started processes are promised to see the new catalog.
- 95% effective window remains Codex-owned: nominal 372000 yields effective 353400 and default auto compact threshold 334800.

## Rejected Alternatives

- Only set `model_context_window = 372000`: Codex clamps it to catalog `max_context_window = 272000`.
- Set a global model override plus only raise max: the root override can affect unrelated models and conflicts with existing manual-value ownership.
- Modify the installed Codex `models.json`/binary: upgrades overwrite it and MSI cannot own arbitrary external installations.
- Prefix-match `gpt-5.6*`: silently opts future models into an unverified policy.
- Set effective percentage to 100 or raise the raw value to compensate for 95%: changes upstream safety semantics and violates the exact 372000 policy.

## Rollback

- Feature disable is the product rollback: regenerate without GPT-5.6 policy, restore original binding if no managed profiles remain, and preserve all unrelated configuration.
- Transaction rollback restores only snapshots still matching the transaction's committed bytes; external drift returns recovery-required.
- Source rollback during development is confined to this branch/worktree. No changes are made to Codex installations or remote repositories.

## Beta Release

- 发布基线只读取和操作 `origin` / `FingerCaster/aio-coding-hub`。功能分支通过 PR 合入 `main` 后，记录合入提交的完整 40 位 SHA，并等待该 SHA 的 required CI 成功。
- 发布前重新读取 `release-channels` 的 `promotion_high_water_version` 和现有 tag/Release。当前高水位为 `0.60.41-beta.8`，因此空闲候选为 `0.60.41-beta.9`；若远端在此期间推进，则选择严格更高的下一个 Beta，绝不复用或移动已有 tag。
- 调度 `release.yml` 时显式传入 `release_channel=beta`、canonical Beta tag 和合入 SHA。所有构建、candidate、promotion、publication 与 channel pointer 都必须消费同一个不可变 SHA。
- 成功判据是公开 `draft=false` / `prerelease=true` / `make_latest=false` Release、精确 14 项官方资产、有效签名和四平台 `latest.json`，以及 `release-channels` 上 manifest/state 的 SHA、source、tag、run identity 一致。
- 任一 CI、构建、签名、资产、发布或 pointer CAS 失败都停止发布；不得覆盖资产、强制移动 tag 或强推 `release-channels`。公开 Release 已成功但 pointer 失败时，只能使用合同定义的显式 Beta pointer repair 路径。
