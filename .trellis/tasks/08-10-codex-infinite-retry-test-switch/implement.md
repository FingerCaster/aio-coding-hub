# 实施清单

## 0. 启动门与基线确认

1. [ ] 确认 `08-10-codex-stream-terminal-firewall` 已实现并合入当前工作基线；记录它实际提供的
   structured SSE frame/terminal API、settings schema 和相关 migration 版本。
2. [ ] 若终态任务仍在规划、实现中或与本任务共享文件存在未集成修改，停止本任务实施；先
   完成串行集成，禁止复制 parser 或同时占用同一 schema 版本。
3. [ ] 重新运行 `trellis-before-dev`，读取 backend/cross-layer/frontend 的 index、pre-development
   checklist 和本任务 manifests；核对 `git status`，保留用户及相邻任务的已有修改。
4. [ ] 从实施时基线分配下一可用 settings schema 和 DB migration 版本，并在提交前用 migration
   tests 证明新旧安装路径一致。

## A. Settings 与请求 eligibility

5. [ ] 在 `AppSettings` 增加
   `codex_infinite_retry_test_enabled=false` 与
   `codex_infinite_retry_test_interval_ms=1000`；后者只接受 `0..=60000`。
6. [ ] 同步 defaults、migration、repair/validation、`SettingsUpdate`、`SettingsPatch`、owned token、
   apply/equality/rollback；补“缺字段保留 canonical 值”和越界拒绝测试。
7. [ ] 更新 generated bindings 及 frontend settings clone/default/validate/patch mapping；运行
   `pnpm tauri:gen-types` 后只保留与本任务相关的生成差异。
8. [ ] 在 Codex classifier 输出 typed `is_codex_system_request`，新增单一 eligibility 函数；覆盖
   用户 Responses、compact、model discovery、Provider test/warmup、token count、system turn 和
   开关快照矩阵。
9. [ ] 调整 middleware 顺序，使 eligibility 在入口校验/鉴权/request plugin/security 后、无
   Provider 短路前确定；普通空 Provider 仍终止，测试模式空计划继续进入 active registry 和
   orchestrator。

## B. Round planner 与 circuit-neutral 模式

10. [ ] 从一次性的 Provider resolution 中提取 `build_provider_round_plan`；每轮重新读取 Provider
    清单、启停、active sort/order、candidate cap、session preference、model route 和 Provider
    配置，输出轮内不可变计划。
11. [ ] 明确请求级与轮次级字段所有权：body/session/request token/test switch/interval/timeouts
    固定；Provider plan 与 Provider-local route/config 每轮刷新。用 barrier test 证明轮中修改
    只影响下一轮。
12. [ ] 保证 `RequestDispatchIntent`/compaction reservation 整个请求只创建一次、第一笔真实 send
    最多消费一次；空轮、gate skip 和新轮次不能重建 reservation。
13. [ ] 新增 `ProviderHealthMode` 并贯穿 selection、provider gate、attempt record、response success
    和 session commit；不要用散落的开关判断替代 typed mode。
14. [ ] 在 `InfiniteRetryTest` 下旁路 session circuit availability、failback/probe planning、circuit
    gate/lease/snapshot 和所有 success/failure/cooldown/recovery 写入；account usage、Provider
    limits、凭据和现有非 circuit gate 保持。
15. [ ] 让 neutral attempt/event 的 circuit/probe 字段为空并标记 health mode；补预置 OPEN、
    cooldown、probe-ready 的快照/spy 测试，证明零 circuit 决策读写且仍真实调用 Provider。

## C. 外层 round orchestrator

16. [ ] 把当前一次 Provider traversal 提取为可复用的单轮 executor，保持普通调用者的现有
    finalize、attempt budget、backoff 和错误投影不变。
17. [ ] 新增 `run_infinite_rounds`：每轮创建新的 Provider plan 和 round-local retry/failure state；
    `ValidatedSuccess` 返回，`RoundExhausted` 等待后重开，empty plan 或进入模式后的临时 plan/DB
    read failure 也形成可观测失败轮。
18. [ ] 测试模式继续复用 Provider-local retry policy；当前 Provider 不再 local retry 后，所有
    上游/准备/协议结果（包括普通路径的 request-level Abort）都切到下一候选，不向客户端提交。
