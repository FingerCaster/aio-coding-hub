# 规则化瞬时错误重试

## Goal

将“熔断与重试”中固定状态码的配置型瞬时错误重试改造成用户可维护的 HTTP 规则，使管理员能够按上游错误码和错误响应内容决定是否在单次请求内重试同一 Provider，而不必为每类上游错误修改代码。

## Background

- 当前 `UpstreamRetryPolicy` 包含启用状态、HTTP 状态码列表、传输错误列表、重试次数、退避间隔及是否计入熔断；默认启用 `502/503/504 + connect/timeout/read`、同 Provider 重试 1 次、退避 100 ms，配置重试默认不计入熔断（`src-tauri/src/infra/settings/types.rs:62`）。
- 当前前端只能勾选固定的 `502/503/504`，不能录入其他状态码或响应内容条件（`src/services/gateway/upstreamRetryPolicy.ts:3`、`src/components/gateway/RetryPolicyFields.tsx:41`）。
- Provider 覆盖优先于全局策略；没有覆盖时继承全局策略，显式禁用的 Provider 覆盖不会回退全局（`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:153`）。
- 配置型瞬时重试按状态码或传输错误匹配并映射为同 Provider 重试（`src-tauri/src/gateway/proxy/handler/failover_loop/attempt/upstream_retry_policy.rs:6`）；底层通用分类仍可让所有 5xx、408/429 和大部分 4xx 使用 Provider 基线尝试预算（`src-tauri/src/gateway/proxy/errors.rs:49`）。
- 当前错误路径已经可以有界读取、解压并扫描响应体；扫描读取上限为 64 KiB，且 401/403 响应体不得进入持久化预览（`src-tauri/src/gateway/proxy/upstream_client_error_rules.rs:3`、`src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs:379`）。
- `sub2api` 的参考规则由错误码、关键词列表、临时不可调度时长和描述组成；匹配语义是“状态码相等 AND 任一关键词按不区分大小写的普通子串命中”，响应体最多扫描 64 KiB（`D:\UGit\sub2api\backend\internal\service\account.go:127`、`D:\UGit\sub2api\backend\internal\service\ratelimit_service.go:2121`）。
- `sub2api` 命中后会写入模型级或账号级临时不可调度状态并影响后续请求（`D:\UGit\sub2api\backend\internal\service\ratelimit_service.go:2241`）；本任务只借鉴规则模型。
- 历史实现曾明确排除响应体关键词匹配；本次用户请求显式重新打开该范围，其他既有重试语义继续兼容。

## Requirements

### Rule Model

- R1：配置型 HTTP 瞬时错误识别采用用户可维护的规则列表。
- R2：用户能够新增、编辑、启停和删除 HTTP 规则。每条规则包含启用状态、HTTP 错误码、零到多个匹配内容及可选描述；启用状态默认开启。
- R3：HTTP 错误码允许 `400-599`，不再局限于固定的 `502/503/504`。所有规则执行同一种策略级重试动作，不提供排序、优先级或单规则重试参数。
- R4：错误码必填、匹配内容可选。匹配内容为空时，任意响应体（包括空响应体）均按该错误码命中；匹配内容非空时，必须同时满足“错误码相等 AND 任一内容命中”，空响应体不命中。
- R5：匹配内容采用不区分大小写的普通子串匹配；一条规则内多个内容以及多条 HTTP 规则均为 OR。不支持正则表达式或通配符。

### Transport Integration

- R6：`connect / timeout / read` 传输错误保持为独立勾选项，不并入 HTTP 规则。HTTP 或传输条件命中后共用策略级重试次数、退避间隔和熔断计数设置。

### Compatibility And Safety

- R7：旧 `status_codes` 中每个有效状态码自动转换为一条“已启用、匹配内容为空、描述为空”的 HTTP 规则。迁移覆盖全局设置、Provider 覆盖及 Provider 分享/导入；其他策略字段原样保留。新配置只持久化新规则字段。
- R8：内容条件针对解压后的整个错误响应体文本扫描，不依赖特定 JSON 字段；最多检查前 64 KiB。扫描仅用于内存判定，不能新增持久化的认证响应体、匹配内容或命中片段。

### Retry Semantics And Scope

