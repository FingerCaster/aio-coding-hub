# 设计：选择性整合上游功能

## 目标与固定边界

上游快照固定为 `420e9958091ae460d152a508b1eb0e2110ab733b`，fork 基线为 `270b808c678af99c5d1ee4535071acae0adbb465`，共同祖先为 `4f02ba3d`。整合采用逐功能适配，不执行整棵 `upstream/main` 合并；`upstream` 保持 fetch-only，仓库操作默认使用 `origin`。

普通模型路由采用上游的匹配和 Provider 预筛选行为，通过兼容适配接入当前 fork 的持久化契约。当前全局规则、Provider 的继承/专属/明确关闭、target 与 reasoning-only 规则继续可读写；不迁移为上游破坏性 `model_policy_json`，不静默删除用户配置。规则仍以不可变客户端模型匹配一次，目标不递归回匹配器；`aio/*` 和 managed Codex route 在上游策略前后都绕过普通路由。

Codex 的 Provider 模型稳定身份、动态/手工目录、受管 Profile、`aio/<profile_name_key>` 与 UUID 兼容别名、目录所有权/恢复、能力和上下文规则继续由 fork 的 catalog/profile 事务负责。上游普通模型映射可以影响普通 Provider 选择，但不能创建或接管 fork 的 `aio` Provider、Profile 文件、生成目录或 Codex-home 绑定。

## 数据流与模块边界

```text
client model
  -> managed Codex alias resolution (aio/*: fork-owned, one Provider)
  -> upstream-compatible Provider eligibility/pattern matcher
       (adapted from ProviderModelPolicyV1; fork policy remains source of truth)
  -> existing provider gates/order/circuit/account/session checks
  -> provider preparation and RequestBeforeSend
  -> configured route adapter applies target + reasoning atomically
  -> final URL/body/fingerprint/transport
  -> attempt evidence, request logs, pricing and UI projections
```

兼容适配把当前 global/Provider policy 展开为每个候选 Provider 的有效上游匹配视图，但不把展开结果持久化。Provider 的明确关闭在预筛选阶段阻止普通映射；继承和专属规则分别解析为有效视图。reasoning-only 规则在上游模型目标映射后由 fork 的协议转换层继续处理。任何 apply 失败仍沿用 fork 的本地路由失败语义：不发送上游请求、不污染健康/熔断/Session，继续当前 Provider 失败转移契约。

OAuth/API-key 动态发现只作为编辑器或刷新输入，复用 bounded/no-redirect/parser 行为；Codex 持久化仍使用 fork 的 `(provider_uuid, model_uuid, remote_model_id)` 和 connection snapshot。上游 WSL 版本探测仅影响 Windows Codex OAuth discovery 的版本信息，不改变 alias、Profile 或 catalog 身份。

## 选择性功能适配

- `def1060c`：在 fork 的 Claude proxy 同步快照与回滚体系中加入与端口无关的 managed 判断，刷新 direct backup；不改 Codex catalog 生命周期。
- `3758d8e8`：保留 fork 的 `PNPM_AUDIT_REGISTRY`、Windows `pnpm.cmd` 解析和 fail-closed 约束，只加入有界 bulk 重试及 OSV fallback。
- `ab83c23f`：把 SQLite 聚合和 Rust 消费链统一改为 `TOTAL()`/`f64`，保持 provider-scoped cost basis 与 raw/client usage 分离。
- `85253db0`：只适配 missing output rate 与 priority base-rate fallback 的后端费用语义；保留 fork 增量 request-log/trace 刷新。
- `3bd9bb59`：把 Settings → upstream proxy 线程到 OAuth 登录、刷新、额度、reset 和 refresh loop，保留显式环境变量优先级、自环拒绝、SOCKS5 local-DNS 与 secret redaction。
- `9234280f` / `37319565`：保留 fork 现有调用顺序持久化，只移植定位 Provider 卡片动作。
- `b3343335` / `b34fe58a` / `867a0db3`：以 fork 现有 `availability_test_model` 和 bounded probe IPC 为数据源增加模型/提示词对话框，不导入上游 model-policy 类型。
- `e2d03792`：选择性接入安全的 Responses input 规范化与整流器注册；thinking-signature 仍要求 fork 的 thinking/signature/redacted 窄触发。Codex priority selector 可接入解析层，但默认仍为 `Actual` 优先。

已经由 fork 覆盖或明确排除：`6007d7a0` 的旧 reasoning/price alias 设置、上游 Codex catalog event/race/discovery 系列、数据库迁移/发布/依赖元数据、已回滚的 response-validation/provider-failover 功能和与 fork README 品牌不一致的发布元数据。

## 兼容、迁移与失败恢复

不新增破坏性 Settings/SQLite schema 迁移来承载上游 `model_policy_json`。已有 settings schema 57、SQLite 45、Provider share/config bundle 版本和 generated bindings 保持兼容；如新增 OAuth proxy、billing selector 或 probe payload 字段，使用当前 settings ownership、版本化 DTO、导入清理和绑定生成流程。旧数据缺失新可选字段时使用 fork 当前默认：Actual billing、窄整流和无新增普通路由。

每个适配按现有锁和事务边界执行：Provider/catalog 生命周期锁、settings write lock、OAuth flow ownership、request-log attempt ownership 和 config import CAS 不改变。准备失败在任何外部写入前返回；部分文件、catalog、backup 或 settings 写入失败按现有 committed-token/CAS 补偿，不能覆盖并发更新。新增路由只在最终 URL/body 校验成功后提交 marker 和 audit evidence。

整合任务只记录固定上游中独立存在的缺陷，不在此任务修复；若冲突解决导致回归，才在本任务内修复。任何 upstream-origin finding 另开任务。

## 重要取舍

- 保留 fork 的 global/Provider/effort 配置，接受上游 Provider 预筛选会影响可用 Provider 集合和 failover 顺序。
- 保留 Actual priority billing，避免历史日志和费用重算变化。
- 保留窄 thinking-signature 触发，避免 generic 400 被新增重试吞掉。
- 保留 `aio/*` 精确 managed route 与当前 Codex 多模型目录，普通上游路由不得覆盖它们。
