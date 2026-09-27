# last_outcome 覆盖 rewrite：**并非缺陷，是契约既定行为**（结论已更正）

> 本文件由主 agent 补写（原定负责此项的 `research-outcome` 未产出任何文件）。
>
> **2026-08-24 重大更正**：本文件初版把「`last_outcome` 被后续 attempt 覆盖导致 rewrite
> 丢弃」判定为**缺陷**并给出方案 A。该判定**错误**。据此实现的步骤 3 在 Gate 3 完整回归中
> 撞上固化契约测试失败，已全部回退。错误根因与正确结论见第 3、7 节。**下文凡涉及「缺陷」
> 「修复」的措辞均已作废，仅事实性核查（第 2 节表格、6.1-6.4）仍然有效。**

## 1. 现象复述（事实，非缺陷）

`failover_loop/response/finalize.rs:266-268` 仅从 `last_outcome` 取
`error_response_rewrite`：

```rust
let resp = last_outcome
    .as_ref()
    .and_then(|outcome| outcome.error_response_rewrite.as_ref())
```

取不到则落入 `:275-287` 的兜底，硬编码 `StatusCode::BAD_GATEWAY`（`:265`）。

**这是有意设计**：最终响应只投射**最后一次**失败证据，早期 attempt 的规则改写不跨越后续
不同失败存活。见第 3 节。

## 2. 九个 `last_outcome` 赋值点的性质（已证实，事实有效）

| 位置 | 带 rewrite | 场景 | 路径性质 |
| --- | --- | --- | --- |
| `response/upstream_error.rs:861-863` | ✅ | 上游 4xx/5xx 错误响应 | 失败 |
| `response/thinking_signature_rectifier_400.rs:583` | ✅ | 400 签名修正 | 失败 |
| `attempt/attempt_record.rs:203` | ❌ | 传输层失败（连接/超时） | 失败 |
| `attempt/retry_engine.rs:333` | ❌ | 重试引擎失败（**带守卫，见 6.1**） | 失败 |
| `response/success_non_stream.rs:1213` | ❌ | `quota_exhausted`，标 `ProviderError` | 失败 |
| `response/success_event_stream.rs:684` | ❌ | pre-commit 流式失败 + failover 决策 | 失败 |
| `response/success_event_stream.rs:874` | ❌ | 流式终态失败，标 `ProviderError` | 失败 |

未带 rewrite 的 7 处均经 `AttemptOutcome::new` 构造，其
`error_response_rewrite` 恒为 `None`（`failover_loop/context.rs:393`）。

**结论（已证实）：七个不带 rewrite 的赋值点全部位于失败路径，没有一个是真正的成功路径。**
尽管两个文件名含 `success_`，其赋值点本身处理的都是失败分支（前者 quota 耗尽、后者流式
终态失败，均显式标记 `ErrorCategory::ProviderError`）。

## 3. 契约语义：覆盖行为是**被明文要求且被测试固化**的（更正后的核心结论）

初版只查了契约第 99 行「A later success clears terminal rewrite candidates」，据此认为
「成功走 `LoopControl::Return` 永不抵达 `all_providers_failed`，故该条由控制流自动满足，
改成保留最后一个非 `None` 的 rewrite 不破坏契约」。这条推理本身没错，**但漏读了同一段落
紧随其后的另一句**：

`.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:199-201`

> A later success clears terminal rewrite candidates. Stream retries retain
> bounded early evidence in the Provider chain; **final failure projects the
> last evidence.**

「final failure projects the last evidence」正是覆盖行为的规范表述：**最终失败投射的是
最后一次证据**，而不是「最后一次命中规则的证据」。

该语义另有**固化测试**把守，`routes.rs:10828`
`mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`：

- 规则配 `500 → 503`，改写文案字面写着 **`"must not survive a later different failure"`**
- 供应商 A 返回 500（命中规则），供应商 B 返回 502（不命中），`max_providers_to_try = 2`
- 断言（`:10883-10886`）：客户端得 **502**（不是 A 的 503）、日志 `status = 502`、
  **`!has_upstream_error_response_rule_marker(&log)`**

即测试用命名与文案双重方式声明：早期 rewrite **不得**存活到后续的不同失败。

**推论：初版第 3 节「不会破坏契约」的结论作废。方案 A/B/C 三者都会违反契约第 200 行并
弄挂上述测试。**

## 4. 所谓「丢失路径」的真实语义（更正）

```
供应商 A：上游返回 502 → match_response_rule 命中 → rewrite 写入 last_outcome
        ↓ FailoverDecision::SwitchProvider
供应商 B：传输层连接失败 → last_outcome 被 rewrite=None 覆盖
        ↓ 供应商耗尽，循环结束
mod.rs:401 → all_providers_failed → finalize.rs 取不到 rewrite → 兜底 502
```

