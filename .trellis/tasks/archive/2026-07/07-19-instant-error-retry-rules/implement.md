# 规则化瞬时错误重试 - 实施计划

## Ordered Checklist

1. **建立 Rust 规则模型与兼容反序列化**
   - 在设置类型中新增 `UpstreamHttpRetryRule`，把 `status_codes` 替换为 `http_rules`。
   - 保留旧 `status_codes` 只读反序列化兼容；序列化只写新字段。
   - 分离严格写入规范化与加载修复，增加数量、字符、控制字符及“至少一个有效匹配器”校验；无效写入不得被 repair sanitizer 静默放宽。
   - 将设置 schema 升至 53，补旧 JSON -> canonical 新 JSON 回写测试。

2. **迁移 Provider 持久化和分享格式**
   - 新增 SQLite v38 -> v39 事务迁移，转换 `providers.upstream_retry_policy_json`。
   - 保持查询层旧格式兼容和 malformed JSON 显式禁用行为；新写入只含 `http_rules`。
   - Provider 分享新增严格 v2 导出/解析；保留严格 v1 读取并转换。
   - 覆盖 v1/v2 确定性序列化、未知字段拒绝、完整重试策略 round-trip 和禁用 Provider 导入。

3. **实现 HTTP 规则匹配与单次 body scan**
   - 在现有 retry-policy helper 中实现 code-only、内容 OR、多规则 OR、大小写无关普通子串和安全诊断结果。
   - 在 `upstream_error` 中先匹配 code-only，再把内容规则扫描并入现有 body read；不得二次读取。增加错误扫描专用 gzip 前缀解码，超限时不得把压缩字节当文本匹配。
   - 保留通用重试、attempt budget、严格辅助路由、退避和熔断计数语义。
   - `reason` 只追加规则序号/描述；增加 401/403 无正文泄漏回归。

4. **更新生成绑定、前端适配与共享 UI**
   - 重新生成 TypeScript bindings，并更新默认策略、clone/normalize、设置校验和所有夹具。
   - 将固定 502/503/504 checkbox 区域替换为共享规则列表；传输错误区域保持独立。
   - 支持新增、编辑、启停、删除以及一行一个匹配内容；全局和 Provider 覆盖复用同一组件。
   - 更新全局保存、Provider 提交、MSW 和服务映射测试。

5. **补齐跨层测试与规范**
   - Rust matcher：code-only、空/非空 body、大小写、多个内容/规则、禁用、错误码边界、字面正则字符、读取失败与非 UTF-8。
   - 路由/错误处理：内容命中重试、内容不命中仍走基线决策、预算耗尽、熔断计数、gzip/超大 body、401/403、count-tokens/model-discovery strict path。
   - 迁移：设置 schema 52、Provider DB v38、Provider share v1 到新 canonical 格式；malformed 数据 fail closed。
   - 前端：默认三规则、增删启停、内容行解析、校验、全局保存、Provider 整套覆盖/继承/显式禁用。
   - 更新 gateway attempt、Provider share 相关 spec 中的可执行契约与版本字段。

## Validation Commands

```powershell
pnpm tauri:gen-types
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm test:unit src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx src/pages/providers/__tests__/ProviderEditorDialog.test.tsx src/services/settings/__tests__/settingsValidation.test.ts src/services/providers/__tests__/providers.service.test.ts src/services/providers/__tests__/providers.msw.test.ts
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:test
git diff --check
```

若全量 Rust 测试在本机再次遇到已知 Windows loader `STATUS_ENTRYPOINT_NOT_FOUND`，仍需完成 `cargo test --no-run`/`pnpm tauri:check`，记录受阻命令，并确保所有可执行的定向与前端测试通过；不能把 loader 失败表述为测试通过。

## Risky Files And Rollback Points

- `src-tauri/src/infra/settings/types.rs`、`migration.rs`、`persistence.rs`：生成绑定和全局设置兼容的共同源头。先完成并测试再进入 UI。
- `src-tauri/src/infra/db/migrations/mod.rs` 与新 v39 migration：启动阻断风险最高；必须事务化并先用 v38 夹具验证。
- `src-tauri/src/domain/providers/share.rs`：严格 v1/v2、secret 边界和 8 MiB 确定性序列化不可回归。
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs`：响应体只能消费一次；任何失败都应先回滚这一层，而不是放宽 body/隐私限制。
- `src/components/gateway/RetryPolicyFields.tsx`：同时被全局与 Provider 表单复用，需同步验证两条保存路径。

## Pre-start Review Gates

- [x] 用户确认 `prd.md`、`design.md` 和 `implement.md`。
- [x] `implement.jsonl` 与 `check.jsonl` 均包含真实 spec/research 条目。
- [x] PRD 已完成无重复、无临时段落、无已解决问题的收敛检查。
- [x] 任务保持 `planning`；评审通过前不运行 `task.py start`，不修改产品代码。
