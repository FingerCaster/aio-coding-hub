# 根因证据

## 运行时证据

- 运行版本：`0.60.41`，对应 tag 包含提交 `7a0ba2f4`。
- 开启无限重试测试模式后的 6 个请求全部以客户端取消结束，共 4608 次 attempt。
- 诊断汇总包含 693 次 `final_wire` 失败；典型 HTTP 200 attempt 的总时长约等于
  response-header latency + 500 ms，随后再经过配置的整轮间隔发起下一轮。
- 诊断会把 `final_wire_wall_clock_timeout` 归一化为 `invalid_final_wire`，因此文件日志只显示反复
  HTTP 200，结构化 request activity 才暴露失败类别。

## 代码证据

- `success_event_stream.rs:23-42`：500 ms 常量和 TTFB floor 断言。
- `success_event_stream.rs:159-189`：外层 wall-clock timeout 包裹完整 collector。
- `success_event_stream.rs:1381-1424`：timeout 变为 Provider/round 失败。
- `success_event_stream.rs:3170-3210`：现有测试只证明慢流必在 500 ms 失败，没有正常长流成功用例。
- `infinite_retry.rs:27-49`：supported path 与错误的 TTFB floor 元数据耦合。

## 合同证据

- 原始无限重试 PRD R15/AC15 要求使用现有首字节、stream idle、非流式 timeout，并明确禁止
  测试模式注入隐藏 timeout。
- 当前 upstream error handling spec 后来写入 500 ms 完整流 deadline，与原始合同和真实长流
  行为冲突，应随代码一起修正。

## Bug Analysis：把 TTFB 误作完整生成时限

### 1. Root Cause Category

- **主类别：E - Implicit Assumption**。实现假定“完整 SSE 必须在最小 TTFB 之前结束”，但 TTFB
  只约束首字节到达，不能表达模型生成完成时间。
- **促成类别：D - Test Coverage Gap**。原暂停时间测试只证明持续慢流会在 500 ms 失败，没有
  “持续有进展且最终合法完成”的正向长流用例；错误假设因此被测试与编译期断言共同固化。
- **合同漂移**。后写入的 upstream error handling spec 复制了实现中的 500 ms 限制，却没有与
  原始无限重试 R15/AC15 的“不得注入隐藏 timeout”重新核对。

### 2. Why Fixes Failed

1. 初始功能验证把“wall-clock cap 早于 TTFB floor”当成安全性质，只验证 cap 一定触发，没有验证
   正常 Codex 长生成能成功，因此测试通过反而掩盖了产品不可用。
2. 文件日志把 `final_wire_wall_clock_timeout` 归一化为 `invalid_final_wire`；只看 HTTP 200 和最终
   客户端取消会误判为 Provider 内容问题，必须结合结构化 attempt activity 和约 500 ms 的稳定
   时间特征才能区分。
3. 固定 cap、path 元数据和编译期断言形成了自洽但语义错误的局部模型；单纯调大常量仍会保留
   隐藏完整响应 deadline，不能满足原始合同。

### 3. Prevention Mechanisms

| 优先级 | 机制 | 具体动作 | 状态 |
| --- | --- | --- | --- |
| P0 | Architecture | path 列表只表达 eligibility，不再携带或派生完整响应 TTFB floor | DONE |
| P0 | Test Coverage | 暂停时间验证合法 SSE 持续推进 600 ms 后仍通过严格 validator | DONE |
| P0 | Test Coverage | 独立验证真实 inter-chunk gap 在配置的 idle budget 精确失败 | DONE |
| P0 | Documentation | 规范明确区分 response-header/first-chunk、per-read idle 与 whole-response deadline | DONE |
| P1 | Review | 任何新增流式总 deadline 必须具有独立语义、配置与正反向长流测试，不能复用 TTFB | DONE |

### 4. Systematic Expansion

- **相似问题审计**：搜索 gateway 中 TTFB、wall-clock 与 stream collector 的组合；生产路径剩余
  timeout 均围绕 response header、首个 SSE chunk 或逐次 read，未发现另一处用 TTFB 包裹完整
  流收集。`usage_tee` 中的短 timeout 命中均为测试等待边界。
- **设计改进**：完整响应 deadline 若未来成为产品需求，应作为独立、语义明确、可配置的策略，
  与首字节和 idle timeout 分开建模；无限测试模式当前只由逐读 idle、20 MiB、取消和关机约束。
- **流程改进**：修改时间预算时必须同时提供“应超时的停滞流”和“总时长更长但持续推进的成功
  流”，避免只有失败方向的暂停时间测试。

### 5. Knowledge Capture

- [x] 更新 `upstream-error-handling-contract.md` 的签名、合同、矩阵、案例、测试和 Wrong/Correct。
- [x] 新增长流成功与 idle gap 失败回归测试。
- [x] 将根因、日志判别方法和相似路径审计保存在本任务研究记录。
- [x] 确认该项目专属 spec 在 `src/templates/markdown/spec/` 中没有对应模板镜像，无文件可同步。
