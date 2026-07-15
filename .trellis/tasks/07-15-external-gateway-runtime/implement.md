# Implementation Plan: External gateway runtime backend

## 1. Verify Foundation and Ownership

- [ ] Confirm HEAD equals the dispatched foundation SHA and read parent/child
      planning artifacts plus relevant backend specs.
- [ ] Record allowed/forbidden paths and baseline focused tests.
- [ ] Refuse implementation if required DTO/trait boundaries are absent; ask the
      coordinator instead of editing shared files.

## 2. Managed State and Source Store

- [ ] Implement feature-root paths, bounded schema parsing, atomic state writes,
      immutable active/previous source pointers, and safe staging cleanup.
- [ ] Implement exact official SHA/main ancestry resolution and GitHub/codeload
      allowlisted download with explicit network error classes.
- [ ] Reuse/factor bounded ZIP extraction, validate layout/syntax, compute
      fingerprints, promote atomically, and revalidate cache before use.
- [ ] Cover invalid SHA/ancestry, redirects, rate limits, archive attacks,
      corrupt cache, same-SHA repair, and offline cases.

## 3. Node Resolver

- [ ] Implement automatic discovery and validated manual absolute override.
- [ ] Probe directly with timeout, parse Node major >=18, preserve a prior valid
      override on failure, and re-probe before launch/update.
- [ ] Test platform paths, old/malformed versions, timeout, directory/symlink
      policy, reset-to-auto, and safe diagnostics.

## 4. Process and Port Ownership

- [ ] Generate managed external config/state with fixed loopback/AIO upstream
      and no usable whole-file restore backup.
- [ ] Implement exact spawn, startup health verification, identity capture,
      full-match reuse/stop, and bounded shutdown.
- [ ] Implement preferred/persisted/4610 fallback selection and commit only after
      identity plus health verification.
- [ ] Test stale/reused PID signals, foreign listeners, health mismatch, crash,
      stop timeout, fallback port, and no-kill mismatch.

## 5. Supervisor Primitives

- [ ] Add health snapshots, recovery counters/backoff/storm protection, explicit
      retry/reset, and generation-aware events/callbacks.
- [ ] Keep route selection outside this worker; assert callbacks are invoked in
      the expected order with fakes.

## 6. Management Bridge

- [ ] Add loopback server/session launch and expiry.
- [ ] Add path/method allowlist, bounds, streaming proxy, and instance checks.
- [ ] Protect managed config fields, overlay status, and intercept restore via
      the foundation callback.
- [ ] Test auth expiry, stale generation, path escape, body limits, protected
      config, restore, hot-apply health failure, and mid-stream process loss.

## 7. Focused Check and Handoff

- [ ] Run formatting and all runtime/source/Node/process/bridge focused Rust
      tests plus `git diff --check`.
- [ ] Audit modified paths against ownership and remove no user files/artifacts.
- [ ] Commit reviewable child-owned changes.
- [ ] Send one `worker_done` with commit SHA, paths, tests, risks, and requested
      integration glue. Do not merge, push, release, archive shared Trellis
      state, or touch main.

