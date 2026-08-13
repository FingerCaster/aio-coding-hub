# 技术设计：CX2CC 路由、透传与 GPT-5.6 能力

## 目标与边界

本设计把 CX2CC 视为一个协议路由边界：请求从 Anthropic/CCH 形状进入，经过
唯一的 CX2CC mapper 变成实际 Codex/OpenAI Responses 请求，再交给来源供应商或
当前 AIO Codex 分流。通用的 configured model mapping/rewrite 不能在该边界之后
再次改写模型；普通 Claude、Codex 及其他 bridge 的既有行为保持不变。

本任务使用 upstream 作为审计证据和冲突参考，不做整合无关的 cherry-pick。明确
不移植 upstream 的完整 `model_policy`/discovery 架构、JWT 清理、目录事件或
`e2d03792` 的反应式 rectifier。实现者不得因为测试方便而把 CX2CC 标为
`managed_model_route`，也不得以 `source_id == None` 或 localhost 白名单替代回环
安全边界。开发 worker 统一使用 Codex；应用本身仍保留 Claude/CX2CC 兼容能力。

## 不变量

1. CX2CC mapper 是模型选择的唯一 owner。translated Responses body 中的最终
   `model` 必须是 mapper 选出的 remote model；configured route 的目标重写对
   CX2CC 为 no-op。普通 provider 仍按原 route 规则执行。
2. 思考参数遵循 presence 语义。调用方显式给出的 `output_config.effort`、
   `reasoning`/`thinking` 禁用或启用状态不能被 settings 的固定 effort 覆盖；
   缺省请求也不能凭空注入固定 effort。持久化的旧
   `cx2cc_model_reasoning_effort` 字段可继续读写以兼容旧 settings，但不再是
   CX2CC 请求的运行时强制值。`service_tier`、`store` 等非思考设置的所有权不变。
3. Responses effort 只接受该协议域的事实值（GPT-5.6 为
   `none/low/medium/high/xhigh/max`）；Codex CLI 的 `ultra` 不能自动泄漏到
   CX2CC Responses。未知值在透传链路中保留或安全地标记 unknown，不能静默降级
   为另一个等级。
4. context window 的查询键是稳定 provider identity 加最终 remote model ID，
   不是 Claude 槽名、默认 gpt-5.4 或 UI 标签。只有
   `capabilities_configured=true`、`stale=false` 且有合法窗口的目录项才是
   confirmed。未知、过期、自定义和目录不可用必须保持 unknown。
5. 指定 Codex source 时可以计算精确窗口。当前 AIO Codex 网关有动态 failover
   候选：所有候选同一 confirmed 窗口才报告 exact；已知窗口不同取最小值并标记
   mixed；任何候选 unknown 时不得宣称 exact。请求级实际 attempt 的 provider/model
   是最终权威，启动级 projection 不能伪造单一值。
6. 合法的“当前 AIO 服务 Codex 网关”CX2CC 再入口只在精确的内部意图下放行：
   一次性 capability/nonce、短 TTL、绑定当前 gateway authority、POST
   `/v1/responses`、trace/provider identity 和 hop budget=1。入口消费 capability
   并剥离内部头；重放、第二跳、不同 authority/path/method、外部自引用仍由
   `validate_gateway_target`/recursion guard 拒绝。普通 self-loop 校验继续启用。

## 数据流与实现分层

### A. 路由准备

在 `prepare_provider` 中先判定 `cx2cc_active`/bridge type，再决定是否解析
`configured_model_route`。CX2CC 分支传入 no target/no rewrite，并保留已有 provider
enabled、credential、health、failover 及（若存在）eligibility/range 筛选。不要
修改通用 resolver 的语义，也不要用 `managed_model_route=true` 伪造审计标签。
更新旧的“CX2CC 会重写成 bridge target”测试，新增普通 provider 正常重写回归。

### B. 思考透传

