# 删除 Codex Provider 转译实施计划

## 进入门槛

- [ ] 用户审阅并批准本子任务的 `prd.md`、`design.md`、`implement.md`。
- [ ] 只启动 `08-08-remove-codex-provider-translation` 子任务，不启动父任务或模型路由子任务。
- [ ] 运行 `trellis-before-dev`，读取 backend、frontend、cross-layer、数据库迁移、Provider 分享和配置导入相关规格。
- [ ] 记录 `git rev-parse HEAD`、SQLite/settings 当前版本和 `git status --short`；若 HEAD 低于 `8757d32c` 或版本 44 已占用，先暂停并更新制品。
- [ ] 保存所有既有 dirty paths 清单，尤其关注 Provider、gateway、配置迁移和生成绑定重叠文件；不得覆盖其他会话改动。

## 1. 固定兼容分类和迁移边界

- [ ] 建立仅供迁移/导入使用的 legacy Codex bridge 分类器和稳定错误分类，活动领域不导出这些常量。
- [ ] 增加 SQLite 43->44 幂等迁移及新安装 baseline，同步 schema 版本测试。
- [ ] 在删除前预检 managed profile -> provider_model -> Provider 引用；异常引用导致完整事务失败。
- [ ] 删除三类 legacy bridge Provider，验证既有 FK 和必要的显式活动引用清理，不触碰 `request_logs`。
- [ ] 将存活 Provider 的 `model_mapping_json` 归一为空对象；覆盖重复迁移和部分旧 schema 形态。

## 2. 删除后端活动契约

- [ ] 从 Provider upsert/summary/query/复制和生成绑定移除 Codex `ModelMapping` 与三类 bridge 的创建、更新和校验分支。
- [ ] 保留通用 `source_provider_id`/`bridge_type` 的 CX2CC 和插件所有权，收紧内建 bridge 校验为仅支持 `cx2cc`。
- [ ] 删除 Codex bridge availability probe、模型目录和用量统计分支及对应夹具。
- [ ] 更新所有完整 Provider fixture，并确认账户用量字段、credentials 和 route gate projection 未被删改。

## 3. 删除网关转译运行时

- [ ] 从 protocol bridge registry 删除三个 Codex factory、模型映射器和只为其服务的请求转换模块。
- [ ] 删除非 CX2CC 的 bridge preparation 分支；只保留经调用证据确认的 CX2CC/插件共享抽象。
- [ ] 删除 Codex bridge 非流式/流式响应转换和专属 response cache/状态字段。
- [ ] 清理死模块、imports、测试夹具和 feature 分支，确认普通 Codex Responses/compact 以及 Claude CX2CC 仍走原路径。
- [ ] 运行 gateway 回归，确认删除工作没有进入账户 gate、`blocked_provider_ids`、健康、熔断、session 或 transport ownership。

## 4. 收紧配置交换

- [ ] 在 `prepare_config_import` 清空当前配置之前原子拒绝含 legacy bridge 的完整配置 v1-v4，并测试当前配置保持不变。
- [ ] 将 bundle 兼容字段 `model_mapping_json` 规范化为空，不在活动导出/导入恢复其语义。
- [ ] 保留 Provider 分享 v1/v2 的严格解析壳：legacy bridge 拒绝，legacy mapping 丢弃，普通/CX2CC 兼容行为不变。
- [ ] 更新复制、分享预览、完整导出导入和失败回滚测试，保证任何入口都不能重建已删除 bridge。

## 5. 删除桌面端入口并生成绑定

- [ ] 删除 `CodexBridgeSection` 及 Codex bridge form state、submit/effect/action/view/card 分支。
- [ ] 将 CX2CC UI 限定为 Claude，验证 Codex 编辑器只保留普通 Provider 能力。
- [ ] 更新 Provider 编辑器、卡片、服务适配器及配置导入/分享前端测试。
- [ ] 从 Rust 重新生成并格式化 `src/generated/bindings.ts`，逐项审计差异，保留账户用量及其他会话字段。

## 6. 联合验收与规格

- [ ] 运行迁移、Provider、config import/share、protocol bridge、gateway route 和前端 focused tests。
- [ ] 运行全量类型、lint、Rust 格式/check/clippy/test 与生成绑定检查。
- [ ] 搜索三类 legacy bridge 和 `CodexBridgeSection`；除迁移、旧格式兼容和断言性负向测试外不得残留活动引用。
- [ ] 执行 `trellis-check`，修复问题后更新相关规格并提交、归档本子任务。
- [ ] 子任务验收完成后再请求启动 `08-08-configurable-model-routing-core`，不得在同一未验收阶段混入路由实现。

## 验证命令

```powershell
rg -n "codex_to_openai_chat|codex_to_openai_responses|codex_to_anthropic_messages|CodexBridgeSection|ModelMapping" src src-tauri
cargo test --manifest-path src-tauri/Cargo.toml migrations --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml providers --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml protocol_bridge --lib --locked
pnpm test:unit
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm tauri:test
git diff --check
```

focused Vitest 参数以实现时实际测试文件为准；先用测试列表确认过滤器确实命中，不能用空过滤结果冒充通过。

## 审查门槛

- [ ] 删除迁移不复制来源 URL/凭据，不删除历史日志、普通 Codex、来源 Provider 或 CX2CC。
- [ ] 任何旧配置入口都不能在当前配置已清空后才发现 legacy bridge。
- [ ] 活动 Rust/TypeScript 契约没有 Codex 转译或模型映射能力，历史格式壳没有运行时解释器。
- [ ] 普通 Codex、CX2CC、插件 bridge 和账户用量路由回归全部通过。
- [ ] 回滚说明明确：数据库成功迁移后只能通过备份恢复旧 bridge，代码降级不受支持。
