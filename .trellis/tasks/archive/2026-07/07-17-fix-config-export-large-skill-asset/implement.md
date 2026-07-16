# Implementation Plan: 大 Skill 资源安全 round trip

## 进入门槛

- [ ] 确认子任务 3 已验收、提交并归档，工作树不存在未归属变更。
- [ ] 只启动本子任务；不派生子代理，不启动子任务 5。
- [ ] 读取本任务 research、cross-layer guide 和 code-reuse guide。
- [ ] 确认尚未访问项目 remote `upstream`。

## 常量与实现

- [ ] 将配置迁移的 Skill 单文件上限与现有 8 MiB 单 Skill 总预算对齐，优先让常量关系
      显式表达而不是复制数值。
- [ ] 确认导出读取、Base64 长度预检和解码后校验均由同一单文件常量驱动。
- [ ] 不改变 8 MiB 总量、256 文件、64 MiB 导入 bundle、路径、符号链接、特殊文件、
      `SKILL.md` 或 source metadata 边界。
- [ ] 不增加静默跳过、文件扩展名特判、截断或 schema 变更。

## 回归测试

- [ ] 更新旧的 1 MiB 失败断言，证明 `1 MiB + 1` 合法文件可导出。
- [ ] 增加嵌套 assets PNG 的 `>1 MiB && <8 MiB` 导出/导入字节级 round trip。
- [ ] 覆盖恰好 8 MiB 和 8 MiB 加 1 字节的导出、导入边界。
- [ ] 覆盖多个合法文件累计超过 8 MiB，以及超长 Base64 在创建目标目录前失败。
- [ ] 保留并运行 257 文件、重复/遍历/超长路径、符号链接逃逸、特殊文件和 v1/v2
      bundle 兼容回归。
- [ ] 保留 64 MiB 配置导入文件读取边界测试。

## 验证

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib --locked`
- [ ] `pnpm tauri:fmt`
- [ ] `pnpm check:generated-bindings`
- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `git diff --check`
- [ ] 审阅 diff，确认仅修改 config migration 常量/测试且没有业务代码范围外预算漂移。

## 退出门槛

- [ ] 记录 round trip 与全部安全负例证据。
- [ ] 只提交本子任务的实现与测试，完成 Trellis check/spec judgment 并归档。
- [ ] 归档与干净工作树确认完成前，不得启动子任务 5 或访问 `upstream`。
