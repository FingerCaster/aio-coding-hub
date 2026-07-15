# Research: External Codex Retry Gateway Integration

## Snapshot

- AIO baseline: `main@5be4b9d7` (`0.60.26`).
- External repository: `nonononull/codex-retry-gateway`.
- External remote snapshot researched: `origin/main@ef7fc5a0` on 2026-07-15.
- The existing local checkout was 33 commits behind before fetch; all external
  findings below use the fetched remote ref, not its old working tree.

## 1. Existing AIO Reasoning-Guard Ownership

The current feature is not one isolated response check:

- Four dedicated backend modules own matching, feature classification,
  continuation repair, and concurrent attempts:
  - `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_guard.rs`
  - `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_features.rs`
  - `src-tauri/src/gateway/proxy/handler/failover_loop/response/codex_reasoning_continuation.rs`
  - `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/codex_reasoning_guard_concurrent.rs`
- The guard is integrated into streaming/non-streaming response handling,
  retry state, provider iteration, request finalization, settings validation
  and migration, request-log aggregation, generated bindings, Codex settings,
  Home/Logs presentation, fixtures, and route tests.
- The settings/UI subset alone contains hundreds of `codex_reasoning_guard`
  references. Removal needs a contract inventory and cannot be implemented as
  a hidden `enabled = false` default.
- Historical product specs show that the original implementation explicitly
  rejected an external gateway and chose in-process same-provider retry. This
  task intentionally reverses that architecture decision:
  `.omx/specs/deep-interview-codex-downgrade-intercept.md`.

Adjacent behavior that is not automatically guard-owned must remain separate:

- generic upstream transient retry;
- OAuth reactive refresh;
- provider-scoped `previous_response_id` recovery;
- provider routing/failover/circuit behavior;
- model-route diagnostics and normal Codex request logging.

## 2. Current External Gateway Contract

### Runtime and lifecycle

- Requires Node.js 18+ (`origin/main:README.md:131`).
- Defaults to `127.0.0.1:4610` and state under
  `~/.codex-retry-gateway` (`origin/main:gateway.mjs:104`).
- Port `4610` is a default rather than a protocol requirement. The official
  launch/install entry points accept a listen port and persist it as
  `listen_port`; health checks and reuse validation read the configured port
  (`origin/main:scripts/admin-lib.mjs:674-688`,
  `origin/main:scripts/launch-ui.mjs:17-18`).
- Cross-platform `.mjs`, PowerShell, and shell entry points exist for launch,
  start, stop, install, and restore.
- `launch-ui` backs up the selected Codex provider, writes gateway config,
  rewrites only that provider's `base_url`, starts the process, validates PID
  identity through health, and rolls back failed transitions
  (`origin/main:scripts/admin-lib.mjs:701-827`).
- The management page is served at
  `/__codex_retry_gateway/ui`; status, config, logs, analytics, probe, and
  restore APIs share the same loopback server
  (`origin/main:gateway.mjs:14-23`, `10165-10654`).
- Config writes are hot-applied. Restore rewrites Codex config and exits the
  process (`origin/main:gateway.mjs:10494-10547`, `10616-10650`).

### Proxy chaining

The compatible front-gateway topology is:

```text
Codex CLI
  -> codex-retry-gateway (loopback :4610)
    -> AIO gateway (/v1)
      -> selected AIO provider
```

Evidence:

- External install captures the current provider `base_url` as
  `upstream_base_url`, then replaces that provider URL with its own loopback
  URL (`origin/main:scripts/admin-lib.mjs:742-815`).
- URL construction preserves an existing `/v1` base path without duplicating
  it (`origin/main:gateway.mjs:10657-10677`).
- Proxy forwarding removes hop-by-hop headers but preserves Authorization
  (`origin/main:gateway.mjs:10680-10703`).
