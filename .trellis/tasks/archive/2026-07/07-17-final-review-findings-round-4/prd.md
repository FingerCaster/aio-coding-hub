# 关闭第四轮终审 findings

## Goal

在不访问任何 remote、不改变用户方案 A common gate、不引入 `DeniedByCircuit` 提前移除、
不回归父任务原四项修复与固定 upstream merge 行为的前提下，关闭 Max round-4 的 3 项 P1、
6 项 P2，并对相同漏洞类别做系统搜索和生产路径回归。全程由唯一主会话严格串行执行；子任务在
工作提交、完整门禁和 journal 完成后单独归档，父任务始终保持 `active/in_progress`。

## Requirements

### R1. History read identity

- 修复 `history.rs` 中 canonical path 验证后又按名称打开的身份漂移。
- Unix 必须从已验证父目录句柄相对 no-follow 打开并复核 `dev+ino`；Windows 必须相对已验证父句柄
  no-follow 打开并复核 volume serial + FileId。读取必须消费同一个已验证 file handle，禁止路径式 reopen。
- 为同名 hardlink 与 path swap 增加确定性 barrier；攻击失败时 root 外部字节不得被读取。

### R2. History persist escape

- 修复 task dir 验证后仍以 `dir.join(name)` + `create_new` 写 image/thumb/ref 的目录重绑定逃逸。
- 所有生成资产必须经已验证 task-dir handle 相对 `O_CREAT|O_EXCL|O_NOFOLLOW` 或 Windows
  handle-relative `FILE_CREATE` 创建，并复核目录/文件身份；无法证明安全时跨平台 fail closed。
- Unix/Windows 确定性 barrier 覆盖 symlink/junction 重绑定；失败时外部目录无变化、SQLite 无残行。

### R3. Skill export privacy TOCTOU

- 修复 `config_migrate/skill_fs.rs` 和 `shared/fs.rs` 中 `file_type`/canonical containment 与
  `metadata`/`read(path)` 分离，以及普通目录 canonicalize 后缺少重新 containment 的问题。
- 导出必须以根目录句柄递归、逐级相对 no-follow 打开；普通目录与文件均从同一已验证 handle 获取
  metadata/identity/内容，并实施明确硬链接策略。目录、文件、junction、hardlink 无法证明属于 root
  时 fail closed，不得导出 root 外字节。
- 覆盖文件替换、目录替换及 Windows junction/hardlink 的确定性生产路径测试。
- 本 finding 仅修复 authority/root 越界 TOCTOU。合法 Skill 根内任意内容（包括看似敏感的任意字节）
  必须逐字节导出并可完整 round-trip；不得增加敏感词扫描、内容过滤、自动剔除或内容拦截。现有导出
  提示保持为唯一内容风险提示。

### R4. Settings stale-snapshot writers

- 系统搜索所有生产 `settings::write`，至少覆盖 `settings/persistence.rs`、`grok_config.rs`、
  `cli_proxy/grok.rs` 与 `settings_service.rs` 指出的锁外读、整份写路径。
- 每个字段 owner 必须在 settings 持久化锁内对最新快照执行 read-modify-validate-write，只提交自己拥有
  的字段；不得以锁外旧快照覆盖 Image Gen root 或其他并发提交。
- rollback 仅恢复本 writer 拥有字段，并使用版本/CAS 或等价比较，若字段已被并发更新则不得覆盖。
- 确定性测试必须让至少一侧经过真实生产 writer，覆盖提交与 rollback 交错，禁止两侧只调用人工 update。

### R5. Hydration budget before IPC

- 修复 `imageGenPersistence.ts` 在四个 read 已返回完整 Base64 后才检查 4 MiB/32 MiB 限制的问题。
- 在后端读取、Base64 编码及 IPC 分配之前，基于可信文件 metadata 预留 per-image 与 aggregate budget；
  优先提供后端批量/受预算 hydrate 命令，并保持有限并发。
- 超限后不得启动额外文件读取；测试必须经过真实生产命令/adapter，证明 cap 和并发边界不是 mock 死代码。

### R6. Generation error body cap and redaction

- Rust transport 根据 HTTP status 分流：失败响应使用 8 KiB body cap，不得沿用 32 MiB 成功正文上限。
- 在 IPC 前将 JSON/multipart/其他失败统一转换为无秘密、最多 512 字符的安全摘要；不得持久化原始
  upstream body。TypeScript adapter/controller 再执行防御性截断与脱敏。
