# 执行计划：流式故障接入上游错误拦截规则

依据 [[prd]] 与 [[design]]。每步含验证命令与回滚点。**未经步骤 0 的补验，不得进入
步骤 3 之后的实现**。

> **基线声明**：实现在 worktree `intercept-stream-error-rules`（分支
> `FingerCaster/intercept-stream-error-rules`，base `origin/main` = `64f2c6a0`）进行。
> 本文行号已按 [[baseline-migration-64f2c6a0]] 更新为新基线；遇冲突以该文件为准。
> 该文件已重验：架构、P1 注入方案、方案 A 修复均无需变更。

## 验证命令基线

```bash
# 前端
pnpm typecheck
pnpm lint
pnpm format:check
pnpm test:unit
pnpm build
pnpm check:spec-links

# 后端（与 CI 对齐，注意 --test-threads=1）
cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked -- --test-threads=1
```

迭代期可用窄范围加速，但**每个 Gate 必须跑上述完整基线的对应部分**：

```bash
cd src-tauri && cargo test upstream_error_response_rules --locked -- --test-threads=1
cd src-tauri && cargo test usage_tee --locked -- --test-threads=1
cd src-tauri && cargo test failover --locked -- --test-threads=1
```

---

## 步骤 0：补验研究未覆盖项 `[0.1-0.3 已完成]`

`research/last-outcome-rewrite-loss.md` 第 6 节与 `design.md` §5、§6 标注了四项未核查
事实。**0.1-0.3 已于规划阶段完成核查，结论均支持现有设计**，详见该文件第 6 节。

- [x] 0.1 probe 路径参与 `last_outcome` 赋值（`upstream_error.rs:391` 的 `probe_active`
      未保护 `:861`），但属现状既有语义，方案 A 不改写入时机故不引入新风险。
      **额外修正**：`retry_engine.rs:332-334` 带 `is_none()` 守卫，实际不会覆盖已有
      rewrite——真实覆盖点为 6 个而非 7 个。
- [x] 0.2 旧基线 `success_event_stream.rs:874` 属 `finalize_buffered_stream_error_response`
      （`:701`），位于 **pre-commit**。**新基线已重验**：对应赋值点 `:970` 改属
      `finalize_sanitized_stream_terminal`（`:842`），函数名不含 "buffered" 一度疑似
      post-commit，但 `:872` 的 `"...terminated before response commit"` 与 `:874` 的
      `set_disposition("sanitized_before_commit")` 证实仍是 **pre-commit**。
      `design.md` §3/§4 两侧划分前提**成立**，无需退回修订设计。
- [x] 0.3 现有两个 rewrite 测试（`routes.rs:7617`、`:7665`）共用 harness，硬编码
      `max_attempts_per_provider = 1` / `max_providers_to_try = 1`（`routes.rs:1919-1920`），
      为单供应商单尝试。
      **⚠ 2026-08-24 更正**：本条初版据此推出「『仅取 `last_outcome`』非有意设计」，
      **该推论错误**。多供应商语义由另一处固化测试
      `routes.rs:10828 mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
      把守（改写文案字面写着 `"must not survive a later different failure"`），且契约第 200 行
      「final failure projects the last evidence」明文要求覆盖。**只按测试名搜同名测试、
      未按行为断言标识符 `has_upstream_error_response_rule_marker` 全仓搜反例，是误判的直接
      操作原因。** 步骤 3 已因此全部回退。
- [x] 0.4 `B: From<Bytes>` bound **编译通过**（`cargo check --locked` 13.94s，零错误零
      警告）。已对 `UsageSseTeeStream` 的 7 处 where 子句（`usage_tee.rs` 行
      290/309/625/640/647/663/673）加 bound；`Bytes` 已在 `:5` 导入。注意另有 8 处
      `B: AsRef<[u8]>` 属 `UsageBodyBufferTeeStream` 等无关类型，**不得**全局替换。
      **首选方案可行，无需退回 `Bytes` 特化层。此改动即步骤 1.6，予以保留。**
- [x] 0.5 `success_event_stream.rs:1593` 位于 `handle_infinite_buffered_event_stream`
      （`:1279`）中 `builder.body()` **构造响应失败**的分支，错误码为
      `GW_RESPONSE_BUILD_ERROR`（非 StreamError）。响应对象未构造成功即未发头，属
      **pre-commit**，且不在本任务注入范围。
- [x] 0.6 `usage_tee.rs` 四个 StreamError 产生点归属已确认：
      - `:454` `poll_next_inner`(:353)，origin `TerminalFrame` — post-commit；上游已发
        error 帧、客户端已收到，通常无需再注入（步骤 4 需据此判断是否跳过）
      - `:509` `poll_next_inner`(:353)，origin `UpstreamReadError` — post-commit，
        **主注入点**（design P1）
      - `:1059` `spawn_usage_sse_relay_body`(:765) — post-commit，codex relay 分支
      - `:1404` **`UsageBodyBufferTeeStream`**(impl :1326) — **非 SSE 路径，不适用注入**

**Gate 0（已通过）**：六项全部完成。0.4 证实首选类型方案可行；0.2 在新基线重验后两侧
划分前提仍成立；0.5/0.6 已在进入步骤 4 之前完成，注入点与跳过条件均已明确。

**回滚点**：0.1-0.3、0.5、0.6 无代码改动；0.4 的 bound 改动保留为步骤 1.6
（备份见 `/tmp/usage_tee.bak`）。

---

## 步骤 1：可见性与类型改造 `[已完成 · Gate 1 通过]`

- [x] 1.1 `proxy/mod.rs:30`：已改为 `pub(in crate::gateway) mod upstream_error_response_rules;`
      （同文件 `:29` 的 `upstream_client_error_rules` 已是此写法，照既有先例）
- [x] 1.2 `upstream_error_response_rules.rs`：`UpstreamErrorResponseRewrite`(:16)、
      `build_response`(:30)、`special_setting`(:74)、`match_response_rule`(:174) 已放宽为
      `pub(in crate::gateway)`；`client_status`(:22) 字段同步放宽。
      **有意保持不变**：`message` / `retry_after` 仍为私有，其余字段仍 `pub(super)`——
      streams 侧只通过方法访问，无需暴露内部字段（见步骤 2 的伪 body 设计）。
- [x] 1.3 `streams/types.rs`：`StreamFinalizeCtx` 已新增
      `upstream_error_response_rules: Vec<crate::settings::UpstreamErrorResponseRule>`，
      带 `#[allow(dead_code)]` + `TODO(step-4)`（字段要到步骤 4 才被读取，否则
      clippy `-D warnings` 不过）。**步骤 4 必须移除该 allow。**