- AIO currently writes Codex directly to `{aio_origin}/v1` and judges
  `applied_to_current_gateway` against that direct URL
  (`src-tauri/src/infra/cli_proxy/mod.rs:297-386`,
  `src-tauri/src/infra/cli_proxy/codex.rs:774`). A front gateway therefore
  requires native changes to AIO's CLI-proxy apply/status/recovery contract.

### Security and update gaps

- Management endpoints have no authentication or request token in the current
  handler. Safety currently relies on the default loopback bind.
- A non-loopback bind would expose config mutation, probes, logs/analytics,
  exports, and restore/stop operations; AIO must force loopback for managed
  instances.
- GitHub metadata reports no license, no tags, and no releases. The repository
  has no versioned artifact/checksum update contract.
- Tracking mutable `main` would execute newly fetched JavaScript with full user
  filesystem/process authority. AIO must not silently auto-run it.
- The latest 33 commits include GPT-5.6 recovery and layered capacity, 429,
  first-progress, and total-deadline policies. This pace reinforces the value
  of delegating behavior, but also makes an explicit trust/update boundary
  mandatory.

## 3. Existing Plugin-System Capability Matrix

| Required capability | Current plugin system | Evidence / result |
| --- | --- | --- |
| Store simple settings | Yes | `configSchema` plus `storage.plugin` (64 KiB) |
| Add a settings panel/button | Partial | Host-rendered fields only |
| Run request/response hooks inside AIO | Yes | Six active gateway hooks |
| Intercept before traffic reaches AIO | No | Hooks execute inside AIO after receipt |
| Start/stop arbitrary external process | No | Public API exposes no process capability |
| Fetch/install/update gateway source | No | `network.fetch`, `file.read`, and `file.write` are reserved, not public |
| Edit Codex config/CLI proxy topology | No | No file/config host API for plugins |
| Render gateway's custom web UI | No | Plugins cannot ship custom GUI code |
| Embed a local page | No | Host schemas are fields/panels/badges; app CSP is `frame-src 'none'` |
| Use protocol bridge declaration | No runtime value | Current execute path returns `PLUGIN_EXTENSION_PROTOCOL_BRIDGE_NOT_IMPLEMENTED` |
| Update plugin package itself | Yes | Marketplace supports signed/previewed `.aio-plugin` updates |
| Update a separately managed Node app | No | Plugin updater owns plugin package only |

Primary evidence:

- SDK capabilities and API surface:
  `packages/plugin-sdk/src/index.ts:58-88`, `260-277`.
- Host API construction exposes only commands, gateway hooks, storage,
  diagnostics, and privacy services:
  `src-tauri/src/app/plugins/extension_host_worker.rs:403-594`.
- Public plugin docs explicitly reserve network/file permissions and forbid
  custom GUI code: `docs/plugin-manifest-v1.md`.
- Only `providers.editor.sections`, `settings.sections`, and
  `logs.detail.tabs` are currently mounted UI slots:
  `docs/plugins/reference/manifest.md:64`.
- Main-webview CSP blocks frames:
  `src-tauri/tauri.conf.json:24`.

### Plugin verdict

The current plugin system cannot own this feature end to end. Making it work as
a plugin would first require new high-risk host APIs for process execution,
network/download, filesystem writes, Codex config ownership, managed binaries,
and custom/remote UI. Those APIs would be a larger core feature than the native
gateway integration and would create a general third-party code-execution
surface.

The plugin marketplace may later distribute an optional presentation/config
adapter, but the external gateway lifecycle and CLI-proxy chain need a native
AIO owner under the current architecture.

## 4. Native Integration Feasibility

A native integration is feasible because AIO already has:

- guarded process execution/termination patterns in CLI update and Codex model
  catalog infrastructure;
- atomic Codex config backup/apply/restore ownership in `infra/cli_proxy`;
- loopback gateway lifecycle and status events;
- a URL opener with scheme validation;
- GitHub download/update primitives used by skills/plugins.

Runtime evidence:

