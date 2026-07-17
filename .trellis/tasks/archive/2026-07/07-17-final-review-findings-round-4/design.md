# Design: 第四轮终审关闭

## Security Model

本轮统一采用 capability/handle authority：路径仅用于定位候选，不能成为验证后的 authority。受信根、
task dir、Skill root 都先取得目录句柄；后续遍历、open/create、metadata、identity 与 read/write 全部相对
该句柄完成。任何平台缺少可靠 primitive、identity 变化、reparse/symlink/hardlink 策略不满足或 budget
无法预留时一律 fail closed。

## Filesystem Handle Layer

复用并扩展 `shared/fs.rs` 的平台 helper，形成小而明确的目录 capability API：

- Unix 使用 `openat`/`fstatat`/`O_NOFOLLOW`，记录 `st_dev+st_ino`；文件从验证后的 fd 直接读取，
  创建使用 `O_CREAT|O_EXCL|O_NOFOLLOW` 并由同一父 fd 约束。
- Windows 使用现有 NT/native handle-relative open，目录/文件拒绝 reparse point，记录 volume serial +
  FileId；创建 disposition 为 `FILE_CREATE`，验证和消费同一个 handle。
- 测试 hook 位于“已取得/验证父 capability、尚未相对打开子项”或“已验证 task dir、尚未创建资产”
  的精确边界，以 barrier 确定性替换同名路径。测试不依赖调度概率。

History read 由 DB opaque reference 定位 allowlisted root/task candidate，取得 task handle 后相对打开目标，
复核 identity 并从该 file handle 读。History persist 先独占取得 task-dir handle，所有 image/thumb/ref 都
经它相对创建；SQLite transaction 只有在全部文件与最终 identity 校验成功后提交，guard 只清理本次
拥有对象。

Skill export 从 Skill root handle 深度优先遍历。目录项相对 no-follow 打开后，从 handle 获取 type、大小、
identity 和内容；普通目录也重新证明仍在 root authority 内。硬链接采用 fail-closed 策略：无法证明 link
只属于受信导出树，或 link count/平台 identity 不满足策略时拒绝导出。既有 file-count、single-file、
aggregate、Base64 与 metadata caps 保持不变。authority 判定只回答“字节是否来自受信 Skill root”，
不检查字节表达的内容：通过 root authority 验证的文件必须从同一 handle 逐字节读取并 round-trip，
即使内容看似 credential 或包含测试敏感词也不得过滤、剔除、脱敏或拦截。现有 UI 导出提示保持不变。

## Settings Ownership And CAS

settings persistence 提供唯一锁内 mutation primitive，并暴露版本/field CAS 语义。生产 writer 不再把
锁外 `read()` 得到的整份快照传给 `write()`；每个 owner closure 在锁内读取最新 settings，只更新自己
拥有字段，再 validate/persist。需要外部副作用的 writer 保存自己提交后的 owned-field token；rollback
仅当当前 owned fields 仍等于该 token 时恢复先前值，否则保留并发更新。测试 hook 固定提交/副作用/
rollback 顺序，并让 grok、CLI proxy 或 settings service 的真实生产 writer 参与。

## Budgeted History Hydration

新增后端批量受预算 hydrate command（或等价 app-service API），输入 opaque asset refs 和明确用途，后端
在 open 后、read 前读取同一 handle 的可信 metadata，并用 checked arithmetic 预留 per-image 4 MiB 与
aggregate 32 MiB decoded budget。只有预留成功才读取并 Base64 编码；worker 数固定有界，预算耗尽后
调度器不再启动新 read。返回逐项安全成功/拒绝结果，bindings 生成到前端；前端不再在完整 Base64 IPC
返回后才承担第一道预算。

## Failure Response And Logging

Image Gen transport 在 status 成功/失败处分支。失败 body 的 streaming reader 上限为 8 KiB，超限立即
停止；解析器只提取允许字段，统一生成最多 512 字符、去除 credential/URL query/body fragment 的安全
摘要。IPC 只传安全分类与摘要，TS adapter/controller 再按同一 512 字符上限和 sanitizer 防御。

历史 JSON 解析失败日志不传异常对象，只传固定 category 与经过验证的 row id。generated IPC 日志在
序列化前删除 OAuth poll/cancel args，通用 sanitizer 将 `flowId`、`flow_id` 及系统审计发现的同类
capability key 视为 secrets；持久化 console 只能收到安全类别。

## Trellis Archive Integrity

`task.py archive` 在移动 task dir 后，按 repo-relative 路径结构化解析被归档任务自己的 JSONL，将
`.trellis/tasks/<task>/...` 精确重写为 `.trellis/tasks/archive/<YYYY-MM>/<task>/...`，不做任意字符串
替换。移动/重写完成后运行全 active/archive manifest validator；验证失败则 archive 命令失败并避免自动
提交。单元/回归测试构造真实 task、self-reference 和 archive，断言重写、全量校验及非本任务路径不变。

## Compatibility And Rollback

生成命令/类型变化通过标准 bindings 生成，二次生成必须无 diff；平台模块以 `cfg(unix)`/
`cfg(windows)` 隔离并检查 Cargo feature。方案 A common gate 和历史 round-1 到 round-3 产品行为不动。
每项 finding 完成聚焦测试后才进入下一项；若失败，只回退当前尚未提交的局部设计，不弱化安全断言。

## Explicit Non-Goal: Skill Content Policy

本轮不建立 Skill 内容审查策略，不新增敏感词扫描、secret scanner、内容分类器、自动剔除或内容拦截。
回归必须同时证明两侧：受信 root 内包含 `SYNTHETIC_SECRET` 等看似敏感字节的合法文件按原字节导出/
导入；root 外包含任意字节的文件即使通过确定性文件/目录/junction/hardlink 替换也不能进入 bundle。
