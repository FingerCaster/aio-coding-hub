# 规则模型与迁移研究

## 本项目现状

- `UpstreamRetryPolicy` 当前以 `status_codes: Vec<u16>` 表示 HTTP 瞬时错误，默认值是 `502/503/504`；传输错误、重试次数、退避和熔断计数是同一策略的独立字段（`src-tauri/src/infra/settings/types.rs:62`）。
- Provider 覆盖整套替代全局策略（`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:153`）。
- 配置型瞬时重试不是通用重试的唯一来源。底层分类仍会让所有 5xx、408/429 和大部分 4xx 在 Provider 基线尝试预算内重试（`src-tauri/src/gateway/proxy/errors.rs:49`）。
- HTTP 策略目前在读取响应体之前按状态码决定；错误处理随后最多读取并解压 64 KiB，用于 4xx 分类、诊断和修复（`src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs:108`、`:357`）。
- 401/403 响应体可以在内存中参与必要分类，但不得进入持久化预览（`src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs:379`）。
- 全局设置 JSON 有版本化修复和规范化回写；Provider 覆盖存放在 SQLite `providers.upstream_retry_policy_json`，当前数据库版本是 38；单 Provider 分享 v1 使用 `deny_unknown_fields` 严格结构（`src-tauri/src/infra/settings/migration.rs:851`、`src-tauri/src/infra/db/migrations/mod.rs:1`、`src-tauri/src/domain/providers/share.rs:40`）。

## sub2api 参考行为

- 规则字段为 `error_code`、`keywords[]`、`duration_minutes`、`description`（`D:\UGit\sub2api\backend\internal\service\account.go:127`）。
- 匹配整个响应体前 64 KiB，不解析 JSON；状态码必须相等，关键词按不区分大小写的普通子串匹配，多个关键词为 OR（`D:\UGit\sub2api\backend\internal\service\ratelimit_service.go:2121`）。
- sub2api 强制关键词非空；本任务按用户决策扩展为空列表表示该状态码全部匹配。
- sub2api 命中后写入模型级或账号级临时不可调度状态（`D:\UGit\sub2api\backend\internal\service\ratelimit_service.go:2241`）。本任务只借鉴规则模型，不引入跨请求摘除。

## 结论

- 新 HTTP 规则应成为配置型瞬时重试的唯一 HTTP 判定来源，但不替代通用故障转移重试。
- 响应体只读取一次：规则扫描需求与现有错误分类、诊断和修复需求合并。
- 旧 `status_codes` 需在全局设置、Provider SQLite JSON 和 Provider 分享 v1 三个入口分别迁移。
- Provider 分享不能向严格 v1 偷加字段；新导出必须使用明确 v2，新版本保留严格 v1 读取并转换。
- 规则内容和描述必须有数量/长度上限。规范化不得把一个原本含有无效内容的规则静默扩大成“空内容全部匹配”。
