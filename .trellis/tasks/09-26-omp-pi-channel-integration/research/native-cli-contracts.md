# Pi 与 OMP 原生契约

基线见 [来源清单](source-baselines.md)。本文件是源码核验结果；真实 CLI 的网络行为仍须按实施计划验收。

## 两个客户端分别适配

| 项目 | Pi 0.87.1 | OMP 18.3.2 |
| --- | --- | --- |
| 默认模型配置 | ~/.pi/agent/models.json | ~/.omp/agent/models.yml，存在 .yaml 时有后备读取规则 |
| 解析 | JSON + 注释/BOM 处理；不是任意 JSON5 | YAML；ConfigFile 还处理旧 JSON 向 YAML 迁移 |
| 目录覆盖 | PI_CODING_AGENT_DIR | 同名 PI_CODING_AGENT_DIR；另有 PI_CONFIG_DIR、命名 profile 和 XDG 规则 |
| 原生默认项/设置 | settings.json | config.yml/config.yaml 及分层设置，modelRoles 具有项目语义 |
| 登录存储 | auth.json | SqliteAuthCredentialStore；默认经 getAgentDbPath 定位 agent.db |
| 供应商身份 | providers 的精确 key | providers 的精确 key |
| 基础请求字段 | api、baseUrl、apiKey、headers，模型可覆盖 api/baseUrl | 相似基础字段，另有 auth、discovery、transport 等独立语义 |
| 思考配置 | reasoning 与 thinkingLevelMap | reasoning 与 thinking 对象（mode、efforts 等） |
| 额外原生传输 | 可扩展 api，包括 pi-messages 等 | transport: pi-native 使用 OMP 自己的 /v1/pi/stream |

## Pi 证据

- packages/coding-agent/src/config.ts:504、528、542、547、552：配置目录、模型、认证和设置路径。
- packages/coding-agent/src/core/model-config.ts:188、229、272、297：模型/供应商 schema；有界业务读入边界由 AIO 自行负责，Pi 原生解析为 JSON.parse(stripJsonComments(stripBom(content)))。
- packages/coding-agent/src/core/provider-composer.ts：合成内置、显式配置、扩展模型；模型级 api/baseUrl 会参与有效配置，不能只看供应商级 api。
- packages/coding-agent/src/core/resolve-config-value.ts：支持环境插值与命令表达式。AIO 只把表达式当作原生文本保存，不执行、不用字符串外形推断真实 secret。
- packages/ai/src/api/openai-completions.ts:789、openai-responses.ts:276、anthropic-messages.ts:923：SDK 的 baseURL 来自模型。
- packages/ai/src/api/google-generative-ai.ts:354：Google 的自定义 baseUrl 已包含版本路径，设置 apiVersion 为空，不能自动再拼一层 v1beta。
- packages/ai/src/types.ts:17：存在多种 API；“Pi”不是单一网络协议。

## OMP 证据

- packages/utils/src/dirs.ts:27、30、82、298、439、582、866：默认目录、配置文件名、profile 优先级、agent 路径和 agent.db 定位。
- packages/coding-agent/src/config/config-file.ts:29、140、170、185：.yml 优先、.yaml 后备、旧 .json 的迁移语义。AIO 的只读探测不得触发 OMP 的自动迁移。
- packages/coding-agent/src/config/models-config.ts:37：自定义模型需要有效 api/baseUrl；apiKey 的要求受 auth: none / oauth 影响，不能套用 Pi 的必填规则。
- packages/coding-agent/src/config/models-config-schema-bundle.ts:107、190、315：协议集合、模型能力、auth / discovery / transport 字段；尤其不能把 Pi 的 thinkingLevelMap 直接写成 OMP 的思考配置。
- packages/coding-agent/src/config/model-registry.ts:413、433：常规本地配置加载 models.yml；部分 OMP 原生 Gateway 模式会忽略本地 models.yml。
- 同 schema 文件:348：transport: pi-native 定向到 OMP auth-gateway 的 POST /v1/pi/stream；它不是 AIO HTTP 代理入口。
- packages/ai/src/auth-storage.ts:252、packages/ai/src/auth/sqlite-credential-store.ts:513：SQLite 登录存储。AIO 不打开用户的 agent.db 来导入登录凭证。

## 对本任务的约束

1. 原生管理适配器分别实现 Pi JSONC 和 OMP YAML，产品服务共享操作语义。
2. 首期的 AIO 网关支持四种明确协议：Anthropic Messages、OpenAI Chat Completions、OpenAI Responses、Google Generative AI。Azure、Bedrock、Vertex、原生订阅/OAuth 专用 API、Pi Messages、OMP pi-native、任意扩展 API 仍可留在原生配置中，但不能伪装成已支持的 AIO 上游。
3. HTTP/SSE 是首期网关传输。生成的 AIO 模型不得启用 OMP preferWebsockets、pi-native、discovery 或未经支持的远程压缩端点。不能把带这些配置的原生模型静默转换后声称等价；导入预览要列出未纳入网关的能力。
4. 原生配置读取不得顺带读 auth.json、agent.db、.env 或执行命令。首期网关上游要求在 AIO 明确配置的 API Key；原生环境/命令凭证可保持直连，但不能在网关未解析时被当成密钥发出。
5. Pi 和 OMP 共用环境变量名不代表共用目录。目标绑定包含客户端、环境和规范化目录；两者误指向同一配置目标时阻止写入并提示修正。
6. OMP 默认 profile 和明确选择的命名 profile 使用各自正确目录。首期只管理已选全局目标，项目配置层保持原生管理；自定义目录明确显示，不能猜 shell 的 cwd 或活动 profile。
7. 对仅有 OMP legacy models.json 的目标，先提供只读预览并提示完成 OMP 原生迁移后再写入；不悄悄创建优先级更高的新文件。若 .yml 与 .yaml 同时存在，显示原生实际使用文件和被遮蔽文件。
8. 原生未知字段和非目标节点语义保留；完整原文件备份承担格式/注释恢复。歧义 YAML（重复 key、目标关联的锚点/合并键等）在没有可靠编辑能力前进入原文只读状态，避免重排配置改变语义。

## 首轮验证必须证明的事实

- 每种 CLI × 四协议：实际 URL/方法、鉴权载体、模型名、工具和思考字段、请求取消、正常 SSE 结束和错误 SSE。
- Google 版本路径、Anthropic /v1、OpenAI /v1 的组合不能出现双重版本路径。
- 原生同名内置 provider、同 ID 不同协议模型、环境/命令密钥、模型级 baseUrl/api 覆盖的导入边界。
- 修改配置后模型列表何时刷新；不能以保存成功推断活动会话已切换。
- 使用隔离 HOME/profile、无真实密钥的本地 mock 上游；不借助用户登录文件让测试“碰巧成功”。