- [x] 1.4 构造点已补齐：生产点 `context.rs` 的 `build_stream_finalize_ctx` 填入
      `ctx.upstream_error_response_rules.to_vec()`；三处测试构造点补 `Vec::new()`
      （`streams/finalize.rs:345`、`streams/request_end.rs:428`、`streams/usage_tee.rs:1537`）。
      注意 `cargo check` **不含**测试目标，须用 `--all-targets` 才能发现这三处。
- [x] 1.5 **偏离 design §6 的决定（已记录）**：design 原写「在调用处从 `ctx: CommonCtx`
      取，`CommonCtxOwned` 不携带，勿走它」。但 `build_stream_finalize_ctx` 的首参本就是
      `&CommonCtxOwned`，且 `CommonCtxOwned<'a, R>` 已有 `'a` 生命周期。故改为给
      `CommonCtxOwned` 补 `upstream_error_response_rules: &'a [...]` 字段并在
      `From<CommonCtx>` 中透传。**理由**：3 个 `build_stream_finalize_ctx` 调用点
      （`success_event_stream.rs:2305`、`success_non_stream.rs:745/846`）一行都不用改，
      比给函数加参数侵入更小；语义上 owned ctx 携带自身所需配置也更自然。
      design §6 的「勿走它」是基于该字段当时不存在的观察，非禁止扩展。
- [x] 1.6 `B: From<Bytes>` bound 已落地（见 0.4）

**Gate 1（已通过）**：`cargo fmt -- --check` ✅ ；
`cargo clippy --all-targets --locked -- -D warnings` ✅ 零错误零警告（1m19s）；
`cargo test --locked -- --test-threads=1` ✅ **lib 2984 passed / 0 failed / 4 ignored**
（622.66s）+ 全部集成测试二进制通过，退出码 0（未接管道，退出码真实）。运行时行为未变。

> **教训（避免重犯）**：首次跑测试时误用 `cargo test ... | tail -30`，管道使退出码变成
> `tail` 的、并丢弃了 lib 单元测试输出，得到虚假的「exit 0」。跑验证命令时**不得接管道**，
> 或必须显式检查 `PIPESTATUS`。

**回滚点**：R1 — 纯结构改动，可单独 revert。此步不改变任何运行时行为，测试结果应与改动前
**完全一致**。

---

## 步骤 2：`match_synthetic_failure_rule` `[已完成 · Gate 2 通过]`

- [x] 2.1 已在 `upstream_error_response_rules.rs` 实现该函数（签名见 `design.md` §2.2）
- [x] 2.2 伪 body 采 JSON `{"error":{"message":<面向用户文案>,"code":<gw码>,"type":"gateway_stream_failure"}}`，
      **委托**给 `match_response_rule`，未复制匹配逻辑。
      **偏离 design §2.2 的记录**：design 原写伪 body 为
      `"{gateway_error_code} {description}"` 纯文本。改为 JSON 的**理由**：`Passthrough` 时
      `extract_upstream_message` 会取 `error.message`，JSON 让该字段直接是面向用户文案而非
      网关码，S2 语义因此不必依赖后置替换即天然成立；keyword 规则仍能命中 `code` 字段里的
      `GW_STREAM_ERROR`。仍保留一层防御：`passthrough` 且 message 含 gw 码时强制回退固定文案。
- [x] 2.3 `Passthrough` 按 §2.3 **S2**：`StreamError`→固定英文文案、`StreamIdleTimeout`→另一条，
      **不透传** gw 码。`Override` 文案按运营者配置原样输出（含其自填的 gw 码）。
- [x] 2.4 伪 body 仅栈上存活（`synthetic_failure_body` 返回 `Vec<u8>`，用后即弃），不写库。
- [x] 2.5 单元测试（8 条，超出计划 6 条）：
      - `synthetic_stream_error_matches_status_only_rule_on_the_synthesized_502`
      - `synthetic_idle_timeout_matches_on_524_and_not_on_502`
      - `real_upstream_200_never_matches_directly`（含 `match_response_rule` 仍拒 200）
      - `keyword_rule_matches_through_the_pseudo_body`（对照：无 body 时 fail-open）
      - `passthrough_uses_fixed_text_and_never_leaks_the_gateway_code`
      - `override_message_is_used_verbatim_even_when_it_names_the_gateway_code`
      - `only_stream_terminal_codes_enter_synthetic_matching`（allow-list：499 等被挡）
      - `audit_metadata_marks_the_status_as_synthesized`（`special_setting` 新增字段）
- [x] 2.6 **审计标记**：`UpstreamErrorResponseRewrite` 新增私有 `synthetic_error_code`，
      `special_setting()` 在合成路径追加 `syntheticErrorCode` + `upstreamStatusSynthetic:true`，
      真实上游错误路径的审计形状**不变**（`match_response_rule` 写 `None`）。
- [x] 2.7 因步骤 4/5 才接线，`match_synthetic_failure_rule` 及其 3 个 helper、2 个文案常量
      暂加 `#[allow(dead_code)]` + `TODO(step-4/5)`。**步骤 4/5 落地后必须移除。**

