# Implementation Plan

## Checklist

1. 在 `provider_projection.rs` 改 `desired_provider_key_from_document` 为 PRD R6 的优先级，并让 projection/status 共用它。补 flag 优先、flag 缺失、注释/其他 table 不生效的单测。
2. 从 `types.rs`、`parsing.rs`、`patching.rs` 删除 `features_remote_compaction` 与 `features_responses_websockets_v2`。`FEATURES_KEY_ORDER` 同步删除这两项。不要把 `responses_websockets_v2` 加进每次保存都删除的 key。
3. 给 patch 增加只接受 `OpenAI` / `aio` 的 `model_provider`。为 `Some` 时收敛身份并删除 `remote_compaction`。`patch_requires_provider_sync` 改看这个字段。非法值零写入。
4. Raw 保存只在 desired key 变化时收敛，并在那次写入删除 legacy flag。key 不变时不改 provider，也不删 flag。
5. `CodexConfigState` 增加由同一函数算出的 `model_provider`。重新生成绑定，更新所有完整 fixture。
6. `CodexTab.tsx`：删除两个过时开关；`fast_mode` 不再碰 `service_tier`，缺失时显示关闭；`multi_agent` 去掉“实验性”；AIO Provider 第一项加 Provider name RadioGroup，并复用三段对话框。
7. 更新 `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md` 里仍把 `remote_compaction` 当身份来源的段落，使契约与 R5/R6 一致。保留 history sync 的内存和进程边界。

## Validation

- `cargo test -p aio_coding_hub_lib --lib infra::codex_config`
- `cargo test -p aio_coding_hub_lib --lib infra::cli_proxy::tests::remote_compaction`
- `cargo test -p aio_coding_hub_lib --test codex_provider_sync`
- `pnpm exec vitest run src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx src/query/__tests__/cliManager.test.tsx src/services/cli/__tests__/cliManager.service.test.ts`
- `pnpm check:generated-bindings`

先跑上述聚焦测试。身份投影和对话框测试通过后再跑仓库既有的全量质量门禁。

## Risky Files

- `src-tauri/src/infra/codex_config/provider_projection.rs`
- `src-tauri/src/infra/codex_config/patching.rs`
- `src-tauri/src/infra/codex_config/mod.rs`
- `src-tauri/src/infra/codex_config/types.rs`
- `src/components/cli-manager/tabs/CodexTab.tsx`
- `src/generated/bindings.ts`
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`

回滚点是 desired-key 函数和 patch 字段切换。不要在 UI 里直接改 TOML 文本。

## Before Start

- 规划摘要获得用户明确批准后再 `task.py start`。
- 实现时先读 `codex-config-contract.md`，再改 Rust 与 UI。
- 搜索所有完整 `CodexConfigState` / `CodexConfigPatch` fixture 后再生成绑定。
