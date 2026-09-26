# 代码证据与规划依据

检查日期：2026-09-26。来源为当前未提交工作区，基线任务 09-26-omp-pi-channel-integration；当前 HEAD 为 3372d11f5a20a212bd551e0b0b7067ab2b639c79。本任务只新增规划文档，未验证或实施跨渠道接入。

## 已确认事实

| 编号 | 证据 | 对本任务的影响 |
| --- | --- | --- |
| E1 | src-tauri/src/domain/native_gateway/catalog.rs:181，catalog 调用同一 client 的 list_enabled_for_gateway_using_connection；metadata.rs:56 限定 native client。 | 当前 Pi/OMP 目录没有跨来源池语义，不能只在前端列出 Codex 就认为已接通。 |
| E2 | src-tauri/src/domain/native_gateway/catalog.rs:19，provider_context 要求显式 protocol、直接 api_key，并拒绝 source_provider/bridge。 | 直接复用现目录不支持来源 OAuth 和统一渠道；需要显式渠道绑定路径，保留原路径约束。 |
| E3 | src-tauri/src/app/native_gateway_service.rs:424、460，native_gateway_import_preview/confirm 以 targetId/nativeKey 读取原生供应商并导入 AIO。 | 现有导入方向与新需求相反；不得复用同名按钮导致混淆。 |
| E4 | src-tauri/src/gateway/proxy/handler/provider_selection.rs:22–29，cli_key 同时参与会话快照与活动源池选择。 | 实时引用可复用源池选择逻辑，但需区分调用方、源池和会话作用域。 |
| E5 | src-tauri/src/gateway/proxy/request_context.rs:18–19，当前上下文只有 cli_key 和 wire_protocol；protocol.rs:93、103 按真实 native client 选择协议用量解析。 | 直接将 cli_key 改成来源会丢失真实 Pi/OMP 身份；需要新增来源维度并逐处审计策略归属。 |
| E6 | src-tauri/src/infra/db/migrations/v46_to_v47.rs:46–58，native_gateway_manifests 主键是 target_id + protocol，另有 target_id + native_key 唯一约束。 | Codex/Grok 同为 Responses 时不能存入同一旧槽位；新绑定应有来源身份，旧节点所有权保持。 |
| E7 | src-tauri/src/gateway/proxy/protocol.rs:10–18，legacy_protocol 映射 Claude Messages、Codex Responses、Gemini Google、Grok 依 path 区分 Responses/Chat。 | 来源渠道和 wire 协议不是一一对应；Grok 必须按协议独立验证，不猜默认。 |
| E8 | src-tauri/src/domain/provider_models.rs:133–154，模型条目含稳定 identity、reasoning/context 等；native_gateway/metadata.rs:49、69–91 还约束工具等原生能力。 | 模型列表不能直接完整投影为原生模型；缺失能力需明确补齐，不按名字推断。 |
| E9 | src-tauri/src/domain/native_gateway/generation.rs，生成器使用回环 URL、占位 Key、白名单字段，OMP 显式 auth=apiKey。 | 新入口复用这些生成安全边界，但加入来源标识和绑定路由。 |
| E10 | src-tauri/src/gateway/routes.rs:105 起，路由 /:cli_key/*path 将路径客户端直接传入 proxy_impl。 | 直接写 /codex 地址会按 Codex 运行；需要受控入口而非隐藏身份替换。 |
| E11 | src-tauri/src/gateway/proxy/provider_adapters/mod.rs 含 claude/codex_chatgpt/gemini_oauth；gateway/oauth/adapters/grok.rs 为现有 Grok 适配；provider_limits.rs:367 起按实际 Provider 判断 OAuth quota/spend gate。 | 认证和限额已有可复用管线；其与真实 Pi/OMP 请求的兼容性仍需端到端验证，不能在规划时宣称通过。 |
| E12 | src/constants/clis.ts 的 CLI_REGISTRY 包含 Claude、Codex、Gemini、Grok、Pi、OMP；现有 native 前端位于 src/pages/providers/native/。 | 可复用渠道展示与原生组件；本方案只允许前四类作为来源，避免 native 相互引用。 |
| E13 | 2026-09-26 搜索 src、src-tauri/src、docs 未找到 Antigravity/反重力实现；gateway/oauth/registry.rs:26 和 oauth/adapters/gemini.rs:75–81 仍注册 GeminiOAuthProvider / gemini_oauth。 | 当前 AIO 没有完成反重力迁移；不能重命名旧 Gemini 入口作为新增来源。 |
| E14 | Pi v0.87.1 的 packages/ai/CHANGELOG.md:964–968 与 providers/google.ts:6–13；OMP v18.3.2 的 packages/ai/src/registry/hooks/oauth-code.ts:12–13。完整固定版本及官方公告见 gemini-antigravity-support.md。 | Pi 已移除两种内置订阅来源但保留标准 API；OMP 仍有原生实现，不能据此认定 AIO 已接通或旧个人 OAuth 仍有效。 |

## 相关规范

- .trellis/spec/aio-coding-hub/cross-layer/pi-omp-native-gateway-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/configured-model-routing-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/provider-oauth-device-flow-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/provider-deletion-and-attempt-identity-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/usage-insights-contract.md
- .trellis/spec/guides/code-reuse-thinking-guide.md

其中原生网关契约要求真实客户端身份、候选同客户端、精确所有权和独立入口。本方案不放宽旧路径，新增渠道绑定作为独立、明确的路由授权，并在实现时更新相应契约。

## 明确未完成的事项

- 未修改任何应用代码、数据库、用户 native 文件、登录状态或默认模型。
- 未执行新路径的 SDK/OAuth/网关测试；设计中全部新矩阵项仍是待实施验收范围。
- 未查询或复制任何真实用户凭证；未调用任何模型/账号/计费接口。2026-09-26 补充读取了公开官方公告、认证文档、仓库元数据和发布信息，用于支持边界核实。
- 规划阶段未创建 worktree、未切分支、未启动新 worker、未提交或远程发布。
