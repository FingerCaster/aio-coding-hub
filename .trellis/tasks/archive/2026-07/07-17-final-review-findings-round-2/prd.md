# 修复第二轮最终审核发现

## Goal

在不改变既有产品承诺和远程状态的前提下，关闭 max 只读终审确认的 F1-F8：Image Gen
落盘/SSRF/MIME/存储切换/分页边界、Skill bundle 路径冲突、Device OAuth 过期与退避，及父子
任务验收证据链。所有安全边界 fail closed，子任务独立提交归档，父任务保持 `in_progress`。

## Confirmed Facts

- `history.rs` 当前先 `create_dir_all` 再 `std::fs::write`，目标文件的 symlink/hardlink 会在
  写入后才被读回校验发现；失败路径可能先覆盖外部 inode。
- Skill 导入用大小写敏感 `HashSet<PathBuf>`，系统 marker 未加入 payload 冲突图，且只有精确
  `SKILL.md` 受 256 KiB 限制。
- IPv6 分类仅特殊处理 IPv4-mapped，`::/96` IPv4-compatible 保留范围未显式拒绝。
- Device OAuth 接受 0/极大 `expires_in`；token 非正过期会降级为 `None` 后持久化；
  `slow_down` 与 `authorization_pending` 同态。
- Image Gen 历史 DB 已存任务目录绝对路径，但所有读取/清理仍用当前设置 root 验证；切换
  新目录时先用新 root 扫描旧行，故存在历史就失败。
- multipart 预检只检查 MIME 非空/长度，语义解析发生在最多 64 MiB Base64 解码之后。
- 归档 NewAPI 子任务缺 post-fix live 证据；merge commit `9e5da346` 的 Git 对象可重建为
  30 个冲突文件、47 个冲突标记组，与材料中的“31 个文本冲突”无法一一对应。
- Image Gen SQL 排序为 `(created_at DESC,id DESC)`，过滤却仅为 `created_at < cursor`。
- 完整代码与历史证据见 `research/confirmed-facts-and-contracts.md`。

## Requirements

### R1. 原子且不可跟随的 Image Gen 落盘

- 新任务目录必须独占创建；既有目录、symlink、junction/reparse point 或预置目标一律拒绝。
- 每个目标文件使用 `create_new` 写入；任何文件或 DB 失败均清理本次新目录，且不得删除或
  改写任务开始前已存在的对象。
- symlink/hardlink/预置目标负例按平台能力 gate，必须断言外部字节不变且无 DB 行。

### R2. Skill bundle 统一冲突图

- payload 路径与系统生成的 managed/source marker 使用同一规范化冲突图。
- Windows 目标语义按 Unicode/ASCII 稳定大小写规范化；其他平台仍明确保持大小写敏感。
- `SKILL.md` 的大小写别名都适用 256 KiB 专用预算，且同址/父子冲突顺序无关地拒绝。
- marker 冲突必须在任何写入前拒绝，不能由写后删除/覆盖掩盖。

### R3. IPv6 fail-closed 分类

- 显式拒绝 `::/96` IPv4-compatible/保留转换范围；IPv4-mapped 继续按映射 IPv4 分类。
- DNS 返回的每一个地址都必须全球可路由；真正全球 IPv6 继续允许。

### R4. Device OAuth 有界过期与 RFC 8628 退避

- device code 与 token `expires_in` 使用有界 `Result` 校验；0、负数、溢出和不合理大值拒绝。
- 无效 token 过期不得调用 token 持久化，也不得完成 flow。
- poll DTO 显式表达 `slow_down`；前端后续轮询间隔在当前值基础上至少增加 5 秒，并保持
  增长后的间隔直到完成/取消/过期。

### R5. 历史跨存储根存续

