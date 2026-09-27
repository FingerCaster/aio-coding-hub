# 归档复核（2026-09-27）

已核对原始功能提交及当前无限重试测试。后续长流超时修复已在 08-23-fix-codex-infinite-retry-stream-cap 独立归档，当前语义以维护中的契约与实现为准。

## 完成交付

- 功能提交：0512eac42306e38e90bdaea54d1060dfc7238346；已核验为 origin/main 的祖先提交。
- 本次核对的 origin/main / 0.60.44 发布提交：96272aff6fc4e4e3caa7c20b9754224ba2f40a26。

## 既有验证证据

- 2026-09-27 发布前 pre-push 全部门禁通过：前端 3084 项、Rust 主测试 3218 项及集成测试；Clippy、生成绑定检查通过。
- 已核对本地既有 release-0.60.44/feature-push.log，相关摘录如下；本次仅归档，未重新执行业务测试。

- test gateway::infinite_retry::tests::eligibility_is_fail_closed_and_excludes_system_turns ... ok
- test gateway::infinite_retry::tests::gateway_shutdown_drops_in_flight_round_work ... ok
- test gateway::infinite_retry::tests::json_success_requires_completed_and_rejects_embedded_errors ... ok
- test gateway::infinite_retry::tests::gateway_shutdown_interrupts_round_wait ... ok
- test gateway::infinite_retry::tests::starting_next_round_marks_orphaned_pending_usage_as_overflowed ... ok

- 完整发布完成记录见相邻归档任务 09-27-omp-cli-settings/research/release-0.60.44.md。
- 历史方案与清单保留，不批量把历史计划复选框伪装为逐项重新验收；当前完成判断以提交、现有测试和正式发布证据为准。