**Gate 2（已通过）**：`cargo test upstream_error_response_rules --locked -- --test-threads=1`
✅ 21 passed / 0 failed（含 8 条新测试）；`cargo clippy --all-targets --locked -- -D warnings`
✅ 零警告；`cargo fmt -- --check` ✅。功能尚未接线，运行时不受影响。

**回滚点**：R2 — 新增函数与测试，无调用方，可单独 revert。

---

## 步骤 3：~~`last_outcome` rewrite 丢弃修复~~ `[已实现后全部回退 · 判定错误]`

> **2026-08-24 更正**：本步骤基于 `research/last-outcome-rewrite-loss.md` 初版把覆盖行为
> 判为缺陷，**判定错误**。契约
> `upstream-error-handling-contract.md:199-201` 明文规定
> **「final failure projects the last evidence」**，覆盖是既定语义；且有固化测试
> `routes.rs:10828 mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
> 把守（规则 `500→503`，改写文案字面写着 `"must not survive a later different failure"`，
> A=500 命中 + B=502 不命中 → 断言客户端 502、日志 502、**无** marker）。
>
> **Gate 3 完整回归实测 2993 passed / 1 failed**，失败项正是该测试。未改动既有测试去迎合
> 实现，如实上报冲突后，运营者裁决**回退步骤 3**。误判复盘见研究文件第 7 节、设计作废说明
> 见 `design.md` §5。

- [x] 3.R **回退已完成**（R3），四个文件恢复至 base 状态：
      - `failover_loop/context.rs`：删 `FailoverRunState::last_rewrite_outcome` 字段、注释与
        `new()` 初始化
      - `failover_loop/mod.rs`：删 provider 迭代末尾的抓取块、删 `AllFailedInput` 构造里的该字段
      - `failover_loop/response/finalize.rs`：删 `AllFailedInput` 字段与解构，
        `final_error_code` / `final_error_category` / `resp` 恢复为直接读 `last_outcome`
      - `routes.rs`：删 harness `run_codex_cross_provider_failover_route` 与两条新测试
        （`..._survives_later_provider_transport_failure`、
        `cross_provider_failover_without_matching_rule_keeps_bad_gateway_fallback`）
- [x] 3.R2 `git diff --stat` 确认这四个文件已不在改动清单内；全仓搜 `last_rewrite_outcome` /
      `terminal_outcome` / `run_codex_cross_provider_failover_route` 零命中

**Gate 3（回退后重验，已通过）**：`cargo fmt -- --check` ✅；
`cargo clippy --all-targets --locked -- -D warnings` ✅ 零警告（39.6s）；
`mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider` ✅ 恢复通过；
`cargo test --locked --lib upstream_error_response` ✅ 23 passed；完整
`cargo test --locked -- --test-threads=1` ✅ **lib 2992 passed / 0 failed / 4 ignored**
（716s，= Gate 1 基线 2984 + 步骤 2 新增 8 条，数目精确吻合）+ 全部集成测试二进制通过，
退出码 0（未接管道）。

**副产品（保留）**：回退时 clippy 暴露 `sse.rs` 的 `sse_data_frame`（步骤 4.2 预置）尚无
调用方，按步骤 2.7 既有约定加 `#[allow(dead_code)]` + `TODO(step-4.3)`，**步骤 4.3 必须移除**。

**对后续步骤的影响：无。** 步骤 4/5 的注入与改写发生在单次 attempt 内部（流终止时刻立即
匹配），不经 `all_providers_failed` 的跨 attempt 取值逻辑，与契约第 200 行无交集；用户报告的
原始故障（单供应商流式 502）也不在该场景内。

**若将来确需「早期改写优先」**：属契约变更，须先改契约第 200 行与上述固化测试并单独立项。

---

## 步骤 4：post-commit 流尾注入 `[主战场，覆盖约 93% 故障]`

> **实现设计（预研已确认，2026-08-23）**：
> - **统一注入点**：codex relay（`spawn_usage_sse_relay_body` 的 `next_item` → `poll_next` →
>   `poll_next_inner(cx,true,true)`）与 claude/gemini/grok 直连（`poll_next` 同样
>   `poll_next_inner(cx,true,true)`）**共享** `poll_next_inner` 的 `Err` 分支（新基线
>   `:475-521` 的 else `:507-519`）与 idle-timeout 分支（`:369-385`）。只需在 `poll_next_inner`
>   注入即可覆盖全部四种协议，无需在 relay loop 另写一份。
> - `with_defer_terminal_error`（relay 用）只影响 `Ok(chunk)` 的 terminal_error 分支与 `None`
>   分支，**不影响 `Err` 分支**，两路径注入行为一致。
> - drain 路径（`next_drain_item` → `poll_next_inner(cx,false,false)`）`finalize_terminal=false`，
>   发生在客户端已断开之后，**不注入**（用 `if finalize_terminal` 守卫，语义天然对齐）。
> - codex late error（`is_codex_stream_tail_error_successish`，completion 后）走容错分支
>   `:485-505`，**不进** else 注入分支——已完成的流不需注入，符合预期。
> - relay 若启用 stream_internal_errors firewall，注入帧会被 `firewall.ingest` 摄入，但注入
>   发生在流终止时刻、firewall 后续判定不影响已注入帧；若需隔离再评估 marker 方案（M1 风格）。
> - **帧不入 tracker**：注入前**不**调 `tracker.ingest_chunk`，避免污染 usage/completion 判断
>   （4.8 有专项测试）。

- [x] 4.1 新增 `pending_tail: Option<Bytes>` 与 `stop_after_tail: bool`，**未复用**
      `stop_after_terminal_error`。**外加一个字段 `relay_owns_tail: bool`**（附 builder
      `with_relay_owned_tail()`），原因见 4.4 的 firewall 发现。
- [x] 4.2 已上提 `sse_frame` helper 至 `gateway/proxy/sse.rs`：拆为 `sse_event_frame`
      （`event: <type>\ndata:…`，codex/claude 用）与 `sse_data_frame`（纯 `data:…`，
      gemini/grok 用）；`anthropic.rs:376` 的 `sse_frame` 改为委托 `sse_event_frame`，
      8 个调用点不变。逻辑单一来源，非重复实现。
