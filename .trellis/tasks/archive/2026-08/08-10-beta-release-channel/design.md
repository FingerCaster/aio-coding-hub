# 完整 Beta 发布频道技术设计

## 1. 边界与所有权

父任务负责跨层合同、生成绑定、稳定回归和最终质量门禁。实现拆为三个子任务：

| 子任务 | 主要所有权 | 前置合同 |
| --- | --- | --- |
| 08-10-beta-release-pipeline | .github/workflows、scripts、发布/频道状态合同 | 产出 Beta manifest schema、tag/asset/channel-state 合同，供后端消费 |
| 08-10-beta-updater-core | src-tauri settings/updater、配置迁移、生成绑定、前端 updater service 类型 | 依赖 pipeline 固定的 Beta endpoint、manifest 字段和 Release URL 规则 |
| 08-10-beta-update-ui | src/pages/settings、src/hooks、src/query、更新对话框/侧栏测试 | 依赖 updater-core 的 channel-aware bindings/service；不得自行解析 manifest |

所有生产代码遵循现有 settings::update、generated IPC 和 invokeGeneratedIpc 边界。发布频道状态只由受控 GitHub Actions 写入；客户端不接受用户输入的 URL。

## 2. 频道与 manifest 数据合同

### 2.1 频道枚举

后端和前端共享 UpdateChannel = stable | beta。持久化默认 stable；缺失字段、旧 schema、导入值和未知字符串均按产品合同归一化为 stable，不能进入 Beta。普通 settings patch/update 不拥有该字段，专用 command 才能变更。

### 2.2 endpoint

- stable：继续使用 tauri.conf.json 中的 https://github.com/FingerCaster/aio-coding-hub/releases/latest/download/latest.json，不改变旧客户端行为。
- beta：后端使用常量 https://raw.githubusercontent.com/FingerCaster/aio-coding-hub/release-channels/latest-beta.json，在每次检查构建 URL 时附加受控 cache-buster，避免频道暂停被 CDN 旧内容延迟隐藏。该 URL 不来自 settings。
- Beta endpoint 404、非 JSON、字段缺失、平台 URL/签名缺失或版本不是有效 SemVer 时，检查返回明确错误/无候选；绝不 fallback 到 stable 并安装。

Manifest 使用 Tauri static format：version、notes、pub_date、四个平台的 url 与 signature。频道分支的 latest-beta.json 字节必须与被引用 Release 中已复核的 latest.json 一致。beta-channel-state.json 是给发布流程用的旁证，不由 updater 解析，包含 schema version、selected channel/tag/version/release id/source SHA、manifest SHA-256、previous ref SHA、action、workflow run identity 和 UTC 时间。

## 3. 发布流水线

### 3.1 入口与来源

在现有 release.yml 增加显式 release_channel（默认 stable）并保持无输入稳定 release-please 两阶段路径不变；Beta 必须同时提供 prerelease tag 和 target SHA。也可以把无构建的暂停/推进操作放进独立手动 beta-channel.yml，避免让暂停动作触发桌面构建。

Beta tag 只允许 aio-coding-hub-vMAJOR.MINOR.PATCH-beta.N，其中数字无前导零且 N 大于等于 1。入口先把 target 解析为完整 SHA 并确认远端 refs/heads/main 可达；随后显式解析已有 tag ref，或在 tag 不存在时以该 SHA 创建不可移动的 tag ref。已有 tag peel 不同、已有公开 Release、非空/身份不符的 draft 都失败关闭。只有 tag ref 已可重新 fetch 且 peel 到同一 SHA 后才创建/读取匹配的空 draft Release，并把 immutable SHA 而不是 tag 传给所有下游 checkout。公开前重新 fetch origin/main 和 tag，再以 git merge-base --is-ancestor SOURCE_SHA origin/main 与 tag peel 双重验证。

### 3.2 版本 overlay

在每个平台构建 job 的 immutable checkout 后运行 scripts/release-version-overlay.mjs：

1. 从已经验证的 Beta tag 取得完整版本字符串，并校验与 channel/tag 一致。
2. 结构化更新 package.json 与 src-tauri/tauri.conf.json 的 JSON version；用受保护的 [package] version = 锚点更新 src-tauri/Cargo.toml；调用 Cargo metadata 让 src-tauri/Cargo.lock 只发生预期 root package version 变化。脚本对四个文件使用确定的 UTF-8、LF 和格式规则，避免平台换行造成摘要漂移。
3. 用 cargo metadata --locked --no-deps、JSON 解析和精确 diff 校验四个文件全部等于同一版本，拒绝额外文件修改。
4. 输出 release-version-attestation.json（channel、tag、source SHA、version、修改文件名/sha256、脚本版本），随平台候选 artifact 上传，不进入公开资产矩阵。

稳定构建运行同一 attestation 检查但 overlay mode 为 none，要求源文件版本与稳定 tag 一致，确保稳定路径合同没有被放宽。候选 manifest schema 增加 channel/overlay 身份字段；稳定 selftest 继续覆盖稳定正例。

### 3.3 构建、晋升和公开

