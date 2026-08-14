# Research: CX2CC 当前 AIO 网关回环与 120 秒延迟

- Query: 诊断 CX2CC 选择“当前 AIO 服务 Codex 网关”时残留的自环误判与 120 秒延迟，追踪请求生命周期、内部重入 nonce、本机 URL 识别、候选过滤、超时/故障转移顺序、请求日志分类和缓存，并提出不削弱普通提供商回环保护的最小修复与测试。
- Scope: internal
- Date: 2026-08-14

## Findings

### 结论

当前现象由两个相邻但独立的生命周期错误共同造成，不是缓存：

1. 外层 Claude/CX2CC 请求把本机 Codex 网关当作普通上游，给本机 HTTP 委派套用了与内层真实 Codex 上游完全相同的首字节超时。外层计时更早开始，所以当设置为 120 秒时，外层会在内层有机会完成自己的 120 秒超时分类、同提供商重试或切换提供商之前先超时。
2. 外层超时按默认传输重试策略重试同一个 CX2CC bridge provider，但进程内 `InternalCodexReentry` 在第一次发送前已被 `take()`/`OnceLock` 永久消费。第二次尝试仍是完全相同、合法的当前网关目标，却失去自环例外资格，于是普通目标保护把它记录为 `provider_target_self_loop` / `GW_INVALID_BASE_URL`。

这与截图的顺序逐项吻合：`上游超时 x1（ALI，120 秒）`，随后 `无效URL x1（ALI）`，末项详细信息为 `target_validation`、`selection=filtered`、`decision=skip`、`reason_code=provider_target_self_loop`。第二项不是第一次请求的根因，而是第一次超时后的本地重试误判。

最小修复必须同时做两件事：

- 将 `InternalCodexReentry` 改成在同一个已准备 provider 的有界重试循环中可重复校验的、不可伪造的进程内描述符；每次真实发送仍签发一个新的、线上只可消费一次的 nonce。
- 对精确匹配且已授权的本机重入发送不应用外层 provider 首字节超时，让内层 Codex 网关独占真实上游的超时、重试和故障转移预算。普通远端 provider 继续使用现有超时。

普通 provider 回环保护不应改动：每次发送仍运行 `validate_gateway_target`，只有进程内描述符同时匹配当前外层 trace、bridge provider、`POST` 和精确当前网关 `/v1/responses` URL 时，才可接受 `SelfLoop` 结果。

### 精确请求生命周期

