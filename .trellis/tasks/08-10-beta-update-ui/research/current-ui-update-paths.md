# UI 更新路径研究记录

## 研究范围

本记录只固定当前代码证据，供 Beta UI 设计和实现复核使用；它不定义新的产品规则。

## 已确认的入口

- src/pages/settings/SettingsAboutCard.tsx 已经是关于页的版本和检查更新区域，适合承载参与 Beta 测试开关及一次性风险确认。
- src/hooks/useAppBackgroundTasks.ts 负责启动时与定时后台检查，当前任务没有频道参数，且后台检查应继续保持静默。
- src/hooks/useUpdateMeta.ts 使用全局 checkingPromise、固定 updaterKeys.check()、候选缓存和安装资源 ID；这些状态若不带频道和 generation，会产生旧频道响应覆盖新频道的竞态。
- src/query/updater.ts 使用固定查询键和 keepPreviousData；切换频道后必须消除跨频道复用。
- src/ui/Sidebar.tsx 从候选状态生成 NEW 标记并打开更新入口；它不能在稳定用户收到 Beta 候选时自行判断或显示。
- src/components/UpdateDialog.tsx 目前使用通用“发现新版本”标题，便携版打开 constants/urls.ts 中的通用 releases URL；Beta 需要消费候选提供的精确 release URL。

## 配置和生成绑定

- 设置导入导出位于 src-tauri/src/infra/config_migrate/mod.rs。UI 应等待 updater-core 的规范化设置结果，不应把导入对象直接写入本地状态。
- src/generated/bindings.ts 是 Rust 命令和类型的生成物；频道字段或新命令变更后必须由项目生成脚本更新，UI 不手工维护同名类型。

## 约束结论

1. 频道状态是单一来源，所有 Hook、query、侧栏和弹窗从同一设置查询读取。
2. query key 包含 channel；每次检查上下文另带单调 generation。切换时先失效并清理旧候选，再开始新频道检查，迟到结果按 generation 丢弃。
3. UI 只展示 updater-core 规范化的 version、channel、releaseUrl 和可安装资源状态。
4. stable 是任何缺失、非法或导入来源 Beta 值的回退，不得通过旧缓存恢复 Beta。
