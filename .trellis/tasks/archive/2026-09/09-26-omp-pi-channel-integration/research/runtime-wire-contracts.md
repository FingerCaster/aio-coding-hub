# W0：固定版本 Pi / OMP 真实 CLI 协议验证

验证日期：2026-09-26（Windows x64，Node v24.14.1，npm 11.12.1，隔离 Bun 1.3.14）。

## 结论与证据范围

已通过真正发布包 CLI 入口完成两 CLI × 四协议的 8 条基本请求；工具、思考、图片、错误、取消、重试与模型覆盖合计 80 项；另有 4 项 Anthropic auth 分支、23 项配置解析与路径边界、24 项网关 CLI runner 自测，均符合本报告记录的原生行为。所有生成请求只访问本机 mock，不是以 SDK 调用替代 CLI。

业务接线最重要的差异：OMP 自定义 Anthropic provider 必须显式写入 auth: apiKey。省略 auth 会选择 OAuth 风格的请求整形；这与假密钥是否长得像 sk-ant-api03 无关。Pi 与 OMP 的错误进程码也不同，不能单独以 exit 0 判断生成成功。

本报告证明 CLI → 本地 mock 的原生契约和 runner 能力。生产 generate_entries → CLI → 真实 AIO → mock 的 W6 八流由协调者统一执行；本方没有将 runner 自测冒充该项已完成。网关预算、数据库、统计、熔断与协议调度的业务验证由 W3/W6 负责。

## 固定版本与可复现入口

| 项目 | Pi | OMP |
| --- | --- | --- |
| npm 包 | @earendil-works/pi-coding-agent | @oh-my-pi/pi-coding-agent |
| 版本 | 0.87.1 | 18.3.2 |
| 源码基线 | f07218c4d4bbc12bef056a7058c3dd49dfe41abe | 7853b4e499936f9dcc13c9b64adb55f6b342aabf |
| 真实入口 | pi/package/dist/bundle/cli.js | omp/package/dist/cli.js |
| 运行时 | 当前 Node | 本地官方 npm Bun 1.3.14 |
| CLI --version | 0.87.1 | omp/18.3.2 |

源码只读参考目录分别为 D:/UGit/pi-source-reference 与 D:/UGit/oh-my-pi-source-reference。发布包 SHA-256：

- pi-0.87.1.tgz：1423ee3c61e7c96464e1cbf3c8dc24d3056cb3410995c3671a98c3ecc527540f
- omp-18.3.2.tgz：a8784d8f9f2524685c505ce66cf5d748d3060d15228dd9aadc3f151309623d59
- Pi 入口文件：e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774
- OMP 入口文件：58e6d9987ff0218be382a8cfda94e338393add0690554147caddbf6bcc75ec57

所有安装、配置、fixture 输入、日志和捕获证据位于仓库 .trellis/.runtime/research/omp-pi/（下文简称 R）。没有全局安装，也没有修改根 package.json。安装器仅从官方 npm 下载固定 tgz 并校验上述 hash，使用空 npmrc、独立 cache、ignore-scripts/no-audit/no-fund。

本地依赖固定为 @oven/bun-windows-x64@1.3.14、@earendil-works/chord@0.87.1、@silvia-odwyer/photon-node@0.3.4、undici@8.10.2、typebox@1.3.27、@oh-my-pi/pi-natives@18.3.2、@oh-my-pi/omptype@18.3.2、@babel/parser@7.29.7。发布包的 node_modules 仅连接到 R/tools/node_modules。当前完整传递依赖锁为 R/tools/package-lock.json，SHA-256 ca954dbd02ce01532634456f2a00656b499b3138cf3957dd3c550571fea3243e；首次从空目录安装仍需 npm 可用，并不宣称离线供应链完全封闭。

从仓库根目录执行：

