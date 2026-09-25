# 继续合并上游功能并保留 Codex 多模型支持

## Goal

继续整合上次同步后尚未纳入的上游功能。普通模型路由采用上游实现与行为；Codex 支持其他模型继续使用当前 fork 的 Provider 模型、受管 Profile 和目录机制。保留其他明确的 fork 产品决策，合并只处理整合所必需的兼容和冲突。

## Requirements

- R1：用户于 2026-09-25 明确允许采用上游模型路由，同时保留当前 fork 的 Codex 多模型支持方式。最终规划已完成；用户于 2026-09-25 明确批准“09-25-upstream-feature-integration 批准这个任务开始吧”，授权按现有 PRD、设计和执行计划进入实施。
- R1.1：用户选择路由兼容适配：保留全局规则、Provider 继承/专属/明确关闭和 `reasoning_effort`，运行时接入上游模型匹配与 Provider 预筛选；不做破坏性持久化切换。
- R1.2：用户选择保留当前 fork 的 Codex `Actual` 优先计费语义；上游 priority billing source 只作为可复用解析/选择器参考，不改变默认费用含义。
- R1.3：用户选择保留当前 fork 的 thinking-signature 窄触发条件；只处理包含 thinking/signature/redacted 线索的错误，同时接入上游安全的 Responses `input` 规范化，不扩大普通 400 的重试范围。
- R2：核对并固定 upstream/main 的确切 SHA，延续已完成的三个低风险补丁，纳入剩余无冲突功能并记录被本地实现覆盖的部分。
- R3：保留 Codex Provider/模型稳定身份、受管 Profile、aio/* 别名、目录所有权与恢复、显式能力、模型上下文规则和已选择的 Provider name。具体接入边界以代码证据和最终设计为准。
- R4：出现新的 fork 产品行为冲突时，给出具体文件、行为差异和可选方案后请用户决定；不得默选上游或本地。
- R5：保持 fork 发布版本和单调数据库迁移，不覆盖已有用户数据，不改动其他任务的未提交文件。
- R6：不修复固定上游版本本身已有且与整合无关的缺陷；将其记录为范围外发现。upstream 保持 fetch-only，仓库操作默认使用 origin。

## Acceptance Criteria

- [x] AC1：记录固定上游 SHA、当前 fork 基线、已接入/新增/覆盖/排除的功能清单和每项依据。
- [x] AC2：普通模型路由符合最终确认的上游行为，保存、导入导出、运行时、日志和费用使用同一契约。
- [x] AC3：Codex 多模型目录、受管 Profile、普通请求与 aio/* 请求各自通过回归，现有目录和身份不被上游实现接管。
- [x] AC4：剩余功能经过逐项冲突分类，所有影响 fork 行为的决策有用户依据。
- [x] AC5：所需迁移保留用户配置，运行范围匹配的前后端测试、绑定生成检查、类型和格式检查；明确区分整合回归与上游已有缺陷。
- [x] AC6：最终规划包含 prd.md、design.md、implement.md 和已整理的上下文清单，在用户批准后才进入实施。
- [x] AC7：Codex priority/default 费用默认仍按 Actual 优先；thinking-signature 整流仍为窄触发，普通 generic 400 不新增重试。

## Background

- 当前 fork 起点为 270b808c；上一批低风险补丁由 071d9e79 纳入，相关任务已归档。
- 本次已核对上游版本仍为 `420e9958091ae460d152a508b1eb0e2110ab733b`，记录在 `.trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/research/upstream-sync.md`，并以该 SHA 作为固定整合输入。
- 本次开始前工作区已有 Astra 流回复任务的 7 个未跟踪规划文件，属于其他任务。

## 验收证据与收尾状态

- 逐项整合清单与验证结果：research/integration-audit.md。
- Rust：3077 通过、4 ignored、明确排除 1 项基线旧失败；最后的设置兼容调整另以全部 42 项 settings_service 用例验证。
- 前端：313 文件、2931 项全量测试通过；类型、Lint、改动文件格式、Rust fmt、Windows all-targets Clippy 和绑定一致性通过。
- 既有 fork 基线例外：research/baseline-findings.md 的 F1（旧 Codex status 测试夹具）与 F2（未改动 tauri.conf.json 的全仓格式失败）；不表述为无条件全仓绿色。Linux/macOS 编译尚需对应 CI 环境补验。
- 产品实施验收完成；用户已回复“可以”确认 Phase 3.4 本地提交与任务归档，详见 commit-plan.md。
