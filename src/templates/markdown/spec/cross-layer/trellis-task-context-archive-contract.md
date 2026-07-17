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

### 5. Good / Base / Bad Cases

- **Good:** `.trellis/tasks/T/research/a.md` becomes
  `.trellis/tasks/archive/2026-07/T/research/a.md`, while a cross-layer spec reference remains byte-for-byte
  unchanged.
- **Base:** a task with no self-reference is moved, no manifest line changes, and full validation still runs.
- **Bad:** global string replacement changes `.trellis/tasks/T-other/...` or text inside `reason`.
- **Bad:** archive commits immediately after the move and leaves its own JSONL pointing at the deleted active
  directory.
- **Bad:** validation checks only the archived task and misses an invalid manifest elsewhere.

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
