# Beta Release And Update Channel Contract

This contract owns the device-local Beta opt-in, channel-bound updater resources,
the manually published Beta Release, and the auditable `release-channels` pointer.
The stable release path remains owned by
[Release operations contract](./release-operations-contract.md).

## 1. Scope / Trigger

Apply this contract when changing any of the following:

- `UpdateChannel`, updater settings migration/import/export, or the dedicated
  channel writer.
- Desktop updater endpoints, manifest parsing, resource lifetime, query keys,
  background checks, install, or update UI.
- Beta release tags, version overlay, candidate promotion, `latest-beta.json`,
  `beta-channel-state.json`, or the `release-channels` branch.

Beta participation is explicit and device-local. Missing, old, imported,
unknown, corrupt, or unreadable settings authorize only `stable`.

## 2. Signatures

Generated IPC:

```text
settings_update_channel_set(
  channel: "stable" | "beta",
  confirm: RiskyIpcConfirm | null
) -> SettingsView

desktop_updater_check(
  expected_channel: "stable" | "beta",
  timeout: u64 | null
) -> DesktopUpdaterMetadata | null

desktop_updater_discard(rid: u32) -> bool
```

The install command is the single handwritten desktop IPC exception because it
contains a Tauri `Channel` callback:

```text
desktop_updater_download_and_install(
  rid: u32,
  on_event: Channel<DesktopUpdaterDownloadEvent>,
  timeout: u64 | null,
  confirm: RiskyIpcConfirm | null
) -> bool
```

Release workflow inputs:

```yaml
release.yml:
  release_channel: stable | beta # default stable
  release_tag: aio-coding-hub-vMAJOR.MINOR.PATCH[-beta.N]
  target_commitish: 40-hex origin/main-reachable SHA # required for Beta

beta-channel.yml:
  action: promote | pause
  release_tag: verified stable or Beta tag
  expected_ref_sha: 40-hex release-channels head, or absent initially
  withdrawn_tag: current unsafe tag # required for pause
```

## 3. Contracts

### Settings And UI

- `AppSettings.update_channel` defaults to `stable`. Direct IPC enum parsing
  rejects unknown strings; persisted unknown strings normalize to `stable`.
- Entering Beta requires `action=settings_update_channel_set` and
  `resource=update_channel:beta`. Exiting requires no risky confirmation.
- Ordinary `SettingsUpdate` and `SettingsPatch` do not own `update_channel`.
  Export and import force it to `stable`; migration from a schema before the
  field existed must not preserve an injected Beta value.
- A settings read error immediately closes the dialog, removes channel-bound
  candidates/resources, increments the renderer generation, and leaves the UI
  `stable` but not ready. A later successful canonical read may recover Beta.
- Query keys, in-flight checks, cached candidates, dialogs, and last-check
  timestamps are channel/generation scoped. Late results are discarded and
  never cross the stable/Beta boundary.
- `channel` describes the subscription/cache boundary. `isPrerelease`
  describes the selected Release and owns Beta labeling. A Beta subscription
  may select a later stable Release without labeling it as a Beta update.

### Updater

- Stable keeps the configured endpoint. Beta uses only
  `https://raw.githubusercontent.com/FingerCaster/aio-coding-hub/release-channels/latest-beta.json`
  plus the backend-owned `aioCheck` cache buster. No settings value can replace
  either endpoint.
- `DesktopUpdaterMetadata` requires `rid`, `channel`, `isPrerelease`,
  `currentVersion`, `version`, `releaseUrl`, `date`, and `body`.
- Beta accepts canonical `MAJOR.MINOR.PATCH-beta.N` and a higher canonical
  stable version. Stable never accepts a prerelease. Release URLs are exact
  `https://github.com/FingerCaster/aio-coding-hub/releases/tag/<canonical-tag>`.
- Beta `Update.raw_json` is the exact static schema
  `version/notes/pub_date/platforms`. `platforms` contains exactly
  `windows-x86_64`, `darwin-x86_64`, `darwin-aarch64`, and `linux-x86_64`;
  every entry contains only its canonical Release asset URL and non-empty
  signature.
- A `rid` is one-shot. The backend consumes it before confirmation, fresh
  check, download, or install. On any install failure the renderer removes the
  spent candidate and performs a fresh check before enabling retry.
- Every resource records channel and transition epoch. Cleanup holds the
  transition guard, reads the latest canonical pair, and discards only
  resources whose pair differs. A canonical read failure discards all updater
  resources.
- Beta install performs a fresh check and requires identical version, target,
  URL, and signature. A final guarded channel/epoch check occurs after download
  and before install.

### Release And Pointer

- Beta tags are manual-only
  `aio-coding-hub-vMAJOR.MINOR.PATCH-beta.N`, without leading zeroes and with
  `N >= 1`. The source is an immutable 40-hex SHA reachable from `origin/main`.
- All build jobs checkout the verified SHA, apply the deterministic four-file
  version overlay, and carry source/tag/version/overlay digest through the
  candidate and promotion attestations.
