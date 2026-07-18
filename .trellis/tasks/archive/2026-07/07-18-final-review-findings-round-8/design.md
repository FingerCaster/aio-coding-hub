# Round 8 Design

## Boundary

Round 8 is an evidence-record correction, not a source-code or upstream-integration task. Its authority is
limited to the two archived statements that claim a historical user decision, the parent task's current
Round 7/8 projection, and new Round 8 task evidence.

## Evidence Rule

The authoritative negative evidence is the post-hoc audit at
`.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2/research/`
`upstream-merge-conflict-decision-audit.md:15-16`. It deliberately distinguishes final merge behavior from a
historical user choice. Therefore:

1. The PRD AC at upstream task line 72 and its matching implement decision-gate row at lines 26-27 become
   unchecked and explain why.
2. The merge SHA, parent relationship, final blobs, and all unrelated verified checks remain unchanged.
3. A conditional rollback row is not changed unless it directly asserts the missing historical user decision.

## Parent Projection

The parent task must describe state, not infer acceptance:

- Round 7: implementation and archive exist at `29133ac0`; its fresh Sol review found the unsupported
  decision assertion.
- Round 8: the next and only active corrective child; it remains incomplete until its own fresh Sol review
  passes.
- Parent: remains `in_progress`; it is not archived or completed as a side effect of either child archive.

## Rollback

If evidence later proves a historical user decision, restore only the two corresponding Markdown checkboxes
and evidence annotations in a new factual-record task. Do not rewrite the merge or product source.
