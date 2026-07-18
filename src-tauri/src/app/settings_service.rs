//! Usage: Settings-related Tauri commands.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::gateway::events::GATEWAY_STATUS_EVENT_NAME;
use crate::gateway_control::{
    app_start_gateway_with_config, try_app_gateway_update_circuit_config,
};
use crate::gateway_runtime_access::app_gateway_status;
use crate::{blocking, cli_proxy, resident, settings};
use tauri::Manager;

fn write_settings_view<R, F>(
    app: &tauri::AppHandle<R>,
    mutate: F,
) -> crate::shared::error::AppResult<SettingsView>
where
    R: tauri::Runtime,
    F: FnOnce(&mut settings::AppSettings) -> crate::shared::error::AppResult<()>,
{
    settings::update(app, |settings| {
        settings.schema_version = settings::SCHEMA_VERSION;
        mutate(settings)
    })
    .map(|(value, ())| SettingsView::from(&value))
}

/// Encapsulates ordinary `settings_set` owned fields only.
/// Rectifier / circuit-notice / Codex-completion / Image Gen / Grok fields are
/// intentionally absent; dedicated writers own those groups.
#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsUpdate {
    pub preferred_port: u16,
    pub show_home_heatmap: Option<bool>,
    pub show_home_usage: Option<bool>,
    pub home_usage_period: Option<settings::HomeUsagePeriod>,
    pub gateway_listen_mode: Option<settings::GatewayListenMode>,
    pub gateway_custom_listen_address: Option<String>,
    pub auto_start: bool,
    pub start_minimized: Option<bool>,
    pub tray_enabled: Option<bool>,
    pub enable_cli_proxy_startup_recovery: Option<bool>,
    pub log_retention_days: u32,
    // Option keeps older frontend payloads valid (0 = keep forever).
    pub request_log_retention_days: Option<u32>,
    pub provider_cooldown_seconds: Option<u32>,
    pub provider_base_url_ping_cache_ttl_seconds: Option<u32>,
    pub upstream_first_byte_timeout_seconds: Option<u32>,
    pub upstream_stream_idle_timeout_seconds: Option<u32>,
    pub upstream_request_timeout_non_streaming_seconds: Option<u32>,
    pub enable_cache_anomaly_monitor: Option<bool>,
    pub enable_debug_log: Option<bool>,
    pub enable_task_complete_notify: Option<bool>,
    pub enable_notification_sound: Option<bool>,
    pub failover_max_attempts_per_provider: u32,
    pub failover_max_providers_to_try: u32,
    pub upstream_retry_policy: Option<settings::UpstreamRetryPolicy>,
    pub circuit_breaker_failure_threshold: Option<u32>,
    pub circuit_breaker_open_duration_minutes: Option<u32>,
    pub update_releases_url: Option<String>,
    pub wsl_auto_config: Option<bool>,
    pub wsl_target_cli: Option<settings::WslTargetCli>,
    pub cli_priority_order: Option<Vec<String>>,
    pub wsl_host_address_mode: Option<settings::WslHostAddressMode>,
    pub wsl_custom_host_address: Option<String>,
    pub codex_home_mode: Option<settings::CodexHomeMode>,
    pub codex_home_override: Option<String>,
    pub codex_oauth_compatible_proxy_mode: Option<bool>,
    pub codex_provider_test_model: Option<String>,
    #[serde(rename = "cx2CcFallbackModelOpus")]
    #[specta(rename = "cx2CcFallbackModelOpus")]
    pub cx2cc_fallback_model_opus: Option<String>,
    #[serde(rename = "cx2CcFallbackModelSonnet")]
    #[specta(rename = "cx2CcFallbackModelSonnet")]
    pub cx2cc_fallback_model_sonnet: Option<String>,
    #[serde(rename = "cx2CcFallbackModelHaiku")]
    #[specta(rename = "cx2CcFallbackModelHaiku")]
    pub cx2cc_fallback_model_haiku: Option<String>,
    #[serde(rename = "cx2CcFallbackModelMain")]
    #[specta(rename = "cx2CcFallbackModelMain")]
    pub cx2cc_fallback_model_main: Option<String>,
    #[serde(rename = "cx2CcModelReasoningEffort")]
    #[specta(rename = "cx2CcModelReasoningEffort")]
    pub cx2cc_model_reasoning_effort: Option<String>,
    #[serde(rename = "cx2CcServiceTier")]
    #[specta(rename = "cx2CcServiceTier")]
    pub cx2cc_service_tier: Option<String>,
    #[serde(rename = "cx2CcDisableResponseStorage")]
    #[specta(rename = "cx2CcDisableResponseStorage")]
    pub cx2cc_disable_response_storage: Option<bool>,
    #[serde(rename = "cx2CcEnableReasoningToThinking")]
    #[specta(rename = "cx2CcEnableReasoningToThinking")]
    pub cx2cc_enable_reasoning_to_thinking: Option<bool>,
    #[serde(rename = "cx2CcDropStopSequences")]
    #[specta(rename = "cx2CcDropStopSequences")]
    pub cx2cc_drop_stop_sequences: Option<bool>,
    #[serde(rename = "cx2CcCleanSchema")]
    #[specta(rename = "cx2CcCleanSchema")]
    pub cx2cc_clean_schema: Option<bool>,
    #[serde(rename = "cx2CcFilterBatchTool")]
    #[specta(rename = "cx2CcFilterBatchTool")]
    pub cx2cc_filter_batch_tool: Option<bool>,
    pub upstream_proxy_enabled: Option<bool>,
    pub upstream_proxy_url: Option<String>,
    pub upstream_proxy_username: Option<String>,
    pub upstream_proxy_password: Option<SensitiveStringUpdate>,
}

/// Explicit ordinary-owner field token used for equality and rollback.
#[derive(Debug, Clone, PartialEq)]
struct SettingsServiceOwnedToken {
    preferred_port: u16,
    show_home_heatmap: bool,
    show_home_usage: bool,
    home_usage_period: settings::HomeUsagePeriod,
    gateway_listen_mode: settings::GatewayListenMode,
    gateway_custom_listen_address: String,
    auto_start: bool,
    start_minimized: bool,
    tray_enabled: bool,
    enable_cli_proxy_startup_recovery: bool,
    log_retention_days: u32,
    request_log_retention_days: u32,
    provider_cooldown_seconds: u32,
    provider_base_url_ping_cache_ttl_seconds: u32,
    upstream_first_byte_timeout_seconds: u32,
    upstream_stream_idle_timeout_seconds: u32,
    upstream_request_timeout_non_streaming_seconds: u32,
    enable_cache_anomaly_monitor: bool,
    enable_debug_log: bool,
    enable_task_complete_notify: bool,
    enable_notification_sound: bool,
    failover_max_attempts_per_provider: u32,
    failover_max_providers_to_try: u32,
    upstream_retry_policy: settings::UpstreamRetryPolicy,
    circuit_breaker_failure_threshold: u32,
    circuit_breaker_open_duration_minutes: u32,
    update_releases_url: String,
    wsl_auto_config: bool,
    wsl_target_cli: settings::WslTargetCli,
    cli_priority_order: Vec<String>,
    wsl_host_address_mode: settings::WslHostAddressMode,
    wsl_custom_host_address: String,
    codex_home_mode: settings::CodexHomeMode,
    codex_home_override: String,
    codex_oauth_compatible_proxy_mode: bool,
    codex_provider_test_model: String,
    cx2cc_fallback_model_opus: String,
    cx2cc_fallback_model_sonnet: String,
    cx2cc_fallback_model_haiku: String,
    cx2cc_fallback_model_main: String,
    cx2cc_model_reasoning_effort: String,
    cx2cc_service_tier: String,
    cx2cc_disable_response_storage: bool,
    cx2cc_enable_reasoning_to_thinking: bool,
    cx2cc_drop_stop_sequences: bool,
    cx2cc_clean_schema: bool,
    cx2cc_filter_batch_tool: bool,
    upstream_proxy_enabled: bool,
    upstream_proxy_url: String,
    upstream_proxy_username: String,
    upstream_proxy_password: String,
}

impl SettingsServiceOwnedToken {
    fn from_settings(settings: &settings::AppSettings) -> Self {
        Self {
            preferred_port: settings.preferred_port,
            show_home_heatmap: settings.show_home_heatmap,
            show_home_usage: settings.show_home_usage,
            home_usage_period: settings.home_usage_period,
            gateway_listen_mode: settings.gateway_listen_mode,
            gateway_custom_listen_address: settings.gateway_custom_listen_address.clone(),
            auto_start: settings.auto_start,
            start_minimized: settings.start_minimized,
            tray_enabled: settings.tray_enabled,
            enable_cli_proxy_startup_recovery: settings.enable_cli_proxy_startup_recovery,
            log_retention_days: settings.log_retention_days,
            request_log_retention_days: settings.request_log_retention_days,
            provider_cooldown_seconds: settings.provider_cooldown_seconds,
            provider_base_url_ping_cache_ttl_seconds: settings
                .provider_base_url_ping_cache_ttl_seconds,
            upstream_first_byte_timeout_seconds: settings.upstream_first_byte_timeout_seconds,
            upstream_stream_idle_timeout_seconds: settings.upstream_stream_idle_timeout_seconds,
            upstream_request_timeout_non_streaming_seconds: settings
                .upstream_request_timeout_non_streaming_seconds,
            enable_cache_anomaly_monitor: settings.enable_cache_anomaly_monitor,
            enable_debug_log: settings.enable_debug_log,
            enable_task_complete_notify: settings.enable_task_complete_notify,
            enable_notification_sound: settings.enable_notification_sound,
            failover_max_attempts_per_provider: settings.failover_max_attempts_per_provider,
            failover_max_providers_to_try: settings.failover_max_providers_to_try,
            upstream_retry_policy: settings.upstream_retry_policy.clone(),
            circuit_breaker_failure_threshold: settings.circuit_breaker_failure_threshold,
            circuit_breaker_open_duration_minutes: settings.circuit_breaker_open_duration_minutes,
            update_releases_url: settings.update_releases_url.clone(),
            wsl_auto_config: settings.wsl_auto_config,
            wsl_target_cli: settings.wsl_target_cli,
            cli_priority_order: settings.cli_priority_order.clone(),
            wsl_host_address_mode: settings.wsl_host_address_mode,
            wsl_custom_host_address: settings.wsl_custom_host_address.clone(),
            codex_home_mode: settings.codex_home_mode,
            codex_home_override: settings.codex_home_override.clone(),
            codex_oauth_compatible_proxy_mode: settings.codex_oauth_compatible_proxy_mode,
            codex_provider_test_model: settings.codex_provider_test_model.clone(),
            cx2cc_fallback_model_opus: settings.cx2cc_fallback_model_opus.clone(),
            cx2cc_fallback_model_sonnet: settings.cx2cc_fallback_model_sonnet.clone(),
            cx2cc_fallback_model_haiku: settings.cx2cc_fallback_model_haiku.clone(),
            cx2cc_fallback_model_main: settings.cx2cc_fallback_model_main.clone(),
            cx2cc_model_reasoning_effort: settings.cx2cc_model_reasoning_effort.clone(),
            cx2cc_service_tier: settings.cx2cc_service_tier.clone(),
            cx2cc_disable_response_storage: settings.cx2cc_disable_response_storage,
            cx2cc_enable_reasoning_to_thinking: settings.cx2cc_enable_reasoning_to_thinking,
            cx2cc_drop_stop_sequences: settings.cx2cc_drop_stop_sequences,
            cx2cc_clean_schema: settings.cx2cc_clean_schema,
            cx2cc_filter_batch_tool: settings.cx2cc_filter_batch_tool,
            upstream_proxy_enabled: settings.upstream_proxy_enabled,
            upstream_proxy_url: settings.upstream_proxy_url.clone(),
            upstream_proxy_username: settings.upstream_proxy_username.clone(),
            upstream_proxy_password: settings.upstream_proxy_password.clone(),
        }
    }

    /// Absorb a conditional preferred_port repair into this ordinary-owned token.
    /// Only the port field changes; concurrent ordinary winners on other fields
    /// are intentionally not rebuilt from the full written snapshot.
    fn absorb_preferred_port_repair(&mut self, repaired_port: u16) {
        self.preferred_port = repaired_port;
    }

