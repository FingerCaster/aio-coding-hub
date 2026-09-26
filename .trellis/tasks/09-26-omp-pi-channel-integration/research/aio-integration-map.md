# AIO 接入面与旧思路的替代方向

AIO 基线见 [来源清单](source-baselines.md)。旧分支已弃用，下列分析来自当前 origin/main 基线。

## 当前结构与必要改动

| 当前证据 | 已存在的约束 | 本任务方向 |
| --- | --- | --- |
| src/constants/clis.ts:3、CliCapability、CLI_REGISTRY | 四个客户端；可按能力生成页面入口 | 注册 pi/omp，增加原生供应商能力，使用能力过滤页面 |
| src-tauri/src/shared/cli_key.rs:45、CliKey | Rust 同样有有限集合与能力注册 | 前后端同步，不能只改 UI union |
| src-tauri/src/gateway/routes.rs:32、proxy_cli_any | 通用 URL 入口把 cli_key 与 path 交给 proxy_impl | 新入口显式解析来源客户端和 wire protocol |
| src-tauri/src/gateway/cli_auth/registry.rs:15、strategy.rs:9 | API Key 注入当前按 CLI 选策略 | 新请求按显式协议注入认证，来源仍记 pi/omp |
| src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs:470 | 是否为模型请求按 cli_key + path 判断 | 协议驱动的请求分类；保持老 CLI 分类规则 |
| src-tauri/src/gateway/streams/usage_tee.rs:254 | Responses 识别与终止帧按 codex/grok 分支 | 协议驱动 SSE 与 usage 解析，不能把 pi 当作 codex 记账 |
| src-tauri/src/gateway/proxy/upstream_error_response_rules.rs:53 | 最终错误体也有 CLI 分支 | 按线上协议生成客户端能消费的错误体/帧 |
| src-tauri/src/domain/provider_models.rs:555、882 | 现有模型目录实现包含 Codex 专用限制 | 首期不将 Pi/OMP 硬塞进 Codex 目录或托管 Profile |
| src-tauri/src/infra/db/migrations/v39_to_v40.rs:110 | provider_model_catalogs.protocol 受 openai_compatible CHECK 限制 | 新的 Pi/OMP 模型元数据需独立存放，不能仅扩大 TS 枚举 |
| src-tauri/src/infra/cli_proxy/mod.rs:16 | 现有托管文件 manifest 和原子写工具 | 复用底层文件工具；新入口要节点级所有权，不能整文件覆盖 |
| src/query/keys.ts:363、src/pages/ProvidersPage.tsx:14 | 现有查询键和能力筛选 | 新查询键包括 cli + target，避免 Pi/OMP/profile 缓存串用 |

## 应复用的网关机制

providers、现有路由模式和配置模型映射、共同 Provider gate、账号用量/配额 gate、同供应商重试、熔断、并发所有权、请求日志和用量持久化保持一条执行链。新请求只增加来源/协议上下文与相应入口适配，不建立第二套代理服务器或重试引擎。

特别遵守：

- gateway-attempt-budget-contract.md：预算和计时归属；协议适配不能重置尝试次数。
- gateway-failover-route-contract.md：共同 gate、route hop 与日志计数。
- configured-model-routing-contract.md：原始模型不可变、每次尝试独立计算最终上游模型。
- upstream-error-handling-contract.md：流式恢复只发生在提交前，真实上游错误不被展示重写掩盖。
- settings-ownership-rollback-contract.md：新增目录设置只写自己拥有的字段。
- provider-share-contract.md：分享导出不能偷偷泄漏原生完整配置、命令凭证或本地访问 token。

## 不能沿用的捷径

- 不能将 Pi/OMP 的四种协议实现为六至八个新的 CLI key；日志来源只有 pi 和 omp，协议是另一维。
- 不能通过把 cli_key 改成 claude/codex/gemini/grok 来“复用”代码；这会误归属日志、路由池、计费和客户端专用中间件。
- 不能只增加认证策略而不补请求分类、模型路由、SSE 错误、终止边界、usage 和计费维度。
- 不能把原生节点存在、网关上游启用和实际模型选择压成一个 enabled 字段。
- 不能将原生模型上的 baseUrl、headers、transport、远程压缩 URL 原样复制到生成的 AIO 节点；可能绕开网关或携带上游密钥。
- 首期不扩大插件 SDK 的 target CLI 契约；没有适配的插件/WSL/托管升级等入口必须按能力隐藏。

## 关键替代方案

“原生配置目录 + 既有网关上游池 + 独立 AIO 原生入口”三者独立：原生档案按节点事实同步；网关上游仍使用 AIO Provider，用户显式导入原生配置快照或手工创建；AIO 入口只承载本地网关地址和经过验证的模型能力。这个边界避免列表刷新隐式改写正在路由的上游。
