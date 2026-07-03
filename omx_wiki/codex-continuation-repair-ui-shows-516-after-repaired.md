---
title: "Codex continuation repair UI shows 516 after repaired"
tags: ["codex", "degradation-guard", "continuation-repair", "ui", "bug"]
created: 2026-07-03T16:03:07.278Z
updated: 2026-07-04T02:08:00.000Z
sources: []
links: []
category: debugging
confidence: medium
schemaVersion: 1
---

# Codex continuation repair UI shows 516 after repaired

## Bug
Codex request detail can still show `思考 Token = 516` after continuation repair succeeded. This is a UI/observability wording issue, not evidence that continuation repair failed.

## Evidence
- Runtime log example: request_logs.id=30325, trace_id=1783093952-124.
- `special_settings_json` contains `codex_reasoning_continuation.status = repaired` and `sentRounds = 1`.
- The same record has `usage_json.output_tokens_details.reasoning_tokens = 516`, so the detail metric reads the folded usage value and looks like the original 516 was not repaired.
- Code path: `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs` replaces `raw` with folded SSE when continuation status is `Repaired`.
- UI path: `src/components/home/RequestLogDetailSummaryTab.tsx` displays `思考 Token` from `resolveRequestLogUsageReasoningTokens(selectedLog.usage_json)`.

## Expected future fix
Add an explicit continuation repair indicator in request detail/list, for example `继续思考补救：已修复，续写 1 轮`, and distinguish first-round matched reasoning token from final usage reasoning token. Avoid implying that `516` means no continuation when `codex_reasoning_continuation.status = repaired`.

## Settings UI follow-up
`继续思考补救` is currently configured inside `降智拦截详情`, but backend behavior is independent: turning off `codex_reasoning_guard_enabled` does not disable `codex_reasoning_guard_continuation_repair_enabled`.

This placement is misleading because users can reasonably read the inner switch as a sub-feature controlled by the outer `降智拦截` switch. In the future UI pass, either:

- Move `继续思考补救` out of `降智拦截详情` and show it as a sibling Codex setting/card.
- Or, if the move is deferred, add explicit copy near the switch: `此开关独立生效，即使降智拦截关闭也会运行`.

Do not change backend behavior to couple continuation repair to the main guard switch unless the product decision changes. The desired behavior remains: users may disable retry/switch/error style degradation guard while keeping continuation repair enabled.

## Settings UI priority copy
The future UI should also explain priority/order, not only independence. Current backend order for successful Codex SSE responses is:

- `继续思考补救` first: if the response looks like a continuation-repair candidate and repair succeeds, the repaired stream is returned and normal degradation rule matching is skipped for that response.
- `继续思考补救` failure states can consume guard budget: `missing_encrypted`, `capped_max_output_tokens`, `still_matched`, and `failed` force a guard-budget decision.
- `降智拦截` normal rule matching then runs only when continuation repair is not repaired and no forced continuation decision already handled the response.

Suggested UI wording when both controls are visible: `优先级：续写补救会先尝试修复疑似截断的成功响应；修复成功后不再触发普通降智拦截。修复失败或不适用时，再按降智拦截规则处理。`

## UI prototype
A static preview page was added at `omx_wiki/prototypes/codex-continuation-repair-ui-preview.html`.

It demonstrates:

- `降智拦截` and `继续思考补救` as sibling setting modules.
- `继续思考补救` marked as independently effective even when the main guard is disabled.
- Independent continuation repair statistics: `补救触发数`, `修复成功数`, `修复率`, and `平均续写轮数`.
- The priority copy users need to understand: continuation repair runs first, successful repair skips normal guard matching, and failure/not-applicable cases fall through to budget/rule handling.
- Request detail examples that explicitly show continuation status and avoid misreading `思考 Token = 516` as no repair.

The prototype was revised to stay close to the real `CLI 管理 > Codex` layout: same tab framing, outer setting cards, stats grid, summary rows, details button, and switch placement. It is intended as a simple UI alignment page, not an audit/dashboard design.

## Current bug: details date range cannot be changed
The current `降智拦截详情` page has a UI bug: after entering the details dialog, users cannot modify the statistics date range as expected.

Read-only code evidence:

- `src/components/cli-manager/tabs/CodexTab.tsx:2328` renders date range controls on the outer `降智拦截` card.
- `src/components/cli-manager/tabs/CodexTab.tsx:2548` also renders `renderCodexReasoningGuardStatsRangeControls("")` inside the `降智拦截详情` dialog, so the control is not simply missing.
- `src/ui/Popover.tsx` always renders `PopoverContent` through the shared Radix Popover wrapper.
- `src/ui/shadcn/popover.tsx` portals popover content to `body`, while `src/ui/shadcn/dialog.tsx` renders the details view as a modal Dialog layer. This makes the likely failure mode an embedded Dialog/Popover portal interaction or layering/focus issue, not an unbound date state.

Fix target for the real UI:

- Inherit the outer card's current range by default.
- Keep the same compact `时间范围` trigger used by the outer card; do not flatten the date controls into always-visible inputs inside details.
- Let users click the trigger and then switch quick ranges such as `今天`, `7 天`, and `30 天`.
- Let users edit explicit start/end dates inside that opened picker and apply them without closing the details dialog.
- Use the same date-range behavior for both `降智拦截详情` and `继续思考补救详情`.

The prototype includes a `详情内日期范围` section showing the intended corrected details UI.

Implementation in the current branch:

- `src/ui/Popover.tsx` and `src/ui/shadcn/popover.tsx` now support `portalled={false}` for popovers that must stay inside a modal Dialog focus tree.
- `src/components/cli-manager/tabs/CodexTab.tsx` keeps the overview date picker portalled, but renders the details date picker inside the Dialog and separates open state by `overview` vs `details`.
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx` covers changing the stats date range from inside `降智拦截详情`.

## Settings UI split implementation

Implemented in the current branch:

- `继续思考补救` is now a sibling Codex settings module instead of a section inside `降智拦截详情`.
- The guard summary card no longer includes a `继续思考补救` row.
- `保存规则` now saves only degradation guard rule/retry/model fallback fields.
- `保存补救` saves only continuation repair enablement and cap fields; the continuation switch also persists independently.
- The continuation card and detail dialog show independent stats: triggered requests, triggered attempts, repaired requests, repair rate, average sent rounds, and status distribution.
- Continuation stats are derived from `special_settings_json` entries with `type = codex_reasoning_continuation`.
- The continuation detail dialog uses the same fixed in-dialog date range picker behavior as `降智拦截详情`.

Validation evidence:

- `cargo test --manifest-path src-tauri/Cargo.toml codex_reasoning_guard_stats --lib`
- `pnpm vitest run src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx src/services/gateway/__tests__/requestLogs.test.ts`

## Tags
codex, degradation-guard, continuation-repair, request-log-ui
