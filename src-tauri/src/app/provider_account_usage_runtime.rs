//! Process-owned provider account-usage cache and bounded scheduler.

pub(crate) use crate::domain::provider_account_usage::ProviderAccountUsageTarget;
use crate::domain::provider_account_usage::{
    ProviderAccountUsageAdapterKind, ProviderAccountUsageFreshness,
    ProviderAccountUsageRefreshSchedule, ProviderAccountUsageResult, ProviderAccountUsageStatus,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, Semaphore};

const DESKTOP_LEASE: Duration = Duration::from_secs(15);
const GATEWAY_LEASE: Duration = Duration::from_secs(15);
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
                route_entries: RwLock::new(HashMap::new()),
                global_recovery_epoch: AtomicU64::new(0),
                fetch_limiter: Arc::new(Semaphore::new(MAX_CONCURRENT_PROVIDER_FETCHES)),
            }),
        }
    }
}

struct RuntimeShared {
    inner: Mutex<RuntimeInner>,
    route_entries: RwLock<HashMap<i64, RouteEntry>>,
    global_recovery_epoch: AtomicU64,
    fetch_limiter: Arc<Semaphore>,
}

#[derive(Default)]
struct RuntimeInner {
    entries: HashMap<i64, RuntimeEntry>,
    scheduler_running: bool,
}