在 Anthropic inbound 解析顶层 request configuration，使用现有 IR metadata 或
等价的 typed helper 表达：字段缺省、显式 disabled、显式 enabled/adaptive、
`output_config.effort` 字符串和未知未来值。不要从历史 assistant thinking 内容块
推断本次请求设置。Responses outbound 只在 request metadata 明确存在时写入同值；
明确 disabled 时抑制 settings fallback；无 metadata 时不注入 CX2CC effort。删除或
绕过 `apply_cx2cc_request_settings` 中固定 reasoning 覆盖，保留非思考 settings。
新增 inbound -> IR -> outbound 单测及 absent/disabled/unknown 回归。

### C. GPT-5.6 目录与 UI

建立单一的 CX2CC Responses 模型常量/选项源，至少包含
`gpt-5.6`、`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`，并保留已有
`gpt-5.4`/`gpt-5.5` 和手动/未知值。不要把该列表塞进动态 Codex app-server
catalog。设置页不再呈现会误导用户的固定 thinking injection 控件；若为兼容保留
控件，必须明确它只编辑 legacy persisted field，运行时不覆盖请求。新增/空白配置
的默认值由前端和 Rust 统一，已有持久化值不做破坏性迁移。

### D. context projection

添加窄的 provider-scoped resolver：输入 `(provider_id, remote_model_id)` 或候选
集合，读取 `provider_models` 的 configured/fresh/context 字段并返回
`Exact | Mixed(min) | Unknown`（命名可遵循现有类型）。CX2CC preparation 在
translation 后暴露最终 model 和 source identity；不要按槽位猜窗口。Claude terminal
launch context/settings JSON 只在 resolver 给出确认或明确保守值时注入
`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 与 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`，并保留
caller model/mapper 语义；不要伪造 `claude-*` ID、响应头或固定默认容量。若当前
launcher 无法在进程启动时知道请求级 model，返回 unknown 并在日志/诊断中说明，
请求级 attempt 仍重新解析。测试指定 source、同窗、混窗、unknown、stale 和目录
不可用；不能以 app-server raw catalog 的缺失 context 字段作为能力来源。

### E. 内部再入口

优先复用现有 attempt/provider context 和 gateway runtime state，新增类型化
`InternalCodexReentry`（或同等命名）及一次性 token registry。token 由 CX2CC
source=None 的唯一准备分支生成，在 target validation 处以精确 authority/path/
method/hop 校验并消费。网络头只用于内部 hop，入口 middleware 消费后移除；不能
让任意客户端构造该头。测试合法入口、普通 self-loop、不同 path/method、过期、
重放和第二跳。

## 兼容、失败与回滚

- settings schema 保持向后兼容；legacy effort 字段不删除，不触发全量迁移。
- context unknown 时不把任意已知窗口复制到另一个型号；沿用 Claude Code 安全的
  默认压缩行为或显式保守上限，并报告 unknown。
- capability registry/内部 token 生成失败时 fail closed，合法 CX2CC 请求可
  走已有可观测错误而不是放开所有 self-loop。
- route/effort 变更仅影响 CX2CC；普通 route 回归测试失败即阻止发布。
- 若跨层实现无法在当前启动模型下精确投影，保留 resolver 和诊断接口，宁可返回
  unknown，也不在 beta 中宣称错误容量。

## 验收测试矩阵

| 层 | 正例 | 反例/回归 |
| --- | --- | --- |
| route | CX2CC mapper model 直达 wire | configured target 不改 CX2CC；普通 provider 仍改 |
| thinking | effort、enabled、disabled、未知值保持 presence/value | settings legacy effort 不覆盖；历史 thinking 不冒充配置 |
| UI/catalog | 四个 GPT-5.6 ID 可选，旧值仍可编辑 | 不把 `ultra` 当 Responses effort；动态 catalog 不被硬编码污染 |
| context | source exact、同窗 exact、混窗 min/mixed | stale/unconfigured/unknown/catalog unavailable 不冒充容量 |
| loopback | 精确内部一次跳转成功 | 直接 self-loop、重放、第二跳、错误 authority/path/method 拒绝 |
| release | 生成绑定和 beta manifest 可复现 | stable latest/Homebrew/非目标平台不被改写 |

