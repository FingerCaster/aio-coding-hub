//! Global HTTP client module for gateway upstream requests.
//!
//! Provides a shared HTTP client with optional proxy support.
//! The client can be hot-reloaded at runtime when proxy settings change.

use crate::settings::{self, AppSettings, GatewayListenMode};
use crate::{gateway::listen, wsl};
use if_addrs::get_if_addrs;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, ClientBuilder, StatusCode, Url};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error as StdError;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Global HTTP client instance.
static GLOBAL_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();

/// Current proxy URL (for logging and status queries).
static CURRENT_PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();

/// Current raw proxy URL, kept private so scoped clients can preserve credentials.
static CURRENT_RAW_PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();

/// Current gateway self-loop context.
static GATEWAY_SELF_CONTEXT: OnceLock<RwLock<GatewaySelfCheckContext>> = OnceLock::new();

/// Short-lived DNS results used only by the Provider target self-loop guard.
static GATEWAY_TARGET_RESOLUTION_CACHE: OnceLock<
    RwLock<BTreeMap<String, GatewayTargetResolutionCacheEntry>>,
> = OnceLock::new();

/// DNS calls can outlive their caller-side timeout on platforms backed by
/// blocking `getaddrinfo`. Keep those detached calls independently bounded.
static GATEWAY_TARGET_RESOLUTION_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Default connection timeout for upstream requests.
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PROXY_TEST_TIMEOUT: Duration = Duration::from_secs(8);
const PROXY_TEST_URL: &str = "https://ifconfig.me/";
const PROXY_EXIT_IP_URL: &str = "https://ifconfig.me/ip";
const PROXY_EXIT_IP_RESPONSE_BODY_LIMIT: usize = 4 * 1024;
const GATEWAY_TARGET_RESOLUTION_TIMEOUT: Duration = Duration::from_millis(750);
const GATEWAY_TARGET_POSITIVE_CACHE_TTL: Duration = Duration::from_secs(30);
const GATEWAY_TARGET_NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(5);
const GATEWAY_TARGET_FAILURE_CACHE_TTL: Duration = Duration::from_secs(1);
const GATEWAY_TARGET_RESOLUTION_CACHE_LIMIT: usize = 128;
const GATEWAY_TARGET_RESOLUTION_ADDRESS_LIMIT: usize = 64;
const GATEWAY_TARGET_RESOLUTION_CONCURRENCY_LIMIT: usize = 8;

#[cfg(test)]
static TEST_PROXY_TEST_URL_OVERRIDE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
#[cfg(test)]
static TEST_PROXY_EXIT_IP_URL_OVERRIDE: OnceLock<RwLock<Option<String>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GatewaySelfCheckContext {
    gateway_port: u16,
    hosts: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GatewayTargetResolution {
    Gateway,
    Remote,
    Failed,
}

#[derive(Clone)]
struct GatewayTargetResolutionCacheEntry {
    context: GatewaySelfCheckContext,
    checked_at: Instant,
    resolution: GatewayTargetResolution,
    pinned_addresses: Option<Arc<Vec<SocketAddr>>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ValidatedGatewayTarget {
    pinned_resolution: Option<(String, Arc<Vec<SocketAddr>>)>,
}

impl ValidatedGatewayTarget {
    pub(crate) fn into_pinned_client(self) -> Result<Option<Client>, String> {
        self.pinned_resolution
            .map(|(host, addresses)| build_pinned_gateway_client(&host, addresses.as_slice()))
            .transpose()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatewayTargetValidationError {
    SelfLoop,
    ResolutionFailed,
}

impl GatewayTargetValidationError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::SelfLoop => "Provider URL points to the gateway itself (self-loop detected)",
            Self::ResolutionFailed => {
                "Provider URL could not be safely resolved for gateway self-loop validation"
            }
        }
    }
}

#[derive(Clone, Default)]
struct Ipv4FirstResolver;

impl Resolve for Ipv4FirstResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();

        Box::pin(async move {
            let mut addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            sort_socket_addrs_ipv4_first(&mut addrs);
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

fn sort_socket_addrs_ipv4_first(addrs: &mut [SocketAddr]) {
    addrs.sort_by_key(|addr| if addr.is_ipv4() { 0 } else { 1 });
}

/// Initialize the global HTTP client.
///
/// Should be called once at application startup.
///
/// # Arguments
/// * `proxy_url` - Proxy URL like `http://127.0.0.1:7890` or `socks5://127.0.0.1:1080`.
///   Pass None or empty string for direct connection.
pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let client = build_client(effective_url)?;

    if GLOBAL_CLIENT.set(RwLock::new(client.clone())).is_err() {
        tracing::warn!(
            "[HttpClient] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        return apply_proxy(proxy_url);
    }

    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(mask_url)));
    let _ = CURRENT_RAW_PROXY_URL.set(RwLock::new(effective_url.map(str::to_string)));

    tracing::info!(
        "[HttpClient] Initialized: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

pub(crate) fn runtime_self_check_context(
    port: u16,
    bind_host: &str,
    base_host: &str,
) -> GatewaySelfCheckContext {
    build_self_check_context(port, &[bind_host.to_string(), base_host.to_string()])
}

pub(crate) fn sync_runtime_context(port: u16, bind_host: &str, base_host: &str) {
    set_gateway_self_context(runtime_self_check_context(port, bind_host, base_host));
}

pub(crate) fn sync_from_settings(settings: &AppSettings) -> Result<(), String> {
    let context = self_check_context_from_settings(settings).map_err(|err| err.to_string())?;
    let proxy_url = effective_proxy_url(settings)?;
    validate_proxy_with_context(proxy_url.as_deref(), &context)?;
    set_gateway_self_context(context);
    apply_proxy(proxy_url.as_deref())
}

pub(crate) fn self_check_context_from_settings(
    settings: &AppSettings,
) -> crate::shared::error::AppResult<GatewaySelfCheckContext> {
    let preferred_port = settings
        .preferred_port
        .max(settings::DEFAULT_GATEWAY_PORT)
        .max(1024);

    let (bind_host, fixed_port) = match settings.gateway_listen_mode {
        GatewayListenMode::Localhost => ("127.0.0.1".to_string(), None),
        GatewayListenMode::Lan => ("0.0.0.0".to_string(), None),
        GatewayListenMode::WslAuto => (wsl::resolve_wsl_host(settings), None),
        GatewayListenMode::Custom => {
            let parsed =
                listen::parse_custom_listen_address(&settings.gateway_custom_listen_address)?;
            (parsed.host, parsed.port)
        }
    };

    let port = fixed_port.unwrap_or(preferred_port);
    let base_host = match settings.gateway_listen_mode {
        GatewayListenMode::Lan => "127.0.0.1".to_string(),
        GatewayListenMode::Custom if listen::is_wildcard_host(&bind_host) => {
            "127.0.0.1".to_string()
        }
        _ => bind_host.clone(),
    };

    Ok(build_self_check_context(port, &[bind_host, base_host]))
}

pub(crate) fn validate_proxy_for_settings(settings: &AppSettings) -> Result<(), String> {
    let context = self_check_context_from_settings(settings).map_err(|err| err.to_string())?;
    let proxy_url = effective_proxy_url(settings)?;
    validate_proxy_with_context(proxy_url.as_deref(), &context)
}

pub(crate) fn build_effective_proxy_url(
    proxy_url: Option<&str>,
    proxy_username: Option<&str>,
    proxy_password: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(raw_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let username = proxy_username
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let password = proxy_password.unwrap_or_default();
    let has_separate_credentials = username.is_some() || !password.is_empty();

    if !password.is_empty() && username.is_none() {
        return Err(
            "upstream_proxy_username cannot be empty when upstream_proxy_password is set"
                .to_string(),
        );
    }

    validate_proxy_url(raw_url)?;

    let mut parsed = Url::parse(raw_url)
        .map_err(|err| format!("Invalid proxy URL '{}': {err}", mask_url(raw_url)))?;
    let url_has_credentials = !parsed.username().is_empty() || parsed.password().is_some();

    if url_has_credentials && has_separate_credentials {
        return Err(
            "Proxy credentials must be specified either in proxy URL or username/password fields, not both"
                .to_string(),
        );
    }

    if !url_has_credentials {
        if let Some(username) = username {
            parsed
                .set_username(username)
                .map_err(|_| "Invalid upstream proxy username".to_string())?;
            parsed
                .set_password(Some(password))
                .map_err(|_| "Invalid upstream proxy password".to_string())?;
        }
    }

    Ok(Some(parsed.to_string()))
}

pub(crate) fn validate_proxy_with_context(
    proxy_url: Option<&str>,
    context: &GatewaySelfCheckContext,
) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());

    if let Some(url) = effective_url {
        if proxy_points_to_gateway_with_context(url, context) {
            return Err("Proxy URL points to the gateway itself (self-loop detected)".to_string());
        }
        validate_proxy_url(url)?;
    }

    Ok(())
}

pub(crate) async fn test_proxy_with_context(
    proxy_url: Option<&str>,
    context: &GatewaySelfCheckContext,
) -> Result<(), String> {
    let effective_url = proxy_url
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Proxy URL cannot be empty".to_string())?;

    validate_proxy_with_context(Some(effective_url), context)?;

    let client = build_client(Some(effective_url))?;
    let test_url = proxy_test_url();
    let response = client
        .get(&test_url)
        .timeout(PROXY_TEST_TIMEOUT)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return Err(format_proxy_request_error(
                "Proxy connectivity test failed",
                effective_url,
                &test_url,
                &err,
            )
            .await)
        }
    };

    if response.status() == StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        return Err(format!(
            "Proxy authentication failed for '{}': HTTP 407",
            mask_url(effective_url)
        ));
    }

    tracing::info!(
        status = %response.status(),
        proxy = %mask_url(effective_url),
        "proxy connectivity test completed"
    );

    Ok(())
}

