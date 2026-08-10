# Neutral display for codex-auto-review model route

## Goal

When Codex auto-review requests a logical model such as `codex-auto-review`
and the upstream returns a real model such as `gpt-5.4` (with matching effort),
keep showing the request→actual mapping, but stop treating it as a severe red
route-mismatch warning.

## Background (verified)

- Real log `id=54059` (2026-07-16):
  - `requestedModel`: `codex-auto-review`
  - `actualModel`: `gpt-5.4`
  - effort request/response: both `low`
  - `model_route_mapping.mismatch = true` because model strings differ
  - no `codex_system_request` marker
- UI renders this as `codex-auto-review-low -> gpt-5.4-low` with rose/danger
  styling in list, audit tag, and detail metric card.
- This mapping is expected auto-review behavior, not a provider downgrade.

## Requirements

### R1. Identify expected auto-review route

- Treat a model-route mapping as **expected auto-review** when the mapping
  `requestedModel` (case-insensitive, trimmed) equals `codex-auto-review` or
  starts with `codex-auto-review-`.
- Do not require `codex_system_request`; production auto-review rows do not
  currently carry that marker.

### R2. Neutral presentation (not hide)

- Still show request → actual model/effort text.
- Still show audit tag and detail metric when a mapping exists.
- Use neutral/info styling instead of rose/danger for expected auto-review.
- Copy should read as expected mapping, not as a fault
  (e.g. summary: automatic-review model mapping / expected).

### R3. Preserve real mismatch warnings

- Ordinary mismatches (e.g. `gpt-5.5-high -> gpt-5.4-mini-low`) keep the
  existing severe red styling, labels, and summary wording.

### R4. Scope

- Frontend presentation only: `requestLogPresentation` + list/live/detail
  consumers and their tests.
- Do not change backend detection, special-setting persistence, or whether
  `model_route_mapping` is written.

## Acceptance Criteria

- [x] `codex-auto-review` (+ optional suffix) → real model mappings remain
      visible as `requested-effort -> actual-effort` text.
- [x] Those rows no longer use rose/danger styling for model badge, audit tag,
      or detail metric card.
- [x] Audit summary/label for those rows indicates expected auto-review mapping.
- [x] Non-auto-review route mismatches remain severe (rose/danger) with existing
      labels/summary.
- [x] Focused unit/component tests cover expected auto-review vs real mismatch.
- [x] No backend/runtime gateway behavior changes.

## Out of Scope

- Suppressing or deleting `model_route_mapping` special settings.
- Heuristics based only on actual model (`gpt-5.4*`) without the
  `codex-auto-review` requested-model prefix.
- Changing provider health, cost, or stats exclusion for auto-review.

## Notes

- Lightweight task: PRD-only is sufficient.
- Branch/worktree: `fix/codex-auto-reviewer-model-routing-detection` at
  `D:/OrcaProjects/aio-coding-hub-fork/codex-auto-reviewer-model-routing`.
