# 本地提交计划（用户已确认）

## 批次 1

`feat(upstream): integrate compatible routing and provider features`

这是一批依赖同一 generated bindings、Provider 与 gateway 契约的选择性整合：普通模型路由、只读发现与探测 UI、OAuth 代理、Claude 备份、cost 聚合、audit fallback、安全 Responses input 规范化，以及相应测试/spec。保持一个可独立构建的工作提交；不执行远程 push。

包含文件：

- .trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/ci-change-scope-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/configured-model-routing-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/index.md
- .trellis/spec/aio-coding-hub/cross-layer/provider-discovery-and-probe-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/provider-oauth-device-flow-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md
- .trellis/spec/aio-coding-hub/cross-layer/usage-insights-contract.md
- .trellis/tasks/09-25-upstream-feature-integration/check.jsonl
- .trellis/tasks/09-25-upstream-feature-integration/commit-plan.md
- .trellis/tasks/09-25-upstream-feature-integration/design.md
- .trellis/tasks/09-25-upstream-feature-integration/implement.jsonl
- .trellis/tasks/09-25-upstream-feature-integration/implement.md
- .trellis/tasks/09-25-upstream-feature-integration/prd.md
- .trellis/tasks/09-25-upstream-feature-integration/research/baseline-findings.md
- .trellis/tasks/09-25-upstream-feature-integration/research/integration-audit.md
- .trellis/tasks/09-25-upstream-feature-integration/research/model-routing.md
- .trellis/tasks/09-25-upstream-feature-integration/research/remaining-features.md
- .trellis/tasks/09-25-upstream-feature-integration/research/upstream-commit-inventory.md
- .trellis/tasks/09-25-upstream-feature-integration/task.json
- scripts/check-pnpm-audit.mjs
- scripts/check-pnpm-audit.selftest.mjs
- src-tauri/src/app/gateway_service/sessions.rs
- src-tauri/src/app/mod.rs
- src-tauri/src/app/provider_model_discovery.rs
- src-tauri/src/app/settings_service.rs
- src-tauri/src/commands/provider_availability.rs
- src-tauri/src/commands/providers/mod.rs
- src-tauri/src/commands/providers/model_discovery.rs
- src-tauri/src/commands/providers/oauth.rs
- src-tauri/src/commands/providers/oauth_limits.rs
- src-tauri/src/commands/providers/oauth_reset.rs
- src-tauri/src/commands/registry.rs
- src-tauri/src/domain/cost.rs
- src-tauri/src/domain/cost/tests.rs
- src-tauri/src/domain/provider_availability.rs
- src-tauri/src/domain/provider_limit_usage.rs
- src-tauri/src/domain/providers/mod.rs
- src-tauri/src/domain/providers/validation.rs
- src-tauri/src/domain/usage_stats/folders.rs
- src-tauri/src/domain/usage_stats/leaderboard_range.rs
- src-tauri/src/domain/usage_stats/leaderboard_v2.rs
- src-tauri/src/domain/usage_stats/tests.rs
- src-tauri/src/gateway.rs
- src-tauri/src/gateway/background_tasks.rs
- src-tauri/src/gateway/configured_model_route.rs
- src-tauri/src/gateway/http_client.rs
- src-tauri/src/gateway/oauth/adapters/codex.rs
- src-tauri/src/gateway/oauth/mod.rs
- src-tauri/src/gateway/oauth/provider_trait.rs
- src-tauri/src/gateway/oauth/refresh_loop.rs
- src-tauri/src/gateway/proxy/failover.rs
- src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs
- src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_limits.rs
- src-tauri/src/gateway/proxy/handler/failover_loop/response/thinking_signature_rectifier_400.rs
- src-tauri/src/gateway/proxy/handler/middleware/mod.rs
- src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs
- src-tauri/src/gateway/proxy/handler/middleware/response_input_rectifier.rs
- src-tauri/src/gateway/proxy/handler/mod.rs
- src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs
- src-tauri/src/gateway/proxy/mod.rs
- src-tauri/src/gateway/reactive_rectifier.rs
- src-tauri/src/gateway/response_input_rectifier.rs
- src-tauri/src/gateway/routes.rs
- src-tauri/src/infra/cli_manager.rs
- src-tauri/src/infra/cli_proxy/claude.rs
- src-tauri/src/infra/cli_proxy/mod.rs
- src-tauri/src/infra/cli_proxy/tests.rs
- src-tauri/src/infra/request_logs.rs
- src-tauri/src/infra/request_logs/types.rs
- src-tauri/src/infra/settings/migration.rs
- src-tauri/src/infra/wsl/detection.rs
- src-tauri/src/infra/wsl/manifest.rs
- src-tauri/src/infra/wsl/mod.rs
- src-tauri/src/infra/wsl/provider_model_discovery.rs
- src-tauri/src/shared/mod.rs
- src-tauri/src/shared/process.rs
- src/components/cli-manager/tabs/GeneralTab.tsx
- src/components/gateway/ModelRoutingPolicyFields.tsx
- src/generated/bindings.ts
- src/pages/providers/ProviderTestDialog.tsx
- src/pages/providers/ProvidersView.tsx
- src/pages/providers/SortableProviderCard.tsx
- src/pages/providers/__tests__/ProviderTestDialog.test.tsx
- src/pages/providers/__tests__/ProvidersView.test.tsx
- src/pages/providers/hooks/useProvidersViewDataModel.ts
- src/pages/providers/providerProbeDefaults.ts
- src/query/__tests__/providers.test.tsx
- src/query/providers.ts
- src/services/gateway/__tests__/modelRoutingPolicy.test.ts
- src/services/gateway/modelRoutingPolicy.ts
- src/services/providers/__tests__/modelDiscovery.service.test.ts
- src/services/providers/__tests__/providers.service.test.ts
- src/services/providers/modelDiscovery.ts
- src/services/providers/providers.ts

## 排除的已有工作

- .trellis/config.yaml
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/check.jsonl
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/design.md
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/implement.jsonl
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/implement.md
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/prd.md
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/research/findings.md
- .trellis/tasks/09-05-gpt-6-astra-stream-intermediate-replies/task.json

忽略的 .trellis/.runtime 测试日志、隔离 baseline 快照与所有编译产物不进入提交。

## 之后的 bookkeeping

工作提交确认并执行后，按 workflow Phase 3.4 → finish-work 顺序归档本任务并记录会话；归档/journal 提交不与产品文件交错。基线已有失败见 research/baseline-findings.md，最终检查结果见 research/integration-audit.md。

## 提交前检查结论

本批次共 95 个文件，实施与本机验证已完成：前端 2931 项、Rust 主回归 3077 项（排除已复现的基线 F1）及 settings 专项 42 项；类型/Lint/改动格式/Rust fmt/Windows Clippy/绑定复验通过。F2 仅为未改动 tauri.conf.json 的既有全仓格式问题，详见 research/baseline-findings.md。Linux/macOS 编译留待对应平台 CI。

按 workflow Phase 3.4，一次确认后执行此本地工作提交，再按 finish-work 归档当前任务并记录 journal；没有远程发布或推送步骤。

## 用户确认

用户回复“可以”，批准本清单的本地工作提交、任务归档及会话记录；由主会话串行执行。