- R9：规则命中后只驱动现有单请求内的同 Provider 瞬时重试；重试耗尽后继续沿用现有切换/失败流程。
- R10：总开关关闭时保留 HTTP 规则和传输错误配置，但两者均不参与配置型重试。总开关开启时，至少存在一条已启用 HTTP 规则或一个传输错误；HTTP 规则列表可为空，只要传输错误非空。
- R11：HTTP 规则不取代底层通用故障转移重试。未命中规则时继续沿用 `failover_max_attempts_per_provider` 和既有错误分类；命中时应用 `max_retries`、`backoff_ms` 与 `counts_toward_circuit_breaker`。
- R12：Provider 未开启覆盖时继承完整全局策略；开启后，Provider 自身的 HTTP 规则、传输错误及重试参数整套替换全局策略，不追加或合并。

### UX And Observability

- R13：首版编辑器提供新增、编辑、启停和删除，并随整套重试策略统一保存；不提供复制、拖拽排序或预设。
- R14：配置型 HTTP 规则实际触发重试时，现有请求尝试记录的 `reason` 包含命中规则的 1-based 序号和非空描述。描述必须经过长度和控制字符约束，不记录具体匹配内容或响应正文。

## Acceptance Criteria

- AC1（R1、R2、R13）：全局及 Provider 覆盖界面均可新增、编辑、启停和删除规则，并通过各自现有保存入口持久化；不出现复制、排序或预设控件。
- AC2（R2、R3）：缺失、非整数、小于 400 或大于 599 的状态码不能保存；`400` 与 `599` 可以保存并参与匹配。
- AC3（R2）：停用规则保留配置但不参与匹配，重新启用后恢复；删除后持久化结果不再包含该规则。
- AC4（R4）：仅错误码规则对对应状态的空/非空响应体均生效；带内容条件的规则只在同状态且内容命中时生效，空正文不命中。
- AC5（R5）：大小写差异不影响匹配；多个内容和多条规则均按 OR 工作；正则/通配符字符按普通文本处理。
- AC6（R6、R10）：HTTP 规则与传输错误可独立为空，但策略开启时二者不能同时没有有效匹配器；总开关关闭后原配置完整保留。
- AC7（R6、R9）：HTTP 规则或传输错误触发的配置型重试共用相同预算、退避和熔断计数语义；耗尽后切换/失败行为不变。
- AC8（R11）：未命中规则的 5xx、408/429 和其他既有错误仍遵循原通用重试/切换决策，不改变基线 Provider 尝试预算；count-tokens、model discovery 及专用内部修复路径保持严格。
- AC9（R12）：Provider 未覆盖、整套覆盖及显式禁用覆盖均有自动化测试；Provider 规则不与全局规则隐式合并。
- AC10（R7）：旧默认 `502/503/504` 升级为三条仅错误码规则；自定义旧状态码、Provider 显式禁用及其他策略字段无损转换，新序列化不再含 `status_codes`。
- AC11（R7）：有效旧 Provider SQLite 覆盖自动改写为新格式；malformed 覆盖保持显式禁用而非继承全局，且不能阻断其他有效数据迁移。
- AC12（R7）：旧 Provider 分享 v1 仍被严格读取并转换；新导出使用能完整保存 HTTP 规则的明确新版本，两版均拒绝各自 schema 外的未知字段并保持原 secret/native-I/O 边界。
- AC13（R8）：JSON、嵌套 JSON 与纯文本均可按整个错误正文命中；压缩、超大、空、非 UTF-8 和读取失败均有有界、无 panic、fail-closed 的测试，扫描文本不超过解压后前 64 KiB。
- AC14（R8、R14）：规则触发记录可定位规则序号和可选描述，但控制台、事件、attempt reason、request log 和 error details 不新增匹配内容、命中片段或响应正文；401/403 同样满足。
- AC15（R1-R14）：规则数量、内容数量/长度、描述长度/控制字符、迁移边界、匹配决策和前后端 round-trip 均有自动化测试；生成绑定、类型检查、lint、Rust 格式/检查及相关全量测试通过或明确记录环境级执行阻断。

## Out of Scope

- 不新增类似 `sub2api` 的跨请求 Provider/模型临时摘除、持续时间、持久化、手动恢复或调度状态展示。
- 不匹配 HTTP `1xx-3xx`，不解析 HTTP 200 成功响应正文中的业务错误。
- 不新增正则、通配符、规则专属动作/重试参数、规则复制、拖拽排序或预设管理。
- 不新增 per-CLI、路由级或请求级规则作用域。
