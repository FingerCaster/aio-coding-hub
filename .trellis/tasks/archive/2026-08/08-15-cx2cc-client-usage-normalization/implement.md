# Implementation Plan

1. Add source-aware OpenAI detail extraction and saturating client input
   normalization in the Responses-to-IR usage parser.
2. Extend unit coverage for cached reads, cache creation aggregates plus
   5-minute/1-hour breakdowns, saturation, no-cache input, and top-level
   Anthropic-style cache fields.
3. Make synthesized Anthropic SSE place input/cache only in `message_start`
   and output only in `message_delta`, without changing true-stream placement.
4. Preserve pre-translation OpenAI provider metrics in the non-streaming CX2CC
   `usage_metrics` channel while retaining client `usage_json`.
5. Reuse the pre-bridge streaming observer for OpenAI provider metrics while
   retaining the post-bridge tracker for terminal behavior; cover raw usage
   selection without changing stream repair topology or client `usage_json`.
6. Correct the CX2CC protocol e2e expectation and add JSON/SSE assertions that
   client-visible fields are mutually exclusive while provider extraction is
   lossless.
7. Leave shared cross-layer spec/index changes to the parent integration
   worktree; record that ownership in this task without duplicating edits.
8. Run focused protocol bridge, usage, non-stream, and stream tests. Run
   `cargo fmt --check`, relevant `cargo clippy`, and `git diff --check`.
9. Review the full diff for scope, ensure only the designated task directory
   changed under `.trellis/tasks`, then commit with the repository's
   conventional message style and shell-derived Node/pnpm hook PATH.

## Risky Areas And Review Gates

- Do not move `BridgeStream`, response fixer, plugin, or relay ordering.
- Do not use normalized client usage for cost/quota/log persistence.
- Do not infer inclusive semantics from top-level cache fields.
- Confirm aggregate cache creation is subtracted once even when breakdowns are
  present.
- Confirm all `StreamFinalizeCtx` fixtures explicitly preserve their existing
  non-bridge behavior.

## Validation Commands

```text
cargo test --manifest-path src-tauri/Cargo.toml protocol_bridge
cargo test --manifest-path src-tauri/Cargo.toml usage
cargo test --manifest-path src-tauri/Cargo.toml success_non_stream
cargo test --manifest-path src-tauri/Cargo.toml usage_tee
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings
git diff --check
```

## Completion

- Implemented source-aware client normalization for nested OpenAI cache
  details, including saturation and aggregate/write-breakdown handling.
- Preserved raw provider metrics for normal non-stream, normal stream, and
  infinite buffered stream accounting while retaining post-bridge
  `usage_json`.
- Removed synthesized SSE input/cache repetition without changing true-stream
  event placement or continuation repair.
- Focused protocol bridge, usage, stream, non-stream, and request-end suites,
  formatting, Clippy with warnings denied, and whitespace validation pass.
