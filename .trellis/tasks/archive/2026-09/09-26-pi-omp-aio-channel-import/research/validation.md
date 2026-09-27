# Pi / OMP 统一渠道接入验证记录

日期：2026-09-26。分支 FingerCaster/omp-pi-channel-integration，基础提交 3372d11f5a20a212bd551e0b0b7067ab2b639c79，包含前置任务及本任务未提交代码。用户已批准实施；代码与隔离测试完成，用户桌面体验验收仍待反馈。

## 已实现的行为

- Pi / OMP 的原生配置和 AIO 网关视图均可接入 Claude Code、Codex、Grok、Gemini；Grok Chat 与 Responses 分开。两视图共用多选、能力补齐、预览、确认、更新和精确撤回流程。
- 独立节点实时引用来源当前活动供应商池；不复制供应商和真实凭证，不改原生默认模型或正在运行的会话。
- consumerCli 与 sourceChannel 分离。来源负责选源、模型映射、认证及 OAuth 适配；日志与用量仍归 Pi / OMP，通过同一记录展示来源和实际上游。
- v48 增加独立绑定和能力声明表；旧 target + protocol manifest 保持语义。复用目标锁、文件与目录版本校验、私有备份、持久意图、原子替换及失败补偿。
- 能力声明绑定来源配置 revision，凭证轮换不需要复制密钥；每次选源和重试前重新核查模型白名单、协议、当前池及已发布能力。
- Gemini 标准 API 是本期可用路径。当前没有权威的企业许可证据字段，因此所有 Gemini OAuth 在目录、发布及运行时都阻塞为资格待核实；不按账号名称、token 或邮箱推断套餐。Antigravity 未加入来源。

## 执行结果

所有相对日志路径均在仓库的 .trellis/.runtime/ 下。

| 检查 | 结果 | 证据 |
| --- | --- | --- |
| pnpm typecheck | 通过 | native-channel-typecheck.log；最终 pnpm build 再执行 tsc |
| pnpm lint | 通过 | native-channel-lint-final.log |
| pnpm test:unit | 326文件、3012项通过 | native-channel-frontend-all.log |
| 最终前端相关回归 | 11文件、58项通过 | native-channel-frontend-targeted.log |
| cargo test --lib --locked | 3178通过、6显式忽略、0失败 | native-channel-rust-tests.log |
| 最终 native_channel 定向回归 | 14通过、1显式忽略、0失败 | native-channel-targeted-final.log |
| 真实 Pi / OMP 基础矩阵 | 10/10通过，CLI退出码均为0 | research/omp-pi/channel-test/results.json及每case evidence.json |
| cargo clippy --all-targets --locked -- -D warnings | 通过 | native-channel-clippy-final.log |
| cargo fmt -- --check | 通过 | 与最终 Clippy 同批执行，失败即停止 |
| pnpm check:generated-bindings | 通过，生成后无漂移 | native-channel-bindings-final.log |
| spec links / gateway error codes | 通过 | native-channel-contracts-final.log |
| pnpm build | 通过 | native-channel-production-build.log |
| 真实 React / Edge 视觉检查 | 亮暗色、1280/720宽度、能力、Gemini阻塞、预览；pageerror为空 | research/omp-pi/channel-ui/browser-evidence.json和8张PNG |

全量 Rust 在最后的 pending-intent 预览恢复修正前运行，修正后重跑全部14项渠道相关测试并运行全量 Clippy。全量前端在最后两项纯样式修正前运行，之后重跑相关58项、lint和包含类型检查的生产构建。未将前置任务的计数冒充本任务结果。Vite 有既存大 chunk 提示，构建无错误。

## 基础通路及认证证据

| 来源与协议 | Pi 0.87.1 | OMP 18.3.2 | 验证性质 |
| --- | --- | --- | --- |
| Claude Code / anthropic-messages | 通过 | 通过 | 标准 API、本地模拟上游、真实 CLI SSE |
| Codex / openai-responses | 通过 | 通过 | 标准 API、本地模拟上游、真实 CLI SSE |
| Grok / openai-completions | 通过 | 通过 | 标准 API、本地模拟上游、真实 CLI SSE |
| Grok / openai-responses | 通过 | 通过 | 标准 API、本地模拟上游、真实 CLI SSE |
| Gemini / google-generative-ai | 通过 | 通过 | 标准 API、本地模拟上游、真实 CLI SSE |

