# Pi / OMP 原生管理与 AIO 网关接入设计

状态：方案已获用户确认并于 2026-09-26 进入实施。用户确认“首期两条路径都支持”和“独立 AIO 入口并存”。下文保留设计推导，最终接口、数据库与运行时约束以 [已实现契约](../../spec/aio-coding-hub/cross-layer/pi-omp-native-gateway-contract.md) 为准，实际验证另见 [实施验证记录](research/implementation-validation.md)。

## 1. 要解决的具体问题

同一个 Pi / OMP 用户可以同时配置 Anthropic、OpenAI Responses、OpenAI Chat 和 Google 模型。把客户端简单注册成一个固定协议，会出现同一 Pi 渠道上认证头正确但 SSE/用量解析错误的情况；把原生 provider 全部改写成代理地址，又会丢失直连配置与原生选择自由。

本方案保留三类不同对象：

1. **原生供应商档案**：用户加入 Pi / OMP 的显式节点，以原生文件成员关系表示“已加入 CLI”。
2. **AIO 网关上游**：现有 AIO Provider 和路由池，由 AIO 保存实际地址、凭证、配额与重试策略。
3. **AIO 网关入口**：AIO 生成到原生文件中的独立 provider，只包含本地网关地址、占位 key 和明确发布的模型能力。

用户在 CLI 中选择原生 provider 时直连；选择 AIO 入口时进入 AIO 的共同网关执行链。启停入口不修改原生默认项，也不宣称可以强制切换现有会话。

~~~text
原生配置页 ── Pi JSONC / OMP YAML 适配器 ── 原生 provider ── 原服务商
      │
      └─ 用户明确“添加到 AIO 网关” ── 预览并保存配置快照
                                      │
AIO 网关页 ── 既有 Provider / 路由 / 模型规则 / 凭证
                                      │
                             生成独立 AIO 原生入口
                                      │
Pi / OMP 选择 AIO 模型 ── 来源 + 明确协议 ── 共同 gate / retry / failover
                                      │
                             协议认证 / 转发 / SSE / usage
                                      │
                              真实上游 + 统一日志统计
~~~

## 2. 首期边界

### 2.1 必须交付

- Pi 和 OMP 两个独立产品入口，原生供应商/模型增删改查、显式节点同步、加入/移除和未加入档案保留。
- 两个客户端的 AIO 网关上游配置、协议分组的模型发布、独立入口配置与移除。
- 四种 HTTP/SSE 协议：anthropic-messages、openai-completions、openai-responses、google-generative-ai。
- 既有 AIO 路由顺序、同协议 failover、重试、熔断、请求日志、用量和计费，来源保持 pi / omp。
- 目录探测、自定义目标、OMP 默认/明确命名 profile、失败恢复和外部改动提示。
- CLI 安装状态/版本只读检测。首期不通过探测启动交互 CLI 或加载用户扩展。

### 2.2 保留原生使用，但不接入首期 AIO 网关

原生 OAuth/订阅登录、Bedrock/Vertex/Azure 专用认证协议、openai-codex-responses 等账户专用 API、Pi Messages、OMP pi-native、WebSocket、扩展自定义 API、自动发现和远程压缩副端点。原生文档保留这些字段；网关导入预览明确显示不兼容项，不静默降级后视为已适配。

这不把网关整体延期；首期四协议的完整网关闭环是发布硬条件。

### 2.3 不顺带扩展

MCP、Skills、Prompts、Session 管理、项目工作区写入、WSL、CLI 自动安装/升级、插件 SDK 新目标和跨协议翻译。不恢复旧实现，不重构无关的旧 CLI 产品行为。

## 3. 产品与页面

Pi / OMP 供应商区域共用现有布局，内部分为“原生配置”和“AIO 网关”两种清晰视图。

### 原生配置视图

