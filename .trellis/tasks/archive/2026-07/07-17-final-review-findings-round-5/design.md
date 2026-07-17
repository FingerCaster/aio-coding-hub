# Design: 第五轮终审闭环

## 1. Skill handle-bound authority

可信边界从“路径校验后再解析”改为“可信父目录句柄 -> 相对枚举 -> no-follow 打开 -> identity
校验 -> 同一子目录句柄读取”。SSOT 数据库行中的 `skill_key` 只作为可信父句柄下的单段名称；
local Skill 名称来自父句柄枚举。两者都不允许 `canonicalize` 结果成为新根。

一个捕获的 Skill 视图拥有：顶层目录 handle、枚举 identity、从该 handle 相对打开的文件
handles/bytes。`SKILL.md` metadata 与 source metadata 从已捕获 bytes 解析，文件 bundle 也从同一
目录 handle 遍历。identity 不一致、reparse point/symlink、特殊文件或替换均明确失败。Unix 使用
dirfd/no-follow 语义；Windows 使用拒绝 reparse point 的目录 handle 与文件 identity 校验。

安全边界只判断对象类型、身份、相对路径和既有资源预算，不检查文件内容。合法文件 bytes
原样进入 bundle，导入后逐字节一致。

## 2. Settings commit and rollback protocol

config import 顺序调整为：准备 candidate 与可回滚资源 -> settings CAS -> 成功后 reconcile
autostart -> runtime sync -> 完成。CAS 失败前不产生 autostart 副作用。CAS 后失败时，只有
`compare_and_swap(committed, previous)` 成功才恢复 previous autostart/runtime；若 CAS 被 winner
击败，则从最新 canonical settings 同步或保持 winner runtime，绝不盲目恢复旧值。

settings service 的 owned rollback 返回显式结果：`Restored`、`ConcurrentOwnerWon` 或 error。
仅 `Restored` 允许调用 previous runtime 恢复；其余分支读取 canonical winner 并同步，若同步
本身失败则保留当前 runtime 并报告组合错误，不制造旧 snapshot 的越权写入。

测试通过仅测试构建启用的 barrier/failpoint 放在真实 CAS、runtime sync 与 rollback 边界，调用
真实 import/service API 形成确定性交错，而不是只测 helper。

## 3. Common gate before ready cap

failover loop 对每个候选先执行 `prepare_provider/run_gates`。`Skipped` 总是写 attempt/route 后
continue，不消费 ready budget。`Ready` 才检查 `providers_tried >= cap`，随后消费额度并发起请求。
这样 cap 限制真实上游 provider 尝试数，同时 authoritative gate 对后续候选仍可观察。保留
`DeniedByCircuit` 到 gate 记录路径，不改变最终状态分类。

## 4. Structured OAuth sanitization

在 `generatedIpc.ts` 建立值级 sanitizer：递归复制 arrays/plain objects，按大小写及命名风格
归一后的敏感键替换值；Error 对象提取安全字段后处理；字符串执行现有及补强后的文本脱敏。
所有 IPC 错误日志先 sanitize value，再 stringify/format。循环引用与不可序列化值采用安全占位，
不能把原对象 fallback 成可能泄密的字符串。

poll/cancel 生产 wrapper 的测试捕获 logger，使用每次随机生成且不含 `SYNTHETIC_SECRET` 等 marker
的 capability，验证原值在任意日志参数的深度序列化结果中均不存在。

## 5. Grok production router regression

使用真实 router/handler 测试夹具启动受控上游：记录两次请求 body，第一次返回精确的
`previous_response_id` 缺失错误，第二次返回带 usage、response-id 的成功响应。断言请求总数为
2、第二次字段缺失、最终映射正确。测试沿用生产 body reader/timing 路径，并补边界断言以防
rectifier 绕过首字节计时或放宽 20 MiB 非 SSE 限制。

## 6. Evidence and executable specs

父任务只修正文档事实，`task.json.status` 保持 `in_progress`。相关契约更新 Skill bundle、settings
ownership rollback、gateway failover route、OAuth device flow，以及必要的 backend attempt budget/
continuation 条款。cross-layer 每次编辑同步模板镜像并用字节比较验证。

## Compatibility And Rollback

- 不改变合法 Skill bundle 格式、用户提示和资源上限。
- 不改变 settings 字段所有权；只收紧副作用时序与失败恢复授权。
- 不改变 ready provider cap 数值和 circuit 判定，只修正 gate/预算顺序与证据。
- 不改变 OAuth API DTO；仅改变错误日志表示。
- 不新增 Grok 产品行为；生产回归锁定已存在的内部 retry。
- 产品代码与工件作为一个工作提交；归档和 journal 分别形成后续可审计提交。