真实 CLI 从生产发布服务生成的隔离配置启动，经 AIO 绑定路由访问 loopback mock。每条基础通路包括先500再接力成功的候选顺序、成功文本、consumer/source日志与usage断言。Pi 使用固定版本官方包入口，OMP 使用固定版本 Windows 独立运行时；精确入口、版本和指纹见 results.json。全部为测试凭证，没有读取真实用户密钥或调用付费推理。

额外的后端路由测试覆盖两个消费者 × 五个来源协议组 × 流式/非流式，共20组合。Claude、Codex、Grok 的现有 OAuth 适配共6个模拟组合通过；另覆盖 Codex 401刷新和重试，仍保留 Pi / OMP 调用方身份。loopback OAuth 地址覆盖仅在 cfg(test) 可用，不能被生产环境开启。这些结果不证明真实账号许可、在线配额或任意第三方上游可用。

工具与推理能力由来源显式声明，并经既有原生验证器、能力交集和运行时覆盖检查约束；本轮10条真实CLI基础矩阵以文本流为验收，不把该计数扩张为所有工具/推理、网络传输和账户组合的实测。

## PRD A1–A9 证据映射

| 验收 | 实际覆盖与入口 |
| --- | --- |
| A1 | NativeChannelFlows.test.tsx、NativeProvidersPanel.test.tsx、NativeGatewayFlows.test.tsx：两页、四来源、监听状态、无候选/无能力、取消、目标切换；真实浏览器截图复核。 |
| A2 | channel_incremental_publication_coexists_and_withdraws_exactly_for_both_clients：同目标多来源、Codex/Grok同协议共存、增量保留、幂等、原生字段与来源数量/凭证隔离。 |
| A3 | channel_runtime_enforces_current_pool_identity_whitelist_and_capability_coverage、native_channel_model_mapping_rotation_and_session_isolation_follow_source：当前池、顺序、映射、凭证轮换、禁用/删除、绑定间会话隔离。 |
| A4 | native_channel_binding_rejects_cross_consumer_forced_source_and_wrong_protocol、native_channel_rechecks_withdrawal_before_same_request_failover，以及能力漂移测试：拒绝跨来源、跨消费端、错误协议、白名单外模型，发送前再次检查撤回。 |
| A5 | native_channel_four_sources_five_protocol_groups_keep_consumer_usage_and_source_auth：20组合保留调用方/来源认证与单次记录；OAuth6组合及刷新单测不改调用方；既有取消/用量链全量回归，前端nativeChannelRoute.test.ts验证来源标记。 |
| A6 | 两消费端目标级绑定/发布、旧目标/Profile回归、外部节点冲突、撤回保留其他节点/默认值；channel_conflicts_and_finalize_failure_leave_foreign_nodes_untouched验证失败补偿。 |
| A7 | 真实CLI10组合和OAuth模拟测试；官方产品依据见gemini-antigravity-support.md，真实付费账户及企业许可未验证，不写成已支持。 |
| A8 | Rust3178、前端3012及最终相关回归；v48迁移幂等和旧manifest主键契约；portable_bundle_preserves_native_channel_bindings_and_declarations_before_replacement；pending-intent恢复测试。 |
| A9 | channel_gemini_standard_api_is_available_but_unverified_oauth_is_blocked验证标准API、OAuth未知/企业名/个人名、混合池、配置转为OAuth后的再准入；生产不猜许可，均阻塞未知OAuth。生成标准Google节点，不依赖已移除的Pi内置订阅provider。UI fixture展示企业/个人提示不构成套餐识别证据。 |

补充恢复案例：预览只核对持久意图的desired/previous精确摘要，不修数据库、不写文件；apply才执行恢复。覆盖文件写入前、文件已写入但DB未完成、撤回意图与外部改动。不会把合法待恢复状态误判为不可恢复的外改，也不会覆盖真正的外部节点。

## 限制与用户验收

