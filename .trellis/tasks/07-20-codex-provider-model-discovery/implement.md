# Codex 供应商模型发现与受管 Profile 实施计划

## 执行原则

- 所有实现只在 `codex-provider-model-discovery` worktree/branch 完成，不修改 `main` 工作区。
- 先建立持久化和身份合同，再接网络发现、profile 文件和网关；前端最后接生成类型。
- 每阶段先跑聚焦测试。共享 provider/gateway/config-migration 代码完成后必须跑完整 Rust suite。
- 发现大范围既有缺陷时记录为 out-of-scope，不在本任务顺手重构。

## 1. Schema 与稳定身份

- [x] 新增共享 canonical UUIDv4 生成/校验 helper及单元测试。
- [x] 新增 v39 -> v40 migration：`providers.provider_uuid`、catalog/models/profiles 三表、唯一索引、FK 与触发保护。
- [x] 更新 fresh baseline 和 migration 完整性/幂等测试。
- [x] provider 新建路径生成 UUID；编辑保持 UUID；复制和 provider-share 导入得到新 UUID。
- [x] `ProviderSummary` 只增加必要的小型只读身份字段；不加入模型数组。
- [x] provider 删除前检查 profile 引用；无引用时事务清理 model/catalog。

验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml infra::db::migrations --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml domain::providers --lib --locked
```

## 2. 配置 bundle v4

- [x] `ConfigBundle`/`ProviderExport` 升 v4并加入 provider/source UUID，保留 v1-v3 feature threshold。
- [x] v4 prepare 阶段严格校验 canonical/duplicate UUID 和 source 引用。
- [x] 导入前检查本机受管 profile 可重绑定集合；legacy + local profile 或 v4 缺失 provider 时失败关闭。
- [x] 同事务保留相同 provider UUID 的本机 model/profile，清理已移除且无 profile 的本机 catalog/models，导入后标 stale。
- [x] 添加 v1-v4、重复/非法 UUID、同机重绑定和破坏前失败回归。

验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
```

## 3. Provider 模型领域与发现传输

- [x] 新建 provider model DTO/query：catalog get、刷新成功事务、刷新失败状态、手工 upsert/delete、alias resolve。
- [x] provider 连接字段变化时只标记 discovered 数据 stale，普通名称/备注编辑不误标。
- [x] 抽取/复用 URL join、effective credential 和 bounded body 能力，实现 no-redirect OpenAI-compatible `/v1/models` adapter。
- [x] 实现 provider-scoped refresh service 和四个 Tauri 命令，注册 Specta。
- [x] 覆盖多 Base URL、同名跨 provider、完整响应验证、资源上限、错误分类、历史/手工保留和凭据脱敏。

验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml provider_models --lib --locked
```

## 4. Codex managed profile

- [x] 扩展 `codex_paths` 解析当前 Codex home，但只由后端构造 profile 文件路径。
- [x] 新建 profile 元数据 query、文件状态 projection、create/delete service 和进程锁。
- [x] 使用 `toml_edit` 生成顶层 `model` / `model_provider`，实现 no-clobber 原子创建和 hash 所有权补偿。
- [x] 删除 modified 文件时保留文件并解除管理；未知同名文件创建失败。
- [x] 注册 list/create/delete IPC 和生成类型。
- [x] 覆盖临时 Codex home、非法名称、同名/大小写冲突、DB/FS 失败、missing/modified 文件。

验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_managed_profile --lib --locked
```

## 5. Gateway alias、固定路由和检测语义

- [x] 增加 `ManagedModelRoute` context 和 ModelInference 后的精确 alias resolve/rewrite middleware。
- [x] alias provider 作为唯一候选加载，绕过普通排序/会话偏好但保留公共 gate和同 provider retry；处理 forced-provider 冲突。
- [x] 记录 provider-scoped `aio_managed_model_route`，保持 `request_logs.requested_model` 为 canonical alias。
- [x] 让 attempt 保存最终 wire model；统一四类响应路径的 wire-vs-observed 比较和 `unobserved/conflict` 规则。
- [x] 成本解析按最终 provider 选择 managed priced model，失败 attempt 不参与。
- [x] 前端 special-setting resolver增加中性类型，不增加 alias 字符串豁免。
- [x] 添加正常、伪造、禁用、无 failover、retry、四响应路径、mismatch/unobserved/conflict、日志和成本集成测试。

