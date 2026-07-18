# Repository Rules

- For PR review, merge, release, and other repository operations, default to the `origin` remote. Do not inspect or operate on `upstream` unless the user explicitly requests upstream work.
- For GitHub CLI operations, do not rely on implicit repository resolution when both `origin` and `upstream` exist. Use `gh repo set-default FingerCaster/aio-coding-hub` for this clone and prefer explicit `--repo` / `-R FingerCaster/aio-coding-hub` on `gh` commands that mutate state or inspect Actions, releases, PRs, or issues.
- Release workflow builds must not checkout by release tag alone. Draft GitHub Releases can exist before their Git tag is fetchable; resolve or create the release tag first, then pass an immutable commit SHA to downstream build jobs.
- Keep `upstream` fetch-only for normal work. Do not restore an `upstream` push URL unless the user explicitly requests upstream push access.
- When the user explicitly requests upstream merge or drift repair work, carry forward non-conflicting `upstream/main` changes. If an upstream change conflicts with fork-specific product behavior or functionality, pause and ask the user with concrete file/behavior evidence and viable options before choosing either side.
- Keep upstream merge and drift-repair tasks integration-only. Make only the minimal changes required to resolve concrete textual or semantic conflicts and preserve an explicit fork decision. Do not fix defects that already exist in the pinned upstream revision independently of the merge, even when review or validation discovers them; record those defects as out-of-scope findings and handle them in a separately authorized follow-up task, not in the merge task or merge commit.
- For local commits from Codex/PowerShell in this clone, ensure the git hook environment can resolve `node` and `pnpm` before `git commit`. Do not hardcode machine-specific install paths; derive them from the current shell, for example: `$env:PATH = "$(Split-Path (Get-Command node -ErrorAction Stop).Source);$(Split-Path (Get-Command pnpm -ErrorAction Stop).Source);$env:PATH"; git commit ...`. The `.githooks/pre-commit` bash hook may otherwise fail to find `node`/`pnpm`.
<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

This project is managed by Trellis. The working knowledge you need lives under `.trellis/`:

- `.trellis/workflow.md` — development phases, when to create tasks, skill routing
- `.trellis/spec/` — package- and layer-scoped coding guidelines (read before writing code in a given layer)
- `.trellis/workspace/` — per-developer journals and session traces
- `.trellis/tasks/` — active and archived tasks (PRDs, research, jsonl context)

If a Trellis command is available on your platform (e.g. `/trellis:finish-work`, `/trellis:continue`), prefer it over manual steps. Not every platform exposes every command.

If you're using Codex or another agent-capable tool, additional project-scoped helpers may live in:
- `.agents/skills/` — reusable Trellis skills
- `.codex/agents/` — optional custom subagents

Managed by Trellis. Edits outside this block are preserved; edits inside may be overwritten by a future `trellis update`.

<!-- TRELLIS:END -->
