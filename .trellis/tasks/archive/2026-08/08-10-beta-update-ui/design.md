# Beta UI 技术设计

## Ownership

本子任务只修改 React 页面、Hook、query、侧栏、更新弹窗及其测试/生成绑定引用。Rust 设置模型、命令实现、manifest 生成、Release workflow 和频道指针由 updater-core 与 release-pipeline 子任务拥有。若接口未定稿，先在接口处停下，不在 UI 里复制实现。

## Shared Contract

updater-core 提供以下稳定接口，具体命名以生成 bindings 为准：

- 当前频道：stable 或 beta，缺失和非法值归一为 stable。
- 专用频道设置写入：stable 切换 beta 前由后端验证一次性风险确认；普通设置 patch 不得覆盖它。
- 频道候选：channel、version、release URL、可安装资源状态和候选资源 ID。
- 频道感知检查、丢弃候选和安装前 fresh-check。

UI 不读取原始 manifest，不枚举 GitHub Release，不拼接下载地址，也不自行实现 prerelease SemVer。

## State Flow

1. 启动时读取规范化频道设置，初始化为 stable；只有本地确认记录明确为 beta 时才显示参与状态。
2. 频道值进入 query key；generation 作为独立的单调异步令牌。检查开始时携带 channel、generation 和请求超时；返回结果若不匹配当前 generation/channel，直接丢弃。
3. stable 与 beta 使用不同缓存命名空间，切换时取消旧请求、失效旧 query、清理候选资源和弹窗状态，然后启动新频道的前台检查。
4. 后台轮询读取同一频道状态；未参与 Beta 时只调用 stable 检查且不产生任何 Beta 可见状态。
5. 组件只消费规范化候选。Beta 候选通过共享标签模型渲染 Beta 更新、准确版本和 release URL。

## Participation Flow

- About 卡片开关从关闭切换开启时，先打开风险确认。确认按钮调用专用 writer；保存成功后更新频道状态并触发前台检查，保存失败保持关闭并展示错误。
- 关闭开关时先写入 stable，成功后清理 beta candidate/cache/dialog/sidebar；不得调用降级安装。若写入失败，保留 Beta 状态并说明未切换。
- 风险确认只与参与开关绑定一次。后续 Beta 更新弹窗只执行普通下载/安装确认，标题、版本和频道标签明确标注 Beta。

## Surface Rules

- Settings About：显示当前频道和开关的可访问名称、忙碌/错误状态。
- Sidebar：只有当前频道候选才生成更新标记；Beta 标记必须包含文本 Beta 更新或等价可访问名称，不用颜色单独表达。
- UpdateDialog：标题、版本、正文、安装按钮和无障碍描述均从候选 channel 派生；候选失效或频道变化时关闭/刷新，禁止继续安装。
- Portable：调用候选的精确 release URL；若 URL 缺失或不匹配候选，显示错误并不打开通用 releases 页面。
- 稳定最终版覆盖同版本线 Beta 时，按 updater-core 的候选排序显示稳定版本，同时不重置已确认的 Beta 开关。

## Cache And Race Protection

- updater query key 形如 updater/check/channel，不能使用跨频道 keepPreviousData；generation 保存在检查上下文和候选提交守卫中，不用来复用旧 query 数据。
- checkingPromise、candidate cache、install resource 和 dialog open state 均携带 channel/generation。
- 频道切换、设置写入成功、fresh-check 失败和候选丢弃都必须使旧候选不可见；迟到响应不得恢复旧状态。

## Accessibility And Copy

- 开关使用明确的 label、checked、disabled 和错误关联；风险确认说明预发布不稳定与不自动降级。
- Beta 版本号用文本呈现，标题不依赖颜色；更新和安装按钮在屏幕阅读器中包含频道和版本。
- 复制文案保持短且动作导向，不新增解释性营销区块。

## Tests

- Hook/query：频道隔离、generation 竞态、后台稳定默认、关闭清理、稳定覆盖 Beta。
- 组件：一次性风险确认、保存失败、Beta 标签/版本、侧栏可见性、便携精确 URL、无障碍属性。
- 集成：导入 Beta=true 的设置仍为稳定；生成 bindings 后 TypeScript 类型与 updater-core 命令一致。
