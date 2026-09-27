# Gemini / Antigravity 支持边界核查

核查日期：2026-09-26。用途：修订 Pi/OMP 接入 AIO 统一渠道任务，提交用户审核。范围为公开官方资料和本地源码，不代表真实账户或新网关链路已验收。

## 结论与任务影响

用户所说“改成反重力”适用于个人 Code Assist 的旧 Gemini CLI 服务：Google 将个人免费和 Google AI Pro/Ultra 用户迁往 Antigravity。它不等于 Gemini 模型/API 整体退役，也不等于 Pi 将 Gemini provider 改名为 Antigravity。

本任务加入 Gemini 来源，通过 Pi/OMP 均具备的标准 google-generative-ai 协议访问 AIO。保留现有来源池为唯一上游和凭证事实来源；排除已退役个人 OAuth，企业 OAuth 需有资格和适配验证。Antigravity 需要 AIO 独立适配，不能伪装成现成来源；推荐作为后续候选交用户审核。

## 1. Google 官方时间线与范围

Google Developers Blog 于 2026-05-19 发布迁移公告，明确 2026-06-18 停止通过旧 Gemini CLI/Code Assist IDE 服务向个人免费 Code Assist、Google AI Pro 和 Ultra 提供请求处理，引导迁往 Antigravity。

同一公告明确保留 Code Assist Standard/Enterprise 许可客户的访问，并继续支持通过付费 Gemini 和 Gemini Enterprise Agent Platform API Key 使用 Gemini CLI。因此不能写成“所有 Gemini CLI 均不支持”，也不能把这段个人订阅迁移公告扩展为所有免费 Gemini API 配额失效。

认证文档页顶部也提示上述个人用户迁移。其正文及仓库 README 仍留有个人 Google 登录说明；发生口径差异时以有明确日期、对象和例外的官方迁移公告为支持范围依据，不以仍能下载安装或 README 中存在登录步骤证明服务资格。

公开仓库 API 在本次核查时显示未归档；GitHub 最新稳定发布和 npm latest 均为 0.61.0，发布于 2026-09-23。这只说明项目仍维护，不证明被公告排除的个人访问仍可用。

## 2. Pi / OMP 固定版本源码

| 客户端 | 检查基线 | 源码事实 | 对 AIO 接入的含义 |
| --- | --- | --- | --- |
| Pi | v0.87.1，提交 f07218c，本地 D:/UGit/pi-source-reference | packages/ai/CHANGELOG.md:964–968 的 0.71.0（2026-04-30）记录同时移除 Gemini CLI 和 Antigravity 的内置 provider、OAuth、模型元数据和导出；providers/google.ts:6–13 仍有 google-generative-ai、GEMINI_API_KEY 与标准 Google API 地址。 | 接 AIO 标准 Google 协议；不依赖恢复 google-gemini-cli / google-antigravity 内置项。 |
| OMP | v18.3.2，提交 7853b4e，本地 D:/UGit/oh-my-pi-source-reference | packages/ai/src/registry/hooks/oauth-code.ts:12–13 仍加载 google-gemini-cli 与 google-antigravity OAuth hook；两个 OAuth 实现分别存在。 | 原生代码可用于后续研究，但不能证明旧个人服务仍接受请求，也不能直接替代 AIO 上游适配。 |

两种产品的 OAuth 实现不能按显示名互换。OMP 的 google-antigravity.ts 说明其凭证不同于 google-gemini-cli；本任务不复制或重新利用用户令牌来跨产品迁移。

## 3. 当前 AIO 代码事实

基线：当前工作区，HEAD 3372d11f5a20a212bd551e0b0b7067ab2b639c79，含前置 Pi/OMP 未提交实现。

- src/constants/clis.ts 保留 Gemini 渠道。
- src-tauri/src/gateway/oauth/registry.rs:26 注册 GeminiOAuthProvider；adapters/gemini.rs:75–81 使用 gemini / gemini_oauth 身份。
- 在 src、src-tauri/src、docs 中检索 antigravity / 反重力，未发现独立渠道或适配。这里只作指定应用路径的检查结论，不声称扫描了所有参考仓库或本机工具。
- 本机 agy 可运行、OMP 保留原生反重力实现，均不能作为 AIO 已支持 Antigravity 的证据。

## 4. 修订后的验收边界

| 路径 | 首期处理 | 尚需验证 |
| --- | --- | --- |
| AIO Gemini 标准 API 来源 → Pi/OMP | 纳入基础矩阵，独立 AIO · Gemini 节点。 | 模型/能力、鉴权、工具、流式、重试与单次统计；上游自身仍须有效。 |
| AIO 企业 Gemini OAuth → Pi/OMP | 有条件保留，不能根据 key/token 或邮箱猜套餐。 | 合法许可元数据及现有 AIO 适配的实际兼容；无证据时显示待核实/未验证。 |
| 已退役个人 Gemini OAuth | 新绑定过滤，展示具体迁移提示。 | 拒绝、混合池过滤和资格变化回归；不以成功登录或刷新作为可用结论。 |
| Antigravity 受 AIO 管控来源 | 当前不可接入，单列后续适配候选。 | 独立认证、上游协议、配额、凭证所有权、许可边界和实际兼容研究。 |

新绑定准入不改写全局 Gemini 登录、现有账号或用户默认模型。官方“企业产品仍支持”与“AIO 已经兼容企业路径”分开陈述。mock 只验证协议/路由，不能替代真实许可或在线账号证据。

## 5. 可追溯来源

| ID | 来源 | 用途 |
| --- | --- | --- |
| G1 | https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/ | 官方公告：发布日期、迁移时间、个人范围、企业与付费 API 例外。 |
| G2 | https://geminicli.com/docs/get-started/authentication/ | 官方认证页迁移 banner；正文与 README 的差异不能作为恢复个人服务的依据。 |
| G3 | https://api.github.com/repos/google-gemini/gemini-cli 及其 /releases/latest | 仓库状态与 v0.61.0 发布时间。 |
| G4 | https://registry.npmjs.org/@google%2fgemini-cli/latest | 包版本交叉核对，不用安装成功证明账户可用。 |
| P1 | https://raw.githubusercontent.com/earendil-works/pi/v0.87.1/packages/ai/CHANGELOG.md | 固定版本内置 provider 移除记录。 |
| P2 | https://raw.githubusercontent.com/earendil-works/pi/v0.87.1/packages/ai/src/providers/google.ts | 固定版本标准 Google API provider。 |

本地原始响应位于 .local/gemini-support-evidence/；summary.json 记录各 URL、HTTP 状态、抓取时间和 SHA256。官方博客另外保存为 transition-blog.txt，SHA256 为 a95786737d70654c94d7a3be3fa2478525e2a73aeb2d95c4d797bc723c612435。本报告保留结论及来源，不能依赖被 git 忽略的 .local 文件才能理解任务。

本次未调用任何真实模型、账号或计费接口，未读取/修改用户凭证，也未开始实现新渠道绑定。
