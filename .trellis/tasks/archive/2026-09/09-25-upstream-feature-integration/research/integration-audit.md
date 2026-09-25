# 选择性整合审计

## 基线与执行方式

- fork 起点：270b808c678af99c5d1ee4535071acae0adbb465。
- 固定 upstream 输入：420e9958091ae460d152a508b1eb0e2110ab733b。
- 共同基点：4f02ba3d；此前三个低风险补丁由 071d9e79 纳入。
- origin 为 FingerCaster/aio-coding-hub；upstream push URL 为 DISABLED。
- 用户于 2026-09-25 批准开始，随后要求主会话执行。已中断先前工作代理；余下实现、检查均由主会话完成，重型检查串行执行。
- .trellis/config.yaml 的 inline 调度变化及 09-05 Astra 任务七个未跟踪规划文件不纳入本任务产品提交。

## 功能归类与落地

| 来源 | 分类 | 最终边界与依据 |
| --- | --- | --- |
| 9e2d84c8 / bcb63382 / 48563377 / 537dd7a8 | 适配 | exact、specific、single-wildcard、无递归 target、explicit-first Provider 集合投影；R1.1 保留 fork global/继承/专属/禁用和 reasoning-only，不导入上游持久化模型。 |
| 模型路由兼容 | 保留 | forced Provider、aio/managed alias、CX2CC 和可信本地 reentry 绕过候选缩窄；final-wire 原子应用、失败零发送、日志和费用身份仍由 fork 负责。旧字面星号规则防御读保留，严格新写验证不成为删除迁移；未提交路由字段的局部设置更新保留旧规则。 |
| 动态模型发现 / Codex native+WSL version | 适配 | 新只读 provider_models_discover，bounded/no-redirect/parser 和 OAuth descriptor；CLI version 与 discovery query/header/UA 一致。保留 Windows 启动包装与受管 WSL 证据；不刷新 token、不接管持久化 catalog/Profile。 |
| 3bd9bb59 | 适配 | AppHandle 贯穿 OAuth login/poll/refresh/quota/reset/background；专用环境覆盖优先，空值回落配置，SOCKS5、系统自环保护、设置变更和脱敏保持 fork 约束。 |
| def1060c | 适配 | Claude direct backup 刷新与端口无关 managed 判断；使用 fork capture/write/CAS committed snapshots，将备份和 live 写入组合到逆序补偿。Codex 生命周期不改。 |
| 3758d8e8 | 适配 | 有界 bulk 重试和 OSV fallback；保留 PNPM_AUDIT_REGISTRY、Windows cmd.exe/pnpm.cmd 和 fail-closed。 |
| ab83c23f | 适配 | Session、Provider limits、folder/day/leaderboard SQL TOTAL 与 f64 消费链；原始单条 cost 存储和 DTO number、raw/client ownership 保持。 |
| 85253db0 | 后端适配，前端覆盖 | 缺 output rate 且存在 output usage 返回未知；保留已有 priority base fallback。上游全量日志刷新不导入，fork 增量 feed 和 trace 对账保留。 |
| 9234280f / 37319565 | 部分覆盖、部分适配 | 调用顺序和持久化已由 fork 覆盖，仅增加真实卡片锚点、清筛选和单次定位。 |
| b3343335 / b34fe58a / 867a0db3 | 适配 | 模型/提示词测试对话框、可选 IPC 参数、协议请求体和有界验证；复用保存模型/目录候选，并提供只读发现。不引入上游 model_policy。 |
| e2d03792 | 选择性适配 | 仅 Responses input 规范化、现有 Claude signature/budget 整流器注册。R1.3 保留窄触发、开关、计数与重试预算；不引入 generic-400 扩张和其他新整流算法/配置。 |
| e2d03792 priority billing selector | 保留 fork | R1.2 保留 Actual 优先；可选 selector 本次未增加，不产生 settings schema 变更。 |
| 6007d7a0 | fork 已覆盖 | 保留现有 reasoning capabilities、effort 观测和 strict price aliases。 |
| a09cbb05 / d0f85364 / 8bf194e0 | fork 已覆盖 | 保留更完整的 catalog/profile ownership、事务恢复和事件/竞态路径。只读候选不是持久化目录替代品。 |
| 上游迁移/版本/依赖/回滚功能/README metadata | 排除或覆盖 | 遵循 R5：不改变 fork release/dependency/schema、不恢复上游已回滚功能，不引入非必要品牌元数据。 |