    fn apply_to(&self, settings: &mut settings::AppSettings) {
        settings.preferred_port = self.preferred_port;
        settings.show_home_heatmap = self.show_home_heatmap;
        settings.show_home_usage = self.show_home_usage;
        settings.home_usage_period = self.home_usage_period;
        settings.gateway_listen_mode = self.gateway_listen_mode;
        settings.gateway_custom_listen_address = self.gateway_custom_listen_address.clone();
        settings.auto_start = self.auto_start;
        settings.start_minimized = self.start_minimized;
        settings.tray_enabled = self.tray_enabled;
        settings.enable_cli_proxy_startup_recovery = self.enable_cli_proxy_startup_recovery;
        settings.log_retention_days = self.log_retention_days;
        settings.request_log_retention_days = self.request_log_retention_days;
        settings.provider_cooldown_seconds = self.provider_cooldown_seconds;
        settings.provider_base_url_ping_cache_ttl_seconds =
            self.provider_base_url_ping_cache_ttl_seconds;
        settings.upstream_first_byte_timeout_seconds = self.upstream_first_byte_timeout_seconds;
        settings.upstream_stream_idle_timeout_seconds = self.upstream_stream_idle_timeout_seconds;
        settings.upstream_request_timeout_non_streaming_seconds =
            self.upstream_request_timeout_non_streaming_seconds;
        settings.enable_cache_anomaly_monitor = self.enable_cache_anomaly_monitor;
        settings.enable_debug_log = self.enable_debug_log;
        settings.enable_task_complete_notify = self.enable_task_complete_notify;
        settings.enable_notification_sound = self.enable_notification_sound;
        settings.failover_max_attempts_per_provider = self.failover_max_attempts_per_provider;
        settings.failover_max_providers_to_try = self.failover_max_providers_to_try;
        settings.upstream_retry_policy = self.upstream_retry_policy.clone();
        settings.circuit_breaker_failure_threshold = self.circuit_breaker_failure_threshold;
        settings.circuit_breaker_open_duration_minutes = self.circuit_breaker_open_duration_minutes;
        settings.update_releases_url = self.update_releases_url.clone();
        settings.wsl_auto_config = self.wsl_auto_config;
        settings.wsl_target_cli = self.wsl_target_cli;
        settings.cli_priority_order = self.cli_priority_order.clone();
        settings.wsl_host_address_mode = self.wsl_host_address_mode;
        settings.wsl_custom_host_address = self.wsl_custom_host_address.clone();
        settings.codex_home_mode = self.codex_home_mode;
        settings.codex_home_override = self.codex_home_override.clone();
        settings.codex_oauth_compatible_proxy_mode = self.codex_oauth_compatible_proxy_mode;
        settings.codex_provider_test_model = self.codex_provider_test_model.clone();
        settings.cx2cc_fallback_model_opus = self.cx2cc_fallback_model_opus.clone();
        settings.cx2cc_fallback_model_sonnet = self.cx2cc_fallback_model_sonnet.clone();
        settings.cx2cc_fallback_model_haiku = self.cx2cc_fallback_model_haiku.clone();
        settings.cx2cc_fallback_model_main = self.cx2cc_fallback_model_main.clone();
        settings.cx2cc_model_reasoning_effort = self.cx2cc_model_reasoning_effort.clone();
        settings.cx2cc_service_tier = self.cx2cc_service_tier.clone();
        settings.cx2cc_disable_response_storage = self.cx2cc_disable_response_storage;
        settings.cx2cc_enable_reasoning_to_thinking = self.cx2cc_enable_reasoning_to_thinking;
        settings.cx2cc_drop_stop_sequences = self.cx2cc_drop_stop_sequences;
        settings.cx2cc_clean_schema = self.cx2cc_clean_schema;
        settings.cx2cc_filter_batch_tool = self.cx2cc_filter_batch_tool;
        settings.upstream_proxy_enabled = self.upstream_proxy_enabled;
        settings.upstream_proxy_url = self.upstream_proxy_url.clone();
        settings.upstream_proxy_username = self.upstream_proxy_username.clone();
        settings.upstream_proxy_password = self.upstream_proxy_password.clone();
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "mode", content = "value")]
pub(crate) enum SensitiveStringUpdate {
    Preserve,
    Clear,
    Replace(String),
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct SettingsView {
    pub schema_version: u32,
    pub preferred_port: u16,
    pub show_home_heatmap: bool,
    pub show_home_usage: bool,
    pub home_usage_period: settings::HomeUsagePeriod,
    pub gateway_listen_mode: settings::GatewayListenMode,
    pub gateway_custom_listen_address: String,
    pub wsl_auto_config: bool,
    pub wsl_target_cli: settings::WslTargetCli,
    pub cli_priority_order: Vec<String>,
    pub wsl_host_address_mode: settings::WslHostAddressMode,
    pub wsl_custom_host_address: String,
    pub codex_home_mode: settings::CodexHomeMode,
    pub codex_home_override: String,
    pub codex_oauth_compatible_proxy_mode: bool,
    pub codex_provider_test_model: String,
    pub auto_start: bool,
    pub start_minimized: bool,
    pub tray_enabled: bool,
    pub enable_cli_proxy_startup_recovery: bool,
    pub log_retention_days: u32,
    pub request_log_retention_days: u32,
    pub provider_cooldown_seconds: u32,
    pub provider_base_url_ping_cache_ttl_seconds: u32,
    pub upstream_first_byte_timeout_seconds: u32,
    pub upstream_stream_idle_timeout_seconds: u32,
    pub upstream_request_timeout_non_streaming_seconds: u32,
    pub update_releases_url: String,
    pub failover_max_attempts_per_provider: u32,
    pub failover_max_providers_to_try: u32,
    pub upstream_retry_policy: settings::UpstreamRetryPolicy,
    pub circuit_breaker_failure_threshold: u32,
    pub circuit_breaker_open_duration_minutes: u32,
    pub enable_circuit_breaker_notice: bool,
    pub verbose_provider_error: bool,
    pub intercept_anthropic_warmup_requests: bool,
    pub enable_thinking_signature_rectifier: bool,
    pub enable_thinking_budget_rectifier: bool,
    pub enable_billing_header_rectifier: bool,
    pub enable_codex_session_id_completion: bool,
    pub enable_claude_metadata_user_id_injection: bool,
    pub enable_cache_anomaly_monitor: bool,
    pub enable_debug_log: bool,
    pub enable_task_complete_notify: bool,
    pub enable_notification_sound: bool,
    pub enable_response_fixer: bool,
    pub response_fixer_fix_encoding: bool,
    pub response_fixer_fix_sse_format: bool,
    pub response_fixer_fix_truncated_json: bool,
    pub response_fixer_max_json_depth: u32,
    pub response_fixer_max_fix_size: u32,
    pub cx2cc_fallback_model_opus: String,
    pub cx2cc_fallback_model_sonnet: String,
    pub cx2cc_fallback_model_haiku: String,
    pub cx2cc_fallback_model_main: String,
    pub cx2cc_model_reasoning_effort: String,
    pub cx2cc_service_tier: String,
    pub cx2cc_disable_response_storage: bool,
    pub cx2cc_enable_reasoning_to_thinking: bool,
    pub cx2cc_drop_stop_sequences: bool,
    pub cx2cc_clean_schema: bool,
    pub cx2cc_filter_batch_tool: bool,
    pub upstream_proxy_enabled: bool,
    pub upstream_proxy_url: String,
    pub upstream_proxy_username: String,
    pub upstream_proxy_password_configured: bool,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct SettingsMutationRuntime {
    pub gateway_rebound: bool,
    pub cli_proxy_synced: bool,
    pub wsl_auto_sync_triggered: bool,
    pub gateway_status: crate::gateway::GatewayStatus,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct SettingsMutationResult {
    pub settings: SettingsView,
    pub runtime: SettingsMutationRuntime,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayRectifierSettingsUpdate {
    pub verbose_provider_error: bool,
    pub intercept_anthropic_warmup_requests: bool,
    pub enable_thinking_signature_rectifier: bool,
    pub enable_thinking_budget_rectifier: bool,
    pub enable_billing_header_rectifier: bool,
    pub enable_claude_metadata_user_id_injection: bool,
    pub enable_response_fixer: bool,
    pub response_fixer_fix_encoding: bool,
    pub response_fixer_fix_sse_format: bool,
    pub response_fixer_fix_truncated_json: bool,
    pub response_fixer_max_json_depth: u32,
    pub response_fixer_max_fix_size: u32,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CircuitBreakerNoticeUpdate {
    pub enable_circuit_breaker_notice: bool,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSessionIdCompletionUpdate {
    pub enable_codex_session_id_completion: bool,
}

#[derive(Debug, Clone)]
struct SettingsRuntimePlan {
    cli_proxy_sync_required: bool,
    #[cfg(windows)]
    wsl_auto_sync_required: bool,
}

impl From<&settings::AppSettings> for SettingsView {
    fn from(value: &settings::AppSettings) -> Self {
        Self {
            schema_version: value.schema_version,
            preferred_port: value.preferred_port,
            show_home_heatmap: value.show_home_heatmap,
            show_home_usage: value.show_home_usage,
            home_usage_period: value.home_usage_period,
            gateway_listen_mode: value.gateway_listen_mode,
            gateway_custom_listen_address: value.gateway_custom_listen_address.clone(),
            wsl_auto_config: value.wsl_auto_config,
            wsl_target_cli: value.wsl_target_cli,
            cli_priority_order: value.cli_priority_order.clone(),
            wsl_host_address_mode: value.wsl_host_address_mode,
            wsl_custom_host_address: value.wsl_custom_host_address.clone(),
            codex_home_mode: value.codex_home_mode,
            codex_home_override: value.codex_home_override.clone(),
            codex_oauth_compatible_proxy_mode: value.codex_oauth_compatible_proxy_mode,
            codex_provider_test_model: value.codex_provider_test_model.clone(),
            auto_start: value.auto_start,
            start_minimized: value.start_minimized,
            tray_enabled: value.tray_enabled,
            enable_cli_proxy_startup_recovery: value.enable_cli_proxy_startup_recovery,
            log_retention_days: value.log_retention_days,
            request_log_retention_days: value.request_log_retention_days,
            provider_cooldown_seconds: value.provider_cooldown_seconds,
            provider_base_url_ping_cache_ttl_seconds: value
                .provider_base_url_ping_cache_ttl_seconds,
            upstream_first_byte_timeout_seconds: value.upstream_first_byte_timeout_seconds,
            upstream_stream_idle_timeout_seconds: value.upstream_stream_idle_timeout_seconds,
            upstream_request_timeout_non_streaming_seconds: value
                .upstream_request_timeout_non_streaming_seconds,
            update_releases_url: value.update_releases_url.clone(),
            failover_max_attempts_per_provider: value.failover_max_attempts_per_provider,
            failover_max_providers_to_try: value.failover_max_providers_to_try,
            upstream_retry_policy: value.upstream_retry_policy.clone(),
            circuit_breaker_failure_threshold: value.circuit_breaker_failure_threshold,
            circuit_breaker_open_duration_minutes: value.circuit_breaker_open_duration_minutes,
            enable_circuit_breaker_notice: value.enable_circuit_breaker_notice,
            verbose_provider_error: value.verbose_provider_error,
            intercept_anthropic_warmup_requests: value.intercept_anthropic_warmup_requests,
            enable_thinking_signature_rectifier: value.enable_thinking_signature_rectifier,
            enable_thinking_budget_rectifier: value.enable_thinking_budget_rectifier,
            enable_billing_header_rectifier: value.enable_billing_header_rectifier,
            enable_codex_session_id_completion: value.enable_codex_session_id_completion,
            enable_claude_metadata_user_id_injection: value
                .enable_claude_metadata_user_id_injection,
            enable_cache_anomaly_monitor: value.enable_cache_anomaly_monitor,
            enable_debug_log: value.enable_debug_log,
            enable_task_complete_notify: value.enable_task_complete_notify,
            enable_notification_sound: value.enable_notification_sound,
            enable_response_fixer: value.enable_response_fixer,
            response_fixer_fix_encoding: value.response_fixer_fix_encoding,
            response_fixer_fix_sse_format: value.response_fixer_fix_sse_format,
            response_fixer_fix_truncated_json: value.response_fixer_fix_truncated_json,
            response_fixer_max_json_depth: value.response_fixer_max_json_depth,
            response_fixer_max_fix_size: value.response_fixer_max_fix_size,
            cx2cc_fallback_model_opus: value.cx2cc_fallback_model_opus.clone(),
            cx2cc_fallback_model_sonnet: value.cx2cc_fallback_model_sonnet.clone(),
            cx2cc_fallback_model_haiku: value.cx2cc_fallback_model_haiku.clone(),
            cx2cc_fallback_model_main: value.cx2cc_fallback_model_main.clone(),
            cx2cc_model_reasoning_effort: value.cx2cc_model_reasoning_effort.clone(),
            cx2cc_service_tier: value.cx2cc_service_tier.clone(),
            cx2cc_disable_response_storage: value.cx2cc_disable_response_storage,
            cx2cc_enable_reasoning_to_thinking: value.cx2cc_enable_reasoning_to_thinking,
            cx2cc_drop_stop_sequences: value.cx2cc_drop_stop_sequences,
            cx2cc_clean_schema: value.cx2cc_clean_schema,
            cx2cc_filter_batch_tool: value.cx2cc_filter_batch_tool,
            upstream_proxy_enabled: value.upstream_proxy_enabled,
            upstream_proxy_url: value.upstream_proxy_url.clone(),
            upstream_proxy_username: value.upstream_proxy_username.clone(),
            upstream_proxy_password_configured: !value.upstream_proxy_password.is_empty(),
        }
    }
}

impl SettingsRuntimePlan {
    fn from_settings(previous: &settings::AppSettings, next: &settings::AppSettings) -> Self {
        let gateway_rebind_required = crate::gateway::listen_rebind_required(previous, next);
        let codex_home_changed = previous.codex_home_mode != next.codex_home_mode
            || previous.codex_home_override != next.codex_home_override;
        let codex_proxy_mode_changed =
            previous.codex_oauth_compatible_proxy_mode != next.codex_oauth_compatible_proxy_mode;
        let cli_proxy_sync_required =
            gateway_rebind_required || codex_home_changed || codex_proxy_mode_changed;
        #[cfg(windows)]
        let wsl_auto_sync_required = next.wsl_auto_config
            && next.gateway_listen_mode != settings::GatewayListenMode::Localhost
            && (previous.wsl_auto_config != next.wsl_auto_config
                || previous.wsl_target_cli != next.wsl_target_cli
                || previous.wsl_host_address_mode != next.wsl_host_address_mode
                || previous.wsl_custom_host_address != next.wsl_custom_host_address
                || codex_home_changed
                || gateway_rebind_required);
        Self {
            cli_proxy_sync_required,
            #[cfg(windows)]
            wsl_auto_sync_required,
        }
    }
}

fn apply_sensitive_string_update(
    update: Option<SensitiveStringUpdate>,
    previous: String,
) -> String {
    match update.unwrap_or(SensitiveStringUpdate::Preserve) {
        SensitiveStringUpdate::Preserve => previous,
        SensitiveStringUpdate::Clear => String::new(),
        SensitiveStringUpdate::Replace(value) => value,
    }
}

fn sync_runtime_side_effects<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    next_settings: &settings::AppSettings,
) -> Result<(), String> {
    #[cfg(test)]
    if let Some(error) = run_settings_runtime_sync_test_hook(next_settings) {
        return Err(error);
    }
    if let Some(resident) = app.try_state::<resident::ResidentState>() {
        resident.set_tray_enabled(next_settings.tray_enabled);
    }

    let circuit_runtime_updated = try_app_gateway_update_circuit_config(
        app,
        next_settings.circuit_breaker_failure_threshold.max(1),
        (next_settings.circuit_breaker_open_duration_minutes as i64).saturating_mul(60),
    );
    if !circuit_runtime_updated {
        tracing::debug!("circuit runtime is not active; settings commit remains canonical");
    }

    crate::gateway::http_client::sync_from_settings(next_settings)?;
    Ok(())
}

fn settings_snapshots_equal(left: &settings::AppSettings, right: &settings::AppSettings) -> bool {
    match (serde_json::to_value(left), serde_json::to_value(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Apply runtime side effects from canonical settings and repeat if a
/// coordinated writer commits while the side effect hook is running. The
/// bounded loop prevents a continuously mutating settings file from turning a
/// user-facing save into an unbounded operation.
fn sync_canonical_runtime_until_stable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<settings::AppSettings, String> {
    let mut canonical = settings::read(app).map_err(|error| error.to_string())?;
    for attempt in 0..3 {
        sync_runtime_side_effects(app, &canonical)?;
        let after = settings::read(app).map_err(|error| error.to_string())?;
        if settings_snapshots_equal(&canonical, &after) {
            // `canonical` is exactly the snapshot just passed to every runtime
            // side effect. Returning it is safe because equality was verified
            // after the side-effect pass.
            return Ok(canonical);
        }
        if attempt == 2 {
            return Err(
                "SETTINGS_RUNTIME_NOT_STABLE: canonical settings changed during runtime convergence"
                    .to_string(),
            );
        }
        canonical = after;
    }
    unreachable!("runtime convergence loop returns on every iteration")
}

#[derive(Debug, Clone)]
struct GatewayConvergenceState {
    /// Snapshot actually consumed by the most recent gateway start. It is
    /// never populated from a post-start canonical read.
    applied: settings::AppSettings,
    starts: usize,
    rebound: bool,
}

enum GatewayConvergenceAction {
    Start(Box<settings::AppSettings>),
    Stable,
    Fail,
}

/// Shared listener convergence state machine. Production owns the real async
/// stop/start and runtime callbacks; MockRuntime tests drive this same state
/// transition with injected start results.
fn gateway_convergence_action(
    previous_gateway_running: bool,
    state: &GatewayConvergenceState,
    canonical: &settings::AppSettings,
) -> GatewayConvergenceAction {
    if !previous_gateway_running
        || !crate::gateway::listen_rebind_required(&state.applied, canonical)
    {
        return GatewayConvergenceAction::Stable;
    }
    if state.starts >= 3 {
        GatewayConvergenceAction::Fail
    } else {
        GatewayConvergenceAction::Start(Box::new(canonical.clone()))
    }
}

fn gateway_applied_snapshot(
    start_input: &settings::AppSettings,
    effective_preferred_port: u16,
) -> settings::AppSettings {
    let mut applied = start_input.clone();
    applied.preferred_port = effective_preferred_port;
    applied
}

/// Record the result of a gateway start only after the start has returned.
/// The test barrier lives here so production and MockRuntime use the same
/// ordering: start result -> concurrent-writer barrier -> applied snapshot.
fn record_gateway_start_result(
    state: &mut GatewayConvergenceState,
    start_input: &settings::AppSettings,
    effective_preferred_port: u16,
) {
    #[cfg(test)]
    run_after_settings_gateway_start_test_hook();

    state.applied = gateway_applied_snapshot(start_input, effective_preferred_port);
    state.starts += 1;
    state.rebound = true;
}

fn current_gateway_status(app: &tauri::AppHandle) -> crate::gateway::GatewayStatus {
    app_gateway_status(app)
}

async fn start_gateway_with_settings_unlocked(
    app: &tauri::AppHandle,
    db_state: &DbInitState,
    next_settings: &settings::AppSettings,
) -> Result<crate::gateway::control_service::GatewayStartResult, String> {
    let db = ensure_db_ready(app.clone(), db_state).await?;
    let next_settings = next_settings.clone();
    let start_result = blocking::run("settings_set_gateway_start", {
        let app = app.clone();
        let db = db.clone();
        move || {
            app_start_gateway_with_config(
                &app,
                db,
                &next_settings,
                Some(next_settings.preferred_port),
            )
        }
    })
    .await?;

    crate::app::heartbeat_watchdog::gated_emit(
        app,
        GATEWAY_STATUS_EVENT_NAME,
        start_result.status.clone(),
    );
    Ok(start_result)
}

fn apply_settings_update_owned_patch(
    latest: &mut settings::AppSettings,
    update: &SettingsUpdate,
) -> crate::shared::error::AppResult<SettingsServiceOwnedToken> {
    let previous_token = SettingsServiceOwnedToken::from_settings(latest);

    let update_releases_url = update
        .update_releases_url
        .clone()
        .unwrap_or_else(|| previous_token.update_releases_url.clone())
        .trim()
        .to_string();
    let tray_enabled = update.tray_enabled.unwrap_or(previous_token.tray_enabled);
    let start_minimized = update
        .start_minimized
        .unwrap_or(previous_token.start_minimized);
    let enable_cli_proxy_startup_recovery = update
        .enable_cli_proxy_startup_recovery
        .unwrap_or(previous_token.enable_cli_proxy_startup_recovery);
    let request_log_retention_days = update
        .request_log_retention_days
        .unwrap_or(previous_token.request_log_retention_days);
    let provider_cooldown_seconds = update
        .provider_cooldown_seconds
        .unwrap_or(previous_token.provider_cooldown_seconds);
    let gateway_listen_mode = update
        .gateway_listen_mode
        .unwrap_or(previous_token.gateway_listen_mode);
    let show_home_heatmap = update
        .show_home_heatmap
        .unwrap_or(previous_token.show_home_heatmap);
    let show_home_usage = update
        .show_home_usage
        .unwrap_or(previous_token.show_home_usage);
    let home_usage_period = update
        .home_usage_period
        .unwrap_or(previous_token.home_usage_period);
    let gateway_custom_listen_address = update
        .gateway_custom_listen_address
        .clone()
        .unwrap_or_else(|| previous_token.gateway_custom_listen_address.clone())
        .trim()
        .to_string();
    let wsl_auto_config = update
        .wsl_auto_config
        .unwrap_or(previous_token.wsl_auto_config);
    let wsl_target_cli = update
        .wsl_target_cli
        .unwrap_or(previous_token.wsl_target_cli);
    let cli_priority_order = update
        .cli_priority_order
        .clone()
        .unwrap_or_else(|| previous_token.cli_priority_order.clone());
    let wsl_host_address_mode = update
        .wsl_host_address_mode
        .unwrap_or(previous_token.wsl_host_address_mode);
    let wsl_custom_host_address = update
        .wsl_custom_host_address
        .clone()
        .unwrap_or_else(|| previous_token.wsl_custom_host_address.clone())
        .trim()
        .to_string();
    let codex_home_mode = update
        .codex_home_mode
        .unwrap_or(previous_token.codex_home_mode);
    let codex_home_override = update
        .codex_home_override
        .clone()
        .unwrap_or_else(|| previous_token.codex_home_override.clone())
        .trim()
        .to_string();
    let codex_oauth_compatible_proxy_mode = update
        .codex_oauth_compatible_proxy_mode
        .unwrap_or(previous_token.codex_oauth_compatible_proxy_mode);
    let mut codex_provider_test_model = update
        .codex_provider_test_model
        .clone()
        .unwrap_or_else(|| previous_token.codex_provider_test_model.clone())
        .trim()
        .to_string();
    if codex_provider_test_model.is_empty() {
        codex_provider_test_model = settings::DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string();
    }

    let cx2cc_fallback_model_opus = update
        .cx2cc_fallback_model_opus
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_fallback_model_opus.clone())
        .trim()
        .to_string();
    if cx2cc_fallback_model_opus.is_empty() {
        return Err("cx2cc_fallback_model_opus cannot be empty".into());
    }
    let cx2cc_fallback_model_sonnet = update
        .cx2cc_fallback_model_sonnet
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_fallback_model_sonnet.clone())
        .trim()
        .to_string();
    if cx2cc_fallback_model_sonnet.is_empty() {
        return Err("cx2cc_fallback_model_sonnet cannot be empty".into());
    }
    let cx2cc_fallback_model_haiku = update
        .cx2cc_fallback_model_haiku
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_fallback_model_haiku.clone())
        .trim()
        .to_string();
    if cx2cc_fallback_model_haiku.is_empty() {
        return Err("cx2cc_fallback_model_haiku cannot be empty".into());
    }
    let cx2cc_fallback_model_main = update
        .cx2cc_fallback_model_main
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_fallback_model_main.clone())
        .trim()
        .to_string();
    if cx2cc_fallback_model_main.is_empty() {
        return Err("cx2cc_fallback_model_main cannot be empty".into());
    }
    let cx2cc_model_reasoning_effort = update
        .cx2cc_model_reasoning_effort
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_model_reasoning_effort.clone());
    let cx2cc_service_tier = update
        .cx2cc_service_tier
        .clone()
        .unwrap_or_else(|| previous_token.cx2cc_service_tier.clone());
    let cx2cc_disable_response_storage = update
        .cx2cc_disable_response_storage
        .unwrap_or(previous_token.cx2cc_disable_response_storage);
    let cx2cc_enable_reasoning_to_thinking = update
        .cx2cc_enable_reasoning_to_thinking
        .unwrap_or(previous_token.cx2cc_enable_reasoning_to_thinking);
    let cx2cc_drop_stop_sequences = update
        .cx2cc_drop_stop_sequences
        .unwrap_or(previous_token.cx2cc_drop_stop_sequences);
    let cx2cc_clean_schema = update
        .cx2cc_clean_schema
        .unwrap_or(previous_token.cx2cc_clean_schema);
    let cx2cc_filter_batch_tool = update
        .cx2cc_filter_batch_tool
        .unwrap_or(previous_token.cx2cc_filter_batch_tool);
    let upstream_proxy_enabled = update
        .upstream_proxy_enabled
        .unwrap_or(previous_token.upstream_proxy_enabled);
    let upstream_proxy_url = update
        .upstream_proxy_url
        .clone()
        .unwrap_or_else(|| previous_token.upstream_proxy_url.clone())
        .trim()
        .to_string();
    let upstream_proxy_username = update
        .upstream_proxy_username
        .clone()
        .unwrap_or_else(|| previous_token.upstream_proxy_username.clone())
        .trim()
        .to_string();
    let upstream_proxy_password = apply_sensitive_string_update(
        update.upstream_proxy_password.clone(),
        previous_token.upstream_proxy_password.clone(),
    );
    if upstream_proxy_enabled && upstream_proxy_url.is_empty() {
        return Err("upstream_proxy_url cannot be empty when upstream proxy is enabled".into());
    }
    let provider_base_url_ping_cache_ttl_seconds = update
        .provider_base_url_ping_cache_ttl_seconds
        .unwrap_or(previous_token.provider_base_url_ping_cache_ttl_seconds);
    let upstream_first_byte_timeout_seconds = update
        .upstream_first_byte_timeout_seconds
        .unwrap_or(previous_token.upstream_first_byte_timeout_seconds);
    let upstream_stream_idle_timeout_seconds = update
        .upstream_stream_idle_timeout_seconds
        .unwrap_or(previous_token.upstream_stream_idle_timeout_seconds);
    let upstream_request_timeout_non_streaming_seconds = update
        .upstream_request_timeout_non_streaming_seconds
        .unwrap_or(previous_token.upstream_request_timeout_non_streaming_seconds);
    let enable_cache_anomaly_monitor = update
        .enable_cache_anomaly_monitor
        .unwrap_or(previous_token.enable_cache_anomaly_monitor);
    let enable_debug_log = update
        .enable_debug_log
        .unwrap_or(previous_token.enable_debug_log);
    let enable_task_complete_notify = update
        .enable_task_complete_notify
        .unwrap_or(previous_token.enable_task_complete_notify);
    let enable_notification_sound = update
        .enable_notification_sound
        .unwrap_or(previous_token.enable_notification_sound);
    let mut upstream_retry_policy = update
        .upstream_retry_policy
        .clone()
        .unwrap_or_else(|| previous_token.upstream_retry_policy.clone());
    settings::sanitize_upstream_retry_policy(&mut upstream_retry_policy);
    let circuit_breaker_failure_threshold = update
        .circuit_breaker_failure_threshold
        .unwrap_or(previous_token.circuit_breaker_failure_threshold);
    let circuit_breaker_open_duration_minutes = update
        .circuit_breaker_open_duration_minutes
        .unwrap_or(previous_token.circuit_breaker_open_duration_minutes);

    let committed_token = SettingsServiceOwnedToken {
        preferred_port: update.preferred_port,
        show_home_heatmap,
        show_home_usage,
        home_usage_period,
        gateway_listen_mode,
        gateway_custom_listen_address,
        auto_start: update.auto_start,
        start_minimized,
        tray_enabled,
        enable_cli_proxy_startup_recovery,
        log_retention_days: update.log_retention_days,
        request_log_retention_days,
        provider_cooldown_seconds,
        provider_base_url_ping_cache_ttl_seconds,
        upstream_first_byte_timeout_seconds,
        upstream_stream_idle_timeout_seconds,
        upstream_request_timeout_non_streaming_seconds,
        enable_cache_anomaly_monitor,
        enable_debug_log,
        enable_task_complete_notify,
        enable_notification_sound,
        failover_max_attempts_per_provider: update.failover_max_attempts_per_provider,
        failover_max_providers_to_try: update.failover_max_providers_to_try,
        upstream_retry_policy,
        circuit_breaker_failure_threshold,
        circuit_breaker_open_duration_minutes,
        update_releases_url,
        wsl_auto_config,
        wsl_target_cli,
        cli_priority_order,
        wsl_host_address_mode,
        wsl_custom_host_address,
        codex_home_mode,
        codex_home_override,
        codex_oauth_compatible_proxy_mode,
        codex_provider_test_model,
        cx2cc_fallback_model_opus,
        cx2cc_fallback_model_sonnet,
        cx2cc_fallback_model_haiku,
        cx2cc_fallback_model_main,
        cx2cc_model_reasoning_effort,
        cx2cc_service_tier,
        cx2cc_disable_response_storage,
        cx2cc_enable_reasoning_to_thinking,
        cx2cc_drop_stop_sequences,
        cx2cc_clean_schema,
        cx2cc_filter_batch_tool,
        upstream_proxy_enabled,
        upstream_proxy_url,
        upstream_proxy_username,
        upstream_proxy_password,
    };

    committed_token.apply_to(latest);
    latest.schema_version = settings::SCHEMA_VERSION;
    settings::validate_bounds(latest)?;
    crate::gateway::http_client::validate_proxy_for_settings(latest)?;
    Ok(previous_token)
}

enum OwnedSettingsRollback {
    Restored,
    ConcurrentWinner(Box<settings::AppSettings>),
    Failed(String),
}

async fn rollback_settings_service_owned_fields<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_token: &SettingsServiceOwnedToken,
    committed_token: &SettingsServiceOwnedToken,
    auto_start_token: Option<crate::app::autostart::AutoStartCommitToken>,
) -> OwnedSettingsRollback {
    let previous_token = previous_token.clone();
    let committed_token = committed_token.clone();
    let rollback_result = blocking::run("settings_set_rollback", {
        let app = app.clone();
        move || {
            // Atomic ownership check under AUTO_START -> SETTINGS: generation,
            // auto_start committed value, and ordinary owned token are validated
            // together before any field is restored.
            Ok::<_, String>(crate::app::autostart::rollback_owned_with_auto_start_token(
                &app,
                auto_start_token,
                |latest| {
                    let current = SettingsServiceOwnedToken::from_settings(latest);
                    if current != committed_token {
                        return false;
                    }
                    previous_token.apply_to(latest);
                    true
                },
            ))
        }
    })
    .await;

    match rollback_result {
        Ok(crate::app::autostart::OwnedRollbackResult::Restored) => OwnedSettingsRollback::Restored,
        Ok(crate::app::autostart::OwnedRollbackResult::ConcurrentWinner(winner)) => {
            OwnedSettingsRollback::ConcurrentWinner(winner)
        }
        Ok(crate::app::autostart::OwnedRollbackResult::Failed(error)) => {
            OwnedSettingsRollback::Failed(error)
        }
        Err(error) => OwnedSettingsRollback::Failed(error.to_string()),
    }
}

fn sync_canonical_runtime_after_lost_rollback<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    winner: Option<settings::AppSettings>,
) -> Result<(), String> {
    let canonical = winner
        .or_else(|| settings::read(app).ok())
        .ok_or_else(|| "failed to read canonical settings for runtime resync".to_string())?;
    sync_runtime_side_effects(app, &canonical).map_err(|error| {
        format!("canonical runtime resync failed after settings rollback lost ownership: {error}")
    })
}

async fn rollback_after_runtime_sync_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_token: &SettingsServiceOwnedToken,
    committed_token: &SettingsServiceOwnedToken,
    auto_start_token: Option<crate::app::autostart::AutoStartCommitToken>,
) -> Option<String> {
    #[cfg(test)]
    run_before_settings_runtime_rollback_test_hook();
    match rollback_settings_service_owned_fields(
        app,
        previous_token,
        committed_token,
        auto_start_token,
    )
    .await
    {
        OwnedSettingsRollback::Restored => {
            let resync = settings::read(app)
                .map_err(|error| format!("failed to read restored settings: {error}"))
                .and_then(|restored| sync_runtime_side_effects(app, &restored));
            resync.err().map(|error| {
                format!(
                    "SETTINGS_RECOVERY_REQUIRED: restored settings runtime sync failed: {error}"
                )
            })
        }
        OwnedSettingsRollback::ConcurrentWinner(winner) => {
            sync_canonical_runtime_after_lost_rollback(app, Some(*winner))
                .err()
                .map(|error| format!("SETTINGS_RECOVERY_REQUIRED: {error}"))
        }
        OwnedSettingsRollback::Failed(error) => {
            tracing::error!(error = %error, "settings update rollback failed");
            let resync_error = sync_canonical_runtime_after_lost_rollback(app, None).err();
            let detail = resync_error
                .map(|resync| format!("; {resync}"))
                .unwrap_or_default();
            Some(format!(
                "SETTINGS_RECOVERY_REQUIRED: settings rollback failed: {error}{detail}"
            ))
        }
    }
}

