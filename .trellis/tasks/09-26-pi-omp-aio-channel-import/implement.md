# 实施与验证清单

状态：2026-09-26 用户已批准并启动任务。代码与隔离验证完成，Windows 测试包已生成；用户体验验收待反馈。证据见 research/validation.md。

## 依赖与入口

- 前置：09-26-omp-pi-channel-integration 的当前已验证代码；其待用户体验验收状态保留，不自动归档。
- 阅读本任务 prd.md、design.md、research/code-evidence.md、research/gemini-antigravity-support.md。
- 遵循 Pi/OMP 原生网关、配置模型路由、failover、OAuth、供应商删除/身份、设置所有权及 usage contracts。
- 当前 codex.dispatch_mode=inline；实现前读 trellis-before-dev。工作分支/基础版本在启动时确定，本任务创建阶段不创建 worktree 或更换分支。

## 阶段 1：纵向验证与能力矩阵

- [x] 以 2026-09-26 核查为基线，在开始实现时复核官方支持公告；分别记录标准 API、企业 OAuth、退役个人 OAuth、未知资格、Antigravity 未适配状态，不根据 CLI 名称或 token 存在推断支持。
- [x] 建立 consumer/source/protocol/binding 的所有权边界及影响清单；定位公共门禁、会话、认证、插件与统计的读取位置。
- [x] 用最小受控绑定接入现有 Codex/Claude 来源池，验证单条请求路径、真实调用方身份和源 OAuth 适配不互相覆盖。
- [x] 补齐 Grok 两协议与 Gemini 标准 API，逐项列出 API Key / 仍受支持 OAuth 通路、流式/非流式、工具与推理支持。企业 OAuth 的产品许可证据和 AIO 适配证据分列，未验证不放行。
- [x] 如需缩小已审核范围或扩大为新协议转换/登录实现，提交证据重新审核后再继续。

## 阶段 2：绑定与配置写入

- [x] 增加增量 schema 与渠道绑定、发布快照/意图/所有权数据，旧 manifest 保持兼容。
- [x] 来源目录/能力补齐及 preview/confirm/update/withdraw 服务，复用原生目标锁、CAS、备份和恢复。
- [x] 实现 target + source + protocol 幂等、多选同目标原子发布、精确撤回、端口/源变化提示。
- [x] 保证托管节点不被普通档案写入覆盖；覆盖 portable/share/import 不能表达新增数据时的明确拒绝与保留。

## 阶段 3：公共路由与可观测性

- [x] 受控路由读取绑定，限定推理端点、来源和模型，清理来路凭证并加载来源认证。
- [x] 来源当前工作模式与顺序 → 模型/协议/能力过滤 → 公共门禁/重试/发送；不做本地 HTTP 再入，不复制供应商。
- [x] Gemini 认证资格准入在预览、确认和请求选源一致生效；个人退役、企业许可、未知资格及混合池分开处理，资格变化不能被旧发布快照绕过。
- [x] 会话按真实调用方/来源/绑定隔离；源供应商健康、额度和费用仍共享同一身份。
- [x] 日志/事件/历史读取新增可选来源信息，客户端仍是 Pi/OMP；统计、定价、流式/非流式与取消路径不重复记账。
- [x] 回归旧渠道直连、自有原生网关及不带绑定的旧请求路径。

## 阶段 4：交互整合

- [x] 两个视图复用“从 AIO 渠道接入”弹窗，来源卡片、多选、显式模型、能力缺失原因和写入预览。
- [x] 已接入卡片提供来源跳转、更新、撤回和状态；区分正反导入方向、配置存在与流量事实。
- [x] Gemini 显示具体认证阻塞原因和官方迁移说明，未知许可标为待核实；不把整类 Gemini 标成退役，不添加假的 Antigravity 来源或自动迁移按钮。
- [x] 目标切换、返回/前进、加载/失败/空态、只读、长路径、同名节点和重复点击回归。
- [x] 明暗主题、窄窗口和键盘交互使用现有组件风格；真实组件截图检查，不读取真实用户凭证。

## 阶段 5：验证与交付

### 用户验收追加：自动获取与下拉表单

- [x] 复用只读发现链路，保留明确能力元数据并合并来源已配置能力。
- [x] 模型选择自动填写；手动/清空/JSON 优先，目标与来源身份隔离。
- [x] Pi/OMP 思考规格使用下拉表单，OMP 模式与后端验证保持一致。
- [x] 模型或思考路由改写要求能力核对，避免误用原模型元数据。
- [x] 真实组件浏览器验证与明暗、窄屏、键盘检查。
- [x] 本轮全量检查收尾、新测试包构建及产物校验。
- [ ] 用户体验验收（新包已交付）。

详见 `research/automatic-model-capabilities.md`。

