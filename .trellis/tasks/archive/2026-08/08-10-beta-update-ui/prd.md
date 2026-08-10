# Beta 参与设置与更新界面

## Goal

为已经由 updater-core 提供频道状态和候选更新能力的桌面端，提供明确、可撤销、按设备保存的 Beta 参与入口，并让所有更新入口在稳定版与 Beta 之间保持一致。默认用户继续使用稳定频道；只有用户主动确认参与 Beta 后，应用才会查询、提示或安装 Beta。

## Background And Anchors

- About 设置卡片已经展示当前版本、平台和检查更新入口，位置为 src/pages/settings/SettingsAboutCard.tsx。
- 后台检查任务在 src/hooks/useAppBackgroundTasks.ts 中启动并以固定间隔运行；当前更新查询键和候选缓存位于 src/hooks/useUpdateMeta.ts 与 src/query/updater.ts。
- 侧栏的新版本标记和打开更新入口位于 src/ui/Sidebar.tsx；更新弹窗和便携版处理位于 src/components/UpdateDialog.tsx。
- 配置导入导出由 src-tauri/src/infra/config_migrate/mod.rs 负责，Beta 参与状态的跨设备导入规则由 updater-core 定义为默认稳定，UI 不得用导入结果绕过确认。
- updater-core 对 UI 暴露规范化的频道值、候选元数据、精确 release URL 和丢弃候选命令；UI 不解析 manifest，也不自行拼接下载地址。

## Requirements

### R1. Explicit Per-Device Opt-In

- 设置的 About/更新区域必须有默认关闭的参与 Beta 测试开关，文案同时说明这是预发布版本。
- 从稳定切换到 Beta 必须先展示一次风险确认：预发布版本可能不稳定、问题版本不会自动降级；取消、关闭弹窗或持久化失败时，频道仍为稳定。
- 只有风险确认完成且专用设置写入成功后，UI 才更新为 Beta 并触发一次前台检查。重复启动、普通升级和同一设备的设置读取必须保留已确认的状态。
- 从其他设备导入、旧配置迁移或备份恢复得到的 Beta=true 必须在 UI 上按稳定处理，不能自动弹风险确认后的 Beta 状态；当前设备须再次手动确认。

### R2. Channel-Scoped Queries And Cache

- 未参与 Beta 的用户只能执行稳定频道检查；不得请求 Beta manifest，也不得显示 Beta toast、对话框、徽标、候选安装项或 Beta 版本号。稳定检查的现有静默后台行为不能回归。
- 查询键必须包含频道；候选缓存、安装资源和后台检查上下文还必须携带频道与 generation。generation 是拒绝迟到异步结果的单调令牌，不作为跨频道复用旧数据的理由。切换频道时不得通过 keepPreviousData 或其他方式把旧频道候选短暂显示在新频道界面。
- 切换回稳定后立即停止 Beta 查询和提示，清理 Beta 候选、缓存、对话框状态及侧栏标记。已安装的 Beta 不得因切换而自动降级；UI 只接受严格高于当前版本的稳定候选。
- 开启 Beta 后的后续 Beta 更新不重复展示参与风险确认；每一次实际下载/安装仍保留普通更新确认，并明确标识 Beta。

### R3. Consistent User-Facing Surfaces

- Beta 候选在 About、侧栏、更新对话框、无障碍名称和便携版路径中都必须显示 Beta 更新及准确的预发布版本号（例如 0.60.41-beta.1），不能只显示通用的新版本。
- 更新对话框的 Beta 文案必须与候选频道绑定，不能依赖当前设置的瞬时值；频道切换或候选失效时不能安装旧频道资源。
- 便携版点击更新应打开该候选的精确 GitHub Release URL，而不是通用 releases 页面；非便携安装仍走 updater-core 提供的受校验安装资源。
- 稳定最终版覆盖同一版本线 Beta 时，UI 显示稳定版本并允许安装；Beta 参与开关保持开启，以便继续接收下一版本线 Beta。

### R4. Failure, Accessibility And Interaction

- 设置写入、频道检查、候选丢弃或安装资源失效时，UI 必须回到可解释且与后端频道一致的状态，不得留下与后台实际频道不一致的 Beta 标记。
- 开关、风险确认、Beta 更新提示和安装操作必须有可访问名称、状态和错误反馈；不能仅用颜色区分 Beta。
- 不新增通用“发现新版本”入口来绕过频道过滤；所有现有入口必须复用同一候选和同一频道标签。

## Acceptance Criteria

- [ ] 新安装或没有本地确认记录的设备默认显示稳定频道且开关关闭；完成风险确认并成功保存后才切换到 Beta，并立即触发一次前台检查。
- [ ] 关闭 Beta 后，下一次检查只访问稳定端点；现有 Beta 候选、缓存、对话框和侧栏标记被清理，已安装 Beta 不发生自动降级。
- [ ] 稳定用户的网络请求、后台轮询和 UI 不包含 Beta manifest、Beta 版本号或 Beta 提示；可通过 query mock 和渲染测试验证。
- [ ] channel 被纳入 query key；候选资源和异步 generation 同时绑定 channel。在切换频道的竞态测试中，旧频道响应不能覆盖或短暂显示。
- [ ] Beta 候选在 About、侧栏、弹窗和无障碍快照中都呈现 Beta 及准确版本；便携版打开精确 release URL。
- [ ] 同版本线稳定最终版优先于 Beta；开关保持开启且下一版本线 Beta 可继续检查。
- [ ] 导入含 Beta=true 的配置后，当前设备仍显示稳定/关闭且没有 Beta 请求；同设备正常重启和普通升级保留已确认参与状态。
- [ ] 参与风险确认只出现一次；后续 Beta 安装仍显示普通安装确认，并在确认前阻止失效或频道不匹配的候选。
- [ ] 设置写入失败、检查失败和候选丢弃失败都有可见错误或与后端一致的回退，且不会留下错误的 Beta UI 状态。
- [ ] 相关 TypeScript lint、类型检查、组件/Hook/query 测试和 Tauri 生成绑定检查通过。

## Out Of Scope

- Nightly、RC、自定义更新源、私有 Beta、遥测和跨设备同步。
- UI 直接读取 GitHub API、解析 manifest、计算 SemVer 或自行拼接下载 URL。
- 自动降级、双版本并行安装，以及首次 Beta 发布本身。

## Dependencies And Boundaries

- updater-core 子任务负责频道持久化、导入清洗、端点选择、候选资源生命周期和版本比较；UI 只消费其稳定接口。
- release-pipeline 子任务负责公开 Beta Release、静态 manifest、精确 Release URL 和可暂停指针；UI 不改变发布状态。
- 本子任务拥有 React 页面、Hook、query、侧栏和更新弹窗的用户体验与测试，不修改 Rust 设置模型或 GitHub workflow。
