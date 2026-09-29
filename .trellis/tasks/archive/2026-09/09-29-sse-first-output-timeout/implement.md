# 实施计划：SSE 首次有效输出单次期限

状态：2026-09-29 已批准并在独立 worktree 实施，全部质量检查通过，用户已确认本地提交和归档。单一交付链，使用一个任务；不拆父子任务、不派发子代理。

## 启动门槛

- [x] D1=A：复用生效流式空闲预算。
- [x] D2=单次：无总请求截止，原重试策略不变。
- [x] PRD、设计、执行计划及脱敏证据齐备。
- [x] 用户在最新最终摘要之后明确批准实现。
- [x] 批准后运行 task.py start；实质方案变化须重新评审。
- [x] 按 trellis-before-dev 读取三份文档、backend/cross-layer 索引、gateway-attempt-budget-contract、upstream-error-handling-contract、guides 的复用/跨层指南，保留其他未提交改动。

## 阶段 1：识别与前置期限

- [x] 定义仅服务本生命周期的有界 first_output 模块、绝对截止和状态。
- [x] 组合既有有效输出识别并显式补现场 custom-tool shape，共享 usage 分类默认不改。
- [x] 先补有效/无效帧、分块、CRLF、截止边界和持续 Ready 的 paused-time 测试。
- [x] 在普通 native Codex SSE 响应入口建立截止，使用同一 effective idle resolver，不改首字节、继承或默认数值。
- [x] 首块 probe 和 prefix 等待受同一新截止约束，明确区分 idle/guard/first-output 唤醒原因。
- [x] 前置到期复用原超时记录、策略匹配、计数、退避和切换，写正确阶段与秒数。
- [x] 定向验证保活请求必定结束，下一次尝试能恢复。

## 阶段 2：提前提交后的连续性

- [x] 保留 1 MiB 和解析兼容放行，移交未满足的原 deadline，不重新计时。
- [x] 可选状态接入 UsageSseTeeStream 与 Codex relay，其他入口保持 None。
- [x] 后段读取、持续 Ready 和发送背压响应同一截止；有效输出后移除新约束。
- [x] 到期释放上游，只终止当前响应；沿用尾帧所有权，不做提交后恢复、不重复 finalize。
- [x] 增加内部 FirstOutputTimeout 来源，更新 exhaustive match、探测收尾及 terminal-details JSON，公共错误码表保持不变。
- [x] 验证 cap/兼容放行、慢下游、完成/取消竞争与错误尾帧，无超时旁路或终结覆盖。

## 阶段 3：可见说明与完整回归

- [x] 全局与 Provider 帮助文本说明首次等待、保活不续命、单次范围及原继承/禁用。
- [x] 现有诊断可识别 first_output_timeout 和实际预算，不伪造或改写 TTFB。
- [x] 真实 router + mock upstream 测保活超时、同 Provider 重试及后续成功；跨 Provider 切换复用共享策略全量回归。
- [x] 验证配置/0、边界、路径别名、桥接/compact/内部重入/无限重试的隔离；复用既有覆盖并扩展配置边界。
- [x] 验证 usage、计费、空响应、防火墙与原始字节转发行为。
- [x] 使用 trellis-update-spec 更新现有两份应用契约，区别新首输出期限、byte idle、guard 与完整生成预算；需要记录根因时遵循 trellis-break-loop。

## 验证命令

从仓库根目录运行。命令来自 package.json；规划阶段不宣称已经执行或通过。

1. pnpm tauri:test --lib first_output
2. pnpm tauri:test --lib buffered_native_stream
3. pnpm tauri:test --lib effective_stream_idle_timeout
4. pnpm tauri:test --lib
5. pnpm test:unit src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx src/pages/providers/__tests__/ProviderEditorDialog.test.tsx
6. pnpm typecheck
7. pnpm lint
8. pnpm tauri:fmt
9. pnpm tauri:clippy
10. pnpm check:generated-bindings
11. pnpm check:gateway-error-codes
12. pnpm check:no-instant-now-sub
13. pnpm check:spec-links
14. pnpm exec prettier --check <明确的变更前端/文档文件列表>
15. git diff --check

first_output 是拟新增测试的统一过滤词，其余过滤词来自现存测试。tauri-test.mjs 调用 cargo test --locked 并使用 target-tests，避免干扰运行程序。真实路由用例加外层限时；不向真实 Provider 重放用户请求。全量 Rust 回归覆盖共享流处理与 failover，验证完成后不无故扩大重复测试。

## 文件与审查重点

| 文件 | 重点 |
| --- | --- |
| gateway/proxy/handler/failover_loop/response/success_event_stream.rs | 资格、起点、优先级、恢复、截止交接 |
| gateway/streams/first_output.rs（新） | 绝对时间、shape、有界解析、不记录 payload |
| gateway/streams.rs / usage_tee.rs | 可选接入、背压、尾帧、一次终结 |
| gateway/streams/types.rs / finalize.rs | 内部来源、错误/探测映射，不误用首字节秒数 |
| gateway/routes.rs 与 stream 模块 tests | 真实路径、次数、取消、隔离 |
| GeneralTab.tsx / ProviderEditorDialog.tsx 与 tests | 字段和值不变，说明准确 |
| 应用 backend/cross-layer spec | 新旧计时契约一致 |

Rust 路径以 src-tauri/src 为根；实现前复核实际文件与局部模式，不照表重建已有模块。设计不要求修改公开生成绑定；检查用于证明这一点。

## 风险与回滚点

- 阶段 1 的 helper/prefix 接线可在本任务 diff 内回退；阶段 2 的 relay/origin 需一起回退，避免只撤一半。
- 不修改 .trellis/config.yaml、09-05 旧任务、真实 settings.json、数据库或安装实例，不用 reset --hard / clean 处理现有工作区。
- 如果需要独立设置、公共错误码、其他协议重构或总请求截止，停止扩大范围，修订规划并重新评审。
- Inline 使用 trellis-check 直接验证，不派发子代理；记录实际通过/失败，不将未运行的命令记成通过。
- 当前已完成实现验收；Phase 3.4 已确认，工作提交 df8e0a66，按计划归档。本地提交遵循 AGENTS.md 的动态 node/pnpm hook PATH 规则。

## 完成定义

AC1—AC10（含 AC1a）均有测试或兼容性证据；保活不能无限延长当前首输出等待；新预算只属于单次尝试和首次输出；原输出、恢复、计费及终止契约无回归，诊断可解释，必需检查完成。

## 实际验证结果

见 [实现与验证记录](research/implementation-review.md)。功能实现与全部质量检查完成；工作提交 df8e0a66，用户已批准任务归档及 journal。当前安装实例未替换。
