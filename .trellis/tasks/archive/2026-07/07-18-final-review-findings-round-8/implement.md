# Round 8 Implementation Plan

## Scope Lock

- [x] Use only the approved Round 8 PRD/design scope. Do not inspect or modify product runtime code, perform
      upstream operations, broaden to conditional rollback statements, or search for unrelated findings.
- [x] Use one fresh Orca-managed Codex `gpt-5.6-sol / effort=max` execution terminal. Do not reuse it for
      final review and do not launch any concurrent implementation or review agent.

## Evidence and Record Correction

- [x] Re-read the Round 7 final review result, the upstream task PRD/implement rows, and the line-15/16 audit
      limitation before editing.
- [x] Change only the two user-decision assertions to unchecked with a concise, evidence-backed explanation.
- [x] Update the parent task map/checklist to reflect Round 7 archive plus P2 and the newly created incomplete
      Round 8 child without marking the parent or Round 8 complete.
- [x] Add a compact Round 8 research note recording the final-review conclusion and exact evidence anchors.

## Validation and Finish

- [x] Verify `git diff --name-only` contains only `.trellis/tasks/` artifacts for this task, the upstream task,
      and the parent task; run `git diff --check`.
- [x] Run `python .trellis/scripts/task.py validate --all`; do not run unrelated build/test gates because no
      production code or test inputs changed.
- [x] Commit the factual-record correction, archive only Round 8, and confirm the execution worktree is clean.
- [ ] Freeze the archive commit and request a separate fresh Sol max read-only final review. Only after no
      P0-P2 may the coordinator update the parent final facts; never merge `main` without user confirmation.