async fn restore_previous_runtime(
    app: &tauri::AppHandle,
    db_state: &DbInitState,
    previous_settings: &settings::AppSettings,
    previous_gateway_status: &crate::gateway::GatewayStatus,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = sync_runtime_side_effects(app, previous_settings) {
        tracing::error!(
            error = %error,
            "settings update rollback failed to restore previous runtime settings"
        );
        errors.push(format!("runtime settings resync failed: {error}"));
    }

    if !previous_gateway_status.running {
        return if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        };
    }

    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    crate::app::cleanup::stop_gateway_best_effort_unlocked(app).await;
    match start_gateway_with_settings_unlocked(app, db_state, previous_settings).await {
        Ok(_) => {}
        Err(err) => {
            tracing::error!(
                error = %err,
                "settings update rollback failed to restore previous gateway runtime"
            );
            errors.push(format!("previous gateway runtime restore failed: {err}"));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

async fn rollback_settings_transaction(
    app: &tauri::AppHandle,
    db_state: &DbInitState,
    previous_settings: &settings::AppSettings,
    previous_token: &SettingsServiceOwnedToken,
    committed_token: &SettingsServiceOwnedToken,
    auto_start_token: Option<crate::app::autostart::AutoStartCommitToken>,
    previous_gateway_status: &crate::gateway::GatewayStatus,
) -> Result<(), String> {
    match rollback_settings_service_owned_fields(
        app,
        previous_token,
        committed_token,
        auto_start_token,
    )
    .await
    {
        OwnedSettingsRollback::Restored => {
            restore_previous_runtime(app, db_state, previous_settings, previous_gateway_status)
                .await
        }
        OwnedSettingsRollback::ConcurrentWinner(winner) => {
            tracing::warn!(
                "settings update rollback skipped because owned fields changed concurrently"
            );
            sync_canonical_runtime_after_lost_rollback(app, Some(*winner))
        }
        OwnedSettingsRollback::Failed(rollback_error) => {
            tracing::error!(
                error = %rollback_error,
                "settings update rollback failed to restore settings.json"
            );
            let resync_error = sync_canonical_runtime_after_lost_rollback(app, None).err();
            let detail = resync_error
                .map(|error| format!("; {error}"))
                .unwrap_or_default();
            Err(format!(
                "SETTINGS_RECOVERY_REQUIRED: settings rollback failed: {rollback_error}{detail}"
            ))
        }
    }
}

async fn sync_cli_proxy_for_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: String,
    apply_live: bool,
) -> bool {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    let status = crate::gateway_runtime_access::try_app_gateway_status(app).unwrap_or(
        crate::gateway::GatewayStatus {
            running: false,
            port: None,
            base_url: None,
            listen_addr: None,
        },
    );
    let (base_origin, apply_live) = if status.running {
        (
            status.base_url.unwrap_or_else(|| {
                format!(
                    "http://127.0.0.1:{}",
                    status.port.unwrap_or(settings::DEFAULT_GATEWAY_PORT)
                )
            }),
            true,
        )
    } else {
        (base_origin, apply_live && status.running)
    };

    match blocking::run("settings_set_cli_proxy_sync", {
        let app = app.clone();
        move || cli_proxy::sync_enabled(&app, &base_origin, apply_live)
    })
    .await
    {
        Ok(results) => {
            let failed_count = results.iter().filter(|row| !row.ok).count();
            if failed_count > 0 {
                tracing::warn!(
                    failed_count,
                    total = results.len(),
                    apply_live,
                    "settings update cli proxy sync completed with partial failures"
                );
            }
            failed_count == 0
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                apply_live,
                "settings update cli proxy sync failed"
            );
            false
        }
    }
}