- [x] 单测与集成覆盖 PRD A1–A9，并在验证记录建立需求到证据映射。
- [x] 最小真实 CLI 矩阵：Pi/OMP × Claude Messages、Codex Responses、Grok Chat、Grok Responses、Gemini Generative AI 标准 API（10 条基础通路）；认证模式及流式/非流式另列，不用基础计数代替认证或账户资格验证。
- [x] 覆盖 Gemini 标准 API、OAuth 资格未知/企业名/个人名统一阻塞、混合池过滤、资格变化及 Antigravity 未适配状态；企业有效许可路径仍缺证据，明确记为未验证，不放行。Pi 配置不依赖已移除内置 provider。
- [x] 真实 CLI 使用隔离目录、本地 mock 和可控认证 fixture；真实账号/付费调用需另行明确授权，不为任务测试读取既有用户凭证。
- [x] 测试来源切换/删除/凭证轮换、失败接力、能力漂移、源会话隔离、native 文件外部改动、写入失败恢复和撤回。
- [x] 运行 typecheck、lint、unit tests、fmt、Rust tests/clippy、bindings 漂移、spec links 和 production build；按变更实际范围选择现有命令。
- [x] 更新 docs/pi-omp.md 与跨层契约，记录兼容矩阵及未验证组合；清楚区分当前证据和未来支持。
- [x] 验证记录分别标记 mock 协议通过、官方产品支持和真实账户验证；无真实许可/账号证据时，企业 OAuth 保持未验证，不将任务测试通过描述为账号已经可用。
- [x] 生成新 Windows 测试包及源码指纹，交用户体验验收。不自动提交、推送或发布。

## 建议命令

前端：pnpm typecheck；pnpm lint；pnpm test:unit；pnpm build。

后端：pnpm tauri:fmt；pnpm tauri:check；pnpm tauri:test；pnpm tauri:clippy。

契约：pnpm tauri:gen-types；pnpm check:generated-bindings；pnpm check:spec-links。

最终命令和结果须写入本任务 research/ 的实际验证记录；此清单不代表已运行或已通过。

## 用户体验跟进：批量添加和完整默认值

- [x] 来源模型搜索、多选、全选结果、清空、批量追加去重。
- [x] 可追溯模型目录及完整参数默认值；手动修改优先。
- [x] 批量流程、隔离、冲突与字段保留测试。
- [x] 真实 UI 检查、重新构建测试包与结果记录。

## 用户体验跟进：聚合网关入口说明

- [x] 核实渠道入口未固定单个上游；Pi/OMP 通过同一 AIO 入口使用渠道当前候选池。
- [x] 导入页展示聚合请求路径；候选折叠区改为 AIO 调度池，并说明模型资料及动态资格筛选。
- [x] 前端与后端回归、真实 CLI 本地失败接力矩阵、明暗与窄屏截图检查。
- [x] 按用户要求重新打包，生成 pi-omp-20260926-235813；MSI/ZIP、资源、文件哈希与源码一致性校验通过。详见 research/aggregate-gateway-clarification.md。

## 用户体验跟进：OMP 版本未知与更新功能核查

- [x] 修复 Windows 独立 OMP 无 npm 元数据时的版本读取，并补充官方安装目录扫描。
- [x] 使用隔离的非交互版本查询，验证不会继承日常配置与凭证环境；保持 Pi 元数据读取路径。
- [x] 相关 Rust 9 项、实机 OMP 版本查询 1 项、前端 12 项通过；TypeScript、ESLint、格式与 Clippy all-targets 检查通过。
- 安装/更新功能现状为命令复制与官方发布页，没有自动检查新版本或一键升级。本轮版本读取修复尚未打包，详情见 research/cli-version-management.md。

## 用户体验跟进：自动检查与主动确认升级

- [x] 用户批准新增版本检查、下载安装和一键升级，禁止自动升级。
- [x] 完成官方源核查、安装方式识别、短期一次性计划、受控执行与校验。
- [x] 接入原生 CLI 版本卡片，自动检查/手动刷新、确认/取消、失败反馈和状态刷新。
- [x] 前端 36 项相关测试及 Windows 隔离目录真实 Pi/OMP 安装通过。
- [x] 最终 Rust/Clippy/生成绑定/规范检查及验证记录收尾，见 research/cli-updates-validation.md。
- 本轮功能尚未进入已交付的 pi-omp-20260926-235813 测试包，详见 research/cli-updates-plan.md。

## 后续 MSI 交付

已按用户要求构建 pi-omp-cli-update-20260927-011000 MSI，源码指纹、内部 EXE 与资源哈希验证通过，等待用户安装测试；详见 research/cli-updates-msi.md（research 目录内文件可直接查看 cli-updates-msi.md）。
