# 执行计划

Worktree: `E:/OrcaProject/aio-coding-hub-fork/codex-retire-outdated-config`

Branch: `FingerCaster/codex-retire-outdated-config`

## Checklist

1. [x] 把已有 `preferred_auth_method` 草稿从主工作区移到这个 worktree。主工作区不再保留这些产品改动。
2. [x] 核对草稿：普通投影和 OAuth 投影都删除 `apikey`，WSL 不再插入并会删除 `apikey`，字段不再托管。
3. [x] 设置页停止推荐和新选 `on-failure`；当前值已是 `on-failure` 时仍显示为弃用项。
4. [x] 从结构化配置类型、解析、补丁和生成绑定移除 `features_exec_policy` 与 `features_apply_patch_freeform`。不在无关保存中删除已有行。
5. [x] 更新 `codex-config-contract.md`，写明上述三个过时项和仍然保留的当前字段。
6. [x] 用临时 Codex home 跑相关 Rust 与前端测试。不启动应用，不访问本机 Codex home。

## Validation

```powershell
node scripts/tauri-test.mjs --lib preferred_auth -- --test-threads=1
node scripts/tauri-test.mjs --lib retired_apikey -- --test-threads=1
node scripts/tauri-test.mjs --lib restore_codex_config_toml -- --test-threads=1
node scripts/tauri-test.mjs --lib manual_applied_codex_selects_wsl -- --test-threads=1
```

前端只跑 Codex 设置页相关测试。若测试 home 指向用户目录，先停下来，不继续运行。

## Rollback

草稿和后续改动都只在这个 worktree。撤回时丢弃该 worktree 的未提交改动，不恢复到本机 Codex home。
