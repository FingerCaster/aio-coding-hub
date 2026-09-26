# W3 多协议网关实施与验证

日期：2026-09-26。工作分派：task_8cd7aa11962f / ctx_c26f25226f28。

## 实施边界

- 新增 GatewayProtocol 四枚举与 kebab-case API：anthropic-messages、openai-completions、openai-responses、google-generative-ai。
- Pi/OMP 使用显式 /{client}/_protocol/{protocol}/... 路径进入既有 middleware、provider gate、retry、failover 链；source cli 始终保留 pi/omp。
- Native 候选在 gate、attempt、健康记账前按 source/protocol/API-key/direct/声明及 publication 能力过滤；每次发送前再次调用 W4 candidate_eligible。
- 各协议独立处理上游认证、推理路径、模型、错误 JSON/SSE、终止信号及 usage；清除客户端凭证，保留协议业务 query。
- Codex 托管模型、500ms SSE guard、压缩、容量错误容错与 Claude rectifier 等仍由原客户端身份 opt-in。Grok 旧 Chat Completions 与 Responses 兼容。
- 配置模型路由沿用既有 owner，每个候选从原始公开模型求解一次；能力校验不执行映射。
- Native metadata-only SSE 在提交前可走同一 failover；可见输出提交后上游错误、EOF、客户端取消不得回放给其他 provider。

## Usage 语义

经协调者明确授权，domain/usage.rs 是新增 native usage 的生产归一化 owner：OpenAI input 扣 cache read + write，Google input 扣 cache read，Anthropic 保持加性。
只改变返回的 metrics 输入桶，usage_json 保留 wire 值；stream finalize 对累计原始值的副本归一化，重复 finalize 不会重复扣减。
未知输入保留 None，total 和各缓存明细保留。最终日志 gateway_protocol marker 标记 input_semantics=exclusive 与 wire_input_tokens，来源仍为 pi/omp。
现有 SQL 聚合与计费将 pi/omp 输入视作 exclusive；本次不修改它们的生产语义，仅扩充对账测试证明四种协议和旧四客户端守恒。

## 测试资产

- native_protocol_route_tests.rs：实际 Axum router + 本地 TCP upstream，覆盖 JSON/SSE 八组合、跨协议零请求零 attempt 零健康惩罚、同协议 failover、提交前错误、提交后错误、无 [DONE] 的 EOF、非法路径和取消。
- fixtures/native_wire_features.json：W0 Pi 0.87.1 / OMP 18.3.2 的 32 个工具、工具结果回合、图像和 thinking 请求/响应；生产 router 逐字段/逐字节验证透传。
- usage_stats/tokens.rs：JSON/SSE/重复 finalize/未知 usage/下溢、真实 SQLite 表达式与生产 cost 函数对账，并覆盖旧四客户端。
- ignored native_real_cli_eight_protocol_streams_through_actual_gateway：真实进程读取生产 catalog + generate_entries 的节点，通过实际监听端口访问 AIO 网关；每种组合包含跨协议跳过和同协议失败后成功。

## 验证状态

代码、定向 rustfmt 与 git diff --check 已完成。最终构建和测试由协调者串行运行，避免共享 Windows 二进制锁冲突。

- 真实 CLI 八组合：8/8 通过，11.98 秒；生产 catalog + generate_entries → 独立 HOME 配置 → Pi/OMP 真实进程 → AIO HTTP listener → 同协议 failover，来源、协议、模型和最终输出均保留。
- 路由模块：10 个非 ignored 测试全部通过，包括 32 个真实 feature 请求、八组合取消、HTTP/SSE usage audit、跨协议预算隔离和提交边界。
- 最终全量 Rust：主库 3164 通过、0 失败、5 ignored；加独立集成套件合计 3290 通过、0 失败。
- JSON/SSE/SQL/计费对账、未知输入/下溢保护、发送前动态资格失效的预算释放测试全部通过。首次复验中的 Anthropic JSON/SSE 包层夹具问题已修正，最终对账结果为通过。
- Clippy：cargo clippy --all-targets --locked -- -D warnings 通过，耗时 1m 11s。
- 协调者最终确认：check:generated-bindings 无漂移；前端 2977/2977 与 build 通过，统一流水线已完成。

最终证据：.trellis/.runtime/research/omp-pi/rust-all-final.log、rust-clippy-final.log、rust-real-cli-gateway.log、gateway-test/aio-gateway-results.json。

W3 请求范围已完成，无待修复项；最终统一发布、提交与产品整体验收由协调者负责。

## 共享区说明

没有提交、推送、修改全局安装，也没有回滚其他 worker 改动。
providers、DB、settings、前端及 .trellis/config.yaml 不属于本 worker 修改范围。
shared/mod.rs 只注册 gateway_protocol；handler/mod.rs 三处既有 ProxyContext 测试 fixture 的 wire_protocol 字段由协调者补齐。
