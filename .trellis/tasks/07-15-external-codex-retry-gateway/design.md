# Design: External Codex retry gateway integration

## Decision Summary

- Replace AIO's in-process Codex reasoning guard and continuation repair with a
  native integration for the third-party `nonononull/codex-retry-gateway`.
- Keep interception behavior, configuration, analytics, and its management UI
  owned by the external project. Keep source trust, Node discovery, process
  lifecycle, Codex routing, health, recovery, and update transactions owned by
  AIO.
- Do not extend the plugin system for this feature. Its current public contract
  cannot safely own arbitrary process execution, source download, Codex config,
  or custom-page embedding.
- Treat the external gateway as an enhanced mode of the host-native Codex CLI
  proxy. The supported route modes are unproxied, direct AIO, and guarded AIO.
- Use an AIO-owned loopback management bridge for both the embedded view and
  system-browser entry. The bridge validates the managed instance, protects
  AIO-owned config fields, and replaces the external whole-file restore action
  with AIO's chain-aware restore transaction.
- Support `remote_compaction` changes in every route mode. AIO remains the only
  owner of Codex config projection and keeps `aio`/`OpenAI`, route target,
  provider-sync data, and rollback state coherent.
- Do not bundle external source or Node.js. Download a user-approved immutable
  official-main commit at runtime and require a re-probed Node.js 18+ binary.

The researched external baseline is
`ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2`. It is only the initial
recommendation candidate. A release may label it reviewed only after the
implementation review and compatibility gates in `implement.md` pass.

## Ownership Boundaries

| Owner | Responsibilities |
| --- | --- |
| External gateway | Request inspection, interception/retry semantics, external config schema, logs, analytics, probes, and management-page implementation |
| AIO infrastructure | Official-commit resolution, bounded download/extraction, source manifests, Node resolution, child process control, loopback bridge, and owned-state files |
| AIO application service | Serialized lifecycle transitions, startup/exit recovery, health supervision, update rollback, status events, and stable error mapping |
| Codex route coordinator | Canonical config backup, route projection, CLI-proxy state, `remote_compaction` provider sync, and crash-safe transition journal |
| AIO frontend | Confirmation, desired/actual status, WSL warning, details navigation, update selection, retry, and uninstall controls |

The external install, launch-ui, and restore entry points are not used as Codex
configuration owners. AIO may reuse the external gateway entry point or its
start helper to run the pinned source, but it supplies an AIO-owned config and
state root and never delegates restoration of the live Codex file.

## Module Shape

Use existing root-package layering rather than adding a privileged plugin API:

```text
src-tauri/src/infra/codex_retry_gateway/
  source.rs             official SHA resolution, archive verification, cache
  node.rs               automatic/manual Node discovery and version probe
  managed_state.rs      bounded, versioned AIO ownership records
  process.rs            spawn, health identity, stop, and process inspection
  bridge.rs             authenticated loopback UI/API reverse proxy
  config.rs             external config and compatibility-state projection

src-tauri/src/app/codex_retry_gateway_service.rs
  lifecycle operations, supervisor, startup/exit reconciliation, events

src-tauri/src/commands/codex_retry_gateway.rs
  typed Tauri command boundary

src-tauri/src/infra/cli_proxy/
src-tauri/src/infra/codex_config/
  chain-aware Codex route projection and provider-mode transaction
```

Exact file splitting may follow source-size conventions discovered during
implementation, but ownership must not drift: frontend code does not parse
external health payloads, command handlers do not implement filesystem or
process policy, and external JavaScript never owns AIO settings.

## Network Topology

```text
Host Codex config
  model_provider = aio | OpenAI
  base_url = effective route
        |
        | Guarded mode only
        v
127.0.0.1:<external-port>  codex-retry-gateway
        |
        | fixed upstream_base_url; never derived from live Codex config
        v
127.0.0.1:<aio-port>/v1    AIO gateway
        |
        v
selected AIO provider
```

- The external listener is always `127.0.0.1`, preferably port `4610`.
- The upstream is the independently obtained AIO loopback origin. It is never
  read back from the active Codex provider after that provider points to the
  external listener; this prevents proxy recursion.
