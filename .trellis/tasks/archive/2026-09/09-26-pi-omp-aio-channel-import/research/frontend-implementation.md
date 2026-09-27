# Pi / OMP 统一渠道接入：前端实现与验收

日期：2026-09-26。AGY 完成初版页面与测试，主协调者接手最终校验、竞态修复与真实浏览器验收。

## 实现范围

- 两个消费端的“原生配置”和“AIO 网关”视图复用“从 AIO 渠道接入”弹窗。
- Claude Code、Codex、Grok Chat、Grok Responses、Gemini 按来源和协议列出；认证阻塞原因与可用候选数分别展示。
- 选择来源与显式模型后先预览目标文件和节点，再确认写入。更新保留已有发布模型；省略其他来源不会撤回；撤回使用准确的 bindingId。
- 托管卡片支持来源跳转、更新、撤回，区分已接入、待恢复、外改和待刷新。
- 能力表单与高级 JSON 共用验证：容量不预填，工具和推理能力初始未选择；Pi 必须明确支持工具，OMP 必须明确 true/false。候选按钮只填模型 ID/名称。思考规格模板仅供编辑，用户须按模型文档确认。
- 目标路径代替不透明 ID；切换目标清理旧预览、加载状态和能力弹窗。旧目标异步结果不能覆盖或关闭新目标流程。
- 通过生成的五个 IPC 命令调用后端；providerId、UUID、targetId 与版本都保留校验。取消发布不写原生文件，能力保存不修改认证。
- 复用共享 Select 保证暗色对比度；共享 Dialog 关闭按钮保持单行，避免窄确认弹窗挤压。

主要文件：src/services/nativeChannels.ts、src/query/nativeChannels.ts、src/pages/providers/native/NativeChannelImportDialog.tsx、NativeChannelModelsDialog.tsx、NativeProvidersPanel.tsx、NativeGatewayEntries.tsx。

## 测试证据

- 全量前端：326 个测试文件、3012 项通过。日志：.trellis/.runtime/native-channel-frontend-all.log。
- 最终相关范围：11 个文件、58 项通过，包括13项渠道流程、5项 service 契约与2项路由标记。日志：.trellis/.runtime/native-channel-frontend-targeted.log。
- TypeScript、ESLint、production build 通过，日志分别为 native-channel-typecheck.log、native-channel-lint-final.log、native-channel-production-build.log（均在 .trellis/.runtime 下）。
- 覆盖多选预览/确认、取消不写、监听未启动、能力缺失、Gemini 阻塞、模型保留、精确撤回、待恢复卡片、外部版本冲突、已打开与在途预览的目标切换、工具/推理必填及小数容量拒绝。
- 全量测试在最终两项纯样式修正前运行，之后重跑相关58项、lint与构建；不将旧任务测试计数作为本轮结果。

## 真实浏览器截图

主协调者通过 Playwright 驱动本机 Edge，加载真实 React 页面、真实组件和 CSS，IPC 使用隔离 fixture。不是安装后桌面端真实账号验收。亮色/暗色、1280与720宽度、模型声明、Gemini阻塞和发布预览均已目视检查，browser-evidence.json 的 pageerror 为空。

目录：.trellis/.runtime/research/omp-pi/channel-ui/

| 文件 | 场景 |
| --- | --- |
| pi-providers-light.png | 原生供应商和 AIO 托管卡片 |
| pi-import-light.png | Pi 亮色统一渠道列表 |
| omp-import-dark.png | OMP 暗色统一渠道列表 |
| omp-import-narrow.png | OMP 720宽度窗口 |
| pi-gemini-eligibility.png | Gemini API与阻塞 OAuth 说明 |
| pi-publish-preview.png | 目标路径、节点与模型的写入前预览 |
| omp-model-capabilities.png | 未预填容量、未预选工具/推理的能力表单 |
| omp-model-capabilities-narrow.png | 能力表单窄窗口 |
| browser-evidence.json | 场景文本与浏览器错误记录 |

Gemini 截图中的企业/个人身份来自 UI fixture，仅验证提示展示；当前生产后端没有许可分级证据，因此所有 Gemini OAuth 都按资格待核实阻塞，不能据截图宣称已自动识别用户套餐。

AGY 曾交付三张生成图片并误称截图，主协调者未采信，已移除 research/screenshots 下对应三张 JPG。此处仅保留真实浏览器渲染的 PNG 作为视觉证据。AGY 初稿中55项计数和 Object.hasOwn 未修记录均被当前复测结果替代。

## 协作结束

AGY task_e04f2df965ad / ctx_0a928211ffec 已 worker_done、ack 并 worker-release。最终改动由主协调者验证，没有遗留协作终端。无提交、推送或发布。
