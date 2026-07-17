# Implementation Plan: 第二轮终审修复

## Entry And Before-Dev

- [x] 完成 PRD convergence，确认 F1-F8 均有事实、契约、AC 与验证命令。
- [x] `task.py start` 后完整重读 prd/design/implement。
- [x] 读取 packages、backend/cross-layer index、Pre-Development Checklist 指向的具体 spec 与
      shared guides；读取本轮 research。

## Strict Serial Fix Order

1. [x] F1：先加链接/预置/回滚测试，再实现独占目录与 create_new。
2. [x] F2：先加 marker/case/order/budget 测试，再实现统一路径图。
3. [x] F3：先加 IPv6 纯函数/DNS 回归，再拒绝 `::/96`。
4. [x] F4：先更新错误行为测试，再实现有界过期、三态 poll、binding 与前端退避。
5. [x] F5：先加 v52 settings migration/切换/路径测试，再实现 settings-owned root allowlist。
6. [x] F6：先加非法 MIME + 大 payload 测试，再前移完整 MIME 解析。
7. [x] F7：执行脱敏 live 验证、生成冲突决策审计、修正父/归档任务材料与 task map。
8. [x] F8：先加同毫秒分页/非法 cursor/前端追加测试，再升级 DTO/SQL/binding/session。

每项完成后立即运行该项聚焦测试；不得并行修复或跳过失败项。

## Specs And Generated Contracts

- [x] 更新 Image Gen trust boundary spec：原子落盘、per-row root、MIME-first、opaque cursor。
- [x] 更新 Skill bundle spec：marker + platform case conflict graph。
- [x] 更新 Device OAuth spec：expires bounds、slow_down DTO/退避序列。
- [x] 生成 bindings 并检查无漂移。

## Focused Validation

- [x] Image Gen Rust（history/transport/migration）和 controller/service Vitest。
- [x] config migrate Rust 全组。
- [x] provider OAuth Rust 与 frontend actions tests。
- [x] generated bindings 生成与一致性检查。
- [x] task artifacts validate + secret scan。

## Full Gates

- [x] `pnpm build`
- [x] `pnpm check:precommit:full`
- [x] `pnpm check:prepush`
- [x] locked Cargo lib + 全 integration suites
- [x] locked all-target Clippy `-D warnings`
- [x] `git diff --check` 与 Trellis full-scope check

## Finish

- [x] 完成 trellis-update-spec 判断和 7-section executable contract。
- [ ] 动态补齐 node/pnpm PATH，提交本轮全部代码/测试/spec/task 工件。
- [ ] 读取并执行 `trellis-finish-work`，归档本子任务并记录 journal；父任务不归档。
- [ ] 确认工作树干净、父任务 `in_progress`、upstream push `DISABLED`、Orca 单终端。

## Validation Evidence

- 聚焦 Rust：Image Gen 48/48、config migrate 33/33、OAuth 86/86；复合 cursor 与多 root
  迁移专项 4/4。OAuth `ACTIVE_FLOW` 测试统一持有 gateway-owned test-only async mutex，未使用
  重试循环。
- 聚焦前端：Image Gen controller/service 与 OAuth actions/dialog/provider service 共 194/194；
  MSW/Rust defaults cross-layer 契约 16/16。
- `pnpm build` 通过（3543 modules）；`pnpm check:precommit:full` 13/13；
  `pnpm check:prepush` 15/15，含 coverage shards、generated bindings、完整 locked Cargo
  （2067 passed、3 ignored 及全部 integration suites）和 all-target Clippy `-D warnings`。
- `muyuan` post-fix 只读验证为 3 个 GET 全部 2xx；类型、USD unit、finite/formula 断言均通过，
  证据未保存 host、query、token、PII 或实际金额。
- merge `9e5da346` 只读审计可复现 30 个冲突文件、47 个 marker groups；逐文件决策表明确
  fork/upstream/融合结果，并说明旧“31 个文本冲突”口径不可一一映射。
