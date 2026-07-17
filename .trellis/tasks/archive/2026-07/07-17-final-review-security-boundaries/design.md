# 设计：最终审核安全边界

## 已验证现状

- installed Skill import/rollback 目前在 `trim`/去重后直接将 `skill_key` join 到 staging；发现成立。
- Skill writer 虽有单文件和总量预检，但 source metadata 在普通文件之后写，路径冲突与专用文件
  预算没有形成完整的全输入写前事务；发现成立。
- Image Gen 下载已有逐 hop client 与部分 IP 拒绝，但判定未覆盖全部全球可路由边界；发现成立。
- `tasks_list` 当前不接收 settings root，前端把返回路径传入 `convertFileSrc`；DB 可影响 renderer
  文件能力，发现成立。
- multipart 当前逐项 decode 后累计 decoded 字节，缺少全条目 decode 前预算；发现成立。
- NewAPI 仅有 status body cap 聚焦回归，其他 URL/redirect/端点 cap 契约缺证据；需补生产边界与测试。

## 边界与数据流

```text
Config bundle -> validate all keys/files/metadata/budgets/conflicts -> stage -> atomic activation

Remote URL -> sanitize diagnostics -> validate hostname -> resolve all IPs -> global check
           -> pin -> no-redirect request -> bounded response

settings root -> canonical authority
SQLite row -> validate id/direct child/filenames/containment -> backend read projection -> renderer

multipart IPC -> validate all metadata/count/encoded+decoded budgets -> decode all -> build form -> send

Grok response -> bounded bytes -> typed validated payload -> clamped poll state machine
NewAPI URL -> normalize/reject credentials -> no-redirect bounded endpoint reads -> all-or-nothing parse
```

## 设计决策

1. 在 config migration filesystem 模块建立单一组件校验和完整 payload validator，import 与 rollback
   只消费已验证值。路径集合以组件序列检测重复和任意祖先/后代冲突，输入顺序不影响结果。
2. Skill payload 采用 validate/materialize 两阶段：第一阶段解析 Base64、检查专用文件预算并序列化
   metadata 到内存；第二阶段才创建目录和写文件。orchestration 在 staging 创建前验证全部 Skill。
3. Image Gen IP 判定集中在一个 `is_global` 语义 helper；IP literal 在 DNS 前无条件拒绝，hostname
   的全部解析结果必须通过，IPv4-mapped 地址按内嵌 IPv4 判断。
4. 历史列表服务必须取得当前 settings root，domain list 对每一行严格验证后仅返回 renderer 可用的
   后端读取标识或后端生成内容 URL。前端停止对 DB 路径调用 `convertFileSrc`，复用现有
   `image_gen_read_image` 能力完成安全读取。
5. Image Gen 不授予 Tauri asset-protocol filesystem scope。Tauri 2 的 `forbid_directory` 优先于
   后续 allow，不能为 root 往返切换提供可逆事务；因此历史图统一经后端 opaque reference 读取，
   启动与 root 切换均不扩大 scope，旧 root 始终无 renderer 文件权限。
6. URL 日志使用专用安全投影（scheme/host/path 或固定 hop 标识，移除 query/fragment/userinfo），
   网络错误按阶段与状态归一化。网关持久化前按 HTTP 状态清除认证错误正文，内存分类仍可读取
   有界正文。
7. multipart 预检用 Base64 长度推导 decoded 上界和 checked aggregate；为可测副作用把 validate、
   decode、send 分层。元数据长度使用固定常量并与 IPC 字符串预算共同受测。
8. Grok 复用/提取 bounded response helper；响应先校验 content/JSON object/字段，再进入 interval
   clamp 与取消感知轮询。NewAPI 三端点共用有界 reader，但每次显式传对应 cap。

## 兼容性

- 不更改 config bundle schema、Base64 表示、8 MiB 资产预算或 v1/v2 语义。
- 若 Image Gen IPC 返回形状必须调整，同步 Rust 类型、生成 bindings、service adapter、UI 与测试；
  不暴露新的任意路径读取接口。
- 不修改 sub2api、failover 分类、provider routing 或账户展示模型。
- Windows 使用 `Path::components` 与平台条件测试；目录 symlink/reparse 验证以 canonical/final target
  containment 为准，不依赖 Unix-only 假设。

## 失败与回滚

- 每个安全域单独落地并运行聚焦测试；失败时回到该域开始前的工作树快照，绝不回退用户已有提交。
- config migration 在 staging 激活前失败时清理本次 staging，旧 SSOT/local 状态保持不变。
- storage root 切换失败保持旧 settings；由于 Image Gen 从不授予 asset scope，新旧 root 均不会
  因失败路径获得 renderer 文件权限。
- schema/IPC 若无法保持兼容，则停止该方案并改用现有后端 read projection，不以扩大 asset scope 解决。
- 全量门槛失败时修复后重跑对应聚焦测试，再重新执行受影响的后续全量门槛。
