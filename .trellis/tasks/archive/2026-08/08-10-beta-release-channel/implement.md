# 完整 Beta 发布频道实施计划

## 0. 启动前门禁

- [x] 用户审阅并批准 prd.md、design.md 以及三个子任务的依赖和验收。
- [x] implement.jsonl 与 check.jsonl 已移除模板占位行并包含真实 spec/research 条目。
- [x] 重新读取当前工作树；保留用户既有修改，不从 upstream 读取或操作。
- [x] 运行 python ./.trellis/scripts/task.py list、python ./.trellis/scripts/task.py current --source、python ./.trellis/scripts/task.py validate <task-dir> 和 python ./.trellis/scripts/get_context.py --mode phase，确认任务仍为 planning 后再 task.py start。

## 1. 发布合同子任务

- [x] 启动 08-10-beta-release-pipeline，先实现/自测 Beta tag、channel-aware candidate schema、版本 overlay attestation、release-channels CAS 指针和暂停 workflow。
- [x] 保持稳定 release.yml 的默认输入/输出、release-please 两阶段、exact asset/no-overwrite、tag-to-SHA 和 Homebrew 分支；为每个新增分支补稳定回归断言。
- [x] 通过 pipeline 子任务门禁后，记录固定 endpoint、manifest 字段、Release URL、state schema 和脚本 API，作为 updater-core 的输入合同。

## 2. 后端更新合同子任务

- [x] 启动 08-10-beta-updater-core，依赖第 1 步输出；新增 UpdateChannel、专用 settings writer、生成绑定和 config import/export 归一化。
- [x] 实现受控 endpoint 选择、带频道的 updater metadata/resource、安装前 Beta fresh check、stale rejection 和 discard command；稳定路径保持默认 builder。
- [x] 运行 Rust 单测、config migration 矩阵、generated bindings 检查，并记录前端 service 可消费的精确字段。

## 3. 前端参与和更新 UX 子任务

- [x] 启动 08-10-beta-update-ui，依赖第 2 步 generated binding/service 合同；改造 query keys、全局检查 epoch、后台任务和安装缓存。
- [x] 实现 About 卡风险确认、逐设备 opt-in/out、Beta 文案/徽标、portable 精确 Release 链接和跨频道迟到结果隔离。
- [x] 运行 Settings/UpdateDialog/Sidebar/query/background focused tests，覆盖确认取消、保存失败、切换中检查、Beta→stable 正式版和 stale candidate。

## 4. 父任务集成

- [x] 合并三个子任务的跨层变更前，审阅所有 generated binding、settings ownership 和 release endpoint 引用，确保没有组件私自解析 manifest 或使用任意 URL。
- [x] 补跨层真实 fixture：稳定用户、Beta 用户、Beta 暂停、指针竞争、Beta→正式版、退出 Beta 且稳定版本较低/较高、导入带 beta=true、检查中切换。
- [x] 运行 pnpm check:generated-bindings、pnpm typecheck、pnpm lint、相关 pnpm test:unit、pnpm tauri:fmt、pnpm tauri:check、pnpm tauri:test、pnpm tauri:clippy（环境允许时）和全部 release/support-matrix selftests。
- [x] 运行 git diff --check、稳定发布合同检查、依赖审计；检查变更没有触及 upstream 或实际发布远端状态。

## 5. 风险停止点

- [x] 版本 overlay 任一平台的四文件摘要不一致：停止，不上传候选。
- [x] Beta target SHA 不是 origin/main ancestor、tag peel 不一致、Release 状态/资产不符：停止，不晋升/不公开。
- [x] channel ref CAS 竞争、manifest digest 不匹配或暂停目标未完整公开：停止，不强制更新 ref。
- [x] backend 不能证明当前频道或 Beta fresh check：关闭 resource，不安装，不 fallback 稳定。
- [x] 稳定回归、生成绑定或现有发布门禁失败：暂停父任务，不进入真实 Beta 发布。

## 6. 完成条件

- [x] 用户复核最终 PRD/design/implement 和子任务结果后明确要求实现。
- [x] trellis-check 质量门禁通过，所有验收项有测试证据。
- [x] 本任务只交付代码、合同、自测和文档；首个 Beta 的实际发布另行授权并使用 origin 的手动 workflow。