pub(crate) async fn settings_get(app: tauri::AppHandle) -> Result<SettingsView, String> {
    blocking::run("settings_get", move || {
        settings::read(&app).map(|value| SettingsView::from(&value))
    })
    .await
    .map_err(Into::into)
}

fn commit_settings_update_owned<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: SettingsUpdate,
) -> Result<
    (
        settings::AppSettings,
        settings::AppSettings,
        SettingsServiceOwnedToken,
        SettingsServiceOwnedToken,
        crate::app::autostart::AutoStartCommitToken,
    ),
    String,
> {
    let desired_auto_start = update.auto_start;
    let committed = crate::app::autostart::commit_auto_start_with_owner(
        app,
        desired_auto_start,
        || {
            let (committed, previous_token) = settings::update(app, |latest| {
            let previous = apply_settings_update_owned_patch(latest, &update)?;
            Ok(previous)
        })
        .map_err(|err| {
            let message = err.to_string();
            // Persistence already classified finalize-only vs double failure.
            // Preserve that stable code instead of reclassifying by message
            // text; the frontend uses this distinction for readonly gating.
            if matches!(
                err.code(),
                "SETTINGS_PERSISTENCE_FAILED" | "SETTINGS_RECOVERY_REQUIRED"
            ) {
                return message;
            }
            // Real settings.json read/parse/corruption -> recovery.
            // Candidate field validation (bounds/proxy) stays SEC_INVALID_INPUT even
            // when the string happens to contain "settings".
            let is_settings_file_problem = message.contains("settings.json")
                || message.contains("SETTINGS_RECOVERY_REQUIRED")
                || message.contains("SETTINGS_CORRUPT")
                || message.contains("could not be read")
                || message.contains("failed to write temp settings file")
                || message.contains("failed to create settings backup")
                || message.contains("failed to finalize settings");
            let is_candidate_bounds = message.contains("SEC_INVALID_INPUT:")
                && !message.contains("settings.json");
            if is_settings_file_problem && !is_candidate_bounds {
                format!(
                    "SETTINGS_RECOVERY_REQUIRED: settings.json could not be read; fix or restore it before saving: {message}"
                )
            } else {
                message
            }
        })?;
            let committed_token = SettingsServiceOwnedToken::from_settings(&committed);
            let previous_auto_start = previous_token.auto_start;
            let committed_auto_start = committed_token.auto_start;
            Ok((
                (previous_token, committed, committed_token),
                previous_auto_start,
                committed_auto_start,
            ))
        },
    );
    #[cfg(test)]
    run_after_settings_autostart_commit_test_hook();
    committed.map(
        |((previous_token, committed, mut committed_token), token, effective_auto_start)| {
            // The ordinary token must describe the settings::update result owned by
            // this writer. The coordinator may correct only auto_start, so apply
            // that returned effective value without rereading another writer's
            // canonical snapshot (notably a preferred_port repair).
            let mut previous_settings = committed.clone();
            previous_token.apply_to(&mut previous_settings);
            let mut final_settings = committed;
            final_settings.auto_start = effective_auto_start;
            committed_token.auto_start = effective_auto_start;
            (
                previous_settings,
                final_settings,
                previous_token,
                committed_token,
                token,
            )
        },
    )
}

