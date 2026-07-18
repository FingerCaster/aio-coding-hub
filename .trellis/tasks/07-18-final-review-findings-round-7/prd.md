# 关闭第七轮最终审核 Findings

## Goal

关闭冻结提交 `35db0f3287ec957e3479fc47b05f8ae1fd882eeb` 上仍有效的
Image Gen 持久化、Skill 导出聚合资源、settings/autostart ABA 回滚及早期子任务
验收事实投影问题。保持现有安全边界和用户可见功能，完成门禁后交由唯一的
Codex `gpt-5.6-sol / effort=max` 只读终审。

## Background

- 父任务 `07-17-sequential-task-acceptance` 的子任务 1-11 已按顺序完成；本任务是
  第 12 个子任务，必须在其归档前独立完成实现、检查、提交与归档。
- 本轮有效审核依据是 Codex `gpt-5.6-sol / effort=max` 的冻结提交报告与协调会话的
  本机代码核验。自 2026-07-18 起，后续审核只使用新的 Sol max 会话；不得启动
  Claude、Pi 或其他 agent 审核。
- 历史 Claude 报告只保留为审计背景，不作为本任务的验收依据，也不会触发新的
  Claude 审核。
- F24 Trellis template-hash / safe-commit 观察项仍不属于本任务；不得修改
  `.trellis/.template-hashes.json`、`.trellis/.version`、`.pi/` 或
  `.trellis/scripts/tests/test_template_hashes.py` 的既有用户变更。

## Confirmed Findings

### F1. Image Gen 重试成功后不能持久化（P1）

- 前端 `retry` 复用原任务 ID，成功后再次调用 `image_gen_task_persist`；后端的
  `history.rs` 明确拒绝已存在的 ID/目录并只执行 INSERT。
- 已持久化的失败任务重试成功后，当前会话可见新图片，但重启后仍是旧错误行；已持久化
  的成功任务被重试时也会保留旧图片。异步首次落盘与重试重叠时同样可能冲突。
- 后端的 insert-only 任务目录和 handle-relative 创建是安全契约，不能为了修复重试
  改为覆盖已有目录或放宽 duplicate-ID 拒绝。

### F2. 配置导出没有跨 Skill 聚合预算（P2）

- 每个 Skill 只受 8 MiB 原始字节与 256 文件限制；已安装和本地 Skill 的 exporter 会先
  将所有 Base64 文件 push 到 `ConfigBundle`，64 MiB 上限只在随后序列化时生效。
- 六个接近 8 MiB 的合法 Skill 已可在 writer 报错前保留约 64 MiB Base64；更多 Skill
  没有总量/数量上限，可能造成高内存或进程级失败。
- 本任务只添加资源预算，不按内容、文件名或敏感词过滤 Skill 内容；根内任意字节仍须
  按既有规则完整 round trip。

### F3. config import whole rollback 的同值 ABA（P1）

- `rollback_whole_settings_with_auto_start_token` 在 token generation 已失效时，仍以字段值
  是否等于 import snapshot 决定恢复。若后来的 ordinary settings writer 将
  `auto_start` 写为与 import 相同的值，`commit_auto_start_with_owner` 仍推进 generation，
  rollback 却会把该新写入回退为 import 之前的值。
- 现有并发回归只覆盖后写入者将 `auto_start` 改为不同值；需要加入相同 committed 值的
  ABA 变体，并同时保留 import 仍拥有的其他字段回滚能力。

### F4. 子任务 1-5 的归档验收事实未闭合（P2）

- 五个 archive `task.json` 均为 `completed`，但其 `prd.md` Acceptance Criteria 和
  `implement.md` 仍保留未勾选项，无法由子任务自身证明已验收。
- 这是任务工件一致性问题，不改变产品运行时行为，也不改变父任务保持 `in_progress` 的
  状态。

## Requirements

### R1. 保持 Image Gen 持久化的不可变 ID/目录边界

- 每次重试必须创建新的逻辑任务 attempt 与新的随机 ID；原任务及其已持久化图片/错误行
  保留，不覆盖、不删除，也不复用已有目录。
- 新 attempt 继续使用原任务的请求快照；持久化参考图时仍通过已有安全读取和新 attempt 的
  后端-owned 文件创建路径。
- 新 attempt 成功必须独立持久化并在重启后可见；失败 attempt 的持久化/显示必须不污染
  原已持久化行。
- 纠正前端和后端中暗示同 ID upsert 的注释/契约，使其与 insert-only 实现一致。

### R2. 在 Base64 分配前施加跨 Skill 导出预算

- 增加配置导出生命周期内共享的 Skill aggregate budget，覆盖 installed 与 local Skills；
  单 Skill 的 8 MiB、256 文件、路径、链接和特殊文件约束保持不变。
