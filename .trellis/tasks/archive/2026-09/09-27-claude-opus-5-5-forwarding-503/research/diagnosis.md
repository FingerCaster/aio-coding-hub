# Claude 单模型失败与可用性误报诊断

## 范围与授权

用户报告 `claude-opus-5-5` 调用出现 503，其他模型可用；另报告发送 `hi` 后上游已计费，但本地测试显示不可用。用户授权对当前启用的 `laoliu-不限端` 自行探测。以下仅诊断，不是实施批准。未修改业务代码、密钥、用户配置或供应商开关。

## 实际环境

- 目标供应商：数据库 ID 73，`cli_key=claude`，`name=laoliu-不限端`，已启用，API Key 鉴权，Base URL `https://laoliu.co`。
- `claude_models_json={}`、`model_mapping_json={}`、供应商路由覆盖为空；全局模型路由策略关闭。该渠道不存在已配置的模型改名规则。
- 运行中的网关：`http://127.0.0.1:37123`，`/health` 报告版本 `0.60.44`。当前工作区 Cargo 版本为 `0.60.43`；不能假定运行二进制与当前工作区完全相同。
- 本机 Claude 全局配置默认选择 `claude-opus-4-8`，但诊断全部显式指定 `claude-opus-5-5`；没有更改默认模型。
- 所有网关诊断固定使用 `/claude/_aio/provider/73/v1/messages`，避免调用其他供应商。

## 运行证据

2026-09-27 UTC 05:46–05:54 执行的诊断；每个 HTTP 探测发送 `hi`、`max_tokens=1`，诊断客户端没有自动重试或跟随重定向。直连诊断未经过 Python 环境代理。

| 场景 | 观测结果 | 时间 | 关联标识 |
| --- | --- | --- | --- |
| 上游直连、测试同形非流式请求 | HTTP 200；响应模型未改名；正常 usage；input=10、output=1 | 3654 ms | upstream `202609270546272023703098268d9d6HeTmJ3q4` |
| 网关定向转发、非流式 | HTTP 200；正常 usage；网关最终状态 200、仅供应商 73、一次成功尝试 | 2848 ms 客户端总时长 | gateway `1790488083-128`；DB #297565 |
| 上游直连、SSE | HTTP 200；收到 message_start/message_delta/message_stop；没有 error 事件 | 首部 3085 ms；收到终止事件 7938 ms | upstream `202609270549234279264298268d9d6AwjsQS9J` |
| 网关定向转发、SSE 诊断读取 | HTTP 200；收到 message_stop；没有 error 事件；诊断器随后主动关闭连接 | 首部 2295 ms；收到终止事件 3714 ms | gateway `1790488170-136`；DB #297574 |
| 实际 Claude CLI，经供应商 73 转发 | exit 0；`subtype=success`、`is_error=false`、结果 `hi`；真实网关最终状态 200，无错误 | API 4554 ms；进程 11221 ms | gateway `1790488418-151`；DB #297589 |

### 诊断器主动断流的边界

第四个场景在读到 `message_stop` 后主动关闭 HTTP 流，没有等 HTTP EOF；因此网关最终记录 #297574 为 `499 / GW_STREAM_ABORTED / origin=direct_drop`。这是本次诊断器产生的客户端中止，不能将其当作用户原始 503 或本地缺陷的复现。第五个场景由真实 Claude CLI 自行消费响应，数据库最终状态是 200。没有删除或改写这些日志。

### Claude CLI 隔离措施

使用本机已安装的 Claude 客户端，`--bare`、`--tools ""`、`--strict-mcp-config`、`--disable-slash-commands`、`--no-session-persistence`、空 setting sources、自定义简短 system prompt，在临时目录运行。不自动发现 CLAUDE.md、不加载项目上下文、不调用工具。子进程仅持有本地网关占位凭据，真实上游密钥由网关注入。设置预算上限 0.02 美元；CLI 报告本次列表价格估算 0.00244 美元，非上游账单证明。

## 代码证据

### 模型与转发

- `src-tauri/src/domain/providers/types.rs:179-213`：Claude 模型类型按 haiku/opus/sonnet 子串识别；无已配置槽位、推理或主模型映射时保留原名。没有针对 `claude-opus-5-5` 的版本白名单。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/claude_model_mapping.rs:28-43`：仅在 Claude 供应商已配置映射时执行替换；从原始请求提取模型并比较实际结果。
- `src-tauri/src/gateway/routes.rs:53-73,108-117`：已有供应商定向路径将 ID 注入内部路由头，不需要修改全局供应商配置。
- 真实 CLI 请求 #297589 的 attempt 记录 `requested_upstream_model=claude-opus-5-5`、`provider_id=73`、`upstream_sent=true`、`reason_code=request_success`。

### 可用性测试与正式转发并非同一调用

- `src-tauri/src/domain/provider_availability.rs:14-19`：连接超时 8 秒，整个探测超时 15 秒；默认提示词 hi；响应体最多 64 KiB，预览 500 字节。
- `src-tauri/src/domain/provider_availability.rs:323-336`：Claude 探测直接 POST 供应商 `/v1/messages`，`max_tokens=1`，不主动请求流式。
- `src-tauri/src/domain/provider_availability.rs:528-548`：探测单独创建 reqwest 客户端并直接请求上游，不经过本地网关转发链路。
- `src-tauri/src/domain/provider_availability.rs:418-439,552-615`：5xx、鉴权失败和发送异常可导致不可用；网络超时报“请求超时（15秒）”。成功判定不要求回复必须等于 hi，也不要求一定返回可见文本 token。
- 本次直连成功响应前 500 字节中没有鉴权失败关键词；按当前源码判定应为可用。这里只核对判定条件，不声称实际调用了 GUI 的 Tauri 命令。
- `src/pages/providers/hooks/useProvidersViewDataModel.ts:953-977`：界面依据后端 `result.ok` 显示结果，失败文本为 `不可用 — ${result.error}`；日志保留 status、latency_ms、error。
- `src/services/consoleLog.ts:227-249`：这类前端日志追加至模块内存中的 entries，而非 request_logs 数据表；仅日志级别偏好使用 localStorage。

## 尚不能下的结论

- 当前成功不否定用户此前 503，也不能证明上游所有时段、长上下文、完整工具集或历史 thinking 签名都正常。
- 上游后台计费证明上游进行了处理，不足以证明当次响应及时、完整地交付给 AIO 探测客户端。
- 15 秒超时是源码中的明确约束，但本次非流式响应均低于 4 秒，不能据此认定用户故障就是超时。
- 未在供应商 73 的已有持久化请求记录中找到用户原始失败；可用性测试本身不经过网关请求日志。
- 原生界面工具初次仅返回空壳控件，截图失败；再次检查旧窗口已不存在，仅列出隐藏的 single-instance 窗口。没有取得测试按钮的历史错误详情，也没有执行 GUI 测试或声称完成 UI 验证。

## 下一项必需证据

用户当次 `不可用 —` 后面的完整错误文字，或应用控制台“供应商可用性测试”的脱敏详情（status、latency_ms、error）。该证据用于区分 15 秒超时、上游 HTTP 503、鉴权关键词命中及传输错误。取得前不提出盲目延长超时、重试、换模型或修改转发机制的补丁。