/// Outcome of a conditional preferred_port repair under settings write lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreferredPortRepairOutcome {
    Repaired { written_port: u16 },
    LostOwnership,
}

/// Production helper: only write repaired preferred_port while this writer still
/// owns the expected committed port. Used by the gateway rebind path and tests.
fn repair_preferred_port_if_owned<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    expected_port: u16,
    repaired_port: u16,
    mut committed_settings: settings::AppSettings,
    mut committed_token: SettingsServiceOwnedToken,
) -> Result<
    (
        PreferredPortRepairOutcome,
        settings::AppSettings,
        SettingsServiceOwnedToken,
    ),
    String,
> {
    let (written, owned) =
        crate::app::autostart::repair_preferred_port_if_current(app, expected_port, repaired_port)?;
    if owned {
        let written_port = written.preferred_port;
        apply_preferred_port_repair_success(
            &mut committed_settings,
            &mut committed_token,
            written_port,
        );
        Ok((
            PreferredPortRepairOutcome::Repaired { written_port },
            committed_settings,
            committed_token,
        ))
    } else {
        Ok((
            PreferredPortRepairOutcome::LostOwnership,
            committed_settings,
            committed_token,
        ))
    }
}

/// Apply a successful preferred_port repair to the ordinary committed snapshot
/// and owned token. Only the port field is absorbed; other ordinary fields keep
/// the values this writer originally committed so concurrent ordinary winners
/// are not stolen into the rollback token.
fn apply_preferred_port_repair_success(
    committed_settings: &mut settings::AppSettings,
    committed_token: &mut SettingsServiceOwnedToken,
    written_port: u16,
) {
    committed_settings.preferred_port = written_port;
    committed_token.absorb_preferred_port_repair(written_port);
}

/// Production settings commit used by IPC and tests with any runtime.
/// Gateway rebind (Wry-only) is only performed by the concrete AppHandle entry.
pub(crate) async fn settings_set_impl_generic<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    update: SettingsUpdate,
    allow_gateway_rebind: bool,
    db_state: Option<&DbInitState>,
) -> Result<SettingsMutationResult, String> {
    let app_for_work = app.clone();
    let update_for_work = update;

    #[cfg(test)]
    run_before_settings_set_lock_test_hook();

    // Durable commit + OS autostart happen under AUTO_START then SETTINGS locks.
    // No OS side effects occur before validate_bounds succeeds inside the patch.
    let (previous_settings, _committed_settings, previous_token, committed_token, auto_start_token) =
        blocking::run("settings_set", move || {
            commit_settings_update_owned(&app_for_work, update_for_work)
        })
        .await?;

    let _ = (allow_gateway_rebind, db_state);

    // Generic/runtime-agnostic path: skip gateway rebind (requires concrete Wry AppHandle).
    let gateway_status = crate::gateway::GatewayStatus {
        running: false,
        port: None,
        base_url: None,
        listen_addr: None,
    };
    let gateway_rebound = false;

    let canonical_settings = match sync_canonical_runtime_until_stable(&app) {
        Ok(settings) => settings,
        Err(sync_error) => {
            let recovery_error = rollback_after_runtime_sync_failure(
                &app,
                &previous_token,
                &committed_token,
                Some(auto_start_token),
            )
            .await;
            let recovery_suffix = recovery_error
                .map(|error| format!("; {error}"))
                .unwrap_or_default();
            return Err(format!("保存设置失败：{sync_error}{recovery_suffix}"));
        }
    };
    let runtime_plan = SettingsRuntimePlan::from_settings(&previous_settings, &canonical_settings);

    let cli_proxy_synced = if runtime_plan.cli_proxy_sync_required {
        let base_origin = crate::gateway::planned_base_url(&canonical_settings)?;
        sync_cli_proxy_for_settings(&app, base_origin, false).await
    } else {
        false
    };

    #[cfg(windows)]
    let wsl_auto_sync_triggered = false;
    #[cfg(not(windows))]
    let wsl_auto_sync_triggered = false;

    tracing::info!(
        preferred_port = canonical_settings.preferred_port,
        auto_start = canonical_settings.auto_start,
        tray_enabled = canonical_settings.tray_enabled,
        gateway_rebound,
        cli_proxy_synced,
        wsl_auto_sync_triggered,
        "settings updated"
    );

    Ok(SettingsMutationResult {
        settings: SettingsView::from(&canonical_settings),
        runtime: SettingsMutationRuntime {
            gateway_rebound,
            cli_proxy_synced,
            wsl_auto_sync_triggered,
            gateway_status,
        },
    })
}

async fn settings_set_impl_with_gateway(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    update: SettingsUpdate,
) -> Result<SettingsMutationResult, String> {
    let app_for_work = app.clone();
    let update_for_work = update;

    #[cfg(test)]
    run_before_settings_set_lock_test_hook();

    let (previous_settings, _committed_snapshot, previous_token, committed_token, auto_start_token) =
        blocking::run("settings_set", move || {
            commit_settings_update_owned(&app_for_work, update_for_work)
        })
        .await?;

    let previous_gateway_status = current_gateway_status(&app);
    let mut gateway_status = current_gateway_status(&app);
    let mut committed_settings = settings::read(&app).map_err(|error| error.to_string())?;
    let mut committed_token = committed_token;
    let mut convergence_state = GatewayConvergenceState {
        applied: previous_settings.clone(),
        starts: 0,
        rebound: false,
    };
    let _gateway_lifecycle = if previous_gateway_status.running {
        Some(crate::app::gateway_lifecycle_lock::lock().await)
    } else {
        None
    };

    for _ in 0..3 {
        let canonical_before_rebind = settings::read(&app).map_err(|error| error.to_string())?;
        match gateway_convergence_action(
            previous_gateway_status.running,
            &convergence_state,
            &canonical_before_rebind,
        ) {
            GatewayConvergenceAction::Start(start_settings) => {
                let start_settings = *start_settings;
                crate::app::cleanup::stop_gateway_best_effort_unlocked(&app).await;
                match start_gateway_with_settings_unlocked(&app, db_state, &start_settings).await {
                    Ok(start_result) => {
                        if start_result.effective_preferred_port != start_settings.preferred_port {
                            let repaired_port = start_result.effective_preferred_port;
                            let expected_port = start_settings.preferred_port;
                            let repair_settings = start_settings.clone();
                            let repair_token = committed_token.clone();
                            let write_result = blocking::run("settings_set_effective_port", {
                                let app = app.clone();
                                move || {
                                    repair_preferred_port_if_owned(
                                        &app,
                                        expected_port,
                                        repaired_port,
                                        repair_settings,
                                        repair_token,
                                    )
                                }
                            })
                            .await;
                            match write_result {
                                Ok((
                                    PreferredPortRepairOutcome::Repaired { written_port },
                                    _repaired_settings,
                                    repaired_token,
                                )) => {
                                    committed_token = repaired_token;
                                    tracing::debug!(
                                        written_port,
                                        "preferred_port repair committed"
                                    );
                                }
                                Ok((
                                    PreferredPortRepairOutcome::LostOwnership,
                                    _repaired_settings,
                                    _repaired_token,
                                )) => {
                                    tracing::warn!(
                                    "preferred_port repair skipped because a concurrent owner committed first"
                                );
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        error = %err,
                                        "failed to persist repaired preferred_port"
                                    );
                                }
                            }
                        }
                        gateway_status = start_result.status;
                        record_gateway_start_result(
                            &mut convergence_state,
                            &start_settings,
                            start_result.effective_preferred_port,
                        );
                    }
                    Err(rebind_error) => {
                        tracing::error!(
                            error = %rebind_error,
                            "settings update failed during gateway rebind; restoring previous runtime"
                        );
                        let recovery_error = rollback_settings_transaction(
                            &app,
                            db_state,
                            &previous_settings,
                            &previous_token,
                            &committed_token,
                            Some(auto_start_token),
                            &previous_gateway_status,
                        )
                        .await;
                        let recovery_suffix = recovery_error
                            .err()
                            .map(|error| format!("; {error}"))
                            .unwrap_or_default();
                        return Err(format!(
                            "监听地址未生效，新的运行态重绑失败：{rebind_error}{recovery_suffix}"
                        ));
                    }
                }
            }
            GatewayConvergenceAction::Stable => {
                if previous_gateway_status.running {
                    gateway_status = current_gateway_status(&app);
                }
            }
            GatewayConvergenceAction::Fail => {
                let instability_error =
                    "SETTINGS_RUNTIME_NOT_STABLE: gateway canonical settings changed after rebind";
                let recovery_error = rollback_settings_transaction(
                    &app,
                    db_state,
                    &previous_settings,
                    &previous_token,
                    &committed_token,
                    Some(auto_start_token),
                    &previous_gateway_status,
                )
                .await;
                let recovery_suffix = recovery_error
                    .err()
                    .map(|error| format!("; {error}"))
                    .unwrap_or_default();
                return Err(format!("{instability_error}{recovery_suffix}"));
            }
        }

        let runtime_settings = match sync_canonical_runtime_until_stable(&app) {
            Ok(settings) => settings,
            Err(sync_error) => {
                let recovery_error = rollback_settings_transaction(
                    &app,
                    db_state,
                    &previous_settings,
                    &previous_token,
                    &committed_token,
                    Some(auto_start_token),
                    &previous_gateway_status,
                )
                .await;
                let recovery_suffix = recovery_error
                    .err()
                    .map(|error| format!("; {error}"))
                    .unwrap_or_default();
                return Err(format!(
                    "监听地址重绑后运行态提交失败，已恢复旧配置：{sync_error}{recovery_suffix}"
                ));
            }
        };
        let canonical_after_runtime = settings::read(&app).map_err(|error| error.to_string())?;
        match gateway_convergence_action(
            previous_gateway_status.running,
            &convergence_state,
            &canonical_after_runtime,
        ) {
            GatewayConvergenceAction::Start(_) => continue,
            GatewayConvergenceAction::Stable => {
                committed_settings = runtime_settings;
                break;
            }
            GatewayConvergenceAction::Fail => {
                let instability_error =
                    "SETTINGS_RUNTIME_NOT_STABLE: gateway canonical settings changed after rebind";
                let recovery_error = rollback_settings_transaction(
                    &app,
                    db_state,
                    &previous_settings,
                    &previous_token,
                    &committed_token,
                    Some(auto_start_token),
                    &previous_gateway_status,
                )
                .await;
                let recovery_suffix = recovery_error
                    .err()
                    .map(|error| format!("; {error}"))
                    .unwrap_or_default();
                return Err(format!("{instability_error}{recovery_suffix}"));
            }
        }
    }

    let final_settings = committed_settings;
    let runtime_plan = SettingsRuntimePlan::from_settings(&previous_settings, &final_settings);
    let gateway_rebound = convergence_state.rebound;

    let cli_proxy_synced = if runtime_plan.cli_proxy_sync_required {
        let base_origin = if gateway_status.running {
            gateway_status.base_url.clone().unwrap_or_else(|| {
                format!(
                    "http://127.0.0.1:{}",
                    gateway_status.port.unwrap_or(final_settings.preferred_port)
                )
            })
        } else {
            crate::gateway::planned_base_url(&final_settings)?
        };
        sync_cli_proxy_for_settings(&app, base_origin, gateway_status.running).await
    } else {
        false
    };

    #[cfg(windows)]
    let wsl_auto_sync_triggered = if runtime_plan.wsl_auto_sync_required {
        match wsl_auto_sync_after_settings(&app).await {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!("WSL auto-sync after settings change failed: {}", err);
                false
            }
        }
    } else {
        false
    };
    #[cfg(not(windows))]
    let wsl_auto_sync_triggered = false;

    tracing::info!(
        preferred_port = final_settings.preferred_port,
        auto_start = final_settings.auto_start,
        tray_enabled = final_settings.tray_enabled,
        gateway_rebound,
        cli_proxy_synced,
        wsl_auto_sync_triggered,
        "settings updated"
    );

    Ok(SettingsMutationResult {
        settings: SettingsView::from(&final_settings),
        runtime: SettingsMutationRuntime {
            gateway_rebound,
            cli_proxy_synced,
            wsl_auto_sync_triggered,
            gateway_status,
        },
    })
}