- 预算采用不超过 56 MiB 的已编码 Skill payload 与不超过 2048 个导出文件的全局上限，
  在为下一个文件分配 Base64 前以 checked arithmetic 拒绝超限；最终 64 MiB bundle writer
  仍保留为全 bundle 的最后防线。
- 聚合超限必须给出明确 `SEC_INVALID_INPUT`，不写目标文件、不静默跳过文件，并且不扩大
  import 或其他配置字段的范围。

### R3. 以 generation 作为 auto_start rollback 的真实所有权

- 若 whole-import rollback 的 autostart token 已失去 generation 所有权，绝不因字段值
  恰好等于 import snapshot 而恢复 `auto_start`；OS 同步必须收敛到当前 canonical winner。
- generation 失效时仍可对其他尚等于 import 值的普通字段执行 field-aware rollback，不能
  因保护 `auto_start` 而把整个 import 残留为失败状态。
- 覆盖完整 snapshot 相等和仅部分字段不同两种 ABA 状态，证明后来的同值 ordinary writer
  始终获胜。

### R4. 对账已归档的子任务 1-5

- 仅在现有实现提交、质量门禁、archive 提交与父任务证据确实支持的前提下，将五个 archive
  的 PRD AC 与 implement checklist 标记为完成。
- 不伪造时间、commit 字段或新的验证结果；不修改产品源码、Trellis CLI 或 F24 范围来关闭
  此项。
- 对账后 `task.py validate --all` 必须通过，且五个已归档任务不再有未勾选的 AC/checklist。

### R5. 执行与审核边界

- 实现、检查、提交和归档由一个新开的 Orca-managed Codex
  `gpt-5.6-sol / effort=max` 执行终端严格串行完成。执行提示只允许按本 PRD、design 和
  implement 的已批准 checklist 推进；不得进行无关探索、额外 finding hunting、重构、架构
  方案扩展或其他发散工作。发现必须扩大范围时，停止并交给协调会话裁决。
- 完成后的唯一最终审核员是新开的 Codex `gpt-5.6-sol / effort=max`；审核只读，不得写入
  tracked 文件、任务状态、分支、remote 或 main。
- 不向任何 remote push；正常 GitHub 操作仍以 `origin` 为准，`upstream` 保持 fetch-only，
  且本任务不做 upstream 操作。

## Acceptance Criteria

- [x] 对已持久化 done/error 任务的成功重试产生新的 task ID，并在后端 insert-only 契约下
      独立持久化；重启后原记录与新成功记录均可正确读取。
- [x] 失败重试、参考图回读、在途首次落盘与重试交叠不会覆盖、删除或误标原持久化记录。
- [x] Image Gen 前后端注释、跨层契约和 focused 前端/Rust 回归一致描述 immutable attempt
      语义。
- [x] 多个 individually legal Skill 在 aggregate budget 触发时于下一次 Base64 分配前得到
      明确失败，目标导出文件保持原样；一项 1-8 MiB 的合法资源仍完整导出/导入。
- [x] installed/local 两条 exporter 共享同一 budget，文件数和字节累计均使用 checked
      arithmetic，且原有 symlink、特殊文件和敏感-looking bytes 的回归继续通过。
- [x] 同值 ordinary `auto_start` writer 推进 generation 后，失败 import rollback 保留该 winner；
      import 独有的其他字段按 field-aware 规则恢复，OS 收敛到 winner。
- [x] 子任务 1-5 的 archive PRD/implement 仅勾选已证实的项目，且全仓任务 manifest/归档
      校验通过。
- [x] 受影响 Rust/前端测试、格式/类型检查、`pnpm check:precommit:full`、`pnpm check:prepush`
      和必要的 Docker/Linux watchdog 验证全部通过并记录。
- [ ] 完成提交与归档后，新的 Sol max 只读审核没有 P0-P2 findings，父任务才可进入最终验收。

## Out Of Scope

- 修改 upstream、本地 reasoning guard、continuation repair、外部受管 retry gateway，或 F24
  Trellis template-hash / safe-commit 机制。
- 以覆盖已有 Image Gen task 目录、删除旧 history row 或放松 handle-relative filesystem checks
  的方式修复重试。
- 基于内容或敏感词过滤 Skill 文件、静默省略合法资源，或改变单 Skill 8 MiB import/export
  一致性。
- 修改 Trellis CLI 的通用 archive/checkbox 校验行为，或把本次工件对账扩展为通用 DAG 工作。
- 启动 Claude、Pi 或任何其他 agent 审核。
- 让 Sol 执行终端自行扩大范围、把实现会话当作审核会话，或在未获协调裁决时修改计划外文件。
