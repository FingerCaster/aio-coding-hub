# 技术设计

## 1. 边界与数据流

修复集中在 Rust updater command 层，不改变发布清单格式：

```text
tauri-plugin-updater::Update
  ├─ target: OS target (windows/darwin/linux) -> 运行时/安装器语义
  └─ tauri_plugin_updater::target(): manifest key (windows-x86_64/...) -> 清单查找与资产语义
```

Beta manifest 的严格校验继续验证四个静态键、版本、UTC 时间、GitHub host、release tag、官方资产名和非空签名。

## 2. 单一映射合同

新增一个私有、可测试的官方平台描述或等价 helper，按当前 support matrix 返回：

| manifest key | OS target | asset |
| --- | --- | --- |
| windows-x86_64 | windows | aio-coding-hub-win64.msi |
| darwin-x86_64 | darwin | aio-coding-hub-macos-intel.tar.gz |
| darwin-aarch64 | darwin | aio-coding-hub-macos-arm.tar.gz |
| linux-x86_64 | linux | aio-coding-hub-linux-amd64.AppImage |

`updater_candidate_identity` 从 `Update.target` 校验 OS target 与当前运行平台一致，再用 manifest key 获取期望资产并校验 URL。测试 helper 接受显式的两个 target 值，避免依赖 host `cfg` 导致 Windows 无法覆盖 Unix 分支。

## 3. 调用点

1. `fetch_updater_candidate`：保留 Tauri updater 的默认 target/json target 选择，只在返回 `Update` 后调用严格 Beta raw manifest 校验。
2. `updater_candidate_identity`：解析当前运行平台的 manifest key，并把 `Update.target` 与该 key 对应的 OS target 比较；将 manifest key 传给 `official_updater_asset_name` 和 URL 校验。
3. Beta fresh-check/install：继续调用同一个 identity helper，确保初次检查和安装前复核一致。
4. Stable：继续使用同一个 OS target 校验与官方资产规则，但不启用 Beta raw manifest 四平台额外校验，保持既有 Stable 兼容性。

## 4. 兼容与错误处理

- 不接受未知 OS target、未知 manifest key、错误架构或空签名。
- 不将静态键直接写入 `Update.target`，不依赖字符串替换或动态 `command.length` 兼容。
- 保持错误码 `UPDATER_MANIFEST_INVALID`；可改进内部诊断文本，禁止前端按文案分支。
- 不需要生成 TypeScript bindings 变化；仍运行 generated-bindings 检查确认无漂移。

## 5. 测试策略

- Rust unit tests：四平台正向映射、OS target mismatch、manifest key mismatch、空签名、错误资产 URL、Beta raw schema。
- 候选 identity 测试：同一输入经初次检查和 fresh-check 得到相同 identity；Windows 场景显式复现之前的 `windows`/`windows-x86_64` 差异。
- Stable regression：稳定候选仍接受规范 stable URL，Beta 版本仍拒绝进入 Stable。
- 发布后：实际 Windows Beta 客户端检查；在线 manifest/pointer/asset 独立核验。

## 6. 发布与回滚

先通过 PR 合入 `origin/main`，再解析远端最新 Beta 版本并选择严格递增版本。发布 workflow 使用合入 commit 的不可变 SHA；任何资产或 pointer 验证失败都停止发布/推进。客户端修复若需要回滚，回滚代码 PR 不覆盖已发布 tag，channel pointer 仍按现有 CAS/recovery 合同管理。