- 覆盖 JSON、multipart、超限与 `SYNTHETIC_SECRET`；任务错误、持久化记录和 console 均不得泄漏。

### R7. Corrupt history log privacy

- `imageGenPersistence.ts` 捕获历史 JSON parse 失败时只记录固定错误分类和安全 row id，禁止把
  parser error、raw JSON 或可能包含原文的异常对象传入 `console.warn`。
- 捕获真实 console 参数的回归必须证明 `SYNTHETIC_SECRET` 不存在。

### R8. OAuth flowId capability logging

- 将 `flowId`/`flow_id` 视为唯一 256-bit bearer capability；poll/cancel 失败日志不得原样记录 args。
- 在 generated IPC 层和通用 sanitizer 层均识别并移除/遮蔽这些键；系统检查其他 capability/token-like
  字段，保证持久化 console 无 bearer capability。
- 覆盖 poll 与 cancel 真实生产调用失败日志的双层脱敏测试。

### R9. JSONL archive integrity

- 将 round-3 `implement.jsonl:3`、`check.jsonl:3` 从 active 路径修正为实际 archive 路径。
- 归档操作必须自动重写任务自身 JSONL 中指向本任务 active 目录的路径，随后自动执行等价于
  `task.py validate --all` 的引用校验；或者在归档前拒绝会制造悬空引用的操作。
- 增加归档机制测试，防止未来重复；不得只手工修改两行。最终 `task.py validate --all` 必须通过。

## Acceptance Criteria

- [x] AC1 history read 使用同一已验证 handle；Unix/Windows 同名 hardlink/path-swap barrier 均 fail closed，外部字节不可读。
- [x] AC2 history persist 的 image/thumb/ref 全部 handle-relative 独占创建；目录重绑定失败时外部无变化、DB 无残行。
- [x] AC3 Skill export 根句柄递归与同句柄读取覆盖文件/目录替换及 Windows junction/hardlink；root 内
      任意字节（含 synthetic sensitive-looking bytes）逐字节 round-trip，root 外字节在任何替换竞态下不可进入 bundle。
- [x] AC4 所有生产 settings writer 均为锁内 field-owned RMW；CAS rollback 不覆盖并发更新，真实 writer barrier 通过。
- [x] AC5 hydration 在后端 read/Base64/IPC 前预留可信预算，满足 4 MiB/32 MiB、有限并发和超限后零额外 read。
- [x] AC6 失败 body 使用 8 KiB cap，IPC/TS 摘要最多 512 字符，JSON/multipart/超限路径的任务与 console 无 secret。
- [x] AC7 corrupt history 仅记录固定分类和安全 row id，console 捕获中无 parser/raw JSON/secret。
- [x] AC8 OAuth poll/cancel 的 `flowId`/`flow_id` 在 IPC 与通用 sanitizer 两层脱敏，其他 capability 字段完成系统审计。
- [x] AC9 round-3 JSONL 指向 archive；归档机制自动重写/校验并有测试，`task.py validate --all` 通过。
- [x] AC10 方案 A common gate、无 `DeniedByCircuit` 提前移除、原四项修复和固定 upstream merge 行为均不回归。
- [x] AC11 生成 bindings 后二次生成零漂移；Windows feature/条件编译检查通过；本机无可用 Unix Rust target/WSL，未联网安装。
- [x] AC12 聚焦测试、全 Rust lib/integration、全部前端、bindings/typecheck/lint/format/build、`git diff --check`、
      `task.py validate --all`、`pnpm check:precommit:full`、`pnpm check:prepush`、all-target clippy 全部通过。
- [x] AC13 完成 Phase 3.4 工作提交；仅归档 round-4，journal 记录工作提交 hash；父任务保持 `in_progress`，
      `upstream` push URL 保持 `DISABLED`，且全程无 fetch/pull/push/gh。

## Constraints And Out Of Scope

- 严格串行、唯一执行终端；禁止子代理、其他终端与任何 remote 访问。
- 安全文件系统操作必须句柄绑定并跨 Windows/Unix fail closed；复用现有 NT/openat/identity helper，
  不接受 canonicalize 后路径式 reopen 或概率 race 测试。
- 不合并 main、不归档父任务、不请求开始/提交/归档确认；主会话准备合并 main 时才由用户确认。
- 不联网安装 Unix target、WSL、依赖或工具。
- 不对 Skill 内容做敏感词扫描、敏感内容检测、自动删除、过滤、脱敏或阻断；不改变现有导出提示。
