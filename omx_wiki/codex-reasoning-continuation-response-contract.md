---
title: "Codex reasoning continuation response contract"
tags: ["continuation-response-contract", "bplus-protocol-reconstruction", "single-visible-response", "release-rule", "usage-accounting"]
created: 2026-07-05T11:13:07.453Z
updated: 2026-07-06T02:01:00.000+08:00
sources: []
links: ["codex-reasoning-guard-retry-count-confusion.md", "upstream-main-reconciliation-2026-07-05.md"]
category: decision
confidence: medium
schemaVersion: 1
---

# Codex reasoning continuation response contract

## Status

B+ target implemented and locally validated on 2026-07-06. Revision 8 was accepted by Claude round-8 review and local review before implementation; the final implementation then passed full local validation and Claude post-validation review using `claude-opus-4-8`.

Validation evidence recorded for the implementation:

- `pnpm tauri:fmt`
- `pnpm tauri:check`
- `pnpm tauri:clippy`
- `cargo test --manifest-path src-tauri\Cargo.toml codex_reasoning_continuation --lib --locked` (54 passed)
- `cargo test --manifest-path src-tauri\Cargo.toml success_event_stream --lib --locked` (11 passed)
- `pnpm tauri:test` (Rust main suite 1935 passed / 3 ignored, plus integration suites passed)
- `pnpm typecheck`
- `pnpm lint`
- `pnpm test:unit` (269 files / 2120 tests passed)
- `pnpm check:prepush` (15/15 passed)

Claude review artifacts:

- `.omx/artifacts/ask-claude-bplus-implementation-review-20260706T011149+0800.md`
- `.omx/artifacts/ask-claude-bplus-implementation-review-20260706T013114+0800.md`
- `.omx/artifacts/ask-claude-bplus-final-post-validation-review-20260706T015431+0800.md`

The selected direction is protocol reconstruction, not the minimal folded-SSE patch. Successful continuation repair should look to clients like one coherent Codex Responses stream: no duplicate cross-round visible assistant text, no lost visible content, protocol-coherent streaming frames, safe handling of reasoning/tool/commentary items, and usage accounting split between client UX and provider cost/logging.

## Problem

Native Codex Responses continuation repair currently folds the initial response and successful continuation responses into one synthetic SSE. The fold starts from the final response, but replaces its `output` with merged output items from all rounds.

Observed code shape:

- `success_event_stream.rs` starts repair with `responses = vec![current.clone()]`, appends each continuation response, and replaces `raw` with `folded_raw` when status is `Repaired`.
- `codex_reasoning_continuation.rs` builds `folded_raw` in `fold_responses_to_sse`.
- `fold_responses_to_sse` clones the last response, then inserts `merged_output_items(responses)` into `response.output`.
- `merged_output_items` deduplicates only narrow cases: same item id, exact visible text, or prefix-extension visible text.
- Continuation payloads append prior `reasoning` items plus `Continue thinking...` to the original input, so real behavior is append-tail compatible and final visible text cannot be assumed complete.

This means user-visible assistant output can contain multiple visible messages from different rounds, while a naive final-round-only fix can lose the first visible segment in append-tail cases.

## Target client contract

- `NotApplicable`: return the original upstream response unchanged.
- `Repaired`: expose one coherent user-visible assistant response, not multiple visible assistant messages from different rounds.
- `Repaired`: final-round visible text may be used alone only when it is classified as complete by a conservative rule. `EmptyPrior`, exact duplicate, and monotonic strict-prefix final extension are safe in v1 only when the non-final visible-payload guard is satisfied. Multiple visible prior rounds must form a prefix chain; they must not be concatenated to create a synthetic prior. Quoted/non-prefix incorporation and non-contiguous subsequence matches are unsafe unless a later sample-backed rule proves them safe.
- `Repaired`: rounds with no visible client output are skipped for visible prefix-chain comparison, but only after they pass the internal-only classifier. A false-empty round with hidden visible payload still blocks final-only branches.
- `Repaired`: clean append-tail rounds may be assembled into one visible message only after Phase 0 real samples prove the pattern and the deterministic overlap rule identifies a safe boundary. Otherwise `CleanAppend` is unsafe in v1.
- `Repaired`: use the final stable round's raw SSE only in the final-full branch. If visible content must be assembled across rounds, emit a synthetic but protocol-coherent Responses SSE with matching added/delta/done/completed frames.
- `Repaired`: early reasoning and synthetic commentary markers are diagnostics/replay state only in v1. Non-final function/tool calls are unsafe in v1 unless a future implementation can replay them with a complete coherent event sequence.
- `Repaired`: visibility detection is a conservative superset. Assistant messages, refusals, message-unwrapped output text, unknown visible-result output items, and ambiguous output items count as visible for classification so `EmptyPrior` cannot silently discard them. Final-only comparison branches may compare only non-final assistant-message `output_text`; non-final refusals, message-unwrapped `output_text`, unknown visible-result items, or mixed visible payloads route unsafe instead of being dropped.
- `Failed`, `StillMatched`, `BudgetExhausted`, or `MissingEncrypted`: do not return a half-repaired continuation response as success; keep the existing guard / failover behavior.
- Successful continuation repair must not emit multiple visible assistant messages from different rounds. It must either assemble a single complete visible message, use a proven complete final message, or mark repair unsafe.
- Intermediate continuation rounds are replay / diagnostic state except for clean append-tail visible text needed to assemble the complete answer.

