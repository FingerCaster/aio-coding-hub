# 技术设计：在最新 main 上恢复 Codex 流错误 UI 归属

## 基线与边界

`926279e6` 与当前 main 不是线性关系，不能整体 cherry-pick。仅提取其 UI 语义并
适配 main 当前组件：main 的 `CodexInfiniteRetryTestSection`、CX2CC 入口和其他新
功能必须保持不变。

## 数据流

```text
AppSettings
  -> useCliManagerPageDataModel（唯一全局 retry policy/guard 草稿）
       -> GeneralTab + RetryPolicyFields（HTTP/transport/共享预算）
       -> CodexTab + CodexStreamInternalErrorFields（Codex stream slice + guard）
  -> persistCommonSettings({ upstream_retry_policy, stream_internal_error_guard_ms })

Provider override（完整 UpstreamRetryPolicy）
  -> ProviderEditorDialog
       -> RetryPolicyFields（所有 CLI）
       -> CodexStreamInternalErrorFields（仅 codex）
```

## 组件契约

- `RetryPolicyFields` 保持接收完整 `UpstreamRetryPolicy`，但只渲染/编辑通用字段；每次
  `onChange` 用展开保留 `stream_internal_errors`。可选的
  `sharesBudgetWithCodexStreamErrors` 仅影响共享预算提示。
- 新的 `CodexStreamInternalErrorFields` 只接收/返回当前 main 的
  `UpstreamStreamInternalErrorPolicy`（编辑 `passthrough_keywords`，只读展示
  `legacy_retry_keywords`），观察窗口 props 可选；全局 Codex tab 传入窗口，Provider
  override 不传。旧提交中的 `retry_keywords/non_retry_keywords` 已不属于当前 schema，
  不应重新引入。
- Codex tab 的保存逻辑复用页面已有 `persistCommonSettings`，先验证完整 policy 与
  0..MAX guard，再以 canonical 返回值同步页面草稿。
- Provider 非 Codex 分支只隐藏组件，不清理 `stream_internal_errors`。

## 保留项与风险控制

- 不改后端原生 Codex-only gate、retry/failover/circuit、buffer、日志或 schema。
- 不删除 main 当前 Codex 无限重试测试模式及其 active request 统计。
- 通过组件和页面测试断言隐藏字段保留、共享字段合并及保存 payload 完整性。

## 文档与生成物

更新 upstream error handling contract 的 UI ownership 描述；本次不应重新生成绑定，
但仍运行生成绑定检查以证明无漂移，并运行 spec-link 检查。