- External and AIO origins are distinct typed values and are verified before a
  route commit.
- WSL Codex continues to use its existing Windows-host AIO origin. It never
  receives the host-loopback external origin in version one.
- The management bridge uses a separate AIO-owned `127.0.0.1` listener. AIO
  never proxies management APIs onto its WSL/LAN-reachable gateway listener.

## Persisted Contracts

### App settings

Add only user-owned choices to the canonical settings schema:

```text
codex_retry_gateway_enabled: bool                 desired state
codex_retry_gateway_selected_commit: string       full 40-hex SHA
codex_retry_gateway_preferred_port: u16            default 4610
codex_retry_gateway_node_override: string          empty means automatic
```

The selected commit defaults to the recommendation embedded by the current AIO
build. Invalid legacy or manual values are not silently normalized to another
commit; they produce a validation state until the user selects a valid commit.
Actual runtime health is never stored as a Boolean setting.

### AIO-owned data root

Use an application-data subtree that cannot collide with the external default:

```text
codex-retry-gateway/
  manager.json                 schema, repo identity, active/previous SHA
  sources/<full-sha>/          immutable extracted snapshots
  downloads/.staging-*/       bounded transactional extraction
  runtime/config/config.json  external gateway config
  runtime/state.json          compatibility projection, no restorable backup
  runtime/gateway.pid         external-compatible PID record
  runtime/logs/
  runtime/analytics/
  route/transition.json       pending route/lifecycle transition journal
```

`manager.json` records source fingerprints, verification time, verified
official-main SHA, Node/process identity, effective port, and the previous
known-working source. Paths are reconstructed under the trusted root rather
than accepted from stored arbitrary absolute paths.

The external-compatible `runtime/state.json` may contain current provider name,
route, paths, and an AIO instance nonce so status remains useful. It must omit a
usable `latest_backup_path`. Direct access to the raw external restore endpoint
therefore fails before modifying Codex; normal AIO-provided UI/browser access
is intercepted by the management bridge.

### Runtime-only state

The application service keeps the child handle when available, supervisor task,
bridge address/session secret, current operation ID, recovery counter, next
retry time, and latest parsed health in managed Tauri state. These values are
reconstructed and verified after application restart rather than trusted from
settings.

## Route and Runtime State Model

`CodexRouteMode` is exhaustive:

| Mode | CLI proxy manifest | Live host Codex target | External process |
| --- | --- | --- | --- |
| `Unproxied` | disabled | user canonical route | stopped or being reaped |
| `DirectAio` | enabled | verified AIO origin | stopped, or temporarily bypassed |
| `Guarded` | enabled | verified external origin | owned and healthy, upstream is AIO |

Desired external state and actual route are separate. The public runtime phase
must distinguish at least:

```text
disabled | preparing | starting | guarded | bypassed_recovering |
recovery_paused | stopping | uninstalling | error
```

`enabled=true` plus `route=DirectAio` is a valid degraded state and is displayed
as bypassed/recovering, never as protected. `enabled=false` plus a still-running
process is a cleanup failure, not an active gateway.

Every status projection includes a monotonically increasing generation so
frontend event/query races cannot replace newer state with stale state.

## Serialized Lifecycle Coordinator

Extend the existing gateway lifecycle critical section to cover every
chain-affecting entry point instead of introducing locks with ambiguous order:

- external gateway enable/disable/retry/update/uninstall;
- Codex CLI-proxy enable/disable and gateway-port rebind;
- `remote_compaction` provider-mode changes and raw config writes that touch the
  managed provider/route;
- startup, application exit, external restore, crash fallback, and recovery.

Long network download and archive staging may happen before the critical
section. The service assigns an operation ID, stages immutable inputs, then
acquires the lifecycle lock and revalidates desired state, generation, AIO
origin, source manifest, Node binary, and port before any route mutation. A
stale operation cannot commit after a newer disable or selection.

For multi-file/process transitions, write `route/transition.json` before the
first mutation. It contains the operation kind, prior committed generation,
target generation, prior and target modes, file hashes/snapshot references, and
source/process intent. Startup reconciliation either verifies and commits the
target or converges to the safest prior/direct state. It never assumes a
half-written settings Boolean proves the live route.

