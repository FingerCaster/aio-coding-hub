# Beta 更新频道后端合同

## Goal

让 Tauri settings/updater 后端成为稳定与 Beta 频道的唯一权威选择点，保证逐设备授权、受控 endpoint、资源绑定、安装前复核和配置迁移的 fail-closed 行为。

## Dependencies

- Parent contract: 08-10-beta-release-channel R1-R3、R7。
- Depends on beta-release-pipeline completing and documenting the fixed Beta endpoint, Tauri manifest shape, release URL rule, channel-state/pointer semantics and candidate version format.
- beta-update-ui must not start until this child publishes generated bindings and frontend service normalizer fields.

## Requirements

- B1: Add a stable-default UpdateChannel value with controlled deserialization and a dedicated settings writer; ordinary settings patch/update cannot own it.
- B2: Sanitize Beta participation out of config export/import and preserve existing settings CAS/rollback behavior.
- B3: Choose the stable default endpoint or fixed Beta endpoint from canonical settings only; reject frontend/backend channel mismatch and all arbitrary URL input.
- B4: Store updater resources with channel/version/pointer identity, expose channel-aware metadata and exact Release URL, and provide discard.
- B5: Before Beta installation, re-check the current pointer and reject stale, switched, malformed or unverifiable candidates without fallback or downgrade.

## Acceptance Criteria

- [ ] Missing/invalid/imported channel is stable and cannot issue a Beta request.
- [ ] Beta enable requires the existing risky IPC confirmation; disable is immediate and durable under the settings lock.
- [ ] Exported settings cannot grant Beta on another device; imported true is normalized off.
- [ ] Stable check behavior and default endpoint remain unchanged.
- [ ] Cross-channel rid, changed settings, pointer pause/advance, manifest mismatch and fresh-check errors close the resource and never install.
- [ ] Generated bindings and focused Rust/service tests cover every new field and error.