- 顶部展示客户端和当前配置目标；路径、文件格式、OMP profile 在后端确认为有效后展示。
- 列表状态使用“已加入 Pi/OMP”“未加入”“配置状态未知”。不使用“当前使用”或“默认”。
- 唯一新增入口沿用现有供应商表单。常用字段包括标识、显示名称、接口格式、地址、凭证、请求头和模型；标识创建后锁定。
- 模型支持 id、name、input、contextWindow、maxTokens 和该客户端真实的思考字段；没有明确能力的数据不依据模型名自动补全。
- 原文编辑保留已存在的未知字段，结构化表单只 patch 自己编辑的字段。
- “移除”仅删原生节点并保存最新档案；“删除档案”明确说明是否同时移除原生节点，沿用一个确认框。
- “添加到 AIO 网关”产生可审查快照，列出协议分组、完整有效 URL、模型、凭证来源和不支持字段。只读列表刷新不更新网关上游。

### AIO 网关视图

- 使用现有 Provider 卡片、路由模式和上游启用开关；接口格式为必填且独立于客户端身份。
- 网关内有一个“AIO 入口”管理区；一个配置操作最多生成四个协议分组节点，而不是四个新产品 Tab。
- 仅为具有完整、兼容模型目录的协议生成入口；没有模型的协议不写空节点。
- 入口状态区分：未配置、已配置、配置被外部修改、配置待刷新；监听器状态另行显示。
- “最近收到 Pi/OMP 请求”基于后端观测，不能因写文件成功就显示“正在使用”。
- 配置完成展示在 CLI 的模型选择器中选择 AIO 模型的提示；不写 settings.json、config.yml 或默认模型。

## 4. 原生配置适配层

拟新增 domain/native_cli 与 infra/native_cli；前者定义操作语义与 DTO，后者分别实现 pi.rs / omp.rs 文件解析。应用服务负责 DB 与文件补偿，commands 只做参数和权限边界。

### 4.1 目标身份

NativeTarget 包含 client（pi/omp）、environment（首期 native）、显式选定的 profile、规范化 agent 目录、实际模型文件路径、路径来源。target_id 从稳定目标身份产生，作为 DB、查询键和写锁维度。

- Pi 默认 ~/.pi/agent/models.json；配置覆盖遵循经过确认的目录，不能根据工作区名称猜路径。
- OMP 默认 ~/.omp/agent/models.yml，正确处理 .yaml 后备和命名 profile。PI_CONFIG_DIR / PI_CODING_AGENT_DIR / profile / XDG 的组合由 OMP 适配器核验，不套 Pi 的拼路径规则。
- 首期不自动读取项目层、不执行 shell 取配置、不读取 .env。配置解析依赖的环境信息只来自明确显示的目标或 AIO 当前进程；不会声称它等同所有终端的环境。
- 显示 canonical 目标；symlink/reparse 解析与原子替换不能意外替换链接自身。不能确定真实写入目标时停在只读状态。
- OMP 旧 JSON 目标只读预览，提示用原生迁移获得稳定 YAML 后再管理；读取不会迁移。

### 4.2 文档与状态

NativeDocumentSnapshot = target + bytes_revision + parse_status + 显式节点集合。节点已配置状态只由当前文档的精确 key 决定；不存在文件与无法读取文件是不同状态。

NativeProviderProfile 保存完整显式节点和 AIO 展示元数据。刷新时同步所有非 AIO 托管的显式节点，包括内置同名 ID；原生删除只令档案变为未加入。AIO 节点按 manifest 的精确目标/key 识别，不能只凭名字前缀过滤用户节点。

原生档案库与网关 Provider 不自动双向同步：原生文件是直接配置的事实来源，AIO Provider 是网关实际请求的事实来源。明确复制而非隐藏同步，避免用户刷新页面就改变正在路由的请求。

### 4.3 写入与补偿

每个目标的操作在进程内共享一把锁；读取最新文件、检查 expected revision、只修改目标节点、验证目标客户端格式、原子替换、持久化档案。首次写入保存受限权限的完整原文备份。