## Internal recording contract

Keep the round history separately for diagnostics and accounting:

- initial response id, model, status, reasoning token pointer, output token count, and visible text hash/length;
- each continuation round response id, status, reasoning token pointer, output token count, visible text hash/length, and failure reason if any;
- `sent_rounds`, configured `max_rounds`, terminal repair status, and terminal reason;
- final stable response id and final client status;
- visible assembly kind;
- client usage plus provider repair usage.

Usage must be split by consumer. Provider repair usage tracks the actual upstream calls for quota, cost, logs, and diagnosis. Client usage should represent one coordinated logical request and must not blindly repeat-count input tokens for every repair round. The repaired-only usage override must not change non-repair native Codex success path usage parsing.

Usage views:

- client usage: coordinated single-request usage exposed in the repaired client SSE;
- provider repair usage: provider-reported usage across all initial and continuation requests used internally for quota, cost, logs, and diagnosis.

Returning a single coherent visible assistant response must not cause internal accounting to undercount continuation repair cost, and provider repair accounting must not force client-visible usage to repeat-count input tokens.

Canonical response id and idle behavior are release gates:

- The final stable response id may be treated as canonical only after Phase 0 validates real upstream `previous_response_id` transcript consistency, not only id resolution, or the release documents a tested non-continuable mitigation.
- V1 repair remains pre-commit to preserve HTTP-level guard/failover. It must cap repair wall-clock below the minimum tested downstream/proxy/client response-header or first-byte (TTFB) timeout threshold and abort repair before committing headers if the cap is reached. Tested downstream paths and residual risk for untested paths must be recorded. Adding, removing, remeasuring, or changing timeout settings for a supported path requires recomputing the minimum threshold and adjusting the cap when needed. The cap-below-minimum-threshold invariant must be backed by an automated configuration/startup assertion or regression test across every currently supported path. Downstream SSE keepalive is future committed-stream work, not v1.
- Final-full passthrough is allowed only when the final raw stream contains no synthetic marker, commentary-visible output, echoed prior reasoning, unclassified pre-completion semantic frame, early `data: [DONE]`, or other internal item that violates the visible-output policy. Completion-before frames that carry reasoning, summary, commentary, or `encrypted_content` through event names, item types, phases, channels, or payload keys must fail closed unless they are explicitly modeled by the visibility classifier and release tests prove they are safe. `data: [DONE]` is a terminal sentinel and is allowed only after exactly one `response.completed` frame.
- Retained repair bytes have a 20 MiB aggregate cap for peak concurrently retained round bytes and reconstructed buffers.

## B+ implementation direction

- Replace the current "fold every round into one synthetic SSE" repaired path with an event-aware reconstruction path.
- Track each repair round as `{raw_sse, aggregated_response, usage, response_id, terminal_status, reasoning_token_state}`. The first matched response and every continuation response must be available to the repair builder.
- Classify visible text across rounds before selecting a reconstruction strategy: final-full passthrough, sample-enabled clean append-tail assembly, or unsafe.
- Preserve final raw event sequence only in the final-full passthrough branch. For append-tail assembly, emit a coherent synthetic Responses stream whose terminal output exactly matches emitted frames.
- Decouple accounting from `raw`: compute client usage for the logical repaired request, compute provider repair usage for quota/cost/request logs, and record both.
- Use unsafe/fallback behavior when reconstruction cannot preserve semantics, especially if visible text assembly is ambiguous or a non-final function/tool call appears.

## Required tests

