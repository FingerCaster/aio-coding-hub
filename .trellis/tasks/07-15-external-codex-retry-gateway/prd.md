# Replace local reasoning guard with retry gateway

## Goal

Stop maintaining AIO Coding Hub's in-process Codex reasoning-degradation
interception and continuation-repair implementation. Replace it with one
user-facing reasoning-guard gateway capability that manages the third-party
[`nonononull/codex-retry-gateway`](https://github.com/nonononull/codex-retry-gateway)
as a front gateway and exposes that project's management page from AIO.

AIO owns trust, installation, enable/disable, launch/stop, routing, status,
recovery, details entry, and update coordination. The external project owns
actual interception/retry behavior, its behavior configuration, logs,
analytics, and management-page implementation.

## Evidence and Architecture Decision

- AIO's local guard was derived from `codex-retry-gateway` and later accumulated
  fork-specific continuation, safety, observability, configuration, statistics,
  and UI behavior. Continuing both implementations duplicates maintenance.
- The existing plugin system cannot own this integration end to end. It exposes
  no public arbitrary-process, network/download, filesystem, Codex-config,
  front-proxy, or custom-page capability. Adding all of them would create a new
  privileged plugin platform rather than isolate this feature. The selected
  architecture is therefore a dedicated native AIO integration; the plugin
  system is not expanded for this task.
- The compatible topology is `Codex -> external gateway -> AIO gateway ->
  provider`. The external proxy preserves Authorization and `/v1` path shape.
  AIO's current Codex CLI-proxy contract assumes direct `Codex -> AIO`, so it
  must become chain-aware.
- The external gateway requires Node.js 18+, defaults to `127.0.0.1:4610`, and
  exposes health, status, config, restore, log, analytics, probe, and UI APIs on
  one listener. Its UI is `/__codex_retry_gateway/ui`.
- AIO does not bundle Node. It has reusable executable discovery, guarded child
  process, bounded GitHub archive, atomic config, gateway lifecycle, and CLI
  proxy patterns.
- AIO reads persisted settings, starts its gateway, and synchronizes CLI proxy
  state on every application launch (`src-tauri/src/app/startup_tasks.rs`,
  `src-tauri/src/app/startup_gateway.rs`). Its independent system `auto_start`
  preference defaults to false.
- At the researched baseline
  `origin/main@ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2`, the external repository
  has no declared license, tag, GitHub Release, or checksum/version feed.
  Mutable-`main` execution would run unreviewed remote JavaScript with user
  authority.
- Detailed source evidence and the plugin capability matrix are in
  `research.md`. The complete local-removal and retained-behavior boundary is
  in `inventory.md`.

## Requirements

### R1. Replace only the local reasoning-guard implementation

- Remove AIO-owned reasoning-degradation detection, forced interception,
  continuation repair, local rule/retry configuration, dedicated statistics,
  observability markers, UI, scripts, active documentation, release rules, and
  tests that exist only for that implementation.
- Keep one compact external reasoning-guard gateway capability and do not keep
  the old and new controls side by side.
- Preserve generic transient retry and provider overrides, OAuth reactive
  refresh, provider-scoped Codex `previous_response_id` recovery, provider
  routing/failover/circuits, model-route diagnostics, normal request/attempt
  logging, streaming/non-streaming passthrough, usage/cost/cancellation,
  Claude/Gemini CLI proxying, native AIO gateway lifecycle, WSL direct-AIO
  routing, and unrelated Codex settings.

### R2. Make the external gateway a Codex CLI-proxy route mode

- The only guarded topology is `Codex -> external -> AIO -> provider`; AIO must
  prevent proxy recursion and credential leakage.
- Enabling the external gateway from a disabled host Codex CLI proxy shows that
  the CLI proxy will also be enabled, then applies both in one rollback-safe
  operation.
- Gateway-only disable first verifies direct `Codex -> AIO`, then stops the
  managed external process and keeps the Codex CLI proxy enabled.
- Explicitly disabling the Codex CLI proxy first turns off the external desired
  state, restores the user's unproxied Codex configuration, and stops the owned
  external process.
- Sidebar CLI-proxy actions, gateway actions, startup, crash fallback,
  external-page restore, update, uninstall, and provider-mode changes use one
  serialized chain-aware coordinator. UI state must never claim protection
  when the CLI proxy is disabled or points elsewhere.

### R3. Confirm and reuse Codex Provider Sync

- Before enable, AIO computes the current and target managed provider. If the
  route operation changes the provider name, the consolidated confirmation
  shows the exact `current -> aio/OpenAI` change, states that existing Codex
  sessions/provider state will be synchronized with a backup, and requires
  Codex to be closed.
- Canceling the dialog performs no provider, session, process, route, or desired
  state mutation.
- The backend recomputes the plan and Codex-process precondition immediately
  before mutation. A stale plan, running Codex, failed process check, or sync
  failure aborts the complete enable transaction.
- The enable flow reuses the existing Codex Provider Sync core and result/error
  reporting rather than adding a gateway-specific history rewrite. The manual
  Provider Sync action remains available.

### R4. Preserve `remote_compaction` in every route state

- Turning `remote_compaction` on or off while external desired state is on
  atomically renames the managed provider between `aio` and `OpenAI`, keeps the
  correct current route target, refreshes rollback state, synchronizes existing
  provider/session data, and retains external desired state. The existing
  Codex-closed requirement remains.
- The canonical unproxied configuration and the live guarded/direct-AIO
  projection are updated coherently. A routed external URL must not be learned
  as the canonical rollback route.
- Guarded `aio` or `OpenAI` points to the verified external listener;
  recovering/bypassed or gateway-disabled-with-CLI-proxy-on points directly to
  AIO; CLI-proxy disable restores the unproxied route.
- External provider state and whole-file backups are not authoritative. AIO
  keeps external status aligned with the current provider and handles restore
  through its config coordinator so a stale backup cannot erase
  `remote_compaction` or later user changes.
- A failed provider-mode transaction restores provider name, feature value,
  provider-sync data, live route, canonical backup, CLI-proxy manifest, and
  managed external metadata to the prior committed generation.

### R5. Expose verified external details without coupling navigation to lifecycle

- The details route embeds the external management page only after validating
  loopback address, AIO ownership, process/health identity, installed commit,
  config/state root, and expected AIO upstream.
- AIO provides back/exit, status, refresh, update, stop/disable, retry, and
  open-in-system-browser controls outside the embedded page.
- Leaving the route, unloading the frame, hiding the AIO window, or using back
  navigation does not stop, disable, or restart the external gateway.
- The external page's restore/exit action is intercepted as an explicit
  gateway-only disable: route direct AIO, persist desired state off, and stop
  the verified process. A disappearing frame alone is not a disable signal.
- Both embedded and browser entry use an AIO-owned loopback management bridge.
  The bridge protects AIO-owned listener/upstream/health config fields and does
  not expose the external whole-file restore operation.
- CSP allows only the managed loopback page shape and the iframe is sandboxed.
  If embedding is unavailable, show status and an explicit browser action.

### R6. Execute only approved official-main commits

- A managed installation is identified by one immutable full 40-hex commit
  SHA. Branch names and abbreviated SHAs are never execution identities.
- The user may select a commit, but version one accepts only commits proven at
  verification time to be official `nonononull/codex-retry-gateway` `main` or
  its ancestor. Forks, caller-provided repository URLs, and branch-only commits
  are rejected.
- Each signed AIO release embeds exactly one maintainer-reviewed full SHA. Only
  exact equality with that value is labeled `recommended, reviewed`.
- Official `main` HEAD and manually entered official-main ancestors remain
  `official, unreviewed` until their exact SHA is embedded in an AIO release.
- Manual install and update confirmation show repository, full SHA, installed
  and candidate commits, commit distance/summary, trust state, source, and
  rollback target. Commit selection does not imply review.
- The update button checks only. Applying another SHA always requires explicit
  confirmation, stages and validates a separate source, switches only after
  process/health verification, and retains the previous known-working commit
  until success. Failure restores the previous source/config/process/route or
  leaves a safe direct-AIO recovery state.
- A remotely updated signed recommendation feed and custom-repository developer
  mode are deferred.

### R7. Download third-party source at runtime, never package it

- AIO identifies the external project as independently owned and does not claim
  publication or require its maintainer to add a license as an integration
  prerequisite. The UI shows that license metadata is undeclared/unavailable.
- AIO source releases, installers, portable archives, and bundled resources
  contain no external gateway source or Node runtime.
- After explicit first-install/enable confirmation, AIO downloads only the
  approved exact official SHA into an AIO-owned per-user data root with bounded
  archive validation and stored integrity metadata.
- Redownloading an already approved identical SHA to repair a corrupt cache is
  not an update. Selecting any different SHA uses the update confirmation.
- A verified cached commit may start offline. A missing/invalid cache cannot be
  installed offline, and download/extraction failure changes no committed route
  or desired/runtime state.

### R8. Require a verified Node.js 18+ runtime

- Version one does not bundle Node.js. Before install/start/update, AIO resolves
  a concrete executable, probes it directly with a timeout, and requires major
  version 18 or newer. It never runs an unverified shell string named `node`.
- Automatic discovery prefers the Codex/npm sibling runtime, then established
  AIO cross-platform locations and process PATH. Status shows executable,
  parsed version, and resolution source without unrelated environment data.
- An advanced manual override accepts only a validated absolute executable
  path. Invalid, timed-out, directory, symlink-policy, or unsupported input does
  not replace the previous valid selection.
- Manual overrides are revalidated before every launch/update, and the user can
  return to automatic discovery.

### R9. Manage only fully owned loopback instances

- AIO may reuse, restart, update, or stop only an instance whose AIO-owned
  manifest, executable/source root, full commit, PID/process identity, health
  `process_id`, listener, config/state path, instance identity, and expected AIO
  upstream all match.
- A manually started compatible instance is not silently adopted. A stale PID,
  occupied port, or matching-looking health endpoint is insufficient authority
  to kill or reconfigure a process.
- The external listener binds only to `127.0.0.1` and prefers port `4610`.
  A foreign occupant is left untouched and AIO selects another free loopback
  port.
- A fallback becomes persisted only after process, health, upstream, and Codex
  route verification. Restart recovery reuses it when possible and may move
  transactionally; UI always displays the effective port.

### R10. Persist desired state and fail open to AIO

- External enabled state is AIO-owned desired state. An AIO application restart
  automatically reconciles and restores a verified managed instance and guarded
  route without another toggle.
- This feature never reads or changes the independent system `auto_start`
  preference. After an OS reboot it recovers when AIO next starts; immediate
  login recovery remains controlled by the existing global preference.
- While desired state is on, AIO periodically verifies process, health,
  listener, commit, config/state root, upstream, and live route.
- On involuntary failure, AIO first routes new host Codex requests directly to
  its still-running gateway, keeps desired state on, and reports actual state as
  bypassed/recovering rather than protected.
- Recovery is serialized with bounded exponential backoff and storm protection.
  Full ownership/health verification is required before returning to guarded.
  Repeated failures pause automatic attempts and expose retry and disable;
  application restart reconciles again.
- External restore/exit is a user disable action, not a crash, and therefore
  persists desired state off.

### R11. Limit version-one coverage to host-native Codex

- Only host-native Codex config is protected. WSL Codex remains on its existing
  direct-AIO route, and WSL AIO routing, MCP, prompt, and skills synchronization
  must not regress.
- The unauthenticated external management listener and the AIO management bridge
  are never bound to a WSL- or LAN-reachable address.
- When existing WSL auto-configuration targets Codex, enable shows a non-blocking
  explicit warning. Status and details continuously show `WSL Codex is not
  protected`, update it when WSL target settings change, and never imply all
  Codex clients are guarded.
- A WSL proxy-only bridge, per-distribution external install, and non-loopback
  external exposure are deferred.

### R12. Migrate and retain data deliberately

- Remove obsolete local-guard settings from the managed schema without mapping
  them to external rules. Old keys are ignored on read and disappear on the
  next canonical settings write.
- Never delete or rewrite generic `request_logs` rows because they contain old
  guard markers. Remove dedicated stats commands, aggregation, badges, and
  historical presentation; markers remain inert until normal row retention.
- Do not import local guard settings or observations into external config or
  analytics. The external gateway begins with its version-appropriate defaults.
- Gateway-only disable is non-destructive: retain active/previous source,
  external config, logs, analytics, backups, and ownership data for re-enable
  and rollback.
- Offer a separate confirmed uninstall/data-removal action only while disabled.
  It verifies no route/process dependency and removes only AIO-owned source and
  state roots. It never reads, changes, or deletes the external default
  `~/.codex-retry-gateway` root or manual installations.
- Re-enable after disable reuses a valid cache; re-enable after uninstall
  performs a fresh verified install.

### R13. Develop in isolated parallel worktrees and gate main integration

- Keep the current `main` worktree untouched by implementation. Create one
  Orca-managed integration worktree from the reviewed exact local `main` SHA,
  then establish and commit a shared contract foundation there.
- Create four child worker worktrees from that exact foundation commit for:
  external runtime backend, Codex route coordination, frontend/details, and
  local-guard backend removal. Each worker has an explicit non-overlapping file
  ownership contract and independently reports focused validation.
- Track the worker DAG and completion through Orca orchestration. Workers never
  merge one another, never operate on `main`, and never push or release.
- Merge all worker branches only into the integration worktree. Resolve shared
  glue and generated bindings there, then run focused, full, packaging, and
  end-to-end validation against the combined tree.
- After all validation passes, report commits, conflicts, fixes, and complete
  test results, then ask the user whether to merge the validated integration
  branch into `main`. Do not merge, fast-forward, push, release, or delete the
  worker worktrees before that explicit decision.

## Acceptance Criteria

- [ ] **AC1 (R1):** `inventory.md` is exhausted and active-source negative
      searches find no local guard/continuation implementation, config,
      statistics endpoint, badge, dedicated history UI, script, or release rule;
      retained adjacent retry/routing/logging tests still pass.
- [ ] **AC2 (Evidence):** `research.md` documents the external contract/update
      path and plugin capability matrix from source and current repository
      metadata; `design.md` and `implement.md` cover the complete lifecycle
      before implementation starts.
- [ ] **AC3 (R2):** automated transitions cover unproxied -> guarded,
      direct-AIO -> guarded, guarded -> direct-AIO, and guarded -> unproxied,
      including partial-failure rollback and truthful CLI-proxy/external status.
- [ ] **AC4 (R3-R4):** provider-change enable confirmation shows exact source/
      target and session-sync impact. Cancel, stale preflight, running Codex,
      provider-sync failure, and every `remote_compaction` route-state failure
      leave the exact prior provider, session data, route, backup, process, and
      desired state; successful cases keep `aio`/`OpenAI` coherent.
- [ ] **AC5 (R5):** verified iframe and browser bridge entry work; navigation/
      frame/window lifecycle never stops the gateway; restore safely returns to
      direct AIO without erasing newer Codex config; unverified or non-loopback
      frames/management requests fail closed.
- [ ] **AC6 (R6-R7):** only full official-main commits execute; trust labels are
      exact-SHA based; check never auto-applies; candidate failure restores the
      previous source or direct-AIO safety; release artifacts contain no
      external source/Node; corrupt/offline first install changes no route.
- [ ] **AC7 (R8):** automatic and manual runtime paths both require a concrete
      re-probed Node 18+ executable; invalid override cannot replace a working
      selection and clearing it returns to automatic discovery.
- [ ] **AC8 (R9):** port `4610` is preferred, foreign/stale instances are never
      adopted or killed, verified fallback is persisted only after success, and
      every stop/reuse/update path proves full process ownership.
- [ ] **AC9 (R10):** killing/corrupting the managed process yields verified
      direct-AIO fallback, desired-on recovering/paused UI, bounded restart, and
      verified guarded recovery; AIO restart restores desired state without
      changing system autostart.
- [ ] **AC10 (R11):** with WSL Codex targeting active, host traffic can be
      guarded while WSL remains direct AIO, and confirmation/status/details keep
      the WSL-unprotected warning without exposing management listeners.
- [ ] **AC11 (R12):** upgrade drops obsolete settings on canonical persistence,
      preserves generic log rows, disable retains AIO-owned runtime data, and
      confirmed uninstall deletes only the verified AIO-owned feature root.
- [ ] **AC12 (R13):** four worker branches originate from one reviewed
      foundation, stay within declared ownership, and merge only into the
      integration worktree; combined validation passes there and `main` remains
      unchanged until the user explicitly approves the final merge.

## Out of Scope

- A general privileged plugin process/network/filesystem/custom-UI platform.
- Custom repositories, forks, mutable branch execution, or an independently
  signed remote recommendation feed.
- Bundled Node.js distribution.
- Automatic adoption of manually started external gateway instances.
- WSL-protected routing or non-loopback external/management listeners.
- Changing AIO's system-login autostart preference.
- Product-code changes, process launch, package publication, or release before
  the final planning artifacts are reviewed and the task is explicitly started.
