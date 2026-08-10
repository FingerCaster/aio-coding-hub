# 完整 Beta 发布频道

## Goal

为 AIO Coding Hub 增加一个明确 opt-in、可退出且可暂停分发的完整 Beta 更新频道。测试用户能够收到与稳定版同等级签名和平台覆盖的预发布构建；默认稳定用户、稳定 `latest.json`、GitHub `latest`、Homebrew 以及现有发布完整性合同不受影响。

## Background And Existing Constraints

- 当前版本线为 `0.60.40`。首个 Beta 绑定下一稳定版本，版本序列为 `0.60.41-beta.1`、后续递增的 `0.60.41-beta.N`，最终正式版为 `0.60.41`。
- 当前发布工作流只接受稳定三段式 tag；手动入口、release source 解析和发布前复核均拒绝 `-beta.N`（[`.github/workflows/release.yml:88`](../../../.github/workflows/release.yml:88)、`:154`、`:756`）。
- 当前晋升脚本要求目标 Release 是空的、`draft=true` 且 `prerelease=false`（[`scripts/release-promotion.mjs:318`](../../../scripts/release-promotion.mjs:318)），因此 Beta 不能复用未经频道化的稳定晋升判定。
- 稳定发布合同要求 tag、构建、候选、晋升和公开都绑定同一 40 位 immutable commit SHA，并核验精确资产集合、大小和摘要（[`release-operations-contract.md`](../../../.trellis/spec/aio-coding-hub/cross-layer/release-operations-contract.md)）。
- `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 的版本共同影响构建；运行时关于页及多个兼容性判断使用 `CARGO_PKG_VERSION`，而 updater manifest 的版本取自 release tag。Beta 构建必须生成可审计、跨平台一致的确定性版本覆盖，不能只改 updater JSON。
- Tauri updater 的默认端点固定为稳定频道 `releases/latest/download/latest.json`（[`src-tauri/tauri.conf.json:60`](../../../src-tauri/tauri.conf.json:60)）；桌面检查直接使用 `app.updater_builder()`（[`src-tauri/src/commands/desktop.rs:651`](../../../src-tauri/src/commands/desktop.rs:651)）。当前依赖的 updater builder 支持运行时受控 endpoint，默认 SemVer 判定为 `remote > current`。
- `update_releases_url` 已存在于设置模型（[`src-tauri/src/infra/settings/types.rs:410`](../../../src-tauri/src/infra/settings/types.rs:410)），但当前 updater 不读取它。Beta 频道不得把该任意文本字段变成用户可编辑的下载源。
- 更新任务在应用启动时执行，并每 5 分钟静默检查一次（[`src/app/useAppBackgroundTasks.ts:35`](../../../src/app/useAppBackgroundTasks.ts:35)）；手动检查、后台检查和安装共用固定的 `updaterKeys.check()`（[`src/hooks/useUpdateMeta.ts:102`](../../../src/hooks/useUpdateMeta.ts:102)）。
- 侧栏 `NEW` 徽标直接来自更新候选缓存（[`src/hooks/useGatewayStatus.ts:12`](../../../src/hooks/useGatewayStatus.ts:12)、[`src/ui/Sidebar.tsx:150`](../../../src/ui/Sidebar.tsx:150)）；关闭 Beta 必须同时处理查询、缓存、对话框和 Rust updater resource。
- 内置配置导出会序列化完整 `AppSettings`，导入会整体解析并通过现有 CAS/回滚路径提交（[`src-tauri/src/infra/config_migrate/mod.rs:369`](../../../src-tauri/src/infra/config_migrate/mod.rs:369)、`:482`）。逐设备 Beta 授权必须显式从该可移植边界排除。

## Requirements

### R1. 逐设备明确参与

“设置 > 关于应用”提供 `参与 Beta 测试` 二态开关，默认关闭。启用前显示一次风险确认，明确预发布可能不稳定以及退出后不会自动降级；只有用户确认且专用设置写入成功后才进入 Beta 并立即检查。取消、写入失败、旧配置迁移、设置损坏或读取失败都不得启用 Beta。

参与资格属于当前设备。内置配置导出不得携带可在其他设备直接生效的授权；导入或恢复必须忽略/归一化输入中的 Beta 开启值。当前设备重启和正常版本升级应保留用户亲自确认过的选择。

### R2. 用户可见更新行为

未参与用户只检查稳定频道，绝不请求 Beta manifest，也不显示 Beta toast、更新对话框、侧栏徽标、portable 下载入口或安装候选。参与后发现预发布版本时不重复风险弹窗；更新对话框、侧栏可访问文案和 portable 精确下载入口必须明确标为 `Beta 更新` 并显示完整预发布版本，现有“下载并安装”确认保持不变。

GitHub Beta Prerelease 和签名资产保持公开。参与开关只控制应用内自动检查、提示和安装入口，不建立下载鉴权；不参与者仍可主动访问 GitHub Releases 手动下载。

### R3. 版本生命周期

用户关闭参与后，客户端立即停止 Beta 查询和提示、关闭 Beta 对话框、清除 Beta 候选并改查稳定频道，但绝不自动降级。只有稳定目标 SemVer 严格高于当前安装版本时才提示和安装；例如 `0.60.41` 可升级 `0.60.41-beta.3`，`0.60.40` 不可覆盖它。

用户保持参与时，Beta 频道也接受更高的正式版。`0.60.41` 应覆盖 `0.60.41-beta.N`；升级到正式版后开关仍开启，之后可继续收到 `0.60.42-beta.1`。

### R4. 手动发布与来源

Beta 仅允许维护者通过 `workflow_dispatch` 手动发布，输入规范 tag 与完整 immutable target SHA。目标 SHA 必须可从 `origin/main` 到达，并在创建/解析发布目标后及最终公开前分别验证；任一阶段来源漂移均失败关闭。`main` push、PR 合并和现有开发构建不得自动创建 Beta Prerelease 或推进 Beta 频道。

### R5. Beta Release 与资产

Beta tag 只能使用 `aio-coding-hub-vMAJOR.MINOR.PATCH-beta.N`，其中数字无前导零且 `N >= 1`。每个 Beta 生成公开的 GitHub Prerelease、完整官方平台矩阵、正式 updater 签名和版本匹配的 manifest；Release 必须显式 `prerelease=true`、`draft=false`、`make_latest=false`。Beta 不更新稳定 `latest.json`，不成为 GitHub `latest`，不触发稳定 Homebrew Cask。

Beta 构建在验证过的源 SHA 上应用由 tag 唯一决定的版本覆盖。所有平台必须证明源 SHA、tag、覆盖后的版本文件摘要、候选 run 身份和资产摘要一致；原 Git tag、Release 和已上传资产不可覆盖或移动。

### R6. Beta 频道指针与暂停

客户端使用固定、公开的独立 Beta manifest 入口，例如 `latest-beta.json`。该入口是可变频道指针，必须与不可变版本 Release 分离；每次推进只引用已公开、资产和签名已复核的稳定版或 Beta Release，并通过 compare-and-swap/非快进拒绝实现原子、可审计更新。

维护者必须能够手动暂停严重问题 Beta 的继续分发。暂停只将频道指针切换到已验证的最近安全目标，不能删除、移动或覆盖坏 Beta 的 tag、Prerelease 或资产。尚未安装的客户端在真正下载前必须复核候选仍是当前 Beta 指针；已安装坏 Beta 的客户端不降级，等待更高修复 Beta 或正式版。

### R7. 客户端频道一致性

受控 `stable | beta` 频道必须贯穿持久化设置、endpoint、查询键、manifest 解析、候选元数据、版本比较、签名验证、下载 resource、安装、重启后的检查和 UI 文案。后台任务与手动检查读取同一份已持久化选择。

频道切换必须取消/失效旧检查，关闭旧对话框，清除旧频道查询并释放已知 Rust resource；迟到结果只能写入原频道隔离缓存，不能重新显示。下载命令重新验证当前持久化频道与 resource 频道一致；Beta 下载还须复核频道指针仍选择同一候选。任何未知频道、损坏设置、endpoint/manifest 语义不匹配、跨频道 rid 或 stale candidate 都失败关闭，不得静默改用另一频道安装。

### R8. 发布完整性与稳定隔离

稳定入口默认行为、release-please 两阶段发布、tag-to-SHA 连续性、no-overwrite、精确资产矩阵、签名密钥范围、Homebrew 显式跳过分支和发布后验证不得回归。频道化发布脚本与自测必须同时覆盖稳定和 Beta 的正例、基线和反例；错误 prerelease/latest 状态、错误来源、重复/回退 tag、缺失/额外/篡改资产、频道指针竞争或不安全暂停目标全部拒绝。

## Out Of Scope

- Nightly、RC 或任意数量的自定义频道。
- 第三方/用户自定义 updater endpoint。
- 私有 Beta、账户体系、下载鉴权或私有制品托管。
- 未签名开发构建进入 Beta、按 `main` 自动发布、自动加入 Beta。
- Beta 遥测、崩溃上报或新的反馈系统。
- 自动降级、并行安装稳定版与 Beta、跨设备同步参与资格。
- 在本功能实现阶段实际发布首个 Beta；真实发布须在功能合并、CI 和发布门禁完成后单独授权执行。

## Acceptance Criteria

- [ ] AC1：默认新安装、升级旧安装、导入配置和设置读取失败均只走稳定端点；不会请求或显示任何 Beta 候选。
- [ ] AC2：启用必须经过风险确认与成功专用写入，随后立即按 Beta 检查；取消或失败保持稳定且无 Beta cache/resource。
- [ ] AC3：内置配置导出/导入往返不能迁移 Beta 授权；同一设备重启与普通升级能保留已确认授权。
- [ ] AC4：Beta 候选在更新对话框、侧栏可访问文案和 portable 精确 Release 链接中清晰标识，仅保留普通安装确认。
- [ ] AC5：关闭 Beta 会取消或隔离在途检查、清除 UI/cache/resource 并改查稳定；低版本稳定包不触发降级。
- [ ] AC6：保持参与者可从 `0.60.41-beta.N` 更新到 `0.60.41`，参与状态不变，之后仍可收到下一版本线 Beta。
- [ ] AC7：Beta 只能由手动规范 tag + `origin/main` 可达 40 位 SHA 发布；开始与公开前的可达性、tag 和 source identity 一致。
- [ ] AC8：所有官方平台产物内的应用版本、候选 manifest、tag、签名 URL 和 source/overlay attestation 一致。
- [ ] AC9：Beta Release 公开且 `prerelease=true`、`make_latest=false`；稳定 `latest.json`、GitHub `latest` 和 Homebrew 不变。
- [ ] AC10：Beta 频道指针只选择已复核 Release，竞争推进失败关闭；暂停后旧候选在下载前被拒绝，原 Release/资产不变。
- [ ] AC11：跨频道 query/rid、迟到检查、损坏设置、未知频道、无效 manifest、网络失败和 stale candidate 均不会安装另一频道资产。
- [ ] AC12：发布脚本自测、Rust 设置/updater 测试、生成绑定检查、前端查询/交互测试及稳定发布回归全部通过。
