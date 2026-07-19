//! Usage: Auto-start side-effect helpers shared by settings-related flows.
//!
//! Lock order: CONFIG_IMPORT_LOCK -> AUTO_START_LOCK -> SETTINGS_WRITE_LOCK.
//! Holders of the settings write lock must never acquire AUTO_START_LOCK.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Precise ownership token for one durable autostart field commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoStartCommitToken {
    generation: u64,
    previous: bool,
    committed: bool,
}

impl AutoStartCommitToken {
    #[cfg(test)]
    pub(crate) fn previous(self) -> bool {
        self.previous
    }

    #[cfg(test)]
    pub(crate) fn committed(self) -> bool {
        self.committed
    }

    #[cfg(test)]
    pub(crate) fn generation(self) -> u64 {
        self.generation
    }
}

/// Result of an ownership-checked settings/autostart rollback.
#[derive(Debug)]
pub(crate) enum OwnedRollbackResult {
    Restored,
    ConcurrentWinner(Box<crate::settings::AppSettings>),
    Failed(String),
}

/// Whole-settings CAS commit outcome used by config import.
#[derive(Debug)]
pub(crate) enum WholeSettingsCommitResult {
    /// CAS lost before any durable write of the import snapshot.
    ConcurrentUpdate,
    /// Durable snapshot committed (and OS converged best-effort when needed).
    Committed {
        settings: crate::settings::AppSettings,
        token: AutoStartCommitToken,
    },
    /// CAS succeeded but ownership/OS could not be converged; caller must
    /// roll back using the returned token + expected snapshot.
    CommitNeedsRollback {
        committed: crate::settings::AppSettings,
        token: AutoStartCommitToken,
        error: String,
    },
    /// Settings persistence failed before ownership was claimed.
    Failed(String),
}

struct AutoStartOwnerState {
    generation: u64,
}

#[cfg(test)]
static AUTO_START_LOCK_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
type AfterWholeSettingsCasTestHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
fn after_whole_settings_cas_test_hook() -> &'static Mutex<Option<AfterWholeSettingsCasTestHook>> {
    static HOOK: OnceLock<Mutex<Option<AfterWholeSettingsCasTestHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn set_after_whole_settings_cas_test_hook(hook: AfterWholeSettingsCasTestHook) {
    *after_whole_settings_cas_test_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
}

#[cfg(test)]
fn run_after_whole_settings_cas_test_hook() {
    let hook = after_whole_settings_cas_test_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
pub(crate) fn reset_auto_start_lock_attempts_for_test() {
    AUTO_START_LOCK_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn auto_start_lock_attempts_for_test() -> usize {
    AUTO_START_LOCK_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst)
}

fn owner_state() -> &'static Mutex<AutoStartOwnerState> {
    static STATE: OnceLock<Mutex<AutoStartOwnerState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AutoStartOwnerState { generation: 0 }))
}

fn lock_owner() -> MutexGuard<'static, AutoStartOwnerState> {
    #[cfg(test)]
    AUTO_START_LOCK_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    owner_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Compute the next generation without publishing it. Fail closed before any
/// durable mutation when overflow would make tokens non-unique.
fn next_generation(owner: &AutoStartOwnerState) -> Result<u64, String> {
    owner.generation.checked_add(1).ok_or_else(|| {
        "SYSTEM_ERROR: autostart generation overflow; process restart required".to_string()
    })
}

/// Publish a previously reserved generation. Call only after durable mutation
/// succeeded so losers/failures never advance the owner generation.
fn publish_generation(owner: &mut AutoStartOwnerState, reserved: u64) {
    debug_assert!(reserved == owner.generation.saturating_add(1) || reserved > owner.generation);
    owner.generation = reserved;
}

/// Reserve+publish in one step. Production paths prefer explicit reserve-before-
/// mutation + publish-after-success; this helper remains for unit tests that
/// exercise pure arithmetic overflow without a full settings environment.
#[cfg(test)]
fn bump_generation(owner: &mut AutoStartOwnerState) -> Result<u64, String> {
    let next = next_generation(owner)?;
    publish_generation(owner, next);
    Ok(next)
}

#[cfg(test)]
fn force_owner_generation_for_test(value: u64) {
    let mut owner = lock_owner();
    owner.generation = value;
}

#[cfg(test)]
fn owner_generation_for_test() -> u64 {
    lock_owner().generation
}

fn next_auto_start_with_sync(
    previous_auto_start: bool,
    desired_auto_start: bool,
    force_sync: bool,
    sync: impl FnOnce(bool) -> Result<(), String>,
) -> Result<bool, String> {
    if !force_sync && previous_auto_start == desired_auto_start {
        return Ok(desired_auto_start);
    }

    sync(desired_auto_start)?;
    Ok(desired_auto_start)
}

#[cfg(desktop)]
fn sync_auto_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    enable_auto_start: bool,
) -> Result<(), String> {
    #[cfg(test)]
    record_auto_start_sync_call(enable_auto_start);
    use tauri::Manager;
    use tauri_plugin_autostart::ManagerExt;

    if app
        .try_state::<tauri_plugin_autostart::AutoLaunchManager>()
        .is_none()
    {
        tracing::debug!("auto-start plugin not initialized, skipping sync");
        return Ok(());
    }

    if enable_auto_start {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("failed to enable autostart: {e}"))
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("failed to disable autostart: {e}"))
    }
}