验证：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml gateway --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml request_logs --lib --locked
```

## 6. 前端 service/query/UI

- [x] 生成 Rust -> TypeScript bindings，添加 provider models/profile adapter 边界校验。
- [x] 新增 provider-scoped query keys、catalog query、refresh/manual/profile mutations及精确 invalidation。
- [x] Provider editor 增加“保存并获取模型”两阶段动作，保证 refresh 失败不回滚已保存 provider。
- [x] 新增模型目录对话框：刷新状态、手工回退、stale 行、profile 创建/删除；只对 Codex 直连 provider 显示入口。
- [x] 请求日志中性显示 `aio_managed_model_route`，严重样式仍只属于真实 mismatch。
- [x] 添加 service/query/component 测试，覆盖晚到请求、provider 隔离、同名模型和文件 preserved 提示。

验证：

```powershell
pnpm tauri:gen-types
pnpm exec vitest run src/services/providers src/query/__tests__/providers.test.tsx src/pages/providers
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
```

## 7. 集成与最终门禁

- [x] 审计 diff，不泄露凭据/上游 body/完整 URL，不改变普通非 alias 路由。
- [x] `cargo fmt`、Clippy、完整 Rust library tests、前端 unit/typecheck/lint、生成绑定、diff check。
- [x] 手工启动桌面开发服务，验证新增 provider -> 获取模型 -> 创建 profile -> 通过 AIO 请求的主流程。
- [x] 更新相关 Trellis spec，记录 stable identity、managed alias 和 route-detection 合同。
- [x] 在 worktree 分支提交；不操作 `main`，不推送 upstream。

## 8. Codex 模型选择器集成（用户反馈修订）

- [x] 新增当前 Codex bundled 完整目录的有界读取，并支持用户已有 `model_catalog_json` 作为基础。
- [x] 生成带所有权/hash 的完整合并目录，追加 `aio/<profile_name_key>` 并保留基础未知字段。
- [x] CLI proxy 启用、启动同步和关闭恢复正确维护根 `model_catalog_json`；无 Profile 时恢复基础状态。
- [x] Profile 新建改写可读 alias，网关同时解析 Profile alias 与旧 UUID alias。
- [x] 创建/删除 Profile 与目录/config 同锁预检和补偿，外部修改及并发漂移失败关闭。
- [x] 前端展示 picker alias、代理前置条件和“新建/重启 Codex 会话后生效”提示。
- [x] 增加 Rust、前端及真实 Codex app-server 聚焦回归，并重新执行完整质量门禁。

最终命令：

```powershell
pnpm tauri:fmt
pnpm tauri:clippy
pnpm tauri:test
pnpm test:unit
pnpm typecheck
pnpm lint
pnpm check:generated-bindings
git diff --check
```

最近完整验证（2026-07-21）：

- `pnpm tauri:fmt`
- `pnpm tauri:clippy`（`--all-targets --locked -- -D warnings`）
- `pnpm tauri:test`（2311 library tests，附加集成测试均通过；真实 Codex smoke 为显式 ignored 手工用例）
- `pnpm test:unit`（296 files / 2588 tests passed）
- `pnpm typecheck`
- `pnpm lint`
- `pnpm check:generated-bindings`
- `git diff --check`
- `cargo test --manifest-path src-tauri/Cargo.toml bundled_catalog_runs_cmd_wrapper_from_a_path_with_spaces --lib --locked`
- `cargo test --manifest-path src-tauri/Cargo.toml installed_codex_reads_the_generated_picker_alias --lib --locked -- --ignored`

## 首版决策与验证限制

- `refresh_locks` 以稳定 `provider_uuid` 为 key，进程生命周期内保留小型 Map entry。首版优先避免并发刷新串写；entry 回收留作后续优化，不能以牺牲 provider identity 隔离为代价。
- Linux 等价 Rust suite 已在 Docker 中通过，但容器以 root 运行，不能作为普通 Linux 用户遇到 `PermissionDenied` 的独立证明。no-clobber、hash ownership、unsafe Codex home 和失败补偿由跨平台单元/集成测试覆盖；非 root Linux 的真实 profile 文件权限场景仍属于手工验证限制。
- 当前 Codex `0.134.0+` profile 使用 `$CODEX_HOME/<name>.config.toml` 顶层键；不再生成已废弃的 `[profiles.<name>]`。Codex 端始终只有 `model_provider = "aio"`。Profile 文件本身不会进入 `/model`，必须完成第 8 阶段的合并目录同步。
- 本任务保持在独立 worktree/branch；用户已授权验证通过后直接提交，不归档，不直接操作 `main` 或 push。

## 风险与回滚点

- migration/config import、profile 文件与 gateway response observer 是三个高风险点；每完成一段先保持独立可测试提交候选。
- profile 文件写入失败只补偿本次 hash 匹配文件，绝不删除未知或外部修改内容。
- gateway alias 发生异常时可通过停止创建 profile 和删除受管 profile 回到普通模型路由；数据库模型目录本身不影响非 alias 请求。
- 若 bundle v4 回归未通过，不以“暂时不导出 UUID”绕过；必须保持旧 bundle兼容与破坏前失败合同。
