# 集成 Ponytail 到 Trellis 双模式工作流

## Goal

在当前项目中选择性引入 Ponytail 的最小实现与复杂度审查能力，使 Trellis 的
Codex `inline` 和 `sub-agent` 两种执行模式都能在不依赖 Ponytail 插件或 hooks 的
前提下使用这些能力；当前项目默认切换为 `inline`，同时保留可切回
`sub-agent` 的完整路径。产出一份可复用流程文档，供其他 Trellis 项目按需手工
完成同类集成。

## Background

- 当前项目使用 Trellis `0.6.6`，`.trellis/config.yaml` 的
  `codex.dispatch_mode` 当前为 `sub-agent`，本任务完成后默认改为 `inline`。
- 当前 `.trellis/workflow.md` 已分别定义 `codex-inline` 与
  `codex-sub-agent` 路径，因此应在两条既有路径中增加 Ponytail，而不是删除任一
  模式或引入第三种调度模式。
- Ponytail 源仓库的 `package.json` 声明版本为 `4.8.4`；本次评估固定在提交
  `16f29800fd2681bdf24f3eb4ccffe38be3baec6b`。六个 Skill 均是独立的
  `SKILL.md`，日常 Trellis 开发闭环只需要核心 `ponytail` 与
  `ponytail-review`。
- Ponytail 原始核心 Skill 带有全会话持久化、广泛自动触发和“最小测试”倾向；
  直接原样引入可能与 Trellis 的阶段门禁、任务产物和项目级测试契约冲突。

## Requirements

### R1. 选择性 Skill 集成

- 只引入实现阶段需要的 Ponytail 核心能力和 diff 复杂度审查能力，并采用
  Trellis-scoped 适配版本，而不是原样 vendoring。
- 适配版保留最小实现、复用现有能力、根因修复和复杂度审查；移除全会话持久
  化、过宽自动触发、固定输出篇幅及可能削弱项目测试要求的语义。
- 不安装 Ponytail Codex 插件，不导入 `SessionStart`、`UserPromptSubmit`、
  `SubagentStart` hooks，也不使用 Ponytail 模式状态文件。
- 不导入 `ponytail-audit`、`ponytail-debt`、`ponytail-gain` 或
  `ponytail-help`。
- Skill 必须仅在 Trellis 工作流明确要求时生效，不能因普通规划、研究、提交或
  收尾请求而隐式进入持久模式。
- PRD、design、implement、项目 spec、必需测试、正确性、安全、数据完整性和
  可访问性约束始终优先于 Ponytail 的精简建议。

### R2. Inline 模式

- 当前项目默认将 `codex.dispatch_mode` 设置为 `inline`。
- Phase 2 实现顺序必须在读取任务产物和 `trellis-before-dev` 规范之后使用
  Ponytail，再进行实现。
- 质量检查必须先评估 Ponytail complexity review 的建议，仅接受不违反任务和
  spec 的精简，再运行完整 `trellis-check`。

### R3. Sub-agent 模式

- 保留 `codex.dispatch_mode: sub-agent` 的兼容路径，用户以后切回该值时无需重新
  修改工作流或 Skill。
- `trellis-implement` 在完成活动任务、任务产物和 JSONL/spec 上下文加载后使用
  Ponytail 核心能力，再实施代码。
- `trellis-check` 使用 Ponytail complexity review 作为补充检查，但仍以 Trellis
  的正确性、spec、lint、类型检查和测试为最终质量门槛。
- Ponytail 不得改变 implement/check 子代理的角色边界、上下文加载顺序、写权限
  或递归派发保护。

### R4. 可复用流程文档

- 新增一份独立流程文档，说明其他 Trellis 项目如何手工完成同类集成。
- 文档必须覆盖前置条件、选择性复制的文件、Skill 作用域适配、inline 与
  sub-agent 修改点、不同平台 Skill/agent 路径的判断方式、优先级规则、验证、
  升级同步、移除和回滚。
- 文档必须使用通用 Trellis 术语和占位路径，不写入当前会话案例或其他与
  Ponytail 集成无关的内容。
- 文档不得声称修改一次即可自动更新其他项目；其他项目按文档逐项目应用并验证。

### R5. 来源、许可与可维护性

- 记录所采用 Ponytail 内容的上游仓库、版本或不可变提交，以及本地适配差异。
- 保留适用的 MIT 许可信息。
- 后续升级必须通过显式 diff 重新评估上游变化，不能静默覆盖 Trellis 作用域与
  优先级约束。

## Acceptance Criteria

- [ ] 当前项目默认 `codex.dispatch_mode` 为 `inline`，hook 注入能够选择
      `planning-inline` / `in_progress-inline` 工作流块。
- [ ] Inline Phase 2 按“加载任务/spec -> Ponytail 实现约束 -> 实现 -> Ponytail
      complexity review -> Trellis 完整检查”执行。
- [ ] 切回 `sub-agent` 时，implement 与 check agent 分别加载正确的 Ponytail
      能力，且原有上下文、权限和递归保护不回归。
- [ ] 规划、研究、提交和收尾阶段不会自动启用 Ponytail。
- [ ] 项目中没有新增 Ponytail lifecycle hook、插件配置、全局状态文件或四个未选
      Skill。
- [ ] Ponytail 建议不能削弱任务产物、项目 spec、必需测试、安全、数据完整性或
      可访问性要求。
- [ ] 可复用流程文档能指导另一个 Trellis 项目分别完成 inline 与 sub-agent
      集成，并包含验证、升级和回滚步骤。
- [ ] 所有新增/修改的 Skill、workflow、agent 和文档通过格式、引用一致性及针对性
      回归检查。

## Out of Scope

- 修改或发布 Trellis npm 包、Trellis 上游源码或 Trellis marketplace。
- 修改其他项目；本任务只提供可复用流程文档。
- 集成 Ponytail 插件、hooks、运行时模式切换、状态栏或营销/审计/债务功能。
- 引入或说明与 Ponytail/Trellis 双模式集成无关的调度方案。