#[cfg(not(desktop))]
fn sync_auto_start<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    enable_auto_start: bool,
) -> Result<(), String> {
    #[cfg(test)]
    record_auto_start_sync_call(enable_auto_start);
    Ok(())
}

#[cfg(test)]
static AUTO_START_SYNC_TEST_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
static AUTO_START_DURABLE_MUTATION_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn auto_start_test_serial_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
fn auto_start_sync_targets() -> &'static Mutex<Vec<bool>> {
    static TARGETS: OnceLock<Mutex<Vec<bool>>> = OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
type AutoStartSyncFailureHook = Box<dyn FnMut(bool) -> Option<String> + Send>;

#[cfg(test)]
fn auto_start_sync_failure_hook() -> &'static Mutex<Option<AutoStartSyncFailureHook>> {
    static HOOK: OnceLock<Mutex<Option<AutoStartSyncFailureHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn record_auto_start_sync_call(enable_auto_start: bool) {
    AUTO_START_SYNC_TEST_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    auto_start_sync_targets()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(enable_auto_start);
}

#[cfg(test)]
pub(crate) fn reset_auto_start_sync_test_calls() {
    AUTO_START_SYNC_TEST_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
    auto_start_sync_targets()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    *auto_start_sync_failure_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(test)]
pub(crate) fn reset_auto_start_durable_mutation_test_calls() {
    AUTO_START_DURABLE_MUTATION_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn auto_start_durable_mutation_test_calls() -> usize {
    AUTO_START_DURABLE_MUTATION_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
fn record_auto_start_durable_mutation_attempt() {
    AUTO_START_DURABLE_MUTATION_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn auto_start_sync_test_calls() -> usize {
    AUTO_START_SYNC_TEST_CALLS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn auto_start_sync_test_targets() -> Vec<bool> {
    auto_start_sync_targets()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
pub(crate) fn set_auto_start_sync_failure_hook(
    hook: Box<dyn FnMut(bool) -> Option<String> + Send>,
) {
    *auto_start_sync_failure_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
}

#[cfg(test)]
fn clear_auto_start_sync_failure_hook() {
    *auto_start_sync_failure_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(test)]
fn maybe_inject_auto_start_sync_failure(enable_auto_start: bool) -> Result<(), String> {
    let mut guard = auto_start_sync_failure_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // One-shot: consume the hook so parallel tests cannot observe a leftover injector.
    if let Some(mut hook) = guard.take() {
        if let Some(error) = hook(enable_auto_start) {
            return Err(error);
        }
        // Hook chose not to fail this call; restore for a later attempt.
        *guard = Some(hook);
    }
    Ok(())
}

fn perform_os_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    enable_auto_start: bool,
) -> Result<(), String> {
    #[cfg(test)]
    maybe_inject_auto_start_sync_failure(enable_auto_start)?;
    sync_auto_start(app, enable_auto_start)
}

/// Conditionally repair the gateway's effective preferred port under the same
/// owner generation used by settings writers. The repair owns only the port;
/// callers must keep any ordinary-field token based on their original update.
pub(crate) fn repair_preferred_port_if_current<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    expected_port: u16,
    repaired_port: u16,
) -> Result<(crate::settings::AppSettings, bool), String> {
    let mut owner = lock_owner();
    let reserved = next_generation(&owner)?;
    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let update = crate::settings::update(app, |latest| {
        if latest.preferred_port != expected_port {
            return Ok(false);
        }
        latest.preferred_port = repaired_port;
        Ok(true)
    })?;
    if update.1 {
        publish_generation(&mut owner, reserved);
    }
    Ok(update)
}

/// Outcome of ownership-checked auto_start correction after OS sync failure.
#[derive(Debug)]
enum AutoStartCorrection {
    /// Correction persisted and OS converged; carries the current valid token.
    Corrected {
        effective: bool,
        token: AutoStartCommitToken,
    },
    /// Another owner won; only converge OS to canonical winner.
    ConcurrentWinner { effective: bool },
}

/// Commit autostart-related settings under the shared owner lock, then sync OS.
///
/// `commit` must only touch settings (SETTINGS_WRITE_LOCK) and must not re-enter
/// this coordinator. On success the returned token identifies the *current*
/// generation after any ownership-checked correction.
pub(crate) fn commit_auto_start_with_owner<R, F, T>(
    app: &tauri::AppHandle<R>,
    desired_auto_start: bool,
    commit: F,
) -> Result<(T, AutoStartCommitToken, bool), String>
where
    R: tauri::Runtime,
    F: FnOnce() -> Result<(T, bool, bool), String>,
{
    let mut owner = lock_owner();
    // Reserve next generation BEFORE any durable mutation. Overflow fails closed
    // without running commit() / advancing ownership.
    let reserved = next_generation(&owner)?;
    let _ = desired_auto_start;
    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let (value, previous_auto_start, committed_auto_start) = commit()?;
    debug_assert_eq!(desired_auto_start, committed_auto_start);
    publish_generation(&mut owner, reserved);
    let token = AutoStartCommitToken {
        generation: reserved,
        previous: previous_auto_start,
        committed: committed_auto_start,
    };

    let os_target = committed_auto_start;
    match next_auto_start_with_sync(previous_auto_start, os_target, true, |enable| {
        perform_os_sync(app, enable)
    }) {
        Ok(effective) => Ok((value, token, effective)),
        Err(sync_error) => {
            // Correction may reserve a new generation before mutating.
            match correct_auto_start_on_os_failure(app, &mut owner, token, previous_auto_start) {
                Ok(AutoStartCorrection::Corrected { effective, token }) => {
                    tracing::warn!(
                        error = %sync_error,
                        effective_auto_start = effective,
                        "auto-start OS sync failed; settings kept with effective auto_start"
                    );
                    Ok((value, token, effective))
                }
                Ok(AutoStartCorrection::ConcurrentWinner { effective }) => {
                    tracing::warn!(
                        error = %sync_error,
                        effective_auto_start = effective,
                        "auto-start OS sync failed and ownership lost; converging to winner"
                    );
                    Err(format!(
                        "{sync_error}; autostart ownership lost to concurrent winner (effective={effective})"
                    ))
                }
                Err(correction_error) => Err(format!(
                    "{sync_error}; autostart correction also failed: {correction_error}"
                )),
            }
        }
    }
}

fn correct_auto_start_on_os_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    owner: &mut AutoStartOwnerState,
    token: AutoStartCommitToken,
    previous_auto_start: bool,
) -> Result<AutoStartCorrection, String> {
    if owner.generation != token.generation {
        let canonical = crate::settings::read(app)
            .map(|settings| settings.auto_start)
            .unwrap_or(previous_auto_start);
        perform_os_sync(app, canonical)?;
        return Ok(AutoStartCorrection::ConcurrentWinner {
            effective: canonical,
        });
    }

    // Reserve correction generation before mutating settings.
    let reserved = next_generation(owner)?;
    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let correction = crate::settings::update(app, |latest| {
        if latest.auto_start != token.committed {
            return Ok(false);
        }
        latest.auto_start = previous_auto_start;
        Ok(true)
    })
    .map_err(|err| err.to_string())?;

    match correction {
        (_, true) => {
            publish_generation(owner, reserved);
            perform_os_sync(app, previous_auto_start)?;
            Ok(AutoStartCorrection::Corrected {
                effective: previous_auto_start,
                token: AutoStartCommitToken {
                    generation: reserved,
                    previous: previous_auto_start,
                    committed: previous_auto_start,
                },
            })
        }
        (latest, false) => {
            // Concurrent loser: do not publish reserved generation.
            perform_os_sync(app, latest.auto_start)?;
            Ok(AutoStartCorrection::ConcurrentWinner {
                effective: latest.auto_start,
            })
        }
    }
}

/// Whole-settings CAS path used by config import. Reserves the next generation
/// *before* any durable CAS so overflow fails closed with zero settings mutation.
/// Publishes the reserved generation only after CAS succeeds, then syncs OS under
/// the same owner lock.
///
/// Best-effort: if OS sync fails but auto_start correction + final OS convergence
/// succeed, returns `Committed` with the effective canonical snapshot so other
/// imported fields are not discarded.
pub(crate) fn commit_whole_settings_with_auto_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_settings: &crate::settings::AppSettings,
    next_settings: &crate::settings::AppSettings,
) -> WholeSettingsCommitResult {
    let mut owner = lock_owner();
    // Reserve BEFORE durable CAS. Overflow must not run compare_and_swap.
    let reserved = match next_generation(&owner) {
        Ok(generation) => generation,
        Err(err) => return WholeSettingsCommitResult::Failed(err),
    };
    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let cas = crate::settings::compare_and_swap(app, previous_settings, next_settings);
    let (latest, committed) = match cas {
        Ok(pair) => pair,
        Err(err) => return WholeSettingsCommitResult::Failed(err.to_string()),
    };
    if !committed {
        // CAS lost: do not publish the reserved generation.
        return WholeSettingsCommitResult::ConcurrentUpdate;
    }

    publish_generation(&mut owner, reserved);
    let token = AutoStartCommitToken {
        generation: reserved,
        previous: previous_settings.auto_start,
        committed: next_settings.auto_start,
    };

    #[cfg(test)]
    run_after_whole_settings_cas_test_hook();

    match next_auto_start_with_sync(
        previous_settings.auto_start,
        next_settings.auto_start,
        true,
        |enable| perform_os_sync(app, enable),
    ) {
        Ok(_) => WholeSettingsCommitResult::Committed {
            settings: latest,
            token,
        },
        Err(sync_error) => {
            // Correct auto_start only while this token still owns generation.
            if owner.generation != token.generation {
                let winner = crate::settings::read(app).unwrap_or(latest.clone());
                let convergence = perform_os_sync(app, winner.auto_start).err();
                // Do NOT pass concurrent winner as this import's committed snapshot.
                return WholeSettingsCommitResult::CommitNeedsRollback {
                    committed: latest,
                    token,
                    error: match convergence {
                        Some(convergence_error) => format!(
                            "{sync_error}; autostart ownership lost before correction; canonical convergence failed: {convergence_error}"
                        ),
                        None => format!(
                            "{sync_error}; autostart ownership lost before correction"
                        ),
                    },
                };
            }

            // Reserve correction generation before mutating auto_start.
            let correction_reserved = match next_generation(&owner) {
                Ok(generation) => generation,
                Err(err) => {
                    return WholeSettingsCommitResult::CommitNeedsRollback {
                        committed: latest,
                        token,
                        error: format!("{sync_error}; {err}"),
                    };
                }
            };
            #[cfg(test)]
            record_auto_start_durable_mutation_attempt();
            let correction = crate::settings::update(app, |current| {
                if current.auto_start != token.committed {
                    return Ok(false);
                }
                current.auto_start = token.previous;
                Ok(true)
            });

            match correction {
                Ok((_settings_after, true)) => {
                    publish_generation(&mut owner, correction_reserved);
                    let corrected_token = AutoStartCommitToken {
                        generation: correction_reserved,
                        previous: token.previous,
                        committed: token.previous,
                    };
                    // Keep the import-owned expected snapshot separate from the
                    // latest canonical value read by the correction update.
                    let mut corrected_settings = latest.clone();
                    corrected_settings.auto_start = token.previous;
                    match perform_os_sync(app, token.previous) {
                        Ok(()) => {
                            // The correction update intentionally reads the latest
                            // canonical snapshot. That snapshot may include fields
                            // written by another settings owner after this import's
                            // CAS, so rollback must never absorb those fields into
                            // A's token.
                            tracing::warn!(
                                error = %sync_error,
                                "auto-start OS sync failed; import kept with corrected auto_start"
                            );
                            WholeSettingsCommitResult::Committed {
                                settings: corrected_settings,
                                token: corrected_token,
                            }
                        }
                        Err(os_error) => WholeSettingsCommitResult::CommitNeedsRollback {
                            committed: corrected_settings,
                            token: corrected_token,
                            error: format!("{sync_error}; final OS convergence failed: {os_error}"),
                        },
                    }
                }
                Ok((winner, false)) => {
                    let convergence = perform_os_sync(app, winner.auto_start);
                    // Keep expected snapshot as *this import's* CAS result, not
                    // the concurrent winner. Whole rollback must not overwrite winner.
                    WholeSettingsCommitResult::CommitNeedsRollback {
                        committed: latest,
                        token,
                        error: match convergence {
                            Ok(()) => format!(
                                "{sync_error}; auto_start corrected by concurrent owner"
                            ),
                            Err(convergence_error) => format!(
                                "{sync_error}; auto_start corrected by concurrent owner; canonical convergence failed: {convergence_error}"
                            ),
                        },
                    }
                }
                Err(err) => WholeSettingsCommitResult::CommitNeedsRollback {
                    committed: latest,
                    token,
                    error: format!("{sync_error}; auto_start correction persistence failed: {err}"),
                },
            }
        }
    }
}

/// Roll back ordinary-owned fields, optionally with auto-start ownership.
///
/// `Some(token)` validates generation, auto_start committed value, and the
/// caller-supplied ordinary equality under AUTO_START then SETTINGS before
/// restoring and converging the OS. `None` uses SETTINGS only and never reads
/// or writes OS auto-start state.
pub(crate) fn rollback_owned_with_auto_start_token<R, F>(
    app: &tauri::AppHandle<R>,
    auto_start_token: Option<AutoStartCommitToken>,
    restore_if_owned: F,
) -> OwnedRollbackResult
where
    R: tauri::Runtime,
    F: FnOnce(&mut crate::settings::AppSettings) -> bool,
{
    let token = match auto_start_token {
        None => {
            let update = crate::settings::update(app, |latest| Ok(restore_if_owned(latest)));
            return match update {
                Ok((_, true)) => OwnedRollbackResult::Restored,
                Ok((winner, false)) => OwnedRollbackResult::ConcurrentWinner(Box::new(winner)),
                Err(err) => OwnedRollbackResult::Failed(err.to_string()),
            };
        }
        Some(token) => token,
    };

    let mut owner = lock_owner();
    if owner.generation != token.generation {
        let winner = crate::settings::read(app).ok();
        let convergence = winner
            .as_ref()
            .map(|settings| perform_os_sync(app, settings.auto_start));
        if let Some(Err(error)) = convergence {
            return OwnedRollbackResult::Failed(format!(
                "autostart canonical convergence failed after losing ownership: {error}"
            ));
        }
        return match winner {
            Some(settings) => OwnedRollbackResult::ConcurrentWinner(Box::new(settings)),
            None => OwnedRollbackResult::Failed(
                "failed to read canonical settings after lost autostart ownership".into(),
            ),
        };
    }

    // Reserve before any durable restore while this path owns generation.
    // Overflow fails closed without running restore_if_owned / settings::update.
    let reserved = match next_generation(&owner) {
        Ok(generation) => generation,
        Err(err) => return OwnedRollbackResult::Failed(err),
    };

    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let update = crate::settings::update(app, |latest| {
        if latest.auto_start != token.committed {
            return Ok(false);
        }
        if !restore_if_owned(latest) {
            return Ok(false);
        }
        latest.auto_start = token.previous;
        Ok(true)
    });

    match update {
        Ok((_, true)) => {
            publish_generation(&mut owner, reserved);
            let effective = crate::settings::read(app)
                .map(|settings| settings.auto_start)
                .unwrap_or(token.previous);
            if let Err(err) = perform_os_sync(app, effective) {
                return OwnedRollbackResult::Failed(format!(
                    "settings restored but OS autostart convergence failed: {err}"
                ));
            }
            OwnedRollbackResult::Restored
        }
        Ok((winner, false)) => {
            // Loser: do not publish reserved generation.
            if let Err(err) = perform_os_sync(app, winner.auto_start) {
                return OwnedRollbackResult::Failed(format!(
                    "autostart canonical convergence failed after losing rollback ownership: {err}"
                ));
            }
            OwnedRollbackResult::ConcurrentWinner(Box::new(winner))
        }
        Err(err) => OwnedRollbackResult::Failed(err.to_string()),
    }
}

/// Whole-snapshot rollback for config import under the autostart owner lock.
pub(crate) fn rollback_whole_settings_with_auto_start_token<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_settings: &crate::settings::AppSettings,
    expected_committed: &crate::settings::AppSettings,
    token: AutoStartCommitToken,
) -> OwnedRollbackResult {
    let mut owner = lock_owner();
    let owns_auto_start_generation = owner.generation == token.generation;

    // Reserve before field-aware restore; overflow fails closed with no mutation.
    // When another owner already advanced the generation, this reservation lets a
    // partial restore publish the new canonical ownership without touching fields
    // that no longer equal this import's committed values.
    let reserved = match next_generation(&owner) {
        Ok(generation) => generation,
        Err(err) => return OwnedRollbackResult::Failed(err),
    };

    #[cfg(test)]
    record_auto_start_durable_mutation_attempt();
    let update = crate::settings::update(app, |latest| {
        if owns_auto_start_generation && settings_snapshots_equal(latest, expected_committed) {
            *latest = previous_settings.clone();
            return Ok(true);
        }
        // A concurrent writer may have changed only a subset of the imported
        // snapshot. Restore each field still owned by this import and preserve
        // every field whose current value differs from the import value. Once
        // generation ownership is stale, auto_start is never restored even when
        // its value still equals the import snapshot (same-value ABA).
        if owns_auto_start_generation {
            restore_owned_settings_fields(latest, previous_settings, expected_committed)
                .map_err(Into::into)
        } else {
            let mut previous_without_auto_start = previous_settings.clone();
            previous_without_auto_start.auto_start = expected_committed.auto_start;
            restore_owned_settings_fields(latest, &previous_without_auto_start, expected_committed)
                .map_err(Into::into)
        }
    });

    match update {
        Ok((_, true)) => {
            publish_generation(&mut owner, reserved);
            let effective_auto_start = crate::settings::read(app)
                .map(|settings| settings.auto_start)
                .unwrap_or(previous_settings.auto_start);
            if let Err(err) = perform_os_sync(app, effective_auto_start) {
                return OwnedRollbackResult::Failed(format!(
                    "whole settings restored but OS autostart convergence failed: {err}"
                ));
            }
            OwnedRollbackResult::Restored
        }
        Ok((winner, false)) => {
            // Loser: do not publish reserved generation.
            if let Err(err) = perform_os_sync(app, winner.auto_start) {
                return OwnedRollbackResult::Failed(format!(
                    "autostart canonical convergence failed after losing field-aware rollback ownership: {err}"
                ));
            }
            if owns_auto_start_generation {
                tracing::warn!(
                    "whole settings rollback found a concurrent field winner; preserving the newer snapshot"
                );
            }
            OwnedRollbackResult::ConcurrentWinner(Box::new(winner))
        }
        Err(err) => OwnedRollbackResult::Failed(err.to_string()),
    }
}

