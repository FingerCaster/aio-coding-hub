# Round 7 Design

## Scope And Ownership

This task owns four independent corrections on the frozen `35db0f32` branch. They are implemented in one
serial child because they share the same final acceptance gate, but their contracts and tests remain separate.
The only permitted future reviewer is Codex `gpt-5.6-sol / effort=max`.

## Image Gen: Immutable Retry Attempts

The backend intentionally owns one new task directory per task ID and rejects an existing ID. Do not add an
upsert or replacement path. `retry` instead reads the original task snapshot and any persisted reference images,
then appends a new loading task with a fresh random ID and a new creation/start timestamp. The original task stays
in the store and, when persisted, remains a valid history row.

`runGeneration` and `persistTask` operate only on the new ID. A successful retry therefore uses the ordinary
create-only persistence path. A failed retry creates or retains only its own attempt state and cannot mutate the
source task. The source task is never deleted as a cleanup shortcut. Tests cover persisted done, persisted error,
missing reference, failed retry, and an earlier persistence promise resolving after a retry starts.

## Config Export: Shared Bounded Collector

Create an export-scoped budget object in the config-migration layer and pass it through both installed and local
Skill exporters into each `SkillFileCollector`. It owns checked counters for encoded payload bytes and file count.
The counters are shared for the full `config_export` invocation, not reset per Skill or per root.

Before Base64 allocation, the collector reserves the prospective encoded payload and one file slot. The limits are
56 MiB encoded payload and 2048 files. A per-file bounded raw read may occur before exact byte reservation, but no
file that would exceed the aggregate may be Base64 encoded or appended. Retain existing per-Skill 8 MiB and 256-file
limits, trusted-root/handle rules, and the 64 MiB final pretty-JSON writer. The final writer still handles all
non-Skill configuration data; the new budget prevents an unbounded Skill-only build-up.

## Settings: Generation-Gated Field Restore

`rollback_whole_settings_with_auto_start_token` already obtains the owner lock and knows whether its token owns the
current generation. Treat that result as authoritative for `auto_start` in both the whole-snapshot fast path and
the field-aware path. When ownership is stale, skip only `auto_start`; continue restoring other fields that still
equal the import's committed values. The returned result must make OS autostart converge to the resulting canonical
winner rather than the import's previous value.

Add deterministic config-import tests that place a real ordinary settings writer during runtime sync. The writer
uses the same `auto_start` value as the import and a newer ordinary field. A second test covers a generation advance
with a snapshot otherwise equal to the import. Both assert the newer autostart value remains while import-owned
ordinary fields follow their documented rollback behavior.

## Archive Fact Reconciliation

Inspect existing commits and archived evidence first. Change only `[ ]` markers that correspond to work already
completed by each of child tasks 1-5. Do not infer or fabricate `task.json.commit` values. Run the repository-wide
task validator and a narrow checkbox audit afterwards. This is data reconciliation, not a generic Trellis CLI change.

## Rollback

- Image Gen: revert the fresh-attempt frontend change as one unit; no storage migration exists because old rows are
  untouched.
- Export: remove the shared budget object and its tests together; no schema/data migration exists.
- Settings: revert the generation guard and deterministic test together; runtime state remains governed by existing
  canonical convergence paths.
- Archive records: restore only the changed Markdown checkboxes if evidence review reveals an incorrect mark.