- 新增遇到同名不同内容节点时明确冲突，不覆盖。
- 结构化更新使用最新节点加 changed-fields patch；编辑期间目标节点变化时让用户重新加载，非目标节点在最新文档中保留。
- 移除先保存最新节点；文件成功而 DB 失败时，只在文件仍符合本次写入结果时条件恢复。
- 非目标节点和未知字段语义保持；不保证格式/注释字节不变，原文备份可恢复。歧义 YAML 或无法安全编辑的语法只读。
- 读写上限、错误脱敏、私有权限复用 shared/fs 和现有敏感设置约定。
- revision + atomic rename 不构成对不合作外部进程的完整多文件事务。实施测试应覆盖可观察的竞争窗口；出现后写变化时报告冲突，不用整文件回滚覆盖新状态。

## 5. 数据模型与接口

以下为建议逻辑模型，实际 migration 编号在实施时取仓库下一版，不能预占固定编号。

| 对象 | 拟定存储 | 约束 |
| --- | --- | --- |
| 原生目标选择 | AppSettings 的独立 pi_omp_native_targets 字段 | 字段级 patch；不整份覆盖设置 |
| 原生保存档案 | native_cli_provider_profiles | profile_uuid；UNIQUE(client,target_id,native_key)；完整节点、显示元数据；不持久化伪当前模型 |
| 网关上游 | 现有 providers + nullable gateway_protocol | 新 pi/omp 上游必须指定四协议之一；老 CLI 的 null 保持既有行为 |
| 新客户端模型声明 | native_gateway_model_specs | provider_id 外键 + request_model_id；已验证的客户端模型能力文档；删除上游级联 |
| AIO 入口写入记录 | 新的节点级 manifest | target_id、协议、精确原生 key、已写节点 digest、源目录 revision、生成代数；不存原生登录凭证 |

现有 Codex provider_models / provider_model_catalogs 有专用校验和 SQL CHECK，首期不扩大其语义。native_gateway_model_specs 仅承载新客户端入口需要的声明，不新建另一套运行时路由器或模型价格库。

拟新增的命令分组：

- native_cli_targets_list / native_cli_target_validate；profile 参数必须枚举或由受控路径选择产生。
- native_cli_providers_list / native_cli_provider_read_for_edit / native_cli_provider_save / native_cli_provider_apply / native_cli_provider_remove / native_cli_provider_delete。
- native_cli_gateway_import_preview / native_cli_gateway_import_confirm：预览绑定配置 revision；确认时重新验证，不能使用过期预览覆盖新内容。
- native_cli_gateway_catalog_preview / native_cli_gateway_entries_apply / native_cli_gateway_entries_remove / native_cli_gateway_status。

列表 DTO 只传摘要、掩码和配置状态；完整原文仅在明确编辑操作时返回。列表、日志、预览错误不携带密钥。所有命令生成 Specta bindings，经 services 和 TanStack Query 暴露；query key 至少包含 client + target_id。IPC 不接受可任意读写的文件路径替代 target_id。

原生档案、入口 manifest 与 token/占位标记默认不进入现有 Provider share 导出。首期隐藏原生档案的通用分享动作；网关 Provider 的导入导出必须验证新增协议/模型字段可完整表达，否则明确拒绝，不能悄悄丢字段生成“可分享”配置。

## 6. 多协议网关

### 6.1 显式请求身份

引入 RequestProtocol / WireProtocol，和来源 client 分开：

~~~text
source_client = pi | omp
wire_protocol = anthropic_messages | openai_chat | openai_responses | google_generative_ai
~~~

拟定入口 /{client}/_protocol/{protocol}/{rest}。只能接受 pi/omp 和四种已支持协议；拒绝来源/协议/路径不匹配的请求。旧 /claude、/codex、/gemini、/grok 入口保留原行为。