## Transition Transactions

### Enable external gateway

1. Build a read-only enable plan containing current CLI-proxy state, current
   provider, target provider (`aio` or `OpenAI`), whether Provider Sync is
   required, source/download trust, Node status, port, and WSL coverage. If the
   provider changes, the consolidated confirmation names the exact transition,
   session/provider-state synchronization, backup, and Codex-closed requirement.
2. Revalidate the plan in the backend after confirmation. If the CLI proxy was
   disabled, require the accepted combined-enable flag and record `Unproxied`
   as rollback. If Provider Sync is required, require its accepted flag and
   verify Codex is closed before source, process, or config mutation.
3. Resolve and probe Node, validate or install the selected immutable source,
   ensure the AIO gateway is running, and choose an available loopback port.
4. Write the external config with protected listener, health path, and AIO
   upstream. Write compatibility state for the current `aio`/`OpenAI` provider.
5. Start the process and require PID, process identity, listen address, state
   root, config path, source SHA, instance nonce, and upstream health to match.
6. If the provider changes, invoke the existing Provider Sync transaction with
   the target routed config so session files, SQLite/global state, backup, and
   live provider identity change together. Otherwise apply the normal route
   projection. Verify the live provider and external route.
7. Commit CLI-proxy enabled state, desired external state, effective port,
   active source, and `Guarded` generation.
8. On failure, restore the exact prior route and provider-sync snapshots. Stop
   only the verified process and
   leave an already-enabled CLI proxy in direct-AIO mode; if this operation
   enabled the CLI proxy, restore the original unproxied state.

### Disable external gateway only

1. Project and verify `DirectAio` while the external process is still healthy.
2. Commit desired state off and direct-AIO route.
3. Stop only the fully verified owned process. A stop failure leaves Codex
   safely direct to AIO and surfaces cleanup-needed state; it never reroutes
   traffic back to the unwanted process.
4. Preserve source, external config, logs, analytics, and rollback source.

### Disable Codex CLI proxy

1. If external desired state is on, use the same coordinator and set it off.
2. Restore and verify the canonical unproxied Codex config while AIO and the
   external process are still available.
3. Commit CLI-proxy disabled and external desired state off.
4. Stop/reap the verified process. A cleanup failure cannot change the restored
   unproxied route.

### Update selected commit

1. Resolve official `main`, validate ancestry, download/extract/validate the
   candidate into a new immutable source directory, and keep the live process
   untouched.
2. Enter `DirectAio`, stop the old process, start and fully verify the candidate,
   then return to `Guarded` and commit the active pointer.
3. If the candidate fails, stop it, restart and verify the previous known-good
   source, restore `Guarded`, and retain the failed candidate only as staged
   diagnostic data until cleanup policy runs.
4. If both candidate and rollback source fail, remain direct AIO with desired
   state on and `recovery_paused`; do not point Codex at a dead listener.

### Application exit and restart

On a normal application exit, preserve the desired setting but restore a safe
unproxied Codex file, stop the external process, and then stop AIO. This mirrors
the existing keep-enabled CLI-proxy cleanup contract and avoids leaving Codex
pointed at a stopped AIO process.

On startup: start AIO first, repair any pending transition, then reconcile the
host Codex route. If desired external state is on, validate cached source and
Node, start/verify the external process, and apply `Guarded`. If restoration
fails, apply `DirectAio`, keep desired state on, expose recovery state, and
continue application startup. WSL finalization runs against AIO, never the
external port.

## Codex Config Projection and `remote_compaction`

The CLI-proxy backup is the canonical unproxied config, not a copy of whichever
routed file happens to be live. Route projection starts from canonical bytes
and applies only AIO-owned overlays:

- effective provider key: `OpenAI` when `features.remote_compaction=true`,
  otherwise `aio`;
- effective provider `base_url`: user canonical route, AIO, or external based
  on `CodexRouteMode`;
- existing CLI-proxy auth strategy and Windows sandbox fields;
- no local reasoning-guard fields.

Structured AIO Codex changes first update canonical semantics, then produce the
live projection for the current mode. Route fields are never learned by copying
the live external URL back into the canonical backup.

