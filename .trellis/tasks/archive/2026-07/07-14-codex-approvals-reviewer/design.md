# Design: Codex approvals reviewer setting

## Architecture

This is a narrow extension of the existing Codex config pipeline:

```text
config.toml
  -> Rust parser / CodexConfigState
  -> Specta-generated TypeScript state
  -> Codex settings selector + compatibility notice
  -> partial frontend patch adapter
  -> Rust CodexConfigPatch validation / root-key upsert
  -> atomic config.toml write
```

No new persistence store, command, settings table, or migration is needed.

## Backend Contract

### Types

Add `pub approvals_reviewer: Option<String>` next to `approval_policy` in both
`CodexConfigState` and `CodexConfigPatch`. The field remains a string rather
than a Rust enum to match the existing Codex config DTO pattern and to preserve
externally written unknown strings on reads.

### Parsing

Initialize the state field to `None` and parse the root
`approvals_reviewer = "..."` assignment with the existing string parser. Known
and unknown strings are returned verbatim. Invalid non-string values remain
outside the structured value and are caught by raw validation when a full-file
save is attempted.

### Structured Patching

Use `validate_enum_or_empty` with `user` and `auto_review`. When the patch field
is:

- omitted / `None`: leave the existing root key untouched;
- `"user"` or `"auto_review"`: upsert one root string key;
- empty: remove the root key;
- anything else: fail before producing output bytes.

The field does not participate in `patch_requires_provider_sync`.

### Raw TOML Validation

Add `validate_root_string_enum` for `approvals_reviewer` with the same two
allowed values. A full raw save containing an unknown value fails closed and
does not write. This differs intentionally from a structured patch of another
field, which preserves an existing unknown reviewer because the reviewer is not
part of that patch.

### Write Safety

Reuse the existing atomic-write and backup paths. No direct filesystem logic is
added. Tests must demonstrate that invalid raw TOML leaves the previous file
bytes unchanged.

## Generated IPC Boundary

Run the established Specta generator so both `CodexConfigPatch` and
`CodexConfigState` gain `approvals_reviewer: string | null`. Add a null default
to `DEFAULT_CODEX_CONFIG_PATCH`; public frontend patches remain partial and the
adapter supplies the generated required shape.

Typed test fixtures that construct a full state must add the field. Do not edit
generated bindings manually as the source of truth; generation owns the final
file.

## Frontend Design

### Pure Presentation State

Add a small pure module next to `CodexTab.tsx` to classify the two config
values. It returns semantic states rather than React or pre-styled markup:

- known reviewer: unset, `user`, or `auto_review`;
- unknown reviewer string;
- notice kind: none, inherited/neutral, inactive, or unsupported;
- whether the explicit `切换为 on-request` action is available.

Classification precedence:

1. Unknown reviewer -> unsupported-current-value state.
2. Unset reviewer -> no reviewer notice.
3. `auto_review + on-request` -> active/no warning.
4. `auto_review + unset policy` -> inherited/neutral notice.
5. `auto_review + never/untrusted/on-failure` -> warning + switch action.
6. `user + never` -> warning + switch action.
7. Other `user` combinations -> no warning.

Keeping this matrix pure makes all combinations independently testable and
keeps the already-large tab component from accumulating conditional branches.

### Selector

Render the selector immediately after `approval_policy`. Use the approved label,
subtitle, and three normal choices. If the current non-empty value is unknown,
append one synthetic option after the three normal choices whose value is the
exact current string and whose label marks it unsupported. A controlled select must never fall back
visually to the unset choice for an unknown value.

Selection persists immediately using the existing `persistCodexConfig` path.
`auto_review` does not invoke `confirmDesktopDialog`.

### Notices And Action

Render compact semantic status content within the setting row's control column:

- amber warning for inert/unsupported explicit combinations;
- muted neutral text when policy is unset/inherited;
- unsupported-current-value warning for unknown reviewer strings.

The switch action is a clear text command using the existing public `Button`
component and calls only:

```ts
persistCodexConfig({ approval_policy: "on-request" })
```

It is disabled while config saving is in progress. Passive renders and selector
changes never auto-edit the companion field.

The control column should use a stable max width and responsive wrapping so the
selector, warning, and action do not resize or overlap the settings row.

## Compatibility

- No config migration or default write occurs. Existing files remain unchanged
  until the reviewer selector is used.
- Existing unknown strings remain visible and survive unrelated structured
  patches, even though explicit raw full-file saves enforce the known enum.
- No CLI version gate is added, matching current AIO behavior for recent Codex
  fields.
- Custom Codex Home resolution, WSL full-file sync, proxy backup/restore, and
  project/managed configuration layers are not changed.
- Removing this UI later requires no data migration because written values are
  native Codex configuration.

## Risks And Mitigations

- **Misleading `never + auto_review`:** the explicit matrix warning prevents it
  from being presented as App-style automatic review.
- **Silent loss of future values:** synthetic unknown-value presentation and
  patch omission preserve externally written strings.
- **Accidental companion-key mutation:** a focused UI test asserts passive
  rendering and reviewer changes never patch `approval_policy`; only the button
  does.
- **Generated contract drift:** regenerate bindings and run the repository's
  generated-binding check.
- **Invalid full-file writes:** backend unit and integration tests assert
  validation happens before atomic write.

## Rollback

The code can be reverted without schema work. Existing native
`approvals_reviewer` keys remain valid Codex configuration and continue to be
preserved by AIO's generic TOML patcher.
