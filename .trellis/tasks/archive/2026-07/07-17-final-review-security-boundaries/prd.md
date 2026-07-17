# 最终审核安全边界修复

## Goal

修复父任务最终审核确认的八类安全边界缺口，使配置迁移、Image Gen、网关日志、
Grok device OAuth 与 NewAPI 用量查询在恶意路径、网络地址、持久化元数据、超限输入、
远端错误和状态切换下均 fail closed，同时保持现有兼容性与正常流程。

## Requirements

### R1. 配置迁移目录键与写前原子验证

- installed Skill 的 `skill_key` 必须由 import 与 rollback 共用同一校验器，且严格只含一个
  `Component::Normal`；拒绝空值、`.`、`..`、路径分隔符、绝对/root/prefix、Windows 盘符与 UNC。
- 全部 key、相对路径、重复项、祖先/后代冲突、Base64/解码预算、`SKILL.md` 256 KiB、
  source metadata 64 KiB、metadata 完整性与序列化结果必须在任何目标/staging 目录创建、普通
  文件写入或 local 注册前验证完毕。
- export/import/restore 对专用预算保持对称；超限解析错误不得降级为通用 8 MiB 文件读取。
- 保持每 Skill 256 文件、8 MiB decoded 总量、8 MiB 单资源完整 round trip，以及 schema v1/v2
  和 installed/local 行为。

### R2. Image Gen 网络边界

- 每一 hop 拒绝全部 IP literal，包括公网 literal。
- hostname 的每个 DNS 结果必须是全球可路由地址；拒绝 IPv4/IPv6 private、loopback、
  link-local、unspecified、broadcast、multicast、CGNAT、benchmark、documentation/reserved，
  以及 IPv4-mapped 非全球地址。
- 保持 HTTPS/443/无凭据、逐 hop 禁自动重定向、重定向预算与 DNS pinning。

### R3. Image Gen 历史元数据和 asset scope

- 列表和读取以当前 settings-derived canonical storage root 为唯一权威；SQLite 的 `dir`、
  `images_json`、`ref_images_json` 全部视为不可信。
- task id、root 直接子目录、非 symlink/reparse、stored filename 单组件与 canonical containment
  必须完整验证；任一元数据篡改使整行/请求失败，不得静默跳过或生成 renderer 可消费路径。
- renderer 不得通过 `convertFileSrc` 绕过后端引用校验；历史图像只能经后端已验证的安全投影/
  读取能力消费。
- Image Gen 不授予 Tauri asset-protocol filesystem scope；storage root 切换后旧 root 保持无权，
  失败切换不得扩大新旧 root 权限，正常历史统一经后端 opaque reference 安全读取。

### R4. URL、错误与网关日志隐私

- Image Gen IPC、console、tracing、对外错误不得包含 URL query/fragment 的签名或 token；
  reqwest 错误必须归一化为不含完整 URL/凭据的安全诊断。
- 网关至少对 401/403 不持久化上游正文；其他状态的正文预览继续服从既有有界与 privacy
  契约，且不能破坏仅在内存中完成的 failover 分类。
- 所有秘密回归使用 `SYNTHETIC_SECRET`，并断言 console、request log 与返回错误均无泄露。

### R5. Image Gen multipart 解码前预算

- 在任何 Base64 decode 或大分配前，一次性验证文件数、field/filename/mime 长度、每项派生
  Base64 上限与 checked aggregate 上限。
- decoded 总量保持 64 MiB；超长字符串、过多文件、聚合溢出或元数据超限均不得触发 decode
  或 HTTP 发送副作用。

### R6. Grok device OAuth 有界状态机

- device/token 响应均通过 bounded reader 读取，不得使用无界 `text`/`json`。
- 校验响应类型和必要字段非空；远端 interval 合理 clamp，safety margin 用 checked/saturating
  算术，`u64::MAX` 不得 panic 或回绕。
- pending、slow/poll、expired、denied、取消、超限和成功流程保持明确状态与取消所有权；错误
  不包含远端正文或 token。

### R7. NewAPI fail-closed 回归

- credentialed/invalid Base URL 在发请求前拒绝；真实 HTTP 3xx 不跟随，Bearer 不转发。
- status、subscription、usage 分别执行自己的响应 body cap；任一端点失败均返回完整查询失败，
  不产生部分 snapshot。
- 不改变 sub2api 解析和网络行为；测试仅使用 reserved host 与 synthetic values。

### R8. 规范、兼容与仓库约束

- 新稳定边界补入最相关现有 cross-layer spec，不降低已有契约。
- Rust/IPC/bindings/TypeScript/UI 如有形状变化必须同步并验证生成文件。
- 保留用户已有工作树、提交与父任务 `in_progress`；不 fetch、push、调用 `gh`、修改 remote、
  操作 upstream 或合并 main。
- 全程严格串行，不启动子代理或第二个 Orca 终端。

## Acceptance Criteria

- [x] traversal、绝对路径、Windows drive/UNC、分隔符和祖先/后代冲突在 import/rollback 写入前被
      共用校验拒绝，staging 外无文件/目录、目标无残留且状态未部分激活。
- [x] `SKILL.md` 与 source metadata 专用预算在 export/import/restore 对称生效，metadata 在普通
      文件前完成序列化与校验，8 MiB 资产及 v1/v2 正常回归通过。
- [x] Image Gen 拒绝全部 IP literal 及要求列出的非全球 DNS 范围，同时保留逐 hop no-redirect
      和 DNS pinning 测试。
- [x] DB tamper 无法经 list/render/read 获得文件能力；正常历史可用，Image Gen 零 asset scope
      保证 root 切换前后旧根均无 renderer 文件权限，失败切换不扩大权限。
- [x] `SYNTHETIC_SECRET` 不出现在 Image Gen console/IPC 错误、request log 或网关认证错误正文中，
      同时保留状态码、错误类别等安全诊断。
- [x] multipart 所有条目在首次 decode 前通过数量、元数据、派生 Base64 与 checked aggregate
      预算；超限时零 decode/零发送副作用，64 MiB 边界通过。
- [x] Grok device/token body 有界，字段/类型/interval 安全，pending/expired/denied/cancel/success
      回归通过且不泄露 token/正文。
- [x] NewAPI invalid/credentialed URL 零请求、真实 3xx 不转发 Bearer，三个端点各自 body cap 与
      all-or-nothing 回归通过，sub2api 测试保持不变。
- [x] 所有聚焦 Rust/TS 测试、生成 bindings 检查、build、完整 precommit/prepush、独立全量 Rust、
      Clippy、`git diff --check` 与 `task.py validate` 全部通过。
- [ ] 代码和任务材料已提交，仅本子任务已归档；父任务仍为 `in_progress`，最终无 push/merge。

## Out of Scope

- 修改供应商路由、账户用量展示语义、sub2api 协议或现有 failover 决策。
- 为通过测试而放宽任何现有安全预算或跳过生产修复。
- 访问 remote/upstream、发布、推送或合并 main。