- Replace tests that preserve distinct/quoted cross-round visible messages as multiple messages; the repaired client output must be one coherent visible response or unsafe.
- Add append-tail tests proving the first visible segment is not lost and duplicated overlap is removed.
- Preserve final-round streaming fidelity in the final-full branch with a test that proves final `response.output_text.delta` frames survive.
- Prove early reasoning and commentary markers do not leak as visible client output.
- Prove non-final function/tool-call handling marks repair unsafe in v1; it must not be silently dropped or partially exposed.
- Prove refusals, message-unwrapped output text, and ambiguous visible-result items are not misclassified as invisible.
- Prove prior-round refusals, message-unwrapped output text, unknown visible-result items, and mixed visible payloads block `EmptyPrior` / `FinalSuperset` / exact-duplicate final-only branches instead of being silently dropped.
- Prove visible prefix-chain comparison skips only rounds with no visible client output and only after those skipped rounds pass the internal-only classifier.
- Prove every final-full passthrough branch (`EmptyPrior`, exact duplicate, and `FinalSuperset`) delivers a client body whose visible hash/length equals the selected final source round visible hash/length.
- Prove final-full passthrough refuses final streams that contain internal markers, commentary-visible output, or echoed prior reasoning.
- Prove final-full passthrough refuses unclassified final raw frames before `response.completed`, especially early `data: [DONE]`, malformed semantic data, unknown events, `response.reasoning_summary_text.delta`, or other reasoning/summary/commentary/`encrypted_content` payloads that are not represented in the aggregated output classifier.
- Prove provider repair usage is available for quota/logging while client usage does not blindly sum input tokens across rounds.
- Prove repaired-only usage override leaves non-repair native Codex success path usage behavior unchanged.
- Prove final stable rounds with no coherent visible text and no final actionable tool/function call do not return intermediate visible text as success.
- Prove v1 repair emits no downstream bytes before reconstruction, and a response-header/TTFB-based pre-commit timeout returns to guard/failover without being logged as repaired success.
- Prove an automated assertion/test fails if the configured pre-commit cap is not below the minimum tested response-header/TTFB timeout threshold.
- Prove the cap invariant recomputes the minimum when supported downstream/client/proxy paths are added, removed, remeasured, or have timeout settings changed.
- Record per-round repair durations and cumulative duration so slow upstream rounds can be distinguished from excessive round counts.
- Keep the existing `NotApplicable` original-body behavior unchanged.
- Add an end-to-end repaired-path test around `success_event_stream` / route behavior covering body content, special settings, and request-log usage.

## Open questions

- External review should challenge the visible-text classifier, especially final superset vs clean append-tail vs ambiguous distinct.
- External review should challenge whether client usage and provider repair usage are split correctly.
- External review should challenge whether non-final function/tool-call carry-forward should ever be allowed, or whether its presence should always make continuation repair unsafe.

## Release rule

Before publishing a version that touches Codex reasoning guard / continuation repair, verify that successful continuation repair exposes one coherent user-visible assistant response with no duplicated intermediate-round visible text and no silently lost visible content. `FinalSuperset` is v1-limited to exact duplicate or monotonic strict-prefix extension without cross-round concatenation; quoted/non-prefix containment is unsafe. Empty rounds may be skipped in prefix-chain comparison only after the internal-only classifier proves they have no visible client output. Non-final refusals, message-unwrapped output text, unknown visible-result items, and mixed visible payloads must block final-only branches instead of being silently dropped. Final-full passthrough must not leak internal markers, commentary-visible output, echoed prior reasoning, or unclassified final raw pre-completion frames carrying reasoning/summary/commentary/`encrypted_content`; unknown or unparseable semantic frames before `response.completed` must fail closed, and `data: [DONE]` is allowed only after exactly one completed frame. Delivered visible hash/length must match the selected final source round visible hash/length for `EmptyPrior`, exact duplicate, and `FinalSuperset`. Early reasoning and commentary markers must not leak as stale visible output. Non-final function/tool calls must be unsafe unless fully and coherently replayed. Client-visible usage must not blindly repeat-count input tokens, provider repair usage must remain available for quota, cost, and logs, non-repair usage behavior must not regress, canonical response id continuity must be real-upstream transcript validated or mitigated, v1 repair must remain pre-commit with a tested wall-clock cap below the minimum measured response-header or first-byte (TTFB) timeout threshold enforced by automated assertion/test across the current supported path set, supported path additions/removals/remeasurements/timeout changes must recompute the minimum threshold, repair wall-clock diagnostics must include per-round and cumulative duration, and peak retained repair bytes must respect the aggregate 20 MiB cap.