For a `remote_compaction` change:

1. Acquire the shared lifecycle section and preserve the existing Codex-closed
   provider-sync requirement.
2. Snapshot canonical config, live config, CLI-proxy manifest, compatibility
   state, and the provider-sync transaction inputs.
3. Build and validate the next canonical provider identity and current-route
   projection before any write.
4. Update the rollback representation, then run the existing provider-sync
   transaction with the routed live bytes so sessions, SQLite rows, workspace
   state, and `config.toml` agree.
5. Update the external compatibility provider name/route, verify live config
   and health/status, and commit the transition generation.
6. A normal provider rename does not restart the external gateway; its upstream
   remains AIO. If any pre-commit step fails, restore every snapshot and keep
   the prior route and desired state.

Gateway enable uses the same provider-sync core when its route projection would
change provider identity. Add a pure `CodexProviderSyncPlan` projection with
current provider, target provider, change-required flag, and Codex-process
precondition. It is advisory for confirmation only; the backend recomputes it
under the lifecycle critical section before accepting a confirmation token.
The existing manual Provider Sync action remains available and shares result/
error formatting with the gateway flow.

Raw TOML saves that alter `model_provider`, `features.remote_compaction`, or a
managed provider table use the same coordinator or fail with a stable busy/
ownership error. They cannot bypass route projection while the CLI proxy is
enabled.

## Source Trust and Installation

### Candidate resolution

- The repository identity is compiled as
  `nonononull/codex-retry-gateway`; callers never supply a URL.
- Manual input is an exact 40-hex SHA. Prefer the user's local Git executable
  with an AIO-owned bare cache of the fixed official repository, then use the
  fetched commit graph to prove the candidate is `main` or its ancestor at that
  verification time. Never trust or reuse an existing user checkout.
- Git execution disables credential prompts, hooks, system/global config,
  protocol substitution, replace refs, grafts, alternates, and shallow graphs.
  If Git is unavailable, GitHub commit/compare REST is a limited fallback;
  Git execution/network failures stay actionable Git errors and do not silently
  change verification mode.
- Only the fixed official Git HTTPS URL, GitHub API fallback, and
  GitHub/codeload download hosts are allowed. Redirects to
  other hosts, forks, branch-only commits, abbreviated SHAs, and mutable ref
  execution are rejected.
- Store the resolved main SHA and verification time with the source manifest.

### Bounded extraction

Reuse or factor the existing GitHub archive and plugin-package safeguards:

- bounded response, timeout, redirect count, archive entries, per-file size,
  expanded total size, and compression ratio;
- accept GitHub codeload's explicit single root-directory entry while rejecting
  top-level files or archives with more than one root directory;
- reject absolute paths, traversal, symlink/reparse entries, duplicate
  normalized paths, and multiple unexpected roots;
- require expected gateway/start-script layout and run bounded Node syntax
  checks before promotion;
- compute and persist archive/source fingerprints, then atomically rename to
  `sources/<full-sha>`;
- revalidate cached manifests and required files before every execution.

A same-SHA repair may redownload after prior approval. Selecting another SHA is
an update and always requires the accepted confirmation flow.

### Trust labels

- Exact equality with the SHA embedded in the current AIO build is
  `aio_reviewed_recommendation`.
- Official `main` HEAD or another official-main ancestor is
  `official_main_unreviewed` unless its exact SHA is embedded.
- The UI always shows repository, full SHA, trust state, and undeclared license
  metadata. AIO does not present itself as publisher of the external source.

## Node and Process Management

- Automatic resolution reuses existing CLI-manager path discovery, preferring
  a Node executable adjacent to the detected Codex/npm runtime, then established
  cross-platform locations and process PATH.
- A manual override must be an absolute regular executable path and pass the
  same bounded direct `--version` probe. Parse the major version and require
  18+. Re-probe before every start/update.
- Spawn the exact resolved executable with an argument vector; never use a
  shell command string. Set a controlled working directory and runtime PATH,
  hide child windows on Windows, bound startup output, and avoid leaking the
  parent environment in diagnostics.
- Record PID plus process start identity, executable, source path, config path,
  state root, commit, instance nonce, and listen/upstream values. Health
  `process_id` must equal the expected PID.
