# Upstream Merge Scope Guide

> **Purpose**: Keep upstream synchronization focused on integration and conflict resolution.

## Owned Scope

An upstream merge or drift-repair task owns only:

- pinning and recording the exact upstream commit;
- creating the real merge and preserving upstream ancestry;
- carrying all non-conflicting upstream changes;
- making the minimal edits required to resolve concrete textual or semantic conflicts while preserving explicit fork decisions;
- validating merge history, remote safety, conflict resolutions, and regressions caused by the integration.

It does not own defects that already exist in the pinned upstream revision independently of the merge. Review findings do not expand this boundary merely because the affected code arrived from upstream.

## Classification Checklist

Before changing code for a finding discovered during an upstream merge, ask:

- [ ] Would the defect reproduce on the pinned upstream revision without the fork merge? If yes, it is an upstream-origin defect, not merge work.
- [ ] Is the edit required because fork and upstream changed the same behavior incompatibly? If yes, it may be conflict resolution.
- [ ] Is the proposed edit the smallest change that reconciles that conflict and preserves the chosen product behavior?
- [ ] Can the merge complete without this edit? If yes, do not include it merely as hardening, cleanup, or refactoring.
- [ ] Did a validation failure come from our conflict resolution, or directly from unchanged upstream behavior? Only the former belongs to the merge task.

Fork product-behavior conflicts still use the repository decision gate: pause with concrete file and behavior evidence before choosing a side.

## Handling Upstream-Origin Findings

- Record the pinned upstream SHA, affected files, reproduction or evidence, and impact.
- Mark the finding explicitly as outside the merge task scope.
- Schedule or open a separately authorized Trellis follow-up task through the normal task workflow.
- Do not place the fix in the merge task, merge commit, or conflict-resolution commit.
- Keep validation reports explicit about merge regressions versus known upstream-origin defects.

If an upstream-origin defect fails a required gate, report that fact without silently patching it in the merge task. Release or follow-up ordering is a separate decision; it does not widen the merge scope.

## Wrong vs Correct

**Wrong**: An upstream sync review discovers an unrelated Image Gen defect and fixes it on the merge task because the code was recently imported.

**Correct**: The merge task records the upstream-origin defect, completes only synchronization and conflict resolution, and leaves the product fix to a separate follow-up task.