fn settings_snapshots_equal(
    left: &crate::settings::AppSettings,
    right: &crate::settings::AppSettings,
) -> bool {
    match (
        serde_json::to_value(left).ok(),
        serde_json::to_value(right).ok(),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Restore only fields that still contain this import's committed value. A
/// concurrent writer can therefore win individual settings fields without
/// forcing the rollback to abandon all other fields or overwrite the winner.
fn restore_owned_settings_fields(
    latest: &mut crate::settings::AppSettings,
    previous: &crate::settings::AppSettings,
    expected: &crate::settings::AppSettings,
) -> Result<bool, String> {
    let mut latest_value = serde_json::to_value(&*latest).map_err(|error| {
        format!("failed to serialize current settings for field rollback: {error}")
    })?;
    let previous_value = serde_json::to_value(previous).map_err(|error| {
        format!("failed to serialize previous settings for field rollback: {error}")
    })?;
    let expected_value = serde_json::to_value(expected).map_err(|error| {
        format!("failed to serialize expected settings for field rollback: {error}")
    })?;

    let Some(latest_object) = latest_value.as_object_mut() else {
        return Err("failed to serialize current settings: expected object".to_string());
    };
    let Some(previous_object) = previous_value.as_object() else {
        return Err("failed to serialize previous settings: expected object".to_string());
    };
    let Some(expected_object) = expected_value.as_object() else {
        return Err("failed to serialize expected settings: expected object".to_string());
    };

    let mut restored_any = false;
    for (field, expected_field) in expected_object {
        if latest_object.get(field) != Some(expected_field) {
            continue;
        }
        let Some(previous_field) = previous_object.get(field) else {
            continue;
        };
        if latest_object.get(field) != Some(previous_field) {
            latest_object.insert(field.clone(), previous_field.clone());
            restored_any = true;
        }
    }

    if !restored_any {
        return Ok(false);
    }

    *latest = serde_json::from_value(latest_value)
        .map_err(|error| format!("failed to deserialize field-aware settings rollback: {error}"))?;
    Ok(true)
}

/// Token-checked rollback of auto_start after a later runtime failure.
/// Prefer `rollback_owned_with_auto_start_token` when ordinary fields must also
/// be restored atomically.
pub(crate) fn rollback_auto_start_with_token_checked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    token: AutoStartCommitToken,
) -> OwnedRollbackResult {
    rollback_owned_with_auto_start_token(app, Some(token), |latest| {
        if latest.auto_start != token.committed {
            return false;
        }
        latest.auto_start = token.previous;
        true
    })
}

#[cfg(test)]
pub(crate) fn rollback_auto_start_with_token<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    token: AutoStartCommitToken,
) -> bool {
    match rollback_auto_start_with_token_checked(app, token) {
        OwnedRollbackResult::Restored => token.previous,
        OwnedRollbackResult::ConcurrentWinner(winner) => winner.auto_start,
        OwnedRollbackResult::Failed(_) => crate::settings::read(app)
            .map(|settings| settings.auto_start)
            .unwrap_or(token.committed),
    }
}

