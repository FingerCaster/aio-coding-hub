# Beta UI 实施计划

## 开始前

- [x] 用户审阅并批准父任务和三个子任务的 PRD、design.md、implement.md 及真实 JSONL manifests。
- [x] updater-core 先落地并生成 bindings；release-pipeline 先冻结 endpoint、manifest 字段和精确 Release URL。
- [x] 记录当前工作区改动，确认不覆盖用户已有编辑。

## 有序步骤

1. [x] 接入 updater-core 的 UpdateChannel、频道设置读取/写入、候选和丢弃接口；移除 UI 中重复的版本/频道判断。
2. [x] 改造 useUpdateMeta、updater query 和后台检查：query key、候选资源、checkingPromise、generation 全部带频道；切换时取消、失效和清理旧频道。
3. [x] 在 SettingsAboutCard 接入默认关闭的参与 Beta 开关、一次性风险确认、保存中/失败状态和成功后的前台检查。
4. [x] 更新 Sidebar 和 UpdateDialog 的候选过滤、Beta 文案、准确版本、安装确认及便携精确 Release URL。
5. [x] 接入导入/恢复后的稳定归一结果，覆盖重启、普通升级和跨设备导入三种生命周期。
6. [x] 补齐 Hook/query/组件/无障碍测试，并在生成 bindings 后修正类型和调用点。

## 验证命令

- pnpm check:generated-bindings
- pnpm typecheck
- pnpm lint
- pnpm test:unit
- cargo fmt --check --manifest-path src-tauri/Cargo.toml
- cargo test --manifest-path src-tauri/Cargo.toml

## 风险与停点

- 若 updater-core 返回字段不能区分候选频道、generation 或精确 Release URL，停止 UI 实现并先修订共享接口。
- 若任何路径仍用固定 updater query key、keepPreviousData 或通用 releases URL，先补齐隔离再继续表面组件。
- 若设置写入失败后 UI 与后端频道不一致，停止并保留稳定/旧频道可见状态，不能用本地乐观值掩盖。
- 若稳定用户的网络 mock 或快照出现 Beta 内容，停止发布 UI 变更并修复过滤。

## 完成门槛

- [x] 所有父任务 AC 与 UI 子任务 AC 有对应测试或可复现验证。
- [x] 不存在跨频道旧候选可见、隐式 Beta 请求、自动降级或通用 URL 回退。
- [x] 通过 Trellis quality check 后再请求 task.py finish/归档；本计划本身不启动任务。
