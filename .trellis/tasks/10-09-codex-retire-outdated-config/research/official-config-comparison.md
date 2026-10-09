# Official Codex config comparison

Source checked on 2026-10-09:

- [Configuration Reference](https://developers.openai.com/codex/config-reference)
- [Sample Configuration](https://developers.openai.com/codex/config-sample)
- Codex 0.35.0 removed `preferred_auth_method` in [openai/codex#3189](https://github.com/openai/codex/pull/3189). Login state now lives in `auth.json`.
- Current `auth.json` still uses `auth_mode: "apikey"` and `OPENAI_API_KEY` (`codex-rs/protocol/src/auth.rs`, `codex-rs/login/src/auth/storage.rs`).

## Fields AIO still writes

| Field | Official status | This task |
| --- | --- | --- |
| `preferred_auth_method` | Removed from `config.toml` in Codex 0.35.0. Absent from the current reference for every value. | Stop inserting it. Drop the whole root key during route projection, WSL configure, and disable restore. |
| `approval_policy = "on-failure"` | Deprecated. Reference says use `on-request` or `never`. | Stop offering and recommending it for new choices. Keep reading an existing value so the control can show it. Do not rewrite a file that already contains it unless the user changes the control. |
| `features.exec_policy`, `features.apply_patch_freeform` | Absent from the current complete reference. The Codex settings UI does not expose them. The structured patch can still write them. | Remove them from the structured read/write surface so a save cannot set them. Leave an existing line untouched on unrelated saves. |
| `model_provider`, `model_providers.*.name`, `base_url`, `wire_api`, `requires_openai_auth` | Current. `wire_api = "responses"` is the only supported value and the default when omitted, but it is still a documented field. | Keep. |
| `model_catalog_json` | Current. | Keep. |
| `windows.sandbox` | Current (`unelevated` or `elevated`). | Keep. |
| `forced_login_method` | Current (`chatgpt` or `api`). | Keep. AIO does not invent it. |
| `auth.json` `auth_mode = "apikey"` and `OPENAI_API_KEY` | Current credential file, not the removed config key. | Keep. |
| `features.remote_compaction` | Absent from the current reference. AIO already stopped writing it and still reads an existing flag to choose the managed provider. | Do not change the reader in this task. |
| `features.responses_websockets_v2` | Absent. Current provider field is `supports_websockets`. AIO no longer writes the old flag. | Do not strip a user's existing line on an unrelated save. |

## Local file constraint

Do not open, read, or write the machine Codex home `config.toml` or `auth.json`. Tests must use temporary Codex homes only.
