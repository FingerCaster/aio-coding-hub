//! Codex infinite-retry policy, bounded diagnostics, and final-wire validation.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

pub(crate) const MAX_RECENT_ATTEMPTS: usize = 100;
pub(crate) const MAX_PROVIDER_BUCKETS: usize = 100;
const MAX_PROVIDER_NAME_CHARS: usize = 96;

pub(crate) type SharedInfiniteRetryLedger = Arc<Mutex<InfiniteRetryLedger>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InfiniteRetryRequestConfig {
    pub(crate) retry_interval_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ProviderHealthMode {
    #[default]
    Normal,
    PassiveSystemRequest,
    InfiniteRetryTest,
}

impl ProviderHealthMode {
    pub(crate) fn bypasses_circuit(self) -> bool {
        matches!(self, Self::InfiniteRetryTest)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EligibilityFacts<'a> {
    pub(crate) cli_key: &'a str,
    pub(crate) method: &'a axum::http::Method,
    pub(crate) path: &'a str,
    pub(crate) observe_request: bool,
    pub(crate) is_compact_request: bool,
    pub(crate) is_model_discovery: bool,
    pub(crate) is_provider_test_or_warmup: bool,
    pub(crate) is_token_count: bool,
    pub(crate) is_system_request: bool,
}

pub(crate) fn request_config(
    enabled: bool,
    retry_interval_ms: u32,
    facts: EligibilityFacts<'_>,
) -> Option<InfiniteRetryRequestConfig> {
    let path = facts.path.trim_end_matches('/');
    (enabled
        && facts.cli_key == "codex"
        && facts.method == axum::http::Method::POST
        && matches!(path, "/responses" | "/v1/responses" | "/v1/codex/responses")
        && facts.observe_request
        && !facts.is_compact_request
        && !facts.is_model_discovery
        && !facts.is_provider_test_or_warmup
        && !facts.is_token_count
        && !facts.is_system_request)
        .then_some(InfiniteRetryRequestConfig { retry_interval_ms })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GatewayShutdown;

pub(crate) fn gateway_shutdown_requested(receiver: &watch::Receiver<bool>) -> bool {
    *receiver.borrow()
}

async fn wait_for_gateway_shutdown(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

pub(crate) async fn run_until_gateway_shutdown<T>(
    receiver: &mut watch::Receiver<bool>,
    future: impl Future<Output = T>,
) -> Result<T, GatewayShutdown> {
    tokio::select! {
        biased;
        _ = wait_for_gateway_shutdown(receiver) => Err(GatewayShutdown),
        output = future => Ok(output),
    }
}

pub(crate) async fn wait_between_rounds(
    retry_interval_ms: u32,
    receiver: &mut watch::Receiver<bool>,
) -> Result<(), GatewayShutdown> {
    run_until_gateway_shutdown(receiver, async move {
        if retry_interval_ms == 0 {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(Duration::from_millis(u64::from(retry_interval_ms))).await;
        }
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureCategory {
    Success,
    EmptyRound,
    PlanRefresh,
    Gate,
    Preparation,
    Transport,
    Timeout,
    Http,
    ResponseTooLarge,
    Transform,
    FinalWire,
    Unknown,
}

pub(crate) fn classify_attempt_outcome(
    outcome: &str,
    status: Option<u16>,
) -> (&'static str, FailureCategory) {
    let normalized = outcome.to_ascii_lowercase();
    if normalized == "success" {
        return ("success", FailureCategory::Success);
    }
    if normalized.contains("response_too_large") || normalized.contains("body_too_large") {
        return ("response_too_large", FailureCategory::ResponseTooLarge);
    }
    if normalized.contains("final_wire") || normalized.contains("invalid_final") {
        return ("invalid_final_wire", FailureCategory::FinalWire);
    }
    if normalized.contains("timeout") {
        return ("timeout", FailureCategory::Timeout);
    }
    if normalized.contains("transport")
        || normalized.contains("stream_error")
        || normalized.contains("body_error")
    {
        return ("transport_error", FailureCategory::Transport);
    }
    if normalized.contains("bridge")
        || normalized.contains("fixer")
        || normalized.contains("plugin")
        || normalized.contains("transform")
    {
        return ("transform_error", FailureCategory::Transform);
    }
    if status.is_some_and(|status| status >= 400) {
        return ("http_error", FailureCategory::Http);
    }
    if normalized.contains("skip") || normalized.contains("gate") {
        return ("gate_skip", FailureCategory::Gate);
    }
    if normalized.contains("prepare") || normalized.contains("credential") {
        return ("preparation_error", FailureCategory::Preparation);
    }
    ("unknown_failure", FailureCategory::Unknown)
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UsageSample {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
    pub(crate) reasoning_tokens: Option<u64>,
    pub(crate) cache_read_input_tokens: Option<u64>,
    pub(crate) cache_creation_input_tokens: Option<u64>,
    pub(crate) cache_creation_5m_input_tokens: Option<u64>,
    pub(crate) cache_creation_1h_input_tokens: Option<u64>,
}

impl UsageSample {
    pub(crate) fn from_metrics(metrics: &crate::usage::UsageMetrics) -> Self {
        fn nonnegative(value: Option<i64>) -> Option<u64> {
            value.and_then(|value| u64::try_from(value).ok())
        }

        Self {
            input_tokens: nonnegative(metrics.input_tokens),
            output_tokens: nonnegative(metrics.output_tokens),
            total_tokens: nonnegative(metrics.total_tokens),
            reasoning_tokens: nonnegative(metrics.reasoning_tokens),
            cache_read_input_tokens: nonnegative(metrics.cache_read_input_tokens),
            cache_creation_input_tokens: nonnegative(metrics.cache_creation_input_tokens),
            cache_creation_5m_input_tokens: nonnegative(metrics.cache_creation_5m_input_tokens),
            cache_creation_1h_input_tokens: nonnegative(metrics.cache_creation_1h_input_tokens),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InfiniteRetryAttemptSummary {
    pub(crate) round: String,
    pub(crate) attempt: String,
    pub(crate) provider_id: Option<i64>,
    pub(crate) provider_name: String,
    pub(crate) status: Option<u16>,
    pub(crate) outcome_code: &'static str,
    pub(crate) failure_category: FailureCategory,
    pub(crate) duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageAccumulator {
    pub(crate) known_attempts: String,
    pub(crate) unknown_attempts: String,
    pub(crate) input_tokens: Option<String>,
    pub(crate) input_tokens_missing_attempts: String,
    pub(crate) output_tokens: Option<String>,
    pub(crate) output_tokens_missing_attempts: String,
    pub(crate) total_tokens: Option<String>,
    pub(crate) total_tokens_missing_attempts: String,
    pub(crate) reasoning_tokens: Option<String>,
    pub(crate) reasoning_tokens_missing_attempts: String,
    pub(crate) cache_read_input_tokens: Option<String>,
    pub(crate) cache_read_input_tokens_missing_attempts: String,
    pub(crate) cache_creation_input_tokens: Option<String>,
    pub(crate) cache_creation_input_tokens_missing_attempts: String,
    pub(crate) cache_creation_5m_input_tokens: Option<String>,
    pub(crate) cache_creation_5m_input_tokens_missing_attempts: String,
    pub(crate) cache_creation_1h_input_tokens: Option<String>,
    pub(crate) cache_creation_1h_input_tokens_missing_attempts: String,
    pub(crate) unpriced_attempts: String,
    pub(crate) cost_usd_femto: Option<String>,
    pub(crate) complete: bool,
    pub(crate) overflowed: bool,
}

#[derive(Debug, Clone, Default)]
struct UsageFieldCounter {
    known_sum: Option<u64>,
    missing_attempts: u64,
}

impl UsageFieldCounter {
    fn record(&mut self, value: Option<u64>, overflowed: &mut bool) {
        match value {
            Some(value) => {
                let current = self.known_sum.unwrap_or(0);
                let next = current.saturating_add(value);
                *overflowed |= next == u64::MAX && current != u64::MAX;
                self.known_sum = Some(next);
            }
            None => {
                let current = self.missing_attempts;
                self.missing_attempts = current.saturating_add(1);
                *overflowed |= self.missing_attempts == u64::MAX && current != u64::MAX;
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct UsageCounters {
    known_attempts: u64,
    unknown_attempts: u64,
    input_tokens: UsageFieldCounter,
    output_tokens: UsageFieldCounter,
    total_tokens: UsageFieldCounter,
    reasoning_tokens: UsageFieldCounter,
    cache_read_input_tokens: UsageFieldCounter,
    cache_creation_input_tokens: UsageFieldCounter,
    cache_creation_5m_input_tokens: UsageFieldCounter,
    cache_creation_1h_input_tokens: UsageFieldCounter,
    unpriced_attempts: u64,
    cost_usd_femto: Option<u128>,
    overflowed: bool,
}

impl UsageCounters {
    fn record(&mut self, usage: Option<UsageSample>, cost_usd_femto: Option<u128>) {
        let usage_reported = usage.is_some();
        let usage = match usage {
            Some(usage) => {
                let current = self.known_attempts;
                self.known_attempts = current.saturating_add(1);
                self.overflowed |= self.known_attempts == u64::MAX && current != u64::MAX;
                usage
            }
            None => {
                let current = self.unknown_attempts;
                self.unknown_attempts = current.saturating_add(1);
                self.overflowed |= self.unknown_attempts == u64::MAX && current != u64::MAX;
                UsageSample::default()
            }
        };
        self.input_tokens
            .record(usage.input_tokens, &mut self.overflowed);
        self.output_tokens
            .record(usage.output_tokens, &mut self.overflowed);
        self.total_tokens
            .record(usage.total_tokens, &mut self.overflowed);
        self.reasoning_tokens
            .record(usage.reasoning_tokens, &mut self.overflowed);
        self.cache_read_input_tokens
            .record(usage.cache_read_input_tokens, &mut self.overflowed);
        self.cache_creation_input_tokens
            .record(usage.cache_creation_input_tokens, &mut self.overflowed);
        self.cache_creation_5m_input_tokens
            .record(usage.cache_creation_5m_input_tokens, &mut self.overflowed);
        self.cache_creation_1h_input_tokens
            .record(usage.cache_creation_1h_input_tokens, &mut self.overflowed);

        if let Some(value) = cost_usd_femto {
            let current = self.cost_usd_femto.unwrap_or(0);
            let next = current.saturating_add(value);
            self.overflowed |= next == u128::MAX && current != u128::MAX;
            self.cost_usd_femto = Some(next);
        } else if usage_reported {
            self.unpriced_attempts = self.unpriced_attempts.saturating_add(1);
            self.overflowed |= self.unpriced_attempts == u64::MAX;
        }
    }

    fn snapshot(&self) -> UsageAccumulator {
        let complete = self.unknown_attempts == 0
            && self.input_tokens.missing_attempts == 0
            && self.output_tokens.missing_attempts == 0
            && self.total_tokens.missing_attempts == 0
            && self.reasoning_tokens.missing_attempts == 0
            && self.cache_read_input_tokens.missing_attempts == 0
            && self.cache_creation_input_tokens.missing_attempts == 0
            && self.cache_creation_5m_input_tokens.missing_attempts == 0
            && self.cache_creation_1h_input_tokens.missing_attempts == 0;
        UsageAccumulator {
            known_attempts: self.known_attempts.to_string(),
            unknown_attempts: self.unknown_attempts.to_string(),
            input_tokens: self.input_tokens.known_sum.map(|v| v.to_string()),
            input_tokens_missing_attempts: self.input_tokens.missing_attempts.to_string(),
            output_tokens: self.output_tokens.known_sum.map(|v| v.to_string()),
            output_tokens_missing_attempts: self.output_tokens.missing_attempts.to_string(),
            total_tokens: self.total_tokens.known_sum.map(|v| v.to_string()),
            total_tokens_missing_attempts: self.total_tokens.missing_attempts.to_string(),
            reasoning_tokens: self.reasoning_tokens.known_sum.map(|v| v.to_string()),
            reasoning_tokens_missing_attempts: self.reasoning_tokens.missing_attempts.to_string(),
            cache_read_input_tokens: self
                .cache_read_input_tokens
                .known_sum
                .map(|v| v.to_string()),
            cache_read_input_tokens_missing_attempts: self
                .cache_read_input_tokens
                .missing_attempts
                .to_string(),
            cache_creation_input_tokens: self
                .cache_creation_input_tokens
                .known_sum
                .map(|v| v.to_string()),
            cache_creation_input_tokens_missing_attempts: self
                .cache_creation_input_tokens
                .missing_attempts
                .to_string(),
            cache_creation_5m_input_tokens: self
                .cache_creation_5m_input_tokens
                .known_sum
                .map(|v| v.to_string()),
            cache_creation_5m_input_tokens_missing_attempts: self
                .cache_creation_5m_input_tokens
                .missing_attempts
                .to_string(),
            cache_creation_1h_input_tokens: self
                .cache_creation_1h_input_tokens
                .known_sum
                .map(|v| v.to_string()),
            cache_creation_1h_input_tokens_missing_attempts: self
                .cache_creation_1h_input_tokens
                .missing_attempts
                .to_string(),
            unpriced_attempts: self.unpriced_attempts.to_string(),
            cost_usd_femto: self.cost_usd_femto.map(|v| v.to_string()),
            complete,
            overflowed: self.overflowed,
        }
    }

    fn usage_metrics(&self) -> Option<crate::usage::UsageMetrics> {
        fn as_i64(value: Option<u64>) -> Option<i64> {
            value.map(|value| value.min(i64::MAX as u64) as i64)
        }

        let metrics = crate::usage::UsageMetrics {
            input_tokens: as_i64(self.input_tokens.known_sum),
            output_tokens: as_i64(self.output_tokens.known_sum),
            total_tokens: as_i64(self.total_tokens.known_sum),
            reasoning_tokens: as_i64(self.reasoning_tokens.known_sum),
            cache_read_input_tokens: as_i64(self.cache_read_input_tokens.known_sum),
            cache_creation_input_tokens: as_i64(self.cache_creation_input_tokens.known_sum),
            cache_creation_5m_input_tokens: as_i64(self.cache_creation_5m_input_tokens.known_sum),
            cache_creation_1h_input_tokens: as_i64(self.cache_creation_1h_input_tokens.known_sum),
        };
        (self.known_attempts > 0).then_some(metrics)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderBucketSnapshot {
    pub(crate) provider_id: Option<i64>,
    pub(crate) provider_name: String,
    pub(crate) attempt_count: String,
    pub(crate) usage: UsageAccumulator,
}

#[derive(Debug, Clone)]
struct ProviderBucket {
    provider_id: Option<i64>,
    provider_name: String,
    attempt_count: u64,
    usage: UsageCounters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AttemptKey {
    provider_id: i64,
    retry_index: u32,
    attempt_started_ms: u128,
}

impl AttemptKey {
    pub(crate) fn new(provider_id: i64, retry_index: u32, attempt_started_ms: u128) -> Self {
        Self {
            provider_id,
            retry_index,
            attempt_started_ms,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingAttemptUsage {
    usage: Option<UsageSample>,
    cost_usd_femto: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InfiniteRetryLedgerSnapshot {
    pub(crate) version: u8,
    pub(crate) rounds: String,
    pub(crate) failed_rounds: String,
    pub(crate) empty_rounds: String,
    pub(crate) attempts: String,
    pub(crate) overflowed: bool,
    pub(crate) phase: &'static str,
    pub(crate) stop_reason: Option<&'static str>,
    pub(crate) failure_categories: BTreeMap<FailureCategory, String>,
    pub(crate) usage: UsageAccumulator,
    pub(crate) recent_attempts: Vec<InfiniteRetryAttemptSummary>,
    pub(crate) providers: Vec<ProviderBucketSnapshot>,
    pub(crate) provider_attribution_overflowed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct InfiniteRetryLedger {
    rounds: u64,
    failed_rounds: u64,
    empty_rounds: u64,
    attempts: u64,
    overflowed: bool,
    phase: &'static str,
    stop_reason: Option<&'static str>,
    failure_categories: BTreeMap<FailureCategory, u64>,
    recent_attempts: VecDeque<InfiniteRetryAttemptSummary>,
    providers: BTreeMap<i64, ProviderBucket>,
    other_providers: Option<ProviderBucket>,
    usage: UsageCounters,
    recorded_attempts_in_round: BTreeSet<AttemptKey>,
    pending_attempt_usage: BTreeMap<AttemptKey, PendingAttemptUsage>,
}

impl Default for InfiniteRetryLedger {
    fn default() -> Self {
        Self {
            rounds: 0,
            failed_rounds: 0,
            empty_rounds: 0,
            attempts: 0,
            overflowed: false,
            phase: "starting",
            stop_reason: None,
            failure_categories: BTreeMap::new(),
            recent_attempts: VecDeque::with_capacity(MAX_RECENT_ATTEMPTS),
            providers: BTreeMap::new(),
            other_providers: None,
            usage: UsageCounters::default(),
            recorded_attempts_in_round: BTreeSet::new(),
            pending_attempt_usage: BTreeMap::new(),
        }
    }
}

impl InfiniteRetryLedger {
    fn increment(value: &mut u64, overflowed: &mut bool) {
        let next = value.saturating_add(1);
        *overflowed |= next == u64::MAX;
        *value = next;
    }

    pub(crate) fn start_round(&mut self, empty: bool) -> u64 {
        self.recorded_attempts_in_round.clear();
        self.pending_attempt_usage.clear();
        Self::increment(&mut self.rounds, &mut self.overflowed);
        if empty {
            Self::increment(&mut self.empty_rounds, &mut self.overflowed);
            Self::increment(
                self.failure_categories
                    .entry(FailureCategory::EmptyRound)
                    .or_default(),
                &mut self.overflowed,
            );
        }
        self.phase = if empty {
            "empty_round"
        } else {
            "running_round"
        };
        self.rounds
    }

    pub(crate) fn finish_failed_round(&mut self) {
        Self::increment(&mut self.failed_rounds, &mut self.overflowed);
        self.phase = "waiting";
    }

    pub(crate) fn record_plan_refresh_failure(&mut self) {
        Self::increment(
            self.failure_categories
                .entry(FailureCategory::PlanRefresh)
                .or_default(),
            &mut self.overflowed,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_attempt(
        &mut self,
        provider_id: i64,
        provider_name: &str,
        status: Option<u16>,
        outcome_code: &'static str,
        failure_category: FailureCategory,
        duration_ms: u64,
        usage: Option<UsageSample>,
        cost_usd_femto: Option<u128>,
    ) {
        Self::increment(&mut self.attempts, &mut self.overflowed);
        if failure_category != FailureCategory::Success {
            Self::increment(
                self.failure_categories.entry(failure_category).or_default(),
                &mut self.overflowed,
            );
        }
        let provider_name: String = provider_name
            .chars()
            .take(MAX_PROVIDER_NAME_CHARS)
            .collect();
        if self.recent_attempts.len() == MAX_RECENT_ATTEMPTS {
            self.recent_attempts.pop_front();
        }
        self.recent_attempts.push_back(InfiniteRetryAttemptSummary {
            round: self.rounds.to_string(),
            attempt: self.attempts.to_string(),
            provider_id: Some(provider_id),
            provider_name: provider_name.clone(),
            status,
            outcome_code,
            failure_category,
            duration_ms,
        });
        let bucket = if self.providers.contains_key(&provider_id)
            || self.providers.len() < MAX_PROVIDER_BUCKETS
        {
            self.providers
                .entry(provider_id)
                .or_insert_with(|| ProviderBucket {
                    provider_id: Some(provider_id),
                    provider_name,
                    attempt_count: 0,
                    usage: UsageCounters::default(),
                })
        } else {
            self.other_providers.get_or_insert_with(|| ProviderBucket {
                provider_id: None,
                provider_name: "other_providers".to_string(),
                attempt_count: 0,
                usage: UsageCounters::default(),
            })
        };
        Self::increment(&mut bucket.attempt_count, &mut self.overflowed);
        bucket.usage.record(usage, cost_usd_femto);
        self.usage.record(usage, cost_usd_femto);
    }

    pub(crate) fn observe_attempt_usage(
        &mut self,
        key: AttemptKey,
        usage: Option<UsageSample>,
        cost_usd_femto: Option<u128>,
    ) {
        if self.recorded_attempts_in_round.contains(&key) {
            return;
        }
        if self.pending_attempt_usage.len() >= 100 && !self.pending_attempt_usage.contains_key(&key)
        {
            self.overflowed = true;
            return;
        }
        self.pending_attempt_usage.insert(
            key,
            PendingAttemptUsage {
                usage,
                cost_usd_femto,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_attempt_once(
        &mut self,
        key: AttemptKey,
        provider_name: &str,
        status: Option<u16>,
        outcome_code: &'static str,
        failure_category: FailureCategory,
        duration_ms: u64,
    ) -> bool {
        if !self.recorded_attempts_in_round.insert(key) {
            return false;
        }
        let pending = self.pending_attempt_usage.remove(&key);
        self.record_attempt(
            key.provider_id,
            provider_name,
            status,
            outcome_code,
            failure_category,
            duration_ms,
            pending.and_then(|pending| pending.usage),
            pending.and_then(|pending| pending.cost_usd_femto),
        );
        true
    }

    pub(crate) fn stop(&mut self, reason: &'static str) {
        self.stop_reason = Some(reason);
        self.phase = "stopped";
    }

    pub(crate) fn round_count(&self) -> u64 {
        self.rounds
    }

    pub(crate) fn attempt_count(&self) -> u64 {
        self.attempts
    }

    pub(crate) fn usage_metrics(&self) -> Option<crate::usage::UsageMetrics> {
        self.usage.usage_metrics()
    }

    pub(crate) fn cost_usd_femto(&self) -> Option<i64> {
        self.usage
            .cost_usd_femto
            .map(|value| value.min(i64::MAX as u128) as i64)
    }

    pub(crate) fn snapshot(&self) -> InfiniteRetryLedgerSnapshot {
        let mut providers = self
            .providers
            .values()
            .chain(self.other_providers.iter())
            .map(|bucket| ProviderBucketSnapshot {
                provider_id: bucket.provider_id,
                provider_name: bucket.provider_name.clone(),
                attempt_count: bucket.attempt_count.to_string(),
                usage: bucket.usage.snapshot(),
            })
            .collect::<Vec<_>>();
        providers.sort_by_key(|bucket| bucket.provider_id.unwrap_or(i64::MAX));
        InfiniteRetryLedgerSnapshot {
            version: 1,
            rounds: self.rounds.to_string(),
            failed_rounds: self.failed_rounds.to_string(),
            empty_rounds: self.empty_rounds.to_string(),
            attempts: self.attempts.to_string(),
            overflowed: self.overflowed,
            phase: self.phase,
            stop_reason: self.stop_reason,
            failure_categories: self
                .failure_categories
                .iter()
                .map(|(key, value)| (*key, value.to_string()))
                .collect(),
            usage: self.usage.snapshot(),
            recent_attempts: self.recent_attempts.iter().cloned().collect(),
            provider_attribution_overflowed: self.other_providers.is_some(),
            providers,
        }
    }
}

pub(crate) fn validate_complete_codex_json(bytes: &[u8]) -> Result<(), &'static str> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "invalid_json")?;
    let object = value.as_object().ok_or("response_not_object")?;
    if object.get("error").is_some_and(|value| !value.is_null())
        || object.get("type").and_then(serde_json::Value::as_str) == Some("error")
    {
        return Err("embedded_error");
    }
    let response = object
        .get("response")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    match response.get("status").and_then(serde_json::Value::as_str) {
        Some("completed") => Ok(()),
        Some("failed" | "incomplete" | "error") => Err("terminal_failure"),
        Some(_) => Err("unknown_status"),
        None => Err("missing_status"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(method: &'a axum::http::Method, path: &'a str) -> EligibilityFacts<'a> {
        EligibilityFacts {
            cli_key: "codex",
            method,
            path,
            observe_request: true,
            is_compact_request: false,
            is_model_discovery: false,
            is_provider_test_or_warmup: false,
            is_token_count: false,
            is_system_request: false,
        }
    }

    #[test]
    fn eligibility_is_fail_closed_and_excludes_system_turns() {
        let method = axum::http::Method::POST;
        assert!(request_config(true, 1_000, facts(&method, "/v1/responses")).is_some());
        let mut system = facts(&method, "/v1/responses");
        system.is_system_request = true;
        assert_eq!(request_config(true, 1_000, system), None);
        assert_eq!(
            request_config(false, 1_000, facts(&method, "/v1/responses")),
            None
        );
        assert_eq!(
            request_config(true, 1_000, facts(&method, "/v1/models")),
            None
        );
    }

    #[tokio::test]
    async fn zero_interval_cooperatively_yields() {
        let (_shutdown, mut receiver) = watch::channel(false);
        wait_between_rounds(0, &mut receiver)
            .await
            .expect("open gateway should continue");
    }

    #[tokio::test]
    async fn gateway_shutdown_interrupts_round_wait() {
        let (shutdown, mut receiver) = watch::channel(false);
        let wait = tokio::spawn(async move { wait_between_rounds(60_000, &mut receiver).await });
        tokio::task::yield_now().await;

        shutdown.send_replace(true);

        let result = tokio::time::timeout(Duration::from_secs(1), wait)
            .await
            .expect("shutdown should interrupt the long retry interval")
            .expect("wait task should not panic");
        assert_eq!(result, Err(GatewayShutdown));
    }

    #[tokio::test]
    async fn gateway_shutdown_drops_in_flight_round_work() {
        struct DropSignal(Arc<std::sync::atomic::AtomicBool>);

        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }

        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (shutdown, mut receiver) = watch::channel(false);
        let dropped_in_round = Arc::clone(&dropped);
        let task = tokio::spawn(async move {
            run_until_gateway_shutdown(&mut receiver, async move {
                let _drop_signal = DropSignal(dropped_in_round);
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .await
        });
        started_rx.await.expect("round work should start");

        shutdown.send_replace(true);

        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("shutdown should interrupt in-flight round work")
            .expect("round task should not panic");
        assert_eq!(result, Err(GatewayShutdown));
        assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn plan_refresh_failures_are_observable_without_creating_attempts() {
        let mut ledger = InfiniteRetryLedger::default();

        ledger.record_plan_refresh_failure();

        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.attempts, "0");
        assert_eq!(
            snapshot
                .failure_categories
                .get(&FailureCategory::PlanRefresh)
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn ledger_bounds_attempts_and_provider_buckets() {
        let mut ledger = InfiniteRetryLedger::default();
        ledger.start_round(false);
        for id in 0..150_i64 {
            ledger.record_attempt(
                id,
                &format!("provider-{id}"),
                Some(503),
                "upstream_failure",
                FailureCategory::Http,
                1,
                None,
                None,
            );
        }
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.recent_attempts.len(), MAX_RECENT_ATTEMPTS);
        assert_eq!(snapshot.providers.len(), MAX_PROVIDER_BUCKETS + 1);
        assert!(snapshot.provider_attribution_overflowed);
        assert_eq!(snapshot.attempts, "150");
        assert_eq!(snapshot.usage.unknown_attempts, "150");
    }

    #[test]
    fn json_success_requires_completed_and_rejects_embedded_errors() {
        assert!(validate_complete_codex_json(br#"{"id":"r","status":"completed"}"#).is_ok());
        assert_eq!(
            validate_complete_codex_json(br#"{"status":"incomplete"}"#),
            Err("terminal_failure")
        );
        assert_eq!(
            validate_complete_codex_json(br#"{"status":"completed","error":{"code":"x"}}"#),
            Err("embedded_error")
        );
    }
}