struct RuntimeEntry {
    schedule: Option<ProviderAccountUsageRefreshSchedule>,
    adapter_kind: Option<ProviderAccountUsageAdapterKind>,
    config_token: Option<[u8; 32]>,
    snapshot: Option<ProviderAccountUsageSnapshot>,
    last_attempt_at: Option<Instant>,
    desktop_lease_until: Option<Instant>,
    gateway_lease_until: Option<Instant>,
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
            adapter_kind: None,
            config_token: None,
            snapshot: None,
            last_attempt_at: None,
            desktop_lease_until: None,
            gateway_lease_until: None,
            generation: 0,
            in_flight_generation: None,
            pending_force: false,
            tail_force: false,
            completion_generation: 0,
            completion,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderAccountUsageSnapshot {
    pub result: ProviderAccountUsageResult,
    pub completed_at: Instant,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderAccountUsageBlockReason {
    ZeroBalance,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderAccountUsageRouteProjection {
    ConfirmedAvailable,
    Blocked(ProviderAccountUsageBlockReason),
    UnknownAllow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderAccountUsageRouteRead {
    pub projection: ProviderAccountUsageRouteProjection,
    pub recovery_epoch: u64,
}

impl Default for ProviderAccountUsageRouteRead {
    fn default() -> Self {
        Self {
            projection: ProviderAccountUsageRouteProjection::UnknownAllow,
            recovery_epoch: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastConfirmedRouteState {
    Available,
    Blocked,
}

struct RouteEntry {
    config_token: [u8; 32],
    generation: u64,
    adapter_kind: ProviderAccountUsageAdapterKind,
    refresh_interval_seconds: i64,
    snapshot: Option<ProviderAccountUsageSnapshot>,
    last_confirmed: Option<LastConfirmedRouteState>,
    provider_recovery_epoch: u64,
    gateway_lease_until: Option<Instant>,
}

impl RouteEntry {
    fn from_target(target: &ProviderAccountUsageTarget, generation: u64) -> Self {
        Self {
            config_token: target.config_token,
            generation,
            adapter_kind: target.adapter_kind,
            refresh_interval_seconds: target.schedule.refresh_interval_seconds,
            snapshot: None,
            last_confirmed: None,
            provider_recovery_epoch: 0,
            gateway_lease_until: None,
        }
    }
}

fn project_account_usage_route(
    entry: &RouteEntry,
    expected: &ProviderAccountUsageTarget,
    monotonic_now: Instant,
    wall_now_unix: i64,
) -> ProviderAccountUsageRouteProjection {
    if entry.config_token != expected.config_token
        || entry.generation == 0
        || entry.adapter_kind != expected.adapter_kind
        || entry.refresh_interval_seconds != expected.schedule.refresh_interval_seconds
    {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    }
    project_current_account_usage_route(entry, monotonic_now, wall_now_unix)
}

fn project_current_account_usage_route(
    entry: &RouteEntry,
    monotonic_now: Instant,
    wall_now_unix: i64,
) -> ProviderAccountUsageRouteProjection {
    let Some(snapshot) = entry.snapshot.as_ref() else {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    };
    if snapshot.generation != entry.generation
        || snapshot.completed_at > monotonic_now
        || snapshot.result.adapter_kind != Some(entry.adapter_kind)
        || snapshot.result.freshness != ProviderAccountUsageFreshness::Fresh
    {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    }
    let Some(last_fetched_at) = snapshot.result.last_fetched_at else {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    };
    if last_fetched_at <= 0 || last_fetched_at > wall_now_unix {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    }
    let Ok(refresh_interval_seconds) = u64::try_from(entry.refresh_interval_seconds) else {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    };
    let Some(route_ttl_seconds) = refresh_interval_seconds.checked_mul(2) else {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    };
    if monotonic_now.duration_since(snapshot.completed_at) >= Duration::from_secs(route_ttl_seconds)
    {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    }
    let numeric_values = [
        snapshot.result.balance,
        snapshot.result.plan_remaining,
        snapshot.result.used,
        snapshot.result.total,
        snapshot.result.daily_used,
        snapshot.result.daily_total,
        snapshot.result.weekly_used,
        snapshot.result.weekly_total,
        snapshot.result.monthly_used,
        snapshot.result.monthly_total,
    ];
    if numeric_values
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
        || snapshot.result.expires_at.is_some_and(|value| value <= 0)
    {
        return ProviderAccountUsageRouteProjection::UnknownAllow;
    }

    match snapshot.result.status {
        ProviderAccountUsageStatus::Available => {
            if snapshot
                .result
                .expires_at
                .is_some_and(|expires_at| expires_at <= wall_now_unix)
            {
                ProviderAccountUsageRouteProjection::UnknownAllow
            } else {
                ProviderAccountUsageRouteProjection::ConfirmedAvailable
            }
        }
        ProviderAccountUsageStatus::ZeroBalance => {
            if [snapshot.result.balance, snapshot.result.plan_remaining]
                .into_iter()
                .flatten()
                .any(|value| value > 0.0)
            {
                ProviderAccountUsageRouteProjection::UnknownAllow
            } else {
                ProviderAccountUsageRouteProjection::Blocked(
                    ProviderAccountUsageBlockReason::ZeroBalance,
                )
            }
        }
        ProviderAccountUsageStatus::Expired => {
            if snapshot
                .result
                .expires_at
                .is_some_and(|expires_at| expires_at > wall_now_unix)
            {
                ProviderAccountUsageRouteProjection::UnknownAllow
            } else {
                ProviderAccountUsageRouteProjection::Blocked(
                    ProviderAccountUsageBlockReason::Expired,
                )
            }
        }
        ProviderAccountUsageStatus::Unsupported
        | ProviderAccountUsageStatus::ConfigurationRequired
        | ProviderAccountUsageStatus::AuthFailed
        | ProviderAccountUsageStatus::QueryFailed => {
            ProviderAccountUsageRouteProjection::UnknownAllow
        }
    }
}

impl ProviderAccountUsageRuntimeState {
    pub(crate) async fn acquire_desktop_lease<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        target: ProviderAccountUsageTarget,
    ) -> Result<(), String> {
        let now = Instant::now();
        let should_start = {
            let mut inner = self.shared.inner.lock().await;
            let entry = inner.entries.entry(target.provider_id).or_default();
            let generation = sync_target(entry, &target)?;
            self.sync_route_target(&target, generation);
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

    pub(crate) async fn replace_gateway_targets<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        targets: Vec<ProviderAccountUsageTarget>,
    ) -> Result<(), String> {
        let now = Instant::now();
        let lease_until = now + GATEWAY_LEASE;
        let target_ids = targets
            .iter()
            .map(|target| target.provider_id)
            .collect::<HashSet<_>>();
        let (removed_ids, should_start) = {
            let mut inner = self.shared.inner.lock().await;
            for target in &targets {
                let entry = inner.entries.entry(target.provider_id).or_default();
                let generation = sync_target(entry, target)?;
                entry.gateway_lease_until = Some(lease_until);
                if entry.snapshot.is_none() {
                    entry.pending_force = true;
                }
                self.sync_route_target(target, generation);
                self.renew_route_gateway_lease(target.provider_id, lease_until);
            }
            let mut removed_ids = Vec::new();
            for (provider_id, entry) in &mut inner.entries {
                if entry.gateway_lease_until.is_some() && !target_ids.contains(provider_id) {
                    entry.gateway_lease_until = None;
                    removed_ids.push(*provider_id);
                }
            }
            let should_start = start_scheduler_if_needed(&mut inner, now);
            (removed_ids, should_start)
        };
        if !removed_ids.is_empty() {
            let mut entries = self.route_entries_write();
            for provider_id in removed_ids {
                entries.remove(&provider_id);
            }
        }
        if should_start {
            self.spawn_scheduler(app.clone());
        }
        self.dispatch_due(app.clone()).await;
        Ok(())
    }

    pub(crate) async fn release_gateway_targets(&self) {
        let mut inner = self.shared.inner.lock().await;
        for entry in inner.entries.values_mut() {
            entry.gateway_lease_until = None;
        }
        self.route_entries_write().clear();
    }

    pub(crate) async fn refresh<R: tauri::Runtime>(
        &self,
        app: tauri::AppHandle<R>,
        target: ProviderAccountUsageTarget,
    ) -> Result<ProviderAccountUsageResult, String> {
        let now = Instant::now();
        let (mut completion, wanted_completion, should_start) = {
            let mut inner = self.shared.inner.lock().await;
            let entry = inner.entries.entry(target.provider_id).or_default();
            let generation = sync_target(entry, &target)?;
            self.sync_route_target(&target, generation);
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
            entry.adapter_kind = None;
            entry.config_token = None;
            entry.snapshot = None;
            entry.last_attempt_at = None;
            entry.desktop_lease_until = None;
            entry.gateway_lease_until = None;
            entry.pending_force = false;
            entry.tail_force = false;
            publish_completion(entry);
        }
        self.route_entries_write().remove(&provider_id);
        Ok(())
    }

    pub(crate) async fn reset_all(&self) {
        let mut inner = self.shared.inner.lock().await;
        inner.entries.clear();
        inner.scheduler_running = false;
        self.route_entries_write().clear();
    }

    pub(crate) fn route_read(
        &self,
        target: &ProviderAccountUsageTarget,
        monotonic_now: Instant,
        wall_now_unix: i64,
    ) -> ProviderAccountUsageRouteRead {
        if !target.is_gateway_eligible() {
            return ProviderAccountUsageRouteRead::default();
        }
        let Ok(entries) = self.shared.route_entries.try_read() else {
            return ProviderAccountUsageRouteRead::default();
        };
        let Some(entry) = entries.get(&target.provider_id) else {
            return ProviderAccountUsageRouteRead::default();
        };
        if entry
            .gateway_lease_until
            .is_none_or(|deadline| deadline <= monotonic_now)
        {
            return ProviderAccountUsageRouteRead::default();
        }
        let projection = project_account_usage_route(entry, target, monotonic_now, wall_now_unix);
        let recovery_epoch =
            if projection == ProviderAccountUsageRouteProjection::ConfirmedAvailable {
                entry.provider_recovery_epoch
            } else {
                0
            };
        ProviderAccountUsageRouteRead {
            projection,
            recovery_epoch,
        }
    }

    pub(crate) fn global_recovery_epoch(&self) -> u64 {
        self.shared.global_recovery_epoch.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn seed_gateway_route_snapshot_for_tests(
        &self,
        target: &ProviderAccountUsageTarget,
        result: ProviderAccountUsageResult,
        completed_at: Instant,
        wall_now_unix: i64,
    ) {
        self.sync_route_target(target, 1);
        self.renew_route_gateway_lease(target.provider_id, completed_at + GATEWAY_LEASE);
        self.publish_route_snapshot(target.provider_id, 1, result, completed_at, wall_now_unix);
    }

    fn route_entries_write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<i64, RouteEntry>> {
        self.shared
            .route_entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn sync_route_target(&self, target: &ProviderAccountUsageTarget, generation: u64) {
        let mut entries = self.route_entries_write();
        if !target.is_gateway_eligible() {
            entries.remove(&target.provider_id);
            return;
        }
        let replace = entries.get(&target.provider_id).is_none_or(|entry| {
            entry.config_token != target.config_token || entry.generation != generation
        });
        if replace {
            entries.insert(
                target.provider_id,
                RouteEntry::from_target(target, generation),
            );
        }
    }

    fn renew_route_gateway_lease(&self, provider_id: i64, lease_until: Instant) {
        if let Some(entry) = self.route_entries_write().get_mut(&provider_id) {
            entry.gateway_lease_until = Some(lease_until);
        }
    }

    fn publish_route_snapshot(
        &self,
        provider_id: i64,
        generation: u64,
        result: ProviderAccountUsageResult,
        completed_at: Instant,
        wall_now_unix: i64,
    ) {
        let mut entries = self.route_entries_write();
        let Some(entry) = entries.get_mut(&provider_id) else {
            return;
        };
        if entry.generation != generation {
            return;
        }
        entry.snapshot = Some(ProviderAccountUsageSnapshot {
            result,
            completed_at,
            generation,
        });
        match project_current_account_usage_route(entry, completed_at, wall_now_unix) {
            ProviderAccountUsageRouteProjection::Blocked(_) => {
                entry.last_confirmed = Some(LastConfirmedRouteState::Blocked);
                entry.provider_recovery_epoch = 0;
            }
            ProviderAccountUsageRouteProjection::ConfirmedAvailable => {
                if entry.last_confirmed == Some(LastConfirmedRouteState::Blocked) {
                    let current = self.shared.global_recovery_epoch.load(Ordering::Relaxed);
                    entry.provider_recovery_epoch = current.checked_add(1).unwrap_or(0);
                    entry.last_confirmed = Some(LastConfirmedRouteState::Available);
                    if entry.provider_recovery_epoch != 0 {
                        self.shared
                            .global_recovery_epoch
                            .store(entry.provider_recovery_epoch, Ordering::Release);
                    }
                } else {
                    entry.last_confirmed = Some(LastConfirmedRouteState::Available);
                }
            }
            ProviderAccountUsageRouteProjection::UnknownAllow => {}
        }
    }

    fn spawn_scheduler<R: tauri::Runtime>(&self, app: tauri::AppHandle<R>) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            state.run_scheduler(app).await;
        });
    }

    async fn run_scheduler<R: tauri::Runtime>(self, app: tauri::AppHandle<R>) {
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

    async fn dispatch_due<R: tauri::Runtime>(&self, app: tauri::AppHandle<R>) {
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

    async fn perform_refresh<R: tauri::Runtime>(
        &self,
        app: tauri::AppHandle<R>,
        provider_id: i64,
        generation: u64,
        permit: OwnedSemaphorePermit,
    ) {
        let result =
            crate::commands::providers::fetch_account_usage_uncached(app.clone(), provider_id)
                .await
                .unwrap_or_else(|_| unavailable_result());
        let completed_at = Instant::now();
        let route_result = result.clone();
        let mut accepted = false;
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
                accepted = true;
            }
            if entry.tail_force {
                entry.tail_force = false;
                entry.pending_force = true;
            }
            publish_completion(entry);
        }
        if accepted {
            self.publish_route_snapshot(
                provider_id,
                generation,
                route_result,
                completed_at,
                crate::shared::time::now_unix_seconds(),
            );
        }
        drop(permit);
    }
}

fn sync_target(
    entry: &mut RuntimeEntry,
    target: &ProviderAccountUsageTarget,
) -> Result<u64, String> {
    if entry.config_token != Some(target.config_token) {
        bump_generation(entry)?;
        entry.config_token = Some(target.config_token);
        entry.schedule = Some(target.schedule);
        entry.adapter_kind = Some(target.adapter_kind);
        entry.snapshot = None;
        entry.last_attempt_at = None;
        entry.pending_force = true;
        entry.tail_force = false;
        publish_completion(entry);
    } else {
        entry.schedule = Some(target.schedule);
        entry.adapter_kind = Some(target.adapter_kind);
    }
    Ok(entry.generation)
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
        if entry
            .gateway_lease_until
            .is_some_and(|deadline| deadline <= now)
        {
            entry.gateway_lease_until = None;
        }
    }
}

fn entry_has_active_consumer(entry: &RuntimeEntry, now: Instant) -> bool {
    entry.schedule.is_some()
        && (entry
            .desktop_lease_until
            .is_some_and(|deadline| deadline > now)
            || entry
                .gateway_lease_until
                .is_some_and(|deadline| deadline > now))
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
    (schedule.timed_refresh_enabled
        || entry
            .gateway_lease_until
            .is_some_and(|deadline| deadline > now))
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
            route_gate_enabled: false,
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

    fn route_target_with_interval(
        token: u8,
        refresh_interval_seconds: i64,
    ) -> ProviderAccountUsageTarget {
        ProviderAccountUsageTarget {
            provider_id: 7,
            schedule: ProviderAccountUsageRefreshSchedule {
                timed_refresh_enabled: false,
                route_gate_enabled: true,
                refresh_interval_seconds,
            },
            adapter_kind: ProviderAccountUsageAdapterKind::Newapi,
            config_token: [token; 32],
            custom_permission_ready: true,
        }
    }

    fn route_target(token: u8) -> ProviderAccountUsageTarget {
        route_target_with_interval(token, 60)
    }

    fn route_result(
        status: ProviderAccountUsageStatus,
        last_fetched_at: i64,
    ) -> ProviderAccountUsageResult {
        ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Newapi,
            status,
            last_fetched_at,
        )
    }

    fn route_entry_with_result(
        target: &ProviderAccountUsageTarget,
        result: ProviderAccountUsageResult,
        completed_at: Instant,
    ) -> RouteEntry {
        let mut entry = RouteEntry::from_target(target, 1);
        entry.snapshot = Some(ProviderAccountUsageSnapshot {
            result,
            completed_at,
            generation: 1,
        });
        entry
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
    fn gateway_target_requires_an_explicit_route_gate_but_not_timed_refresh() {
        let mut context = crate::providers::ProviderAccountUsageFetchContext {
            provider_uuid: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            base_urls: vec!["https://api.example.test".to_string()],
            auth_mode: "api_key".to_string(),
            source_provider_id: None,
            source_provider_uuid: None,
            extension_values: Vec::new(),
        };
        assert!(ProviderAccountUsageTarget::from_gateway_fetch_context(7, &context).is_none());

        context.extension_values = vec![crate::providers::ProviderExtensionValues {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": "sub2api",
                "timedRefreshEnabled": false,
                "routeGateEnabled": false,
                "refreshIntervalSeconds": 60,
            }),
            updated_at: 1,
        }];
        assert!(ProviderAccountUsageTarget::from_gateway_fetch_context(7, &context).is_none());

        context.extension_values[0].values["routeGateEnabled"] = serde_json::Value::Bool(true);
        assert!(ProviderAccountUsageTarget::from_gateway_fetch_context(7, &context).is_some());

        context.auth_mode = "oauth".to_string();
        assert!(ProviderAccountUsageTarget::from_gateway_fetch_context(7, &context).is_none());
        context.auth_mode = "api_key".to_string();
        context.source_provider_id = Some(9);
        assert!(ProviderAccountUsageTarget::from_gateway_fetch_context(7, &context).is_none());
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

        let mut gateway = entry_with_snapshot(false);
        let started_at = gateway.last_attempt_at.expect("last attempt");
        gateway.gateway_lease_until = Some(started_at + Duration::from_secs(120));
        assert!(!entry_is_due(
            &gateway,
            started_at + Duration::from_secs(59)
        ));
        assert!(entry_is_due(&gateway, started_at + Duration::from_secs(60)));
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

    #[test]
    fn route_projection_uses_strict_monotonic_ttl_and_current_config_token() {
        let target = route_target(1);
        let completed_at = Instant::now();
        let entry = route_entry_with_result(
            &target,
            route_result(ProviderAccountUsageStatus::Available, 100),
            completed_at,
        );
        assert_eq!(
            project_account_usage_route(
                &entry,
                &target,
                completed_at + Duration::from_secs(119),
                100,
            ),
            ProviderAccountUsageRouteProjection::ConfirmedAvailable
        );
        assert_eq!(
            project_account_usage_route(
                &entry,
                &target,
                completed_at + Duration::from_secs(120),
                100,
            ),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let changed_target = route_target(2);
        assert_eq!(
            project_account_usage_route(&entry, &changed_target, completed_at, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let long_target = route_target_with_interval(1, 300);
        let long_entry = route_entry_with_result(
            &long_target,
            route_result(ProviderAccountUsageStatus::Available, 100),
            completed_at,
        );
        assert_eq!(
            project_account_usage_route(
                &long_entry,
                &long_target,
                completed_at + Duration::from_secs(599),
                100,
            ),
            ProviderAccountUsageRouteProjection::ConfirmedAvailable
        );
        assert_eq!(
            project_account_usage_route(
                &long_entry,
                &long_target,
                completed_at + Duration::from_secs(600),
                100,
            ),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let future_display = route_entry_with_result(
            &target,
            route_result(ProviderAccountUsageStatus::Available, 101),
            completed_at,
        );
        assert_eq!(
            project_account_usage_route(&future_display, &target, completed_at, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );
    }

    #[test]
    fn route_projection_blocks_only_consistent_zero_balance_or_expiry() {
        let target = route_target(1);
        let now = Instant::now();

        let zero = route_entry_with_result(
            &target,
            route_result(ProviderAccountUsageStatus::ZeroBalance, 100),
            now,
        );
        assert_eq!(
            project_account_usage_route(&zero, &target, now, 100),
            ProviderAccountUsageRouteProjection::Blocked(
                ProviderAccountUsageBlockReason::ZeroBalance
            )
        );

        let mut positive_result = route_result(ProviderAccountUsageStatus::ZeroBalance, 100);
        positive_result.plan_remaining = Some(0.01);
        let positive = route_entry_with_result(&target, positive_result, now);
        assert_eq!(
            project_account_usage_route(&positive, &target, now, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let mut negative_result = route_result(ProviderAccountUsageStatus::ZeroBalance, 100);
        negative_result.balance = Some(-0.01);
        let negative = route_entry_with_result(&target, negative_result, now);
        assert_eq!(
            project_account_usage_route(&negative, &target, now, 100),
            ProviderAccountUsageRouteProjection::Blocked(
                ProviderAccountUsageBlockReason::ZeroBalance
            )
        );

        let mut positive_balance_result =
            route_result(ProviderAccountUsageStatus::ZeroBalance, 100);
        positive_balance_result.balance = Some(0.01);
        let positive_balance = route_entry_with_result(&target, positive_balance_result, now);
        assert_eq!(
            project_account_usage_route(&positive_balance, &target, now, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let mut invalid_result = route_result(ProviderAccountUsageStatus::ZeroBalance, 100);
        invalid_result.balance = Some(f64::NAN);
        let invalid = route_entry_with_result(&target, invalid_result, now);
        assert_eq!(
            project_account_usage_route(&invalid, &target, now, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let mut expired_result = route_result(ProviderAccountUsageStatus::Expired, 100);
        expired_result.balance = Some(100.0);
        let expired = route_entry_with_result(&target, expired_result, now);
        assert_eq!(
            project_account_usage_route(&expired, &target, now, 100),
            ProviderAccountUsageRouteProjection::Blocked(ProviderAccountUsageBlockReason::Expired)
        );

        let mut contradictory_expiry = route_result(ProviderAccountUsageStatus::Expired, 100);
        contradictory_expiry.expires_at = Some(101);
        let contradictory = route_entry_with_result(&target, contradictory_expiry, now);
        assert_eq!(
            project_account_usage_route(&contradictory, &target, now, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        let mut past_expiry_available = route_result(ProviderAccountUsageStatus::Available, 100);
        past_expiry_available.expires_at = Some(100);
        let past_expiry_available = route_entry_with_result(&target, past_expiry_available, now);
        assert_eq!(
            project_account_usage_route(&past_expiry_available, &target, now, 100),
            ProviderAccountUsageRouteProjection::UnknownAllow
        );

        for status in [
            ProviderAccountUsageStatus::Unsupported,
            ProviderAccountUsageStatus::ConfigurationRequired,
            ProviderAccountUsageStatus::AuthFailed,
            ProviderAccountUsageStatus::QueryFailed,
        ] {
            let failure = route_entry_with_result(&target, route_result(status, 100), now);
            assert_eq!(
                project_account_usage_route(&failure, &target, now, 100),
                ProviderAccountUsageRouteProjection::UnknownAllow,
                "{status:?}",
            );
        }

        for adapter_kind in [
            ProviderAccountUsageAdapterKind::Sub2api,
            ProviderAccountUsageAdapterKind::Newapi,
            ProviderAccountUsageAdapterKind::Custom,
        ] {
            let mut adapter_target = route_target(1);
            adapter_target.adapter_kind = adapter_kind;
            let result = ProviderAccountUsageResult::fetched(
                adapter_kind,
                ProviderAccountUsageStatus::ZeroBalance,
                100,
            );
            let entry = route_entry_with_result(&adapter_target, result, now);
            assert_eq!(
                project_account_usage_route(&entry, &adapter_target, now, 100),
                ProviderAccountUsageRouteProjection::Blocked(
                    ProviderAccountUsageBlockReason::ZeroBalance
                ),
                "{adapter_kind:?}",
            );
        }
    }

    #[test]
    fn recovery_epoch_publishes_once_for_each_blocked_to_available_transition() {
        let state = ProviderAccountUsageRuntimeState::default();
        let target = route_target(1);
        let now = Instant::now();
        state.sync_route_target(&target, 1);
        state.renew_route_gateway_lease(target.provider_id, now + GATEWAY_LEASE);

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 100),
            now,
            100,
        );
        assert_eq!(state.global_recovery_epoch(), 0);

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::ZeroBalance, 101),
            now + Duration::from_secs(1),
            101,
        );
        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::QueryFailed, 102),
            now + Duration::from_secs(2),
            102,
        );
        assert_eq!(state.global_recovery_epoch(), 0);
        assert_eq!(
            state
                .route_read(&target, now + Duration::from_secs(2), 102)
                .recovery_epoch,
            0
        );

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 103),
            now + Duration::from_secs(3),
            103,
        );
        assert_eq!(state.global_recovery_epoch(), 1);
        assert_eq!(
            state
                .route_read(&target, now + Duration::from_secs(3), 103)
                .recovery_epoch,
            1
        );

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 104),
            now + Duration::from_secs(4),
            104,
        );
        assert_eq!(state.global_recovery_epoch(), 1);

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::QueryFailed, 105),
            now + Duration::from_secs(5),
            105,
        );
        assert_eq!(
            state
                .route_read(&target, now + Duration::from_secs(5), 105)
                .recovery_epoch,
            0
        );
        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 106),
            now + Duration::from_secs(6),
            106,
        );
        assert_eq!(state.global_recovery_epoch(), 1);
        assert_eq!(
            state
                .route_read(&target, now + Duration::from_secs(6), 106)
                .recovery_epoch,
            1
        );

        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::ZeroBalance, 107),
            now + Duration::from_secs(7),
            107,
        );
        assert_eq!(
            state
                .route_read(&target, now + Duration::from_secs(7), 107)
                .recovery_epoch,
            0
        );
        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 108),
            now + Duration::from_secs(8),
            108,
        );
        assert_eq!(state.global_recovery_epoch(), 2);
    }

    #[test]
    fn recovery_epoch_overflow_never_wraps_or_publishes_provider_signal() {
        let state = ProviderAccountUsageRuntimeState::default();
        let target = route_target(1);
        let now = Instant::now();
        state
            .shared
            .global_recovery_epoch
            .store(u64::MAX, Ordering::Release);
        state.sync_route_target(&target, 1);
        state.renew_route_gateway_lease(target.provider_id, now + GATEWAY_LEASE);
        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::ZeroBalance, 100),
            now,
            100,
        );
        state.publish_route_snapshot(
            target.provider_id,
            1,
            route_result(ProviderAccountUsageStatus::Available, 101),
            now + Duration::from_secs(1),
            101,
        );

        assert_eq!(state.global_recovery_epoch(), u64::MAX);
        let read = state.route_read(&target, now + Duration::from_secs(1), 101);
        assert_eq!(
            read.projection,
            ProviderAccountUsageRouteProjection::ConfirmedAvailable
        );
        assert_eq!(read.recovery_epoch, 0);
    }
}
