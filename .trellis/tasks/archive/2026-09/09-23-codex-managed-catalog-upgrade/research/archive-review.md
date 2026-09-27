# 归档复核（2026-09-27）

实施清单全部完成；后续提交 00cbb43f 更新生成绑定、929bc93c 修正大枚举变体。当前发布基线包含三项提交，并通过目录、前端与绑定检查。

## 完成交付

- 功能提交：d95b489aad47ccd62387538b3499f42de05d0b23；已核验为 origin/main 的祖先提交。
- 本次核对的 origin/main / 0.60.44 发布提交：96272aff6fc4e4e3caa7c20b9754224ba2f40a26。

## 既有验证证据

- 2026-09-27 发布前 pre-push 全部门禁通过：前端 3084 项、Rust 主测试 3218 项及集成测试；Clippy、生成绑定检查通过。
- 已核对本地既有 release-0.60.44/feature-push.log，相关摘录如下；本次仅归档，未重新执行业务测试。

- ✓ src/components/cli-manager/tabs/__tests__/CodexModelContextRulesSection.test.tsx (10 tests) 1697ms
- test infra::codex_model_catalog::managed::tests::base_catalog_alias_conflicts_fail_closed ... ok
- test infra::codex_model_catalog::managed::tests::bundled_base_catalog_descriptor_tracks_launch_and_executable_changes ... ok
- test infra::codex_model_catalog::managed::tests::catalog_deactivation_failure_restores_every_committed_prior_stage ... ok
- test infra::codex_model_catalog::managed::tests::catalog_deactivation_rollback_restores_config_even_if_generated_drifted ... ok

- 完整发布完成记录见相邻归档任务 09-27-omp-cli-settings/research/release-0.60.44.md。
- 历史方案与清单保留，不批量把历史计划复选框伪装为逐项重新验收；当前完成判断以提交、现有测试和正式发布证据为准。