- [x] 4.3 帧构造落地为 `usage_tee.rs` 的 `synthetic_tail_frame(cli_key, path, payload)`：
      codex `/v1/responses` → `event: response.failed`（文案在 `response.error.message`）；
      claude → `event: error`；codex 非 responses 路径 / grok / gemini → 纯 `data:`。
      **偏离改进**：错误对象本体不在此处手写，而是取
      `UpstreamErrorResponseRewrite::client_error_payload(cli_key)`——该方法从
      `build_response` 抽出，使**流式帧与非流式信封的 error 对象同源**，避免两处形状漂移。
      已有测试断言四协议形状且**均不含 `[DONE]`**。
- [x] 4.4 `Err` 分支 else 注入，严格按 §3.4 时序：`build_synthetic_tail`（内部先
      `match_synthetic_failure_rule` → **再 push 审计 special setting**）→ `finalize(...)` →
      最后交付帧。用 `if finalize_terminal` 守卫，drain 不注入。
      **⚠ 执行期重大发现（design 未预见的深度）**：codex relay 把每个 `Ok(chunk)` 都过
      `CodexTerminalFirewall::ingest`，而 `inspect_frame` 对
      `event: response.failed` 会走 `classify_codex_stream_internal_error` →
      `FrameDecision::DropTerminal`，**注入帧会被"post-commit 丢弃"**，正好在用户报告的
      codex 场景下静默失效。design §3.2 只把这标为「若需隔离再评估」，实测为必然发生。
      **落地方案**：firewall 的职责是审查**上游**帧，网关自造帧不受其管辖。故 relay 模式下
      注入帧改走 `pending_tail`，由 relay 在两个终止臂**直连 `tx` 递送**，绕过 firewall：
      - `Err` 臂：`take_pending_tail()` 命中则发 `Ok(frame)` 替代原 `Err(err)`
      - `None` 臂（idle timeout 的干净 EOF）：`firewall.finish()` 处理完后补发 `Ok(frame)`
      直连路径（claude/gemini/grok）无 relay，仍由 `poll_next_inner` 直接返回 `Ok(frame)`。
      注入帧不计入 `forwarded_chunks/bytes`——该计数衡量的是转发的**上游**字节。
- [x] 4.4b **自查发现并修正一个真实缺陷（我自己的 4.4 实现）**，详见 design §3.2.1.1：
      relay `Err` 臂初版沿用既有形状「`firewall.finish()` fail-closed → `break`」，
      而真实 TCP 中断多切在 **SSE 帧中间**，firewall `pending` 非空 →
      `finish()` 必返回 `Some("partial_frame_at_eof")` → `break` 发生在
      `take_pending_tail()` **之前**，**尾帧被吞**，恰好在主目标场景失效。
      初版桩在**帧边界**截断，故未暴露。
      修正：fail-closed 只记为 `firewall_dropped_tail_bytes` 标志，不再早退；尾帧无条件
      递送；仅「无规则命中 **且** firewall 已 fail-closed」时保持既有静默结束。
      **承重性已实测**：探针打印 `fail_closed_reason == Some("partial_frame_at_eof")`，
      新增的帧中间截断端到端测试通过。
- [x] 4.5 idle timeout 分支（干净 EOF、无 `Err`）同样注入 `GW_STREAM_IDLE_TIMEOUT` / 524
      （PRD R1），已有专项测试。
- [x] 4.6 命中后置 `stop_after_tail`，下次 poll 走 `Poll::Ready(None)` 干净 EOF；
      **全程不返回 `Err`**。
- [x] 4.7 未命中（无规则 / 规则不匹配 / 协议无已知帧形状）时逐字节保持现状：原 `Err`
      透传、原干净 EOF。已有两条对照测试。
- [x] 4.8 测试（5 条，其中 2 条为端到端）：
      `usage_tee.rs` 单元：
      - `synthetic_tail_frame_uses_each_protocols_own_stream_shape`（四协议形状 + 无 `[DONE]`
        + 未知 cli_key 返回 None）
      - `idle_timeout_appends_rule_frame_then_ends_the_body_cleanly`（先 `Ok(frame)` 后
        `None`；文案送达；**不泄露 `GW_STREAM_IDLE_TIMEOUT`**；日志仍记 idle timeout 失败；
        审计元数据含 `syntheticErrorCode` / `upstreamStatusSynthetic` / `upstreamStatus=524`；
        **改写文案未被持久化**）
      - `idle_timeout_without_a_matching_rule_keeps_the_current_silent_eof`（规则只配 502，
        合成 524 不误命中）
      `routes.rs` 端到端（**`reqwest::Error` 无公开构造函数，`Err` 分支只能真机式触发**，
      故新增桩 `spawn_truncating_chunked_sse_upstream`：发头 + 一帧 → 等待网关越过
      pre-commit 窗口并下发 → 不发 `0\r\n\r\n` 直接断连）：
      - `truncated_codex_stream_appends_rule_error_event_instead_of_cutting_off`
        （**用户报告场景的完整复现**：HTTP 仍 200、已发前缀不变、尾部追加
        `event: response.failed` 带运营者文案、不泄露 `GW_STREAM_ERROR`、日志
        `GW_STREAM_ERROR` + status 502、审计标记按合成 502 匹配、文案未持久化）
      - `truncated_codex_stream_without_a_rule_keeps_the_current_truncation`（无规则不回归）
      **补充（4.4b 的复现器，桩加参数 `partial_tail`）**：
      - `mid_frame_truncated_codex_stream_still_gets_the_rule_error_event`（帧中间截断 →
        firewall fail-closed → 尾帧仍须送达；与帧边界版共用同一套断言，客户端可见结果
        **不得**依赖截断落点）
      - `mid_frame_truncated_codex_stream_without_a_rule_still_ends_silently`（无规则时
        firewall 扣下的半帧**不得泄露**、结论仍为 `GW_STREAM_ERROR`）
      **附带发现（既有行为，不改，见 design §3.2.1.2）**：`Err` 路径上
      `stream_terminal_firewall` 审计条目写入晚于 `finalize()`，**不会被持久化**，
      故不对其做断言并在测试注释中写明原因。
      **注**：首版桩无延迟即断连，落在 pre-commit 窗口内返回 502 而非 200——加 700ms
      提交延迟后才真正命中 post-commit 路径。这也侧证了 design §3/§4 两侧划分的真实存在。
      该端到端测试同时覆盖 firewall 旁路：`stream_internal_errors.enabled` 默认 `true` 且
      `disable_upstream_retry_policy` 不触碰它，故测试中 firewall 活跃。
