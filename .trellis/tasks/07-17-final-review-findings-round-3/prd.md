# 关闭第三轮终审 findings

## Goal

在不访问 remote、不弱化安全边界的前提下，关闭
第三轮 max 只读终审的 3 项 P1、5 项 P2、2 项 P3。九项非冲突 finding 先严格串行完成；
网关冲突已由用户明确选择 A：保留当前 common-gate 产品语义。子任务和父任务均保持
`in_progress`，在全部实现与门禁通过前不提交、不归档。

## Requirements

### R1. IPv4 转换 IPv6 SSRF

- 按标准转换格式识别 IPv4-mapped、IPv4-compatible、NAT64 `64:ff9b::/96` 与 6to4
  `2002::/16` 的内嵌 IPv4，并复用唯一 IPv4 global 分类器。
- 无法可靠分类的转换前缀 fail closed；纯全球 IPv6 继续允许。
- DNS 任一 private/loopback 内嵌地址即拒绝，混合 A/AAAA 不得挑选其余 public 地址继续。

### R2. Device OAuth flow 所有权

- 活动 device flow 绑定 flow id、provider id、CLI/provider type、device/user codes 与 deadline。
- poll 的网络请求与 token 落库只使用服务端绑定状态；调用者除 flow id 外的旧字段若为兼容保留，
  必须与绑定值恒等，否则在请求/持久化前拒绝。
- 跨 provider、跨 CLI、改码、过期、取消、替换或任何失败均不得写 token。

### R3. 历史 storage root 不可重绑定

- 受信 root 与任务/文件路径逐组件拒绝 symlink、junction/reparse point；不允许 canonicalize
  动态跟随父组件后重新解释 allowlist 身份。
- read/delete/clear/cleanup 在验证与使用之间保持可证明的 root/目录身份边界；攻击失败时外部
  字节和 DB 行不变。
- Unix symlink 与 Windows junction/reparse 负例按平台 gate 覆盖。

### R4. 网关产品决策 A

- 保留当前 `provider_selection.rs` common-gate 行为，不引入 upstream `DeniedByCircuit` 提前
  `retain` 移除语义。
- session-bound provider 被 circuit/cooldown 拒绝时仍作为 eligible candidate 进入统一 gate，
  产生 skipped attempt/route、零 upstream call，且继续检查其他候选。
- 所有候选最终不可用时返回 503，并保留完整 skipped attempt/route 诊断；skipped 不消耗 Ready
  provider budget，session binding 不因临时 gate denial 被清除。

### R5. Skill 原子写临时名

- 共享 atomic writer 使用 `create_new` 的随机/唯一临时文件，或等价独立 staging 命名空间；
  不占用任何合法 payload 目标名，失败只清理自己创建的临时对象。
- 覆盖 `.aio-coding-hub.source.json.aio-tmp`、`a.aio-tmp`/`a` 两种顺序、marker 冲突和完整
  round-trip 字节。

### R6. settings 原子更新

- settings 层提供同一锁内 read-modify-validate-write API；storage root 切换和普通设置保存
  不得用锁外旧快照覆盖对方字段。
- barrier 测试覆盖双切换及切换与普通保存的两种交错。

### R7. persist 数据库回滚

- INSERT 与最终读取/路径验证处于同一事务或等价原子结构；最终验证失败不得留下 DB 行。
- 失败只清理本次独占创建的目录；覆盖 before-insert 与 post-insert validation failure。

### R8. 历史首屏内存上限

- hydrate 优先只读取缩略图；有缩略图时不读原图，参考原图和显示/下载原图按需读取。
- 历史解码使用显式并发上限与聚合字节边界，不能对 50 项无界 `Promise.all`。
- 卡片、详情、预览、下载、复用参考图及缺失文件交互保持完整。

### R9. Image Gen secret-free 错误与日志

- 非法 path 返回固定错误，不回显 query/fragment；前端 IPC 日志只记录端点枚举和脱敏错误。
- JSON/multipart 与异常路径使用 synthetic secret 证明错误、console、日志均不含秘密。

### R10. 归档 JSONL 引用完整性

- 前五个原始子任务的 10 个 JSONL research 引用改为真实 archive 路径。
- 增加所有任务 JSONL `file` 引用存在性校验，防止归档后悬空。

## Acceptance Criteria

- [x] AC1 NAT64/6to4/private/loopback/public、纯 AAAA、混合 A/AAAA SSRF 回归通过。
- [x] AC2 Device flow 跨 provider/CLI、改码、过期、失败不落 token，成功只写绑定 provider。
- [x] AC3 Unix/Windows 路径重绑定对 read/delete/clear/cleanup 均 fail closed，外部字节与 DB 行不变。
- [x] AC4 决策 A 回归证明 session-bound circuit/cooldown denial 由 common gate 记录 skipped，
      继续其他候选，最终 503 保留完整诊断，且不存在 `DeniedByCircuit` 提前移除。
- [x] AC5 随机 `create_new` 临时名不与合法 payload/marker 冲突，round trip 字节一致。
- [x] AC6 settings 双切换及普通保存交错不丢失任何一方字段。
- [x] AC7 persist 的 before/post-insert 失败均无残行，仅清理本次目录。
- [x] AC8 hydrate 不读有缩略图的原图，按需读取有效，并发与聚合字节上限有确定性测试。
- [x] AC9 JSON/multipart path 错误和前后端日志不含 synthetic secret。
- [x] AC10 10 条历史 JSONL 已修正，仓库所有 JSONL file 引用存在性检查通过。
- [x] 聚焦测试、bindings/typecheck/diff-check、`pnpm build`、`pnpm check:precommit:full`、
      `pnpm check:prepush`、Rust lib/integration/all-target clippy 全部通过。
- [x] 无 remote 操作、无 key/PII/余额泄露，`upstream` push URL 仍为 `DISABLED`。

## Out Of Scope

- fetch/pull/push/gh、访问或修改任何 remote、修改 main、提交或归档任务。
- 引入 upstream `DeniedByCircuit` 提前移除语义或重新定义既有 attempt/route/503 产品契约。
- 拆分或并发执行新的子任务/代理。