初版称此为「A 的规则改写被丢弃」的缺陷。更正：**这正是契约要求的结果**——最终失败是 B 的
传输层失败，客户端应当看到反映 B 的响应，而不是 A 的历史改写。运营者为 502 配的规则针对的
是「上游确实返回 502」这一事实，B 从未返回 502。

## 5. 修复方案（整节作废）

初版的方案 A（finalize 保留最后一个非 `None` 的 rewrite）、方案 B（7 个赋值点透传）、
方案 C（改 `AttemptOutcome::new` 签名）**全部作废**：三者都在改变契约第 200 行规定的语义。

若将来确有运营诉求要「早期改写优先」，那属于**契约变更**，须先改
`upstream-error-handling-contract.md` 与上述固化测试，并在 PRD 层面给出理由，
不能作为本任务的顺带修复。本任务范围内**不动** finalize 的取值逻辑。

## 6. 补验结论（主 agent 后续核查，事实均有效）

### 6.1 `retry_engine.rs:333` 实际不会覆盖（修正第 2 节）

该点带 `is_none()` 守卫（`attempt/retry_engine.rs:332-334`）：

```rust
if loop_state.last_outcome.is_none() {
    *loop_state.last_outcome = Some(AttemptOutcome::new(category.as_str(), error_code));
}
```

故它只在 `last_outcome` 为空时赋值，不会擦除已有 rewrite。第 2 节表格中该行应视为
「不构成覆盖」。实际构成覆盖的赋值点为 6 个，而非 7 个。

### 6.2 probe 路径参与赋值

`upstream_error.rs` 的 `probe_active`（`:391`）未用于保护 `:861` 的 `last_outcome`
赋值，故探测路径同样会写入 rewrite。这是**现状既有语义**，本任务不改写入时机。
`:830-835` 显示探测失败仍正常记录 attempt（`probe: true`、`probe_result: "failed"`）。

### 6.3 `success_event_stream.rs:874` 位于 pre-commit（关键，设计前提成立）

该行所属函数为 `finalize_buffered_stream_error_response`（旧基线 `success_event_stream.rs:701`）
——「buffered」即缓冲阶段，响应头尚未发出。研究文件
[[downstream-commit-boundary]] 亦证实该函数是 pre-commit 的「保留 200 但清空 body」
完整改写路径。新基线对应点为 `:970` / `finalize_sanitized_stream_terminal`，
经 `:872` 的 `"...terminated before response commit"` 与 `:874` 的
`set_disposition("sanitized_before_commit")` 重验，**仍是 pre-commit**。

**结论：`design.md` §3/§4 的两侧划分前提成立。** 这是本文件唯一被步骤 4/5 依赖的结论，
不受本次更正影响。

### 6.4 现有测试的覆盖范围（事实有效，推论已更正）

仓库已有两个相关测试：

- `routes.rs:7617` `upstream_error_response_rule_rewrites_direct_abort_after_original_attempt_audit`
- `routes.rs:7665` `upstream_error_response_rule_rewrites_last_all_failed_attempt`

两者共用 harness `run_codex_error_response_rule_route`，硬编码
`failover_max_attempts_per_provider = 1` 与 `failover_max_providers_to_try = 1`
（`routes.rs:1919-1920`），即单供应商、单尝试。

测试名中的「last all failed attempt」指「最后一次（也是唯一一次）失败尝试」，验证 rewrite
能在 `all_providers_failed` 路径生效。

**初版推论「『仅取 last_outcome』非有意设计」已作废**：多供应商语义并非无人覆盖，而是由
第 3 节的 `mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
在另一个位置固化。**只查这两个同名测试、未全仓搜索规则相关断言，是本次误判的直接操作原因。**

### 6.5 已完成核查

- `UsageSseTeeStream` 的 `B: From<Bytes>` bound 已实测编译通过（见 `implement.md` 步骤 0.4）。

## 7. 误判复盘（供后续研究引以为戒）

| 环节 | 发生了什么 |
| --- | --- |
| 触发 | 负责 outcome 研究的子 agent 未产出文件，主 agent 手写替补 |
| 漏读 1 | 契约只读到第 99 行的 success 清空条，未读第 200 行的 `final failure projects the last evidence` |
| 漏读 2 | 只按测试名搜 `upstream_error_response_rule_*`，未按行为搜 `has_upstream_error_response_rule_marker` 的**否定断言**，因而漏掉 `routes.rs:10828` |
| 放大 | 「非有意设计」的结论被写进 `design.md` §5 与 `implement.md` 步骤 3，进入实现 |
| 拦截 | Gate 3 完整回归（2993 passed / 1 failed）撞上该固化测试 |
| 处置 | 主 agent 未改动既有测试去迎合实现，如实上报冲突；运营者选择回退步骤 3 |

**方法论结论**：判定「某行为非有意设计」之前，至少要做两件事——(a) 通读契约相关段落的
**整段**而非命中的单行；(b) 用**行为断言的标识符**（此处为 marker 函数名）全仓搜索反例，
而不是只搜与该功能同名的测试。