- [x] 4.9 已移除 `match_synthetic_failure_rule` 及其 helper/常量的 `#[allow(dead_code)]`
      （步骤 2.7 约定）、`types.rs` `upstream_error_response_rules` 字段的 allow（步骤 1.3）、
      以及 `sse.rs` `sse_data_frame` 的 allow（步骤 3 回退期临时加的）。

**Gate 4（已通过）**：`cargo test usage_tee --locked -- --test-threads=1` ✅ 32 passed（含 3 条
新单元测试）；针对性 `truncated_codex_stream` ✅ 2 passed；`cargo fmt -- --check` ✅；
`cargo clippy --all-targets --locked -- -D warnings` ✅ 零警告；完整
`cargo test --locked -- --test-threads=1` ✅ **lib 2997 passed / 0 failed / 4 ignored**
（520.91s，= 步骤 3 回退后的 2992 + 新增 5 条，数目精确吻合）+ 全部集成测试二进制通过，
退出码 0（未接管道）。§7 六条不变量各有对应断言。

**回滚点**：R4 — 流式注入独立成块，可 revert 回步骤 2 状态（步骤 3 已回退，不在链内）。

---

## 步骤 5：pre-commit 完整改写 `[已完成 · 落点由三处收敛为一处]`

> **方案修订（2026-08-24，详见 design §4.1）**：原计划「在三个 pre-commit 点判定 `Abort`
> 后各自调 `build_response` 返回」。实测发现 pre-commit 的 `Abort` **并不自建响应**——
> `attempt_record.rs:240` 只返回 `LoopControl::BreakRetry`，最终响应由
> `finalize::all_providers_failed` 统一构造，而**那里已经会应用
> `last_outcome.error_response_rewrite`**。缺的只是「pre-commit 流式失败的 outcome 从不
> 携带 rewrite」这一环。故改为在唯一汇聚点挂钩。

- [x] 5.1 在 `attempt_record.rs` 的 `record_system_failure_and_decide_impl` 里，
      `*last_outcome = Some(AttemptOutcome::new(...))` 处挂钩
      `match_synthetic_failure_rule_by_code`，命中经既有
      `AttemptOutcome::with_error_response_rewrite` 附上。**1 处改动覆盖全部 pre-commit
      流式失败点**（`success_event_stream.rs:1801` / `:1913` / `:2159` 及
      `collect_bounded_final_wire` / `validate_complete_codex_sse` 分支均汇聚于此），
      未来新增点自动覆盖。
- [x] 5.2 完整信封复用既有 `all_providers_failed` → `build_response` 链路，**未新建任何
      响应构造代码**，与非流式最终错误改写同源，形状不会漂移。审计 push 亦由该链路完成。
- [x] 5.3 **PRD R2a 天然满足**：只往 outcome 挂数据，**完全不改 `decision`**；
      `RetrySameProvider` / `SwitchProvider` 照原样重试与切换，只有整个循环真的失败到
      `all_providers_failed` 才用到该 rewrite。初版设计在三点直接返回响应，反而有短路
      failover 的风险。
- [x] 5.4 未降级为「仅追加事件」：pre-commit 得到的是完整信封（状态码 + 文案），已有测试
      断言 `503` + 规则文案 + body 中**不含** `response.failed`。
- [x] 5.5 新增字符串入口 `match_synthetic_failure_rule_by_code`（调用方只有 `&'static str`
      错误码），其 allow-list 由 `as_str()` 反查枚举（`synthetic_failure_code_from_str`），
      **不写第二份硬编码列表**。非流式错误码返回 `None`，其余失败类别行为不变。
- [x] 5.6 端到端测试
      `precommit_stream_truncation_gets_the_full_rule_rewrite_not_a_tail_frame`：
      复用步骤 4 的截断桩但**延迟设为 0**，使断点落在 pre-commit 窗口内 →
      客户端得 `503` + 规则文案、body 无 `response.failed`、日志有 rewrite marker 与
      `syntheticErrorCode=GW_STREAM_ERROR` / `upstreamStatusSynthetic=true`、文案未持久化。
      与步骤 4 的 700ms 版本构成 commit 两侧的对照。

**Gate 5**：`cargo fmt -- --check` ✅；`cargo clippy --all-targets --locked -- -D warnings`
✅ 零警告；针对性测试 ✅ 1 passed；完整 `cargo test --locked -- --test-threads=1` 见下方记录。

**回滚点**：R5 — 单点改动，可独立 revert。

---

## 步骤 6：R10 `terminal_origin` `[已完成 · 前提被实测推翻，改动缩小]`

> **⚠ 2026-08-24 前提更正（同类错误第二例）**：R10 称「日志无法定位终止来源」。**实测为假。**
> 持久化请求日志本就记录 `terminal_origin`——最终写入走 `streams/request_end.rs:293` 的
> `terminal_details_json`（**既有代码**，`git diff` 显示该文件本任务仅 +1 行且是测试固件）。
> `usage_tee.rs` 那一行写的是 `touch_activity` 的**待定行**
> （`status IS NULL AND error_code IS NULL`，`infra/request_logs.rs:566-596`），随后被最终
> 写入取代。
>
> **验证方法（照步骤 3 的教训，先证伪再动手）**：在端到端测试中断言
> `activity_details_json` 含 `terminal_origin` 与 `upstream_read_error` → **临时回退
> `usage_tee.rs` 改动后该断言依然通过** → 前提推翻。

