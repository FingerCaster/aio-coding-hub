# 实施计划：兼容账户额度多种查询方式

## 进入门槛

- [ ] 用户审阅并批准 `prd.md`、`design.md`、`implement.md` 后运行 `task.py start`。
- [ ] 执行代理读取账户用量、供应商分享和完整配置迁移规格。
- [ ] 记录并保留工作区既有改动，尤其是 `src/generated/bindings.ts` 及供应商分享相关文件。

## 1. 数据库与凭据领域

- [ ] 增加 schema migration、baseline 和迁移测试，创建 `provider_account_usage_credentials`。
- [ ] 将有效历史 `newApiUserId` 搬到私有表并从内置扩展 JSON 移除；覆盖无效值和幂等执行。
- [ ] 在账户用量领域增加 User ID/令牌规范化、长度限制、摘要投影、私有读取和事务补丁函数；秘密承载类型不得派生 `Debug`。
- [ ] 建立内置账户用量扩展的共享后端净化函数，并接入保存、迁移、完整导出导入及分享导出导入，保证历史 User ID/账户令牌不能回流 `values_json`。
- [ ] 扩展 `ProviderSummary`、`ProviderUpsertInput` 和内部参数，落实 preserve/set/clear 语义。
- [ ] 更新创建、编辑、删除和本机复制路径；复制必须在后端事务边界内携带合成账户凭据。

## 2. NewAPI 协议

- [ ] 增加 `billing/account` 显式模式解析，旧配置默认 `billing`。
- [ ] 重构 NewAPI 部署根和同源 URL 构造，覆盖 root 与尾部 `/v1`。
- [ ] billing 解析精确识别官方无限额度哨兵并返回“可用/模型令牌无限额度”的无金额结果；保留有限额度、负余额、日期窗口和错误顺序。
- [ ] 实现 status + user/self 账户查询：禁止重定向、分别认证、限制响应体、验证用户身份、单位和换算基数。
- [ ] 将账户 quota/used_quota 映射为 balance/历史 used，不伪造 total；所有失败保持全有或全无。
- [ ] 调整 IPC 分支，使每种模式只加载和发送自己的凭据。

## 3. sub2api 日额度

- [ ] 在共享解析器中识别唯一 `window == "1d"` 项。
- [ ] 校验 limit/used/remaining、窗口时间和内部一致性，映射 daily used/total。
- [ ] 未知窗口保持兼容，畸形或重复已知窗口失败关闭。
- [ ] 保持现有根级 balance、remaining、planRemaining、subscription 和过期语义测试通过。

## 4. 编辑器、适配器与展示

- [ ] 更新账户用量配置类型、默认值、扩展合并和旧值兼容测试。
- [ ] 增加 NewAPI 查询方式选择、User ID、遮蔽令牌、可见性图标、已配置状态和显式清除草稿；任何模式/适配器切换都不提交清除补丁。
- [ ] 允许账户模式凭据不完整地保存，并在编辑器/卡片直接显示“需配置账户凭据”。
- [ ] provider upsert 适配器深度脱敏账户令牌，保存后清空秘密草稿。
- [ ] 账户模式显示“历史已用”，sub2api 显示日 used/total；确认窄屏文本不溢出。
- [ ] 保持自动/定时/手动刷新同一 query owner 及编辑/删除缓存清理行为。

## 5. 完整配置、分享与复制

- [ ] 将完整配置 bundle 提升到 v3，加入可选账户用量凭据快照；分别保留 v1 旧行为、v2 完整 Skill 载荷和 v3 凭据往返语义。
- [ ] 完整导出/导入原子往返合成模式、User ID、令牌和刷新设置；错误与日志不得包含秘密。
- [ ] 单供应商 share v1 导出和导入规范化均剔除账户 User ID/令牌，同时保留显式账户模式。
- [ ] 分享导入仍为禁用且不写路由/模板，导入后显示账户凭据缺失。
- [ ] 覆盖本机复制完整凭据、完整备份包含、单供应商分享排除三种不同策略。

## 6. 绑定与规格

- [ ] 从 Rust 重新生成并格式化 `src/generated/bindings.ts`，审计并保留来源支持的既有用户改动。
- [ ] 更新前端服务类型映射和所有完整 `ProviderSummary` / upsert fixture。
- [ ] 完成代码检查后更新账户用量和供应商分享规格，并补充完整配置凭据往返约束。

## 测试矩阵

- [ ] Rust：迁移、凭据补丁、摘要不含秘密、复制与删除级联。
- [ ] Rust：NewAPI billing 有限/无限、账户模式成功、缺凭据、身份不匹配、认证失败、未知单位、非法数值、重定向、响应上限和部分失败。
- [ ] Rust：sub2api `1d` 成功、未知窗口、重复窗口、负数、非有限、算术不一致、时间无效及旧夹具。
- [ ] Rust：完整配置 v1/v2/v3 能力矩阵、账户快照往返、v2 Skill 载荷不回归、分享净化、禁用导入、无路由写入和失败回滚。
- [ ] 前端：配置规范化、模式切换不清除、令牌 preserve/replace/clear、允许不完整保存、日志脱敏和界面状态。
- [ ] 前端：账户余额/历史已用、sub2api 日额度、配置缺失、刷新 loading/error 和缓存竞态回归。

## 验证命令

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml providers --lib --locked`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml migrations --lib --locked`
- [ ] 运行账户配置、编辑器、服务适配器、查询缓存、展示和分享相关 focused Vitest。
- [ ] `pnpm check:generated-bindings`
- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `pnpm tauri:fmt`
- [ ] `pnpm tauri:check`
- [ ] `pnpm tauri:clippy`
- [ ] 风险范围要求时运行完整 `pnpm test:unit` 与 `pnpm tauri:test`。
- [ ] `git diff --check`

## 最终验收与敏感信息审计

- [ ] 审查所有差异和测试输出，不出现真实令牌、User ID、主机、响应体、个人字段或账户金额。
- [ ] 确认分享/摘要/日志排除秘密，完整备份与本机复制按批准策略包含合成秘密。
- [ ] 确认账户用量没有调用或修改路由、熔断、可用性、启停、排序或 OAuth 配额。
- [ ] 合成测试全部通过后，仅在不记录凭据和响应内容的前提下复核已授权的只读现场契约；输出只保留状态、字段类型和一致性布尔值。
- [ ] 质量检查通过后才进入规格更新、提交和归档流程。
