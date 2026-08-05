# Validation Plan

- [x] 运行分类器和三条文档合同、support/upstream 策略检查。
- [x] 解析并静态审计 workflow；运行 actionlint 1.7.12。
- [x] `pnpm install --frozen-lockfile` 后运行当前 frontend job 的全部命令。
- [x] 运行当前 rust job 的 fmt、lock、clippy、test、audit 等价命令。
- [x] 复核 origin protection/rulesets、完整 diff、spec 和任务验收项。
- [x] 修复问题、复测、记录验证证据并准备提交/归档。