- [x] 6.1 `usage_tee.rs` finalize 内的活动 touch 由 `details_json(terminal_signal)` 改为
      `terminal_details_json(terminal_signal, terminal_evidence)`。
      **保留理由（已缩小到真实价值）**：使**进行中（pending）**活动行与最终行字段一致，
      对「从未完成最终写入」的行（进程异常、reconcile 兜底）有实际价值。
      **不是**原文所称的日志缺口修复。
- [x] 6.2 只新增字段，未改既有 `terminal_signal` 语义与字段名（PRD R10）。
      逐块 touch（`usage_tee.rs:547` 的 `details_json(None)`）仍用非终态形式，语义正确，
      故 `details_json` 未变成死代码。
- [x] 6.3 断言已加入端到端测试
      `truncated_codex_stream_appends_rule_error_event_instead_of_cutting_off`，
      **注释明确标注它守护的是既有保证**，避免后人误读为本步骤的成果。

**Gate 6**：与 Gate 7/8 合并跑完整回归。

**回滚点**：R6 — 纯日志增强，可独立 revert（且回退不影响任何断言）。

---

## 步骤 7：契约与 UI `[已完成]`

- [x] 7.1 改 `upstream-error-handling-contract.md` 第 3 节：原「HTTP 200 stream errors and
      transport errors never enter rewrite matching」改为 allow-list 语义（`GW_STREAM_ERROR`
      按 502、`GW_STREAM_IDLE_TIMEOUT` 按 524；仅此两码可合成；client abort 等一律排除），
      并写入伪 body、`Passthrough` 固定文案、伪 body 不持久化。
- [x] 7.2 改同文件第 4 节矩阵行：
      `Transport failure or HTTP 200 SSE error` → 「仅对上述两码求值；commit 前完整信封、
      commit 后追加尾部事件」。
- [x] 7.3 契约新增一整段记录 commit 两侧差异：commit 前 rewrite 挂 attempt outcome、由
      `all_providers_failed` 构造完整信封且**不影响重试/failover 决策**（R2a）；commit 后
      状态行不可变、仅追加协议合法事件 + 干净 EOF、已发字节不改（R3）；**网关自造帧绕过
      terminal firewall**（该 firewall 只审查上游帧）；attempt status 仍记合成码（R8）；
      审计用 `syntheticErrorCode` / `upstreamStatusSynthetic` 区分合成状态与上游真实状态。
- [x] 7.4 UI 标签更名：`最终 HTTP 错误改写` → `最终错误改写`（tab label + aria-label +
      卡片标题 `最终错误改写规则`），已不止管 HTTP。测试内 8 处引用同步更新。
- [x] 7.5 UI 补充说明三处：
      - 卡片描述：原文「不处理网络失败或 HTTP 200 SSE 错误」**与实现相反**，改为「处理…
        以及流式传输中断（按 502 匹配）与流式空闲超时（按 524 匹配）」
      - 匹配条件区：流式故障按合成状态码匹配（上游真实返回 200）；关键词匹配的是网关故障
        描述而非上游文本
      - 改写行为区：commit 前构造完整响应；commit 后状态码不可改（仍 200）、在流末尾追加
        错误事件、已发内容不改；`Passthrough` 在流式路径使用网关固定文案
- [x] 7.6 前端测试新增
      `explains that stream failures match on the synthesized status and how commit limits rewriting`，
      锁住上述文案，防止 UI 与实现再次相反。

**Gate 7（已通过）**：`pnpm typecheck` ✅、`pnpm lint` ✅、`pnpm test:unit` ✅
**310 files / 2913 tests passed**（含新增 1 条）、`pnpm build` ✅、
`pnpm check:spec-links` ✅。

> **`pnpm format:check` 未通过，但与本任务无关**：唯一告警文件是 `src-tauri/tauri.conf.json`，
> 它**不在本任务改动清单内**（`git status` 无该文件），且 `origin/main` 上的同一文件同样
> 不合 prettier 规范（已实测 `git show origin/main:src-tauri/tauri.conf.json` 后 prettier
> 报同一告警）。属**基线既有违规**，未擅自修改以免混入无关改动。本任务触碰的所有前端文件
> 均已通过 `npx prettier --check src/components/cli-manager/`。

**回滚点**：R7。

---

## 步骤 8：端到端与真机验证 `[代码侧已完成 · 真机项待运营者执行]`

- [x] 8.1 端到端测试：客户端 body 含追加的 error 帧 →
      `truncated_codex_stream_appends_rule_error_event_instead_of_cutting_off`
- [x] 8.2 端到端测试：未配置规则时全链路行为与现状一致 →
      `truncated_codex_stream_without_a_rule_keeps_the_current_truncation`
- [x] 8.2b **补覆盖直连路径（执行期发现的缺口）**：`use_sse_relay` 实为
      `is_codex_responses_event_stream_path`，**仅 codex `/v1/responses` 走 relay**；
      claude / gemini / grok 及 codex 的 OpenAI 兼容路径走**直连** `UsageSseTeeStream`，
      命中 `Ok(frame)` 分支而非 `pending_tail`。原测试只覆盖 relay 分支，已补
      `direct_stream_path_delivers_the_tail_frame_as_a_stream_item`（claude 形状
      `event: error`，先 `Ok(frame)` 后 `None`，不泄露 gw 码）。
- [ ] 8.3 **真机验证 codex CLI 实际解析追加事件——待运营者执行，无法自动化替代。**
      需实际触发一次流式中断，确认 codex 正常展示错误、无协议解析报错、界面不卡在未完成状态。