- Gemini企业OAuth无经过验证的许可/适配证据，当前仍阻塞；没有新增自动许可探测或登录，Antigravity独立适配不属于本期。
- 本轮没有真实账户、付费上游、macOS/Linux实机及任意CLI版本验证。Windows隔离矩阵与浏览器fixture是已完成证据。
- 原生CRUD、现有自有网关入口及源渠道沿用原实现；portable旧格式不能表示新绑定/能力时明确拒绝导出或替换导入。
- 前端AGY报告已由主协调者更正，生成图片不作为截图，采用真实浏览器渲染PNG。协作worker已释放。
- 不自动安装测试包，不改真实CLI配置，不自动提交、推送或创建远程发布。任务保留待用户体验验收。

## Windows 测试包记录

版本0.60.43，Windows x64 Release，本地未签名。构建命令 pnpm tauri:build -- --bundles msi，使用 src-tauri/target 缓存。

构建和打包脚本分别记录：

- .local/test-builds/pi-omp-source.json：分支、基础提交、工作区文件SHA-256及总指纹；构建前捕获、完成后校验。
- .trellis/.runtime/native-channel-tauri-build.log：实际生产构建结果。
- .local/test-builds/latest-pi-omp-package.json：最终MSI/portable路径、大小、SHA-256及MSI资源检查。

交付时以新生成清单和实际产物校验为准，不复用前置任务包。数据库包含v48；回退旧版前需保留用户数据备份，portable和安装版共用用户数据目录。测试重点为多渠道接入、实际CLI选择入口、来源池变化、日志来源标记及撤回。

### 构建实绩

构建验证时间：2026-09-26T21:28:28.9624121+08:00。MSI 与 portable 均已生成，EXE 为 AMD64 / PE32+，ProductVersion 为 0.60.43，MSI 身份与必需插件资源、ZIP 内全部文件哈希均通过检查。

构建输入指纹：8a6f7c7cd042486696c6fc70fd469511ffd198ea1bde68bb530cc151c1d457f6；构建后已校验无漂移。打包后仅补写 task.json、implement.md 与本报告的完成状态/产物记录，编译源码与打包内容未改。

| 产物 | 字节 | SHA-256 |
| --- | --- | --- |
| D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-212827/AIO-Coding-Hub-0.60.43-pi-omp-win64.msi | 18382848 | f01af0075a8a3d4f25223aab2b4a7a62953e265e095b89045b5d1b9074d65261 |
| D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-212827/AIO-Coding-Hub-0.60.43-pi-omp-win64-portable.zip | 18644183 | e223f1f85f5a251d6757788669345815ee30ad0c646674894332d1a0a07408a6 |


## 2026-09-26 用户验收追加：自动模型能力与下拉编辑

- 实现和字段来源详见 `automatic-model-capabilities.md`；原测试包不含本轮改进。
- 前端全量：327 个测试文件、3024 项通过。最后一次必填提示微调后，30 项相关测试再次通过。
- Rust 全量：3186 项通过、6 项原有 ignored。新增读取快照/来源资格/路由核对测试与元数据解析测试通过。
- TypeScript、定向 ESLint、Rust fmt、Clippy `--all-targets --locked -- -D warnings`、spec links 通过。生成接口类型一致性检查通过。
- Edge 真实组件：Pi 浅色/OMP 深色、宽窄窗口、自动填写、手动编辑后的刷新、键盘焦点和 Escape 均通过；页面异常为 0。截图已目视核对。
- 本轮使用本地 IPC/网络 fixture，无真实付费推理或真实账号许可验证。此前 CLI/协议矩阵为既有基线；此次没有修改 CLI 发布格式或推理发送语义。
- 测试日志：`.local/test-builds/automatic-capabilities-vitest.log`、`automatic-capabilities-rust.log`、`automatic-capabilities-clippy.log`。
- 新包构建与校验实绩在完成后附于本节。用户体验验收仍 pending。

### 自动获取改进包构建实绩

构建完成：2026-09-26T22:43:35.4592712+08:00；Windows x64 Release，版本 0.60.43。MSI/ZIP、AMD64/PE32+、资源文件与哈希已校验；release EXE 的修改时间晚于本次源码快照。

源码指纹：8c8c6d5debdbb6e0bb89bdb586407909376b2dbf6565f8110cf46ba75963ca9d。构建后冻结校验通过；交付记录仅更新当前任务的 task.json、implement.md 和两份验证报告，编译源码未改。

