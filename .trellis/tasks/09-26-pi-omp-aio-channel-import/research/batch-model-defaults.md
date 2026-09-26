# 批量导入与模型默认参数

日期：2026-09-26；用户已授权在当前任务继续实施。

## 实现与证据

- NativeModelPicker 支持搜索、跨搜索保留勾选、全选当前结果、清空、批量追加及去重。主导入弹窗原有全选/清空继续保留。完整模型收起为摘要，缺项展开；所有参数仍可编辑。
- 只读发现补充 272 条 consumer/provider/protocol/model 目录记录，来自 Pi AI 0.87.1 发布包及 OMP 18.3.2 源码。仅抽取能力字段，无账号、endpoint 或认证内容。
- scripts/generate-native-model-defaults.mjs 生成 native_model_defaults.json，保留版本、OMP commit、源 JSON SHA256 和来源。MIT 许可随 resources/native-model-defaults.LICENSE.txt 分发。
- Pi 缺省档位语义来自 packages/ai/src/models.ts:getSupportedThinkingLevels；保留非同名及大小写映射。工具能力依据 OMP packages/catalog/src/types.ts 的明确语义（只有 false 禁用），该规则不用于普通 /models 响应。
- OMP 使用目录原始 mode/efforts/effortMap，默认等级先取明确默认，再选 medium、low 或首个有效等级。未声明的扩展能力不猜测。
- 精确匹配 consumer、来源对应 provider（含 API/OAuth 区分）、协议和模型 ID。不进行前缀/别名猜测、跨协议套用，不把整个目录发布为可用模型。Grok Chat Completions 没有同协议目录记录时仍依赖上游或手动声明。
- 优先级：已保存/手动草稿 > 来源显式配置 > 上游显式元数据 > 内置目录默认值。手动清空也保留；刷新更新未手改的自动值。上下文被明确收窄时，目录输出建议限制在该上下文内。
- 路由改写或能力冲突时不使用目录兜底；显式等级只能收窄 nativeThinking，明确关闭推理/工具时不重新启用。
- CCS PiProviderForm.tsx 仍以逐模型表单为主；本次参考其可编辑字段，不沿用逐个添加流程。

## 验证

- 真实 React 页面、Edge、本地 mock IPC 使用实际生成目录：Pi/OMP 一次导入 3 个模型，自动容量/思考、跨筛选勾选、手动修改后刷新、批量保存、窄屏和键盘焦点均通过。
- 已目视浅色/深色截图，证据在 .trellis/.runtime/research/omp-pi/channel-ui/batch-defaults/。
- 前端相关 34 项通过；Rust 全量 3189 项通过、6 ignored；其他检查与包路径见 validation.md。
- 未使用真实凭证或进行付费推理。旧单选包不包含本次改进。

交付：D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-232554；最终前端全量 3028 项通过，后端 3189 项通过（6 ignored），类型/静态检查及生成 bindings 一致性通过。
