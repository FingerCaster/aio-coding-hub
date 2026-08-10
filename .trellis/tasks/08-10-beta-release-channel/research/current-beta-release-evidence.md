# Beta 发布频道现状研究

## 代码证据

| 边界 | 证据 | 结论 |
| --- | --- | --- |
| 稳定发布 tag | .github/workflows/release.yml:88,154,756 | 现有正则只接受 aio-coding-hub-vMAJOR.MINOR.PATCH。Beta 必须新增显式频道分支，不能放宽稳定分支后让 prerelease 误入。 |
| Release 晋升 | scripts/release-promotion.mjs:318-360 | 目标必须是空的 non-prerelease draft，资产禁止覆盖；Beta 需参数化 prerelease 断言但保留 exact matrix/no-overwrite。 |
| 稳定 manifest | src-tauri/tauri.conf.json:60 | 稳定端点继续使用 releases/latest/download/latest.json。旧客户端天然不会看到 Beta。 |
| updater 入口 | src-tauri/src/commands/desktop.rs:651-716 | 运行时可用 UpdaterBuilder::endpoints 选择受控 endpoint；当前 resource 直接存 Update，下载前没有频道或候选复核。 |
| updater 比较 | tauri-plugin-updater 2.10.1/src/updater.rs:527-548 | 默认规则是 remote.version > current.version，因此正式版 0.60.41 高于 0.60.41-beta.N，稳定旧版不会降级 Beta。 |
| 后台检查 | src/app/useAppBackgroundTasks.ts:35-55 | 启动和每 300000ms 调用 updateCheckNow，后台不会开弹窗但会更新候选缓存。 |
| 候选缓存/徽标 | src/hooks/useUpdateMeta.ts:102-252、src/hooks/useGatewayStatus.ts:12、src/ui/Sidebar.tsx:150 | 查询键、全局 promise、安装 rid 和侧栏徽标目前没有频道身份，必须一起改。 |
| 设置写入 | src-tauri/src/infra/settings/types.rs:410,509、src/pages/settings/useSettingsPersistence.ts | AppSettings 由共享设置服务持久化；新增 Beta 字段应由专用 writer 所有，不能让普通 whole-snapshot patch 获得隐式所有权。 |
| 配置迁移 | src-tauri/src/infra/config_migrate/mod.rs:369-377,482-510 | 导出会序列化全量 settings，导入会解析后整体 CAS；导出和 prepare import 都要把 Beta 授权归一化为 stable。 |
| UI 更新入口 | src/pages/settings/SettingsAboutCard.tsx、src/components/UpdateDialog.tsx、src/hooks/useGatewayStatus.ts | About 卡适合承载参与开关；portable 目前只打开通用 releases 页，需改为候选的精确 Release URL 并标记 Beta。 |
| 版本来源 | package.json:3、src-tauri/Cargo.toml:3、src-tauri/tauri.conf.json:4、src-tauri/src/commands/app.rs:144 | 多个 Rust 运行时路径使用 CARGO_PKG_VERSION，不能只覆盖 Tauri JSON；Beta 构建需要同步 package/Cargo/Tauri/Cargo.lock 的临时版本覆盖。 |

## 选定技术结论

1. Beta manifest 指针存放在专用 release-channels 分支的 latest-beta.json，而不是可覆盖的 GitHub Release asset。一次 Git tree/commit/ref 更新同时写 manifest 与 beta-channel-state.json；ref 更新使用预期旧 SHA 和 force=false，竞争时失败关闭。
2. Beta 与稳定使用同一官方签名密钥和支持矩阵。Beta Release 自身附带标准 latest.json，频道分支只复制已经公开并复核过的 manifest 字节；稳定发布在版本顺序允许时也推进 Beta 指针，以便 Beta 用户收到正式版。
3. Beta 构建在 immutable checkout 后执行受保护的版本 overlay 脚本，严格修改四个版本文件并产生 digest attestation；不会为每个 beta.N 提交版本 bump 到 main。Cargo metadata/locked 校验负责确认 Cargo.lock 与 overlay 一致。
4. 客户端把频道放进查询键、全局检查 epoch 和 Rust updater resource。Beta 安装前重新请求当前 Beta manifest，版本/平台 URL/签名不一致就关闭 resource 并要求重新检查，覆盖暂停或指针推进后的 stale candidate。
5. participate_beta 的产品语义由专用设置 writer 保证；普通 settings_patch、配置导入和导出不能写入授权。Beta 启用由 UI 风险确认触发，关闭无需二次确认。

## 未纳入的替代方案

- 使用 releases/latest 或移动 Git tag 作为 Beta 指针：无法稳定选择 prerelease，且把可变频道状态混入不可变 Release 身份。
- 每次 Beta 直接修改 main 的四个版本文件：会污染 release-please 版本历史，且失败/重试时难以保持稳定主线。
- 在客户端访问 GitHub Releases API 枚举 prerelease：增加 API rate-limit、排序和撤回语义，失去固定 manifest 的简单签名边界。
- 通过 update_releases_url 让用户输入任意 endpoint：违反受控来源和稳定隔离合同。