| 原生 api | 本地 baseUrl 示例（仅路径） | 网关验证的请求后缀 |
| --- | --- | --- |
| anthropic-messages | /pi/_protocol/anthropic-messages | /v1/messages |
| openai-completions | /pi/_protocol/openai-completions/v1 | /chat/completions，经 SDK 后为 /v1/chat/completions |
| openai-responses | /pi/_protocol/openai-responses/v1 | /responses，经 SDK 后为 /v1/responses |
| google-generative-ai | /pi/_protocol/google-generative-ai/v1beta | /models/{id}:generateContent 或 :streamGenerateContent，完整路径含 /v1beta |

OMP 使用相同协议入口结构但 source_client=omp。URL 拼接以真实 CLI capture 为最终证据，实施 W0 必须确认这一表；不得在未实测时把路径猜测上线。

### 6.2 保留一条执行链

~~~text
解析来源/协议/路径 -> 标准化原始模型 -> 同客户端/同协议/支持模型的候选
 -> 既有 route mode 与共同 gate -> 每次尝试应用现有模型规则
 -> 清除客户端认证 -> 注入上游认证 -> URL/请求定稿 -> dispatch
 -> 按协议解析错误/SSE/usage -> 按 pi/omp 来源保存日志和计费
~~~

- 不将 ctx.cli_key 改成 codex/claude 以复用功能。必要的协议 helper 接收 WireProtocol，来源日志一直使用 pi/omp。
- 旧 CLI 的协议由既有入口/路径投影，Grok 的 Chat 与 Responses 继续区分，不把它固化成一种协议。
- 新上游一条记录对应一个有效协议；原生 provider 存在模型级 api/baseUrl/header 差异时，导入预览拆分为明确的有效上游分组，不能丢弃 override。
- 只在同协议、具有公开模型声明的候选之间 failover；不兼容排除不消耗真实请求尝试、不记熔断失败。
- 公开模型别名的上游映射只由既有 configured model routing 完成一次；新目录不再维护一份暗中的 mapping。
- 上游 API Key 由 AIO Provider 管理。Pi/OMP 原生 auth.json、agent.db、命令 key 和 shell 私有环境不作为网关凭证来源。
- 原生无认证/原生 OAuth 可以直连；首期未支持的网关认证模式明确阻止启用。已有 AIO OAuth / CX2CC 特殊桥接不因来源为 pi/omp 自动打开。
- 入口使用已有 CLI proxy 占位 key 机制，不宣称占位字符串是安全 token。沿用 AIO 当前监听/客户端访问策略；出站前清除占位和客户端认证，再注入真实上游认证。

### 6.3 必须一起改的协议面

认证、端点校验、模型提取与请求最终重写、成功/失败模型观测、推理参数观测、非流式/流式 usage、工具调用、终止帧、错误帧、提交前重试、取消、日志和计费都消费协议上下文。

客户端专用行为仍显式 opt-in：Codex 会话补全、托管 Profile、压缩/容量特殊规则、Claude warmup 和账单头修正不能只因 wire 协议相同就应用到 Pi/OMP。四协议通用终止语义与 Codex 专用策略必须分开。

## 7. 模型发布与独立 AIO 节点

### 7.1 目录来源

只使用用户在 AIO 明确保存或从原生快照明确导入的模型声明；不抓取并复制内置模型全目录，不凭模型名推断上下文/视觉/思考能力。

每个模型声明绑定同客户端、同协议、有效 AIO Provider。实际远端模型的转换仍由现有精确模型规则控制。缺少构造原生入口所需能力的模型留在“待补全”，不生成可选但实际不兼容的占位模型。

同协议下同 request_model_id 可以由多个上游提供。发布能力取各实际候选已声明能力的交集：容量取已验证数值的最小值，输入类型和支持的思考档位取交集。缺失或无法比较的能力不猜默认值；先补全或从该公开模型候选中排除。协议不同的同名模型始终分组。

路由、模型声明、上游禁用/删除或能力变化时，目录变为待应用。运行时候选还必须覆盖当前已发布的模型能力快照；能力更弱的新上游不能在目录待更新时直接加入旧模型池。发布更新重新验证候选集，避免配置列出模型但网关没有可用上游。旧会话仍可能缓存先前目录，超出当前可支持能力的请求需明确失败，不能静默截断或转发给不兼容上游。