- Stop or reuse only when every available identity signal matches. A stale PID
  file or occupied port is insufficient authority. Non-owned listeners are
  reported as conflicts and never killed.

The supervisor probes on a bounded interval. On unhealthy transition it first
commits `DirectAio`, then attempts serialized restart with bounded exponential
backoff and storm protection. Success requires a new full identity check before
returning to `Guarded`; repeated failure enters `recovery_paused` with explicit
retry and disable actions.

## Port Selection

- Prefer the persisted effective port, otherwise `4610`.
- Probe loopback occupancy before start. Reuse only a fully matching AIO-owned
  process; otherwise select another free loopback port without touching the
  occupant.
- Persist a new fallback port only after process health and guarded-route
  verification. Failed activation leaves the previous persisted port intact.
- A runtime port move uses the same direct-AIO/start/verify/guarded transaction
  as an update.

## Management Bridge

The bridge is a dedicated AIO-owned loopback HTTP server. Both iframe and
system-browser entry first use a short-lived launch URL that establishes an
HttpOnly, SameSite session cookie and redirects to the external UI path. API
requests without a current bridge session are rejected. Sessions expire on app
restart, uninstall, or ownership change.

Before each proxied request, validate the active process generation and raw
external health identity. Use an explicit method/path allowlist and bounded
request/response streaming; do not create an arbitrary local reverse proxy.

Bridge behavior:

- `GET /__codex_retry_gateway/ui`: proxy the external HTML after ownership
  validation.
- status APIs: proxy and overlay AIO-authoritative provider, route, commit,
  desired/actual state where needed for compatibility.
- config writes: structurally parse input and require AIO-owned
  `listen_host`, `listen_port`, `upstream_base_url`, and `health_path` to equal
  their managed values. Reject attempts to change them; forward external-owned
  behavior fields and re-probe health after hot apply.
- restore: do not forward. Invoke gateway-only disable through the lifecycle
  coordinator, return a compatible success/error payload, and let the bridge
  remain alive long enough for the page to render the result.
- logs, analytics, imports, exports, and probes: forward only their known paths
  with explicit body/response/time limits. Popup-dependent flows may require
  the system-browser view.

The Tauri CSP changes from `frame-src 'none'` to only
`http://127.0.0.1:*`. The iframe URL is accepted only from the typed bridge
command, and the frame sandbox allows scripts, same-origin, forms, modals, and
downloads but not Tauri IPC, top navigation, or unrestricted popups. Chromium
may label iframe API requests cross-site relative to the Tauri top-level page;
the bridge accepts only an exact loopback Origin or loopback Referer proof for
those GETs, while mutations remain exact-Origin protected.

Leaving the details route unloads the frame and revokes that view session if
appropriate; it does not change desired state or stop the process. The outer
page keeps back, refresh, open-browser, update, retry/stop, and status controls
outside the frame. Details entry is disabled unless the gateway is desired-on
and its management bridge is available. Both details-page exit controls return
to `/cli-manager?tab=codex`, which initializes the Codex tab explicitly.

## Frontend and IPC Contract

Generate all DTOs from Rust. The main status type includes:

```text
generation, desired_enabled, runtime_phase, route_mode,
cli_proxy_enabled, cli_proxy_applied, effective_port,
selected_commit, active_commit, previous_commit, recommended_commit,
trust_state, node_status, process_status, update_candidate,
wsl_codex_unprotected, last_error, details_available
```

Commands are narrow capabilities, not generic process/file/network APIs:

```text
codex_retry_gateway_status
codex_retry_gateway_enable_plan
codex_retry_gateway_set_enabled
codex_retry_gateway_check_update
codex_retry_gateway_validate_commit
codex_retry_gateway_apply_commit
codex_retry_gateway_set_node_override
codex_retry_gateway_retry
codex_retry_gateway_uninstall
codex_retry_gateway_create_details_session
```

The enable command requires explicit confirmation flags for first download,
unreviewed source, implicit Codex CLI-proxy enable, provider-name/session sync,
and detected WSL coverage gap. The request also carries the plan generation;
backend recomputation rejects stale confirmation. Hiding a frontend dialog
cannot bypass consent.

