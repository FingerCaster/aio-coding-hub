//! Process-owned provider account-usage cache and bounded scheduler.

use crate::domain::provider_account_usage::{
    config_from_extension_values, custom_account_usage_authorization_fingerprint,
    custom_account_usage_permission_scope, ProviderAccountUsageAdapterKind,
    ProviderAccountUsageConfigState, ProviderAccountUsageRefreshSchedule,
    ProviderAccountUsageResult, ProviderAccountUsageStatus,
};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, Semaphore};

const DESKTOP_LEASE: Duration = Duration::from_secs(15);
const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub(crate) const SUCCESS_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_CONCURRENT_PROVIDER_FETCHES: usize = 4;

#[derive(Clone)]
pub(crate) struct ProviderAccountUsageRuntimeState {
    shared: Arc<RuntimeShared>,
}

impl Default for ProviderAccountUsageRuntimeState {
    fn default() -> Self {
        Self {
            shared: Arc::new(RuntimeShared {
                inner: Mutex::new(RuntimeInner::default()),
                fetch_limiter: Arc::new(Semaphore::new(MAX_CONCURRENT_PROVIDER_FETCHES)),
            }),
        }
    }
}

struct RuntimeShared {
    inner: Mutex<RuntimeInner>,
    fetch_limiter: Arc<Semaphore>,
}

#[derive(Default)]
struct RuntimeInner {
    entries: HashMap<i64, RuntimeEntry>,
    scheduler_running: bool,
}

struct RuntimeEntry {
    schedule: Option<ProviderAccountUsageRefreshSchedule>,
    config_token: Option<[u8; 32]>,
    snapshot: Option<ProviderAccountUsageSnapshot>,
    last_attempt_at: Option<Instant>,
    desktop_lease_until: Option<Instant>,
    generation: u64,
    in_flight_generation: Option<u64>,
    pending_force: bool,
    tail_force: bool,
    completion_generation: u64,
    completion: watch::Sender<u64>,
}