pub(crate) async fn detect_proxy_exit_ip_with_context(
    proxy_url: Option<&str>,
    context: &GatewaySelfCheckContext,
) -> Result<String, String> {
    let effective_url = proxy_url
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Proxy URL cannot be empty".to_string())?;

    validate_proxy_with_context(Some(effective_url), context)?;

    let client = build_client(Some(effective_url))?;
    let detect_url = proxy_exit_ip_url();
    let response = client
        .get(&detect_url)
        .timeout(PROXY_TEST_TIMEOUT)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return Err(format_proxy_request_error(
                "Proxy exit IP detection failed",
                effective_url,
                &detect_url,
                &err,
            )
            .await)
        }
    };

    if response.status() == StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        return Err(format!(
            "Proxy authentication failed for '{}': HTTP 407",
            mask_url(effective_url)
        ));
    }

    if !response.status().is_success() {
        return Err(format!(
            "Proxy exit IP detection failed for '{}': HTTP {}",
            mask_url(effective_url),
            response.status()
        ));
    }

    let body = read_limited_probe_body(response, PROXY_EXIT_IP_RESPONSE_BODY_LIMIT)
        .await
        .map_err(|err| {
            format!(
                "Proxy exit IP detection failed for '{}': failed to read probe response: {}",
                mask_url(effective_url),
                err
            )
        })?;

    parse_exit_ip_response(&body.text).ok_or_else(|| {
        format!(
            "Proxy exit IP detection failed for '{}': probe response is not a valid IP address ({})",
            mask_url(effective_url),
            summarize_probe_body(&body)
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LimitedProbeBody {
    text: String,
    truncated: bool,
    limit: usize,
}

fn append_limited_probe_body_chunk(bytes: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    let remaining = limit.saturating_sub(bytes.len());
    if remaining == 0 {
        return !chunk.is_empty();
    }

    let keep = chunk.len().min(remaining);
    bytes.extend_from_slice(&chunk[..keep]);
    keep < chunk.len()
}

async fn read_limited_probe_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<LimitedProbeBody, String> {
    let content_length = response.content_length();
    let mut truncated = content_length.is_some_and(|len| len > limit as u64);
    let capacity = content_length
        .and_then(|len| usize::try_from(len).ok())
        .unwrap_or_default()
        .min(limit);
    let mut bytes = Vec::with_capacity(capacity);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format_error_chain(&err))?
    {
        if append_limited_probe_body_chunk(&mut bytes, chunk.as_ref(), limit) {
            truncated = true;
            break;
        }
        if bytes.len() >= limit && content_length != Some(limit as u64) {
            truncated = true;
            break;
        }
    }

    Ok(LimitedProbeBody {
        text: String::from_utf8_lossy(&bytes).to_string(),
        truncated,
        limit,
    })
}

fn format_error_chain(err: &(dyn StdError + 'static)) -> String {
    let mut parts = vec![err.to_string()];
    let mut current = err.source();

    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }

    parts.join(" | caused by: ")
}

fn proxy_uses_socks5_local_dns(proxy_url: &str) -> bool {
    Url::parse(proxy_url)
        .ok()
        .is_some_and(|parsed| parsed.scheme() == "socks5")
}

fn proxy_test_url() -> String {
    #[cfg(test)]
    if let Some(lock) = TEST_PROXY_TEST_URL_OVERRIDE.get() {
        if let Ok(url) = lock.read() {
            if let Some(override_url) = url.clone() {
                return override_url;
            }
        }
    }

    PROXY_TEST_URL.to_string()
}

fn proxy_exit_ip_url() -> String {
    #[cfg(test)]
    if let Some(lock) = TEST_PROXY_EXIT_IP_URL_OVERRIDE.get() {
        if let Ok(url) = lock.read() {
            if let Some(override_url) = url.clone() {
                return override_url;
            }
        }
    }

    PROXY_EXIT_IP_URL.to_string()
}

#[cfg(test)]
fn set_proxy_test_url_for_tests(url: Option<&str>) {
    let lock = TEST_PROXY_TEST_URL_OVERRIDE.get_or_init(|| RwLock::new(None));
    let mut current = lock.write().expect("proxy test url override lock");
    *current = url.map(str::to_string);
}

#[cfg(test)]
fn set_proxy_exit_ip_url_for_tests(url: Option<&str>) {
    let lock = TEST_PROXY_EXIT_IP_URL_OVERRIDE.get_or_init(|| RwLock::new(None));
    let mut current = lock.write().expect("proxy exit ip url override lock");
    *current = url.map(str::to_string);
}

fn proxy_test_url_has_hostname(test_url: &str) -> bool {
    Url::parse(test_url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .and_then(|host| normalize_host_token(&host))
        .is_some_and(|host| host.parse::<IpAddr>().is_err())
}

fn to_socks5h_url(proxy_url: &str) -> Option<String> {
    let mut parsed = Url::parse(proxy_url).ok()?;
    if parsed.scheme() != "socks5" {
        return None;
    }
    parsed.set_scheme("socks5h").ok()?;
    Some(parsed.to_string())
}

async fn probe_socks5h_fallback(proxy_url: &str, test_url: &str) -> bool {
    let Some(fallback_proxy_url) = to_socks5h_url(proxy_url) else {
        return false;
    };

    let Ok(client) = build_client(Some(&fallback_proxy_url)) else {
        return false;
    };

    client
        .get(test_url)
        .timeout(PROXY_TEST_TIMEOUT)
        .send()
        .await
        .is_ok()
}

fn is_socks5_handshake_eof(err: &reqwest::Error) -> bool {
    format_error_chain(err).contains("SOCKS error: io error during SOCKS handshake")
}

fn build_socks5_local_dns_hint(proxy_url: &str, test_url: &str) -> String {
    let suggested = to_socks5h_url(proxy_url).unwrap_or_else(|| proxy_url.to_string());
    format!(
        "Local-DNS SOCKS5 handshake failed for hostname test target '{}'. This proxy works with proxy-side DNS on the current reqwest stack. Try '{}' instead.",
        test_url,
        suggested
    )
}

fn default_proxy_request_error(operation: &str, proxy_url: &str, err: &reqwest::Error) -> String {
    format!(
        "{} for '{}': {}",
        operation,
        mask_url(proxy_url),
        format_error_chain(err)
    )
}

async fn format_proxy_request_error(
    operation: &str,
    proxy_url: &str,
    test_url: &str,
    err: &reqwest::Error,
) -> String {
    if proxy_uses_socks5_local_dns(proxy_url)
        && proxy_test_url_has_hostname(test_url)
        && is_socks5_handshake_eof(err)
        && probe_socks5h_fallback(proxy_url, test_url).await
    {
        return format!(
            "{} {}",
            default_proxy_request_error(operation, proxy_url, err),
            build_socks5_local_dns_hint(proxy_url, test_url)
        );
    }

    default_proxy_request_error(operation, proxy_url, err)
}

fn parse_exit_ip_response(body: &str) -> Option<String> {
    let candidate = body.trim();
    if candidate.parse::<IpAddr>().is_ok() {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn summarize_probe_body(body: &LimitedProbeBody) -> String {
    let normalized = body.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut snippet = if normalized.is_empty() {
        "<empty>".to_string()
    } else {
        normalized.chars().take(80).collect::<String>()
    };
    if normalized.chars().count() > 80 {
        snippet.push_str("...");
    }
    if body.truncated {
        snippet.push_str(&format!("; truncated after {} bytes", body.limit));
    }
    snippet
}

/// Validate proxy URL format without building a client.
fn validate_proxy_url(url: &str) -> Result<(), String> {
    let parsed =
        Url::parse(url).map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;

    let scheme = parsed.scheme();
    if !["http", "https", "socks5", "socks5h"].contains(&scheme) {
        return Err(format!(
            "Invalid proxy scheme '{}' in URL '{}'. Supported: http, https, socks5, socks5h",
            scheme,
            mask_url(url)
        ));
    }

    Ok(())
}

/// Apply proxy configuration (assumes already validated).
///
/// Should be called after validate_proxy succeeds.
pub fn apply_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.filter(|s| !s.trim().is_empty());
    let new_client = build_client(effective_url)?;

    if let Some(lock) = GLOBAL_CLIENT.get() {
        let mut client = lock.write().unwrap_or_else(|poisoned| {
            tracing::warn!("[HttpClient] Recovered poisoned client lock");
            poisoned.into_inner()
        });
        *client = new_client;
    } else {
        return init(proxy_url);
    }

    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().unwrap_or_else(|poisoned| {
            tracing::warn!("[HttpClient] Recovered poisoned proxy URL lock");
            poisoned.into_inner()
        });
        *url = effective_url.map(mask_url);
    }
    if let Some(lock) = CURRENT_RAW_PROXY_URL.get() {
        let mut url = lock.write().unwrap_or_else(|poisoned| {
            tracing::warn!("[HttpClient] Recovered poisoned raw proxy URL lock");
            poisoned.into_inner()
        });
        *url = effective_url.map(str::to_string);
    } else {
        let _ = CURRENT_RAW_PROXY_URL.set(RwLock::new(effective_url.map(str::to_string)));
    }
    clear_gateway_target_resolution_cache();

    tracing::info!(
        "[HttpClient] Proxy applied: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// Get the global HTTP client.
///
/// Returns the client configured with proxy (if set), otherwise a direct-connection client.
pub fn get() -> Client {
    GLOBAL_CLIENT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|c| c.clone())
        .unwrap_or_else(|| {
            tracing::warn!("[HttpClient] Client not initialized, using fallback");
            build_client(None).unwrap_or_default()
        })
}

/// Get the current proxy URL.
///
/// Returns None if direct connection is configured.
pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

/// Build a request-scoped client while preserving the gateway's current proxy policy.
pub(crate) fn build_client_with_current_proxy(
    user_agent: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    redirect_policy: reqwest::redirect::Policy,
) -> Result<Client, String> {
    let proxy_url = CURRENT_RAW_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone());
    let builder = Client::builder()
        .user_agent(user_agent)
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .redirect(redirect_policy)
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60));
    configure_proxy(builder, proxy_url.as_deref())?
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))
}

