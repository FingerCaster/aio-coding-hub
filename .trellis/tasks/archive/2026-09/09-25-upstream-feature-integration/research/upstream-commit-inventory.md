# Upstream commit evidence

Pinned upstream: 420e9958091ae460d152a508b1eb0e2110ab733b.

Session diff directory: C:\Users\admin\AppData\Local\Temp\aio-upstream-planning-xy61hsur

## 9e2d84c8

commit 9e2d84c87ef14929ef7b20dc559f3421f2f2f761
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Thu Aug 6 18:26:35 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Thu Aug 6 18:26:35 2026 +0800

    feat(providers): add unified model policy routing

 src-tauri/src/app/provider_service.rs              |  43 ++-
 src-tauri/src/domain/provider_limit_usage.rs       |   1 +
 src-tauri/src/domain/provider_oauth_limits.rs      |   1 +
 src-tauri/src/domain/providers/mod.rs              |   5 +
 src-tauri/src/domain/providers/model_policy.rs     | 334 +++++++++++++++++++++
 src-tauri/src/domain/providers/queries.rs          |  87 ++++--
 src-tauri/src/domain/providers/tests.rs            |  98 ++++++
 src-tauri/src/domain/providers/types.rs            |   9 +
 src-tauri/src/gateway/active_requests.rs           |   1 +
 src-tauri/src/gateway/events.rs                    |  71 +++++
 src-tauri/src/gateway/manager.rs                   |   1 +
 src-tauri/src/gateway/proxy/error_code.rs          |  12 +
 src-tauri/src/gateway/proxy/failover/tests.rs      |   2 +
 src-tauri/src/gateway/proxy/handler/early_error.rs |  21 ++
 .../failover_loop/attempt/attempt_executor.rs      |   2 +
 .../handler/failover_loop/attempt/retry_engine.rs  |   1 +
 .../gateway/proxy/handler/failover_loop/context.rs |   3 +-
 .../proxy/handler/failover_loop/event_helpers.rs   |   2 +
 .../src/gateway/proxy/handler/failover_loop/mod.rs |   2 +
 .../failover_loop/prepare/provider_iterator.rs     |  22 +-
 .../failover_loop/prepare/provider_model_policy.rs | 183 +++++++++++
 .../failover_loop/response/response_router.rs      |   2 +
 .../middleware/cx2cc_count_tokens_interceptor.rs   |   2 +
 .../handler/middleware/provider_resolution.rs      |  93 +++++-
 src-tauri/src/gateway/proxy/handler/mod.rs         |   2 +
 .../src/gateway/proxy/handler/provider_order.rs    |   2 +
 .../gateway/proxy/handler/provider_selection.rs    |  49 +++
 .../proxy/handler/provider_selection/tests.rs      | 121 +++++++-
 src-tauri/src/gateway/proxy/request_end.rs         | 132 +++++++-
 src-tauri/src/gateway/routes.rs                    | 328 +++++++++++++++++++-
 src-tauri/src/infra/config_migrate/export.rs       | 134 ++++++---
 src-tauri/src/infra/config_migrate/import.rs       |  61 +++-
 src-tauri/src/infra/config_migrate/mod.rs          |  22 +-
 src-tauri/src/infra/config_migrate/tests.rs        |  78 ++++-
 src-tauri/src/infra/db/migrations/baseline_v25.rs  |   1 +
 src-tauri/src/infra/db/migrations/mod.rs           |   6 +-
 src-tauri/src/infra/db/migrations/tests.rs         |  75 +++++
 src-tauri/src/infra/db/migrations/v37_to_v38.rs    |  48 +++
 src-tauri/src/test_support.rs                      |   1 +
 .../tabs/__tests__/ClaudeOAuthCard.test.tsx        |   2 +
 src/components/home/HomeRequestLogsPanel.tsx       |   8 +-
 src/components/home/RealtimeTraceCards.tsx         |   5 +-
 .../home/__tests__/HomeRequestLogsPanel.test.tsx   |   1 +
 .../HomeTodayProviderUsageOverview.test.tsx        |   1 +
 .../home/__tests__/RequestLogDetailDialog.test.tsx |   1 +
 src/components/home/previewData.ts                 |   3 +
 src/components/home/requestLogSpecialSettings.ts   |  16 +
 src/constants/gatewayErrorCodes.ts                 |  18 ++
 src/generated/bindings.ts                          |  21 ++
 .../hooks/__tests__/useHomeOAuthQuota.test.tsx     |   2 +
 src/pages/providers/ProviderEditorDialog.tsx       |  11 +-
 src/pages/providers/ProviderModelPolicySection.tsx | 306 +++++++++++++++++++
 .../__tests__/ProviderEditorDialog.test.tsx        | 116 ++++---
 .../__tests__/ProviderModelPolicySection.test.tsx  | 121 ++++++++
 .../__tests__/SortableProviderCard.test.tsx        |   2 +
 .../__tests__/providerEditorOAuthActions.test.ts   |   6 +-
 .../__tests__/providerEditorSaveRunner.test.ts     |   6 +-
 .../__tests__/providerEditorSubmitModel.test.ts    |  54 ++++
 src/pages/providers/providerEditorActionContext.ts |   6 +-
 src/pages/providers/providerEditorOAuthActions.ts  |   4 +-
 src/pages/providers/providerEditorSaveRunner.ts    |   2 +-
 src/pages/providers/providerEditorSubmitModel.ts   |  43 +++
 src/pages/providers/providerModelPolicy.ts         |  53 ++++
 src/pages/providers/useProviderEditorEffects.ts    |  24 ++
 src/pages/providers/useProviderEditorForm.ts       |  39 ++-
 src/query/__tests__/providers.test.tsx             |   2 +
 .../__fixtures__/gatewayEvents/attempt.json        |   3 +-
 .../__fixtures__/gatewayEvents/request.json        |   3 +-
 .../__tests__/gatewayEvents.contract.test.ts       |  35 +++
 .../__tests__/requestActivityProjection.test.ts    |   2 +
 .../__tests__/requestLogSpecialSettings.test.ts    |  53 ++++
 src/services/gateway/__tests__/traceStore.test.ts  |   2 +
 src/services/gateway/gatewayEvents.ts              |  17 +-
 src/services/gateway/modelRedirect.ts              |  59 ++++
 src/services/gateway/requestActivityProjection.ts  |  15 +-
 src/services/gateway/requestLogSpecialSettings.ts  |  31 ++
 src/services/gateway/traceStore.ts                 |  17 +-
 .../providers/__tests__/providers.service.test.ts  |   2 +
 src/services/providers/providers.ts                |  22 +-
 src/test/msw/handlers.ts                           |   9 +
 80 files changed, 3042 insertions(+), 159 deletions(-)

