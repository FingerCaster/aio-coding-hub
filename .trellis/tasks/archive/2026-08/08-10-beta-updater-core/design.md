# Beta 更新频道后端设计

## Dependency boundary

先读取 beta-release-pipeline 的最终 manifest/pointer contract；若 endpoint、字段名或 Release URL 规则改变，停止并同步父任务，不在 Rust 内自行兼容多个形状。

## Settings

在 infra settings 定义 UpdateChannel，默认 stable 并对未知 serde 值归一化 stable。SettingsView 只读暴露 update_channel。新增 settings_update_channel_set 专用 command，stable 到 beta 用 RiskyIpcConfirm，所有写入使用 settings::update 只拥有该字段；普通 SettingsUpdate/SettingsPatch 和 config migration 的导入输入不拥有它。

config_export 对序列化副本强制 stable；prepare_config_import 在 CAS 前强制 stable。新增测试证明导出 JSON 不包含 beta 或总是 stable，导入 beta=true 后 canonical settings 仍 stable，并且并发 import rollback 仍遵循现有 token/owner 规则。

## Updater selection

desktop_updater_check 接收 expected channel 和 timeout，读取 canonical settings；不一致返回 typed error。stable 保留 app.updater_builder 默认 endpoints，beta 用固定常量 endpoint 并附加 cache-buster。响应 metadata 包括 rid、channel、currentVersion、version、date、body 和 releaseUrl。

ResourceTable 存储 ChannelBoundUpdate，至少包含 Update、channel、version、manifest digest/平台 URL identity。discard command 幂等关闭 rid。download command 先验证 confirmation、canonical channel 和 resource channel，再对 Beta fresh-check 固定 endpoint；fresh 结果必须与 resource 的 version、当前平台 URL 和 signature digest 一致。任一失败 close rid 并返回 stale/closed error，绝不 fallback stable。

## Compatibility and tests

生成绑定后更新 app updater service 的 normalizer；旧 stable endpoint、SemVer comparator 和 install progress event 保持兼容。Rust tests 覆盖 defaults/unknown/import, settings race, endpoint selection, metadata, resource cross-channel, fresh-check mismatch, successful close and error close。
