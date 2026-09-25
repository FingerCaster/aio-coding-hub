# 执行计划：选择性整合上游功能

## 顺序与检查点

1. **锁定基线与范围**
   - 记录 upstream/fork/base SHA，确认 `upstream` 无 push URL，审计活动与归档 Trellis 任务重复项。
   - 保留工作区已有的 Astra 任务规划文件，不修改或纳入本任务提交。
   - 复核 `upstream-commit-inventory.md`、`model-routing.md`、`remaining-features.md` 和最终 PRD。

2. **普通模型路由兼容适配**
   - 抽取上游 ProviderModelPolicy 的 exact/specific/wildcard 匹配、无递归 target 和 Provider eligibility 算法。
   - 将当前 global/Provider 三态规则解析为每次请求的 Provider 有效视图；保持持久化、share/bundle 版本和 reasoning-only 规则。
   - 在 Provider selection 与现有 configured route 的正确边界接入预筛选和一次性 final-wire apply。
   - 验证 `aio/*`、managed Codex alias、CX2CC、普通模型、forced Provider、apply failure、route marker 和 failover 顺序。
   - 先运行路由/domain/gateway focused tests；若数据模型需要迁移，停在设计复核点，不删除旧字段。

3. **Codex discovery 与 OAuth 适配**
   - 将上游 bounded/no-redirect discovery parser、OAuth upstream proxy、Codex dynamic client version/WSL fallback 接到 fork 现有 Provider UUID、manual-row、catalog ownership 和 token flow。
   - 线程 Settings proxy 到 login/refresh/quota/reset/refresh-loop，保持环境变量优先级、self-loop、SOCKS5 local-DNS、flow ownership 和 redaction。
   - 验证新发现失败保留旧 discovered/manual rows，绝不覆盖 managed Profile/catalog 或已运行会话。

4. **低风险后端与工具链适配**
   - Claude direct backup refresh：只改 Claude sync 判定，复用现有 capture/write/rollback。
   - Audit fallback：加入有界 pnpm bulk/OSV fallback，保留 registry、Windows wrapper 与 fail-closed。
   - Cost：统一 TOTAL/f64 聚合和消费者，保留 provider-scoped/raw-client cost 契约；加入 missing output price 与 priority base fallback。
   - Codex service tier：若增加 selector，默认和旧数据均为 Actual 优先。

5. **前端功能适配**
   - Provider route item 增加 locate-card 行为，保留 draft/order/save 语义。
   - Probe dialog 使用现有 availability model/prompt payload 和 IPC，保留 bounded backend validation。
   - 只在 DTO 变化时重新生成 bindings，并更新完整 fixtures/契约测试。

6. **响应整流与安全边界**
   - 接入 Responses input 规范化和新增整流器注册到现有 middleware/failover loop。
   - 保留 thinking/signature/redacted 窄触发、strict phased-message validator、stream firewall、CX2CC one-hop/self-loop 和 final rewrite 优先级。
   - 明确 generic invalid request 无 thinking 线索时不新增重试；Actual billing selector 不影响 transport/route ownership。

7. **整合验证与审计**
   - 运行改动范围匹配的 Rust 单元/集成测试、Vitest、typecheck、lint、Rust fmt/check/clippy、generated bindings/spec-link/CI contract 检查。
   - 运行 `git diff --check`，检查无 upstream push URL、无意外 schema/release/dependency 文件和无重复 active/archive task。
   - 将 pinned upstream defect 与本次冲突回归分开记录；全部通过后才进入 finish/commit。

## 必须覆盖的验证场景

- 路由：global 继承、Provider 专属、明确关闭、exact/通配符优先级、无递归、reasoning-only、forced Provider、A→B failover、apply failure 零上游请求、普通 Codex 与 `aio/*`。
- Codex：稳定 provider/model UUID、manual/discovered rows、dynamic discovery 失败保留旧值、managed catalog/profile 事务、context rules、new-session-only 行为。
- OAuth：配置代理、环境变量覆盖、SOCKS5、self-loop、运行时代理变化、登录/刷新/额度/reset、秘密与 flow capability 脱敏。
- Claude/audit/cost：端口变化与 direct backup、pnpm registry/Windows wrapper/bulk/OSV fallback、超 i64 聚合、零/负值处理、缺失输出价、Actual priority billing。
- UI/probe：定位卡片清筛选、缺失 Provider no-op、模型/提示词默认/空输入/动作 guard、现有 availability model 持久化。
- 整流：安全 input normalization、窄 thinking-signature 触发、generic 400 不触发、stream/CX2CC/strict validator 回归。

## 风险与回滚点

- 路由预筛选可能改变 Provider 集合和 failover 顺序；若 focused route tests 显示现有配置无法等价表达，回到设计复核，不删除旧配置。
- OAuth proxy 线程涉及多条命令和 refresh loop；任何身份、secret 或 self-loop 回归都阻止继续。
- f64 聚合必须一次更新所有 SQL/Rust/DTO 消费者；出现类型截断或成本漂移时整片回滚。
- 上游整流器不能覆盖 fork 的 stream firewall 或 phased-message strictness；普通 400 重试增加即视为失败。
- 不回滚或覆盖其他活动任务的未跟踪规划文件。

## 提交前门槛

- `prd.md`、`design.md`、`implement.md` 已完成并通过人工规划复核。
- `implement.jsonl` 与 `check.jsonl` 各含真实 spec/research 条目并通过 `task.py validate`。
- 用户明确批准本规划后才运行 `task.py start`、派发实现或编辑产品代码。