| 阶段 | 当前行为 | 证据 |
| --- | --- | --- |
| 1. 外层入口 | Claude 请求进入正常代理处理，生成外层 trace；外部请求没有可信重入标记。 | `src-tauri/src/gateway/proxy/handler/mod.rs:173` |
| 2. 外层候选 | 正常 Claude 候选选择到 CX2CC bridge provider。候选入口仍是按 `cli_key` 的启用 provider 列表。 | `src-tauri/src/gateway/proxy/handler/provider_selection.rs:20`, `src-tauri/src/gateway/proxy/handler/provider_selection.rs:29` |
| 3. 当前网关准备 | `source_id=None` 分支从实时 `app_gateway_status(...).base_url` 取得当前网关地址，构造绑定外层 trace、bridge provider id、`POST` 和精确 `/v1/responses` URL 的 `InternalCodexReentry`，再把 Anthropic 请求翻译为 Responses 请求。它不走显式 source provider 的 base URL 选择。 | `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:109`, `:171`, `:216`, `:226`, `:253` |
| 4. 外层尝试 1 | 构造最终 URL 后，`consume_and_match` 先 `take()` 掉描述符；目标校验正确识别为 `SelfLoop`，由于第一次匹配成功而放行。请求指纹已生成后签发新 nonce，插入私有头，并使用无代理、禁止重定向的直连 client。 | `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:372`, `:382`, `:394`, `:445`, `:477`, `:486`, `:515` |
| 5. 内层入口 | 当前网关收到 `POST /v1/responses`；入口先从 header map 删除 `x-aio-internal-reentry-nonce`，再原子消费 registry 项，得到 `TrustedInternalReentry`。私有头不会进入后续 provider 请求。 | `src-tauri/src/gateway/proxy/handler/mod.rs:54`, `:62`, `:189`; `src-tauri/src/gateway/internal_reentry.rs:99` |
| 6. 内层路由 | 内层仍按普通 Codex 模式加载、排序和筛选 Codex providers；可信重入目前只跳过 generic configured-model route，并不创建新的回环例外，也不绕过 circuit/cooldown/limit/session/provider-enabled 等普通门。 | `src-tauri/src/gateway/proxy/handler/provider_selection.rs:29`; `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:90`, `:110`, `:392` |
| 7. 内层真实发送 | 选中的 Codex provider 在自己的 attempt loop 中发送；每次真实 provider 目标仍做普通回环校验。 | `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:372`, `:394` |
| 8. 相同超时嵌套 | 外层本机 HTTP `.send()` 与内层真实 provider `.send()` 都使用同一全局 `upstream_first_byte_timeout`。外层 timer 在内层 timer 之前启动，所以相同的 120 秒 deadline 下外层先到期。 | `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:141`, `:520`; `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/send.rs:13`, `:37`; `src-tauri/src/gateway/proxy/request_context.rs:155`, `:256` |
| 9. 外层超时决策 | 外层把本机委派记作 `GW_UPSTREAM_TIMEOUT`。默认策略启用 Timeout transport retry、`max_retries=1`、`backoff_ms=100`，所以决定 `RetrySameProvider`。 | `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/send_timeout.rs:27`, `:36`; `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/upstream_retry_policy.rs:127`, `:185`; `src-tauri/src/infra/settings/types.rs:239` |
| 10. 外层尝试 2 | retry loop 重用同一个 `PreparedProvider`，但其中的 `internal_codex_reentry` 已在尝试 1 被消费。相同当前网关 URL 因而不再授权；普通校验再次返回 `SelfLoop`，这次变成发送前的 `GW_INVALID_BASE_URL` skip。 | `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:49`; `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs:23`, `:180`, `:392` |

### 两层授权边界不能混为一谈

`InternalCodexReentry` 和 nonce 的职责不同：

- `InternalCodexReentry` 是只存在于进程内 `PreparedProvider` 的类型化意图。字段/构造器是模块私有的，绑定外层 trace、bridge provider id、HTTP method 和规范化后的精确 URL。外部 HTTP 客户端不能构造它。见 `cx2cc_preparation.rs:12`、`:25`、`:36`。
- `x-aio-internal-reentry-nonce` 是线上 capability。registry 默认 TTL 10 秒、容量 1024、随机 32 bytes（256 bit）；header 只携带 64 字符十六进制 nonce，不暴露 trace/provider。见 `internal_reentry.rs:9`、`:11`、`:30`、`:51`、`:147`。
- ingress 消费时先从 map 中 `remove(nonce)`，再验证 `cli_key=codex`、`POST`、path 精确等于 `/v1/responses` 且无 query；伪造、重放、过期、错误方法/路径/query 都失败关闭，错误契约也会烧掉 nonce。见 `internal_reentry.rs:110`、`:119`、`:126`。
- nonce 在本机发送前即时签发并在内层入口即时消费，因此 10 秒 TTL 不限制后续 120 秒或更长的响应处理。
- 直连 client 强制 `no_proxy()` 且 `redirect(Policy::none())`，防止私有 nonce 被系统/上游代理或 HTTP redirect 带离本机。见 `src-tauri/src/gateway/http_client.rs:685`；现有测试见 `:1756`。

因此，当前 `InternalCodexReentry` 的 `Arc<OnceLock<()>>` 是把“每个线上 hop 只用一次”错误地提升成“整个 provider 重试生命周期只准发送一次”。正确边界是：进程内意图可在该 `PreparedProvider` 的有界 retries 中重复做精确匹配；每个实际 HTTP hop 各自签发不同 nonce，且每个 nonce 仍只能消费一次。

### 当前网关 URL 识别