- 更改当前 root 成功后，新任务写入新 root；旧任务仍可 list/read/delete/clear/cleanup。
- 每行历史必须绑定其创建时的受信存储根；旧 DB 行可确定性迁移，迁移不移动文件。
- 外部设置变化不能重解释历史 root；路径仍须验证为 `root/<task-id>` 且拒绝链接逃逸。
- 设置写失败不得改变持久化设置；目录探针和历史统计失败不得产生半迁移状态。

### R6. multipart 先验 MIME 校验

- 第一次 Base64 decode 前验证所有可判定字段，包括完整 MIME 语义。
- 非法 MIME 必须零 decode 副作用；正常 multipart 行为不变。

### R7. 证据链闭合

- 使用本机现有 `muyuan` 配置执行 post-fix 最小只读验证；仅记录脱敏类型/布尔公式断言、
  UTC/本地时间和结果。凭据不可用时精确记录已穷尽的安全路径与 blocker，绝不伪造。
- 对 merge `9e5da346` 只读重建冲突，形成逐文件/逐行为决策表，分别标记保留 fork、采用
  upstream、融合及证据来源；不 fetch、不重新 merge、不杜撰用户决定。
- 父 PRD/design/implement/task map 更新为真实 7 个子任务与顺序；父任务不归档。
- 任务校验与 secret scan 通过，证据文件不得含 token、host、query、账号或实际金额。

### R8. 稳定分页

- 后端签发版本化 opaque cursor，内部绑定 `(created_at,id)`；SQL 使用严格字典序 seek。
- 命令、generated bindings、service 与前端 session 状态统一使用 cursor 字符串。
- `null` 表示首屏；非法/未知版本/旧 numeric 游标明确 fail closed，不静默漏项。
- 同毫秒超过页大小时，多页无重复无遗漏；前端追加使用最后一行 cursor。

## Acceptance Criteria

- [x] AC1 F1 的 symlink/hardlink/预置目录与写/DB 失败回滚测试通过，外部字节不变、无残行。
- [x] AC2 F2 的 marker、case alias、顺序无关、SKILL.md 专用预算测试通过。
- [x] AC3 F3 的 IPv6 表驱动与 DNS 多结果回归通过。
- [x] AC4 F4 的过期边界、无持久化副作用和 `5s` 累进退避序列通过。
- [x] AC5 F5 的旧 DB 兼容、带历史切换、新旧 root 读写/删除/清理及路径逃逸测试通过。
- [x] AC6 F6 的非法 MIME + 大 Base64 零 decode 回归和正常 multipart 回归通过。
- [x] AC7 F7 的脱敏 live 证据、冲突决策表、父任务映射及 secret/task validation 完整。
- [x] AC8 F8 的同时间戳 51+ 行、多页、非法游标及前端追加测试通过。
- [x] 聚焦 Rust/Vitest、binding 生成/一致性检查和完整仓库门禁全部通过。
- [x] 子任务提交并归档、journal 已记录、父任务仍 `in_progress`、工作树干净且无远程操作。

## Validation Commands

- `cargo test --manifest-path src-tauri/Cargo.toml image_gen --lib --locked`
- `cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked`
- `cargo test --manifest-path src-tauri/Cargo.toml oauth --lib --locked`
- `pnpm exec vitest run src/pages/image-gen/__tests__/useImageGenController.test.tsx src/pages/providers/__tests__/providerEditorOAuthActions.test.ts src/services/image-gen/__tests__/service.test.ts`
- `pnpm generate:bindings && pnpm check:generated-bindings`
- `pnpm build`, `pnpm check:precommit:full`, `pnpm check:prepush`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`
- 全部 `src-tauri/tests/*` integration suites（按仓库脚本/locked Cargo 执行）
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`
- `git diff --check`、Trellis validate、secret scan、工作树/remote/Orca 终端只读核验。

## Out of Scope

- fetch/pull/push/gh、远程修改、重新 merge upstream、合并 main 或修改 upstream push URL。
- 恢复已删除的 reasoning guard/continuation repair 或改变账户展示的 display-only 边界。
- 归档父任务或自行开始最终 max 审核。
