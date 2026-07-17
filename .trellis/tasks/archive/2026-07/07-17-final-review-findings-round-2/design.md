# Design: 第二轮终审修复

## Architecture

本轮保持现有 Tauri command -> app service -> domain/infra -> SQLite/filesystem 与 React service
-> controller 分层。安全预检都在分配大内存、写磁盘或写 DB 之前完成。

## Decisions

### F1 + F5: settings-owned trusted storage roots

- `AppSettings v52` 增加 `image_gen_storage_roots: Vec<String>`。切换时把旧 current root、新 root
  与已有历史 root 规范化去重后和 current root 一次写入 settings；SQLite `dir` 始终只是候选。
- 历史操作要求行目录匹配 allowlist 中某个 canonical `root/id`，当前 setting 只决定新写入。
- 新目录用 `create_dir` 独占创建，文件用 `OpenOptions::create_new(true)`；失败只删除本轮成功
  创建的目录。旧任务 ID/目录存在即拒绝，避免覆盖与回滚误删。
- stats 的 `dir` 仍返回当前设置目录，但 `total_bytes/task_count` 覆盖所有受信历史行。

### F2: normalized path graph

- 预检先将 payload 与两个 marker 组成节点；节点同时保留原始 `PathBuf` 与比较 key。
- Windows 下每个 normal component 做稳定 lowercase 后比较，非 Windows 使用原组件；比较
  duplicate 和 ancestor/descendant，结果与输入顺序无关。
- `SKILL.md` 判断使用相同平台语义的单组件 key，保证 Windows case alias 进入专用预算。

### F3 + F6: pure validation before effects

- IPv6 classifier 在 mapped IPv4 分支后显式拒绝 `::/96`，再走全球 IPv6 表。
- multipart 预检用 `reqwest::multipart::Part::bytes(Vec::new()).mime_str(...)` 复用实际 MIME
  parser；全部文件通过后才 decode。

### F4: explicit backoff DTO

- `ProviderOAuthDeviceCodePollResult` 增加 `slow_down: bool`。Grok poll 内部返回 Pending/
  SlowDown/Complete 三态；Codex pending 返回 false。
- 前端维护 `pollIntervalMs`，每次 slow_down 用 checked/capped 方式增加至少 5000ms，之后等待
  新间隔。device/token 过期分别设明确上限并用 checked arithmetic 生成 deadline/epoch。

### F8: opaque keyset cursor

- cursor 编码为版本化 URL-safe Base64 JSON（仅后端解析/签发），字段 `v/created_at/id`。
- SQL 条件为 `created_at < ? OR (created_at = ? AND id < ?)`，排序保持 DESC/DESC。
- list 响应改为 `{items,next_cursor}`，前端 session 保存 `nextCursor`；不从任务集合重新推导。

### F7: evidence artifacts

- `research/muyuan-post-fix-readonly-validation.md` 只保存允许字段；执行脚本/命令不打印秘密。
- `research/upstream-merge-conflict-decision-audit.md` 以 commit parents、merge-base、remerge diff
  和最终 tree 为证据，按文件/行为分组列全冲突文件；对材料数量差异明确标注。
- 父任务三份工件和 task map 同步到 7 个 children；归档 NewAPI/upstream implement 补证据链接，
  不改写历史事实。

## Compatibility And Rollback

- v52 settings migration 初始化空 allowlist；现有 DB 不变，首次切换从切换前 current root 受控
  登记旧根，不移动/删除图片。
- opaque cursor 不接受旧 numeric cursor，调用方同一提交升级；错误显式返回。
- 任何设置写失败保留旧 setting；任何新任务持久化失败删除新目录且不留下 DB 行。
- 所有更改作为一个安全回归提交可整体 revert；父任务保持 active。