- The AIO desktop bundle does not currently ship a Node sidecar or external
  binary (`src-tauri/tauri.conf.json` has no `externalBin`; bundled resources
  contain only official plugin assets).
- Existing CLI update code can locate npm beside a detected CLI executable or
  through the CLI manager and already applies hidden-process, timeout, bounded
  output, and kill-on-drop controls (`src-tauri/src/infra/cli_update.rs:293-424`).
- A managed gateway can reuse the discovery style, but it needs a dedicated
  Node executable probe and a hard major-version check for Node 18+.
- AIO's own gateway already treats its configured port as preferred, binds the
  first available loopback port when appropriate, returns the effective port,
  and persists a fallback (`src-tauri/src/gateway/control_service.rs:46-55`,
  `src-tauri/src/app/gateway_control.rs:48-55`). The external integration can
  follow this established pattern if automatic fallback is selected.

It still requires a dedicated subsystem rather than a Boolean-only patch:

- source/runtime discovery and version state;
- Node 18+ validation or a packaged runtime decision;
- serialized install/start/stop/update/restore operations;
- PID plus health identity validation;
- a two-hop CLI proxy manifest (`external -> AIO`);
- crash/startup recovery and rollback;
- management-page navigation and safe loopback identity checks;
- migration/removal of the existing local reasoning-guard contract.

## 5. Initial Native Recommendation (Resolved)

Use a native AIO integration with a small user-facing switch and details entry.
Do not expand the general plugin API merely to host one privileged process.

Keep external behavior/config in the external management page. AIO should own
only installation/version trust, process lifecycle, chain health, enable/disable,
and recovery. Sections 6-18 record the subsequently resolved source, update,
runtime, lifecycle, recovery, distribution, and configuration decisions.

## 6. Resolved Source/Update Direction

The user accepted commit-pinned execution and explicit update confirmation:

- Store and display the full installed commit SHA.
- Offer an AIO-recommended reviewed commit and an advanced manual commit
  selector as distinct trust states.
- The update action performs discovery only until the user approves a concrete
  candidate.
- Keep the previous checkout as the rollback target; never update a live
  working directory in place.
- Validate candidate identity and health before committing the active pointer.

The evaluated source-policy choice was whether manual selection should be
restricted to commits from the official repository or allow arbitrary forks.

Decision: first-version manual selection is restricted to full commits that
are ancestors of the fetched official `origin/main`. Forks, custom repository
URLs, and branch-only commits are out of scope.

Runtime decision: first version requires a discovered local Node.js 18+
runtime and does not bundle Node. Enable/update must fail before any config or
process mutation when the runtime probe is missing or unsupported.

Details/lifecycle decision:

- The verified management UI is embedded in an AIO details route.
- Leaving that route never changes gateway runtime state.
- The outer AIO switch owns enable/disable. Disable restores direct AIO routing
  and stops the managed external process.
- External self-restore/exit is reconciled back into the outer switch state by
  health/process observation.

Ownership decision: first version manages only instances created and recorded
by AIO. It does not auto-adopt or terminate manually started compatible
gateways.

## 7. Startup Persistence Evidence

- AIO loads persisted settings during every application startup, starts its
  internal gateway, and then synchronizes the Codex CLI proxy
  (`src-tauri/src/app/startup_tasks.rs`,
  `src-tauri/src/app/startup_gateway.rs`). A persisted external-gateway desired
  state can therefore be reconciled in the same startup pipeline.
- AIO already has a system `auto_start` setting backed by the Tauri autostart
  plugin (`src-tauri/src/app/autostart.rs`). It defaults to `false`
  (`src-tauri/src/infra/settings/types.rs:538`).
- Consequently, an AIO application restart can restore the managed external
  gateway automatically. After an OS restart, immediate restoration requires
  AIO autostart; otherwise restoration can only occur when the user next opens
  AIO.
- Product decision: persist the external gateway's enabled desired state and
  reconcile it automatically on every AIO application start. The feature does
  not inspect or mutate the global AIO autostart preference; OS-login startup
  remains outside this integration's scope.