## bcb63382

commit bcb633824d2eef70faae8d8aebe24908b66ad042
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Fri Aug 7 11:53:17 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Fri Aug 7 11:53:17 2026 +0800

    feat(providers): add upstream model discovery

 src-tauri/src/app/mod.rs                           |    1 +
 src-tauri/src/app/provider_model_discovery.rs      | 1211 ++++++++++++++++++++
 src-tauri/src/commands/providers/mod.rs            |    2 +
 .../src/commands/providers/model_discovery.rs      |   18 +
 src-tauri/src/commands/registry.rs                 |    9 +
 src-tauri/src/domain/providers/mod.rs              |   12 +-
 src-tauri/src/domain/providers/model_policy.rs     |   17 +
 src-tauri/src/domain/providers/validation.rs       |    2 +-
 src-tauri/src/gateway.rs                           |   11 +
 src-tauri/src/gateway/http_client.rs               |   42 +
 src-tauri/src/gateway/oauth/adapters/codex.rs      |   72 ++
 src-tauri/src/gateway/oauth/provider_trait.rs      |    8 +
 src-tauri/src/gateway/oauth/refresh.rs             |  365 ++++++
 src-tauri/src/gateway/proxy/failover.rs            |   50 +-
 src-tauri/src/gateway/proxy/failover/tests.rs      |   35 +-
 .../handler/failover_loop/prepare/codex_chatgpt.rs |   19 +-
 src-tauri/src/gateway/proxy/mod.rs                 |    1 +
 src-tauri/src/shared/http_body.rs                  |   15 +-
 src/generated/__tests__/bindings.contract.test.ts  |    1 +
 src/generated/bindings.ts                          |   34 +
 src/pages/providers/ProviderEditorDialog.tsx       |    3 +
 src/pages/providers/ProviderModelPolicySection.tsx |  117 +-
 .../__tests__/ProviderEditorDialog.test.tsx        |  244 ++++
 .../__tests__/ProviderModelPolicySection.test.tsx  |  102 +-
 .../__tests__/providerModelPolicy.test.ts          |   42 +
 src/pages/providers/providerModelPolicy.ts         |   63 +-
 src/pages/providers/useProviderEditorForm.ts       |  179 ++-
 .../providers/__tests__/providers.service.test.ts  |   61 +
 src/services/providers/providers.ts                |   29 +
 29 files changed, 2716 insertions(+), 49 deletions(-)

## 48563377

commit 48563377053944a139dd412edb6bb97778534d61
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Fri Aug 7 23:47:09 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Fri Aug 7 23:47:09 2026 +0800

    feat(providers): improve model policy routing UX

 src-tauri/src/app/provider_service.rs              |   6 +-
 src-tauri/src/domain/providers/mod.rs              |   3 +-
 src-tauri/src/domain/providers/model_policy.rs     | 439 +++++++++++------
 src-tauri/src/domain/providers/tests.rs            |  13 +-
 .../failover_loop/prepare/provider_model_policy.rs |   2 +-
 .../gateway/proxy/handler/provider_selection.rs    |  69 ++-
 .../proxy/handler/provider_selection/tests.rs      |  74 ++-
 src-tauri/src/gateway/routes.rs                    |  17 +-
 src-tauri/src/infra/config_migrate/tests.rs        |   4 +-
 src-tauri/src/infra/db/migrations/baseline_v25.rs  |   2 +-
 src-tauri/src/infra/db/migrations/tests.rs         |   4 +-
 src-tauri/src/infra/db/migrations/v37_to_v38.rs    |   4 +-
 .../tabs/__tests__/ClaudeOAuthCard.test.tsx        |   2 +-
 src/generated/bindings.ts                          |   7 +-
 .../hooks/__tests__/useHomeOAuthQuota.test.tsx     |   7 +-
 src/pages/providers/ProviderModelPolicySection.tsx | 541 +++++++++++++--------
 .../__tests__/ProviderEditorDialog.test.tsx        |  99 ++--
 .../__tests__/ProviderModelPolicySection.test.tsx  | 140 +++---
 .../__tests__/SortableProviderCard.test.tsx        |   2 +-
 .../__tests__/providerEditorOAuthActions.test.ts   |   9 +-
 .../__tests__/providerEditorSaveRunner.test.ts     |   9 +-
 .../__tests__/providerEditorSubmitModel.test.ts    |   7 +-
 .../__tests__/providerModelPolicy.test.ts          |  74 ++-
 src/pages/providers/providerModelPolicy.ts         |  89 ++--
 src/pages/providers/useProviderEditorEffects.ts    |   6 +-
 src/query/__tests__/providers.test.tsx             |   7 +-
 .../providers/__tests__/providers.service.test.ts  |   2 +-
 src/services/providers/providers.ts                |   4 +-
 src/test/msw/handlers.ts                           |   4 +-
 29 files changed, 1046 insertions(+), 600 deletions(-)

