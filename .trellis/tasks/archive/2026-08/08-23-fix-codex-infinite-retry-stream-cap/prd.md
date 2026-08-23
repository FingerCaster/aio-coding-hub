# 修复 Codex 无限重试流完成超时

## Goal

修复 Codex 无限重试测试模式对完整 SSE 响应施加 500 ms 总墙钟时限的问题，使持续有进展、
最终产生合法 `response.completed` 的正常长流可以通过 final-wire 校验并一次性交付客户端，
同时保持测试模式现有的提交前完整缓冲、安全校验和资源边界。

## Background

- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:23-42`
  定义 `INFINITE_FINAL_WIRE_WALL_CLOCK_CAP=500ms`，并把完整流完成时限错误地约束在最小
  TTFB 之下；TTFB 只描述首字节到达，不描述生成完成。
- 同文件 `:159-189` 用外层 `tokio::time::timeout` 包裹整个 final-wire 收集过程，
  `:1390-1424` 将超时作为 `final_wire_wall_clock_timeout` 失败并切换 Provider。
- 本机稳定版 `0.60.41` 的诊断记录显示，开启模式后的 6 个请求累计 4608 次尝试，所有请求
  最终均由客户端取消；其中 693 次已经收到 HTTP 200 的流被记为 final-wire 失败。典型尝试在
  收到响应头约 500 ms 后进入下一轮，与该硬编码时限一致。
- 原始功能合同 `.trellis/tasks/08-10-codex-infinite-retry-test-switch/prd.md:45-48,76-79`
  要求使用既有首字节、stream idle、非流式响应超时，且不得为测试模式注入隐藏超时。
- 当前 `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:136-141`
  反而固化了 500 ms 总时限，已与原始 R15/AC15 和实际 Codex 长流行为发生漂移。

## Requirements

- R1：无限重试测试模式不得再对完整 SSE 收集施加固定 500 ms 或其他隐藏的总墙钟时限；只要
  每次读取在有效 stream idle timeout 内持续推进，就允许该 attempt 超过 500 ms 后正常完成。
- R2：保留既有每次读取的 stream idle timeout。连续无新数据达到配置值时，当前 attempt 仍应
  以 `final_wire_idle_timeout` 失败并进入既有 Provider/整轮重试流程；配置为 `0`/未设置时继续
  表示不启用该超时。
- R3：保留 20 MiB final-wire 上限、decode/bridge/fixer/plugin 顺序、严格 Codex SSE 完成校验、
  成功前不提交任何下游字节，以及失败 buffer 释放语义。
- R4：保留首字节 timeout、客户端取消、网关关闭和普通非无限模式行为；本修复不得改变普通
  Codex/Claude/Gemini 流式路径、Provider-local retry、circuit/cooldown 或整轮重试间隔。
- R5：移除只为错误总时限服务的 supported-path TTFB floor 数据和编译期断言；支持的 Codex
  Responses path 仍由一个共享常量维护，eligibility 与 event-stream path 判断继续复用它。
- R6：更新 upstream error handling spec，明确无限模式的 final-wire collector 由 per-read idle
  timeout、20 MiB 上限和取消/关闭信号约束，不把完整响应时间与 TTFB 比较。
- R7：增加暂停时间回归测试，覆盖一个合法 SSE 在持续进展下超过旧 500 ms 边界仍成功，以及
  真正的 idle gap 仍按配置超时。

## Acceptance Criteria

- [x] AC1：一个分段到达、总时长大于 500 ms 且最终包含唯一合法 `response.completed` 的 SSE
  能被完整收集，并通过严格 final-wire validator。（R1、R3、R7）
- [x] AC2：分段间隔超过配置 stream idle timeout 时仍返回 `IdleTimeout`，且触发时刻等于配置
  的 idle budget，不受已移除的旧总时限影响。（R2、R7）
- [x] AC3：生产代码中不再存在 `INFINITE_FINAL_WIRE_WALL_CLOCK_CAP`、`WallClockTimeout`、
  `final_wire_wall_clock_timeout` 或 TTFB-floor 编译期断言。（R1、R5）
- [x] AC4：20 MiB 上限、严格终态校验、首字节 timeout、取消/关闭和普通流式路径的现有测试
  保持通过。（R3、R4）
- [x] AC5：共享 Codex Responses path 列表继续覆盖 `/v1/responses`、`/responses`、
  `/v1/codex/responses` 及尾斜杠 eligibility，用例保持通过。（R5）
- [x] AC6：upstream error handling spec 与原始无限重试 R15/AC15 一致，不再要求完整 SSE 在
  TTFB 阈值内结束。（R6）

## Out of Scope

- 新增可配置的完整响应总 timeout 或修改设置/UI/schema。
- 放宽严格 final-wire 内容、终态、response ID、`[DONE]` 或可见载荷安全校验。
- 修改整轮 retry interval、Provider 排序、attempt budget、circuit/cooldown 或 usage 计费。
- 修改 Codex reasoning guard / continuation repair；本任务只修复无限重试测试模式的完整流收集。
- 清理历史日志、数据库记录或本机已取消请求。