- [ ] 8.4 **真机验证 524 idle timeout 场景——待运营者执行。**
- [x] 8.5 已复核 PRD 全部验收标准并逐条标注证据；期间发现并纠正一条**与实现相反**的表述
      （原写「attempt 状态保留上游真实状态」，实测 `attempts[0].status == 502` 为合成码，
      属 R8 已接受的既有行为），已在端到端测试中固化该断言。

**Gate 8**：后端 `cargo fmt -- --check` ✅ + `cargo clippy --all-targets --locked -- -D warnings`
✅ 零警告 + `cargo test --locked -- --test-threads=1` ✅ **2998 passed / 0 failed / 4 ignored**
（505.71s，退出码 0，未接管道）；前端 `typecheck` ✅ / `lint` ✅ / `test:unit` ✅
**310 files / 2913 tests** / `build` ✅ / `check:spec-links` ✅
（`format:check` 唯一告警为基线既有的 `src-tauri/tauri.conf.json`，见步骤 7 说明）。
**8.3 / 8.4 未完成：不得以「测试全绿」代替真机确认。**

---

## 步骤 9：独立评审回应 `[2026-08-24]`

独立评审子代理（只读，无写权限）交付 9 条发现。逐条处置如下——**接受 5 条并已改，
驳回 2 条（附反证），2 条记录为超范围缺口**。

### 已接受并修正

| # | 发现 | 处置 |
| --- | --- | --- |
| 应修 1 | `synthetic_tail_frame` 把 Responses 分帧绑在 `is_codex_responses_path`（硬要求 `cli_key == "codex"`），而 **grok 同样说 Responses 协议**（`configured_model_route.rs` 的 `is_supported_inference_request`：`"grok" => is_responses_path(..)`），其 SSE 确带 `event:` 行（既有固件 `mock_runtime_router_grok_responses_sse_is_transparent_and_logged`）。grok + `/v1/responses` 会收到无 `event:`、无 `type` 的裸 `data:`，客户端无从派发 → 截断依旧静默，违反 PRD R4 | 新增 `is_responses_protocol_stream(cli_key, path)`（codex ∪ grok，**路径集合与原函数逐字相同**，不改 codex 行为），`synthetic_tail_frame` 改用它；单元测试补 grok-on-`/v1/responses` 断言 |
| 应修 2 | 审计无条件写 `clientStatus`（规则配置值）+ `scope: "response"`，但 post-commit 客户端状态恒为 200，**规则的状态行为在此路径上是 no-op**；且无字段可区分「完整信封改写」与「追加尾帧」 | 新增 `special_setting_for_stream_tail()`：`scope: "stream_tail"` + `clientStatusApplied: false`。**未按评审建议把 `clientStatus` 改成 200**——见下「驳回」第 1 条。**渲染层一并修正**（见下「评审后追加」） |
| 测试 (a)(b) | 两条 no-rule 测试的可观测结果完全相同，恰在它们本该区分的维度上；且「干净 EOF、绝不 `Err`」这条 design §3.2 的核心保证在端到端层**完全没有断言**（把实现改回「发帧后仍发 `Err`」，四条测试照样全绿） | 桩返回值由三元组改为 `TruncatedStreamOutcome { .., ended_cleanly, .. }`。现在：命中规则（两种截断形状）断言 `ended_cleanly == true`；no-rule 帧边界断言 `!ended_cleanly`（传输错误仍透传，既有行为）；no-rule 帧中间断言 `ended_cleanly`（firewall 已自行结束）。三者不再同义 |
| 测试 (c) | 帧中间那条 with-rule 测试没有任何断言证明 firewall 真的 fail-closed，将来把桩「修」成以空行结尾会静默退化为重复用例 | 共享 helper 内按 `partial_tail.is_empty()` 条件断言 `!delivered.contains("\"delta\":\"wor")` |
| 可选 7 | UI 文案「网关的 故障描述」中夹了字面空格（JSX 换行折叠所致），而新测试用 `\s*` 迁就了它 | 删空格并经 prettier 复核不会重新折行；测试正则收紧为字面量，另两处 `\s*` 收紧为 `\s`（该处空格是中西文混排的**有意**空格） |

### 驳回（附反证）

1. **「`clientStatus` 应改成 200」——会让审计条目彻底消失。**
   前端 `src/services/gateway/requestLogSpecialSettings.ts:169-171` 对整条 marker
   fail-closed：`clientStatus < 400 || clientStatus > 599` → `return null`。写 200 等于让
   运营者**什么都看不到**，比报告一个未生效的状态更糟。故保留 `clientStatus` 为规则配置值，
   另加 `clientStatusApplied: false` 表达「未生效」，并在代码注释与契约中写明该取舍。
2. **「可选 8：`CommonCtxOwned.upstream_error_response_rules` 加了没人读」——事实相反，该字段承重。**
   `failover_loop/context.rs:293` 的 `build_stream_finalize_ctx(ctx: &CommonCtxOwned<'_, R>, ..)`
   在 `:351` 读它：`upstream_error_response_rules: ctx.upstream_error_response_rules.to_vec()`。
   **这正是规则进入 `StreamFinalizeCtx` 从而进入 `usage_tee` 的唯一通路**，删掉会让整个功能失效。
3. **「可选 6：`Passthrough` 兜底文案应改中文」——与既有约定不符，不改。**
   本仓库客户端可见（API wire）文案是英文：`failover_loop/response/finalize.rs:263`
   即 `format!("all providers failed for cli_key={cli_key}")`。中文只用于**运营者可见**的 UI。
   另外该常量同时充当关键词匹配的伪 body 内容（PRD R1a），改中文会连带改变关键词语义。