- 复用现有 support-matrix.mjs 和四个平台官方矩阵，不增加未经登记的构建脚本。
- release-promotion.mjs 接受 channel 参数，验证 candidate 的 channel、tag、source SHA、run id/attempt、overlay digest、精确 14 项资产、大小、sha256 和签名。
- 稳定目标必须 draft=true、prerelease=false；Beta 目标必须 draft=true、prerelease=true，且二者都必须空资产、同一 release id、overwrite_files=false。
- 公开步骤将 Beta Release 更新为 draft=false、prerelease=true、make_latest=false；稳定步骤保留现有发布和 Homebrew 分支。Beta publish-homebrew-cask 完全跳过，且不读取 Beta macOS zip 生成 Cask。
- Release 公开成功、资产 digest 再验证通过后，调用 release-channel.mjs promote 将 Release 的标准 latest.json 写入 latest-beta.json 并更新状态文件。稳定公开也调用该步骤，但仅当目标 SemVer 严格高于当前指针时推进；这样正式版覆盖同线 Beta，未来 Beta 不被较低稳定版覆盖。

## 4. 可变频道指针与暂停

release-channel.mjs 只操作 GitHub Git Data API：读取 refs/heads/release-channels 当前 commit/tree，生成两个 blob，创建以旧 commit 为 parent 的 tree/commit，再以 force=false 更新 ref。首次发布在受控 workflow 中创建分支；并发者若旧 ref SHA 不匹配则失败，不重试覆盖。

promote 只接受公开、非 draft、身份和资产 manifest 已复核的稳定或 Beta Release。普通推进不得降低已选版本；pause 是单独显式动作，可选择已验证稳定版或此前安全 Beta，并记录 withdrawn target、操作者输入、旧/新 ref、manifest digest 和 workflow run。暂停不删除 Release/asset。

下载前复核由后端执行：Beta resource 记录候选版本和频道；安装前用同一 Beta endpoint 重新检查，要求仍返回同版本且平台 URL/签名摘要相同。指针暂停、推进到更高版本、设置切换或复核网络失败都会关闭旧 resource，返回可重试的 stale/error，不执行安装。

## 5. Rust 设置与 updater

### 5.1 设置所有权

在 AppSettings 增加 update_channel（默认 stable、受控 serde 归一化），SettingsView 暴露只读值。新增专用 settings_update_channel_set command：

- stable -> beta 要求现有 RiskyIpcConfirm，在共享 settings lock 内只写 owned field；
- beta -> stable 无额外确认；
- 返回 canonical SettingsView，不调用 whole-snapshot settings::write；
- 普通 settings_set/settings_patch 的 generated 输入不包含该字段。

config_export 序列化前 clone settings 并强制 stable；prepare_config_import 解析后强制 stable，再交给既有 whole-import CAS。导入失败或并发获胜者沿用现有回滚合同。

### 5.2 Resource 与 command

把当前直接存入 ResourceTable 的 Update 包装为带 channel、version、候选 manifest/platform digest 的 ChannelBoundUpdate。DesktopUpdaterMetadata 增加 channel 和经过固定 tag 规则生成的 releaseUrl。

desktop_updater_check(expected_channel, timeout) 在 backend 读取 canonical settings，若与 expected 不一致则返回 UPDATER_CHANNEL_CHANGED；stable 使用默认 builder，beta 使用 endpoints 常量。结果 resource 只允许对应频道下载。增加 desktop_updater_discard(rid) 供频道切换和 stale 清理调用。

desktop_updater_download_and_install 保留现有风险确认和进度事件；先读取 resource、确认当前 settings channel 相同，再按上节规则对 Beta 做安装前 fresh check。任何 mismatch 都 close resource 后返回错误；成功或明确失败都不遗留 resource。

## 6. React/query/UI

- updaterKeys.check(channel) 以频道为 key；useUpdaterCheckQuery 只在 canonical channel 已成功读取且用户允许时 enabled，不能使用跨频道 keepPreviousData。
- useUpdateMeta、全局 checkingPromise、安装 promise 和 last-checked storage 都携带 channel/generation。切换频道时递增 generation、取消/invalidates 旧 key、关闭对话框并 discard 已知 rid；迟到 promise 只能清理资源，不能提交 UI。
- SettingsAboutCard 显示开关、当前频道状态和检查按钮。启用先开风险 Dialog，确认后调用专用 mutation；成功才更新 query/cache 并立即 foreground check，失败恢复关闭。关闭立即切稳定并清除 Beta 状态。
- UpdateDialog 读取候选 channel：Beta 标题/徽标/版本文案显式写 Beta 更新；portable 使用 metadata 的精确 Release URL，不打开通用稳定列表。侧栏徽标在参与者有 Beta 候选时可见，未参与者永远无候选/徽标。
- 所有 IPC/service 输出继续通过现有 generated binding 和 normalizer 校验，不在组件内对 unknown 做私有 cast。

## 7. 兼容、回滚与安全

旧稳定客户端只访问稳定 latest.json，因此 Beta 公开不会改变其行为。新增设置字段缺失时 stable；旧客户端无法理解 Beta endpoint，不作为 Beta 回退路径。频道指针回滚只改变未来检查，安装包和 Release 不变；客户端永不自动降级。发布 workflow 的 stable 默认分支、Homebrew、签名密钥作用域和 immutable SHA 门禁必须有基线回归。

## 8. 验证策略

脚本层覆盖 tag/version/channel-state/CAS/asset/prerelease/latest/overlay；Rust 层覆盖 settings ownership、import/export、endpoint、resource channel binding、SemVer 与 stale install；前端覆盖 opt-in/out、确认/失败、query key、后台检查、迟到 promise、徽标/对话框/portable；最后运行现有完整 prepush、generated bindings、Rust check/clippy/test、稳定 release selftests 和真实 Git fixture 模拟。