- gateway 启动/状态变化时同步当前 port、bind host 和 base host 到自环上下文。见 `src-tauri/src/gateway/control_service.rs:414`、`src-tauri/src/gateway/http_client.rs:173`。
- null-source CX2CC 使用同一运行时状态提供的 `base_url`，因此最终 URL 的 host/port 是本机上下文已知值。见 `src-tauri/src/app/gateway_runtime_access.rs:6`、`cx2cc_preparation.rs:217`。
- `validate_gateway_target` 先同步调用 `url_points_to_gateway_with_context`；只要 port 相同且规范化 host 已在上下文集合中，就立即返回 `SelfLoop`。见 `http_client.rs:1045`、`:1056`。
- 只有“同端口但 host 不是已知本机 token”的 DNS alias 才进入异步解析、短期缓存和地址固定。见 `http_client.rs:1067`。当前由自身状态生成的 URL 不需要 DNS，因此不会在这里等待 120 秒。
- 已有 route regression 证明普通 Codex provider 指向 `127.0.0.1:<gateway-port>` 会被 health-neutral skip，随后切换远端 provider；circuit 和 session 不受污染。见 `src-tauri/src/gateway/routes.rs:4498`、`:4563`。

### 候选过滤和为何不会无限再次选中 bridge

- 外层仍是正常 Claude provider 集合，当前网关 CX2CC bridge 不是在候选阶段被提前过滤；它准备为 Ready 后，在最终发送目标校验点取得精确自环例外。
- 内层是新的 `cli_key=codex` 请求，按 active mode 正常加载 Codex providers。可信重入只关闭 generic configured-model route，其他 provider gate 不变。见 `provider_selection.rs:29`、`provider_iterator.rs:90`、`:392`。
- 数据约束要求 `bridge_type=cx2cc` 只能用于 `cli_key=claude`，所以发起重入的 bridge provider 不会自然出现在内层 Codex candidates 中。见 `src-tauri/src/domain/providers/queries.rs:1749`。
- 若某个普通 Codex provider 自身错误配置成当前网关，它没有 `InternalCodexReentry`，即使当前请求是受信内层请求也必须被普通 guard 拒绝。例外绑定的是外层已准备 bridge provider 的一次本机委派，不是给整个 trusted request 或所有内层 candidates 的通行证。

### 120 秒的确定性来源

首字节 wrapper 只包围 reqwest `.send()` 到“收到 response headers”为止：`send.rs:31` 构造 future，`:37` 用 `tokio::time::timeout` 包裹。每次 ingress 都从同一运行时设置构造 `RequestContext`，`0` 才表示禁用；外层和内层没有不同的 timeout domain。

令设置为 `T=120s`，外层本机委派 timer 在 `t0` 启动，内层真实 provider timer 只能在接受本机连接、解析请求、重新选择 provider 后的 `t0+delta` 启动，且 `delta>0`：

```text
outer deadline = t0 + T
inner deadline = t0 + delta + T
outer deadline < inner deadline
```

所以内层 provider 若在 T 内没有返回 headers，外层必定先报超时；内层还来不及把自己的 timeout 转化成同 provider retry、provider switch 或最终网关响应。120 秒是用户当前配置值在外层本机 hop 上被重复应用的结果，不是内部识别、DNS 或缓存耗时。

### 重试、故障转移和日志分类

- timeout handler 用 transport `Timeout` 规则计算决策。默认 retry policy 包含 Connect/Timeout/Read、允许一次 configured retry，退避 100 ms，且默认不计入 circuit。见 `send_timeout.rs:27`、`upstream_retry_policy.rs:116`、`:127`、`src-tauri/src/infra/settings/types.rs:239`。
- retry #2 继续使用同一 `PreparedProvider`，这正是 `InternalCodexReentry` 的生命周期应覆盖的边界。当前单次消费测试甚至明确把错误语义固定住：同一意图第二次精确匹配失败、clone 共享一次消费。见 `cx2cc_preparation.rs:440`、`:475`。
- 发送前 `SelfLoop` 拒绝追加一个 `outcome=skipped`、`error_category=target_validation`、`error_code=GW_INVALID_BASE_URL`、`reason_code=provider_target_self_loop` 的 attempt，但不会替换前一次真实 timeout 保存的 `last_outcome`。见 `retry_engine.rs:180`、`:392`。
- all-failed 的请求级 error code 来自 `last_outcome`，所以请求级 `gateway_error_code` 仍是 timeout。见 `src-tauri/src/gateway/proxy/handler/failover_loop/response/finalize.rs:224`、`:247`。
- 请求日志的 `error_details_json` 反向选择最后一个包含诊断信号的 attempt，并把它的 `error_code` 写成展示码，同时单独保留请求级 `gateway_error_code`。见 `src-tauri/src/gateway/proxy/request_end.rs:535`、`:593`、`:601`、`:608`。
- 前端优先使用 `error_details.error_code` 作为 `displayErrorCode`，仅回退到 `gatewayErrorCode`，所以卡片顶标显示最后的 `GW_INVALID_BASE_URL`；失败汇总又从完整 `attempts_json` 分组，因此同时展示 `120 秒 timeout x1` 和 `invalid URL x1`。见 `src/components/home/requestLogErrorDetails.ts:131`、`:332`、`:336`。

