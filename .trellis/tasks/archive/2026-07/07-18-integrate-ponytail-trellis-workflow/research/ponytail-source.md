# Ponytail 来源与适配评估

## 来源快照

- 上游仓库：`https://github.com/DietrichGebert/ponytail`
- 本地检出：`D:/UGit/ponytail`
- `package.json` 版本：`4.8.4`
- 固定提交：`16f29800fd2681bdf24f3eb4ccffe38be3baec6b`
- `git describe`：`v4.8.4-53-g16f2980`
- 评估时工作树：干净
- 许可：MIT，Copyright (c) 2026 DietrichGebert

不可变提交是本次集成的权威来源；包内版本号只作为上游元数据记录，不能替代
提交固定值。

## 选择范围

| 能力 | 上游文件 | 采用方式 |
| --- | --- | --- |
| 最小实现约束 | `skills/ponytail/SKILL.md` | 创建 Trellis-scoped 适配版 |
| 复杂度审查 | `skills/ponytail-review/SKILL.md` | 创建 Trellis-scoped 适配版 |

不采用插件、lifecycle hooks、运行时模式状态，以及 audit、debt、gain、help
Skill。两个选中的 Skill 本身不要求这些组件即可工作。

## 需要适配的语义

| 上游语义 | 与 Trellis 的冲突 | 适配规则 |
| --- | --- | --- |
| 任意编码任务自动触发 | 会绕过 Trellis 阶段路由 | 仅由 Phase 2 或对应 agent 明确加载 |
| 全会话持续生效 | 会影响规划、研究和收尾 | 仅作用于当前实现或复杂度审查步骤 |
| 固定强度与启停命令 | 引入额外状态模型 | 不引入强度和持久状态 |
| 最多三行输出 | 会覆盖 Trellis 报告契约 | 服从调用方 workflow/agent 的输出要求 |
| 最小化测试倾向 | 可能削弱项目质量门槛 | spec、任务和风险要求的测试优先 |
| review 只检查复杂度 | 不覆盖完整质量检查 | 始终作为 `trellis-check` 前置补充 |

保留的核心是先理解真实代码路径，再按“不需要新增、复用现有、标准库、原生
能力、已安装依赖、最小正确改动”的顺序选择方案；修复缺陷时仍定位共同根因。

## 当前 Trellis 证据

- `.trellis/workflow.md` 已有 `codex-inline` 和 `codex-sub-agent` 两条 Phase 2
  路径，无需增加第三种模式。
- `.trellis/config.yaml` 当前显式选择 `sub-agent`；本任务将其改为 `inline`。
- `.codex/hooks/inject-workflow-state.py` 和
  `.trellis/scripts/common/workflow_phase.py` 已识别 `inline | sub-agent`，缺省值为
  `inline`。
- Codex 使用 `.agents/skills/` 读取共享 Skill，并使用
  `.codex/agents/trellis-{implement,check}.toml` 定义 sub-agent 行为。
- 当前仓库忽略 `.agents/` 与 `.codex/`。这些是本机 Trellis/Codex 集成文件，
  不通过本仓库 Git 自动传播；其他项目需要按流程文档逐项目应用。

