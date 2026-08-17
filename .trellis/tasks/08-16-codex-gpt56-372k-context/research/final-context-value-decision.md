# Final context value decision

- Date: 2026-08-17
- Decision: GPT-5.6 372K uses the exact JSON integer `372000`.
- Rationale: Codex `rust-v0.147.0` writes the existing 272K window as `272000` in `codex-rs/models-manager/models.json`; the feature follows that upstream decimal catalog convention.
- Superseded value: `380928` (`372 * 1024`) is not the feature value and is valid only as a negative test case.
- Preserved upstream behavior: the 95% effective-window rule remains unchanged, so nominal `372000` yields `353400`; the default 90% auto-compact threshold is `334800`.
- Scope: both `context_window` and `max_context_window` are rewritten to `372000` for only `gpt-5.6-sol`, `gpt-5.6-terra`, and `gpt-5.6-luna`.