~~~powershell
node scripts/pi-omp-bootstrap.mjs
node scripts/pi-omp-wire-capture.mjs
node scripts/pi-omp-wire-extended.mjs
node scripts/pi-omp-wire-extended.mjs --omp-only --anthropic-only --scenarios=auth-generic,auth-apikey,auth-omitted-generic,auth-omitted-apikey
node scripts/pi-omp-config-probe.mjs
node scripts/pi-omp-runner-selftest.mjs
~~~

bootstrap 如找不到与 Node 同目录的 npm-cli.js，可将其绝对路径作为第一个参数传入。当前 bootstrap/OMP runtime 专门验证 Windows x64，其他操作系统不在已通过范围。各测试失败返回非零；每次 run 使用独立目录，证据包含 CLI 参数、stdout/stderr、原始 HTTP headers/body、返回 fixture 和断言。正常包入口及版本检查也是真实子进程。

### 最终证据索引

| 批次 | 结果 | 相对 R 的 summary.json |
| --- | --- | --- |
| 基本四协议 | 8/8 | runs/2026-09-26T08-45-32-883Z/summary.json |
| 10 场景增强 | 80/80 | runs/extended-2026-09-26T08-45-32-903Z/summary.json |
| Anthropic auth 分支 | 4/4 | runs/extended-2026-09-26T08-39-26-042Z/summary.json |
| 配置/路径/profile | 23/23 | runs/config-2026-09-26T08-37-50-542Z/summary.json |
| runner/entry-json/隔离 | 24/24 | selftest/mui5fg2q/summary.json |

早期探索目录包含依赖缺失、Windows 路径过长、OMP 隐式 auth 和错误 fixture 调整产生的失败记录，不应作为最终批次。尤其早期 Google 429 使用了通用 OpenAI 风格错误 envelope；最终按 Google code/status/message 修正后，Pi 开启 agent retry 的三次请求已经在最终 80 项批次通过。

## 隔离与网络边界

- 不继承用户环境，仅保留 Windows 系统进程启动所需变量；PATH 限当前 Node 目录及 System32。HOME/USERPROFILE/APPDATA/XDG/temp、agent 目录、Bun cache、git 全局配置均指向该 case 的 R 内 sandbox。
- 清除真实 API key、proxy、NODE_OPTIONS/BUN_OPTIONS、OMP_PROFILE/PI_PROFILE 等继承值。配置测试用例仅设置自己要验证的 profile，目录和内容均由 fixture 生成。
- 空工作目录创建 .git/.pi/.omp 标记；关闭 session、扩展、skills、项目上下文/规则及其他自动扫描入口。OMP 关闭 LSP/PTY/prewalk/title；Pi 另开 offline。
- 使用无效假密钥 sk-ant-api03-w0-fake-not-a-real-key，auth 对照还使用 w0-fake-not-a-real-key；没有访问用户真实 auth.json、OAuth 令牌、认证 SQLite 或收费上游。
- Node --import / Bun --preload 加载 pi-omp-network-guard.mjs，只放行该 case 的精确 HTTP localhost origin，拦截 fetch/http/https/net 出站。redirect 被拒绝。Node 与 Bun 的外部访问负例均通过。
- OMP 启动自身会尝试本地模型发现端口及 catalog.stencil.so 等；guard 记录并拦截非 fixture 请求。此为 JS 进程级护栏，不是 OS 防火墙，也不宣称约束任意外部原生程序。
- OMP 原生 CLI 在隔离 agent 内创建 agent.db/models.db 属于被测程序行为；抽查其 auth_credentials 等表为零条凭证，不应与 AIO 管理功能写认证状态混同。这里未验证 AIO 的文件写入边界。
- 每个 CLI 默认 30 秒超时；真实取消测试通过 RPC abort 和连接关闭验证，不以 kill 子进程代替取消。case 目录使用短 hash，避免 Windows native helper 的 MAX_PATH 限制。

## 8 条实际基础 wire 契约

下面的 P 为 /fixture/{protocol}，origin 为 mock 实际随机本地端口。请求全部为 POST，模型为 w0-model。OpenAI/Anthropic 的模型在 JSON body.model，Google 在 URL；各协议均通过完整基本 SSE，CLI 输出 W0_LOCAL_FIXTURE_OK 并结束 agent turn。