/// Check if proxy is currently enabled.
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

fn effective_proxy_url(settings: &AppSettings) -> Result<Option<String>, String> {
    if !settings.upstream_proxy_enabled {
        return Ok(None);
    }

    build_effective_proxy_url(
        Some(settings.upstream_proxy_url.as_str()),
        Some(settings.upstream_proxy_username.as_str()),
        Some(settings.upstream_proxy_password.as_str()),
    )
}

fn set_gateway_self_context(context: GatewaySelfCheckContext) {
    if let Some(lock) = GATEWAY_SELF_CONTEXT.get() {
        let mut current = lock.write().unwrap_or_else(|poisoned| {
            tracing::warn!("[HttpClient] Recovered poisoned self-context lock");
            poisoned.into_inner()
        });
        *current = context;
    } else {
        let _ = GATEWAY_SELF_CONTEXT.set(RwLock::new(context));
    }

    clear_gateway_target_resolution_cache();
}

fn current_self_context() -> GatewaySelfCheckContext {
    GATEWAY_SELF_CONTEXT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|ctx| ctx.clone())
        .unwrap_or_else(default_self_check_context)
}

fn gateway_target_resolution_cache(
) -> &'static RwLock<BTreeMap<String, GatewayTargetResolutionCacheEntry>> {
    GATEWAY_TARGET_RESOLUTION_CACHE.get_or_init(|| RwLock::new(BTreeMap::new()))
}

fn gateway_target_resolution_limiter() -> Arc<Semaphore> {
    GATEWAY_TARGET_RESOLUTION_LIMITER
        .get_or_init(|| Arc::new(Semaphore::new(GATEWAY_TARGET_RESOLUTION_CONCURRENCY_LIMIT)))
        .clone()
}