### 记录为超范围缺口（不在本任务修）
- **应修 3**：`Err` 路径上 `stream_terminal_firewall` 审计条目写入晚于 `finalize()`，**不入日志**
  （实测证据见 design §3.2.1.2）。评审建议「移到 `poll_next_inner` 的 `Err` 臂、`finalize` 之前」
  在结构上不可行——firewall 实例属于 relay 闭包，`poll_next_inner` 拿不到它；真要修需改
  `finalize()` 的调用位置，blast radius 远超本任务。**既有时序**，本任务未加剧其成因，
  仅在测试注释与设计文档中如实标注不可依赖。
- **可选 4**：非 relay 路径上，来自**上游主动发出的终态错误帧**的 post-commit
  `GW_STREAM_ERROR`（`usage_tee.rs` 的 `defer_terminal_error` 未设分支）不做规则匹配、
  不注入。该分支属既有 `stream_internal_errors` 关键词机制的辖区，PRD「Out Of Scope」
  明确不改其匹配逻辑；扩到那里等于给「上游确实发了错误帧」的场景新增行为，超出用户
  报告的传输中断问题。评审提到的 CX2CC 连带推论**未被证实**，建议单独立项验证。
- **可选 5 / 可选 9**：claude/gemini/grok 的端到端注入测试与 524 全链路端到端测试
  （现有单元测试已覆盖 524 与 claude 直连形状）；尾帧 `tx.send` 为 best-effort 而审计
  无条件写（客户端已断开，无实害，与应修 2 同族）。均记录不修。

### 评审后追加：渲染层同步 `[2026-08-24]`

评审在接受全部驳回后留了一条残余观察：Rust 侧数据已诚实，但**没有任何 TS/TSX 读**
`clientStatusApplied` / `scope`，所以运营者在日志/UI 上看到的「前后状态」仍是
「502 → 503」，而客户端实收 200。**已修**——理由是本任务之前 `clientStatus` 恒为真实下发值，
是本任务引入 post-commit 路径后才可能失真，属本任务引入的展示失真，不能只修数据层：

- `src/services/gateway/requestLogSpecialSettings.ts`：`UpstreamErrorResponseRuleMarker`
  增补 `clientStatusApplied: boolean`，normalizer 用 `setting.clientStatusApplied !== false`
  ——**只有显式 `false` 才算「未生效」**，既有日志与全部 pre-commit 改写读数逐字节不变；
  `formatUpstreamErrorResponseRuleTooltip` 在其为 `false` 时把状态行改为
  「状态码：502 → 503（未生效：流已下发，客户端仍为 200）」并追加
  「改写方式：在流末尾追加错误事件」。
- badge 本体（`RealtimeTraceCards.tsx` 的 `UpstreamErrorResponseRuleBadge`）只显示规则名，
  状态信息全在 tooltip，故无需改动。
- 兼容性已实测确认：该 normalizer 全文**不校验** `scope`，故新值 `stream_tail` 安全通过；
  `clientStatus` 仍留在 400..=599，marker 不会被 fail-closed 丢弃。
- 测试：`requestLogSpecialSettings.test.ts` 新增
  「marks a post-commit stream-tail rewrite as not having changed the status」，并在既有
  用例上补断言「缺字段 ⇒ `clientStatusApplied === true` 且 tooltip 不含「未生效」」。

### 评审同时确认无问题的部分（摘）

`stop_after_tail` 与 `stop_after_terminal_error` 互斥、无二次 `take`、drain 永不构造尾帧；
注入点在 `tracker.ingest_chunk` 之后故不污染 usage/completion 判定（R7）；`DropTerminal`
（firewall 判定上游发了终态错误帧）根本走不到 `take_pending_tail()`，故该语义未被触碰；
pre-commit 钩子的 allow-list 确实只放行两个码、`decision` 未动（R2a）；契约第 99 与第 200 行
仍成立；泄漏面（伪 body 仅在栈上、`special_setting()` 从不带 `message`、`Passthrough` 强制
固定文案）。

### 评审后的复验 `[最终门]`

后端 `cargo fmt --all -- --check` ✅ + `cargo clippy --all-targets --locked -- -D warnings` ✅
零警告 + `cargo test --locked -- --test-threads=1` ✅ **lib 3001 passed / 0 failed / 4 ignored**
（539.99s；**未接管道，`EXIT=0` 为 cargo 自身退出码**——此前一次运行接了 `| tail`，退出码来自
`tail` 而非 cargo，已重跑纠正）+ 全部集成测试二进制通过。
前端 `typecheck` ✅ / `lint` ✅ / `test:unit` ✅ **310 files / 2914 tests** / `build` ✅ /
`check:spec-links` ✅ / `npx prettier --check src/services/gateway/ src/components/` ✅
（仓库级 `format:check` 唯一告警仍是基线既有的 `src-tauri/tauri.conf.json`，见步骤 7）。

---

## 全局回滚策略

功能入口是「规则是否命中」，最快回滚为停用规则即可恢复现状。代码层面 R1–R7 各点相互
独立，按 R7 → R1 逆序回退即可。步骤 3（R3）已主动回退，不在回退链内——
`failover_loop` 与 `finalize` 已恢复 base 行为。

## 风险清单

| 风险 | 触发条件 | 应对 |
| --- | --- | --- |
| tracker 污染 | 在 `usage_tee.rs:426` 上游注入 | 严格用 P1 注入点（tracker 之后）；4.8 有专项测试 |
| 审计不进日志 | push 晚于 `finalize` | 严格遵守 §3.4 时序；4.8 有专项测试 |
| 泛型 bound 失败 | `B: From<Bytes>` 推导受阻 | 步骤 0.4 先试编译，失败改 `Bytes` 特化层 |
| 伪 body 泄露 | `Passthrough` 透传伪 body | 采用 S2 固定文案；2.5 有专项测试 |
| 524 未生效 | 只改 `Err` 分支 | 4.5 单列 idle timeout 分支 |
| 两侧划分错误 | 0.2 结论为「874 在 commit 后可达」 | Gate 0 拦截，退回 Plan 修订 design |
| 契约矛盾 | 只改代码不改契约 | 步骤 7 强制；`check:spec-links` 把关 |