19. [ ] 使用 cancellation-aware timer；interval 大于 0 时只在整轮失败后等待一次，interval 为 0
    时强制 `yield_now` 并检查 cancel/shutdown，禁止空轮 busy loop。
20. [ ] 让 transport、body read、transform 和 sleep 同时响应 client cancel/disconnect/gateway
    shutdown；触发后不再创建 attempt，也不回放失败 bytes。

## D. 有界 final-wire 响应处理

21. [ ] 基于前置终态任务的共享 parser 增加完整 Codex Responses success validator；覆盖唯一
    completed、合法 `[DONE]`、failed/error/incomplete、缺 terminal、EOF、malformed、重复/冲突
    terminal 和 completed 后未知 semantic frame。
22. [ ] 为测试模式新增 SSE buffered success path，严格按 decode -> bridge -> response fixer ->
    response plugin -> bounded final-wire buffer -> validator 执行；验证前不得 record success、
    bind session、build downstream body 或发送 header。
23. [ ] 在非流 success path 的 bridge/fixer/plugin 之后、success side effects 之前验证唯一完整
    completed JSON；所有 HTTP failure 和 2xx embedded failure 都返回 attempt failure control。
24. [ ] 对所有 attempt 的 upstream-read bytes 和 final-wire payload 使用共享 20 MiB hard cap：
    已知长度预拒绝、未知长度追加前检查、每个 transform 后复查；transform 消费旧 buffer 或
    有界输出，不能长期保留两个完整 payload。超限清空并 drop，记录 `response_too_large`，不写
    body/帧到日志或磁盘。
25. [ ] 在失败 buffer 释放前提取结构化 usage/evidence；普通 plugin error 可继续 Provider，typed
    security `Blocked` 走 local terminal，禁止通过文案判断。
26. [ ] 成功时只回放最终 buffer 一次：清理 hop-by-hop/旧 length/失败 header，保留成功响应语义
    并只添加一次 trace id；提交后的 downstream abort 不重新重试。

## E. 有界诊断、usage 与 cost

27. [ ] 实现 `InfiniteRetryLedger`：saturating `u64` counters + overflow flag、最近 100 条专用
    `InfiniteRetryAttemptSummary` ring、固定 failure categories、最多 100 个 Provider bucket 和
    `other_providers`；运行期不得保留完整 `FailoverAttempt`、base URL 或上游 message。
28. [ ] 删除无限分支对 `attempts.len()` 的全局索引依赖；event DTO clamp 并标记 overflow，JSON
    counters 使用 decimal string。每条 reason/error/code 继续使用现有脱敏和长度上限。
29. [ ] 分离 final client usage 与 cumulative log usage：客户端/`usage_json` 只用最终成功 attempt，
    request log token columns 使用所有实际上报的已知总量。改造日志 adapter，使 aggregate metrics
    不再被 final raw usage 覆盖；每个 token 字段记录 known sum + missing count，missing 不填零。
30. [ ] 抽取 request-log 现有 price alias/effective basis/priority/multiplier 为共享 calculator；按
    attempt 的 Provider/model/config 快照立即计价，缺价格标 unpriced，所有加法 saturating。
31. [ ] 增加实施时 DB migration 的 Provider usage/cost 子账本和 typed persistence DTO；父 trace
    仍只有一条 request log。普通日志行继续原路径，无限行的父总量和 Provider 子账本不能双计。
32. [ ] 更新 Provider quota/cost、usage aggregation 和 request detail 查询：总体使用父累计值，
    Provider 维度使用子账本；超过 100 Provider 时明确 attribution incomplete，不能把 overflow
    归到 final Provider。
33. [ ] 将版本化 infinite summary 写入 `activity_details_json`，最多 1 Hz upsert，终止强制一次；
    success/cancel/disconnect/shutdown/local terminal 都记录 stop reason、总量和最近摘要。为 100 条
    attempt + 100 个 Provider bucket 设置固定字段/长度/总字节预算，最坏 Unicode 输入仍小于现有
    256 KiB JSON 上限，不能退化成通用截断占位。
34. [ ] 给 `RequestAbortGuard` 共享轻量 ledger snapshot handle；future drop 时释放 active entry 并
    持久化最新有界状态，不复制或保留 20 MiB response buffer。

## F. Active registry 与 Codex UI