impl Default for RuntimeEntry {
    fn default() -> Self {
        let (completion, _) = watch::channel(0);
        Self {
            schedule: None,
            config_token: None,
            snapshot: None,
            last_attempt_at: None,
            desktop_lease_until: None,
            generation: 0,
            in_flight_generation: None,
            pending_force: false,
            tail_force: false,
            completion_generation: 0,
            completion,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderAccountUsageTarget {
    pub provider_id: i64,
    pub schedule: ProviderAccountUsageRefreshSchedule,
    config_token: [u8; 32],
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderAccountUsageSnapshot {
    pub result: ProviderAccountUsageResult,
    pub completed_at: Instant,
    pub generation: u64,
}

impl ProviderAccountUsageTarget {
    pub(crate) fn from_fetch_context(
        provider_id: i64,
        context: &crate::providers::ProviderAccountUsageFetchContext,
    ) -> Option<Self> {
        let ProviderAccountUsageConfigState::Configured(config) =
            config_from_extension_values(&context.extension_values)
        else {
            return None;
        };
        let schedule = ProviderAccountUsageRefreshSchedule {
            timed_refresh_enabled: config.timed_refresh_enabled,
            refresh_interval_seconds: config.refresh_interval_seconds,
        };
        let mut hasher = Sha256::new();
        hash_segment(&mut hasher, b"provider-account-usage-runtime-v1");
        hash_segment(&mut hasher, context.provider_uuid.as_bytes());
        hash_segment(&mut hasher, context.auth_mode.as_bytes());
        hash_segment(
            &mut hasher,
            context
                .source_provider_uuid
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        let base_url = context
            .base_urls
            .iter()
            .map(|value| value.trim())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let base_origin =
            crate::domain::provider_account_usage::custom_account_usage_base_origin(base_url)
                .unwrap_or_default();
        hash_segment(&mut hasher, base_origin.as_bytes());
        hash_segment(
            &mut hasher,
            match config.adapter_kind {
                ProviderAccountUsageAdapterKind::Sub2api => b"sub2api".as_slice(),
                ProviderAccountUsageAdapterKind::Newapi => b"newapi".as_slice(),
                ProviderAccountUsageAdapterKind::Custom => b"custom".as_slice(),
            },
        );
        hash_segment(
            &mut hasher,
            match config.new_api_query_mode {
                crate::domain::provider_account_usage::NewapiQueryMode::Billing => {
                    b"billing".as_slice()
                }
                crate::domain::provider_account_usage::NewapiQueryMode::Account => {
                    b"account".as_slice()
                }
            },
        );
        hash_segment(&mut hasher, &config.refresh_interval_seconds.to_be_bytes());
        if let Some(custom) = config.custom.as_ref() {
            hash_segment(&mut hasher, &[u8::from(custom.enabled)]);
            hash_segment(&mut hasher, &custom.timeout_seconds.to_be_bytes());
            let valid_permission = custom_account_usage_permission_scope(
                &context.provider_uuid,
                &context.auth_mode,
                context.source_provider_uuid.as_deref(),
                base_url,
            )
            .ok()
            .map(|scope| custom_account_usage_authorization_fingerprint(custom, &scope))
            .filter(|expected| {
                custom.enabled
                    && custom.permission_fingerprint.as_deref() == Some(expected.as_str())
            })
            .unwrap_or_default();
            hash_segment(&mut hasher, valid_permission.as_bytes());
        }
        Some(Self {
            provider_id,
            schedule,
            config_token: hasher.finalize().into(),
        })
    }
}

fn hash_segment(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

impl ProviderAccountUsageRuntimeState {
    pub(crate) async fn acquire_desktop_lease(
        &self,
        app: &tauri::AppHandle,
        target: ProviderAccountUsageTarget,
    ) -> Result<(), String> {
        let now = Instant::now();
        let should_start = {
            let mut inner = self.shared.inner.lock().await;
            let entry = inner.entries.entry(target.provider_id).or_default();
            sync_target(entry, &target)?;
            entry.desktop_lease_until = Some(now + DESKTOP_LEASE);
            if entry.snapshot.is_none() {
                entry.pending_force = true;
            }
            start_scheduler_if_needed(&mut inner, now)
        };
        if should_start {
            self.spawn_scheduler(app.clone());
        }
        self.dispatch_due(app.clone()).await;
        Ok(())
    }

    pub(crate) async fn heartbeat_desktop_lease(&self, provider_id: i64) -> bool {
        let mut inner = self.shared.inner.lock().await;
        let Some(entry) = inner.entries.get_mut(&provider_id) else {
            return false;
        };
        if entry.desktop_lease_until.is_none() || entry.schedule.is_none() {
            return false;
        }
        entry.desktop_lease_until = Some(Instant::now() + DESKTOP_LEASE);
        true
    }

    pub(crate) async fn release_desktop_lease(&self, provider_id: i64) {
        let mut inner = self.shared.inner.lock().await;
        if let Some(entry) = inner.entries.get_mut(&provider_id) {
            entry.desktop_lease_until = None;
        }
    }

    pub(crate) async fn refresh(
        &self,
        app: tauri::AppHandle,
        target: ProviderAccountUsageTarget,
    ) -> Result<ProviderAccountUsageResult, String> {
        let now = Instant::now();
        let (mut completion, wanted_completion, should_start) = {
            let mut inner = self.shared.inner.lock().await;
            let entry = inner.entries.entry(target.provider_id).or_default();
            sync_target(entry, &target)?;
            entry.desktop_lease_until = Some(now + DESKTOP_LEASE);
            let wanted_completion = request_forced_completion(entry);
            let completion = entry.completion.subscribe();
            let should_start = start_scheduler_if_needed(&mut inner, now);
            (completion, wanted_completion, should_start)
        };
        if should_start {
            self.spawn_scheduler(app.clone());
        }
        self.dispatch_due(app).await;
        loop {
            if *completion.borrow_and_update() >= wanted_completion {
                break;
            }
            if completion.changed().await.is_err() {
                break;
            }
            let inner = self.shared.inner.lock().await;
            if inner
                .entries
                .get(&target.provider_id)
                .and_then(|entry| entry.config_token)
                != Some(target.config_token)
            {
                return Ok(unavailable_result());
            }
        }
        Ok(self
            .snapshot(target.provider_id)
            .await
            .map(|snapshot| snapshot.result)
            .unwrap_or_else(unavailable_result))
    }

    pub(crate) async fn snapshot(&self, provider_id: i64) -> Option<ProviderAccountUsageSnapshot> {
        let now = Instant::now();
        let inner = self.shared.inner.lock().await;
        let entry = inner.entries.get(&provider_id)?;
        let snapshot = entry.snapshot.as_ref()?;
        if snapshot.generation != entry.generation {
            return None;
        }
        if is_success_result(&snapshot.result)
            && now.saturating_duration_since(snapshot.completed_at) >= SUCCESS_CACHE_TTL
        {
            return None;
        }
        Some(snapshot.clone())
    }

    pub(crate) async fn invalidate(&self, provider_id: i64) -> Result<(), String> {
        let mut inner = self.shared.inner.lock().await;
        if let Some(entry) = inner.entries.get_mut(&provider_id) {
            bump_generation(entry)?;
            entry.schedule = None;
            entry.config_token = None;
            entry.snapshot = None;
            entry.last_attempt_at = None;
            entry.desktop_lease_until = None;
            entry.pending_force = false;
            entry.tail_force = false;
            publish_completion(entry);
        }
        Ok(())
    }

    pub(crate) async fn reset_all(&self) {
        let mut inner = self.shared.inner.lock().await;
        inner.entries.clear();
        inner.scheduler_running = false;
    }

    fn spawn_scheduler(&self, app: tauri::AppHandle) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            state.run_scheduler(app).await;
        });
    }

    async fn run_scheduler(self, app: tauri::AppHandle) {
        loop {
            self.dispatch_due(app.clone()).await;
            tokio::time::sleep(SCHEDULER_TICK).await;
            let now = Instant::now();
            let mut inner = self.shared.inner.lock().await;
            expire_leases(&mut inner, now);
            if !inner
                .entries
                .values()
                .any(|entry| entry_has_active_consumer(entry, now))
            {
                inner.scheduler_running = false;
                return;
            }
        }
    }

    async fn dispatch_due(&self, app: tauri::AppHandle) {
        loop {
            let permit = match self.shared.fetch_limiter.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => return,
            };
            let selected = {
                let now = Instant::now();
                let mut inner = self.shared.inner.lock().await;
                expire_leases(&mut inner, now);
                inner.entries.iter_mut().find_map(|(provider_id, entry)| {
                    if !entry_has_active_consumer(entry, now) || !entry_is_due(entry, now) {
                        return None;
                    }
                    let generation = entry.generation;
                    entry.in_flight_generation = Some(generation);
                    entry.last_attempt_at = Some(now);
                    entry.pending_force = false;
                    Some((*provider_id, generation))
                })
            };
            let Some((provider_id, generation)) = selected else {
                drop(permit);
                return;
            };
            let state = self.clone();
            let app_for_fetch = app.clone();
            tauri::async_runtime::spawn(async move {
                state
                    .perform_refresh(app_for_fetch, provider_id, generation, permit)
                    .await;
            });
        }
    }

    async fn perform_refresh(
        &self,
        app: tauri::AppHandle,
        provider_id: i64,
        generation: u64,
        permit: OwnedSemaphorePermit,
    ) {
        let result =
            crate::commands::providers::fetch_account_usage_uncached(app.clone(), provider_id)
                .await
                .unwrap_or_else(|_| unavailable_result());
        let completed_at = Instant::now();
        {
            let mut inner = self.shared.inner.lock().await;
            let Some(entry) = inner.entries.get_mut(&provider_id) else {
                return;
            };
            if entry.in_flight_generation != Some(generation) {
                return;
            }
            entry.in_flight_generation = None;
            if entry.generation == generation && entry.schedule.is_some() {
                entry.snapshot = Some(ProviderAccountUsageSnapshot {
                    result,
                    completed_at,
                    generation,
                });
            }
            if entry.tail_force {
                entry.tail_force = false;
                entry.pending_force = true;
            }
            publish_completion(entry);
        }
        drop(permit);
    }
}

fn sync_target(
    entry: &mut RuntimeEntry,
    target: &ProviderAccountUsageTarget,
) -> Result<(), String> {
    if entry.config_token != Some(target.config_token) {
        bump_generation(entry)?;
        entry.config_token = Some(target.config_token);
        entry.schedule = Some(target.schedule);
        entry.snapshot = None;
        entry.last_attempt_at = None;
        entry.pending_force = true;
        entry.tail_force = false;
        publish_completion(entry);
    } else {
        entry.schedule = Some(target.schedule);
    }
    Ok(())
}

fn bump_generation(entry: &mut RuntimeEntry) -> Result<(), String> {
    entry.generation = entry
        .generation
        .checked_add(1)
        .ok_or_else(|| "SYSTEM_ERROR: account usage generation exhausted".to_string())?;
    Ok(())
}

fn publish_completion(entry: &mut RuntimeEntry) {
    entry.completion_generation = entry.completion_generation.saturating_add(1);
    entry.completion.send_replace(entry.completion_generation);
}

fn request_forced_completion(entry: &mut RuntimeEntry) -> u64 {
    if entry.in_flight_generation.is_some() {
        entry.tail_force = true;
        entry.completion_generation.saturating_add(2)
    } else {
        entry.pending_force = true;
        entry.completion_generation.saturating_add(1)
    }
}

fn start_scheduler_if_needed(inner: &mut RuntimeInner, now: Instant) -> bool {
    if inner.scheduler_running
        || !inner
            .entries
            .values()
            .any(|entry| entry_has_active_consumer(entry, now))
    {
        return false;
    }
    inner.scheduler_running = true;
    true
}

fn expire_leases(inner: &mut RuntimeInner, now: Instant) {
    for entry in inner.entries.values_mut() {
        if entry
            .desktop_lease_until
            .is_some_and(|deadline| deadline <= now)
        {
            entry.desktop_lease_until = None;
        }
    }
}

fn entry_has_active_consumer(entry: &RuntimeEntry, now: Instant) -> bool {
    entry.schedule.is_some()
        && entry
            .desktop_lease_until
            .is_some_and(|deadline| deadline > now)
}

fn entry_is_due(entry: &RuntimeEntry, now: Instant) -> bool {
    let Some(schedule) = entry.schedule else {
        return false;
    };
    if entry.in_flight_generation.is_some() {
        return false;
    }
    if entry.pending_force || entry.snapshot.is_none() {
        return true;
    }
    let Some(last_attempt_at) = entry.last_attempt_at else {
        return true;
    };
    if entry.snapshot.as_ref().is_some_and(|snapshot| {
        is_success_result(&snapshot.result)
            && now.saturating_duration_since(snapshot.completed_at) >= SUCCESS_CACHE_TTL
    }) {
        return true;
    }
    schedule.timed_refresh_enabled
        && now.duration_since(last_attempt_at)
            >= Duration::from_secs(schedule.refresh_interval_seconds as u64)
}

fn is_success_result(result: &ProviderAccountUsageResult) -> bool {
    matches!(
        result.status,
        ProviderAccountUsageStatus::Available
            | ProviderAccountUsageStatus::ZeroBalance
            | ProviderAccountUsageStatus::Expired
    )
}

fn unavailable_result() -> ProviderAccountUsageResult {
    ProviderAccountUsageResult::local_status(
        None,
        ProviderAccountUsageStatus::QueryFailed,
        "账户用量尚未获取",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::provider_account_usage::{
        ProviderAccountUsageFreshness, ProviderAccountUsageStatus,
    };

    fn schedule(timed_refresh_enabled: bool) -> ProviderAccountUsageRefreshSchedule {
        ProviderAccountUsageRefreshSchedule {
            timed_refresh_enabled,
            refresh_interval_seconds: 60,
        }
    }

    fn result() -> ProviderAccountUsageResult {
        let mut result = ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Newapi,
            ProviderAccountUsageStatus::Available,
            100,
        );
        result.freshness = ProviderAccountUsageFreshness::Fresh;
        result.balance = Some(12.5);
        result
    }

    fn entry_with_snapshot(timed_refresh_enabled: bool) -> RuntimeEntry {
        let now = Instant::now();
        RuntimeEntry {
            schedule: Some(schedule(timed_refresh_enabled)),
            config_token: Some([1; 32]),
            snapshot: Some(ProviderAccountUsageSnapshot {
                result: result(),
                completed_at: now,
                generation: 1,
            }),
            last_attempt_at: Some(now),
            desktop_lease_until: Some(now + DESKTOP_LEASE),
            generation: 1,
            ..RuntimeEntry::default()
        }
    }

    #[test]
    fn saved_interval_controls_due_refresh_and_hard_expiry_is_independent() {
        let timed = entry_with_snapshot(true);
        let started_at = timed.last_attempt_at.expect("last attempt");
        assert!(!entry_is_due(&timed, started_at + Duration::from_secs(59)));
        assert!(entry_is_due(&timed, started_at + Duration::from_secs(60)));

        let untimed = entry_with_snapshot(false);
        let completed_at = untimed.snapshot.as_ref().expect("snapshot").completed_at;
        assert!(!entry_is_due(
            &untimed,
            completed_at + Duration::from_secs(60)
        ));
        assert!(entry_is_due(&untimed, completed_at + SUCCESS_CACHE_TTL));
    }

    #[test]
    fn force_arriving_during_in_flight_requests_one_tail_completion() {
        let mut entry = entry_with_snapshot(true);
        entry.in_flight_generation = Some(entry.generation);
        let current_completion = entry.completion_generation;
        assert_eq!(
            request_forced_completion(&mut entry),
            current_completion.saturating_add(2)
        );
        entry.in_flight_generation = None;
        if entry.tail_force {
            entry.tail_force = false;
            entry.pending_force = true;
        }
        publish_completion(&mut entry);
        assert!(entry.pending_force);
        assert_eq!(entry.completion_generation, current_completion + 1);
    }

    #[test]
    fn force_while_idle_requests_only_the_next_completion() {
        let mut entry = entry_with_snapshot(true);
        let current_completion = entry.completion_generation;

        assert_eq!(
            request_forced_completion(&mut entry),
            current_completion.saturating_add(1)
        );
        assert!(entry.pending_force);
        assert!(!entry.tail_force);
    }

    #[test]
    fn generation_never_wraps() {
        let mut entry = RuntimeEntry {
            generation: u64::MAX,
            ..RuntimeEntry::default()
        };
        assert!(bump_generation(&mut entry).is_err());
        assert_eq!(entry.generation, u64::MAX);
    }

    #[test]
    fn limiter_is_nonwaiting_and_caps_spawnable_fetches() {
        let limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_PROVIDER_FETCHES));
        let permits = (0..MAX_CONCURRENT_PROVIDER_FETCHES)
            .map(|_| limiter.clone().try_acquire_owned().expect("permit"))
            .collect::<Vec<_>>();
        assert!(matches!(
            limiter.clone().try_acquire_owned(),
            Err(tokio::sync::TryAcquireError::NoPermits)
        ));
        drop(permits);
    }
}
