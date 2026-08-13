# 实施计划：CX2CC 路由与 GPT-5.6

## 执行顺序

1. **启动任务与基线**
   - 校验 `prd.md`、`design.md`、两个 JSONL manifest 和 upstream 审计报告。
   - `task.py start` 后记录当前 commit、origin/upstream ref 和 dirty 文件；只提交
     本任务文件，保留用户已有的 workspace、`.orca` 和临时产物。
   - 在 Orca Run 中创建四个 Codex worker，使用独立 top-level worktree，基于当前
     beta 分支的规划 commit；worker 不 push、不合并，只提交自己的实现 commit。

2. **Wave A：四条独立实现线**
   - Route/thinking：CX2CC 跳过 configured mapping/rewrite；实现 inbound effort/
     thinking presence 到 Responses 的透传；移除固定 effort 覆盖；补 Rust 回归。
   - Loopback：实现精确、一次性的内部 Codex reentry capability；保留普通 self-loop
     和递归拒绝；补 target/attempt/middleware 测试。
   - UI/model：统一 CX2CC Responses 模型常量，加入 GPT-5.6 四型号；修复供应商
     选择；移除/标注固定 thinking 控件；补 TS 单测和旧值兼容。
   - Context：按 provider/model 精确读取 provider_models context，处理 exact/mixed/
     unknown；接入 Claude terminal launch projection；补 Rust resolver/JSON 测试。
   - 每个 worker 只修改任务提示中列出的 owner 文件；遇到跨 owner 需求写报告并
     停在可 cherry-pick 的边界，不直接改他人文件。

3. **Wave B：主会话集成**
   - 逐一读取 worker_done、释放已完成终端，记录 commit SHA 和测试结果。
   - 按依赖顺序 cherry-pick；优先 route/thinking，再 loopback/context，最后 UI。
   - 解决冲突时保持 `design.md` 的 fork 决策；不得顺手引入 upstream 无关修复。
   - 若 context/loopback 需要共享类型，主会话新增最小 adapter，并补跨层测试。
   - 必要时运行生成绑定命令，确保 Rust/TS schema 同步。

4. **质量门**
   - 先运行 `git diff --check`、定向 Rust tests、frontend tests/typecheck/lint。
   - 再运行受影响 crate 的 `cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings`
     以及项目要求的 `pnpm tauri:fmt`、`pnpm check:generated-bindings`。
   - 按 trellis-check 复核 storage -> service -> bridge -> transport -> UI 数据流，
     尤其是 model identity、effort presence、context status、internal capability
     的生命周期和失败隔离。
   - 检查 upstream 分类、未移植理由、无 Claude worker/无 upstream push URL、dirty
     用户文件未被纳入 commit。

5. **提交与 Beta 发布**
   - 主分支完成质量门后提交实现和必要 spec 更新；使用当前 shell 可解析的 node/pnpm
     PATH 运行 hook。
   - 推送 `origin` 的 beta 分支，使用显式 `-R FingerCaster/aio-coding-hub` 创建或
     更新 PR；按 release contract 合并到目标分支，不触碰 upstream。
   - 读取 manifest 和历史 `Release-As:`，计算下一个 beta 版本；先让 release PR/
     workflow 固定到不可变 40-hex SHA，再发布 draft/正式 Beta。验证 tag、Release
     target、六个版本文件、四平台 updater manifest、asset digest，确认 stable
     latest 和 Homebrew 未被改写。
   - 发布后执行 beta channel smoke test（查询、下载、安装/回滚 pointer）并记录
     结果；失败时停止发布 promotion，保留可回滚的前一 beta。

6. **收尾**
   - 若发现重复修复循环，运行 trellis-break-loop 并把防回归规则写入 spec。
   - 更新必要的 cross-layer spec，归档任务并写 session journal；归档前确认只剩
     用户原有 dirty 文件。

## 关键命令

```text
python ./.trellis/scripts/task.py validate 08-13-cx2cc-routing-gpt56
python ./.trellis/scripts/task.py start 08-13-cx2cc-routing-gpt56
git diff --check
pnpm typecheck
pnpm lint
pnpm check:generated-bindings
```

## 集成回滚点

- 每个 worker commit 独立可逆；主会话按 cherry-pick 顺序保留日志。
- 若单条实现线无法满足安全不变量，暂不 cherry-pick 该 commit，保留其测试/报告，
  由主会话补 adapter 或回到设计阶段，不放宽 loopback/unknown fail-closed 规则。
- 发布前任何 Beta contract 失败都停止 promotion，不修改 stable channel。

