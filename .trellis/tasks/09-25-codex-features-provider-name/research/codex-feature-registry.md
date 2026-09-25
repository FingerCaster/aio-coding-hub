# Codex feature registry snapshot

Read on 2026-09-25 from `openai/codex` `main`:

- `codex-rs/features/src/lib.rs`: `remote_compaction` is absent. `remote_compaction_v2`, `responses_websockets`, and `responses_websockets_v2` are `Stage::Removed`. `fast_mode` and `multi_agent` (`Feature::Collab`) are `Stage::Stable`. `fast_mode.default_enabled` is `true`, so deleting the key turns it on.
- Commit `3dc1e2a5` retired the compaction toggle. Supported providers always use streamed remote compaction v2.
- `config.schema.json` documents `service_tier` as `default`, `priority`, or `flex`, with legacy `fast` still accepted. Commit `7abf2a3b` requires an explicit `flex` to survive fast mode being disabled.

AIO currently couples the switch at `src/components/cli-manager/tabs/CodexTab.tsx:117` and selects provider identity only from `features.remote_compaction` at `src-tauri/src/infra/codex_config/provider_projection.rs:202`.