Use one consolidated enable dialog whose rows are conditional. A provider-sync
row shows `current -> target`, states that existing Codex sessions/provider
records will be synchronized with a backup, and requires Codex to be closed.
After success, summarize the existing `CodexProviderSyncResult` rather than
claiming only that the gateway switch changed.

Replace the local guard controls/statistics in the Codex tab with one compact
gateway section. Show desired and actual state separately, effective host route,
full commit/trust, Node status, port, and persistent WSL coverage warning. The
update button checks only; apply occurs from a second explicit confirmation.

## WSL Boundary

- Detect the existing WSL Codex auto-configuration target from AIO settings.
- Host enable remains allowed after a non-blocking explicit warning.
- Status and details surfaces keep a visible `WSL Codex is not protected`
  state while the target is active, including after restart and runtime
  recovery.
- Do not alter WSL route, install Node/source in distributions, or bind the
  external/bridge listeners beyond Windows loopback.

## Migration and Local-Guard Removal

- Remove all `codex_reasoning_guard_*` schema fields, runtime branches,
  continuation modules, dedicated statistics/query/UI paths, generated types,
  tests, scripts, active docs, and release rules listed in `inventory.md`.
- Old settings keys are ignored on read and disappear on the next canonical
  settings write. They are not translated to external config.
- Keep generic request-log rows and `special_settings_json`; historical guard
  markers become inert data and age out normally.
- Preserve generic transient retries, OAuth refresh, provider-scoped
  `previous_response_id` recovery, failover/circuit behavior, model-route
  diagnostics, normal request logs, Claude/Gemini CLI proxying, and unrelated
  Codex config fields.
- Regenerate bindings from Rust. Never edit generated declarations as the
  source of truth.
- Preserve existing user changes in `AGENTS.md` while surgically removing only
  the obsolete local continuation-release rule. Do not touch the untracked
  analysis HTML.

## Error and Diagnostic Contract

Map internal failures to stable categories such as source resolution/archive,
Node missing/unsupported, port conflict, ownership mismatch, health timeout,
route apply/verify, provider sync, bridge session, update rollback, and cleanup.
Public messages contain safe paths/version/SHA/port data but no environment,
auth, request bodies, or arbitrary external output.

Diagnostics record operation ID, generation, prior/target modes, commit, Node
source/version, per-stage duration, process identity checks, route hashes, and
rollback result. External logs remain under the feature root and are not merged
into AIO request logs.

## Compatibility and Rollback

- The integration is additive to CLI-proxy routing but destructive to the old
  local guard implementation. A code rollback can reintroduce old fields, but
  no migration relies on their data after this release.
- Disable is the normal operational rollback: direct AIO remains available and
  all external data is retained.
- Confirmed uninstall is allowed only when no live route points to the external
  gateway and no owned process remains. It deletes only the AIO-owned feature
  root; the external default root and manual installations are untouched.
- Source update rollback always keeps the prior known-working SHA until the new
  candidate has passed process and route verification.
- A release build must contain no external gateway source or Node runtime. The
  embedded recommendation is metadata only.

## Parallel Worktree Delivery

The current task is the integration parent and has four independently
verifiable gateway child tasks plus one already-committed supplemental UI fix:

| Child task | Orca worker name | Primary ownership |
| --- | --- | --- |
| `07-15-external-gateway-runtime` | `retry-gateway-runtime` | New Rust source store, Node/process ownership, managed state, bridge, runtime service |
| `07-15-codex-route-coordinator` | `retry-gateway-routing` | Existing CLI proxy, Codex config/provider sync, route projection and rollback |
| `07-15-external-gateway-frontend` | `retry-gateway-frontend` | Frontend services/query/UI/details, confirmation UX, frontend legacy removal, CSP |
| `07-15-remove-local-reasoning-guard` | `retry-gateway-removal` | Old Rust guard/runtime/statistics/settings removal plus scripts/docs/spec rules |

The supplemental task `07-16-codex-auto-review-route-neutral` remains owned by
`codex-auto-reviewer-model-routing` at exact commit
`1d5d2ac904fea3b5590dbf13b61a8c44956f7c74`. It changes request-log model-route
presentation and tests only. Its untracked `.codex-review-last.md` is local
review output and is never an integration input.