fn clear_gateway_target_resolution_cache() {
    if let Some(cache) = GATEWAY_TARGET_RESOLUTION_CACHE.get() {
        cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

fn gateway_target_resolution_cache_ttl(resolution: GatewayTargetResolution) -> Duration {
    match resolution {
        GatewayTargetResolution::Gateway => GATEWAY_TARGET_POSITIVE_CACHE_TTL,
        GatewayTargetResolution::Remote => GATEWAY_TARGET_NEGATIVE_CACHE_TTL,
        GatewayTargetResolution::Failed => GATEWAY_TARGET_FAILURE_CACHE_TTL,
    }
}

fn cached_gateway_target_resolution(
    host: &str,
    context: &GatewaySelfCheckContext,
) -> Option<GatewayTargetResolutionCacheEntry> {
    let cache = gateway_target_resolution_cache()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.get(host).and_then(|entry| {
        let ttl = gateway_target_resolution_cache_ttl(entry.resolution);
        (entry.context == *context && entry.checked_at.elapsed() <= ttl).then_some(entry.clone())
    })
}

fn cache_gateway_target_resolution(
    host: String,
    context: &GatewaySelfCheckContext,
    resolution: GatewayTargetResolution,
    pinned_addresses: Option<Arc<Vec<SocketAddr>>>,
) {
    let mut cache = gateway_target_resolution_cache()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|_, entry| {
        entry.checked_at.elapsed() <= gateway_target_resolution_cache_ttl(entry.resolution)
    });
    if cache.len() >= GATEWAY_TARGET_RESOLUTION_CACHE_LIMIT && !cache.contains_key(&host) {
        if let Some(first_key) = cache.keys().next().cloned() {
            cache.remove(&first_key);
        }
    }
    cache.insert(
        host,
        GatewayTargetResolutionCacheEntry {
            context: context.clone(),
            checked_at: Instant::now(),
            resolution,
            pinned_addresses,
        },
    );
}

fn default_self_check_context() -> GatewaySelfCheckContext {
    build_self_check_context(
        settings::DEFAULT_GATEWAY_PORT,
        &["127.0.0.1".to_string(), "localhost".to_string()],
    )
}

fn build_self_check_context(port: u16, configured_hosts: &[String]) -> GatewaySelfCheckContext {
    let local_hosts = collect_local_host_tokens();
    let mut hosts = BTreeSet::new();

    for host in configured_hosts {
        extend_self_hosts(&mut hosts, host, &local_hosts);
    }

    if hosts.is_empty() {
        hosts.extend(local_hosts.loopback.iter().cloned());
    }

    GatewaySelfCheckContext {
        gateway_port: port,
        hosts,
    }
}

#[derive(Default)]
struct LocalHostTokens {
    all: BTreeSet<String>,
    loopback: BTreeSet<String>,
}

fn collect_local_host_tokens() -> LocalHostTokens {
    let mut tokens = LocalHostTokens::default();

    for host in ["localhost", "127.0.0.1", "::1"] {
        if let Some(token) = normalize_host_token(host) {
            tokens.all.insert(token.clone());
            tokens.loopback.insert(token);
        }
    }

    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            if let Some(token) = normalize_host_token(&iface.ip().to_string()) {
                let is_loopback = iface.ip().is_loopback();
                tokens.all.insert(token.clone());
                if is_loopback {
                    tokens.loopback.insert(token);
                }
            }
        }
    }

    if let Ok(value) = hostname::get() {
        if let Some(hostname) = value.to_str().and_then(normalize_host_token) {
            tokens.all.insert(hostname);
        }
    }

    for key in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = env::var(key) {
            if let Some(token) = normalize_host_token(&value) {
                tokens.all.insert(token);
            }
        }
    }

    tokens
}

fn extend_self_hosts(targets: &mut BTreeSet<String>, host: &str, local_hosts: &LocalHostTokens) {
    let Some(normalized) = normalize_host_token(host) else {
        return;
    };

    if matches!(normalized.as_str(), "0.0.0.0" | "::") {
        targets.insert(normalized);
        targets.extend(local_hosts.all.iter().cloned());
        return;
    }

    targets.extend(resolve_host_tokens(&normalized, local_hosts));
}

fn resolve_host_tokens(host: &str, local_hosts: &LocalHostTokens) -> BTreeSet<String> {
    let mut resolved = BTreeSet::new();

    if let Some(token) = normalize_host_token(host) {
        resolved.insert(token.clone());
        if local_hosts.loopback.contains(&token) {
            resolved.extend(local_hosts.loopback.iter().cloned());
        }
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if let Some(token) = normalize_host_token(&ip.to_string()) {
            resolved.insert(token);
        }
        return resolved;
    }

    if let Ok(addrs) = (host, 0u16).to_socket_addrs() {
        for addr in addrs {
            if let Some(token) = normalize_host_token(&addr.ip().to_string()) {
                if local_hosts.all.contains(&token) {
                    resolved.insert(token);
                }
            }
        }
    }

    resolved
}

fn normalize_host_token(host: &str) -> Option<String> {
    let trimmed = host.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(match ip {
            IpAddr::V6(ipv6) => ipv6
                .to_ipv4_mapped()
                .map(|ipv4| ipv4.to_string())
                .unwrap_or_else(|| ipv6.to_string()),
            IpAddr::V4(ipv4) => ipv4.to_string(),
        });
    }

    Some(trimmed.to_ascii_lowercase())
}

/// Build HTTP client with optional proxy.
fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    configure_proxy(gateway_client_builder(), proxy_url)?
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

fn gateway_client_builder() -> ClientBuilder {
    Client::builder()
        .user_agent(format!(
            "aio-coding-hub-gateway/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(UPSTREAM_CONNECT_TIMEOUT)
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
}

fn build_pinned_gateway_client(host: &str, addrs: &[SocketAddr]) -> Result<Client, String> {
    let proxy_url = CURRENT_RAW_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone());
    configure_proxy(
        gateway_client_builder().resolve_to_addrs(host, addrs),
        proxy_url.as_deref(),
    )?
    .build()
    .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

fn configure_proxy(
    mut builder: ClientBuilder,
    proxy_url: Option<&str>,
) -> Result<ClientBuilder, String> {
    if let Some(url) = proxy_url {
        validate_proxy_url(url)?;
        if proxy_uses_socks5_local_dns(url) {
            builder = builder.dns_resolver2(Ipv4FirstResolver);
        }
        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| format!("Invalid proxy URL '{}': {}", mask_url(url), e))?;
        builder = builder.proxy(proxy);
        tracing::debug!("[HttpClient] Proxy configured: {}", mask_url(url));
    } else if system_proxy_points_to_gateway() {
        builder = builder.no_proxy();
        tracing::warn!("[HttpClient] System proxy points to gateway, bypassing to avoid recursion");
    } else {
        tracing::debug!("[HttpClient] Following system proxy (no explicit proxy configured)");
    }

    Ok(builder)
}

/// Check if system proxy environment variables point to the gateway.
fn system_proxy_points_to_gateway() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    let context = current_self_context();
    KEYS.iter()
        .filter_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| proxy_points_to_gateway_with_context(&value, &context))
}

pub(crate) async fn validate_gateway_target(
    url: &Url,
) -> Result<ValidatedGatewayTarget, GatewayTargetValidationError> {
    let context = current_self_context();
    if url_points_to_gateway_with_context(url, &context) {
        return Err(GatewayTargetValidationError::SelfLoop);
    }

    validate_gateway_target_resolution(url, &context).await
}