## 8. Legacy Settings and Request-Log Shape

- Local guard configuration is a set of strongly typed `AppSettings` fields,
  including templates, retry budgets, continuation controls, labels, and
  deprecated compatibility values
  (`src-tauri/src/infra/settings/types.rs:367-416`). Settings persistence reads
  JSON into that type and writes a canonical serialization of the current type
  (`src-tauri/src/infra/settings/persistence.rs:477-505`). Removing those
  fields therefore allows old keys to be ignored on read and omitted on the
  next managed write; preserving them would require an intentional legacy
  compatibility container.
- Guard observations and continuation outcomes are stored inside the generic
  `request_logs.special_settings_json` text column rather than guard-specific
  schema columns (`src-tauri/src/infra/db/migrations/baseline_v25.rs:72`).
- Dedicated backend queries and frontend presentation currently interpret
  those JSON markers for guard statistics and badges
  (`src-tauri/src/infra/request_logs/queries.rs:694-1350`,
  `src/services/gateway/requestLogSpecialSettings.ts:456-688`). They can be
  removed without deleting or rewriting the owning request-log rows.
- Existing request-log retention deletes complete old rows by age and already
  provides a non-destructive lifecycle for historical guard markers
  (`src-tauri/src/infra/request_logs.rs:385-417`).

## 9. Managed Source and Runtime-State Retention

- The external scripts accept `--state-root` across launch, start, stop, and
  restore. That root owns config, logs, backups, install state, PID state, and
  analytics (`origin/main:scripts/admin-lib.mjs:153-162`, README line 123).
- `stopGateway` verifies process ownership and removes only a matching PID
  file after stopping the process. It does not delete configuration, logs,
  analytics, backups, or source. `restoreCodexConfig` restores Codex, stops the
  process, and removes install state, but likewise does not perform a complete
  data purge (`origin/main:scripts/admin-lib.mjs:590-668`, `859-879`).
- AIO already isolates managed feature data under its own application data
  directory and separates installed/cache/data/log subtrees for plugins
  (`src-tauri/src/infra/app_paths.rs:46-107`). The external integration can use
  an analogous AIO-owned root instead of the external project's default
  `~/.codex-retry-gateway`, which prevents collision with manually managed
  installations.
- Retaining the active and previous known-working source commits is required by
  the already accepted transactional update/rollback policy. Deleting sources
  on every disable would force a network reinstall and eliminate immediate
  rollback, while deleting config/analytics would make the lifecycle switch a
  destructive data action.

## 10. Recommended-Commit Trust Publication

- The external repository has no signed release, tag, checksum feed, or other
  upstream-reviewed artifact channel. Its official `main` commit can establish
  repository provenance and ancestry, but not that AIO maintainers reviewed
  the behavior of that commit.
- AIO's application updater fetches release metadata from the fork's `origin`
  repository and verifies artifacts with a compiled public key
  (`src-tauri/tauri.conf.json:48-55`). A recommendation embedded as an app
  resource or compile-time manifest is therefore covered by the AIO release
  trust boundary and remains available offline.
- AIO also implements Ed25519 verification for signed plugin-market indexes
  (`src-tauri/src/infra/plugins/market.rs:94-103`), so an independently updated
  signed gateway recommendation feed is technically possible. There is no
  current gateway-specific feed, production signing key, publishing workflow,
  rollback policy, or revocation process; introducing one expands the first
  version's operational security scope.
- The smallest coherent trust model is to ship one recommended reviewed full
  SHA with each signed AIO release and label official `main` HEAD or a manual
  official-main ancestor as unreviewed. Users can still update immediately by
  explicit confirmation, while a new "reviewed" label arrives with a later AIO
  release.

## 11. Crash Detection and Recovery Boundary

