# 实施计划

## 1. 建立基线

- 记录当前脏工作树，只操作本任务列出的文件。
- 读取任务产物、Ponytail 来源研究和 cross-layer thinking guide。
- 确认 `.agents/skills/ponytail*` 尚不存在，避免覆盖本地 Skill。

## 2. 创建适配 Skill

- 创建 `.agents/skills/ponytail/SKILL.md`，实现 Phase 2 限定作用域、优先级和
  最小正确实现 ladder。
- 创建 `.agents/skills/ponytail-review/SKILL.md`，实现复杂度补充审查合同。
- 为两个 Skill 各保留相邻的上游 MIT `LICENSE`，并在 Skill 中记录固定提交。
- 不创建脚本、references、assets、UI metadata 或其他 Ponytail Skill。

## 3. 接入双模式工作流

- 将 `.trellis/config.yaml` 的 `codex.dispatch_mode` 改为 `inline`。
- 更新 `.trellis/workflow.md` 的 Phase Index 状态提示、Active Task Routing、
  Phase 2.1 和 Phase 2.2，使 inline 顺序完整且 sub-agent 语义明确。
- 保留现有 Phase 1、任务生命周期和 Phase 3 行为。

## 4. 接入 Codex sub-agent

- 在 `.codex/agents/trellis-implement.toml` 完成任务/spec 上下文加载后，明确读取
  并应用 `ponytail`，不改递归保护或写边界。
- 在 `.codex/agents/trellis-check.toml` 完成上下文加载后，先读取
  `ponytail-review`、处理合规的精简项，再执行现有完整 review/checklist。
- 比较修改前后 agent，确认原有上下文、权限、验证和报告合同全部保留。

## 5. 编写复用文档

- 新增 `docs/trellis-ponytail-integration.md`。
- 覆盖前置条件、来源固定、选择性复制、适配原则、平台路径判断、inline 与
  sub-agent 修改点、优先级、验证、升级、移除和回滚。
- 明确其他项目必须逐项目应用和验证，不声称自动传播。

## 6. 验证

- 对两个 Skill 运行 `quick_validate.py`。
- 调用 Trellis 配置解析器，确认当前默认值为 `inline`。
- 分别用 `inline` 与 `sub-agent` 配置对象验证 breadcrumb key 和 Codex 有效平台
  解析结果。
- 运行 `get_context.py --mode phase --step 2.1 --platform codex` 与 2.2，检查
  inline 的 Skill 顺序；再静态检查 sub-agent workflow 与两个 agent 文件。
- 搜索集成文件，确认只存在两个选中 Skill 的引用，且未新增 Ponytail hook、
  插件或状态配置。
- 运行
  `python ./.trellis/scripts/task.py validate 07-18-integrate-ponytail-trellis-workflow`。
- 检查 `git diff` 与 ignored 文件的显式 diff，只审阅本任务改动。

## 风险与回滚点

- Skill description 过宽会造成自动触发；验证必须检查“仅 workflow/agent 显式
  加载”的措辞。
- workflow 状态块与 Phase 2 正文必须同步，否则每轮提示和详细步骤会分歧。
- agent 修改不得放在上下文加载之前，也不得弱化递归保护。
- 任一步验证失败时先回退对应局部文件；不要修改 Trellis parser、npm 包或应用
  源码来补偿本地工作流问题。