| 产物 | 字节 | SHA-256 |
| --- | --- | --- |
| D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-224335/AIO-Coding-Hub-0.60.43-pi-omp-win64.msi | 18427904 | 382a89df70d5c0f0e929fd43556767737b36502d0504130e245dfa0084959f01 |
| D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-224335/AIO-Coding-Hub-0.60.43-pi-omp-win64-portable.zip | 18686684 | 958fa222c915205e22d75b48fcc742532dba99e506cecea57ec304a6a99a017a |

## 批量选择与默认参数（2026-09-26）

- 前端全量：327 files，3028 passed。相关 34 项包含批量追加/过滤全选/去重、原生映射、自动默认、刷新自动值与手动保留。
- Rust 全量：3189 passed、6 ignored；新增 3 项验证目录有效性、精确身份、容量/思考默认、显式能力优先与路由/冲突保护。
- TypeScript、所改前端/生成器 ESLint、Rust fmt、Clippy all-targets -D warnings、spec-links 均通过。
- Edge 真实组件 + mock IPC：Pi 浅色/OMP 深色、宽窄屏、一次添加 3 模型、容量/思考自动填充、手改后刷新、批量保存和焦点通过，截图已目视。
- 目录数据按 Pi AI 0.87.1 / OMP 18.3.2 生成并包含输入哈希、源码 revision 与随包 MIT 许可。未使用真实账号或付费推理。
- 生成 bindings 一致性和新测试包路径在本次构建完成后补记。
- 生成 bindings 一致性检查通过；最终页面回归（含原生默认选项）通过；准备构建 Windows Release MSI/ZIP。

### 本轮测试包已完成

- 包目录：D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-232554
- 源码指纹：6fc75ecf65688c10dca83f14170b1ea4410e23a60bf961b0324dce00187d260b
- Release 构建 8m02s；MSI / ZIP 内容与许可文件检查通过，ZIP 逐文件哈希一致，构建前后 251 个文件指纹一致。
- AIO-Coding-Hub-0.60.43-pi-omp-win64.msi，SHA256：7b15fd7c1a1dd55975ecf76f0a7df4a457405cb979eb30d94e7c1ea2ed61078f
- AIO-Coding-Hub-0.60.43-pi-omp-win64-portable.zip，SHA256：1cf56897b245597562d977ae68ec476d49c8f4763666a5dd52dd20d1f260995a
- 交付后仅补写本任务文档；代码不变的证明见 .local/test-builds/batch-defaults-postbuild-proof.json。用户体验验收仍为 pending。

## 聚合网关入口说明（2026-09-26）

- 导入页明确聚合请求路径、AIO 调度池和模型资料作用；后端原本即使用渠道聚合入口，未修改路由机制。
- 前端相关 13 项、Rust 相关 19 项通过；显式运行真实 Pi/OMP 的 10 条本地失败接力通路，全部通过。
- TypeScript、定向 ESLint / Prettier、spec-links 通过；真实组件明暗主题、窄屏及单入口预览检查通过，截图已目视。
- 用户随后要求重新打包，已生成 pi-omp-20260926-235813，含本次界面说明调整。构建前后 252 个文件一致；Windows x64 EXE/MSI 版本、资源与 ZIP 哈希校验通过。详情见 aggregate-gateway-clarification.md。
- 本包源码指纹：24fa62261608174890bbecd1db67e6ac8ef7e9091e1fc51f8eb8e056f64f5d0d。前节源码指纹与“代码不变”证明仅对应各自当时交付时点。

## OMP 独立程序版本识别（2026-09-27）

- 修复仅支持 npm 元数据造成的“版本未知”，增加隔离且限时的独立二进制版本查询及 Windows 官方安装目录后备扫描。
- Rust CLI manager 9 项与实机 OMP 查询 1 项通过，实机返回 18.3.2；前端 12 项通过。
- TypeScript、定向 ESLint/Prettier、Rust fmt、Clippy all-targets --locked -D warnings、spec-links 通过；正式依赖的 Windows 编译问题已修复后重验。
- 本轮源码修复尚未打包；Pi/OMP 自动检查更新和一键升级仍未实现。详见 cli-version-management.md。

## CLI 自动检查与主动升级（2026-09-27）

前端 36 项、Rust 30 项、联网隔离安装 2 项通过；类型、规范、Clippy、绑定与生产构建通过。不会自动安装或升级；完整证据见 cli-updates-validation.md。此轮尚未重新打包。