pub(crate) async fn settings_set_impl(
    app: tauri::AppHandle,
    db_state: &DbInitState,
    update: SettingsUpdate,
) -> Result<SettingsMutationResult, String> {
    settings_set_impl_with_gateway(app, db_state, update).await
}

#[cfg(test)]
pub(crate) async fn settings_set_impl_for_test<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    update: SettingsUpdate,
) -> Result<SettingsMutationResult, String> {
    settings_set_impl_generic(app, update, false, None).await
}

#[cfg(test)]
type BeforeSettingsRuntimeRollbackTestHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
type BeforeSettingsSetLockTestHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
type AfterSettingsAutostartCommitTestHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
type AfterSettingsGatewayStartTestHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
fn before_settings_runtime_rollback_test_hook(
) -> &'static std::sync::Mutex<Option<BeforeSettingsRuntimeRollbackTestHook>> {
    static HOOK: std::sync::OnceLock<
        std::sync::Mutex<Option<BeforeSettingsRuntimeRollbackTestHook>>,
    > = std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn before_settings_set_lock_test_hook(
) -> &'static std::sync::Mutex<Option<BeforeSettingsSetLockTestHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<BeforeSettingsSetLockTestHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn after_settings_autostart_commit_test_hook(
) -> &'static std::sync::Mutex<Option<AfterSettingsAutostartCommitTestHook>> {
    static HOOK: std::sync::OnceLock<
        std::sync::Mutex<Option<AfterSettingsAutostartCommitTestHook>>,
    > = std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn after_settings_gateway_start_test_hook(
) -> &'static std::sync::Mutex<Option<AfterSettingsGatewayStartTestHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<AfterSettingsGatewayStartTestHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn set_before_settings_runtime_rollback_test_hook(hook: BeforeSettingsRuntimeRollbackTestHook) {
    *before_settings_runtime_rollback_test_hook()
        .lock()
        .expect("settings rollback test hook lock") = Some(hook);
}

#[cfg(test)]
pub(crate) fn set_before_settings_set_lock_test_hook(hook: BeforeSettingsSetLockTestHook) {
    *before_settings_set_lock_test_hook()
        .lock()
        .expect("settings set lock test hook") = Some(hook);
}

#[cfg(test)]
fn set_after_settings_autostart_commit_test_hook(hook: AfterSettingsAutostartCommitTestHook) {
    *after_settings_autostart_commit_test_hook()
        .lock()
        .expect("settings autostart commit test hook lock") = Some(hook);
}

#[cfg(test)]
fn set_after_settings_gateway_start_test_hook(hook: AfterSettingsGatewayStartTestHook) {
    *after_settings_gateway_start_test_hook()
        .lock()
        .expect("settings gateway start hook lock") = Some(hook);
}

#[cfg(test)]
fn clear_after_settings_gateway_start_test_hook() {
    *after_settings_gateway_start_test_hook()
        .lock()
        .expect("settings gateway start hook lock") = None;
}

#[cfg(test)]
fn run_before_settings_runtime_rollback_test_hook() {
    let hook = before_settings_runtime_rollback_test_hook()
        .lock()
        .expect("settings rollback test hook lock")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn run_before_settings_set_lock_test_hook() {
    let hook = before_settings_set_lock_test_hook()
        .lock()
        .expect("settings set lock test hook")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn run_after_settings_autostart_commit_test_hook() {
    let hook = after_settings_autostart_commit_test_hook()
        .lock()
        .expect("settings autostart commit test hook lock")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn run_after_settings_gateway_start_test_hook() {
    let hook = after_settings_gateway_start_test_hook()
        .lock()
        .expect("settings gateway start hook lock")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
type SettingsRuntimeSyncTestHook = Box<dyn FnMut(&settings::AppSettings) -> Option<String> + Send>;

#[cfg(test)]
fn settings_runtime_sync_test_hook(
) -> &'static std::sync::Mutex<Option<SettingsRuntimeSyncTestHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<SettingsRuntimeSyncTestHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn set_settings_runtime_sync_test_hook(hook: SettingsRuntimeSyncTestHook) {
    *settings_runtime_sync_test_hook()
        .lock()
        .expect("settings runtime sync test hook lock") = Some(hook);
}

#[cfg(test)]
fn run_settings_runtime_sync_test_hook(settings: &settings::AppSettings) -> Option<String> {
    settings_runtime_sync_test_hook()
        .lock()
        .expect("settings runtime sync test hook lock")
        .as_mut()
        .and_then(|hook| hook(settings))
}

/// Synchronous production body for gateway rectifier settings.
/// Shared by the async IPC path and ownership barrier tests (no nested block_on).
pub(crate) fn settings_gateway_rectifier_set_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: GatewayRectifierSettingsUpdate,
) -> Result<SettingsView, String> {
    write_settings_view(app, move |settings| {
        settings.verbose_provider_error = update.verbose_provider_error;
        settings.intercept_anthropic_warmup_requests = update.intercept_anthropic_warmup_requests;
        settings.enable_thinking_signature_rectifier = update.enable_thinking_signature_rectifier;
        settings.enable_thinking_budget_rectifier = update.enable_thinking_budget_rectifier;
        settings.enable_billing_header_rectifier = update.enable_billing_header_rectifier;
        settings.enable_claude_metadata_user_id_injection =
            update.enable_claude_metadata_user_id_injection;
        settings.enable_response_fixer = update.enable_response_fixer;
        settings.response_fixer_fix_encoding = update.response_fixer_fix_encoding;
        settings.response_fixer_fix_sse_format = update.response_fixer_fix_sse_format;
        settings.response_fixer_fix_truncated_json = update.response_fixer_fix_truncated_json;
        settings.response_fixer_max_json_depth = update.response_fixer_max_json_depth;
        settings.response_fixer_max_fix_size = update.response_fixer_max_fix_size;
        Ok(())
    })
    .map_err(|err| err.to_string())
}

pub(crate) async fn settings_gateway_rectifier_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    update: GatewayRectifierSettingsUpdate,
) -> Result<SettingsView, String> {
    let app_for_work = app.clone();
    let result = blocking::run("settings_gateway_rectifier_set", move || {
        settings_gateway_rectifier_set_sync(&app_for_work, update)
    })
    .await
    .map_err(|err| -> String { err.into() })?;

    tracing::info!(
        verbose_provider_error = result.verbose_provider_error,
        intercept_anthropic_warmup_requests = result.intercept_anthropic_warmup_requests,
        enable_thinking_signature_rectifier = result.enable_thinking_signature_rectifier,
        enable_thinking_budget_rectifier = result.enable_thinking_budget_rectifier,
        enable_billing_header_rectifier = result.enable_billing_header_rectifier,
        enable_claude_metadata_user_id_injection = result.enable_claude_metadata_user_id_injection,
        enable_response_fixer = result.enable_response_fixer,
        "gateway rectifier settings updated"
    );

    Ok(result)
}

/// Synchronous production body for circuit-breaker notice ownership.
pub(crate) fn settings_circuit_breaker_notice_set_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: CircuitBreakerNoticeUpdate,
) -> Result<SettingsView, String> {
    write_settings_view(app, move |settings| {
        settings.enable_circuit_breaker_notice = update.enable_circuit_breaker_notice;
        Ok(())
    })
    .map_err(|err| err.to_string())
}

pub(crate) async fn settings_circuit_breaker_notice_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    update: CircuitBreakerNoticeUpdate,
) -> Result<SettingsView, String> {
    let app_for_work = app.clone();
    blocking::run("settings_circuit_breaker_notice_set", move || {
        settings_circuit_breaker_notice_set_sync(&app_for_work, update)
    })
    .await
    .map_err(Into::into)
}

/// Synchronous production body for Codex session-id completion ownership.
pub(crate) fn settings_codex_session_id_completion_set_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    update: CodexSessionIdCompletionUpdate,
) -> Result<SettingsView, String> {
    write_settings_view(app, move |settings| {
        settings.enable_codex_session_id_completion = update.enable_codex_session_id_completion;
        Ok(())
    })
    .map_err(|err| err.to_string())
}

pub(crate) async fn settings_codex_session_id_completion_set<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    update: CodexSessionIdCompletionUpdate,
) -> Result<SettingsView, String> {
    let app_for_work = app.clone();
    blocking::run("settings_codex_session_id_completion_set", move || {
        settings_codex_session_id_completion_set_sync(&app_for_work, update)
    })
    .await
    .map_err(Into::into)
}