- Beta publishes exactly the official 14 signed assets as a public
  `prerelease=true`, `draft=false`, `make_latest=false` Release. It never runs
  Homebrew publication or changes stable `latest.json`/GitHub latest.
- `release-channels` contains only `latest-beta.json` and
  `beta-channel-state.json`. Manifest bytes equal the verified Release
  `latest.json`; state binds the previous ref, selected Release/source,
  manifest SHA-256, action, run identity, operator, and UTC time.
- Pointer writes use the Git Data API with the old commit as parent and
  `force=false`. `expected_ref_sha` is a compare-and-swap precondition. Pause
  selects a previously verified safe stable/Beta Release; it never moves a tag
  or mutates Release assets.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Missing/unknown/corrupt settings channel | Normalize renderer and persisted settings to stable; do not request Beta |
| Beta opt-in lacks the exact confirmation | `SEC_CONFIRM_REQUIRED` or confirmation mismatch; no channel change |
| `expected_channel` differs from canonical channel/epoch | `UPDATER_CHANNEL_CHANGED`; no fallback |
| Beta endpoint/build/check fails | Typed updater error; no stable fallback and no candidate |
| Dynamic, partial, extra, `v`-prefixed, or wrong-asset Beta manifest | `UPDATER_MANIFEST_INVALID` |
| Resource is absent, wrong type, or already consumed | `UPDATER_RESOURCE_CLOSED` or `UPDATER_RESOURCE_INVALID` |
| Beta pointer changed before install | `UPDATER_CANDIDATE_STALE`; consume the old resource |
| Download/install fails after resource take | Keep the error, remove the candidate, and fresh-check for a new `rid` |
| Late cleanup races with a newer channel transition | Re-read channel+epoch under the guard; preserve the newer valid resource |
| Beta tag/source/asset/Release identity differs at any gate | Abort before publication or pointer update |
| Pointer CAS loses or pause target is not verified/current | Fail closed; do not force-update the branch |

Frontend code branches on typed error prefixes, never on prose after the
prefix.

## 5. Good / Base / Bad Cases

- Good: a confirmed device moves to Beta, receives `0.60.41-beta.3`, and later
  receives stable `0.60.41`; the second candidate is labeled as a normal update
  while participation remains enabled.
- Good: an install download fails after consuming `rid=21`; the cache removes
  21, a fresh check stores `rid=22`, and retry uses only 22.
- Base: a new/old/imported device stays stable, never requests
  `latest-beta.json`, and never renders a Beta toast, dialog, badge, or portable
  link.
- Bad: infer prerelease UI copy from `channel=beta`. The Beta pointer may
  legitimately select a stable final Release.
- Bad: clean up every resource from the channel requested by an old command.
  A newer transition may already have returned to that channel with a new epoch.
- Bad: retry an install with the same `rid`; every install attempt consumes it
  before any other validation.

## 6. Tests Required

- Rust settings: missing/unknown persisted values, direct unknown IPC enum,
  old-schema injected Beta, confirmed opt-in, ordinary writer exclusion, export
  normalization, import normalization, rollback, and concurrent winner.
- Rust updater: fixed endpoint, exact static manifest and four-platform asset
  matrix, stable-on-Beta lifecycle, leading-zero/`v` rejection, channel/epoch
  mismatch, wrong/closed resource, pause/fresh-check identity, switch during
  fresh check/download/install, and delayed cleanup preserving a newer epoch.
- Frontend: isolated query keys/generations, settings error fail-closed and
  recovery, stale settings response after a writer, import reread failure,
  install failure acquiring a new `rid`, opt-in confirmation/cancel/failure,
  opt-out cleanup, and stable Release labeling on a Beta subscription in About,
  dialog, and sidebar.
- Release: source, tag/channel, version overlay, exact 14 assets, promotion,
  signing-secret scope, strict UTF-8 manifest/signatures, pointer state/parent,
  CAS race, pause, support matrix, stable default, Homebrew, and CI scope
  self-tests.
- Regenerate bindings and run frontend type/lint/unit tests, Rust fmt/check/
  Clippy/tests, release self-tests, and `git diff --check`.

## 7. Wrong vs Correct

### Wrong

```ts
const isBetaUpdate = candidate.channel === "beta";

try {
  await install(candidate.rid);
} catch {
  // Keep candidate so the user can retry the same rid.
}
```

### Correct

```ts
const isBetaUpdate = candidate.isPrerelease;

try {
  await install(candidate.rid);
} catch (error) {
  removeChannelCandidate(candidate.channel);
  await freshCheck(candidate.channel); // provisions a different one-shot rid
  throw error;
}
```

### Wrong

```rust
discard_resources_for_channel(channel_requested_by_the_old_command);
```

### Correct

```rust
let _guard = lock_update_channel_transition();
let (channel, epoch) = read_latest_canonical_channel_and_epoch()?;
discard_resources_where(|resource| (resource.channel, resource.epoch) != (channel, epoch));
```
