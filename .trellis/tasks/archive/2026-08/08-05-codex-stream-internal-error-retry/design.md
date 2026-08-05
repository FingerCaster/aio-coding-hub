# 技术设计

## 策略与迁移

扩展 UpstreamRetryPolicy：

    UpstreamStreamInternalErrorPolicy {
      enabled: bool,
      retry_keywords: Vec<String>,
      non_retry_keywords: Vec<String>,
    }

缺失字段使用默认策略；保存时 trim、拒绝空值/控制字符、按大小写不敏感去重，列表上限与当前 HTTP body matcher 合同一致。全局 stream_internal_error_guard_ms 默认 500、范围 0..=5000，由普通 settings writer 拥有；Provider override 继续替换完整 UpstreamRetryPolicy。

迁移在当前 settings schema 上顺延，扫描全局和非空 Provider override。以下任一已表达用户意图：启用/禁用的 400 status-only rule、任何包含 capacity 关键词的 400 rule、显式全 400 覆盖。否则追加可见、可编辑的 400 + capacity 默认 rule；迁移幂等，不删除用户规则。

## SSE 保护状态机

原生 Codex Responses 路径把 upstream bytes 转成单一可解码流。prefix parser 与后续 relay 共用 SSE frame parser；其他协议保持当前路径。

    Buffer metadata
      -> first meaningful output starts deadline
      -> inspect known error envelope
           retry match -> discard buffer -> failover retry decision
           non-retry/unknown -> commit buffer -> raw relay
      -> completion/EOF/deadline -> commit buffer
      -> 1 MiB cap -> commit buffer + guard diagnostic

meaningful output 包括真实 text、refusal、reasoning summary、function/tool arguments、具体 output/tool item；response.created/response.in_progress 等 metadata 不启动 deadline。guard 0 在第一次可观察的真实 output 时立即提交。

错误 evidence 由 Rust helper 统一分类：事件名、data.type、已知 error type/code/message 组成匹配文本；不扫描任意正常 output 字段。正向词优先，message 脱敏后再持久化。

## Failover 接入

- 新增 RetryPolicyMatch::StreamInternalError，在现有 transient decision 中走 RetrySameProvider、SwitchProvider 或 Abort。
- configured_transient_retries_used 对 HTTP/transport/stream-internal 统一计数；OAuth、continuation、thinking rectifier 和 helper retry 不占用该计数。
- backoff 只由现有最终决策 helper 执行。stream guard 返回 retry intent 后不得在 guard 自己等待；切换 Provider 无额外 delay。
- counts_toward_circuit_breaker 由 effective policy 决定；guard cap/guard timeout/下游已提交不计 failure。
- 所有 Provider exhausted 时把最后证据投影到 terminal error_details_json，并返回标准 fake200 502；早期证据继续保留在 attempts_json。

## 日志与 UI

FailoverAttempt 增加可选 stream_internal_error：event/type/code/message/classification/matched_keyword/disposition/truncated。字段各自有界，message 最多 2048 Unicode 字符并清理 Bearer、sk-/API key、access_token/api_key/token 值。下游提交后 tracker 只能更新当前 optimistic success attempt 的 evidence，不能触发 retry。

前端 attemptsJson.ts 是 evidence 类型守卫和 bounded parser 的唯一入口；ProviderChainView、RequestLogErrorObservationCard、Home/Logs 使用 projection。复制按钮只复制持久化后的脱敏 message。

## 测试策略

- parser/classifier 纯函数测试覆盖四类 terminal type、未知字段、chunk 边界、priority keyword。
- guard 使用 Tokio paused time，测试 deadline/cap/EOF/completion 和 no-splice。
- route 测试真实 Provider chain、同 Provider retry、切换、circuit count、health-neutral、continuation/encoding/backoff 回归。
- settings/migration/share/import/backup 测试缺失字段、完整 override、幂等默认规则与显式禁用。