/// Background WSL sync triggered after settings change.
/// Delegates to the shared `wsl_auto_sync_core` which handles all precondition checks.
#[cfg(windows)]
async fn wsl_auto_sync_after_settings(app: &tauri::AppHandle) -> Result<(), String> {
    crate::commands::wsl::wsl_auto_sync_core(app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SettingsTestEnv {
        old_home: Option<std::ffi::OsString>,
        old_dotdir: Option<std::ffi::OsString>,
        _home: tempfile::TempDir,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl SettingsTestEnv {
        fn new() -> Self {
            let lock = crate::test_support::test_env_lock();
            let home = tempfile::tempdir().expect("settings tempdir");
            let old_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
            let old_dotdir = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
            std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
            std::env::set_var(
                "AIO_CODING_HUB_DOTDIR_NAME",
                format!(
                    ".aio-settings-owner-test-{}",
                    crate::shared::time::now_unix_millis()
                ),
            );
            crate::test_support::clear_settings_cache();
            Self {
                old_home,
                old_dotdir,
                _home: home,
                _lock: lock,
            }
        }
    }

    impl Drop for SettingsTestEnv {
        fn drop(&mut self) {
            match self.old_home.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
                None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
            }
            match self.old_dotdir.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
                None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
            }
            crate::test_support::clear_settings_cache();
        }
    }

    #[test]
    fn runtime_sync_failure_preserves_concurrent_owner_winner() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let previous = settings::read(&handle).expect("previous settings");
        let mut committed = previous.clone();
        committed.log_retention_days = previous.log_retention_days.saturating_add(1);
        settings::compare_and_swap(&handle, &previous, &committed).expect("commit candidate");

        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let hook_observed = observed.clone();
        let mut calls = 0usize;
        set_settings_runtime_sync_test_hook(Box::new(move |settings| {
            hook_observed
                .lock()
                .expect("observed settings")
                .push(settings.log_retention_days);
            calls += 1;
            (calls == 1).then(|| "forced runtime sync failure".to_string())
        }));
        assert!(sync_runtime_side_effects(&handle, &committed).is_err());

        let winner_retention = committed.log_retention_days.saturating_add(1);
        let hook_handle = handle.clone();
        set_before_settings_runtime_rollback_test_hook(Box::new(move || {
            settings::update(&hook_handle, |winner| {
                winner.log_retention_days = winner_retention;
                Ok(())
            })
            .expect("commit concurrent owner winner");
        }));
        let previous_token = SettingsServiceOwnedToken::from_settings(&previous);
        let committed_token = SettingsServiceOwnedToken::from_settings(&committed);
        assert!(
            tauri::async_runtime::block_on(rollback_after_runtime_sync_failure(
                &handle,
                &previous_token,
                &committed_token,
                None,
            ))
            .is_none()
        );

        let canonical = settings::read(&handle).expect("canonical settings");
        assert_eq!(canonical.log_retention_days, winner_retention);
        assert_eq!(
            *observed.lock().expect("observed settings"),
            vec![committed.log_retention_days, winner_retention]
        );
        *settings_runtime_sync_test_hook()
            .lock()
            .expect("settings runtime sync test hook lock") = None;
    }

    #[test]
    fn settings_update_deserializes_cx2cc_fields_from_specta_keys() {
        let json = serde_json::json!({
            "preferredPort": 37123,
            "autoStart": false,
            "logRetentionDays": 30,
            "failoverMaxAttemptsPerProvider": 5,
            "failoverMaxProvidersToTry": 3,
            "cx2CcFallbackModelOpus": "gpt-5",
            "cx2CcFallbackModelSonnet": "gpt-4.1",
            "cx2CcFallbackModelHaiku": "gpt-4.1-mini",
            "cx2CcFallbackModelMain": "gpt-5.4",
            "cx2CcModelReasoningEffort": "high",
            "cx2CcServiceTier": "flex",
            "cx2CcDisableResponseStorage": false,
            "cx2CcEnableReasoningToThinking": true,
            "cx2CcDropStopSequences": true,
            "cx2CcCleanSchema": false,
            "cx2CcFilterBatchTool": true
        });

        let update: SettingsUpdate = serde_json::from_value(json).expect("should deserialize");
        assert_eq!(update.cx2cc_fallback_model_opus.as_deref(), Some("gpt-5"));
        assert_eq!(
            update.cx2cc_fallback_model_sonnet.as_deref(),
            Some("gpt-4.1")
        );
        assert_eq!(
            update.cx2cc_fallback_model_haiku.as_deref(),
            Some("gpt-4.1-mini")
        );
        assert_eq!(update.cx2cc_fallback_model_main.as_deref(), Some("gpt-5.4"));
        assert_eq!(update.cx2cc_model_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(update.cx2cc_service_tier.as_deref(), Some("flex"));
        assert_eq!(update.cx2cc_disable_response_storage, Some(false));
        assert_eq!(update.cx2cc_enable_reasoning_to_thinking, Some(true));
        assert_eq!(update.cx2cc_drop_stop_sequences, Some(true));
        assert_eq!(update.cx2cc_clean_schema, Some(false));
        assert_eq!(update.cx2cc_filter_batch_tool, Some(true));
    }

    #[test]
    fn settings_update_cx2cc_fields_default_to_none_when_absent() {
        let json = serde_json::json!({
            "preferredPort": 37123,
            "autoStart": false,
            "logRetentionDays": 30,
            "failoverMaxAttemptsPerProvider": 5,
            "failoverMaxProvidersToTry": 3
        });

        let update: SettingsUpdate = serde_json::from_value(json).expect("should deserialize");
        assert!(update.cx2cc_model_reasoning_effort.is_none());
        assert!(update.cx2cc_fallback_model_opus.is_none());
        assert!(update.cx2cc_filter_batch_tool.is_none());
    }

    fn ordinary_update_from_settings(
        settings: &settings::AppSettings,
        auto_start: bool,
        circuit_breaker_failure_threshold: Option<u32>,
    ) -> SettingsUpdate {
        SettingsUpdate {
            preferred_port: settings.preferred_port,
            show_home_heatmap: Some(settings.show_home_heatmap),
            show_home_usage: Some(settings.show_home_usage),
            home_usage_period: Some(settings.home_usage_period),
            gateway_listen_mode: Some(settings.gateway_listen_mode),
            gateway_custom_listen_address: Some(settings.gateway_custom_listen_address.clone()),
            auto_start,
            start_minimized: Some(settings.start_minimized),
            tray_enabled: Some(settings.tray_enabled),
            enable_cli_proxy_startup_recovery: Some(settings.enable_cli_proxy_startup_recovery),
            log_retention_days: settings.log_retention_days,
            request_log_retention_days: Some(settings.request_log_retention_days),
            provider_cooldown_seconds: Some(settings.provider_cooldown_seconds),
            provider_base_url_ping_cache_ttl_seconds: Some(
                settings.provider_base_url_ping_cache_ttl_seconds,
            ),
            upstream_first_byte_timeout_seconds: Some(settings.upstream_first_byte_timeout_seconds),
            upstream_stream_idle_timeout_seconds: Some(
                settings.upstream_stream_idle_timeout_seconds,
            ),
            upstream_request_timeout_non_streaming_seconds: Some(
                settings.upstream_request_timeout_non_streaming_seconds,
            ),
            enable_cache_anomaly_monitor: Some(settings.enable_cache_anomaly_monitor),
            enable_debug_log: Some(settings.enable_debug_log),
            enable_task_complete_notify: Some(settings.enable_task_complete_notify),
            enable_notification_sound: Some(settings.enable_notification_sound),
            failover_max_attempts_per_provider: settings.failover_max_attempts_per_provider,
            failover_max_providers_to_try: settings.failover_max_providers_to_try,
            upstream_retry_policy: Some(settings.upstream_retry_policy.clone()),
            circuit_breaker_failure_threshold,
            circuit_breaker_open_duration_minutes: Some(
                settings.circuit_breaker_open_duration_minutes,
            ),
            update_releases_url: Some(settings.update_releases_url.clone()),
            wsl_auto_config: Some(settings.wsl_auto_config),
            wsl_target_cli: Some(settings.wsl_target_cli),
            cli_priority_order: Some(settings.cli_priority_order.clone()),
            wsl_host_address_mode: Some(settings.wsl_host_address_mode),
            wsl_custom_host_address: Some(settings.wsl_custom_host_address.clone()),
            codex_home_mode: Some(settings.codex_home_mode),
            codex_home_override: Some(settings.codex_home_override.clone()),
            codex_oauth_compatible_proxy_mode: Some(settings.codex_oauth_compatible_proxy_mode),
            codex_provider_test_model: Some(settings.codex_provider_test_model.clone()),
            cx2cc_fallback_model_opus: Some(settings.cx2cc_fallback_model_opus.clone()),
            cx2cc_fallback_model_sonnet: Some(settings.cx2cc_fallback_model_sonnet.clone()),
            cx2cc_fallback_model_haiku: Some(settings.cx2cc_fallback_model_haiku.clone()),
            cx2cc_fallback_model_main: Some(settings.cx2cc_fallback_model_main.clone()),
            cx2cc_model_reasoning_effort: Some(settings.cx2cc_model_reasoning_effort.clone()),
            cx2cc_service_tier: Some(settings.cx2cc_service_tier.clone()),
            cx2cc_disable_response_storage: Some(settings.cx2cc_disable_response_storage),
            cx2cc_enable_reasoning_to_thinking: Some(settings.cx2cc_enable_reasoning_to_thinking),
            cx2cc_drop_stop_sequences: Some(settings.cx2cc_drop_stop_sequences),
            cx2cc_clean_schema: Some(settings.cx2cc_clean_schema),
            cx2cc_filter_batch_tool: Some(settings.cx2cc_filter_batch_tool),
            upstream_proxy_enabled: Some(settings.upstream_proxy_enabled),
            upstream_proxy_url: Some(settings.upstream_proxy_url.clone()),
            upstream_proxy_username: Some(settings.upstream_proxy_username.clone()),
            upstream_proxy_password: Some(SensitiveStringUpdate::Preserve),
        }
    }

    #[test]
    fn invalid_auto_start_candidate_makes_zero_os_sync_calls() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        crate::app::autostart::reset_auto_start_sync_test_calls();

        let previous = settings::read(&handle).expect("previous");
        assert!(!previous.auto_start);
        let update = ordinary_update_from_settings(&previous, true, Some(0));

        let err =
            tauri::async_runtime::block_on(settings_set_impl_for_test(handle.clone(), update))
                .expect_err("invalid circuit threshold must fail");
        assert!(
            err.contains("SEC_INVALID_INPUT:"),
            "illegal candidate must stay SEC_INVALID_INPUT, got: {err}"
        );
        assert!(
            err.contains("circuit_breaker_failure_threshold"),
            "unexpected error: {err}"
        );
        assert!(
            !err.contains("SETTINGS_RECOVERY_REQUIRED"),
            "must not misclassify candidate validation as recovery: {err}"
        );

        let canonical = settings::read(&handle).expect("canonical");
        assert!(!canonical.auto_start);
        assert_eq!(crate::app::autostart::auto_start_sync_test_calls(), 0);
    }

    #[test]
    fn ordinary_save_preserves_dedicated_owner_fields_under_lock_barrier() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let previous = settings::read(&handle).expect("previous");

        // Real Image Gen production path needs a writable absolute dir + DB.
        let img_root = tempfile::tempdir().expect("img root");
        let img_path = img_root.path().to_path_buf();
        let db = crate::db::init(&handle).expect("init db for image gen owner");

        // Hook only pauses A (ordinary save). B runs real dedicated production
        // writers from the test thread, then A is released.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let barrier_for_hook = barrier.clone();
        set_before_settings_set_lock_test_hook(Box::new(move || {
            barrier_for_hook.wait();
            // Pause until the test thread finishes real dedicated writers.
            barrier_for_hook.wait();
        }));

        let mut update = ordinary_update_from_settings(&previous, previous.auto_start, None);
        update.log_retention_days = previous.log_retention_days.saturating_add(3);

        let handle_for_save = app.handle().clone();
        let join = std::thread::spawn(move || {
            tauri::async_runtime::block_on(settings_set_impl_for_test(handle_for_save, update))
        });

        // Wait until A is paused at the pre-settings-lock barrier.
        barrier.wait();

        // Real production dedicated writers (not hand-written field assigns).
        settings_circuit_breaker_notice_set_sync(
            &handle,
            CircuitBreakerNoticeUpdate {
                enable_circuit_breaker_notice: true,
            },
        )
        .expect("circuit notice production command path");
        settings_codex_session_id_completion_set_sync(
            &handle,
            CodexSessionIdCompletionUpdate {
                enable_codex_session_id_completion: true,
            },
        )
        .expect("codex completion production command path");
        settings_gateway_rectifier_set_sync(
            &handle,
            GatewayRectifierSettingsUpdate {
                verbose_provider_error: true,
                intercept_anthropic_warmup_requests: true,
                enable_thinking_signature_rectifier: false,
                enable_thinking_budget_rectifier: true,
                enable_billing_header_rectifier: false,
                enable_claude_metadata_user_id_injection: true,
                enable_response_fixer: false,
                response_fixer_fix_encoding: false,
                response_fixer_fix_sse_format: true,
                response_fixer_fix_truncated_json: false,
                response_fixer_max_json_depth: 9,
                response_fixer_max_fix_size: 12345,
            },
        )
        .expect("rectifier production command path");

        crate::app::image_gen_service::commit_image_gen_storage_dir_settings(
            &handle,
            &db,
            img_path.to_string_lossy().as_ref(),
        )
        .expect("image gen storage production API");

        crate::grok_config::set(
            &handle,
            crate::grok_config::GrokProxyPreferences {
                model_id: "test-model".to_string(),
                api_backend: crate::grok_config::GrokApiBackend::Responses,
                context_window: Some(128000),
                telemetry: Some(false),
                supports_backend_search: Some(true),
            },
        )
        .expect("grok production set");

        // Resume A so ordinary owned patch commits after dedicated winners.
        barrier.wait();
        let result = join.join().expect("join save").expect("ordinary save");

        let canonical = settings::read(app.handle()).expect("canonical");
        assert_eq!(
            canonical.log_retention_days,
            previous.log_retention_days + 3
        );
        assert!(canonical.enable_circuit_breaker_notice);
        assert!(canonical.enable_codex_session_id_completion);
        assert!(canonical.verbose_provider_error);
        assert!(canonical.intercept_anthropic_warmup_requests);
        assert!(!canonical.enable_thinking_signature_rectifier);
        assert!(canonical.enable_thinking_budget_rectifier);
        assert!(!canonical.enable_billing_header_rectifier);
        assert!(canonical.enable_claude_metadata_user_id_injection);
        assert!(!canonical.enable_response_fixer);
        assert!(!canonical.response_fixer_fix_encoding);
        assert!(canonical.response_fixer_fix_sse_format);
        assert!(!canonical.response_fixer_fix_truncated_json);
        assert_eq!(canonical.response_fixer_max_json_depth, 9);
        assert_eq!(canonical.response_fixer_max_fix_size, 12345);
        let expected_img = std::fs::canonicalize(&img_path)
            .unwrap_or(img_path.clone())
            .to_string_lossy()
            .to_string();
        assert_eq!(
            canonical.image_gen_storage_dir.as_deref(),
            Some(expected_img.as_str())
        );
        assert!(
            canonical
                .image_gen_storage_roots
                .iter()
                .any(|root| root == &expected_img),
            "image gen roots must include production path: {:?}",
            canonical.image_gen_storage_roots
        );
        assert_eq!(
            canonical
                .grok_proxy_preferences
                .as_ref()
                .map(|p| p.model_id.as_str()),
            Some("test-model")
        );
        assert_eq!(
            result.settings.log_retention_days,
            canonical.log_retention_days
        );

        // Ordinary SettingsUpdate must ignore rectifier exclusive fields.
        let with_extra = serde_json::json!({
            "preferredPort": canonical.preferred_port,
            "autoStart": canonical.auto_start,
            "logRetentionDays": canonical.log_retention_days,
            "failoverMaxAttemptsPerProvider": canonical.failover_max_attempts_per_provider,
            "failoverMaxProvidersToTry": canonical.failover_max_providers_to_try,
            "verboseProviderError": true,
            "enableThinkingSignatureRectifier": false,
            "enableResponseFixer": true,
            "responseFixerFixEncoding": true
        });
        let parsed: SettingsUpdate =
            serde_json::from_value(with_extra).expect("extra rectifier fields ignored");
        let _ = parsed.preferred_port;
    }

    #[test]
    fn preferred_port_repair_success_only_updates_port_on_token() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();

        let previous = settings::read(&handle).expect("previous");
        let mut committed = previous.clone();
        committed.preferred_port = 37123;
        committed.log_retention_days = 11;
        settings::compare_and_swap(&handle, &previous, &committed).expect("seed committed port");

        // Concurrent ordinary winner changes another owned field while keeping port.
        settings::update(&handle, |latest| {
            latest.log_retention_days = 99;
            Ok(())
        })
        .expect("concurrent ordinary winner");

        let mut token = SettingsServiceOwnedToken::from_settings(&committed);
        let (repair_outcome, repaired_settings, repaired_token) =
            repair_preferred_port_if_owned(&handle, 37123, 38080, committed, token)
                .expect("repair should succeed while port still owned");
        committed = repaired_settings;
        token = repaired_token;
        match repair_outcome {
            PreferredPortRepairOutcome::Repaired { written_port } => {
                assert_eq!(written_port, committed.preferred_port);
            }
            PreferredPortRepairOutcome::LostOwnership => {
                panic!("repair must succeed while preferred_port still owned")
            }
        }

        assert_eq!(token.preferred_port, 38080);
        // Token must NOT absorb concurrent ordinary winner fields.
        assert_eq!(token.log_retention_days, 11);
        let canonical = settings::read(&handle).expect("canonical");
        assert_eq!(canonical.preferred_port, 38080);
        assert_eq!(canonical.log_retention_days, 99);
    }

    #[test]
    fn preferred_port_repair_loser_keeps_original_token_and_runtime_rollback_preserves_winner() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        crate::app::autostart::reset_auto_start_sync_test_calls();

        let previous = settings::read(&handle).expect("previous");
        let mut committed = previous.clone();
        committed.preferred_port = 37123;
        committed.log_retention_days = 11;
        committed.auto_start = true;
        settings::compare_and_swap(&handle, &previous, &committed).expect("seed committed");

        // Concurrent ordinary winner takes preferred_port + retention.
        let mut winner = committed.clone();
        winner.preferred_port = 39000;
        winner.log_retention_days = 77;
        winner.auto_start = false;
        settings::compare_and_swap(&handle, &committed, &winner).expect("seed winner");
        crate::app::autostart::converge_auto_start_to_canonical(&handle)
            .expect("converge canonical winner autostart");

        let token = SettingsServiceOwnedToken::from_settings(&committed);
        let (repair_outcome, committed_after_repair, token_after_repair) =
            repair_preferred_port_if_owned(&handle, 37123, 38080, committed.clone(), token)
                .expect("repair call must not fail hard");
        assert_eq!(committed_after_repair.preferred_port, 37123);
        let token = token_after_repair;
        match repair_outcome {
            PreferredPortRepairOutcome::LostOwnership => {
                // Production gateway path keeps original committed token on loser.
            }
            PreferredPortRepairOutcome::Repaired { written_port } => {
                panic!("repair must lose ownership to concurrent winner, wrote {written_port}")
            }
        }
        assert_eq!(token.preferred_port, 37123);
        assert_eq!(token.log_retention_days, 11);

        // Runtime/rebind failure rollback with the original token must not overwrite winner.
        let previous_token = SettingsServiceOwnedToken::from_settings(&previous);
        set_settings_runtime_sync_test_hook(Box::new(|_| None));
        assert!(
            tauri::async_runtime::block_on(rollback_after_runtime_sync_failure(
                &handle,
                &previous_token,
                &token,
                None,
            ))
            .is_none()
        );

        let canonical = settings::read(&handle).expect("canonical winner preserved");
        assert_eq!(canonical.preferred_port, 39000);
        assert_eq!(canonical.log_retention_days, 77);
        assert!(!canonical.auto_start);

        *settings_runtime_sync_test_hook()
            .lock()
            .expect("settings runtime sync test hook lock") = None;
    }

    #[test]
    fn post_coordinator_preferred_port_winner_is_not_absorbed_by_runtime_rollback() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let previous = settings::read(&handle).expect("previous settings");
        let a_port = previous.preferred_port.saturating_add(1);
        let b_port = a_port.saturating_add(1);

        let b_handle = handle.clone();
        set_after_settings_autostart_commit_test_hook(Box::new(move || {
            let committed = settings::read(&b_handle).expect("A durable commit");
            let token = SettingsServiceOwnedToken::from_settings(&committed);
            let (outcome, _, _) =
                repair_preferred_port_if_owned(&b_handle, a_port, b_port, committed, token)
                    .expect("B preferred-port repair");
            assert_eq!(
                outcome,
                PreferredPortRepairOutcome::Repaired {
                    written_port: b_port
                }
            );
        }));

        let observed_ports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_hook = observed_ports.clone();
        let mut calls = 0usize;
        set_settings_runtime_sync_test_hook(Box::new(move |settings| {
            observed_for_hook
                .lock()
                .expect("runtime observations")
                .push(settings.preferred_port);
            calls += 1;
            (calls == 1).then(|| "forced runtime sync failure".to_string())
        }));

        let mut update = ordinary_update_from_settings(&previous, previous.auto_start, None);
        update.preferred_port = a_port;
        let err =
            tauri::async_runtime::block_on(settings_set_impl_for_test(handle.clone(), update))
                .expect_err("runtime failure must surface");
        assert!(
            err.contains("forced runtime sync failure"),
            "unexpected error: {err}"
        );

        let canonical = settings::read(&handle).expect("canonical winner");
        assert_eq!(canonical.preferred_port, b_port);
        assert_eq!(
            *observed_ports.lock().expect("runtime observations"),
            vec![b_port, b_port],
            "initial sync and rollback resync must both use B, never A's old port"
        );

        *settings_runtime_sync_test_hook()
            .lock()
            .expect("settings runtime sync test hook lock") = None;
    }

    #[test]
    fn successful_settings_response_and_runtime_converge_to_latest_winner() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let previous = settings::read(&handle).expect("previous settings");
        let a_port = previous.preferred_port.saturating_add(1);
        let b_port = a_port.saturating_add(1);
        let b_handle = handle.clone();

        set_after_settings_autostart_commit_test_hook(Box::new(move || {
            settings::update(&b_handle, |latest| {
                latest.preferred_port = b_port;
                latest.log_retention_days = latest.log_retention_days.saturating_add(11);
                Ok(())
            })
            .expect("B winner commit");
        }));

        let observed_ports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_hook = observed_ports.clone();
        set_settings_runtime_sync_test_hook(Box::new(move |latest| {
            observed_for_hook
                .lock()
                .expect("runtime observations")
                .push(latest.preferred_port);
            None
        }));

        let mut update = ordinary_update_from_settings(&previous, previous.auto_start, None);
        update.preferred_port = a_port;
        let result =
            tauri::async_runtime::block_on(settings_set_impl_for_test(handle.clone(), update))
                .expect("settings save");

        let canonical = settings::read(&handle).expect("canonical winner");
        assert_eq!(canonical.preferred_port, b_port);
        assert_eq!(result.settings.preferred_port, b_port);
        assert_eq!(
            *observed_ports.lock().expect("runtime observations"),
            vec![b_port],
            "success must sync the canonical winner, never stale A"
        );

        *settings_runtime_sync_test_hook()
            .lock()
            .expect("settings runtime sync hook lock") = None;
    }

    #[test]
    fn gateway_start_return_barrier_rebinds_from_canonical_listener_winner() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let previous = settings::read(&handle).expect("previous settings");
        let a_port = previous.preferred_port.saturating_add(1);
        let b_port = a_port.saturating_add(1);

        let mut imported = previous.clone();
        imported.preferred_port = a_port;
        settings::compare_and_swap(&handle, &previous, &imported).expect("A listener commit");

        let winner_handle = handle.clone();
        set_after_settings_gateway_start_test_hook(Box::new(move || {
            settings::update(&winner_handle, |latest| {
                latest.preferred_port = b_port;
                latest.log_retention_days = latest.log_retention_days.saturating_add(17);
                Ok(())
            })
            .expect("B listener winner after start returns");
        }));

        let mut state = GatewayConvergenceState {
            applied: previous.clone(),
            starts: 0,
            rebound: false,
        };
        let mut start_inputs = Vec::new();

        let canonical_a = settings::read(&handle).expect("canonical A");
        let GatewayConvergenceAction::Start(start_input) =
            gateway_convergence_action(true, &state, &canonical_a)
        else {
            panic!("A listener change must request a start");
        };
        let start_input = *start_input;
        start_inputs.push(start_input.preferred_port);
        let effective_a_port = a_port;
        // The helper contains the exact production barrier: start has returned,
        // then the writer can interleave before applied is recorded.
        record_gateway_start_result(&mut state, &start_input, effective_a_port);

        let canonical_b = settings::read(&handle).expect("canonical B after barrier");
        let GatewayConvergenceAction::Start(next_start_input) =
            gateway_convergence_action(true, &state, &canonical_b)
        else {
            panic!(
                "B listener winner must force a second start; applied must remain A input until recorded"
            );
        };
        let next_start_input = *next_start_input;
        start_inputs.push(next_start_input.preferred_port);
        record_gateway_start_result(&mut state, &next_start_input, b_port);

        let canonical_after = settings::read(&handle).expect("canonical after B start");
        assert!(matches!(
            gateway_convergence_action(true, &state, &canonical_after),
            GatewayConvergenceAction::Stable
        ));
        assert_eq!(start_inputs, vec![a_port, b_port]);
        assert_eq!(state.applied.preferred_port, canonical_after.preferred_port);
        assert_eq!(
            SettingsView::from(&canonical_after).preferred_port,
            state.applied.preferred_port,
            "response snapshot and applied gateway listener must use B"
        );
        assert!(state.rebound);

        clear_after_settings_gateway_start_test_hook();
    }

    #[test]
    fn runtime_failure_restores_ordinary_fields_when_autostart_changed() {
        let _env = SettingsTestEnv::new();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        crate::app::autostart::reset_auto_start_sync_test_calls();

        let previous = settings::read(&handle).expect("previous");
        assert!(!previous.auto_start);
        let mut update = ordinary_update_from_settings(&previous, true, None);
        update.log_retention_days = previous.log_retention_days.saturating_add(2);

        set_settings_runtime_sync_test_hook(Box::new(|_| {
            Some("forced runtime sync failure".to_string())
        }));

        let err =
            tauri::async_runtime::block_on(settings_set_impl_for_test(handle.clone(), update))
                .expect_err("runtime failure should surface");
        assert!(err.contains("保存设置失败") || err.contains("forced runtime sync failure"));

        let canonical = settings::read(&handle).expect("canonical");
        assert_eq!(canonical.log_retention_days, previous.log_retention_days);
        assert!(!canonical.auto_start);
        assert_eq!(
            crate::app::autostart::auto_start_sync_test_targets()
                .last()
                .copied(),
            Some(false)
        );

        *settings_runtime_sync_test_hook()
            .lock()
            .expect("settings runtime sync test hook lock") = None;
    }
}