fn url_points_to_gateway_with_context(url: &Url, context: &GatewaySelfCheckContext) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let Some(normalized) = normalize_host_token(host) else {
        return false;
    };

    url.port_or_known_default() == Some(context.gateway_port) && context.hosts.contains(&normalized)
}

async fn validate_gateway_target_resolution(
    url: &Url,
    context: &GatewaySelfCheckContext,
) -> Result<ValidatedGatewayTarget, GatewayTargetValidationError> {
    if url.port_or_known_default() != Some(context.gateway_port) {
        return Ok(ValidatedGatewayTarget::default());
    }

    let host = url
        .host_str()
        .and_then(normalize_host_token)
        .ok_or(GatewayTargetValidationError::ResolutionFailed)?;
    if host.parse::<IpAddr>().is_ok() {
        return Ok(ValidatedGatewayTarget::default());
    }
    if let Some(cached) = cached_gateway_target_resolution(&host, context) {
        return match cached.resolution {
            GatewayTargetResolution::Gateway => Err(GatewayTargetValidationError::SelfLoop),
            GatewayTargetResolution::Remote => cached
                .pinned_addresses
                .map(|addresses| ValidatedGatewayTarget {
                    pinned_resolution: Some((host, addresses)),
                })
                .ok_or(GatewayTargetValidationError::ResolutionFailed),
            GatewayTargetResolution::Failed => Err(GatewayTargetValidationError::ResolutionFailed),
        };
    }

    let addresses = match bounded_gateway_target_lookup(&host).await {
        Ok(addresses) => addresses,
        Err(error) => {
            cache_gateway_target_resolution(host, context, GatewayTargetResolution::Failed, None);
            return Err(error);
        }
    };
    let addresses = match validate_gateway_target_addresses(addresses, context) {
        Ok(addresses) => addresses,
        Err(error) => {
            let resolution = match error {
                GatewayTargetValidationError::SelfLoop => GatewayTargetResolution::Gateway,
                GatewayTargetValidationError::ResolutionFailed => GatewayTargetResolution::Failed,
            };
            cache_gateway_target_resolution(host, context, resolution, None);
            return Err(error);
        }
    };
    let addresses = Arc::new(addresses);
    cache_gateway_target_resolution(
        host.clone(),
        context,
        GatewayTargetResolution::Remote,
        Some(addresses.clone()),
    );

    Ok(ValidatedGatewayTarget {
        pinned_resolution: Some((host, addresses)),
    })
}

fn validate_gateway_target_addresses(
    addresses: Vec<SocketAddr>,
    context: &GatewaySelfCheckContext,
) -> Result<Vec<SocketAddr>, GatewayTargetValidationError> {
    if addresses.is_empty() || addresses.len() > GATEWAY_TARGET_RESOLUTION_ADDRESS_LIMIT {
        return Err(GatewayTargetValidationError::ResolutionFailed);
    }

    let mut unique_addresses = Vec::with_capacity(addresses.len());
    for address in addresses {
        if normalize_host_token(&address.ip().to_string())
            .is_some_and(|resolved| context.hosts.contains(&resolved))
        {
            return Err(GatewayTargetValidationError::SelfLoop);
        }
        if !unique_addresses.contains(&address) {
            unique_addresses.push(address);
        }
    }

    Ok(unique_addresses)
}

async fn bounded_gateway_target_lookup(
    host: &str,
) -> Result<Vec<SocketAddr>, GatewayTargetValidationError> {
    let host = host.to_string();
    bounded_gateway_target_lookup_with(
        gateway_target_resolution_limiter(),
        GATEWAY_TARGET_RESOLUTION_TIMEOUT,
        move || {
            (host.as_str(), 0u16).to_socket_addrs().map(|addresses| {
                addresses
                    .take(GATEWAY_TARGET_RESOLUTION_ADDRESS_LIMIT + 1)
                    .collect()
            })
        },
    )
    .await
}

async fn bounded_gateway_target_lookup_with<T, F>(
    limiter: Arc<Semaphore>,
    timeout: Duration,
    lookup: F,
) -> Result<T, GatewayTargetValidationError>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    let deadline = Instant::now() + timeout;
    let permit = tokio::time::timeout(timeout, limiter.acquire_owned())
        .await
        .map_err(|_| GatewayTargetValidationError::ResolutionFailed)?
        .map_err(|_| GatewayTargetValidationError::ResolutionFailed)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(GatewayTargetValidationError::ResolutionFailed);
    }

    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        lookup()
    });

    match tokio::time::timeout(remaining, task).await {
        Ok(Ok(Ok(result))) => Ok(result),
        Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => Err(GatewayTargetValidationError::ResolutionFailed),
    }
}

fn proxy_points_to_gateway_with_context(value: &str, context: &GatewaySelfCheckContext) -> bool {
    if let Ok(parsed) = Url::parse(value) {
        return url_points_to_gateway_with_context(&parsed, context);
    }

    let with_scheme = format!("http://{value}");
    if let Ok(parsed) = Url::parse(&with_scheme) {
        return url_points_to_gateway_with_context(&parsed, context);
    }

    false
}

