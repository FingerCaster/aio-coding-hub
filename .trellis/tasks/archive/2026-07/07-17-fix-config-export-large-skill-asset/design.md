# Design: 在既有总预算内放宽 Skill 单文件

## 当前数据流

```text
Skill 目录
  -> 递归遍历与路径/文件类型检查
  -> 每文件按 1 MiB 上限读取
  -> 单 Skill 原始字节累计不超过 8 MiB
  -> Base64 写入 ConfigBundle
  -> 导入时按 1 MiB Base64/解码上限预检
  -> 单 Skill累计不超过 8 MiB
  -> 全部校验成功后写入目标目录
```

问题发生在第一处单文件读取：一个合法且仍处于 8 MiB 总预算内的资源会让整个导出提前
失败。PNG 没有专门解析分支，文件扩展名和 Windows 路径不是根因。

## 目标数据流

```text
Skill 目录
  -> 保持全部遍历与路径安全检查
  -> 每文件最多读取 8 MiB
  -> 保持单 Skill 原始字节累计最多 8 MiB
  -> 保持 Base64 bundle 格式
  -> 导入按同一个 8 MiB 单文件预算预检并解码
  -> 保持单 Skill累计、64 MiB bundle 与写前校验
```

## 实现边界

1. 在 `src-tauri/src/infra/config_migrate/mod.rs` 中让
   `CONFIG_SKILL_FILE_MAX_BYTES` 显式复用或等于
   `CONFIG_SKILL_TOTAL_MAX_BYTES`，使两者不会形成无产品依据的 1 MiB/8 MiB落差。
2. 继续让 `CONFIG_SKILL_FILE_BASE64_MAX_BYTES` 从原始单文件上限派生；不手写第二个
   魔法数字。
3. 复用 `SkillFileCollector::push_file` 和 `decode_skill_files_for_write` 的现有检查顺序。
   不新增按扩展名跳过逻辑，也不绕开 `read_file_with_max_len`。
4. 不改变 `CONFIG_SKILL_TOTAL_MAX_BYTES`、`CONFIG_SKILL_FILE_COUNT_MAX`、
   `CONFIG_IMPORT_FILE_MAX_BYTES`、路径和专用 metadata/`SKILL.md` 常量。
5. 不改变 `ConfigBundle`/`SkillFileExport` 序列化结构，因此无需 schema 或数据迁移。

## 安全分析

- 单文件上限提高后，攻击者可控制的单 Skill 原始载荷总量仍是 8 MiB，内存和解码总预算
  没有提高。
- 单个 8 MiB 文件的 Base64 文本约为 10.67 MiB，但导入仍先做派生长度预检，并受
  64 MiB 输入 bundle 上限约束。
- 导出仍是全有或全无：合法资源完整携带；真正超出 8 MiB 或违反路径规则时明确失败。
  这避免静默产生不完整、只能在另一台机器上才暴露问题的 Skill。
- 符号链接目标必须位于 Skill 根目录，特殊文件仍拒绝，全部导入文件仍在创建目标目录
  前完成解码和预算校验。

## 测试设计

在 `src-tauri/src/infra/config_migrate/tests.rs` 使用临时目录和确定性字节，不依赖用户真实
Skill：

- 将原“超过 1 MiB 必须失败”测试改为证明 `1 MiB + 1` 可导出。
- 增加 `>1 MiB && <8 MiB` PNG 的 export/import 字节级 round trip。
- 覆盖恰好 8 MiB、8 MiB 加 1、多个文件合计超 8 MiB。
- 在 import fixture 中覆盖派生 Base64 上限、解码后上限和失败前不创建目录。
- 保留并运行文件数、路径、重复路径、符号链接、特殊文件、v1/v2 兼容测试。
- 运行 `src-tauri/src/commands/config_migrate.rs` 的 64 MiB 导入读取边界测试。

## 风险与回滚

- **风险：** 测试若一次分配多个边界文件会放大 CI 内存。测试应复用最小必要 fixture，
  避免同时保留多份 8 MiB Base64。
- **风险：** 只修改导出会造成不可导入；因此常量、Base64 派生值和双向测试必须作为
  同一提交。
- **回滚：** 可整体回退常量和对应测试提交；无 schema、数据库或用户数据迁移。