## 537dd7a8

commit 537dd7a838e9ec720acef3db79a4f086023bf5ac
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Sat Aug 8 21:23:33 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Sat Aug 8 21:23:33 2026 +0800

    fix(providers): correct model policy routing

 src-tauri/src/app/provider_model_discovery.rs      | 198 +++++++----
 src-tauri/src/domain/providers/model_policy.rs     | 218 +++---------
 src-tauri/src/domain/providers/queries.rs          |  11 +-
 src-tauri/src/domain/providers/tests.rs            |  24 ++
 src-tauri/src/gateway/events.rs                    |  60 ++--
 src-tauri/src/gateway/http_client.rs               |  17 +-
 src-tauri/src/gateway/oauth/refresh.rs             | 365 ---------------------
 src-tauri/src/gateway/proxy/gemini_oauth.rs        |  34 +-
 .../failover_loop/prepare/provider_checks.rs       |   3 +-
 .../failover_loop/prepare/provider_iterator.rs     |   7 +
 .../failover_loop/prepare/provider_model_policy.rs |  74 +++--
 src-tauri/src/gateway/proxy/request_end.rs         |  65 ++--
 src-tauri/src/gateway/routes.rs                    |  31 +-
 src-tauri/src/gateway/util.rs                      |  30 +-
 src-tauri/src/infra/config_migrate/tests.rs        |   2 +-
 src/components/home/requestLogSpecialSettings.ts   |   4 +-
 src/generated/bindings.ts                          |   3 +-
 src/pages/providers/ProviderModelPolicySection.tsx | 211 +++++++-----
 .../__tests__/ProviderEditorDialog.test.tsx        | 107 +++---
 .../__tests__/ProviderModelPolicySection.test.tsx  |  98 +++++-
 .../__tests__/providerModelPolicy.test.ts          |  56 +---
 src/pages/providers/providerModelPolicy.ts         |  57 +---
 src/pages/providers/useProviderEditorForm.ts       |  50 +--
 .../__tests__/gatewayEvents.contract.test.ts       |  27 +-
 .../__tests__/requestLogSpecialSettings.test.ts    |  56 ++--
 src/services/gateway/modelRedirect.ts              |  27 +-
 src/services/gateway/requestLogSpecialSettings.ts  |   2 +-
 .../providers/__tests__/providers.service.test.ts  |  29 ++
 src/services/providers/providers.ts                |  14 +-
 29 files changed, 754 insertions(+), 1126 deletions(-)

## def1060c

