# 技术设计：CX2CC 模型默认与槽位上下文

## 产品边界

CX2CC 四槽 mapper 继续只负责从 Claude 模型族选择最终 Responses model。本任务新增
的上下文是 CX2CC Provider 对同槽模型的显式能力声明，不参与 model rewrite，也不
改变请求 body。reasoning 链路保持现有 presence-preserving IR：做协议字段转换，
不做等级枚举转换。

## 数据模型与所有权

在 provider-scoped `ClaudeModels` JSON 增加四个可选字段：

```rust
pub main_context_window: Option<u64>;
pub haiku_context_window: Option<u64>;
pub sonnet_context_window: Option<u64>;
pub opus_context_window: Option<u64>;
```

- 不增加 `reasoning_context_window`，因为 CX2CC mapper 不使用 reasoning_model。
- 合法范围复用 provider capability 的 1,024..10,000,000。
- context 只允许 `bridge_type=cx2cc`，且要求同槽 model 显式存在。普通 Claude
  Provider 或 context-only 槽位严格拒绝。
- 旧 `claude_models_json` 缺字段时自然为 None；不新增 DB 列或 settings schema。
- Provider duplicate 与 config bundle 继续通过整个 JSON 保留字段并增加 round-trip
  回归。Provider share 新增 strict v5 wire；v1-v4 转换为 context None，新导出 v5。

前端仅在 CX2CC 的 `ClaudeModelSection` 为 main/haiku/sonnet/opus 显示 number input，
并与模型输入成对布局。切换默认模型或修改某槽 model 时清空该槽旧 context；API 若在
一次原子 upsert 中同时提交 model/context，则视为显式新配对。

## 默认与目录

- Rust `DEFAULT_CX2CC_FALLBACK_MODEL` 和 TypeScript
  `CX2CC_PROVIDER_DEFAULT_MODEL` 都改为 `gpt-5.6-sol`。
- 预设移除裸 `gpt-5.6`，有效 5.6 只保留 sol/terra/luna。
- 历史持久化值（包括 `gpt-5.5`、`gpt-5.4` 和自定义值）保持可编辑且不迁移；
  新建/空白 Provider 和新 settings 使用新默认。

## Context Projection

先按 mapper 真实优先级构造四条 `EffectiveCx2ccSlot`，不能预先仅按 model ID 去重：

1. provider 同槽 model 存在时使用它，否则使用 AppSettings 同槽 fallback。
2. provider 同槽 custom context 存在时直接得到该槽窗口；验证保证此时 model 也存在。
3. 无 custom context 时，为指定 source 或当前 AIO Codex 分流的全部稳定 provider
   identity 构造该 model 的 provider-scoped candidates，调用现有 catalog resolver。
4. 任一槽结果 Unknown，则整个终端进程结果 Unknown。
5. 所有槽已知时，若每个候选最终值相同返回 Exact；任一槽/provider 混合或值不同
   返回 Mixed，过程上限为所有已知值的最小值。

只在 Exact/Mixed 时向 Claude launcher settings 注入 MAX_CONTEXT 与 AUTO_COMPACT；
Unknown 省略。三个非 Claude alias 仍只控制模型族路由，不承载容量，也不新增
`ANTHROPIC_MODEL`。由于一个 Claude 进程只有一个窗口变量，四槽不同容量只能采用
保守最小值，不能宣称运行期会动态切换四个窗口。

## Reasoning Contract

当前链路保持不变并补边界测试：

| Claude 输入 | Responses 输出 |
| --- | --- |
| 无 thinking/effort | 无 reasoning |
| `output_config.effort=E` | `reasoning.effort=E` |
| enabled/adaptive + E | `reasoning.effort=E` |
| enabled/adaptive 无 E | 不猜测 effort |
| disabled | `reasoning.effort=none` |

`E` 可为已知值、`ultra` 或未来字符串，均原样透传。legacy
`cx2cc_model_reasoning_effort` 继续仅作 schema 兼容，运行时不读取。

## 失败与兼容

- Provider upsert 校验失败整体回滚；前端保留表单并显示错误。
- 自定义 context 是 operator 对当前 CX2CC Provider/槽位的显式能力声明；删除后
  立即恢复 provider-scoped catalog/unknown 语义。
- v5 Provider share 完整保留声明，旧版本导入为 None；未知未来 share 版本拒绝。
- config bundle 原样携带 `claude_models_json`，增加当前版本 round-trip 测试。
- 不修改 CX2CC mapper、configured routing 或 reentry 实现；既有测试作为回归门。

