# 集成设计

## 1. 集成边界

工作树以 `origin/main` 为基线，按以下顺序纳入变更：

1. cherry-pick 已提交的余额修复 `69e9fab2`；
2. 从 `beta-updater-target-fix` 工作树提取 updater 源码/spec/task 变更，形成独立集成提交；
3. 重新读取 `origin/main`，处理期间新增提交带来的语义冲突；
4. 仅在最终集成树上运行发布前门禁。

余额修复涉及 `provider_account_usage` 共享请求构造和 custom script request builder，不能只合入某一个适配器。updater 修复涉及 `desktop.rs` 的平台身份映射，必须保留 `Update.target` 与 `tauri_plugin_updater::target()` 的不同语义。

## 2. 冲突策略

- 同一文件若同时包含余额/更新两类改动，按调用链逐段合并，不接受整文件版本覆盖。
- 生成绑定只在 Rust 命令签名变化时更新；无签名变化的生成文件漂移视为冲突并恢复到生成器结果。
- 跨层 spec 合并新增可执行合同，不删除已有 Beta、账户用量或稳定发布规则。
- 任何安全校验、凭据隔离、路由/circuit 副作用保护冲突均 fail closed，暂停发布并复审。

## 3. 发布数据流

```text
origin/main -> integration branch -> PR merge SHA
  -> release.yml beta build (immutable SHA)
  -> public prerelease + exact 14 assets
  -> release-channels latest-beta.json/state CAS
  -> Windows updater check + account-usage smoke
```

发布版本必须从现有 Beta 高水位递增，不能复用 tag 或覆盖资产。Stable latest、Homebrew 和稳定用户路径不应产生变化。

## 4. 回滚

任何测试、PR CI、资产摘要、pointer CAS 或 Windows smoke 失败都停止发布；已合入代码通过后续修复 PR 回滚，不能移动既有 Beta tag 或强制覆盖 `release-channels`。
