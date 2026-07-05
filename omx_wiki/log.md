# Wiki Log

## [2026-07-06T02:01:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md
- **Summary:** Marked the B+ continuation repair contract as implemented and locally validated. Recorded full validation evidence, including `pnpm check:prepush` 15/15 passed, and Claude post-validation review artifacts using the full model id `claude-opus-4-8`.

## [2026-07-06T00:55:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Folded architecture WATCH finding into the B+ release rule: `data: [DONE]` is a terminal sentinel and final-full raw passthrough must reject it before `response.completed`, while still allowing it after exactly one completed frame.

## [2026-07-06T00:36:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Folded code-review finding into the B+ release rule: final-full raw passthrough must reject unclassified pre-`response.completed` semantic frames, especially reasoning/summary/commentary/`encrypted_content` deltas such as `response.reasoning_summary_text.delta`.

## [2026-07-05T21:40:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md
- **Summary:** Marked the B+ continuation repair contract as accepted after Claude round-8 review and local sanity review. Implementation has not started.

## [2026-07-05T21:37:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Folded Claude round-7 hardening into the B+ contract: cap invariant now recomputes across the current supported path set, final-full branches share delivered visible hash/length assertions, and repair diagnostics record per-round plus cumulative durations.

## [2026-07-05T21:31:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Folded Claude round-6 non-blocking hardening into the B+ contract: record tested TTFB paths and cap invariant status, remeasure new downstream paths, skip only classifier-proven empty rounds in prefix chains, and assert FinalSuperset delivered hash/length matches the final round.

## [2026-07-05T21:25:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Revised the B+ continuation repair contract after Claude round-5 review: final-only branches now block prior non-message visible payloads instead of dropping them, pre-commit timeout is based on measured response-header/TTFB thresholds, and the 20 MiB cap is defined as peak concurrently retained repair bytes.

## [2026-07-05T21:24:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Revised the B+ continuation repair contract after Claude round-4 review: v1 stays pre-commit with a wall-clock cap instead of downstream keepalive, FinalSuperset requires a monotonic prefix chain, final-full passthrough needs internal-item classification, canonical id continuity requires transcript consistency, and retained repair bytes use an aggregate 20 MiB cap.

## [2026-07-05T21:04:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Revised the B+ continuation repair contract after Claude round-3 review: conservative visible-output classification, strict-prefix-only FinalSuperset in v1, repaired-only usage override, real-upstream canonical response-id gate, and tested keepalive/idle handling are now release rules.

## [2026-07-05T20:36:00.000+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Revised the B+ continuation repair contract after Claude review: final-round-only is unsafe for append-tail continuation, so the target is now one coherent visible response with append-tail assembly, diagnostics-only early reasoning, unsafe non-final tool calls, and split client/provider usage semantics.

## [2026-07-05T20:07:55.325+08:00] update
- **Pages:** codex-reasoning-continuation-response-contract.md
- **Summary:** Selected B+ protocol reconstruction as the planned continuation repair contract, replacing the minimal folded-SSE implementation note with final-round raw SSE fidelity, usage decoupling, safe non-visible carry-forward, and unsafe fallback rules.

## [2026-07-05T11:45:20.420Z] update
- **Pages:** codex-reasoning-continuation-response-contract.md, AGENTS.md
- **Summary:** Narrowed continuation repair release rule from final-response-only to final-visible-message-only, preserving non-visible output items and summed usage across rounds.

## [2026-07-05T11:13:07.453Z] add
- **Pages:** codex-reasoning-continuation-response-contract.md, codex-reasoning-guard-retry-count-confusion.md
- **Summary:** Recorded proposed final-response-only Codex reasoning continuation repair contract, internal trace/accounting split, release rule, and backlink from the prior retry-count note.

## [2026-07-04T22:34:41.2488663+08:00] add
- **Pages:** realtime-trace-card-cli-tab-leak-analysis.md
- **Summary:** Added high-confidence analysis of Request Logs processing realtime card CLI tab leakage.

## [2026-07-04T09:48:26.129Z] ingest
- **Pages:** codex-reasoning-guard-retry-count-confusion.md
- **Summary:** Created new page "Codex reasoning guard retry count confusion"

## [2026-07-04T09:48:26.142Z] add
- **Pages:** codex-reasoning-guard-retry-count-confusion.md
- **Summary:** Created wiki page codex-reasoning-guard-retry-count-confusion.md

## [2026-07-04T10:36:11.000Z] delete
- **Pages:** obsolete continuation-repair UI bug note and split-UI prototype
- **Summary:** Removed obsolete split continuation-repair UI bug document and prototype; unified guard spec supersedes them.

## [2026-07-05T05:33:51.842Z] add
- **Pages:** upstream-main-reconciliation-2026-07-05.md
- **Summary:** Recorded upstream merge baseline, fork-specific preservation rules, release version rationale, and future sync checklist.

## [2026-07-05T05:35:17.448Z] lint
- **Pages:** codex-reasoning-guard-retry-count-confusion.md, realtime-trace-card-cli-tab-leak-analysis.md, upstream-main-reconciliation-2026-07-05.md
- **Summary:** Lint: 3 issues (3 orphan, 0 stale, 0 broken, 0 contradictions)
## [2026-07-05T11:14:04.024Z] lint
- **Pages:** codex-reasoning-continuation-response-contract.md, realtime-trace-card-cli-tab-leak-analysis.md
- **Summary:** Lint: 3 issues (2 orphan, 0 stale, 0 broken, 1 contradictions)
## [2026-07-05T11:14:40.613Z] lint
- **Pages:** realtime-trace-card-cli-tab-leak-analysis.md
- **Summary:** Lint: 1 issues (1 orphan, 0 stale, 0 broken, 0 contradictions)
## [2026-07-05T11:46:03.029Z] lint
- **Pages:** realtime-trace-card-cli-tab-leak-analysis.md
- **Summary:** Lint: 1 issues (1 orphan, 0 stale, 0 broken, 0 contradictions)

## [2026-07-05T11:46:18.748Z] lint
- **Pages:** realtime-trace-card-cli-tab-leak-analysis.md
- **Summary:** Lint: 1 issues (1 orphan, 0 stale, 0 broken, 0 contradictions)