| CLI | API | 配置 baseUrl 后缀 | 捕获的 URL | 认证载体 |
| --- | --- | --- | --- | --- |
| Pi | anthropic-messages | P | P/v1/messages?beta=true | x-api-key |
| OMP，auth: apiKey | anthropic-messages | P | P/v1/messages | Authorization: Bearer |
| Pi | openai-completions | P/v1 | P/v1/chat/completions | Authorization: Bearer |
| OMP | openai-completions | P/v1 | P/v1/chat/completions | Authorization: Bearer |
| Pi | openai-responses | P/v1 | P/v1/responses | Authorization: Bearer |
| OMP | openai-responses | P/v1 | P/v1/responses | Authorization: Bearer |
| Pi | google-generative-ai | P/v1beta | P/v1beta/models/w0-model:streamGenerateContent?alt=sse | x-goog-api-key |
| OMP | google-generative-ai | P/v1beta | P/v1beta/models/w0-model:streamGenerateContent?alt=sse | x-goog-api-key |

OpenAI/Anthropic body 使用 stream: true，Google 使用 streamGenerateContent + alt=sse。传入 Google /v1beta 不会再重复追加 /v1beta。生成网关入口只需将 P 改为 /{client}/_protocol/{protocol} 并保持同样 API base 后缀。

### OMP Anthropic auth 不可省略

真实两轮 read 工具对照：

| provider 配置 | 假密钥外形 | URL | header | 声明/返回工具名 | User-Agent |
| --- | --- | --- | --- | --- | --- |
| auth: apiKey | 普通和 sk-ant-api03 两种均测 | /v1/messages | Bearer | read | omp/18.3.2 |
| 省略 auth | 普通和 sk-ant-api03 两种均测 | /v1/messages?beta=true | Bearer | _read | claude-cli/... |

省略 auth 的分支还产生 OAuth beta/system 整形；原始 key 外形不是该差异的根因。固定源码 packages/coding-agent/src/config/custom-models.ts 的 resolveCustomModelIsOAuth 在无 providerAuth 且 API 为 anthropic-messages 时返回 true；显式 apiKey 才退出该隐式分支。packages/ai/src/providers/anthropic.ts 对非官方 baseUrl 使用 Bearer 是另一独立行为。AIO 下游不可只接受 Anthropic x-api-key，也不能把 OMP 请求天然当作官方 OAuth 会话。

## 80 项增强结果与限制

每个 CLI/协议分别执行下面 10 项，全部通过脚本断言：

| 场景 | 实际验证 |
| --- | --- |
| tool | mock 首轮返回 read 调用，真实 CLI 执行只读 sandbox fixture.txt，第二轮将工具结果发送回 mock，随后成功结束；每 case 两次请求 |
| thinking | 显式 reasoning 与 OMP thinking 配置，wire 有对应思考请求字段，CLI 收到并保留模拟 thinking 内容 |
| image | 通过真实 CLI @pixel.png 附件路径输入 2×2 PNG，捕获请求含实际图像 MIME/负载并完成回复 |
| http400 | 正确协议的 HTTP JSON 错误可见，单次请求、无重放 |
| sse-error | HTTP 200 后协议 SSE error 使 assistant stopReason=error，单次请求 |
| midstream-error | 部分文本输出后发 error，保持错误结局，不重放或伪装成功 |
| retry429 | 关闭 agent retry 后观察各 SDK/CLI 原生重试差异，按准确请求数断言 |
| cancel | RPC prompt → mock 保持连接 → RPC abort 成功，连接关闭、assistant aborted、无第二请求 |
| model-override | provider 的 API/baseUrl 故意设置为错误分支，model 级覆盖为目标协议/正确地址；model header 覆盖同名 provider header，并保留其他 provider header |
| retry429-agent | 显式开启 agent retry，前两次 429、第三次成功；两 CLI 四协议均准确三次请求 |