## 实施中修正的规划事实

- research/remaining-features.md 将内部已有 model override 误写成公开 probe IPC 已支持 model/prompt。实际公开命令只有 provider_id。本次增加可选 model/prompt，更新 domain/service/query/dialog 并重新生成绑定；旧调用缺参使用原默认值。
- 原设计中 schema 57/SQLite 45 是相关路由能力最初版本，不是本次要回退的版本。本次没有修改任何 schema/release/dependency 版本。
- 严格的新 wildcard 写校验与旧数据防御读分离；不能因旧规则含多个字面星号就在启动时删除用户配置。

## 检查记录

- 首批路由 focused Rust：15 通过；只读发现/WSL focused Rust：33 通过。
- 最新 frontend typecheck / lint：通过。
- 首轮 frontend focused：6 文件、129 测试通过（dialog/location/service/query/routing）。最终全量单 worker：313 文件、2931 测试全部通过，含新增异常脱敏和 probe payload UI 回归。
- Audit self-test、spec-links、no-instant-now-sub、gateway error codes：通过。
- 实际依赖审计：434 个包；bulk endpoint 两次失败后按设计回落 OSV；info 0 / low 1 / moderate 1 / high 0 / critical 0，gate 通过。
- CI change-scope/classifier/workflow contract 与 support matrix：通过；implement.jsonl/check.jsonl 各 5 项验证通过。
- Rust 首次完整串行：3073 通过、4 失败、4 ignored。两项移植测试夹具缺 fork provider_uuid、一项 Grok 旧请求形状断言已按规范化契约修正。剩余 Codex status 用例已在固定 fork 基线隔离复现，见 baseline-findings.md；不在本次修复。
- Rust 完整串行复验：3077 通过、0 失败、4 ignored、1 filtered。仅排除已经在固定 fork 基线隔离复现的 Codex status 旧失败；不声称无条件全量绿色。
- 最后追加 settings 局部 patch 兼容保护后，settings_service 全部 42 测试通过（含新增旧路由保存回归）；未再次运行未受影响的全部 Rust 用例。
- 最终 typecheck、ESLint、本次涉及的 20 个前端/脚本文件格式及 Rust fmt 全部通过。全仓 format:check 只有未改动的 tauri.conf.json 基线问题（F2），未改写该配置。
- Windows host cargo clippy --all-targets --locked -- -D warnings 通过（包括编译检查）；单构建 job、禁用 incremental，未启动并行代理。
- 本机 WSL 仅有 docker-desktop，无可用 Linux 开发发行版。本次未声称验证 Linux/macOS 编译；若后续推送，仍需 CI 执行对应平台 gate。
- check-generated-bindings 通过：从 Rust 重新生成并格式化后与已保存 TypeScript 绑定一致。
- git diff --check 通过；本任务 95 个文件全部在提交清单中，8 个其他改动/规划文件排除；版本、依赖锁定、数据库迁移未改动。

## 提交边界

只纳入本任务实现、生成绑定、回归测试、相关 specs 与此任务规划/证据。
不纳入 .trellis/config.yaml、09-05 Astra 任务、编译/测试日志与隔离基线快照。
不执行 upstream merge/cherry-pick/push，也不改变任何远程。

## 当前结论

本任务实现、范围内修复与本机验证完成。F1/F2 是独立复现或确认未改动的既有 fork 基线问题，未在整合中修复；相关全仓 gate 的例外已如实列出。用户已回复“可以”确认 workflow Phase 3.4 的 commit-plan.md；工作提交 88883c9053026900ad259c7401d80a0cec8561ea 已完成，95 文件范围与清单完全一致，前端/Rust 提交钩子通过。按 trellis-finish-work 归档当前任务并记录 journal；8 个无关文件内容保持不变。没有远程推送步骤。

提交后的研究记录空白行格式清理随任务归档保存，不改变产品代码。