- The external health/status response exposes `process_id`, listen address,
  upstream/config, and state. Its own scripts already require the health
  `process_id` to equal the expected PID before treating an instance as owned
  (`origin/main:gateway.mjs:10180-10188`,
  `origin/main:scripts/admin-lib.mjs:284-314`). These fields are sufficient for
  an AIO-owned periodic health/identity probe.
- AIO serializes gateway/CLI-proxy lifecycle operations and already has
  best-effort proxy restoration on startup and shutdown failures
  (`src-tauri/src/app/startup_gateway.rs:23-31`,
  `src-tauri/src/app/cleanup.rs:143-180`). The two-hop design still needs a new
  chain-aware operation that can move Codex from external-gateway routing back
  to direct AIO without disabling AIO's existing proxy ownership.
- AIO's WebView heartbeat watchdog provides an established recovery pattern:
  exponential backoff capped at five minutes, a failure threshold, serialized
  checks, and storm prevention (`src-tauri/src/app/heartbeat_watchdog.rs:64-67`,
  `347-364`, `1026-1038`). It monitors the frontend and cannot itself supervise
  the external Node process, but its control principles can be reused.
- Decision: use fail-open direct-AIO routing. It preserves Codex availability
  while temporarily bypassing interception; desired state remains enabled and
  the UI reports bypassed/recovering until full external ownership and health
  verification succeeds again.

## 12. WSL Topology Constraint

- AIO's optional WSL auto-configuration includes Codex by default when enabled.
  It writes the WSL Codex provider base URL to the resolved Windows-host address
  and AIO gateway port (`src-tauri/src/infra/settings/types.rs:319-331`,
  `src-tauri/src/commands/wsl.rs:193-242`,
  `src-tauri/src/infra/wsl/config_codex.rs:6-13`).
- In WSL/LAN modes that address is not necessarily Windows loopback; it is
  explicitly selected so the distribution can reach the Windows-side AIO
  gateway (`src-tauri/src/infra/wsl/detection.rs:296-311`,
  `src-tauri/src/commands/wsl.rs:23-44`).
- The external gateway serves proxy, configuration, logs, analytics, probes,
  restore, and management UI on the same listener, without authentication.
  Binding that listener to a WSL-reachable host address would violate the
  accepted loopback-only security requirement and may expose privileged
  management APIs to the LAN.
- Secure WSL coverage therefore needs additional architecture: either an
  AIO-owned WSL-reachable proxy-only bridge into the Windows-loopback external
  gateway with a provable non-recursive return path, or one managed external
  gateway plus Node/source/state lifecycle per WSL distribution. The latter
  also fragments configuration, analytics, updates, and UI across instances.
- A host-native-only first version can leave existing WSL Codex routing direct
  to AIO and label it unprotected, but removal of the local in-process guard
  means this is a real capability gap for WSL users rather than an invisible
  implementation detail.

## 13. External License and Distribution Boundary

- GitHub repository metadata queried on 2026-07-15 reports the official
  repository as public and unarchived with default branch `main`, but
  `license: null`. The `origin/main` tree has no LICENSE, LICENCE, COPYING, or
  NOTICE file, and it has no `package.json` package-license declaration.
- Public source visibility and GitHub clone access do not themselves specify
  redistribution, modification, or derivative-work permissions. This is a
  release/legal risk rather than a runtime identity problem.
- The proposed integration does not need to vendor the JavaScript in the AIO
  source tree or bundle it in AIO installers. It can download an immutable
  official commit archive only after a user enables the third-party gateway,
  display the repository/SHA/trust state, and keep the cached source in
  AIO-owned user data.
- Runtime-only acquisition reduces AIO's redistribution footprint but leaves a
  residual legal/trust risk while the upstream project has no declared license.
  Accepted product decision: absence of that license is not an AIO release gate
  for this runtime integration. AIO must not vendor or bundle the source; it
  downloads the user-approved official commit at runtime and identifies it as
  third-party code with undeclared license metadata.

## 14. Node Runtime Discovery and Override Feasibility

