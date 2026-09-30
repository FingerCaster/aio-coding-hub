# 实现与验证记录

日期：2026-09-29。状态：实现及全部质量检查完成；用户已确认本地提交和归档，工作提交 df8e0a66。

## 位置与范围

- Worktree：E:/OrcaProject/aio-coding-hub-fork/sse-first-output-timeout
- 分支：FingerCaster/sse-first-output-timeout
- 基线：fb1883f7b78f8b4d2e77040e7f54b5574f22e352（0.60.44）
- 原 main 工作区的产品代码未改动；保留原有 config.yaml 和其他任务。
- 按批准的 A / 单次方案实现，没有新增设置、迁移、公开错误码或总请求期限。

## 实现结果

首次有效输出使用已生效的流式空闲预算，收到符合资格的 SSE 响应头后建立绝对截止。
空状态、注释和心跳不续期；正文、拒绝、摘要、函数参数和已确认 custom-tool 进展可解除期限。
保留原首字节、500 ms guard、字节 idle 和恢复策略，重试独立计时。

未提交的首次输出超时走原 transport Timeout 重试/退避/切换路径，记录真实生效秒数。
缓冲 cap 或兼容放行后传递原截止；读取持续 Ready 或下游背压均不能绕过它。
已提交后释放上游、仅终止当前流、一次收尾，错误尾帧走 relay 所有权。

客户端已发 HTTP 200 保持不变；现有请求日志会把超时投影为 524，并记录
terminal_origin=first_output_timeout。before_commit / after_commit、预算和来源写入现有 JSON。
全局/Provider 设置说明已更新，计费、TTFB 和共享 usage 分类未改。

## 验证

| 检查 | 结果 |
| --- | --- |
| 两个直接修改的前端测试文件 | 108 项通过 |
| 前端全量，maxWorkers=2 | 332 文件、3084 项通过 |
| Rust 首轮全量 | 3229 项通过，10 项原有忽略 |
| 新增 first_output 回归 | 11 项均在首轮全量中通过 |
| typecheck / lint / production build | 通过 |
| 生成绑定一致性 | 通过，bindings.ts 无变化 |
| 错误码同步 / Instant 安全检查 / spec-links | 通过 |
| Rust 格式 / 前端 Prettier / diff whitespace | 通过 |
| 最终 Rust 全量与定向复验 | 3229 通过 / 10 原有忽略；first_output 11、buffered_native_stream 10、effective_idle 1 全部通过 |
| cargo check / strict Clippy | 通过，Clippy --all-targets --locked -- -D warnings |

生产构建保留已有 Browserslist 陈旧和大 chunk 提示，无构建错误。
首轮测试曾修正两处测试问题：日志 524 与线上 HTTP 200 的既有口径区别；
paused-time 不应以 50 ms 虚拟时钟竞速阻塞数据库日志线程。后者改为先验证
原虚拟截止时已释放上游，再用有界真实时间接收日志。

## 验收对应

- AC1/1a：300 秒保活模拟、正值覆盖/0/空值/禁用组合，真实路由 1 秒 Provider 覆盖。
- AC2：首块先后期限、分块 LF/CRLF、边界后增量、始终 Ready、客户端取消同刻优先。
  gzip/compact/内部重入等沿用完整 Rust 回归。
- AC3/8：custom-tool 进展永久解除；真实第二次尝试在 600 ms 有进展，guard 与完成跨过 1 秒。
- AC4/5：真实路由验证同 Provider 重试/耗尽/失败前缀丢弃；原切换、退避、取消和停止由共享回归覆盖。
- AC6：沿用原 native Codex 路径资格，显式排除 bypasses_circuit；原桥接/无限收集等全量回归保持。
- AC7：全部复现使用合成数据和本地 HTTP；未重放真实 Provider 请求。
- AC9：超过 1 MiB 的元数据提前提交后仍超时；背压移交保留原截止，上游及时释放，无提交后重试。
- AC10：独立错误来源、单次日志终结、网关错误尾帧绕过上游防火墙；既有 usage/计费回归通过。

## 交付状态

修复当前位于独立 worktree，当前安装的应用尚未使用这份源码。
全部质量检查已完成，用户于 2026-09-29 批准 commit-plan.md 的三个本地提交。工作提交为 df8e0a669658e2e7fa9dedd674cf3335e2d8604c，提交钩子 lint/typecheck/Instant/cargo check 通过；随后按 trellis-finish-work 归档当前任务并记录 journal，均在独立分支完成。