/// Mask sensitive information in URL (for logging).
pub fn mask_url(url: &str) -> String {
    if let Ok(parsed) = Url::parse(url) {
        let host = parsed.host_str().unwrap_or("?");
        match parsed.port() {
            Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
            None => format!("{}://{}", parsed.scheme(), host),
        }
    } else if url.len() > 20 {
        format!("{}...", &url[..20])
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, MutexGuard};
    use std::thread;

    fn settings_with_listen_mode(listen_mode: GatewayListenMode) -> AppSettings {
        AppSettings {
            gateway_listen_mode: listen_mode,
            preferred_port: 37123,
            ..AppSettings::default()
        }
    }

    struct ProxyTestUrlGuard<'a> {
        _guard: MutexGuard<'a, ()>,
    }

    impl Drop for ProxyTestUrlGuard<'_> {
        fn drop(&mut self) {
            set_proxy_test_url_for_tests(None);
            set_proxy_exit_ip_url_for_tests(None);
        }
    }

    fn use_http_proxy_test_url() -> ProxyTestUrlGuard<'static> {
        let guard = crate::test_support::test_env_lock();
        set_proxy_test_url_for_tests(Some("http://example.com/"));
        ProxyTestUrlGuard { _guard: guard }
    }

    fn use_http_proxy_exit_ip_test_url() -> ProxyTestUrlGuard<'static> {
        let guard = crate::test_support::test_env_lock();
        set_proxy_exit_ip_url_for_tests(Some("http://ifconfig.me/ip"));
        ProxyTestUrlGuard { _guard: guard }
    }

    fn spawn_http_proxy_server_with_response(
        response: Vec<u8>,
    ) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind proxy listener");
        let addr = listener.local_addr().expect("proxy listener addr");
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy request");
            let mut buf = [0_u8; 4096];
            let size = stream.read(&mut buf).expect("read proxy request");
            let request = String::from_utf8_lossy(&buf[..size]).to_string();
            tx.send(request).expect("send proxy request");
            stream.write_all(&response).expect("write proxy response");
        });

        (format!("http://127.0.0.1:{}", addr.port()), rx)
    }

    fn spawn_http_proxy_server() -> (String, mpsc::Receiver<String>) {
        spawn_http_proxy_server_with_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        )
    }

    fn spawn_http_origin_server() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind origin listener");
        let addr = listener.local_addr().expect("origin listener addr");
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept origin request");
            let mut buf = [0_u8; 4096];
            let size = stream.read(&mut buf).expect("read origin request");
            let request = String::from_utf8_lossy(&buf[..size]).to_string();
            tx.send(request).expect("send origin request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write origin response");
        });

        (format!("http://127.0.0.1:{}/", addr.port()), rx)
    }

    #[test]
    fn test_mask_url() {
        assert_eq!(mask_url("http://127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(
            mask_url("http://user:pass@127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            mask_url("socks5://admin:secret@proxy.example.com:1080"),
            "socks5://proxy.example.com:1080"
        );
        assert_eq!(
            mask_url("http://proxy.example.com"),
            "http://proxy.example.com"
        );
        assert_eq!(
            mask_url("https://user:pass@proxy.example.com"),
            "https://proxy.example.com"
        );
    }

    #[test]
    fn test_sort_socket_addrs_ipv4_first_prefers_ipv4_entries() {
        let mut addrs = vec![
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 443),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443),
        ];

        sort_socket_addrs_ipv4_first(&mut addrs);

        assert!(addrs[0].is_ipv4());
        assert!(addrs[1].is_ipv6());
    }

    #[test]
    fn test_build_client_direct() {
        let result = build_client(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let result = build_client(Some("http://127.0.0.1:7890"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_https_proxy() {
        let result = build_client(Some("https://127.0.0.1:8443"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let result = build_client(Some("socks5://127.0.0.1:1080"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5h_proxy() {
        let result = build_client(Some("socks5h://127.0.0.1:1080"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_scheme() {
        let result = build_client(Some("invalid-scheme://127.0.0.1:7890"));
        assert!(result.is_err(), "Should reject invalid proxy scheme");
    }

    #[test]
    fn test_proxy_points_to_gateway_loopback_and_custom_hosts() {
        let wildcard_context =
            build_self_check_context(37123, &["0.0.0.0".to_string(), "127.0.0.1".to_string()]);
        let custom_context = build_self_check_context(
            37123,
            &["192.168.1.10".to_string(), "devbox.internal".to_string()],
        );
        let http_context =
            build_self_check_context(80, &["127.0.0.1".to_string(), "localhost".to_string()]);
        let https_context =
            build_self_check_context(443, &["127.0.0.1".to_string(), "localhost".to_string()]);

        assert!(proxy_points_to_gateway_with_context(
            "http://127.0.0.1:37123",
            &wildcard_context
        ));
        assert!(proxy_points_to_gateway_with_context(
            "socks5://localhost:37123",
            &wildcard_context
        ));
        assert!(proxy_points_to_gateway_with_context(
            "http://192.168.1.10:37123",
            &custom_context
        ));
        assert!(proxy_points_to_gateway_with_context(
            "https://devbox.internal:37123",
            &custom_context
        ));
        assert!(proxy_points_to_gateway_with_context(
            "http://localhost/v1",
            &http_context
        ));
        assert!(proxy_points_to_gateway_with_context(
            "https://localhost/v1",
            &https_context
        ));

        assert!(!proxy_points_to_gateway_with_context(
            "http://127.0.0.1:7890",
            &wildcard_context
        ));
        assert!(!proxy_points_to_gateway_with_context(
            "socks5://localhost:1080",
            &wildcard_context
        ));
    }

    #[test]
    fn test_gateway_target_detection_covers_ipv4_ipv6_and_remote_addresses() {
        let loopback_context =
            build_self_check_context(37123, &["127.0.0.1".to_string(), "localhost".to_string()]);
        let ipv6_context = build_self_check_context(37123, &["::1".to_string()]);
        let lan_context = build_self_check_context(
            37123,
            &["192.168.1.10".to_string(), "devbox.internal".to_string()],
        );

        for value in [
            "http://127.0.0.1:37123/v1/messages",
            "http://LOCALHOST:37123/v1/responses?stream=true",
        ] {
            let url = Url::parse(value).expect("loopback URL");
            assert!(url_points_to_gateway_with_context(&url, &loopback_context));
        }

        let ipv6 = Url::parse("http://[::1]:37123/v1").expect("IPv6 URL");
        assert!(url_points_to_gateway_with_context(&ipv6, &ipv6_context));

        let mapped_ipv6 =
            Url::parse("http://[::ffff:127.0.0.1]:37123/v1").expect("mapped IPv6 URL");
        assert!(url_points_to_gateway_with_context(
            &mapped_ipv6,
            &loopback_context
        ));

        let wildcard_v4_context = build_self_check_context(37123, &["0.0.0.0".to_string()]);
        let wildcard_v6_context = build_self_check_context(37123, &["::".to_string()]);
        let wildcard_v4 = Url::parse("http://0.0.0.0:37123/v1").expect("IPv4 wildcard URL");
        let wildcard_v6 = Url::parse("http://[::]:37123/v1").expect("IPv6 wildcard URL");
        assert!(url_points_to_gateway_with_context(
            &wildcard_v4,
            &wildcard_v4_context
        ));
        assert!(url_points_to_gateway_with_context(
            &wildcard_v6,
            &wildcard_v6_context
        ));

        let lan = Url::parse("http://192.168.1.10:37123/v1").expect("LAN URL");
        assert!(url_points_to_gateway_with_context(&lan, &lan_context));

        let different_port = Url::parse("http://localhost:37124/v1").expect("different port");
        let remote = Url::parse("https://192.0.2.1:37123/v1").expect("remote URL");
        assert!(!url_points_to_gateway_with_context(
            &different_port,
            &loopback_context
        ));
        assert!(!url_points_to_gateway_with_context(
            &remote,
            &loopback_context
        ));
    }

    #[test]
    fn test_custom_hostname_expands_to_local_interface_tokens() {
        let local_hosts = LocalHostTokens {
            all: BTreeSet::from(["127.0.0.1".to_string(), "::1".to_string()]),
            loopback: BTreeSet::from(["127.0.0.1".to_string(), "::1".to_string()]),
        };
        let mut targets = BTreeSet::new();

        extend_self_hosts(&mut targets, "localhost", &local_hosts);

        assert!(targets.contains("localhost"));
        assert!(targets.contains("127.0.0.1") || targets.contains("::1"));
    }

    #[test]
    fn test_gateway_target_detection_resolves_and_caches_local_aliases() {
        let _guard = crate::test_support::test_env_lock();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            clear_gateway_target_resolution_cache();
            let context = GatewaySelfCheckContext {
                gateway_port: 37123,
                hosts: BTreeSet::from(["127.0.0.1".to_string(), "::1".to_string()]),
            };
            let alias = Url::parse("http://localhost:37123/v1").expect("alias URL");
            let different_port =
                Url::parse("http://localhost:37124/v1").expect("different-port alias URL");

            assert_eq!(
                validate_gateway_target_resolution(&alias, &context)
                    .await
                    .expect_err("local alias should be rejected"),
                GatewayTargetValidationError::SelfLoop
            );
            assert_eq!(
                cached_gateway_target_resolution("localhost", &context)
                    .map(|entry| entry.resolution),
                Some(GatewayTargetResolution::Gateway)
            );
            assert!(
                validate_gateway_target_resolution(&different_port, &context)
                    .await
                    .expect("different port should remain valid")
                    .into_pinned_client()
                    .expect("different-port target should not need a scoped client")
                    .is_none()
            );
        });
    }

    #[test]
    fn test_gateway_target_address_validation_rejects_any_local_or_ambiguous_answer() {
        let context = GatewaySelfCheckContext {
            gateway_port: 37123,
            hosts: BTreeSet::from(["127.0.0.1".to_string(), "::1".to_string()]),
        };
        let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 20)), 0);
        let local = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let mapped_local = SocketAddr::new(
            IpAddr::V6("::ffff:127.0.0.1".parse().expect("mapped IPv6")),
            0,
        );

        assert_eq!(
            validate_gateway_target_addresses(vec![remote, local], &context),
            Err(GatewayTargetValidationError::SelfLoop)
        );
        assert_eq!(
            validate_gateway_target_addresses(vec![mapped_local], &context),
            Err(GatewayTargetValidationError::SelfLoop)
        );
        assert_eq!(
            validate_gateway_target_addresses(Vec::new(), &context),
            Err(GatewayTargetValidationError::ResolutionFailed)
        );
        assert_eq!(
            validate_gateway_target_addresses(
                vec![remote; GATEWAY_TARGET_RESOLUTION_ADDRESS_LIMIT + 1],
                &context,
            ),
            Err(GatewayTargetValidationError::ResolutionFailed)
        );
        assert_eq!(
            validate_gateway_target_addresses(vec![remote, remote], &context),
            Ok(vec![remote])
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_gateway_target_lookup_timeout_holds_its_concurrency_permit() {
        let limiter = Arc::new(Semaphore::new(1));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first = bounded_gateway_target_lookup_with(
            limiter.clone(),
            Duration::from_millis(100),
            move || {
                started_tx.send(()).expect("signal resolver start");
                release_rx.recv().expect("release resolver");
                Ok::<_, std::io::Error>(())
            },
        )
        .await;
        assert_eq!(first, Err(GatewayTargetValidationError::ResolutionFailed));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("resolver should have started");

        let second_ran = Arc::new(AtomicBool::new(false));
        let second_ran_in_lookup = second_ran.clone();
        let second = bounded_gateway_target_lookup_with(
            limiter.clone(),
            Duration::from_millis(50),
            move || {
                second_ran_in_lookup.store(true, Ordering::SeqCst);
                Ok::<_, std::io::Error>(())
            },
        )
        .await;
        assert_eq!(second, Err(GatewayTargetValidationError::ResolutionFailed));
        assert!(!second_ran.load(Ordering::SeqCst));

        release_tx.send(()).expect("release timed-out resolver");
        bounded_gateway_target_lookup_with(limiter, Duration::from_secs(1), || {
            Ok::<_, std::io::Error>(())
        })
        .await
        .expect("permit should be released when detached resolver finishes");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_pinned_gateway_client_uses_validated_addresses_for_transport() {
        let (origin_url, request_rx) = spawn_http_origin_server();
        let origin = Url::parse(&origin_url).expect("origin URL");
        let origin_port = origin.port().expect("origin port");

        let _guard = crate::test_support::test_env_lock();
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::remove_var(key);
        }
        std::env::set_var("NO_PROXY", "*");
        std::env::set_var("no_proxy", "*");
        apply_proxy(None).expect("use direct proxy policy");
        let pinned = build_pinned_gateway_client(
            "provider-rebind.invalid",
            &[SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)],
        )
        .expect("build pinned client");
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
        drop(_guard);

        let response = pinned
            .get(format!(
                "http://provider-rebind.invalid:{origin_port}/pinned"
            ))
            .send()
            .await
            .expect("pinned request should not use system DNS");
        assert_eq!(response.status(), StatusCode::OK);
        let request = request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("pinned origin request");
        assert!(request.starts_with("GET /pinned HTTP/1.1"));
        assert!(
            request.contains(&format!("Host: provider-rebind.invalid:{origin_port}"))
                || request.contains(&format!("host: provider-rebind.invalid:{origin_port}"))
        );
    }

    #[test]
    fn test_gateway_target_resolution_cache_is_bounded() {
        let _guard = crate::test_support::test_env_lock();
        clear_gateway_target_resolution_cache();
        let context = build_self_check_context(37123, &["127.0.0.1".to_string()]);

        for index in 0..=GATEWAY_TARGET_RESOLUTION_CACHE_LIMIT {
            cache_gateway_target_resolution(
                format!("remote-{index}.example"),
                &context,
                GatewayTargetResolution::Remote,
                None,
            );
        }

        assert!(
            gateway_target_resolution_cache()
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len()
                <= GATEWAY_TARGET_RESOLUTION_CACHE_LIMIT
        );
    }

    #[test]
    fn test_system_proxy_points_to_gateway() {
        let _guard = crate::test_support::test_env_lock();

        set_gateway_self_context(build_self_check_context(
            37123,
            &["127.0.0.1".to_string(), "localhost".to_string()],
        ));

        let keys = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ];

        for key in &keys {
            std::env::remove_var(key);
        }

        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:37123");
        assert!(system_proxy_points_to_gateway());

        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(!system_proxy_points_to_gateway());

        for key in &keys {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn test_validate_proxy_for_settings_rejects_lan_self_loop() {
        let mut settings = settings_with_listen_mode(GatewayListenMode::Lan);
        settings.upstream_proxy_enabled = true;
        settings.upstream_proxy_url = "http://127.0.0.1:37123".to_string();
        let result = validate_proxy_for_settings(&settings);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("self-loop"));
    }

    #[test]
    fn test_validate_proxy_for_settings_rejects_custom_host_self_loop() {
        let mut settings = settings_with_listen_mode(GatewayListenMode::Custom);
        settings.gateway_custom_listen_address = "devbox.internal:37123".to_string();
        settings.upstream_proxy_enabled = true;
        settings.upstream_proxy_url = "http://devbox.internal:37123".to_string();
        let result = validate_proxy_for_settings(&settings);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_effective_proxy_url_with_separate_http_credentials() {
        let effective = build_effective_proxy_url(
            Some("http://127.0.0.1:7890"),
            Some("proxy-user"),
            Some("secret"),
        )
        .expect("build proxy url")
        .expect("proxy url should exist");
        let parsed = Url::parse(&effective).expect("parse effective proxy url");

        assert_eq!(parsed.scheme(), "http");
        assert_eq!(parsed.username(), "proxy-user");
        assert_eq!(parsed.password(), Some("secret"));
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(7890));
    }

    #[test]
    fn test_build_effective_proxy_url_with_separate_socks_credentials() {
        let effective = build_effective_proxy_url(
            Some("socks5://127.0.0.1:1080"),
            Some("proxy-user"),
            Some("secret"),
        )
        .expect("build proxy url")
        .expect("proxy url should exist");
        let parsed = Url::parse(&effective).expect("parse effective proxy url");

        assert_eq!(parsed.scheme(), "socks5");
        assert_eq!(parsed.username(), "proxy-user");
        assert_eq!(parsed.password(), Some("secret"));
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(1080));
    }

    #[test]
    fn test_build_effective_proxy_url_preserves_embedded_credentials() {
        let effective =
            build_effective_proxy_url(Some("http://inline:secret@127.0.0.1:7890"), None, None)
                .expect("build proxy url")
                .expect("proxy url should exist");
        let parsed = Url::parse(&effective).expect("parse effective proxy url");

        assert_eq!(parsed.username(), "inline");
        assert_eq!(parsed.password(), Some("secret"));
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(7890));
    }

    #[test]
    fn test_build_effective_proxy_url_rejects_password_without_username() {
        let err =
            build_effective_proxy_url(Some("http://127.0.0.1:7890"), Some("   "), Some("secret"))
                .expect_err("password without username should fail");

        assert!(err.contains("upstream_proxy_username cannot be empty"));
    }

    #[test]
    fn test_build_effective_proxy_url_rejects_mixed_embedded_and_separate_credentials() {
        let err = build_effective_proxy_url(
            Some("http://inline:secret@127.0.0.1:7890"),
            Some("proxy-user"),
            Some("override"),
        )
        .expect_err("mixed credentials should fail");

        assert!(err.contains("either in proxy URL or username/password fields"));
    }

    #[test]
    fn test_validate_proxy_for_settings_accepts_separate_credentials() {
        let mut settings = settings_with_listen_mode(GatewayListenMode::Localhost);
        settings.upstream_proxy_enabled = true;
        settings.upstream_proxy_url = "https://proxy.example.com:8443".to_string();
        settings.upstream_proxy_username = "proxy-user".to_string();
        settings.upstream_proxy_password = "secret".to_string();

        let result = validate_proxy_for_settings(&settings);
        assert!(result.is_ok(), "expected settings validation to succeed");
    }

    #[test]
    fn test_validate_proxy_for_settings_rejects_conflicting_credentials() {
        let mut settings = settings_with_listen_mode(GatewayListenMode::Localhost);
        settings.upstream_proxy_enabled = true;
        settings.upstream_proxy_url = "http://inline:secret@proxy.example.com:7890".to_string();
        settings.upstream_proxy_username = "proxy-user".to_string();
        settings.upstream_proxy_password = "override".to_string();

        let err = validate_proxy_for_settings(&settings)
            .expect_err("mixed credentials should be rejected");
        assert!(err.contains("either in proxy URL or username/password fields"));
    }

    #[test]
    fn test_validate_proxy_self_loop_uses_current_context() {
        set_gateway_self_context(build_self_check_context(
            37123,
            &["127.0.0.1".to_string(), "localhost".to_string()],
        ));

        let result =
            validate_proxy_with_context(Some("http://127.0.0.1:37123"), &current_self_context());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("self-loop"));

        let result =
            validate_proxy_with_context(Some("http://127.0.0.1:7890"), &current_self_context());
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_exit_ip_response_accepts_ipv4_and_ipv6() {
        assert_eq!(
            parse_exit_ip_response("203.0.113.42\n"),
            Some("203.0.113.42".to_string())
        );
        assert_eq!(
            parse_exit_ip_response("2001:db8::1"),
            Some("2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_parse_exit_ip_response_rejects_non_ip_body() {
        assert_eq!(parse_exit_ip_response("<html>not an ip</html>"), None);
        assert_eq!(parse_exit_ip_response(""), None);
    }

    #[test]
    fn test_append_limited_probe_body_chunk_keeps_bounded_prefix() {
        let mut bytes = b"abcd".to_vec();
        let truncated = append_limited_probe_body_chunk(&mut bytes, b"efgh", 6);

        assert_eq!(bytes, b"abcdef");
        assert!(truncated);
    }

    #[test]
    fn test_summarize_probe_body_marks_truncated_payload() {
        let summary = summarize_probe_body(&LimitedProbeBody {
            text: "<html>not an ip</html>".to_string(),
            truncated: true,
            limit: 12,
        });

        assert_eq!(summary, "<html>not an ip</html>; truncated after 12 bytes");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_proxy_performs_real_network_request() {
        let _guard = use_http_proxy_test_url();
        let (proxy_url, request_rx) = spawn_http_proxy_server();
        set_gateway_self_context(build_self_check_context(
            37123,
            &["127.0.0.1".to_string(), "localhost".to_string()],
        ));

        test_proxy_with_context(Some(&proxy_url), &current_self_context())
            .await
            .expect("proxy test should succeed");

        let request = request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("proxy should receive a request");
        assert!(request.starts_with("GET http://example.com/ HTTP/1.1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_detect_proxy_exit_ip_returns_probe_ip() {
        let _guard = use_http_proxy_exit_ip_test_url();
        let (proxy_url, request_rx) = spawn_http_proxy_server_with_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n203.0.113.42\n"
                .to_vec(),
        );
        set_gateway_self_context(build_self_check_context(
            37123,
            &["127.0.0.1".to_string(), "localhost".to_string()],
        ));

        let exit_ip = detect_proxy_exit_ip_with_context(Some(&proxy_url), &current_self_context())
            .await
            .expect("proxy exit ip detection should succeed");

        assert_eq!(exit_ip, "203.0.113.42");

        let request = request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("proxy should receive exit ip request");
        assert!(request.starts_with("GET http://ifconfig.me/ip HTTP/1.1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_scoped_client_uses_current_proxy_and_preserves_redirect_policy() {
        let _guard = use_http_proxy_test_url();
        let (proxy_url, request_rx) = spawn_http_proxy_server_with_response(
            b"HTTP/1.1 302 Found\r\nLocation: http://example.com/next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        );
        let credentialed_proxy_url =
            proxy_url.replacen("http://", "http://proxy-user:proxy-secret@", 1);
        set_gateway_self_context(build_self_check_context(
            37123,
            &["127.0.0.1".to_string(), "localhost".to_string()],
        ));
        init(Some(&credentialed_proxy_url)).expect("init credentialed proxy");

        let client = build_client_with_current_proxy(
            "aio-model-discovery-test",
            Duration::from_secs(1),
            Duration::from_secs(2),
            reqwest::redirect::Policy::none(),
        )
        .expect("build scoped client");
        let response = client
            .get("http://example.com/models")
            .send()
            .await
            .expect("scoped proxied request");
        assert_eq!(response.status(), StatusCode::FOUND);

        let request = request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("proxy should receive scoped request");
        assert!(request.starts_with("GET http://example.com/models HTTP/1.1"));
        let public_proxy_url = get_current_proxy_url().expect("masked proxy status");
        assert!(!public_proxy_url.contains("proxy-user"));
        assert!(!public_proxy_url.contains("proxy-secret"));

        apply_proxy(None).expect("restore direct client");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_apply_proxy_none_restores_direct_connection() {
        let _guard = use_http_proxy_test_url();
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::remove_var(key);
        }

        let (proxy_url, proxy_request_rx) = spawn_http_proxy_server();
        let (origin_url, origin_request_rx) = spawn_http_origin_server();

        init(Some(&proxy_url)).expect("init proxy");

        let proxied_response = get()
            .get("http://example.com/")
            .send()
            .await
            .expect("proxied request");
        assert_eq!(proxied_response.status(), StatusCode::OK);
        let proxied_request = proxy_request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("proxy should receive proxied request");
        assert!(proxied_request.starts_with("GET http://example.com/ HTTP/1.1"));

        apply_proxy(None).expect("disable proxy");

        let direct_response = get()
            .get(&origin_url)
            .send()
            .await
            .expect("direct request after disabling proxy");
        assert_eq!(direct_response.status(), StatusCode::OK);

        let direct_request = origin_request_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("origin should receive direct request");
        assert!(direct_request.starts_with("GET / HTTP/1.1"));
    }
}
