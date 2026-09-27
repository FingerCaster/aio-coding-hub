# 归档复核（2026-09-27）

PRD AC1–AC7 已记录完成；关闭自动回切、路由变更及全 Open 恢复均有当前回归通过记录。

## 完成交付

- 功能提交：81332107b4b447a35487d84d1dd9d34495693993；已核验为 origin/main 的祖先提交。
- 本次核对的 origin/main / 0.60.44 发布提交：96272aff6fc4e4e3caa7c20b9754224ba2f40a26。

## 既有验证证据

- 2026-09-27 发布前 pre-push 全部门禁通过：前端 3084 项、Rust 主测试 3218 项及集成测试；Clippy、生成绑定检查通过。
- 已核对本地既有 release-0.60.44/feature-push.log，相关摘录如下；本次仅归档，未重新执行业务测试。

- test app::settings_service::tests::partial_patch_persists_disabled_provider_failback_strategy ... ok
- test gateway::proxy::handler::provider_selection::probe_planner::tests::disabled_session_preserves_route_change_for_complete_prefix ... ok
- test gateway::proxy::handler::provider_selection::probe_planner::tests::disabled_stable_session_ignores_all_automatic_failback_sources ... ok
- test gateway::proxy::handler::provider_selection::probe_planner::tests::disabled_unbound_session_preserves_complete_all_open_recovery ... ok
- test infra::settings::persistence::tests::parse_settings_json_round_trips_disabled_provider_failback_strategy ... ok

- 完整发布完成记录见相邻归档任务 09-27-omp-cli-settings/research/release-0.60.44.md。
- 历史方案与清单保留，不批量把历史计划复选框伪装为逐项重新验收；当前完成判断以提交、现有测试和正式发布证据为准。