图片不是 base64 原样透传断言：CLI 可自行 resize/re-encode，OMP 实际出现 PNG 转 WebP/放大至 200×200；不能把重新编码误判为丢图。该项证明图像请求形成与 SSE 完成，没有真实视觉模型语义能力证明。

思考用 maxTokens=8192 避免 OMP budget low 的约 4000-token 预算与早期 1024 输出上限冲突。Anthropic/Google 用 OMP budget，OpenAI 用 effort；没有按模型名字猜能力。未遍历所有 adaptive/level/budget 特殊组合。

### 错误、退出码与重试

- Pi JSON print 模式发生 HTTP/SSE 错误仍可原生退出 0；OMP 对这类错误退出 1。因此 runner 同时检查最终 assistant.stopReason=stop、非空文本、agent_end、非超时、CLI exit=0，并可要求 expect-text。负例已经验证 Pi CLI exit=0 时 runner 仍 exit=1。
- Pi Google SSE error 可丢弃上游原始 message，呈现 Google stream ended without a finish reason；错误状态可见不等于原始错误文案保真。HTTP Google error 使用规范 code/status/message 后可正常识别 429。
- 关闭 agent retry：Pi 四协议对首次 429 仅请求一次，error；OMP 四协议仍在内部 retry 后共三次成功。开启 agent retry：Pi 也三次成功。AIO 每次收到的 CLI 重试是新的入站请求，网关重试预算不能仅依据 CLI retry.enabled 推断。
- 中流失败与取消均没有重放。stream error、HTTP JSON error 与成功的非流式生成响应是不同能力，本报告只实测前两者及流式成功。

## 原生配置解析及目录边界：23 项

通过真实 CLI --mode rpc 的 get_available_models 探测，并对 models 文件做调用前后 hash 比较。

### Pi v0.87.1（8 项）

- models.json 接受 JSON、BOM 加 // 行注释、尾随逗号、重复 key（后者覆盖）；不接受 /* block */ 注释、单引号、无效 JSON。
- 缺省路径为 HOME/.pi/agent；PI_CODING_AGENT_DIR override 已实测。
- 无效文件在原生 RPC 下可能 success=true 但模型列表空、没有解析诊断；用例记录 knownNativeSilentEmpty=true，这是已确认原生缺陷，不能称为安全的空配置。AIO 必须自行解析并区分损坏文件与合法空列表。
- packages/coding-agent/src/utils/json.ts 是局部 JSONC 支持：仅清除 // 和尾随逗号；不是任意 JSON5。原生读取未改写 models 文件。

### OMP v18.3.2（15 项）

- models.yml 优先于 models.yaml；仅不存在 .yml 时 fallback 到 .yaml。损坏 .yml 退出 1 且不从有效 .yaml 静默回退。
- YAML 重复 key 使用最后值；anchors/merge 可被原生解析。AIO 的只读或拒绝复杂 YAML 策略应保留，不能以原生能够解析推导安全可编辑。
- 仅有 legacy models.json 时，真正 OMP CLI 会迁移并生成 YAML。这是被测 CLI 自己的行为；AIO 枚举/预览不得据此擅自迁移。
- HOME/.omp/agent 为默认；PI_CONFIG_DIR 可修改配置目录名；PI_CODING_AGENT_DIR 覆盖默认 agent。
- 命名 profile 使用 HOME/.omp/profiles/{name}/agent，并忽略 PI_CODING_AGENT_DIR。选择优先级 --profile > OMP_PROFILE > PI_PROFILE；显式 --profile default 恢复 agent override。
- OMP_PROFILE 为空串时不 fallback 到 PI_PROFILE，使用默认/override；../escape profile 被拒绝。各路径均为当前 Windows 环境的真实验证。

### 高级能力字段的源码核对

Pi ai/types.ts:86 和 coding-agent/core/model-config.ts:55–63：thinkingLevelMap 为七档 off/minimal/low/medium/high/xhigh/max 的可选键，值仅 string|null，不支持数字；缺键保留原生默认，null 表示不支持。要求所有七档都显式填写是 AIO 更严格的可表达性约束，不能说是 CLI 必需。

