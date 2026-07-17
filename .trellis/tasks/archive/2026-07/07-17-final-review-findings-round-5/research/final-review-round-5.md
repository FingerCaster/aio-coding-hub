# Max Final Review Round 5

## Review Conclusion

第五轮终审判定当前分支仍有三项 P1 与三项 P2，尚不能进入最终通过状态。以下为必须完整
关闭的原始结论及其验收含义。

## P1 Findings

### F1. Skill 顶层可信根不成立

`src-tauri/src/infra/config_migrate/export.rs:502` 对 `ssot_root.join(skill_key)` 使用会跟随链接的
`is_dir`；`skill_fs.rs:79` 随后 `canonicalize` 并把解析结果当作新根。local Skill 在
`export.rs:565` 枚举后仍通过路径读取 metadata。攻击者可利用顶层 symlink/junction 或目录替换，
让导出越出可信 SSOT/CLI 根，或使 metadata 与实际导出内容来自不同对象。

修复必须从可信 SSOT/CLI 父根句柄相对 no-follow 枚举并打开顶层 Skill 子目录，校验 identity，
并从同一捕获 handle/file bytes 解析 `SKILL.md` 与 source metadata。SSOT/local 生产入口均需顶层
symlink/junction/替换竞态测试。合法 Skill 根内任意字节必须逐字节导出和 round-trip；严禁敏感
词扫描、内容过滤、脱敏、自动剔除或阻断，现有提示不变。

### F2. Settings CAS 前副作用与越权 runtime rollback

`src-tauri/src/infra/config_migrate/mod.rs:365` 在 settings CAS 之前修改 autostart；CAS 失败后以
`committed_settings=None` 进入 `rollback.rs:139`，因此不会恢复该副作用。另有
`src-tauri/src/app/settings_service.rs:988` 忽略 owned-fields rollback 是否成功，并在约 1000 行
无条件恢复旧 runtime，可能覆盖并发 winner。

autostart 必须移到成功提交后，或使用可靠可回滚 token。只有 owned rollback 明确成功才能恢复
旧 runtime；失败时保持现状或按当前 canonical winner 重同步。确定性测试必须走真实 import/service
生产路径，覆盖 `SETTINGS_CONCURRENT_UPDATE`、runtime sync failure 与并发 owner winner。

### F3. 方案 A common gate 被 ready cap 截断

`src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs:311` 在 `prepare_provider/run_gates` 前根据
`providers_tried` cap break，导致达到 ready cap 后的 circuit-open/cooldown 候选不经过 authoritative
common gate，也没有 skipped attempt/route，违背用户选定的方案 A。

common gate 必须先执行；skipped 写记录并 continue，只有 `Ready` 才消费/检查上限。补 `cap=2` 且
顺序为 Ready、Ready、circuit-open/cooldown 的生产 router 回归，第三项必须留下 skipped attempt/
route；不得引入 `DeniedByCircuit` 提前移除。

## P2 Findings

### F4. OAuth capability sanitizer 对结构化错误失效

`src/services/generatedIpc.ts` 的 regex 无法可靠处理 JSON/对象错误中的 `flowId`、`flow_id` 与
capability-like keys，现有 `SYNTHETIC_SECRET` marker 又掩盖了缺陷。应在 stringify 前对结构化值按
敏感键递归脱敏，再格式化。测试必须使用不含特殊 marker 的随机 capability，同时覆盖字符串与
对象错误，以及 poll/cancel 真实调用日志。

### F5. Grok continuation 缺生产回归

现有测试只证明 helper，未证明实际 Grok router 的双请求行为。新增生产 router 测试，证明
`previous_response_id` 错误只触发一次重试、第二次请求已移除字段、最终 usage/response-id 正确，
且不回归现行 TTFB 与 20 MiB 边界。不得恢复已删除的 FinalSuperset/reasoning-guard 产品面。

### F6. 父任务证据互相矛盾

父 `prd.md`/`implement.md` 同时存在“仅 1-7 已归档”“round-4 in progress”和 1-9 清单未完成等
陈述，而真实状态是子任务 1-9 已完成。修正文档事实，将 round-5 作为当前修复与下一次 Max
终审前置；父 `task.json` 全程保持 `in_progress`。

## Global Review Constraints

- 搜索同类别生产入口并按最小必要范围修复。
- 更新 executable cross-layer specs，保持 `.trellis/spec/aio-coding-hub/cross-layer` 与
  `src/templates/markdown/spec/cross-layer` 镜像逐字节一致。
- 先聚焦、后完整门禁；bindings 二次生成必须零漂移。
- 本机无 Unix target 时不得联网安装，只记录限制并进行 cfg/API 审计。
- 所有门禁通过后提交工作、仅归档 round-5、记录 journal 并提交；父任务继续 `in_progress`，
  不启动下一次 Max 终审。
