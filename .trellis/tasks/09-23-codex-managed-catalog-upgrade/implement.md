# 实施清单

## Sequence

- [x] 在 `managed.rs` 增加强制重读意图。跳过指纹一致时复用旧生成字节的短路，但仍使用现有基础来源、生成器和三文件 apply。
- [x] 抽出不写盘的基础校验，收集全部可修复的启用规则。重复 slug、坏 JSON、模板缺失和别名冲突保持硬错误。
- [x] 实现 `Apply` 与 `DisableInvalidAndApply`。后者复用规则设置 CAS 和目录补偿，并拒绝过期的模型 ID 集合。
- [x] 注册 specta 命令和前端 generated bindings。不要把规则字段塞进普通 `SettingsUpdate`。
- [x] 在 Codex 模型上下文规则区域加升级按钮、无效规则列表和禁用确认。启动降级且错误含规则目标码时自动请求一次 `Apply`。
- [x] 成功后失效设置、Codex 目录和上下文候选查询。文案只承诺新启动的 Codex 会话。
- [x] 更新 `codex-managed-model-route-contract.md` 的升级命令和显式禁用边界。不放宽启动失败时保留最后有效目录的要求。

## Validation

- Rust：`cargo test --manifest-path src-tauri/Cargo.toml managed_catalog -- --test-threads=8`
- Rust 规则事务：`cargo test --manifest-path src-tauri/Cargo.toml codex_model_context_rules -- --test-threads=8`
- 前端：`pnpm test:unit src/components/cli-manager/tabs/__tests__/CodexModelContextRulesSection.test.tsx src/components/app/__tests__/AppStartupStatusBanner.test.tsx src/pages/__tests__/CliManagerPage.test.tsx`
- 合同测试若 bindings 断言受影响：`pnpm test:unit src/generated/__tests__/bindings.contract.test.ts`

以上命令只覆盖本次路径。实现中新增的测试名可以替换对应过滤器，但不运行完整仓库套件。

## Risk

- `managed.rs` 的 prepare/apply 同时被启动、代理和规则保存使用。强制重读必须是升级意图的显式参数，不能改变 `Background` 的指纹短路。
- 禁用与目录写入必须在同一 Profile 生命周期锁里。先改设置再异步对账会产生「规则已停用、目录仍是旧基础」的窗口。
- Windows bundled 启动继续把 `.cmd` / `.bat` 和固定参数分开传给 `Command`，不能为了强制重读改回拼接命令行。
- 用户目录夹具必须继续绑定绝对测试目录，不能让开发机上的真实 Codex 进入成功路径。

## Rollback Point

命令注册前的改动只存在于目录计划内部。若事务补偿无法恢复，停止扩展 UI，先修 `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` 路径。

## Before Start

- 本计划批准后才运行 `task.py start`。
- 开始改代码前读取 `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md` 和 `settings-ownership-rollback-contract.md`。