OMP coding-agent/config/models-config-schema-bundle.ts:109–155：mode 为 effort/budget/google-level/anthropic-adaptive/anthropic-budget-effort；efforts/defaultLevel 为 minimal/low/medium/high/xhigh/max；effortMap 为上述可选键的 string 值；supportsDisplay/requiresEffort 为 boolean。legacy minLevel/maxLevel/levels 也能归一化，但 AIO 无需自动接纳。supportsTools 与 preferWebsockets 属于模型级。SDK/catalog 类型还有 prefixBinding/effortRouting/effortBudgets/suppressWhenOff 等字段，不能据此假设自定义 models.yml schema 会完整保留它们。

## W6 CLI runner 接口

~~~powershell
node scripts/pi-omp-gateway-client.mjs --client pi --protocol openai-completions --base-origin http://127.0.0.1:PORT --model w0-model --target .trellis/.runtime/research/omp-pi/gateway-test --entry-json .trellis/.runtime/research/omp-pi/gateway-test/entry-pi-openai-completions.json --expect-text W0_LOCAL_OK
~~~

PORT 须替换为真实测试监听端口，示例不直接复制执行。支持 pi/omp 及四协议。entry-json 接受生产序列化 GeneratedEntry（protocol/nativeKey/baseUrl/models/node）；只有协议、路径、安全边界检查，不修改 node 的 api/auth/baseUrl/models，真实 --provider 使用 nativeKey。最终 sandbox models 文件为 providers[nativeKey]=node，外层 models 元数据不覆盖 node。证据记录输入文件 SHA-256 和原 node，输入文件不改写。

target 与 entry-json 必须位于 R 的规范路径内，base-origin 仅允许显式端口的 http://127.0.0.1。entry node 的 baseUrl 必须同 origin，拒绝命令形式值及显式非 apiKey auth。不要向该研究接口提交用户凭证；上层使用生产生成的无效本地占位 key。省略 entry-json 时生成受控 mock fixture 配置，不能用这一分支证明生产生成器格式正确。

stdout 为一行 JSON，含 passed/client/version/protocol/provider/cliExit/timedOut/stopReason/text/evidencePath，失败附 error；wrapper 失败返回 1。24 项自测包括 8 条内置配置成功、8 条 entry 原样成功、4 个非法 entry 拒绝、2 条原生错误传播、Node/Bun 各 1 条网络拒绝。entry 自测本身仍使用测试构造 node；生产生成器来源由 W6 Rust 测试保障。

## 未验证项与交接

1. 真正 CLI → 生产生成节点 → AIO listener → mock 的八流以及网关 failover/预算/SQLite/统计，由协调者运行已接收本 runner 的 W6 ignored 测试；本报告不提前宣布通过。
2. 真实 CLI 的成功非流式生成未验证；当前已确认 CLI 通路使用 streaming，没有用 SDK 直接调用补作 CLI 证据。
3. 未调用官方收费上游、真实 OAuth、Bedrock/Vertex/Gemini CLI、WebSocket、pi-native 等范围外协议。四协议本地 SSE 正确不代表这些范围已支持。
4. Linux/macOS、其他 Bun/Node/CLI 版本、文件系统权限/损坏 SQLite、所有 YAML 特殊标签、全部 thinking 档位或大体积工具/图片未穷举。
5. 捕获依赖与原始证据保留在 R（通常被 git 忽略）；持久提交候选为 8 个 scripts/pi-omp-*.mjs 与本报告。本 worker 未编辑业务源码、其他任务文档、全局安装或用户认证，也未提交、推送、回滚共同工作区改动。

最终已对 8 个脚本运行 node --check，全部通过。首次 8 条基本请求完成时已发 checkpoint，随后将 OMP auth 归因修正和高级类型边界同步给协调者、wire 与 catalog；报告中的最终批次优先于探索期结论。