### Foundation wave

Create a top-level Orca integration worktree from the exact reviewed local
`main` commit. Because Orca currently identifies this repository through its
upstream metadata while repository operations must default to `origin`, every
create command must pass the exact repo ID and an explicit local base branch;
no worker may rely on implicit remote resolution.

The integration coordinator first transfers the reviewed Trellis task tree and
the protected current `AGENTS.md` baseline without touching the current main
worktree or its untracked analysis HTML. It then implements only the minimum
shared contract scaffold:

- settings field and DTO shapes;
- route/runtime enums and command signatures;
- module/command registration sufficient for generated TypeScript bindings;
- a generated-binding snapshot and mockable not-yet-implemented boundaries;
- shared constants and test fixture contracts.

Commit this foundation and record its immutable SHA. All four child worktrees
use that SHA/local branch as Git base and appear as Orca children of the
integration worktree.

### Parallel wave

Use Orca orchestration tasks with the foundation task as an explicit dependency
and a maximum concurrency of four. Each worker receives its child `prd.md`,
`design.md`, `implement.md`, the parent artifacts, exact foundation SHA, file
ownership allowlist, forbidden shared files, and required focused tests.

Workers may ask the coordinator about contract defects but do not edit another
worker's files or merge/rebase other worker branches. A required cross-boundary
change is proposed in the completion report and applied later by the
integration coordinator. Each worker creates reviewable commits and reports
commit SHA, modified paths, validation results, and residual risks through
`worker_done`.

The current Trellis Codex mode is inline. Planning may prepare this topology,
but actual Orca worker dispatch requires an orchestration-capable execution
turn. If execution is still forced to inline mode after approval, stop and
surface that constraint rather than claiming sequential work is parallel.

### Integration wave

Merge worker branches sequentially into the integration branch, never into
`main`, in this order:

1. Codex route coordinator;
2. external runtime backend;
3. local backend guard removal;
4. frontend/details migration;
5. supplemental route-neutral UI commit.

The order exposes the routing contract before lifecycle glue and removes old
backend contracts before the final binding regeneration. File ownership should
make merges mostly disjoint; the coordinator owns all registry, startup,
cleanup, generated-binding, task-state, and journal conflicts.

After merging, the coordinator adds only cross-worker glue, regenerates types,
fixes integration failures, runs the complete validation plan, and records any
behavioral conflict. A conflict that reveals mutually exclusive product
behavior is a user decision gate; ordinary compatible integration fixes remain
inside the approved scope.

The supplemental commit deliberately follows the frontend merge because both
touch `RequestLogDetailSummaryTab.tsx`. The integration resolution must retain
the gateway detail/status behavior while applying neutral sky/info treatment
only to expected `codex-auto-review*` mappings; ordinary route mismatches remain
severe.

### Independent review wave

After all sources and cross-worker glue are committed, freeze one clean
integration SHA. Create two isolated, read-only Orca review runs from that exact
commit: one launched with Codex model `gpt-5.6-sol` and reasoning effort `max`,
and one with Claude. The launch command, model/effort identity, and reviewed SHA
are part of each report; failure to launch the requested model and effort is
surfaced rather than silently substituted.

Reviewers report correctness, regression, security, lifecycle, rollback,
frontend interaction, and missing-test findings with file/line evidence. They
do not edit the integration worktree. Accepted findings are fixed on the
integration branch, the candidate is re-frozen, and affected findings receive
reviewer confirmation. Focused checks may run throughout integration, but the
authoritative full suite, build, package audit, and end-to-end gate run only
after both independent reviews are clear on the final candidate.

### Main decision gate

Freeze the independently reviewed and validated integration commit and leave
all worker and review worktrees intact.
Re-read local `main`, its current dirty `AGENTS.md`, and the untracked analysis
HTML. If `main` advanced, integrate that new local base into the integration
worktree and rerun affected plus full gates before asking. Present the exact
validated commit and results, then wait for explicit user approval before any
main merge/fast-forward. Remote push, release, and worktree cleanup are separate
later decisions.
