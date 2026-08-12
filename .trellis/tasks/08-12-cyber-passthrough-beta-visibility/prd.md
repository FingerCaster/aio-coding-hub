# 修复并验证 Cyber 透传例外在 Beta 中可见

## Goal

修复已安装 Beta 用户升级后 `high-risk cyber` 默认透传例外丢失的问题，使旧配置在升级到新 schema 后恢复该默认值，并从通过验证的 `origin/main` 不可变提交发布下一版公开 Beta。

## Background

- 本机已安装 `0.60.41-beta.2`，安装包与发布源提交 `99956fd2` 一致；二进制包含 Cyber 透传功能，不是安装了错误资产。
- 本机 canonical settings 为 schema 61，`stream_internal_errors.passthrough_keywords` 已持久化为 `[]`。
- 当前反序列化合同只在字段缺失时填入 `high-risk cyber`，显式空数组会保留；schema 59-61 的迁移没有修复旧 Beta 写入的空数组。
- 历史产品决策明确要求 Cyber 错误作为默认透传例外，但旧配置已经无法区分“旧版本生成的空默认值”和“用户主动清空”。
- 当前远端基线为 `origin/main=b5a5028b`，现有最新公开 Beta 为 `0.60.41-beta.2`；发布前必须重新读取远端状态并动态确定下一规范 Beta 版本。

## Requirements

### R1. 默认值合同

- 新安装、缺失 `stream_internal_errors`、或缺失 `passthrough_keywords` 时，canonical 默认列表包含且仅包含 `high-risk cyber`。
- 关键词继续使用现有规范化、去重和大小限制；不得在前端单独伪造一个只显示不生效的默认值。
- `enabled=false`、非空自定义列表、Provider 完整 override 和其他 retry/firewall 设置保持原值。

### R2. 一次性旧 Beta 修复

- settings schema 从 61 升到 62，并增加一个可审计、幂等的一次性迁移。
- 对 schema 61 及以下的 canonical 全局设置，若 `passthrough_keywords` 为空，则补入 `high-risk cyber`。
- schema 62 及以上的显式空列表必须永久保留；用户在完成本次迁移后再次清空，后续启动不得补回。
- 旧 schema 的配置导入走同一迁移语义；当前 schema 导入、非空自定义列表和 Provider override 不得被扩大。
- 迁移必须经共享 settings 持久化路径原子写入，不新增旁路 writer，也不直接修改用户配置文件。

### R3. UI 可见性

- UI 从后端 canonical settings 渲染并保存透传例外，不在 React 层覆盖迁移结果。
- 升级后的用户可在 `CLI 管理 -> 通用 -> 上游错误处理 -> 重试规则 -> Codex 流终态防火墙` 中看到 `high-risk cyber`。
- 编辑、清空、保存和重新读取保持一致；空列表在 schema 62 中是有效的用户选择。

### R4. 回归验证

- Rust 迁移测试至少覆盖：schema 58/59/60/61 空列表补回、schema 62 空列表保留、非空列表保留、关闭开关保留、重复迁移幂等、旧配置导入。
- Rust 反序列化测试继续覆盖字段缺失使用默认值、当前 schema 显式空列表保留。
- 前端测试覆盖默认模型与 canonical settings 对齐，以及迁移后 UI 文本框显示 `high-risk cyber`；不得只断言硬编码常量。
- 运行生成绑定检查、前端类型/lint/聚焦测试、Rust fmt/check/Clippy/聚焦与全量测试、发布合同自测和 `git diff --check`。

### R5. 集成与 Beta 发布

- 修复从最新 `origin/main` 的独立 worktree 开始，不在当前落后且有用户改动的主工作区实现。
- 只操作 `origin` / `FingerCaster/aio-coding-hub`；不得读取或写入 `upstream`。
- 通过 PR 将修复合入 `origin/main`，发布源必须是合入后、全部门禁通过且 `origin/main` 可达的 40 位提交 SHA。
- 通过现有手动 Beta workflow 发布下一规范版本；若远端状态不变，候选为 `0.60.41-beta.3`。
- Release 必须 `draft=false`、`prerelease=true`、`make_latest=false`，资产、签名、manifest 和 `release-channels` Beta 指针均通过独立复核。
- 稳定 `latest`、稳定 `latest.json`、Homebrew 和未选择 Beta 的用户不得受影响。

## Acceptance Criteria

- [ ] AC1：用本机当前 schema 61 空列表 fixture 启动迁移后，canonical settings 升至 schema 62 且列表为 `["high-risk cyber"]`。
- [ ] AC2：schema 62 中用户主动清空并重启后仍为空；非空自定义列表、`enabled=false` 和 Provider override 均不变。
- [ ] AC3：UI 聚焦测试及实际桌面 smoke test 均能在正确设置路径看到 `high-risk cyber`。
- [ ] AC4：相关 Rust、前端、生成绑定、全量质量门禁和发布合同全部通过，且 spec 与实现一致。
- [ ] AC5：修复经 PR 合入最新 `origin/main`，没有覆盖当前主工作区或其他并行成果。
- [ ] AC6：下一公开 Beta 从不可变 `origin/main` SHA 构建成功，官方资产、签名、manifest、版本和 Beta 指针完整一致。
- [ ] AC7：GitHub stable latest、稳定 updater 指针、Homebrew 及 Beta 未参与用户保持不变。

## Out Of Scope

- 自动安装新 Beta 或直接手改本机 `settings.json`。
- 改变终态防火墙的分类、重试、透传优先级或 Provider override 语义。
- 发布稳定版、RC、Nightly，或覆盖既有 tag / Release 资产。
- 操作 `upstream`。

## Resolved Decisions

- 用户已确认：对 schema 61 及以下的所有空列表执行一次补回。这能自动修复已受影响用户；极少数曾在旧 Beta 中主动清空列表的用户会在本次升级后看到 Cyber 被补回一次，他们在 schema 62 中再次清空后会永久保留。
