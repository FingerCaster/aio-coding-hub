# 继承模型预览依据

OMP 18.3.2，源码 commit 7853b4e499936f9dcc13c9b64adb55f6b342aabf，参考仓库 D:/UGit/oh-my-pi-source-reference。

- config/model-resolver.ts：resolveConfiguredRolePattern 中 smol/slow 未配置时先继承已配置 default；tiny 回退 smol，memory 回退 tiny；advisor 仅继承显式配置 slow，否则走 slow 内置优先链。
- 自定义角色支持 @role、兼容 pi/role 和 *；循环不能无限展开。回退列表和模糊选择器最终取决于可用模型池。
- resolveEffectiveAgentModelSelection：显式覆盖优先于 Agent 定义；Agent 定义的 @default、* 及未配置 @task 以运行中父会话为准，不能冒充始终使用全局默认。
- config/model-settings.ts：modelRoles 原生默认是空 record，不存在每个角色都固定某款模型的静态默认。
- cli/models-cli.ts：models ls 只返回可用目录，不提供角色最终解析，并可能刷新网络目录。为预览不启动 CLI、加载插件或读取认证。

实现只做持久化配置/当前草稿的只读解析；可确定时展示具体 selector 和目录名称。不确定时说明原生选择依据，不推断登录状态，不产生补丁。Agent 的父会话仅显示新会话默认参考，不宣称探测了运行中会话。

## 修复与验证

- 模型选择框的继承项展示实际可解析的模型名称；下方显示完整 selector、角色来源链与思考设置。
- 预览跟随草稿；清除默认后立即回到明确的运行时状态，不保留旧模型，也不自动填充角色配置。
- Agent 定义与显式覆盖都展开角色引用；父会话只显示参考，新会话默认不冒充运行中模型。项目/插件定义未加载时明确说明。
- 长选项与 selector 使用 min-width:0 和换行，避免展开两列角色时被原生 select 的固有宽度撑开。
- 前端 4 个测试文件 52 项通过：22 项解析测试、15 项 OMP 设置交互、12 项 NativeCliTab、3 项服务测试。TypeScript、定向 ESLint 通过。
- 真实 React + mock IPC 的 Edge headless 检查 1440/1280/720 宽度无横向溢出，包含很长的模型名称；浅色/深色截图已人工查看。
- 浏览器核对继承名称、默认草稿变化、Agent 展开和保存载荷；一次默认模型保存仅提交 modelRoles.default，没有隐式角色写入。
- 浏览器证据：.trellis/.runtime/research/omp-inheritance-ui/evidence.json，5 张截图。该检查不等于安装后的用户验收。

- 额外按原生 isSessionInheritedAgentPattern 核对 Agent 定义：无后缀 @default/* 继承父会话；带思考后缀且 default 已配置时解析该角色模型，无已配置目标时才回退父会话。新增 4 项回归验证，后续打包使用更新后的源码快照。
