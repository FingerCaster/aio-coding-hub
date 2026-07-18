# Ponytail 与 Trellis 双模式集成设计

## 设计目标

在不改变 Trellis 调度模型的前提下，将 Ponytail 的最小实现纪律和复杂度审查
作为 Phase 2 的显式步骤。当前项目默认使用 Codex inline，保留切回 sub-agent
后立即可用的完整路径。

## 执行流

### Inline

```text
读取任务产物
  -> trellis-before-dev
  -> ponytail
  -> 主会话实现
  -> ponytail-review
  -> 应用不违反任务/spec 的精简建议
  -> trellis-check
```

`ponytail-review` 只提供复杂度发现；正确性、安全、测试和跨层一致性仍由
`trellis-check` 决定。

### Sub-agent

```text
主会话派发 trellis-implement
  -> agent 读取任务与 JSONL/spec 上下文
  -> agent 加载 ponytail
  -> agent 实现

主会话派发 trellis-check
  -> agent 读取任务与 JSONL/spec 上下文
  -> agent 加载 ponytail-review
  -> agent 处理可接受的精简建议
  -> agent 执行完整 Trellis 检查
```

既有 agent 的上下文读取、写权限、报告格式和递归派发保护保持不变。

## Skill 合同

### 共同优先级

从高到低：

1. 用户明确要求和已审阅的任务产物。
2. 项目 spec、正确性、安全、数据完整性、可访问性和必需测试。
3. Ponytail 的最小化建议。

两项 Skill 均只在 workflow 或 agent 明确要求时加载，不因普通编码请求自动触发，
也不跨步骤持久化。

### `ponytail`

- 保留需求必要性检查、仓库现有能力复用、标准库/原生能力优先、避免新依赖和
  最小正确改动。
- 保留修复共同根因而非表面症状的要求。
- 删除强度模式、会话持久化、固定输出长度和独立测试下限。
- 明确要求先加载 Trellis 任务与 spec，再做最小化判断；验证规模服从项目风险
  和质量门槛。

### `ponytail-review`

- 只审查不必要复杂度，保留 delete/stdlib/native/yagni/shrink 分类。
- 不建议删除任务或 spec 要求的实现、测试、安全、数据完整性和可访问性措施。
- 发现由调用方按自身写权限处理；Skill 不取代完整 review。
- 无可精简项时明确报告；随后仍继续 `trellis-check`。

## 文件边界

| 文件 | 责任 |
| --- | --- |
| `.agents/skills/ponytail/SKILL.md` | Trellis-scoped 最小实现规则 |
| `.agents/skills/ponytail/LICENSE` | 上游 MIT 许可 |
| `.agents/skills/ponytail-review/SKILL.md` | Trellis-scoped 复杂度审查规则 |
| `.agents/skills/ponytail-review/LICENSE` | 上游 MIT 许可 |
| `.trellis/config.yaml` | 将 Codex 默认值改为 `inline` |
| `.trellis/workflow.md` | 同步两种 Phase 2 路径和状态提示 |
| `.codex/agents/trellis-implement.toml` | sub-agent 加载 `ponytail` |
| `.codex/agents/trellis-check.toml` | sub-agent 先运行 `ponytail-review` 再完整检查 |
| `docs/trellis-ponytail-integration.md` | 可跨项目手工复用的集成流程 |

不新增 Skill 脚本、assets、UI metadata、hooks 或运行时状态。当前项目已有 Skill
均采用单一 `SKILL.md` 结构；额外文件仅保留第三方许可。

## 来源与升级

Skill 内记录上游 URL、固定提交和“已适配”声明，并要求复制时保留相邻 MIT
许可。升级时将新提交的两个原始 `SKILL.md` 与本地适配版逐项比较，人工判断
哪些核心规则可合入；不得覆盖本设计的作用域和优先级。

## 兼容与回滚

- `codex.dispatch_mode` 继续只接受 `inline | sub-agent`；切换配置即可选择路径。
- 现有 parser/hook 已支持两个值，本任务不修改其代码。
- 回滚默认模式只需把配置改回 `sub-agent`；移除集成则同时撤销 workflow/agent
  引用并删除两个项目级 Skill。
- `.agents/` 与 `.codex/` 在当前仓库被忽略，因此本机集成文件需与仓库内流程
  文档分别验证。

## 验证策略

1. 用 Skill Creator validator 检查两个 Skill 的 frontmatter、名称和目录结构。
2. 验证配置解析结果为 `inline`，并分别断言两个模式映射到正确的平台与状态块。
3. 检查 Phase 2 的 inline 输出包含完整顺序，sub-agent 输出保留派发规则。
4. 检查 implement/check agent 的上下文加载和递归保护未被改写。
5. 负向搜索确认未引入未选择的 Ponytail Skill、hooks、状态或插件配置。
6. 按文档做一次路径、升级和回滚步骤的静态走查。