现有日志投影逻辑解释了 UI，但不是应修的层。修复重入 retry 后，虚假的最后 self-loop attempt 消失，展示会自然回到真实内层结果；不需要用前端特判掩盖它。

内层重入是一个新的正常 Codex request，私有 header 在计算后续观测信息前已被移除；当前代码没有把 trusted reentry 自动排除出 request log。因此正常情况下会有独立的内层 Codex 日志。外层先超时并断开连接后，内层最终是继续完成还是记录 client abort 取决于运行时断连传播；没有原始运行日志时不能仅靠静态代码断言其最终状态。

### 缓存核查

没有 AIO 响应缓存参与这条 120 秒路径：

- DNS target cache 只服务 provider 自环防护的同端口未知 host 解析，正/远端负/失败 TTL 分别为 30/5/1 秒，单次解析上限 750 ms，容量 128。当前状态生成的已知本机 host 在同步 fast path 就返回 `SelfLoop`，不读 DNS cache。见 `http_client.rs:31`、`:46`、`:1045`。
- provider base URL ping cache 只在 `source_id=Some(...)` 的显式 source provider 分支选择实际 upstream base URL 时调用；当前网关的 `source_id=None` 分支直接读取运行时 `base_url`。见 `cx2cc_preparation.rs:109`、`:171`、`:216`；缓存实现见 `src-tauri/src/gateway/proxy/caches.rs:126`。
- recent error cache 只会短路已缓存的 all-providers-unavailable 结果。写入仅发生在 all-unavailable、有正 retry-after 且没有 account-usage skip 时。当前请求包含一次真实 timeout，走 all-providers-failed，不会由最后一个 gate skip 转成 cacheable all-unavailable，也不会在该分支写入。见 `src-tauri/src/gateway/proxy/handler/request_fingerprint.rs:66`、`failover_loop/loop_helpers.rs:101`、`failover_loop/response/finalize.rs:158`。
- UI 中“大上下文、冷缓存”的文字是所有 `GW_UPSTREAM_TIMEOUT` 共用的静态排查建议，不是一次 cache hit 的观测证据。见 `src/constants/gatewayErrorCodes.ts:120`。

真实 Codex provider 自身是否有 prompt cache 不在本次证据范围内；即使它冷启动，也只能解释真实 provider 为什么慢，不能解释外层必定在同一 deadline 先超时及随后出现的本地 self-loop skip。

### 建议的最小修复

1. 在 `cx2cc_preparation.rs` 删除 `InternalCodexReentry.consumed: Arc<OnceLock<()>>`，把 `consume_and_match(&mut Option<Self>, ...)` 改为对 `Option`/`&Self` 的不可变精确匹配，例如 `authorizes(...)`。不要 `take()`。
2. `attempt_executor.rs` 每次尝试仍先构造最终 URL、每次仍运行 `validate_gateway_target`。仅当结果是 `SelfLoop` 且类型化意图精确匹配时允许继续；`ResolutionFailed`、错误 trace/provider/method/URL、普通 provider self-loop 全部保持失败关闭。
3. 每次获准发送都继续调用 `InternalReentryRegistry::issue`，因此 retry 1、retry 2 得到不同 nonce；nonce 继续在 ingress 原子消费一次。不要复用线上 nonce，也不要扩大 header 契约。
4. 对 `authorized_internal_reentry=true` 的本机发送传 `first_byte_timeout=None`；其他 provider 原样传 `ctx.upstream_first_byte_timeout`。内层网关拥有实际 provider 的 timeout/retry/failover，外层只等待本机路由委派返回 headers。
5. 保留现有直连、无 proxy、无 redirect、指纹后注入 nonce、每次发送前 provider-enabled 复读和 dispatch ownership 边界。

