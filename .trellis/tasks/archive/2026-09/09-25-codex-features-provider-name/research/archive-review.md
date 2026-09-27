# 归档复核（2026-09-27）

已核对 Provider name、旧 flag 优先级与清理、fast_mode 不改 service_tier 的实现及回归；原 PRD 未勾选条目属于记录未收尾，不代表功能未合入。

## 完成交付

- 功能提交：28d17ddb1056240226694cbaf49acd482d47dd2f；已核验为 origin/main 的祖先提交。
- 本次核对的 origin/main / 0.60.44 发布提交：96272aff6fc4e4e3caa7c20b9754224ba2f40a26。

## 既有验证证据

- 2026-09-27 发布前 pre-push 全部门禁通过：前端 3084 项、Rust 主测试 3218 项及集成测试；Clippy、生成绑定检查通过。
- 已核对本地既有 release-0.60.44/feature-push.log，相关摘录如下；本次仅归档，未重新执行业务测试。

- test infra::codex_config::provider_projection::tests::desired_key_keeps_exact_legacy_flag_ahead_of_model_provider ... ok
- test infra::codex_config::tests::patch_provider_name_creates_model_provider_and_renames_table ... ok
- test infra::codex_config::tests::patch_provider_name_renames_aio_table_to_openai ... ok
- test infra::codex_config::tests::patch_provider_name_reverts_openai_table_to_aio_and_drops_legacy_flag ... ok
- test infra::codex_config::tests::patch_writes_fast_mode_without_service_tier ... ok

- 完整发布完成记录见相邻归档任务 09-27-omp-cli-settings/research/release-0.60.44.md。
- 历史方案与清单保留，不批量把历史计划复选框伪装为逐项重新验收；当前完成判断以提交、现有测试和正式发布证据为准。
