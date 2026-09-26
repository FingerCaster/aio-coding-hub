# OMP CLI 设置实施记录

## 依据

- OMP 本地参考源码：D:/UGit/oh-my-pi-source-reference，commit 7853b4e499936f9dcc13c9b64adb55f6b342aabf，版本 18.3.2。
- 官方源码仓库：`https://github.com/can1357/oh-my-pi`。
- 核对 docs/settings.md、docs/task-agent-discovery.md、src/config/model-settings.ts、src/config/model-roles.ts、src/config/service-tier.ts、src/task/settings.ts、src/task/agents.ts、src/discovery/helpers.ts、src/task/executor.ts 与 catalog/effort.ts。
- 默认模型使用 modelRoles.default；默认思考等级没有 off 选项（注册值为 minimal/low/medium/high/xhigh/max/auto）。Agent/具体模型选择器可以保留原生思考后缀。
- 用户 Agent 位于所选 agentDir/agents/*.md；自定义优先于同名内置定义。项目和插件发现不在本次执行范围。

## 实现

新增 4 个生成式 IPC：omp_settings_read、omp_settings_save、omp_agent_read、omp_agent_save。

OMP CLI 管理新增三类表单：模型/会话、子任务行为、Agent。25 个 scalar 设置、modelRoles 成员与 Agent 模型/服务等级/prewalk/advisor/禁用列表均可管理。模型列表读取当前 models 文件内的原生与 AIO 节点，并补充版本化内置目录；默认选择保留继承语义，既有未知模型和回退列表不自动改写。自定义 Agent 通过原生 Markdown 编辑器创建/修改，支持工具、派生列表和系统提示词。

写入绑定所选 target 与路径/字节修订，使用既有目标锁、私有原文备份和原子替换。默认/提供商/网关入口的写入权限保持分开。settings 只回传白名单字段；Agent 原文仅显式编辑读取、不缓存、不记录参数日志。配置保存保留未知字段，Agent 保存保留完整原文。外部变化保留草稿并报冲突。

## 已通过验证

| 验证 | 结果 |
| --- | --- |
| 前端定向套件 | 5 文件、42 项通过，包含旧 Pi/OMP 版本管理回归 |
| Rust 原生配置套件 | 32 项通过、1 项默认忽略；覆盖设置、模型文档、路径、原子写入/备份及 Windows ACL |
| 显式真实 OMP 配置读回 | 1 项通过；AIO 实际保存后由 OMP 读取 6 组关键值 |
| 原生 schema 核对 | 25 个字段及默认值从源码提取，与隔离 OMP config list 的默认值核对，差异 0；见 native-schema-check.json |
| TypeScript / ESLint | 通过 |
| 前端生产构建 | pnpm build 通过；现有 Browserslist 数据与大 chunk 提示不阻断构建 |
| 文档链接、任务上下文、git diff --check | 通过；inline 模式没有 sub-agent jsonl |
| 浏览器视觉/交互 | 真实 React CLI 页面 + 模拟 IPC，6 张截图；浅色、深色、720px 窄屏；无横向溢出或页面异常；核对保存载荷 |

Rust Clippy all-targets（-D warnings）、Rust fmt 与生成绑定漂移检查均已通过。任务状态为 ready_for_user_acceptance；所有本次开发与本地验证项已完成。

## 隔离和范围

真实 OMP 检查仅使用临时 HOME/USERPROFILE/AppData/XDG/agent/cwd，清空继承环境；未更新或改写用户真实 OMP/Pi 配置、认证、供应商或 CLI 安装。未启动需要模型请求的 Agent 会话。自定义 Agent 文件由源码对照和本地读写测试验证，未对外发送提示词。

浏览器证据：.trellis/.runtime/research/omp-settings-ui/。视觉检查采用真实组件的 mock IPC，不等价于安装包内 WebView 的人工验收。复用已有 Edge，无需安装 Chrome。

开发验证阶段未生成 MSI。随后按用户要求完成本地 MSI 打包，详见 msi-delivery.md；仍未 commit/push/release。保留前序集成改动，等待用户安装验收。
