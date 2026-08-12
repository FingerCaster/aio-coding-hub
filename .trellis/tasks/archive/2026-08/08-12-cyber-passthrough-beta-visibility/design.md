# 技术设计

## 1. 修复边界

根因在 settings schema 演进，不在 MSI 打包或 UI 控件。修复应落在 canonical settings 迁移层：后端先把旧空默认值升级为真实默认值，前端继续只消费 canonical settings。这样运行时分类、UI 展示、配置导出和后续保存使用同一份数据。

## 2. Schema 62 迁移

新增 `SCHEMA_VERSION_RESTORE_CYBER_PASSTHROUGH = 62`，沿现有顺序迁移链执行：

1. 若原始配置明确为 schema 62 或更高，迁移不修改列表。
2. 若 schema 缺失或小于 62，先完成既有迁移和规范化。
3. 仅当全局 `upstream_retry_policy.stream_internal_errors.passthrough_keywords` 为空时，写入共享常量 `DEFAULT_CYBER_PASSTHROUGH_KEYWORD`。
4. 将 schema 升至 62，并通过既有原子持久化路径保存。
5. 重复运行结果不变。

不修改当前 wire 反序列化语义：字段缺失仍使用默认值，schema 62 显式 `[]` 仍表示用户主动清空。非空列表不自动追加 Cyber，避免扩大用户自定义的透传边界；Provider 完整 override 也不参与全局迁移。

## 3. 跨层投影

Rust 的默认常量仍是唯一行为源。TypeScript 默认工厂必须与其一致，但 UI 不根据 schema 或空数组自行补值。设置页沿现有字段所有权保存 canonical 列表，避免旧快照覆盖迁移或并发 writer。

实际可见路径为：

```text
CLI 管理
  -> 通用
  -> 上游错误处理
  -> 重试规则
  -> Codex 流终态防火墙
  -> 透传例外关键词
```

## 4. 测试策略

- 构造 schema 58-61 的空列表 fixture，验证迁移到 62 后补入 Cyber。
- 构造 schema 62 空列表 fixture，验证读取、保存、重启后仍为空。
- 验证非空自定义列表、关闭开关、Provider override、旧字段 alias 和重复迁移不变。
- 用真实 settings adapter/组件路径验证 canonical Cyber 出现在文本框，不以局部硬编码代替后端数据。
- 用本机问题配置的脱敏副本做桌面 smoke test，不直接覆写生产配置。

## 5. 集成与发布

实现 worktree 从发布前最新 `origin/main` 创建。修复提交经 PR 和 CI 合入后，重新解析当前稳定版本、现有 Beta tag 和 Beta pointer，选择严格递增版本。手动 Beta workflow 接收合入提交的 40 位 SHA，先完成 Release/资产发布，再以 CAS 推进 Beta 指针。

发布后独立核对：tag 目标、Release flags、14 项官方资产、updater manifest 字节与签名、四平台 URL、Beta pointer/state，以及 stable latest/Homebrew 未变化。任何门禁失败都停止在 tag/Release/pointer 变更之前，或按现有 recovery 流程恢复，不手工覆盖资产。

## 6. 风险与取舍

- schema 61 及以下无法识别空列表来源。一次性补回优先兑现已确认的默认 Cyber 产品语义，代价是旧 Beta 中主动清空过的用户需再清空一次。
- 不在 UI 层兜底，避免界面显示与运行时策略不一致。
- 不直接修改本机配置；发布后的更新检测可以验证版本可见，安装仍由用户决定。
