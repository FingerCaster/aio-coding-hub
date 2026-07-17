# Design: 第三轮终审关闭

## Boundaries

本轮保持现有 Rust command -> app service -> domain/infra -> SQLite/filesystem 与 React service
-> controller -> view 分层。所有网络、路径、设置和 OAuth 权限均由后端绑定状态决定；前端只持有
opaque reference 与惰性显示状态。

## Decisions

### IPv6 转换分类

增加纯函数从已知转换格式提取 `Ipv4Addr`，统一交给 `is_global_ipv4`。mapped/compatible/NAT64
使用末 32 位；6to4 使用第 17-48 位。已知但不能证明安全的转换范围显式拒绝，再处理纯 IPv6。

### Device flow capability

把 `ACTIVE_FLOW` 扩展为可选 device-flow binding。start 只有在远端响应全部验证且 deadline
checked 后才登记绑定；poll 先取得不可变快照，校验兼容字段，再按绑定 CLI/codes 请求。最终 token
持久化仍在持有 lifecycle mutex 的 `complete_current_flow` 内重验全部 ownership。

### Filesystem authority

根和其祖先逐组件用 `symlink_metadata`/Windows reparse attributes no-follow 校验，并记录稳定
filesystem identity。read 从已验证 file handle 读取；破坏性操作通过受信 root 目录句柄把任务原子
rename 到唯一 quarantine，核对原 task identity 后再做 handle-relative 递归删除，不在复验后重新按
原路径打开。批量删除在第一项删除前完成全量验证及 identity capture。

### Unique atomic temp files

shared atomic writer 在目标同目录循环生成随机后缀，以 `OpenOptions::create_new(true)` 独占创建，
写完 flush 后交给既有原子 replace。guard 只删除本次创建的临时文件，任何 payload 名均可合法存在。

### Atomic settings mutation

settings persistence 抽出持锁内部 write，并暴露 `update`：锁内读取最新快照、执行 closure、校验与写入。
Image Gen root 切换使用该 API。普通 settings 最终提交通过同一 API，仅覆盖它拥有的字段并保留最新
Image Gen 字段，避免锁外 candidate 覆盖并发 root mutation。

### Persist transaction

文件仍先写入独占目录；SQLite transaction 包住 INSERT 与用同一 transaction 查询的最终 row/path
验证。验证成功才 commit；失败自动 rollback DB，并由目录 ownership guard 清理本次目录。

### Lazy history hydration

disk image 状态区分 thumbnail URL 与尚未加载的 full URL。hydrate 只以小并发读取每项首屏所需
缩略图，并受聚合 decoded-byte 预算限制；打开详情/预览时按需填充 full image，下载与设为参考仍从
opaque path 读原字节。参考图不在首屏读取，在详情/复用时按需加载。

### Secret-free endpoint diagnostics

path 先映射到固定 endpoint enum；非法值固定返回 `SEC_INVALID_INPUT: image gen endpoint is not
allowed`。日志只使用 enum，不序列化原 IPC args；transport 错误继续走安全类别。

### Task artifact integrity

修正归档 JSONL 后，在 Trellis task validation 或独立仓库测试中遍历 active/archive 的 JSONL，解析每个
非 `_example` `file` 并断言 repo-relative path 存在。

## Product Decision A

用户已明确选择 A。保持 `resolve_session_bound_provider_id` 只决定 preference，不从 eligible candidates
移除临时被 circuit/cooldown 拒绝的 bound provider；后续 common gate 是 deny 与 skipped attempt/route
的唯一 owner。继续其他候选，skipped 不占 Ready budget；全拒绝时统一 503 带完整诊断。无需改生产
行为，但必须新增或保留跨层回归，防止未来重新引入 `DeniedByCircuit` 分支。

## Rollback

九项非冲突改动按聚焦测试串行推进但保持一个任务。任何阶段失败只修复当前边界，不弱化前序安全
断言。全部实现与门禁完成前保持工作树和任务 active，不提交、不归档。