### 7.2 生成内容

建议原生 key 以 aio-coding-hub-加协议标识生成，最多四个；名称冲突时要求选择不同 key，不覆盖用户已有节点。精确 key 持久化在 manifest，后续不按前缀猜所有权。

生成器使用字段白名单：本地 baseUrl、占位 key、原生 api、模型 ID/名称和本客户端可表达的已确认能力。不能整段复制原始 provider/model JSON：上游 apiKey、headers、baseUrl、transport、discovery、remoteCompaction 等可能泄密或绕过网关的字段不得进入托管节点。

Pi thinkingLevelMap 和 OMP thinking 分别投影；无法保持语义的能力在入口中不可宣称支持。首期只生成 HTTP/SSE，OMP preferWebsockets 不启用。

### 7.3 应用与移除

- 应用先确认网关监听就绪、同协议上游与目录可用，再生成预览并原子 patch 对应节点。
- 文件、DB、manifest 不能用一笔事务覆盖；应用服务用生成代数记录阶段，并对本次仍拥有的节点做补偿。失败后返回可恢复的实际状态。
- 重复应用是幂等的；网关端口改变时只刷新仍与已写 digest 一致的节点。
- 用户外部编辑托管节点后，显示“已修改”；不自动覆盖、不自动删除，需明确重新应用或解除托管。
- 移除只删除 manifest 指定且内容仍匹配的节点；原生 provider、登录文件、默认项与网关上游档案继续保留。
- 手工选择 AIO 后保存为 CLI 默认是用户自己的操作；AIO 移除入口时可提示引用可能失效，但不修改该默认值。

## 8. 能力矩阵与渐进发布

| 能力 | Pi / OMP 首期 |
| --- | --- |
| nativeProvider（新增） | 开启 |
| provider / gateway | 以四协议完整验收为开放条件 |
| cliProxy | 关闭；使用独立 AIO 节点发布，不进入旧整文件代理恢复生命周期 |
| logs / usage / pricing | 开启；来源为 pi/omp，计量协议独立记录 |
| cliManager | 只读安装/版本/目录状态 |
| mcp / skills / prompts / workspaces | 关闭 |
| wsl / managedUpdate / providerPluginTarget | 关闭 |

前后端能力表、路由验证、数据库/IPC枚举、页面筛选和测试 fixture 同步更新。实现过程中可以用内部开关验证，但对外首版必须同时满足原生管理和网关闭环，不发布只有标签没有能力的渠道。

## 9. 选择与取舍

- 不重用旧方案：用户已弃用，当前源码契约也与简化的 Pi 家族同构假设不同。
- 不覆盖原生 provider 实现接管：用户已选择独立入口，且原生登录优先级/默认值会制造不可控状态。
- 不自动将原生修改同步成运行时网关配置：原生文件与 AIO 上游池的责任不同，明确快照导入避免隐式流量变化。
- 不做跨协议 failover：原生请求的消息/工具/思考语义不同，本任务先把四种协议各自完整接通。
- 不重做 Codex 模型库：现有库包含特定 Profile 规则与 schema 约束，新增最小元数据比破坏已运行的契约更容易验证。
- 不读取原生 OAuth：认证可以改写端点/模型和刷新凭证，把它当普通 key 导入会跨越本任务的责任边界。

## 10. 风险与证据门

W0 的真实 CLI × 四协议 mock capture 是开始大范围接线前的第一道技术门。若发现路径、解析器、终止语义、模型字段或 OMP profile 与本设计不同，更新调研、PRD 和设计后继续；不能默默砍掉已经确认的双路径范围。

原生文件备份、节点 digest、状态未知只读、非目标字段保留、路由协议不混用、源/协议分别记账、失败条件补偿，以及四个现有 CLI 的回归共同构成发布门。完整实施与验证步骤见 [实施计划](implement.md)。