35. [ ] 扩展 `ActiveRequestStart`/snapshot/entry 的无限模式标记、phase 和安全 round/attempt 投影；
    更新所有构造点、shutdown reconcile、生成绑定和 registry 生命周期测试。
36. [ ] 在 `CodexTab` 增加紧凑测试设置区：Switch、`0..=60000 ms` 数字输入、当前活动数量和
    “真实持续调用 + 活动请求数 × 20 MiB”风险提示；复用现有控件与排版，不嵌套卡片。
37. [ ] 从现有 active request snapshot query 过滤得到数量；关闭开关后仍显示已运行请求，直到
    success/abort/shutdown 从 registry 释放。不得从 request logs 推算活动数量。
38. [ ] 补 settings 保存失败/回滚、边界值、开关只影响新请求、窄宽度/最长文本、不重叠和活动
    数量变化的 frontend tests。

## G. 聚焦回归测试

39. [ ] 用暂停时间覆盖 interval `0/1000/60000`、Provider 内无额外 interval、整轮只等待一次、
    success 不等待、普通 retry backoff 不变。
40. [ ] route-test 至少三个 Provider 多轮失败后成功，断言每轮调用顺序、local retry/candidate cap、
    round refresh 和最终客户端唯一响应；HTTP auth/quota/invalid/5xx、transport、timeout、prep 和
    protocol failure 都不能提前返回。
41. [ ] route-test 上游已发可见 SSE 后 failed/EOF/malformed，证明 header/body 均未提交；后续成功
    回放完整有序 SSE。非流 2xx embedded failure 也继续下一 Provider/轮。
42. [ ] 覆盖恰好 20 MiB 与超出 1 byte 的 SSE/JSON，连续超限超过 100 attempts 后 retained bytes
    和 ring 大小稳定，失败 payload 不在 log/event/client 中。
43. [ ] 覆盖空轮后管理员新增/启用 Provider、轮中改序/禁用/改配置、开关与 interval 请求快照；
    同时验证 original body/session identity 不变。
44. [ ] 在 attempt、buffer、sleep 三个阶段测试 client cancel/disconnect/shutdown；断言无新 send、
    buffer drop、active count 归零、单父日志和 stop reason 正确。
45. [ ] 构造多 Provider 已知/未知 usage、不同 multiplier/price alias/priority、missing price、超大值
    和取消；核对 client final usage、父总量、Provider 子账本、quota 查询和 overflow flags。
46. [ ] 并发运行多个无限请求，证明无测试专用准入/排队/拒绝，活动数量准确，单请求 buffer cap
    相互独立；普通 Codex/Claude/Gemini、compact/system/warmup/token count 路径全量回归。

## H. 质量门、审查与回滚

47. [ ] 运行聚焦 Rust tests（state machine、route、response、settings、migration、request logs、
    active registry）和聚焦 Vitest（settings service、CodexTab、active snapshot）。
48. [ ] 运行 `pnpm check:generated-bindings`、`pnpm typecheck`、`pnpm lint`、`pnpm test:unit`、
    `pnpm tauri:fmt`、`pnpm tauri:check`、`pnpm tauri:clippy`、完整 Rust library suite 和
    `git diff --check`。
49. [ ] 使用 `trellis-check` 按 PRD AC1-AC21、settings ownership、upstream error、attempt budget、
    failover route 和 usage/cost 跨层合同审查；修复后重复相关质量门。
50. [ ] 用 `trellis-update-spec` 写入最终可执行合同：outer-round ownership、circuit-neutral test
    mode、final-wire commit gate、bounded ledger 和跨 Provider cost attribution。
51. [ ] 回滚验证：关闭开关后新请求完全走普通路径；活动请求只能由客户端取消或网关重启终止；
    migration 回滚不得删除已产生的 request logs/provider usage 数据。

## 启动前检查

- [x] `prd.md` 已无开放产品问题，AC1-AC21 覆盖所有需求。
- [x] `design.md` 已确定 round state machine、快照所有权、health mode、final-wire、日志和 UI。
- [x] 已明确先完成 `codex-stream-terminal-firewall`，本任务不与其并发实现共享 parser。
- [x] `implement.jsonl` 与 `check.jsonl` 已填入真实 spec/research 并通过 `task.py validate`。
- [ ] 用户完成最终规划审阅并明确同意开始；在此之前不得执行 `task.py start` 或修改业务代码。