commit def1060cbabbbb58f93061c26549deb64159286c
Author:     He Ruizhe <123885799+YOLOHIHI@users.noreply.github.com>
AuthorDate: Sat Aug 15 22:15:52 2026 +0800
Commit:     GitHub <noreply@github.com>
CommitDate: Sat Aug 15 22:15:52 2026 +0800

    fix(cli-proxy): 重启后重连代理时刷新 Claude 直连备份，避免关闭期间切换的供应商被覆盖丢失 (#370)

    * fix(cli-proxy): refresh Claude backup snapshot before re-applying stale proxy state

    Exit cleanup restores the direct settings.json but intentionally leaves the
    cli-proxy manifest enabled, so the proxy silently re-applies on the next
    launch (sync_enabled). That re-apply never refreshed the backup snapshot, so
    any edit made to ~/.claude/settings.json between "exit" and "next launch"
    (including manually switching provider base_url/auth_token while the app was
    closed) was immediately overwritten by the gateway address without ever being
    captured. Closing the app again then restored the stale snapshot from the
    very first time the proxy was ever enabled, discarding whatever the user had
    switched to in between.

    Add claude::is_proxy_managed(), a port-independent check (based on our
    placeholder auth token) for whether settings.json is currently under our
    management. In sync_enabled(), when the Claude target is not proxy-managed,
    re-capture the backup from the current on-disk file before applying the
    gateway config, so a later disable restores the latest direct config instead
    of the original one.

    Add a regression test simulating: enable -> exit restore (keep-state) ->
    direct edit to a different provider while closed -> startup sync -> disable,
    asserting the restored config matches the edit made while closed.

    * fix(cli-proxy): 备份刷新判定加入网关地址兜底并补齐守卫测试

    `is_proxy_managed` 只比对 `ANTHROPIC_AUTH_TOKEN` 时，用户在代理运行期间
    手改 token（把占位符换成自己的 key）会让一份仍指向网关的配置被判定为
    直连配置。随后网关端口变化触发 sync，网关地址就被当成直连配置写进备份，
    关闭代理时用户真实的直连配置被永久替换成失效的本机网关地址。

    - claude.rs: token 或本机网关 `/claude` 地址任一命中即视为受管
    - mod.rs: `refresh_backup_from_direct_state` 改为复用既有的
      `capture_current_target_state` + `write_captured_backups`，与 codex
      rebind 走同一套读写路径，同时修掉 `cargo fmt --check` 失败
    - tests.rs: 补齐守卫的反向与失败路径
      - 仅端口变化时不得刷新备份
      - token 被手改后仍不得把网关地址当成直连配置
      - 备份失败时返回 CLI_PROXY_BACKUP_FAILED 且不覆盖原文件

    ---------

    Co-authored-by: dyndynjyxa <andrewguai93@gmail.com>

 src-tauri/src/infra/cli_proxy/claude.rs |  45 +++++++
 src-tauri/src/infra/cli_proxy/mod.rs    |  52 ++++++++
 src-tauri/src/infra/cli_proxy/tests.rs  | 226 ++++++++++++++++++++++++++++++++
 3 files changed, 323 insertions(+)

## 3758d8e8

commit 3758d8e8bd8aadeb9c43f3bd28ffb74aec2ca232
Author:     Skill Test <skill-test@example.com>
AuthorDate: Fri Sep 4 16:24:45 2026 +0800
Commit:     Skill Test <skill-test@example.com>
CommitDate: Fri Sep 4 16:24:45 2026 +0800

    fix(ci): 为依赖审计增加可靠回退

 scripts/check-pnpm-audit.mjs          | 295 ++++++++++++++++++++++++++++++----
 scripts/check-pnpm-audit.selftest.mjs | 254 ++++++++++++++++++++++++++++-
 2 files changed, 521 insertions(+), 28 deletions(-)

## ab83c23f

commit ab83c23f1c782d4c274ead5839f83246952cea78
Author:     Skill Test <skill-test@example.com>
AuthorDate: Fri Sep 4 15:02:08 2026 +0800
Commit:     Skill Test <skill-test@example.com>
CommitDate: Fri Sep 4 15:02:08 2026 +0800

    fix: 修复费用聚合整数溢出

 src-tauri/src/app/gateway_service/sessions.rs      |  21 +-
 src-tauri/src/domain/provider_limit_usage.rs       |  67 +++--
 src-tauri/src/domain/usage_stats/folders.rs        |   6 +-
 .../src/domain/usage_stats/leaderboard_range.rs    |  14 +-
 src-tauri/src/domain/usage_stats/leaderboard_v2.rs |  24 +-
 src-tauri/src/domain/usage_stats/tests.rs          | 101 +++++++
 .../failover_loop/prepare/provider_limits.rs       | 318 ++++++++++++++++++---
 src-tauri/src/infra/request_logs.rs                |  54 +++-
 src-tauri/src/infra/request_logs/types.rs          |   2 +-
 9 files changed, 505 insertions(+), 102 deletions(-)

## 85253db0

commit 85253db097f66f1693ae345257e5dff4525132a3
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Tue Sep 1 19:27:08 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Tue Sep 1 19:27:57 2026 +0800

    fix(request-logs): 修复输出计费与日志费用刷新

 src-tauri/src/domain/cost.rs                    |   3 +
 src-tauri/src/domain/cost/tests.rs              |  44 ++++++++++
 src/hooks/__tests__/useRequestLogsFeed.test.tsx |  63 ++++++++-------
 src/hooks/useRequestLogsFeed.ts                 |  10 +--
 src/query/__tests__/requestLogs.test.tsx        | 103 ++++--------------------
 src/query/requestLogs.ts                        |  70 ++--------------
 6 files changed, 110 insertions(+), 183 deletions(-)

## 3bd9bb59

commit 3bd9bb59143bf94ccaafc91db1afda2d68aca7f2
Author:     He Ruizhe <123885799+YOLOHIHI@users.noreply.github.com>
AuthorDate: Fri Sep 4 09:34:46 2026 +0800
Commit:     GitHub <noreply@github.com>
CommitDate: Fri Sep 4 09:34:46 2026 +0800

    feat(oauth): 上游代理同时作用于 OAuth 登录、令牌刷新与额度查询 (#374)

    * feat(oauth): route OAuth login/refresh through the configured upstream proxy

    Settings -> Upstream Proxy previously only applied to the gateway's
    post-login API calls to Claude/Codex/Gemini/Grok. The OAuth login,
    token-refresh, quota/limit-check and Codex-quota-reset flows built
    their own HTTP client that only looked at AIO_OAUTH_PROXY_URL or raw
    system proxy env vars, so logging in still required a system-wide
    proxy/VPN even with Upstream Proxy configured in the app.

    Reuse the same upstream_proxy_* settings (http/https/socks5/socks5h,
    already validated and tested for self-loop/exit-IP/credentials) for
    every OAuth HTTP client, with AIO_OAUTH_PROXY_URL kept as an explicit
    override for advanced setups. A blank AIO_OAUTH_PROXY_URL now counts as
    unset instead of silently shadowing the configured proxy.

    The socks5 local-DNS IPv4-first workaround the gateway client relies on
    moves into http_client::apply_socks5_local_dns_workaround and is applied
    to the OAuth clients too, so a socks5:// proxy behaves the same on both
    paths. resolve_app_configured_proxy_url also runs the gateway's
    validate_proxy_for_settings, so a hand-edited settings.json pointing at
    the gateway cannot self-loop OAuth traffic.

    The background token-refresh loop keeps its client between polls and
    rebuilds it only when the configured proxy changes, so a proxy toggled
    at runtime applies without restarting the app while pooled connections
    are still reused in the steady state.

    * fix(oauth): 收口上游代理行为并补齐回归测试

    * fix(ci): 修复 Windows lib 测试 manifest

    * fix(codex): 修复 Windows catalog launcher 参数转义

    ---------

    Co-authored-by: dyndynjyxa <andrewguai93@gmail.com>

 src-tauri/build.rs                                 |  56 +---
 src-tauri/src/commands/providers/oauth.rs          |  12 +-
 src-tauri/src/commands/providers/oauth_limits.rs   |   3 +-
 src-tauri/src/commands/providers/oauth_reset.rs    |   3 +-
 src-tauri/src/gateway/background_tasks.rs          |   4 +-
 src-tauri/src/gateway/http_client.rs               |  66 +++-
 src-tauri/src/gateway/oauth/mod.rs                 | 364 ++++++++++++++++++---
 src-tauri/src/gateway/oauth/refresh_loop.rs        | 113 ++++++-
 src-tauri/src/infra/cli_manager.rs                 |  31 +-
 .../src/infra/codex_model_catalog/protocol.rs      |  49 ++-
 src/components/cli-manager/tabs/GeneralTab.tsx     |  10 +-
 .../cli-manager/tabs/__tests__/GeneralTab.test.tsx |   1 +
 12 files changed, 546 insertions(+), 166 deletions(-)

## 9234280f

commit 9234280fa3fefed7241b824b27effb0b126f4cf7
Author:     Skill Test <skill-test@example.com>
AuthorDate: Fri Sep 4 16:44:40 2026 +0800
Commit:     Skill Test <skill-test@example.com>
CommitDate: Fri Sep 4 16:44:40 2026 +0800

    fix(providers): 默认显示当前激活的调用顺序

 .../providers/__tests__/ProvidersView.test.tsx     | 100 +++++++++++++++++++++
 .../providers/hooks/useProvidersViewDataModel.ts   |  22 +++--
 2 files changed, 117 insertions(+), 5 deletions(-)

## 37319565

commit 3731956509e2483ffe24b00576bae7239ad41239
Author:     Lx <33782374+Mlxa0324@users.noreply.github.com>
AuthorDate: Sat Aug 15 21:59:12 2026 +0800
Commit:     GitHub <noreply@github.com>
CommitDate: Sat Aug 15 21:59:12 2026 +0800

    feat(providers): 调用顺序支持定位供应商卡片 (#373)

    * feat(providers): 调用顺序支持定位供应商卡片

    调用顺序侧栏每项新增定位图标，点击后清空搜索/标签过滤条件，
    并平滑滚动定位到供应商列表中对应的卡片，方便快速查看与编辑配置。

    - RouteOrderItemTrailing 新增定位按钮（LocateFixed）
    - 点击清空 setProviderSearch / setSelectedTags 后滚动到 data-provider-id 对应卡片
    - SortableProviderCard 容器增加 data-provider-id 属性供定位查询

    * fix(providers): 定位供应商卡片滚动到列表顶部

    此前定位使用 block: center，目标卡片落在可视区中间。
    改为 block: start，点击定位后目标卡片出现在列表可视区顶部（第一个位置），
    便于查看该卡片及其后序卡片。

    - ProvidersView: scrollIntoView 改为 block: "start"
    - 测试断言锁定 block: "start" 行为

    * fix(deps): bump nanoid to 3.3.18 for GHSA-2v37-7h3g-55p8

    npm 新发布 nanoid 高危 advisory（GHSA-2v37-7h3g-55p8，受影响 <3.3.18），
    阻断 frontend CI 依赖审计。将 override 提升到 3.3.18；因 3.3.18 发布不足 7 天
    被 minimumReleaseAge 拦截，临时加入 minimumReleaseAgeExclude 豁免
    （满 7 天后可移除）。

    - pnpm-workspace.yaml: nanoid override 目标 → 3.3.18，新增 nanoid@3.3.17 覆盖项
    - pnpm-workspace.yaml: minimumReleaseAgeExclude 临时豁免 nanoid
    - pnpm-lock.yaml: nanoid 3.3.17 → 3.3.18

    * chore(deps): 将 nanoid 安全升级移出本 PR

    本 PR 的目标是「调用顺序支持定位供应商卡片」，nanoid 3.3.18（GHSA-2v37-7h3g-55p8）
    属于独立的依赖安全修复，且附带 minimumReleaseAgeExclude 供应链冷却期豁免，
    需要单独审查、单独回滚、单独跟踪豁免回收，因此从本 PR 移出。

    This reverts commit b9bb5dbb1f83e5e302921c8ada0316ed9c6b6896.

    * fix(providers): 定位供应商卡片改用 state 驱动并清理待定意图

    原实现把待定位 providerId 存在 ref 里，靠 effect 依赖 filteredProviders 的引用变化
    来触发滚动。但无过滤时清空过滤并不改变过滤结果，effect 能跑完全依赖
    setSelectedTags(new Set()) 每次新建 Set 造成的引用抖动——一旦该 setter 后续加上
    值相等短路（同一 hook 内 setCreateModeDialogOpen 已是此写法），无过滤定位会静默失效。

    改为用 locateTargetId state 驱动：
    - 三次更新在同一轮批处理内生效，effect 由 locateTargetId 自身变化触发，
      不再依赖 filteredProviders 是否恰好换了引用；
    - 无论是否命中目标都清空 locateTargetId，避免目标被并发删除时意图残留，
      在后续列表变化中触发用户未发起的滚动。

    * test(providers): 补齐定位供应商卡片的边界与失败路径

    - 无过滤时定位仍需滚动：锁定不依赖 filteredProviders 引用抖动的行为契约
    - 路由项对应 provider 缺失时定位按钮禁用
    - 定位完成后再次改变过滤不得重复滚动：验证待定位意图只被消费一次

    同时把 scrollIntoView 打桩从用例内直接赋值 Element.prototype 提到模块级，
    并由 afterEach 清计数——原写法会把 vi.fn 永久留在原型上，调用记录跨用例累积。

    * fix(deps): bump nanoid to 3.3.18 for GHSA-2v37-7h3g-55p8

    nanoid 3.3.18 已于 2026-08-07 发布，至今已超过 minimumReleaseAge 的 7 天冷却期，
    因此只需 overrides 收敛版本，无需 minimumReleaseAgeExclude 豁免——
    也就免掉了「满 7 天后回来移除豁免」这笔无人跟踪的债务。

    ---------

    Co-authored-by: Lx <mlx950325@163.com>
    Co-authored-by: dyndynjyxa <andrewguai93@gmail.com>

 pnpm-lock.yaml                                     |  11 +-
 pnpm-workspace.yaml                                |   3 +-
 src/pages/providers/ProvidersView.tsx              |  62 ++++++++++-
 src/pages/providers/SortableProviderCard.tsx       |   2 +-
 .../providers/__tests__/ProvidersView.test.tsx     | 113 +++++++++++++++++++++
 5 files changed, 180 insertions(+), 11 deletions(-)

## b3343335

commit b33433354aeb6d324a687215f8249b7403f9ecde
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Sat Aug 15 22:18:19 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Sat Aug 15 22:18:19 2026 +0800

    fix(providers): 可用性测试改用供应商已配置的模型

    探测请求此前对 codex 硬编码 gpt-4o-mini、对 claude 硬编码 claude-sonnet-4-6，
    供应商只提供其他模型时测试必然失败（#371）。改为从供应商自己的 model_policy
    取第一个具体模型，再经 resolve_mapping 换算成上游模型名；excluded 模式的
    模型是黑名单不参与探测，取不到候选时回退到原有默认值。

 src-tauri/src/domain/provider_availability.rs | 168 ++++++++++++++++++++++++--
 1 file changed, 157 insertions(+), 11 deletions(-)

## b34fe58a

commit b34fe58afd5539493484498b1cff245f394921d7
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Sat Aug 15 22:41:00 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Sat Aug 15 22:41:00 2026 +0800

    feat(providers): 可用性探测支持自定义模型与提示词

    provider_test_availability 新增可选 model / prompt 参数，优先级为
    覆盖值 → 供应商 model_policy 模型 → 各 CLI 默认值；模型覆盖对
    claude/codex/grok/gemini 均生效（gemini 替换 URL 路径中的模型）。
    默认探测提示词由 ping 改为 hi。

    覆盖值属信任边界：模型拒绝通配符与控制字符，gemini 额外拒绝路径
    分隔符与空白（模型名进 URL 路径），提示词上限 4096 字符，校验失败
    返回 SEC_INVALID_INPUT 且不发起上游请求。

 src-tauri/src/commands/provider_availability.rs |   4 +-
 src-tauri/src/domain/provider_availability.rs   | 265 +++++++++++++++++++++---
 src/generated/bindings.ts                       |   6 +-
 3 files changed, 245 insertions(+), 30 deletions(-)

## 867a0db3

commit 867a0db38588c6e91360bbe4e822e68096dda7b1
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Sat Aug 15 22:41:55 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Sat Aug 15 22:41:55 2026 +0800

    feat(providers): 测试供应商前弹出模型与提示词对话框

    点击「测试」改为先打开对话框：模型用 Input + datalist 展示该供应商
    已配置的具体模型（可自由输入清单外模型），提示词默认 hi，确认后才
    发起探测，取消不产生任何 IPC。空值一律传 null 交由后端回退，避免
    前后端各写一份默认值。测试结果沿用既有 toast。

 src/pages/providers/ProviderTestDialog.tsx         |  93 ++++++++++++++
 src/pages/providers/ProvidersView.tsx              |  25 +++-
 .../__tests__/ProviderTestDialog.test.tsx          | 134 +++++++++++++++++++++
 .../providers/__tests__/ProvidersView.test.tsx     |  23 ++++
 .../__tests__/providerProbeDefaults.test.ts        |  38 ++++++
 .../providers/hooks/useProvidersViewDataModel.ts   |  15 ++-
 src/pages/providers/providerProbeDefaults.ts       |  25 ++++
 src/query/__tests__/providers.test.tsx             |  14 ++-
 src/query/providers.ts                             |   9 +-
 .../providers/__tests__/providers.service.test.ts  |  25 +++-
 src/services/providers/providers.ts                |  10 +-
 11 files changed, 401 insertions(+), 10 deletions(-)

## 6007d7a0

commit 6007d7a09dace7a775a2fb5300c05e165050b340
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Tue Aug 11 21:47:31 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Tue Aug 11 21:47:31 2026 +0800

    feat(app): 添加思考等级展示和模型价格别名优化

    - 在App根组件添加Codex目录刷新反馈的全局监听
    - 在请求日志面板及实时追踪卡片中新增思考等级徽章显示
    - 为请求日志专题数据添加reasoning_effort字段支持
    - 引入多个测试用例覆盖思考等级的展示逻辑
    - 优化模型价格别名对所有CLI模型列表的统一查询
    - 新增按供应商分组展示模型及对应的切换标签页
    - 禁用模型映射编辑时的只读限制，允许随时编辑
    - 更新Provider模型策略编辑区隐藏CX2CC模型映射编辑器逻辑
    - 精简相关API绑定，移除旧版basellm同步接口
    - 类型定义中新增reasoning_effort及供应商vendor字段支持
    - 修正模型映射保存及输入框禁用状态处理
    - 删除未使用的hasClaudeModelMappingSpecialSetting辅助函数及测试
    - 添加旧版映射展示参考，提升模型路由编辑体验

 src-tauri/src/app/gateway_service/lifecycle.rs     |  14 +-
 src-tauri/src/app/provider_model_discovery.rs      |  40 +--
 src-tauri/src/app/provider_service.rs              | 313 +++++++++---------
 src-tauri/src/app/sort_mode_service.rs             |  76 ++---
 src-tauri/src/app/startup_gateway.rs               |   2 +
 src-tauri/src/app/startup_tasks.rs                 |   3 +-
 src-tauri/src/commands/model_prices.rs             |  55 ++--
 src-tauri/src/commands/registry.rs                 |   4 +-
 src-tauri/src/domain/cost.rs                       | 144 ++++-----
 src-tauri/src/domain/cost/tests.rs                 |  73 +++++
 src-tauri/src/domain/cost_stats.rs                 |  85 ++++-
 src-tauri/src/domain/providers/model_policy.rs     |   5 +-
 src-tauri/src/gateway/events.rs                    |  33 ++
 src-tauri/src/gateway/oauth/adapters/codex.rs      |  10 +-
 src-tauri/src/gateway/proxy/abort_guard.rs         |  18 ++
 .../handler/failover_loop/attempt/attempt_auth.rs  |   8 +
 .../failover_loop/attempt/attempt_executor.rs      |  28 +-
 .../failover_loop/attempt/attempt_record.rs        |   6 +
 .../failover_loop/attempt/reasoning_effort.rs      | 146 +++++++++
 .../handler/failover_loop/attempt/retry_engine.rs  |   4 +-
 .../gateway/proxy/handler/failover_loop/context.rs |   6 +
 .../proxy/handler/failover_loop/loop_helpers.rs    |   4 +
 .../src/gateway/proxy/handler/failover_loop/mod.rs |   2 +
 .../failover_loop/prepare/claude_model_mapping.rs  |  41 +--
 .../failover_loop/prepare/provider_iterator.rs     |  36 ++-
 .../failover_loop/prepare/provider_model_policy.rs | 114 +------
 .../failover_loop/response/response_router.rs      |   4 +
 .../failover_loop/response/success_event_stream.rs |   4 +
 .../failover_loop/response/success_non_stream.rs   |  12 +
 .../response/thinking_signature_rectifier_400.rs   |   4 +
 .../failover_loop/response/upstream_error.rs       |   4 +
 .../gateway/proxy/handler/failover_loop/tests.rs   |  20 +-
 .../handler/middleware/provider_resolution.rs      |   7 +-
 .../proxy/handler/middleware/warmup_interceptor.rs |   4 +
 .../gateway/proxy/handler/provider_selection.rs    |  11 +-
 .../proxy/handler/provider_selection/tests.rs      |  55 +++-
 src-tauri/src/gateway/proxy/model_rewrite.rs       | 118 ++++++-
 src-tauri/src/gateway/proxy/request_end.rs         | 314 ++++++------------
 src-tauri/src/gateway/routes.rs                    | 173 ++++++++--
 src-tauri/src/infra/cli_proxy/codex.rs             | 161 ++++++----
 src-tauri/src/infra/cli_proxy/mod.rs               |   1 -
 .../src/infra/codex_model_catalog/projection.rs    |   6 +-
 src-tauri/src/infra/config_migrate/export.rs       |  14 +-
 src-tauri/src/infra/config_migrate/tests.rs        |  18 +-
 src-tauri/src/infra/db/migrations/baseline_v25.rs  |   1 +
 src-tauri/src/infra/db/migrations/mod.rs           |   6 +-
 src-tauri/src/infra/db/migrations/tests.rs         |  63 ++++
 src-tauri/src/infra/db/migrations/v38_to_v39.rs    |  54 ++++
 src-tauri/src/infra/model_prices.rs                |  17 +-
 src-tauri/src/infra/model_prices_sync.rs           | 354 ++++++++++++++-------
 src-tauri/src/infra/model_prices_sync/tests.rs     | 203 +++++++++++-
 src-tauri/src/infra/request_logs.rs                |  99 +++++-
 src-tauri/src/infra/request_logs/costing.rs        |   2 +-
 src-tauri/src/infra/request_logs/queries.rs        |  55 +++-
 src-tauri/src/infra/request_logs/semantics.rs      |  60 ++++
 src-tauri/src/infra/request_logs/types.rs          |   2 +
 src/App.tsx                                        |   5 +
 src/components/home/HomeRequestLogsPanel.tsx       |  10 +-
 src/components/home/LogBadges.tsx                  |  15 +
 src/components/home/RealtimeTraceCards.tsx         |   4 +-
 .../home/__tests__/HomeRequestLogsPanel.test.tsx   |  23 ++
 .../home/__tests__/RealtimeTraceCards.test.tsx     |  39 +++
 .../__tests__/requestLogSpecialSettings.test.ts    |   5 -
 src/components/home/previewData.ts                 |   3 +
 src/components/home/requestLogSpecialSettings.ts   |   1 -
 .../settings/ModelPriceAliasesDialog.tsx           | 142 ++++++---
 .../__tests__/ModelPriceAliasesDialog.test.tsx     |  94 ++++--
 .../__tests__/UsageAvailabilityPanel.test.tsx      |   1 +
 .../__tests__/crossLayerContracts.test.ts          |  10 +-
 src/generated/bindings.ts                          |  28 +-
 .../hooks/__tests__/useHomeOAuthQuota.test.tsx     |   1 +
 src/pages/providers/ProviderEditorDialog.tsx       |   1 +
 src/pages/providers/ProviderModelPolicySection.tsx | 295 +++++++++--------
 src/pages/providers/ProvidersView.tsx              |   2 -
 src/pages/providers/SortableProviderCard.tsx       |   8 +
 .../__tests__/ProviderEditorDialog.test.tsx        |  71 ++++-
 .../__tests__/ProviderModelPolicySection.test.tsx  |  96 +++++-
 .../__tests__/SortableProviderCard.test.tsx        |  11 +-
 .../useCodexCatalogRefreshFeedback.test.tsx        |   5 +-
 .../hooks/useCodexCatalogRefreshFeedback.ts        |   2 +-
 src/pages/providers/providerEditorSubmitModel.ts   |  13 +-
 src/pages/providers/providerModelPolicy.ts         |   1 +
 src/pages/providers/useProviderEditorEffects.ts    |   3 +-
 src/pages/providers/useProviderEditorForm.ts       |  58 +++-
 src/pages/settings/SettingsDataSyncCard.tsx        |  71 ++---
 .../__tests__/SettingsDataSyncCard.test.tsx        | 162 ++++------
 .../settings/__tests__/SettingsSidebar.test.tsx    |  49 ++-
 .../__tests__/settingsSidebarModel.test.ts         |  21 +-
 src/pages/settings/settingsSidebarModel.ts         |   8 +-
 src/pages/settings/useSettingsSidebar.ts           |  14 +-
 src/pages/settings/useSettingsSidebarController.ts |  74 +++--
 src/query/__tests__/keys.test.ts                   |   1 -
 src/query/__tests__/modelPrices.test.tsx           |  75 ++---
 src/query/keys.ts                                  |   1 -
 src/query/modelPrices.ts                           |  34 +-
 src/services/app/__tests__/startup.test.ts         |  33 +-
 src/services/app/startup.ts                        |   7 +-
 .../__fixtures__/gatewayEvents/request.json        |   7 +-
 .../__tests__/gatewayEvents.contract.test.ts       |   3 +
 .../__tests__/requestActivityProjection.test.ts    |   1 +
 .../__tests__/requestLogSpecialSettings.test.ts    |   3 -
 src/services/gateway/__tests__/traceStore.test.ts  |   1 +
 src/services/gateway/gatewayEvents.ts              |  29 +-
 src/services/gateway/requestActivityProjection.ts  |   1 +
 src/services/gateway/requestLogFixtures.ts         |   2 +
 src/services/gateway/requestLogSpecialSettings.ts  |  26 +-
 src/services/gateway/traceStore.ts                 |  18 +-
 src/services/providers/providerEvents.ts           |  13 +-
 .../usage/__tests__/modelPrices.service.test.ts    |  58 ++--
 src/services/usage/modelPrices.ts                  |  66 ++--
 src/test/msw/handlers.ts                           |   7 +-
 111 files changed, 3135 insertions(+), 1713 deletions(-)

## e2d03792

commit e2d037928d2200e3321c3ce614db55db66da32bc
Author:     dyndynjyxa <andrewguai93@gmail.com>
AuthorDate: Wed Aug 12 13:00:10 2026 +0800
Commit:     dyndynjyxa <andrewguai93@gmail.com>
CommitDate: Wed Aug 12 13:00:10 2026 +0800

    fix(gateway): 对齐 CCH v0.9.2 网关整流器行为

 src-tauri/src/app/settings_service.rs              |  45 ++
 src-tauri/src/domain/usage.rs                      | 264 ++++++-
 src-tauri/src/domain/usage/tests.rs                | 111 +++
 src-tauri/src/gateway.rs                           |   6 +
 src-tauri/src/gateway/claude_client_fingerprint.rs | 186 +++++
 .../gateway/claude_metadata_user_id_injection.rs   | 195 ++++--
 .../src/gateway/gemini_function_id_rectifier.rs    | 186 +++++
 src-tauri/src/gateway/oauth/adapters/claude.rs     |  52 +-
 src-tauri/src/gateway/proxy/fake_200.rs            | 347 ++++++++-
 .../failover_loop/attempt/attempt_executor.rs      |  47 ++
 .../handler/failover_loop/attempt/retry_engine.rs  |  61 +-
 .../gateway/proxy/handler/failover_loop/context.rs |   5 +
 .../src/gateway/proxy/handler/failover_loop/mod.rs |   3 +
 .../failover_loop/prepare/codex_service_tier.rs    |  53 +-
 .../failover_loop/prepare/grok_chat_usage.rs       | 124 ++++
 .../failover_loop/prepare/provider_iterator.rs     |  20 +
 .../failover_loop/response/response_router.rs      |   8 +
 .../failover_loop/response/success_event_stream.rs |   6 +
 .../failover_loop/response/success_non_stream.rs   | 122 +++-
 .../response/thinking_signature_rectifier_400.rs   | 241 ++++---
 .../failover_loop/response/upstream_error.rs       | 136 +++-
 .../src/gateway/proxy/handler/middleware/mod.rs    |   5 +
 .../handler/middleware/response_input_rectifier.rs |  75 ++
 src-tauri/src/gateway/proxy/handler/mod.rs         |  32 +-
 .../src/gateway/proxy/handler/runtime_settings.rs  |  24 +-
 src-tauri/src/gateway/proxy/mod.rs                 |   2 +-
 src-tauri/src/gateway/proxy/request_context.rs     |  33 +-
 src-tauri/src/gateway/reactive_rectifier.rs        | 137 ++++
 src-tauri/src/gateway/response_input_rectifier.rs  | 150 ++++
 .../src/gateway/response_output_normalizer.rs      | 177 +++++
 src-tauri/src/gateway/routes.rs                    | 772 ++++++++++++++++++++-
 src-tauri/src/gateway/session_manager.rs           |  10 +-
 src-tauri/src/gateway/session_manager/tests.rs     |  32 +
 src-tauri/src/gateway/streams/usage_tee.rs         | 120 ++--
 .../gateway/thinking_effort_conflict_rectifier.rs  | 184 +++++
 .../src/gateway/thinking_signature_rectifier.rs    |   4 +-
 .../gateway/thinking_signature_rectifier/tests.rs  |  14 +-
 src-tauri/src/gateway/upstream_identity.rs         |   2 +-
 src-tauri/src/infra/db/migrations/mod.rs           |   6 +-
 src-tauri/src/infra/db/migrations/tests.rs         |  47 ++
 src-tauri/src/infra/settings/defaults.rs           |  12 +-
 src-tauri/src/infra/settings/migration.rs          |  78 ++-
 src-tauri/src/infra/settings/mod.rs                |   4 +-
 src-tauri/src/infra/settings/types.rs              |  26 +
 src/__tests__/msw-default-settings.test.ts         |  12 +-
 src/components/cli-manager/tabs/GeneralTab.tsx     |  57 ++
 .../cli-manager/tabs/__tests__/GeneralTab.test.tsx |  11 +
 src/generated/bindings.ts                          |  13 +
 .../cli-manager/useCliManagerPageDataModel.ts      |  18 +-
 .../settingsGatewayRectifier.service.test.ts       |  14 +
 src/services/settings/settings.ts                  |  10 +
 src/services/settings/settingsGatewayRectifier.ts  |  65 +-
 src/test/fixtures/settings.ts                      |   6 +-
 src/test/msw/state.ts                              |  12 +-
 54 files changed, 4037 insertions(+), 345 deletions(-)