不要采用以下更宽的绕过：按 `cx2cc_active` 全局跳过自环校验、按 `source_id=None` 跳过、按 trusted ingress 给所有内层 provider 放行、允许 localhost alias/任意 path/query、或复用同一个 nonce。这些都会削弱普通 provider loopback guard 或扩大 capability。

若产品必须给本机委派再加一层总时限，它必须由内层最坏情况预算推导并严格大于 `provider 数 x 每 provider attempts x first-byte timeout + backoff/本地开销`；固定为与内层相同的 120 秒必然重现当前竞态。就现有有界 provider retry/failover 设计而言，禁用外层首字节计时是最小且语义最清晰的修复。

### 建议测试

1. **类型化意图生命周期单测**：替换当前 `is_consumed_once...` 和 `clones_share_single_consumption` 期望。同一 prepared intent 对相同 trace/provider/POST/exact URL 连续两次都应授权；错误 trace、provider、method、scheme、host token、port、path、query 每次都不授权。
2. **nonce 防重放回归**：保留并扩展 `internal_reentry.rs` / handler 现有测试，断言两个合法 retries 签发两个不同的 64 hex nonce，各自只能在精确 Codex Responses contract 消费一次；重放、伪造、过期、错误 method/path/query 仍失败，并且 header 在 ingress 后不存在。
3. **超时域单测**：抽出小 helper 或直接测 attempt 参数，断言 authorized local reentry 的有效 outer first-byte timeout 为 `None`，普通 provider 仍等于配置值，未授权 self-loop 根本不进入 send。
4. **端到端等 deadline 回归**：启动真实本机 gateway listener。外层 Claude 候选只有 null-source CX2CC bridge；内层 Codex provider A 延迟超过一个很短的测试 first-byte timeout，provider B 成功。外层与内层读取同一设置。预期内层先记录 A timeout 并切换 B，外层最终成功；外层 attempts 不含 `provider_target_self_loop` 或本机 timeout。
5. **本机 hop retry 回归**：使第一次受信本机调用返回一个按配置会 `RetrySameProvider` 的结果、第二次成功。捕获两次入站 nonce，断言二者不同且都被接受；外层 attempts 不出现 `GW_INVALID_BASE_URL`。
6. **普通 guard 保持测试**：保留 `routes.rs:4498`；再增加“请求本身是 trusted reentry，但所选普通 Codex provider 指向当前 gateway”的用例，必须仍被 `provider_target_self_loop` 跳过并切换远端，证明授权不向内层 candidates 扩散。
7. **日志投影回归**：成功恢复时外层 request log 不含虚假 invalid-URL attempt；内层最终失败时，外层展示/网关码应反映真实内层返回而非本地 self-loop。若本机 hop 确实重试，两次应都是已发送 attempt，不应有 `selection=filtered` 的第二项。
8. **传输保密回归**：继续执行 `http_client.rs:1756` 的 proxy/redirect 测试，确保新增 retry 也始终使用直连 client，nonce 不经过代理或 redirect。

