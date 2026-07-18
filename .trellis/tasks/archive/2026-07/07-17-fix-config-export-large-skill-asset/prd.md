# 修复配置导出被大体积 Skill 资源阻断

## Goal

让配置导出完整携带单个大于 1 MiB、但仍处于现有 8 MiB 单 Skill 总预算内的必要资源，
并可无损导入；同时不放宽文件数量、总量、路径、符号链接、特殊文件、Base64 或
64 MiB 导入 bundle 等既有安全边界。

## Background

### 用户观察

- 另一台 Windows 机器在“设置 -> 导出配置”时整个导出失败。
- 核心错误为
  `SEC_INVALID_INPUT: file C:\Users\admin\.codex\skills\keep-codex-fast\assets\keep-codex-fast-cover.png too large (max 1048576 bytes)`。
- 触发对象是 Skill 的 PNG 资源，而不是 `SKILL.md`。

### 已验证事实

- `src-tauri/src/infra/config_migrate/mod.rs:20-26` 同时定义 64 MiB 导入文件上限、
  1 MiB Skill 单文件上限、8 MiB 单 Skill 总量、256 个文件及专用元数据上限。
- `src-tauri/src/infra/config_migrate/skill_fs.rs:97-124` 在递归导出时以 1 MiB 读取每个
  文件；任一文件超限会立即返回错误并中止整个 bundle。
- 同文件 `317-373` 在导入前对 Base64 长度、解码后单文件大小和 8 MiB 总量做对称
  校验；目标目录写入发生在校验之后。
- 1 MiB 单文件限制来自安全加固提交 `13bba9b0`。现场失败不是 PNG 解码、Windows
  路径或已达到 8 MiB 总量造成的。
- 完整证据、可证伪结论和回归矩阵见
  `research/config-export-large-asset.md`。

## Requirements

### R1. 对齐单文件与单 Skill 预算

- 将配置迁移的 Skill 单文件原始字节上限由 1 MiB 对齐到现有 8 MiB 单 Skill 总预算。
- 导出读取、导入 Base64 预检和解码后校验必须使用同一上限，避免生成可导出但不可导入
  的 bundle。
- 大于 1 MiB 且不超过 8 MiB 的 PNG、二进制或其他必要文件必须原样纳入；不得静默
  跳过、截断、替换或仅保留文件名。

### R2. 保持安全边界

- 单个 Skill 的全部导出文件仍不得超过 8 MiB，文件数仍不得超过 256。
- 保留相对路径长度与组件校验、重复路径拒绝、UTF-8 路径要求、符号链接逃逸拒绝、
  特殊文件拒绝及循环目录防护。
- 保留 `SKILL.md`、source metadata 的专用较小读取上限。
- 保留 Base64 编码前后双重大小校验和 64 MiB 配置导入文件上限；不得通过压缩、
  流式绕过或延后到写盘后再校验。

### R3. 兼容与错误语义

- 不改变配置 bundle schema、Skill 文件相对路径或 Base64 字段格式。
- 现有 v1/v2 导入兼容、installed/local Skill 行为及其他配置项导出保持不变。
- 单文件超过 8 MiB、总量超过 8 MiB或违反其他安全规则时，仍以明确错误使导出或
  导入失败；本任务不把安全失败降级为警告。

### R4. 串行依赖

- 本任务是父任务第 4 项；只有子任务 3 已验收、提交并归档后才能启动。
- 本任务验收、提交并归档前不得启动子任务 5。
- 本任务不得访问项目 git remote `upstream`。

## Acceptance Criteria

- [x] 嵌套 `skills/<name>/assets/` 下 `>1 MiB && <8 MiB` 的 PNG 可导出并导入，回读
      字节与源文件完全一致。
- [x] 恰好 8 MiB 的单文件在单 Skill 总量允许时可 round trip；8 MiB 加 1 字节在导出
      和导入两侧均被拒绝。
- [x] 多文件各自合法但合计超过 8 MiB、257 个文件、超长/遍历/重复路径、符号链接逃逸、
      特殊文件和超长 Base64 均继续被拒绝。
- [x] 64 MiB 导入 bundle 上限及“校验失败前不创建目标目录/不写文件”回归保持通过。
- [x] 正常小文件、installed/local Skill、v1/v2 bundle 和非 Skill 配置迁移测试通过。
- [x] 未修改业务范围外的 Skill 安装、同步或运行时文件预算。

## Out of Scope

- 静默遗漏大文件后仍宣称导出成功。
- 放宽单 Skill 8 MiB 总预算、256 文件上限或 64 MiB 导入 bundle 上限。
- 引入压缩包、新 schema、分卷导出或媒体转码。
- 修改通用 Skill 安装/复制、WSL Skill 同步的独立安全常量。
