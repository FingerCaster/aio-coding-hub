# Provider name 与过时 feature 清理

## Boundaries

身份收敛仍只有 `src-tauri/src/infra/codex_config/provider_projection.rs` 一条实现。UI、结构化 patch、raw TOML 和 proxy 投影都调用它，不另写字符串替换。

`CodexConfigPatch.features_remote_compaction` 与 `features_responses_websockets_v2` 从 Rust 类型、解析、patch 写入和生成绑定中删除。新增 `model_provider: Option<String>`，只接受精确的 `OpenAI` 或 `aio`；其他值在写入前失败，不改文件。`None` 表示本次不改身份。

`CodexConfigState.model_provider` 返回生效身份，类型为 `"OpenAI" | "aio"`，由同一个 `desired_provider_key_from_document` 计算。UI 不复制这套优先级。

## Desired Key

```text
if [features].remote_compaction is an exact boolean:
    true -> OpenAI
    false -> aio
else if root model_provider == "OpenAI":
    OpenAI
else:
    aio
```

注释、引号字符串、其他 table 里的同名 key 不算。`model_provider` 精确为 `aio` 且 legacy flag 不存在时是 aio。第三方根 provider 在 flag 不存在时也显示 aio，但只在用户真正提交 Provider name 或 desired key 发生变化时才重写。

`project_active_provider`、`is_managed_projection_applied` 和 raw 保存的前后比较都改用这个函数。投影仍只拥有 `model_provider` 与目标 provider 的 `name`、`base_url`、`wire_api`、`requires_openai_auth`。

## Write Path

结构化 Provider name patch：

1. 先按现有 patch 流程改其他字段。
2. `model_provider` 为 `Some` 时调用 `reconcile_provider_identity`。
3. 同一结果里删除 `[features].remote_compaction`。不写 `false`。
4. `patch_requires_provider_sync` 改为 `patch.model_provider.is_some()`。history sync 的 source/target 仍来自收敛前后的 managed key。

Raw TOML：

- desired key 未变：不收敛，不删除 legacy flag，不改第三方 `model_provider`。
- desired key 变化：收敛到新 key，并删除 `remote_compaction`，避免 flag 在下一次读取时盖过刚写的 `model_provider`。
- 路由开启时仍先把用户增量合并进 baseline，再投影。gateway URL 不进入 baseline。

无关 feature patch 继续走 `upsert_keys_auto_style`。未知 `[features]` key 保持现有“排到已知 key 后面”的行为。不要把 `responses_websockets_v2` 加进 `remote_models` 那种每次保存都删除的列表。`remote_compaction` 只在身份写入时删除。

`fast_mode` patch 只 upsert `fast_mode`。关闭写 `false`，因为 Codex 对该 key 的默认是开启，删除 key 等于重新开启。

## UI

AIO Provider 区第一项使用现有 `RadioGroup`，`name="provider_name"`，选项只有 `OpenAI` 和 `aio`。当前值取 `codexConfig.model_provider`。只有目标值不同于当前生效值才打开对话框。

对话框复用现有受控语义：取消、仅更新配置、同步会话记录。提交期间禁用选项、三个按钮、Escape 和遮罩关闭。成功且返回非空才关闭；失败保留目标并允许重试。标题改为切换 Provider name，并写明目标 key。按钮文案不变。

`useCliManagerCodexConfigSetMutation` 已经在每次 settled 时失效 config、raw config 和 proxy status，不需要再按字段加分支。

`fast_mode` 文案改为只描述 `features.fast_mode`。`multi_agent` 去掉“实验性”，保留“未设置时使用 Codex 默认行为”的现有分支。

## Compatibility

- 已被收敛且 flag 与 `model_provider` 一致的配置，显示和投影都不变。
- flag 仍在时，显示和投影继续跟 flag，即使它和根 `model_provider` 不一致。
- 成功的 Provider name 保存后，flag 消失，根 `model_provider` 成为之后的来源。
- 现有冲突错误码 `CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT` 保持不变。
- 手动 Provider Sync 仍固定 `sync_history = true`。

## Rollback

身份写入失败继续使用现有 config/backup/catalog/history 回滚。不要在 reconcile 之外改 provider table。若上线后需要撤回，恢复 `features_remote_compaction` 字段会重新引入已删除的绑定，不能只回 UI。