- AIO's CLI manager already resolves executables through the login shell,
  process PATH, common user/package-manager directories, Windows Node install
  directories, and bounded nvm version scans. It also builds a runtime PATH
  containing the resolved executable's sibling directory
  (`src-tauri/src/infra/cli_manager.rs:442-540`, `608-715`).
- The Codex launch spec exposes a concrete executable and runtime PATH, and CLI
  update code already prefers sibling npm. A gateway-specific resolver can
  reuse these patterns but must probe the Node binary itself with a timeout and
  parse a major version of at least 18.
- AIO already exposes a guarded native open-file dialog command
  (`src-tauri/src/commands/desktop.rs:236-290`, `451-482`). An optional manual
  Node override therefore does not require a new desktop permission surface.
- A persisted manual path still must be normalized, required to be an absolute
  regular executable file, re-probed before every launch, and never invoked
  merely because it was previously selected. Clearing it should return to
  automatic discovery.

## 15. Embedded Management UI Security Contract

- AIO currently blocks all frames with `frame-src 'none'`; loopback is already
  allowed for parent-webview `connect-src`
  (`src-tauri/tauri.conf.json:24`). Embedding requires an explicit loopback-only
  `frame-src` change and must not add arbitrary HTTP or remote domains.
- The external UI response sets HTML content type and no-cache headers but no
  `X-Frame-Options` or `Content-Security-Policy`, so it is technically
  frameable (`origin/main:gateway.mjs:6327-6333`, `10174-10176`).
- The page uses same-origin `fetch` for status, config, analytics, imports,
  probes, and restore. A sandbox must therefore allow scripts and same-origin;
  omitting either makes the management page nonfunctional. It also uses
  `window.confirm`, reload, external `_blank` links, and a popup-oriented export
  flow (`origin/main:gateway.mjs:7910`, `8750-8753`, `9871`, `9956`).
- The embedded frame should allow only the minimum needed capabilities:
  scripts, same-origin, forms, modals, and downloads. It must not receive Tauri
  IPC capability, top navigation, or an unrestricted popup escape. Popup-only
  functions that cannot operate under the sandbox remain available through the
  always-visible AIO "open in browser" action.
- AIO provides the frame URL only after health, PID, commit, state root, port,
  and upstream validation. The frame is unloaded immediately on an unhealthy
  transition or route exit; route exit does not stop the process. Global CSP is
  only a coarse origin allowlist and is not treated as instance identity.

## 16. Official Commit Resolution and Archive Safety

- AIO already downloads GitHub zipballs for skills and contains bounded ZIP
  extraction, entry-count/expanded-size limits, staging directories, and atomic
  replacement patterns (`src-tauri/src/domain/skills/repo_cache.rs:490-598`,
  `754-832`). Plugin packages separately compute SHA-256 before bounded
  extraction (`src-tauri/src/infra/plugins/package.rs:99-127`). These mechanisms
  should be reused or factored rather than copied unsafely.
- GitHub zipball downloads do not embed a machine-readable canonical commit
  identity in the extracted snapshot
  (`src-tauri/src/domain/skills/repo_cache.rs:355-362`). Commit identity and
  official-main ancestry must therefore be resolved through the GitHub API
  before downloading, then bound to the staged source manifest.
- Candidate resolution uses only `nonononull/codex-retry-gateway`: resolve the
  requested ref to a canonical 40-hex SHA, require the exact SHA to be equal to
  or an ancestor of current official `main`, and download by that canonical SHA.
  Redirects are limited to GitHub/codeload hosts; forks and caller-supplied URLs
  are never accepted.
- The researched snapshot contains 55 blobs totaling about 1.70 MiB, with the
  largest file about 505 KiB. Conservative compressed, expanded, entry-count,
  per-file, timeout, and redirect limits can reject pathological archives while
  leaving ample growth room.