/// Converge OS autostart to the latest canonical settings value under the owner lock.
pub(crate) fn converge_auto_start_to_canonical<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<bool, String> {
    let _owner = lock_owner();
    let canonical = crate::settings::read(app)
        .map_err(|err| format!("failed to read canonical settings: {err}"))?
        .auto_start;
    perform_os_sync(app, canonical)?;
    Ok(canonical)
}

/// Force OS sync for a known desired value under the owner lock.
/// Used only by callers that already hold durable ownership of the value.
pub(crate) fn force_sync_auto_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    desired_auto_start: bool,
) -> Result<(), String> {
    let _owner = lock_owner();
    perform_os_sync(app, desired_auto_start)
}

pub(crate) fn restore_auto_start_best_effort<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    auto_start: bool,
) -> Result<(), String> {
    force_sync_auto_start(app, auto_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_auto_start_skips_sync_when_value_unchanged_and_not_forced() {
        let mut sync_called = false;
        let next_auto_start = next_auto_start_with_sync(true, true, false, |_| {
            sync_called = true;
            Ok(())
        })
        .expect("next auto start");

        assert!(next_auto_start);
        assert!(!sync_called);
    }

    #[test]
    fn next_auto_start_forces_sync_when_requested() {
        let mut sync_calls = Vec::new();
        let next_auto_start = next_auto_start_with_sync(true, true, true, |enable_auto_start| {
            sync_calls.push(enable_auto_start);
            Ok(())
        })
        .expect("next auto start");

        assert!(next_auto_start);
        assert_eq!(sync_calls, vec![true]);
    }

    #[test]
    fn next_auto_start_propagates_sync_error() {
        let err = next_auto_start_with_sync(false, true, true, |_| {
            Err("failed to enable autostart".to_string())
        })
        .expect_err("sync should fail");

        assert!(err.contains("failed to enable autostart"));
    }

    #[test]
    fn auto_start_sync_failure_hook_can_inject_os_errors() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        set_auto_start_sync_failure_hook(Box::new(|_| Some("injected os failure".to_string())));
        let app = tauri::test::mock_app();
        let err = force_sync_auto_start(app.handle(), true).expect_err("hook fails");
        assert!(err.contains("injected os failure"));
        clear_auto_start_sync_failure_hook();
    }

    #[test]
    fn commit_token_generation_is_monotonic_across_calls() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();

        let (_, token_a, _) =
            commit_auto_start_with_owner(&handle, false, || Ok(((), false, false)))
                .expect("commit a");
        let (_, token_b, _) = commit_auto_start_with_owner(&handle, true, || Ok(((), false, true)))
            .expect("commit b");

        assert!(token_b.generation() > token_a.generation());
        assert!(!token_a.previous());
        assert!(!token_a.committed());
        assert!(!token_b.previous());
        assert!(token_b.committed());
        assert_eq!(auto_start_sync_test_targets().last().copied(), Some(true));
    }

    #[test]
    fn generation_overflow_fails_closed() {
        let mut owner = AutoStartOwnerState {
            generation: u64::MAX,
        };
        let err = bump_generation(&mut owner).expect_err("overflow");
        assert!(err.contains("generation overflow"));
        assert_eq!(owner.generation, u64::MAX);
    }

    #[test]
    fn generation_overflow_skips_settings_commit_closure() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        reset_auto_start_durable_mutation_test_calls();
        force_owner_generation_for_test(u64::MAX);

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let mut closure_calls = 0usize;
        let err = commit_auto_start_with_owner(&handle, true, || {
            closure_calls += 1;
            Ok(((), false, true))
        })
        .expect_err("overflow must fail before commit closure");

        assert!(err.contains("generation overflow"), "unexpected: {err}");
        assert_eq!(closure_calls, 0, "commit closure must not run on overflow");
        assert_eq!(
            auto_start_durable_mutation_test_calls(),
            0,
            "durable mutation must not be attempted on overflow"
        );
        assert_eq!(
            auto_start_sync_test_calls(),
            0,
            "OS sync must not run on overflow"
        );
        assert_eq!(owner_generation_for_test(), u64::MAX);

        // Restore a usable generation for later tests that share the process owner.
        force_owner_generation_for_test(0);
    }

    #[test]
    fn generation_overflow_skips_whole_cas_mutation() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        reset_auto_start_durable_mutation_test_calls();
        force_owner_generation_for_test(u64::MAX);

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let old_dot = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
        std::env::set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(
                ".aio-autostart-overflow-{}",
                crate::shared::time::now_unix_millis()
            ),
        );
        crate::test_support::clear_settings_cache();

        let previous = crate::settings::read(&handle).expect("previous");
        let mut next = previous.clone();
        next.auto_start = true;
        next.log_retention_days = previous.log_retention_days.saturating_add(9);

        match commit_whole_settings_with_auto_start(&handle, &previous, &next) {
            WholeSettingsCommitResult::Failed(err) => {
                assert!(err.contains("generation overflow"), "unexpected: {err}");
            }
            other => panic!("overflow must fail before CAS: {other:?}"),
        }

        let canonical = crate::settings::read(&handle).expect("canonical unchanged");
        assert_eq!(canonical.log_retention_days, previous.log_retention_days);
        assert_eq!(canonical.auto_start, previous.auto_start);
        assert_eq!(
            auto_start_durable_mutation_test_calls(),
            0,
            "whole CAS must not be attempted on overflow"
        );
        assert_eq!(auto_start_sync_test_calls(), 0);
        assert_eq!(owner_generation_for_test(), u64::MAX);

        force_owner_generation_for_test(0);
        match old_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        match old_dot {
            Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
            None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
        }
        crate::test_support::clear_settings_cache();
    }

    #[test]
    fn generation_overflow_skips_correction_mutation_and_convergence() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        reset_auto_start_durable_mutation_test_calls();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let old_dot = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
        std::env::set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(
                ".aio-autostart-correction-overflow-{}",
                crate::shared::time::now_unix_millis()
            ),
        );
        crate::test_support::clear_settings_cache();

        let previous = crate::settings::read(&handle).expect("previous");
        let mut next = previous.clone();
        next.auto_start = true;
        next.log_retention_days = previous.log_retention_days.saturating_add(2);

        force_owner_generation_for_test(u64::MAX - 1);
        set_auto_start_sync_failure_hook(Box::new(|_| Some("initial OS failure".to_string())));
        let result = commit_whole_settings_with_auto_start(&handle, &previous, &next);
        clear_auto_start_sync_failure_hook();

        let error = match result {
            WholeSettingsCommitResult::CommitNeedsRollback { error, .. } => error,
            other => panic!("correction overflow must require rollback: {other:?}"),
        };
        assert!(error.contains("generation overflow"), "unexpected: {error}");
        assert_eq!(
            auto_start_durable_mutation_test_calls(),
            1,
            "initial whole CAS may commit, but correction mutation must not run"
        );
        assert_eq!(
            auto_start_sync_test_calls(),
            0,
            "correction/convergence OS sync must not run after reserve overflow"
        );
        assert_eq!(owner_generation_for_test(), u64::MAX);

        let canonical = crate::settings::read(&handle).expect("committed settings");
        assert_eq!(canonical.log_retention_days, next.log_retention_days);
        assert!(canonical.auto_start);

        // Restore the fixture directly after proving the coordinator failed closed;
        // the production rollback path is itself covered by the dedicated test.
        crate::settings::compare_and_swap(&handle, &next, &previous)
            .expect("restore overflow fixture");
        force_owner_generation_for_test(0);
        match old_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        match old_dot {
            Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
            None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
        }
        crate::test_support::clear_settings_cache();
    }

    #[test]
    fn generation_overflow_skips_owned_and_whole_rollback_mutation() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let old_dot = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
        std::env::set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(
                ".aio-autostart-rb-overflow-{}",
                crate::shared::time::now_unix_millis()
            ),
        );
        crate::test_support::clear_settings_cache();

        // Establish a real owner token at a finite generation, then force overflow.
        let previous = crate::settings::read(&handle).expect("previous");
        let mut committed = previous.clone();
        committed.auto_start = true;
        committed.log_retention_days = previous.log_retention_days.saturating_add(1);
        let token = match commit_whole_settings_with_auto_start(&handle, &previous, &committed) {
            WholeSettingsCommitResult::Committed { token, .. } => token,
            other => panic!("setup commit failed: {other:?}"),
        };
        reset_auto_start_sync_test_calls();
        reset_auto_start_durable_mutation_test_calls();
        force_owner_generation_for_test(token.generation());
        // Force next reserve to overflow while token still matches current generation.
        force_owner_generation_for_test(u64::MAX);
        // Re-align token generation so ownership check passes and reserve overflows.
        let overflow_token = AutoStartCommitToken {
            generation: u64::MAX,
            previous: token.previous(),
            committed: token.committed(),
        };

        let mut owned_restore_calls = 0usize;
        match rollback_owned_with_auto_start_token(&handle, Some(overflow_token), |latest| {
            owned_restore_calls += 1;
            latest.auto_start = overflow_token.previous();
            true
        }) {
            OwnedRollbackResult::Failed(err) => {
                assert!(err.contains("generation overflow"), "unexpected: {err}");
            }
            other => panic!("owned rollback overflow must fail closed: {other:?}"),
        }
        assert_eq!(
            owned_restore_calls, 0,
            "restore closure must not run on overflow"
        );
        assert_eq!(
            auto_start_durable_mutation_test_calls(),
            0,
            "owned rollback mutation must not be attempted on overflow"
        );

        match rollback_whole_settings_with_auto_start_token(
            &handle,
            &previous,
            &committed,
            overflow_token,
        ) {
            OwnedRollbackResult::Failed(err) => {
                assert!(err.contains("generation overflow"), "unexpected: {err}");
            }
            other => panic!("whole rollback overflow must fail closed: {other:?}"),
        }

        let canonical = crate::settings::read(&handle).expect("canonical");
        assert_eq!(canonical.log_retention_days, committed.log_retention_days);
        assert_eq!(canonical.auto_start, committed.auto_start);
        assert_eq!(
            auto_start_durable_mutation_test_calls(),
            0,
            "whole rollback mutation must not be attempted on overflow"
        );
        assert_eq!(
            auto_start_sync_test_calls(),
            0,
            "rollback OS sync must not run on overflow"
        );
        assert_eq!(owner_generation_for_test(), u64::MAX);

        force_owner_generation_for_test(0);
        match old_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        match old_dot {
            Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
            None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
        }
        crate::test_support::clear_settings_cache();
    }

    #[test]
    fn os_failure_correction_returns_current_generation_token() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let old_dot = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
        std::env::set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(
                ".aio-autostart-corr-{}",
                crate::shared::time::now_unix_millis()
            ),
        );
        crate::test_support::clear_settings_cache();

        // First OS sync fails once; correction + final OS sync succeed.
        let mut calls = 0usize;
        set_auto_start_sync_failure_hook(Box::new(move |_| {
            calls += 1;
            (calls == 1).then(|| "first os failure".to_string())
        }));

        let previous = crate::settings::read(&handle).expect("previous");
        assert!(!previous.auto_start);
        let mut next = previous.clone();
        next.auto_start = true;
        next.log_retention_days = previous.log_retention_days.saturating_add(3);

        let result = commit_whole_settings_with_auto_start(&handle, &previous, &next);
        let (settings, token) = match result {
            WholeSettingsCommitResult::Committed { settings, token } => (settings, token),
            other => panic!("expected committed after correction: {other:?}"),
        };
        // Effective auto_start corrected back to previous; token generation advanced.
        assert!(!settings.auto_start);
        assert!(!token.committed());
        assert!(token.generation() >= 2);

        // Subsequent rollback with the *current* token must restore ordinary fields.
        let previous_token_settings = previous.clone();
        match rollback_whole_settings_with_auto_start_token(
            &handle,
            &previous_token_settings,
            &settings,
            token,
        ) {
            OwnedRollbackResult::Restored => {}
            other => panic!("expected restored with current token: {other:?}"),
        }
        let canonical = crate::settings::read(&handle).expect("canonical");
        assert_eq!(canonical.log_retention_days, previous.log_retention_days);
        assert!(!canonical.auto_start);

        clear_auto_start_sync_failure_hook();
        match old_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        match old_dot {
            Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
            None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
        }
        crate::test_support::clear_settings_cache();
    }

    #[test]
    fn stale_token_rollback_converges_to_canonical_winner() {
        let _serial = auto_start_test_serial_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_auto_start_sync_test_calls();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let old_dot = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
        std::env::set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(
                ".aio-autostart-token-{}",
                crate::shared::time::now_unix_millis()
            ),
        );
        crate::test_support::clear_settings_cache();

        let previous = crate::settings::read(&handle).expect("previous");
        let mut first = previous.clone();
        first.auto_start = true;
        let token = match commit_whole_settings_with_auto_start(&handle, &previous, &first) {
            WholeSettingsCommitResult::Committed { token, .. } => token,
            other => panic!("first commit failed: {other:?}"),
        };

        let mut winner = first.clone();
        winner.auto_start = false;
        match commit_whole_settings_with_auto_start(&handle, &first, &winner) {
            WholeSettingsCommitResult::Committed { .. } => {}
            other => panic!("winner commit failed: {other:?}"),
        }

        let effective = rollback_auto_start_with_token(&handle, token);
        assert!(!effective);
        let canonical = crate::settings::read(&handle).expect("canonical");
        assert!(!canonical.auto_start);
        assert_eq!(auto_start_sync_test_targets().last().copied(), Some(false));

        match old_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        match old_dot {
            Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
            None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
        }
        crate::test_support::clear_settings_cache();
    }
}
