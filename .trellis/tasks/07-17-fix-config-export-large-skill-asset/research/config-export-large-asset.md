# 配置导出大 Skill 资源根因研究

## 证据分类

### 用户观察

另一台 Windows 机器在“设置 -> 导出配置”时，整个导出因以下错误失败：

```text
SEC_INVALID_INPUT: file C:\Users\admin\.codex\skills\keep-codex-fast\assets\keep-codex-fast-cover.png too large (max 1048576 bytes)
```

这证明现场文件超过 1 MiB，但单凭错误不能证明它超过 8 MiB 单 Skill 总预算。

### 已验证事实

1. `src-tauri/src/infra/config_migrate/mod.rs:20-26` 定义：
   - `CONFIG_IMPORT_FILE_MAX_BYTES = 64 * 1024 * 1024`
   - `CONFIG_SKILL_FILE_MAX_BYTES = 1024 * 1024`
   - `CONFIG_SKILL_TOTAL_MAX_BYTES = 8 * 1024 * 1024`
   - `CONFIG_SKILL_FILE_COUNT_MAX = 256`
   - 512 字符相对路径、64 KiB source metadata、256 KiB `SKILL.md` 边界
2. `src-tauri/src/infra/config_migrate/skill_fs.rs:97-124` 的
   `SkillFileCollector::push_file` 先按 1 MiB 调用 `read_file_with_max_len`，随后才累计
   8 MiB 总量。单文件超限会立即返回错误，调用方不会生成部分 bundle。
3. `skill_fs.rs:128-217` 递归遍历 Skill；符号链接逃逸和特殊文件均明确拒绝。
4. `skill_fs.rs:242-276` 校验相对路径组件、UTF-8 和长度。
5. `skill_fs.rs:317-373` 在导入写盘前检查文件数、重复路径、派生 Base64 长度、解码后
   单文件大小和 8 MiB 累计总量。
6. `src-tauri/src/commands/config_migrate.rs:20-33` 在 JSON 解析前以 64 MiB 限制导入文件。
7. `src-tauri/src/infra/config_migrate/tests.rs:640-706` 已覆盖符号链接逃逸、旧 1 MiB
   单文件拒绝、文件数与超长 Base64，但没有覆盖 1–8 MiB 必要资源的 round trip。
8. `git show`/`git blame` 将这组边界追溯到安全加固提交 `13bba9b0`。限制是通用原始
   字节读取边界，不是 PNG 解码器或 Windows 路径处理错误。

### 仍需实现阶段动态验证

- 使用合成字节确认 `>1 MiB && <=8 MiB` 的嵌套 PNG 在导出/导入后逐字节相同。
- 确认恰好 8 MiB 被接受、8 MiB 加 1 被双向拒绝。
- 重跑全部路径、文件类型、总量和 64 MiB bundle 安全负例。

这些是回归证明，不是尚未确认的根因假设。

## 可证伪根因结论

配置导出递归收集所有 Skill 文件；1 MiB 单文件读取上限早于 8 MiB 单 Skill 累计预算
生效，因此一个大于 1 MiB 的必要资源会让整个导出失败。现场错误与该精确路径一致。

该结论在以下任一情况出现时被证伪：

- 基线代码能在不跳过文件的前提下成功导出 `1 MiB + 1` 字节资源；
- 错误发生在到达 `SkillFileCollector::push_file` 之前；
- 现场文件实际触发的是 8 MiB 累计、路径或特殊文件错误而非所记录的 1 MiB 错误。

现有代码与错误字符串均不支持这些反例。

## 最小修复边界与安全策略

- 仅把 config migration 的 Skill 单文件上限对齐到现有 8 MiB 单 Skill总预算。
- 导出读取、Base64 预检和解码后校验保持同一来源、双向对称。
- 1–8 MiB 资源完整携带；不静默跳过 PNG，因为跳过会生成语义不完整的 Skill。
- 保留 8 MiB 单 Skill 总量、256 文件、64 MiB 导入文件、路径、符号链接、特殊文件、
  专用 metadata/`SKILL.md` 和写前验证边界。
- 不修改通用 Skill 安装、复制或 WSL 同步的独立限制。

提高单文件上限不会提高单 Skill 原始载荷总预算；它只允许现有 8 MiB 预算由一个必要
资源使用，而不是强制拆成多个小文件。

## 回归矩阵

| 场景 | 预期 |
| --- | --- |
| 普通小文本/PNG | 行为不变，可 round trip |
| 嵌套 PNG，`1 MiB + 1` | 完整导出/导入，字节一致 |
| 单文件恰好 8 MiB，Skill 无其他载荷 | 可 round trip |
| 单文件 8 MiB 加 1 | 导出和导入均拒绝 |
| 两个文件各自合法、合计 8 MiB 加 1 | 累计预算拒绝 |
| 256/257 个文件 | 256 可继续校验，257 拒绝 |
| Base64 文本超派生上限或解码后超限 | 写盘前拒绝 |
| 重复、遍历、绝对、超长或非 UTF-8 路径 | 拒绝 |
| 符号链接逃逸、循环或特殊文件 | 既有防护保持 |
| 导入文件 64 MiB 加 1 | JSON 解析前拒绝 |
| installed/local Skill 与 v1/v2 bundle | 兼容行为不变 |

## 风险与回滚点

- Base64 会产生约 4/3 的瞬时表示开销，但 8 MiB 原始单 Skill预算和 64 MiB 导入文件
  上限不变；边界测试需避免不必要的并行大分配。
- 最大风险是只放宽一侧造成导出/导入不对称，因此常量与双向测试必须同一提交。
- 无 schema 或持久化迁移。若发现内存或兼容回归，可整体回退该常量/测试提交，恢复
  原 1 MiB 行为。
