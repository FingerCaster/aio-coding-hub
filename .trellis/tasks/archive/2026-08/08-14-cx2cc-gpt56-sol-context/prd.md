# 修正 CX2CC GPT-5.6 Sol 默认与模型上下文

## Goal

修复 Beta 5 中把当前供应商目录不提供的裸 `gpt-5.6` alias 暴露为可选模型的问题，将 CX2CC
的新建/空白默认统一为 `gpt-5.6-sol`，并让用户能在每个 CX2CC Provider 的
四个有效模型槽位中自定义上下文窗口，供 Claude Code 启动时安全计算压缩边界。

## Background

- 已归档任务 `08-13-cx2cc-routing-gpt56` 完成了 CX2CC 单次模型映射、内部网关
  回环隔离、reasoning presence 透传和 provider-scoped context projection。
- 当前预设错误包含供应商选择器不可用的裸 `gpt-5.6` alias；本项目应暴露的 5.6 型号为 `gpt-5.6-sol`、
  `gpt-5.6-terra`、`gpt-5.6-luna`。
- Rust 与前端的新建默认仍是 `gpt-5.5`，与产品要求不一致。
- 已发现能力目录可以提供上下文窗口，但自定义/尚未发现的模型会保持 unknown；
  用户需要在 CX2CC 设置页按模型精确覆盖该容量。
- 当前 CX2CC 已把 Claude `output_config.effort` 转成 Responses
  `reasoning.effort` 并保留原值；本任务必须锁定该行为，不能恢复固定等级覆盖。

## Requirements

- R1：从所有用户可见的 CX2CC 模型预设和选择项移除裸 `gpt-5.6`；保留
  `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5`、`gpt-5.4`
  及手动模型能力。
- R2：Rust 共享 fallback 默认与前端 Provider 默认必须统一为
  `gpt-5.6-sol`。新建、空白和真正缺省配置使用该值；已有显式持久化模型值
  不做破坏性覆盖。
- R3：在 CX2CC Provider 的 `main/haiku/sonnet/opus` 四个有效 mapper 槽位增加
  可选上下文窗口；`reasoning_model` 不是 CX2CC mapper 槽位，不得增加对应窗口。
  值必须是整数 token 数，范围与 provider model capability 保持一致
  （1,024..10,000,000）。
- R4：自定义窗口只允许 CX2CC bridge 保存，普通 Claude Provider 必须拒绝；窗口
  必须与同槽显式 model 一起存在，避免分享后绑定到接收端不同的全局 fallback。
- R5：每槽投影优先级为该 CX2CC Provider 的显式 slot context > 对最终 model 的
  fresh/configured discovered catalog > unknown。覆盖不能从一个槽位复制到另一个；
  UI 改变槽位 model 时清空旧 context，避免容量继续绑定新模型。
- R6：Claude terminal 是单进程窗口，四个 CX2CC mapper 槽位及当前分流的全部
  可达候选都必须参与计算。任何未覆盖且目录未知的候选使结果为 Unknown；全部
  已知但容量不同则使用最小值并标记 Mixed；全部同值才是 Exact。
- R7：Exact/Mixed 才注入 `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 与
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW`；Unknown 不注入伪造容量。模型映射、来源
  Provider、普通 Codex/Claude 路由和内部回环安全不变。
- R8：CX2CC 继续进行协议字段转换，但不进行 effort 枚举换算：
  `output_config.effort = E` 原样成为 `reasoning.effort = E`；唯一语义转换为
  `thinking.type=disabled -> none`。缺省不造值，legacy 固定设置不得覆盖请求。
- R9：新增字段必须贯通 Rust `ClaudeModels` JSON、Provider create/update/read/
  duplicate、生成绑定、前端 Provider 编辑器、配置 bundle 与 Provider share。
  Provider share 升为 v5；v1-v4 导入窗口均为 None，新导出严格使用 v5。

## Acceptance Criteria

- [x] CX2CC 预设与 Provider 选择中不再出现裸 `gpt-5.6`，默认显示并保存
  `gpt-5.6-sol`，前后端默认契约测试一致。
- [x] CX2CC Provider 编辑器在四个有效模型旁可编辑上下文窗口；普通 Claude 不显示
  且后端拒绝该字段，错误输入不会污染已提交 Provider。
- [x] 旧 Provider 缺字段时窗口为 None；duplicate、配置 bundle 与 Provider share
  v5 保留上下文，v1-v4 导入兼容且未来版本严格拒绝。
- [x] 覆盖命中、目录回退、混合最小值、unknown、动态多 Provider、四槽去重均有
  Rust 回归，且只有 Exact/Mixed 注入终端窗口环境变量。
- [x] CX2CC effort 的 absent/disabled/known/unknown/`ultra` 透传矩阵通过；没有
  固定 legacy fallback，也没有新增等级重命名或静默降级。
- [x] 普通 Provider 路由、CX2CC 单次模型路由、合法内部 reentry 和真实回环拒绝
  的既有测试保持通过。
- [x] 前端定向测试、TypeScript、生成绑定、Rust 定向/全量测试及 pre-push 门通过。

## Notes

- 这是一项跨设置、Rust 投影、生成绑定和 React UI 的复杂回归任务，必须有
  `design.md`、`implement.md` 和 implement/check context manifests。
- 不从模型名称猜测上下文，不硬编码 5.6 型号容量，不恢复 CX2CC 通用模型路由。