- Extraction must reject absolute paths, `..` traversal, symlink/reparse entries,
  multiple unexpected roots, and duplicate normalized paths. Before promotion,
  require the expected gateway/scripts structure, run bounded Node syntax
  validation, compute a local archive/source fingerprint, write a managed
  manifest, and atomically rename staging to `sources/<full-sha>`.
- A cached source is executable only when its path, manifest SHA, archive/source
  fingerprint, required files, repository identity, and current AIO ownership
  all revalidate. Cache corruption triggers same-SHA repair download when the
  previous user approval permits it; it never silently selects another commit.

## 17. Local Removal Inventory

The complete deletion/edit/retention boundary is recorded in `inventory.md`.
It identifies the four dedicated backend modules, shared failover branches,
settings and migration surfaces, request-log statistics/presentation, Codex UI,
generated bindings, tests, scripts/docs/specs, and the adjacent behaviors that
must survive. Implementation must use that file as a negative-search and
regression checklist rather than treating all Codex retry code as guard-owned.

## 18. Codex CLI-Proxy and Remote-Compaction Coordination

- The existing CLI-proxy manifest is the persisted ownership record for the
  Codex route. Enabling ensures the AIO gateway is running before applying the
  target, while status and startup sync compare the recorded target with the
  live configuration (`src-tauri/src/app/cli_proxy_service.rs:31-76`,
  `src-tauri/src/infra/cli_proxy/mod.rs:917-951`, `1148-1247`). An external
  gateway therefore cannot be a truthful independent switch; it must be a
  chain-aware Codex CLI-proxy mode.
- AIO already treats `remote_compaction` as a provider-identity change. It
  writes the feature value, renames `[model_providers.aio]` to
  `[model_providers.OpenAI]` (and back when disabled), and updates the root
  `model_provider` (`src-tauri/src/infra/codex_config/patching.rs:814-835`).
  The provider-sync transaction also updates Codex session and SQLite/global
  state and refuses to run while Codex is open
  (`src-tauri/src/infra/codex_provider_sync.rs:113-229`).
- The existing Codex page exposes a manual Provider Sync action and reports the
  target provider after success. It maps the running-process and process-check
  failures to explicit close/verify-Codex guidance
  (`src/components/cli-manager/tabs/CodexTab.tsx:5160-5176`,
  `src/pages/cli-manager/useCliManagerPageDataModel.ts:599-621`). Structured
  `remote_compaction` saves already enter the same provider-sync core through
  `patch_requires_provider_sync` (`src-tauri/src/infra/codex_config/mod.rs:193-201`,
  `314-358`). Gateway enable should reuse this transaction and user vocabulary,
  while adding a pre-mutation confirmation because enable may implicitly change
  provider identity.
- Config changes made while the CLI proxy is enabled refresh its guarded backup
  transactionally, and current tests explicitly cover preserving
  `remote_compaction` across cleanup restore and rolling back a failed backup
  refresh (`src-tauri/src/infra/codex_config/mod.rs:50-97`, `314-358`;
  `src-tauri/tests/cli_proxy_startup_recovery.rs:114-193`, `196-266`). This
  mechanism must be generalized to represent the unproxied, direct-AIO, and
  external-gateway routes without confusing a routed live file with its
  rollback source.
- The external project resolves the current provider name at install time,
  requires that name to remain unchanged when patching `base_url`, and stores it
  in runtime state (`origin/main:scripts/admin-lib.mjs:147-207`, `428-445`,
  `495-524`). Its status path later uses the stored provider name to locate the
  live `base_url` (`origin/main:gateway.mjs:5290-5327`). Renaming `aio` to
  `OpenAI` behind it would therefore make status stale or null.
- The external restore path copies its saved TOML backup over the entire live
  Codex config (`origin/main:gateway.mjs:5330-5344`). That can erase a
  `remote_compaction` change or any other edit made after gateway enable. The
  native integration must consequently keep AIO as the sole Codex-config owner
  and route external restore/exit requests through an AIO-managed loopback
  bridge and configuration coordinator rather than exposing raw restore
  semantics.
