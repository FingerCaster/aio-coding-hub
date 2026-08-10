# Trellis Task Context Archive Contract

### 1. Scope / Trigger

Apply this contract whenever `task.py archive <task>` moves a Trellis task from
`.trellis/tasks/<task>` to `.trellis/tasks/archive/<YYYY-MM>/<task>`, or whenever JSONL context
validation/archive auto-commit behavior changes. The trigger includes active and already archived
`implement.jsonl` and `check.jsonl` manifests.

### 2. Signatures

```python
def _rewrite_archived_context_paths(
    original_task_dir: Path,
    archive_dest: Path,
    repo_root: Path,
) -> None: ...

def validate_all_context_manifests(repo_root: Path) -> int: ...

def validate_all_context_manifests_with_count(
    repo_root: Path,
) -> tuple[list[Path], int]: ...
```

`cmd_archive(args) -> int` must invoke rewrite and full validation after the move and before
`_auto_commit_archive`.

### 3. Contracts

- Parse each archived `implement.jsonl` and `check.jsonl` line independently as JSON.
- Rewrite only a string `file` equal to the old task prefix or beginning with the old prefix plus `/`.
- The replacement prefix is the actual monthly archive destination relative to repository root.
- Preserve unrelated task/spec paths and every `reason`, `type`, and unknown field value.
- Run the same recursive active-plus-archive validator used by `task.py validate --all` after rewriting.
- A parse or validation error returns non-zero and prevents archive auto-commit. The moved state remains visible
  for repair; the command must not hide the failure with a bookkeeping commit.
- The parent task is neither archived nor completed as an implicit consequence of archiving a child.
- A completed task's formal archive is the status/path authority, but a same-named active directory can shadow it
  in `task.py list`; before removing that active directory, compare every file and review semantic differences
  (research, decisions, acceptance evidence, and manifest references). Merge active-only material into the formal
  archive first, and remove the active copy only after the review records that no independent, non-superseded
  meaning remains.
- Never resolve a duplicate cleanup target with a broad glob or suffix-only deletion. Resolve and verify the exact
  in-repository path `.trellis/tasks/<name>` (and only that path) before removing it; leave concurrent or unrelated
  active tasks untouched.

### 4. Validation & Error Matrix

| Condition | Rewrite result | Command result / commit |
| --- | --- | --- |
| `file == old_prefix` | Replace with exact archive prefix | Continue to full validation |
| `file` begins `old_prefix + "/"` | Replace prefix, preserve suffix | Continue to full validation |
| Similar lexical prefix without slash boundary | No change | Continue |
| Unrelated spec/task path | No change | Continue |
| Blank JSONL line | Preserve blank line | Continue |
| Malformed JSON in archived manifest | No silent recovery | Non-zero; no auto-commit |
| Any active/archive manifest target missing | Rewritten files remain inspectable | Non-zero; no auto-commit |
| All manifests valid | Keep rewritten archive state | Auto-commit unless `--no-commit` |
| Completed archive plus same-named active directory | Treat the archive as status/path authority; review every file and merge non-superseded active-only material | Remove only the exact active path after recording the semantic review |
| Same-named active directory has unreviewed or unresolved unique material | Preserve both directories | Stop cleanup; do not remove or overwrite either copy |
| Concurrent or unrelated active task appears during cleanup | No rewrite or move | Leave its path and contents untouched |
| Cleanup name is not in the reviewed allowlist or is not one basename | Do not resolve or delete a target | Stop before `Join-Path`; reject separators and traversal forms |

### 5. Good / Base / Bad Cases

- **Good:** `.trellis/tasks/T/research/a.md` becomes
  `.trellis/tasks/archive/2026-07/T/research/a.md`, while a cross-layer spec reference remains byte-for-byte
  unchanged.
- **Base:** a task with no self-reference is moved, no manifest line changes, and full validation still runs.
- **Bad:** global string replacement changes `.trellis/tasks/T-other/...` or text inside `reason`.
- **Bad:** archive commits immediately after the move and leaves its own JSONL pointing at the deleted active
  directory.
- **Bad:** validation checks only the archived task and misses an invalid manifest elsewhere.
- **Good:** a completed archive and same-named active shadow are compared file by file; active-only research is
  merged into the archive, all manifests validate, and only `.trellis/tasks/<exact-name>` is removed.
- **Base:** no same-named active shadow exists; the completed archive remains unchanged.
- **Bad:** copying the whole active directory over the archive loses completed metadata or later archive design.
- **Bad:** a broad name/glob cleanup removes a concurrently created active task.

### 6. Tests Required

- In a temporary repository, create an active task containing an exact self path, a self descendant, a similar
  non-boundary prefix, an unrelated spec path, and preserved metadata fields.
- Invoke the production rewrite helper and assert only exact/boundary self references gain the monthly archive
  prefix.
- Create all referenced targets and assert the shared all-manifest validator returns zero.
- Add malformed JSON and a missing target separately; assert archive returns non-zero and auto-commit is not
  invoked.
- Run `python .trellis/scripts/task.py validate --all` against the real repository after task/spec edits and
  after archive.
- For manual duplicate reconciliation, assert the target has exactly one completed archive, the exact active path
  is absent after review, no active JSONL self-reference remains, and every concurrent task path/hash is unchanged.

### 7. Wrong vs Correct

```python
# Wrong: rewrites unrelated prefixes/reasons and commits without global validation.
text = manifest.read_text().replace(old_prefix, new_prefix)
manifest.write_text(text)
auto_commit()

# Correct: parse records, rewrite only the file field at an exact path boundary, then validate all manifests.
data = json.loads(line)
file_path = data.get("file")
if isinstance(file_path, str) and (
    file_path == old_prefix or file_path.startswith(f"{old_prefix}/")
):
    data["file"] = f"{new_prefix}{file_path[len(old_prefix):]}"
if validate_all_context_manifests(repo_root) != 0:
    return 1
auto_commit()
```

```powershell
# Wrong: suffix/glob deletion can remove an unrelated or concurrent task.
Get-ChildItem .trellis/tasks -Directory -Filter "*$name*" | Remove-Item -Recurse

# Correct: require one pre-reviewed name, then resolve one repository child and fail closed.
$approvedTaskNames = @("07-19-example-completed-task")
if ($name -notin $approvedTaskNames -or $name -ne [IO.Path]::GetFileName($name)) {
    throw "unapproved or non-exact task name"
}
$taskRoot = (Resolve-Path .trellis/tasks).Path
$target = [IO.Path]::GetFullPath((Join-Path $taskRoot $name))
if ([IO.Path]::GetDirectoryName($target) -ne $taskRoot) { throw "unsafe task path" }
Remove-Item -LiteralPath $target -Recurse
```
