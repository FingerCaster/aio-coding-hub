# 归档复核（2026-09-27）

已核对流终态处理、开关和当前协议回归；本任务的既有功能交付不等同于仍待真机验证的 08-23 流错误规则追加任务。

## 完成交付

- 功能提交：48ddd915016931b9746d243558de2fdbb19faf09；已核验为 origin/main 的祖先提交。
- 本次核对的 origin/main / 0.60.44 发布提交：96272aff6fc4e4e3caa7c20b9754224ba2f40a26。

## 既有验证证据

- 2026-09-27 发布前 pre-push 全部门禁通过：前端 3084 项、Rust 主测试 3218 项及集成测试；Clippy、生成绑定检查通过。
- 已核对本地既有 release-0.60.44/feature-push.log，相关摘录如下；本次仅归档，未重新执行业务测试。

- test gateway::routes::tests::codex_capacity_code_is_raw_passthrough_when_terminal_firewall_is_disabled ... ok
- test gateway::streams::terminal_firewall::tests::buffers_split_frames_and_preserves_exact_bytes ... ok
- test gateway::streams::terminal_firewall::tests::complete_validator_accepts_clean_eof_or_done_after_one_completed ... ok
- test gateway::streams::terminal_firewall::tests::complete_validator_accepts_real_codex_responses_shape_without_done ... ok
- test gateway::streams::terminal_firewall::tests::complete_validator_rejects_mismatched_response_ids ... ok

- 完整发布完成记录见相邻归档任务 09-27-omp-cli-settings/research/release-0.60.44.md。
- 历史方案与清单保留，不批量把历史计划复选框伪装为逐项重新验收；当前完成判断以提交、现有测试和正式发布证据为准。
