# Provider selection 产品冲突决策简报

## Evidence

- 固定 merge：`9e5da3461e2db200a488cef17ac85ecd52c0d6e2`。
- fork parent：`4499c71d17e3d51544e57fdebabb1831b9676d37`。
- fixed upstream parent：`419086fb36a4976e30d384add2fec086d99e648c`。
- 冲突文件：`src-tauri/src/gateway/proxy/handler/provider_selection.rs`，并影响
  `middleware/provider_resolution.rs`、failover common gate、attempt/route projection 与最终 503。
- 当前 merge tree 选择了 fork parent 的 common-gate 行为；历史材料没有可证明的用户产品决定。

## Option A: 保留当前 common-gate 行为

- session-bound provider 仍在 eligible candidates 中但 circuit/cooldown 拒绝时，
  `resolve_session_bound_provider_id` 返回无 preference，不从 providers 删除。
- 后续统一 `run_gates` 产生一个 `outcome=skipped` attempt 与 route hop，零 upstream call；绑定只在
  provider 不再 eligible 时清除。
- skipped 不消耗 Ready-provider budget；若其余 provider 也不可用，最终统一返回 503，并保留所有
  被拒候选的 attempt/route 证据。
- 与当前 cross-layer `gateway-failover-route-contract.md`、前序多供应商 503 修复及“common gate 是
  deny 决策唯一 owner”一致。

## Option B: 采用 fixed upstream DeniedByCircuit 行为

- `resolve_session_bound_provider_id` 接受 `&mut Vec`，circuit 拒绝时立即 `retain` 删除 bound provider，
  返回 `SessionBoundResult::DeniedByCircuit { provider_id, snapshot }`。
- `middleware/provider_resolution.rs` 消费专用 outcome；该 provider 不再进入后续 common gate。
- session-bound circuit 拒绝可更早形成专用诊断/失败路径，但 common-gate attempt/route 中不会自然出现
  该候选；需要明确决定是否另行投影，否则 attempt 数、route hop 和切换展示与 A 不同。
- 单 provider 或剩余候选为空时可能更早结束为 503；多 provider 情况下剩余候选继续的入口和最终 503
  证据形状取决于 provider-resolution 的专用分支，而非统一 gate。

## Recommendation

推荐 A。理由不是“fork 优先”，而是当前已验收产品契约要求所有 eligible candidate 经过同一 gate，
并把 circuit/cooldown 拒绝记录为 skipped attempt/route；这直接修复了用户观察到的 503 证据缺失。
选择 B 也可行，但必须同步重定义 attempt/route 与最终 503 的公开诊断语义，不能只搬入 enum/retain。

## User Decision

用户已明确选择 **A**。实施约束如下：

- 保留当前 common-gate 行为，不引入 upstream `DeniedByCircuit`/`retain` 提前移除。
- session-bound provider 的 circuit/cooldown denial 由统一 gate 产生 skipped attempt/route。
- skipped 后继续其他候选且不消耗 Ready-provider budget。
- 全候选不可用时最终 503 必须保留完整 attempt/route 诊断。
- 临时 denial 不清除 session binding。

该决定解决唯一产品门；后续无需再次询问，按 round-3 严格串行实现与验证。