### Files Found

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs`：null-source 当前网关准备、进程内意图及当前错误的一次性消费测试。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs`：最终 URL 校验、自环例外、nonce 注入、client 选择和首字节 timeout 传递。
- `src-tauri/src/gateway/internal_reentry.rs`：256-bit nonce registry、TTL/容量、精确 ingress contract 和一次消费语义。
- `src-tauri/src/gateway/proxy/handler/mod.rs`：入口删除并消费私有 header，建立 trusted context。
- `src-tauri/src/gateway/http_client.rs`：本机 URL 识别、DNS 防重绑定缓存、直连无代理无重定向 client。
- `src-tauri/src/gateway/control_service.rs`：将当前 gateway host/port 同步到 HTTP 自环上下文。
- `src-tauri/src/app/gateway_runtime_access.rs`：向 null-source CX2CC 暴露当前运行中 gateway base URL。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs`：按 cli active mode 加载内外层候选。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs`：普通 provider gates、CX2CC prepare 和 trusted reentry 的 configured-route 抑制。
- `src-tauri/src/domain/providers/queries.rs`：CX2CC bridge 仅允许 `cli_key=claude` 的数据约束。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/send.rs`：包住 reqwest `.send()` 的首字节 timeout 实现。
- `src-tauri/src/gateway/proxy/request_context.rs`：把同一全局秒数转换为每个 ingress 的 timeout duration。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/send_timeout.rs`：timeout attempt 分类和 retry decision。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/upstream_retry_policy.rs`：configured retry/failover 顺序和 backoff。
- `src-tauri/src/infra/settings/types.rs`：默认 Timeout transport retry、一次 retry、100 ms backoff。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs`：重用 prepared provider、追加发送前 target-validation skip。
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/finalize.rs`：all-unavailable cache 写入条件与 all-failed 请求级错误选择。
- `src-tauri/src/gateway/proxy/request_end.rs`：attempts 日志序列化及最后诊断 attempt 投影。
- `src/components/home/requestLogErrorDetails.ts`：展示码优先级及 attempt failure 分组。
- `src/constants/gatewayErrorCodes.ts`：timeout 的静态“冷缓存”排查提示。
- `src-tauri/src/gateway/proxy/caches.rs`：recent error 和 provider base URL ping cache 实现。
- `src-tauri/src/gateway/routes.rs`：普通 provider self-loop health-neutral failover 的现有集成测试。

### External References

- 未使用外部文档或网络资料；因果链由当前 worktree 源码、现有测试和当前任务截图建立。
- 相关 API 语义均由仓库中的直接调用验证：`tokio::time::timeout`、`reqwest::ClientBuilder::no_proxy`、`redirect(Policy::none)`；本研究不依赖未锁定的外部版本行为。
- 参考了上一任务的内部研究 `D:/UGit/aio-coding-hub-fork/.trellis/tasks/08-13-cx2cc-routing-gpt56/research/loopback.md` 以区分“最初缺少受信重入机制”和“机制落地后 retry/timeout 生命周期仍错误”这两个阶段。

### Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/cx2cc-routing-contract.md`：精确当前网关意图、一次性 nonce、one-hop、无 proxy/redirect、内层路由和普通 loop guard 契约。
- `.trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md`：每次发送前最终 URL 校验、自环 health-neutral skip 和 fallback 行为。
- `.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md`：configured retries 与 provider attempt budget、同 provider backoff 的顺序。
- `.trellis/spec/aio-coding-hub/cross-layer/reliability-boundaries-contract.md`：请求生命周期、超时和故障恢复边界。
- `.trellis/spec/aio-coding-hub/cross-layer/reasoning-effort-observability-contract.md`：本任务相邻的 CX2CC 观测契约；本报告未分析 effort mapping 本身。

## Caveats / Not Found

- 本研究遵守 research-only 范围，没有修改或运行 source tests，也没有启动 live gateway 重放请求。根因是静态控制流与当前任务截图的强一致诊断，不是抓包结果。
- 未取得截图对应的原始 `request_logs` 数据库行或内层 Codex trace；因此可以确定外层 attempt 的分类顺序，但不能确定外层断连后内层 handler 最终记录为成功、失败还是 client abort。
- 截图明确显示 120 秒，本研究未读取用户运行实例的 settings 数据库来二次确认该具体配置值。
- 当前树中未找到覆盖“null-source CX2CC -> 真实 HTTP 当前 gateway -> nonce ingress -> 内层 Codex failover -> 外层响应”的端到端测试；现有覆盖主要是意图/nonce/client 单测和普通 self-loop route 测试。这是该回归能残留的主要测试缺口。
- 禁用外层本机 hop 的 first-byte timeout 意味着非 provider 类的内层死锁不再由该 timeout 截断；现有内层 provider timeout/failover 和本机 connect timeout覆盖正常失败。若要补独立 watchdog，应从完整内层最坏预算推导，不能复用同一 provider timeout。
- 外层在内层返回可重试 HTTP 错误后可能按既有 policy 再执行一次完整本机委派；可重复精确意图会使这次重试合法且 nonce 仍一次性。是否要进一步抑制“双层 HTTP retry”是独立策略问题，不应与本次错误的 self-loop/相同 deadline 修复混在一起。
