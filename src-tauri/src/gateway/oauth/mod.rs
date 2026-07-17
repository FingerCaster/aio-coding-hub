//! Usage: OAuth adapter pattern for multi-CLI OAuth login support.

pub(crate) mod adapters;
pub(crate) mod callback_server;
pub(crate) mod pkce;
pub(crate) mod provider_trait;
pub(crate) mod refresh;
pub(crate) mod refresh_loop;
pub(crate) mod registry;
pub(crate) mod token_exchange;

use std::sync::Mutex;
use tokio::sync::watch;

struct ActiveOAuthFlow {
    flow_id: String,
    _abort: watch::Sender<()>,
    device: Option<DeviceOAuthFlowBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceOAuthFlowBinding {
    pub(crate) provider_id: i64,
    pub(crate) cli_key: String,
    pub(crate) provider_type: String,
    pub(crate) device_code: String,
    pub(crate) user_code: String,
    pub(crate) deadline_unix: i64,
}

pub(crate) struct OAuthFlowLifecycle {
    pub(crate) flow_id: String,
    pub(crate) abort_rx: watch::Receiver<()>,
}

/// Global lifecycle handle for in-progress OAuth flows.
/// When a new flow starts, it cancels any prior pending flow so the old callback
/// listener is dropped immediately (frees the port) and stale device-code polls
/// can no longer persist tokens.
static ACTIVE_FLOW: Mutex<Option<ActiveOAuthFlow>> = Mutex::new(None);

#[cfg(test)]
pub(crate) async fn oauth_flow_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static FLOW_TEST_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    FLOW_TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn generate_flow_id() -> String {
    use rand::RngCore;

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Cancel any in-progress OAuth flow and return a receiver that the new flow
/// should select on so it can itself be cancelled by a future invocation.
pub(crate) fn begin_flow_lifecycle() -> OAuthFlowLifecycle {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    // Dropping the old sender causes the old receiver to see a channel-closed signal,
    // which aborts the old `wait_for_callback` via the tokio::select! in the caller.
    let (tx, rx) = watch::channel(());
    let flow_id = generate_flow_id();
    *guard = Some(ActiveOAuthFlow {
        flow_id: flow_id.clone(),
        _abort: tx,
        device: None,
    });
    OAuthFlowLifecycle {
        flow_id,
        abort_rx: rx,
    }
}

pub(crate) fn bind_device_flow(
    flow_id: &str,
    binding: DeviceOAuthFlowBinding,
) -> crate::shared::error::AppResult<()> {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    let active = guard
        .as_mut()
        .filter(|active| active.flow_id == flow_id)
        .ok_or_else(|| {
            crate::shared::error::AppError::from(
                "OAuth flow cancelled: login attempt is no longer current".to_string(),
            )
        })?;
    active.device = Some(binding);
    Ok(())
}

pub(crate) fn current_device_flow(
    flow_id: &str,
    now_unix: i64,
) -> crate::shared::error::AppResult<DeviceOAuthFlowBinding> {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    let binding = guard
        .as_ref()
        .filter(|active| active.flow_id == flow_id)
        .and_then(|active| active.device.clone())
        .ok_or_else(|| {
            crate::shared::error::AppError::from(
                "OAuth flow cancelled: login attempt is no longer current".to_string(),
            )
        })?;
    if now_unix >= binding.deadline_unix {
        *guard = None;
        return Err(crate::shared::error::AppError::from(
            "OAuth device flow expired".to_string(),
        ));
    }
    Ok(binding)
}

pub(crate) fn is_current_flow(flow_id: &str) -> bool {
    let guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .is_some_and(|active| active.flow_id == flow_id)
}

pub(crate) fn cancel_flow(flow_id: &str) -> bool {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    if guard
        .as_ref()
        .is_some_and(|active| active.flow_id == flow_id)
    {
        *guard = None;
        true
    } else {
        false
    }
}

pub(crate) fn complete_current_flow<T>(
    flow_id: &str,
    complete: impl FnOnce() -> crate::shared::error::AppResult<T>,
) -> crate::shared::error::AppResult<T> {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    if guard
        .as_ref()
        .is_none_or(|active| active.flow_id != flow_id)
    {
        return Err(crate::shared::error::AppError::from(
            "OAuth flow cancelled: login attempt is no longer current".to_string(),
        ));
    }

    let result = complete();
    if result.is_ok() {
        *guard = None;
    }
    result
}

pub(crate) fn complete_current_device_flow<T>(
    flow_id: &str,
    expected: &DeviceOAuthFlowBinding,
    now_unix: i64,
    complete: impl FnOnce() -> crate::shared::error::AppResult<T>,
) -> crate::shared::error::AppResult<T> {
    let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|e| e.into_inner());
    let Some(active) = guard.as_ref().filter(|active| active.flow_id == flow_id) else {
        return Err(crate::shared::error::AppError::from(
            "OAuth flow cancelled: login attempt is no longer current".to_string(),
        ));
    };
    if active.device.as_ref() != Some(expected) {
        return Err(crate::shared::error::AppError::from(
            "OAuth device flow ownership changed".to_string(),
        ));
    }
    if now_unix >= expected.deadline_unix {
        *guard = None;
        return Err(crate::shared::error::AppError::from(
            "OAuth device flow expired".to_string(),
        ));
    }

    let result = complete();
    if result.is_ok() {
        *guard = None;
    }
    result
}

/// Default User-Agent for OAuth HTTP requests (mirrors the supported Codex CLI).
pub(crate) const DEFAULT_OAUTH_USER_AGENT: &str =
    crate::gateway::upstream_identity::CODEX_CLI_USER_AGENT;
/// Default request timeout in seconds for OAuth HTTP requests.
pub(crate) const DEFAULT_OAUTH_TIMEOUT_SECS: u64 = 30;
/// Default connect timeout in seconds for OAuth HTTP requests.
pub(crate) const DEFAULT_OAUTH_CONNECT_TIMEOUT_SECS: u64 = 15;

/// Build an HTTP client with default OAuth settings.
pub(crate) fn build_default_oauth_http_client() -> Result<reqwest::Client, String> {
    build_oauth_http_client(
        DEFAULT_OAUTH_USER_AGENT,
        DEFAULT_OAUTH_TIMEOUT_SECS,
        DEFAULT_OAUTH_CONNECT_TIMEOUT_SECS,
    )
}

fn mask_oauth_proxy_env_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if reqwest::Url::parse(trimmed).is_err() && trimmed.contains('@') {
        return "[redacted]".to_string();
    }
    super::http_client::mask_url(trimmed)
}

/// Build an HTTP client suitable for OAuth token exchange and refresh requests.
///
/// Respects standard proxy environment variables (`HTTPS_PROXY`, `HTTP_PROXY`,
/// `ALL_PROXY`) automatically via reqwest defaults.  Additionally, if the user
/// has set `AIO_OAUTH_PROXY_URL`, that URL will be configured as an explicit
/// "all traffic" proxy, which is useful in corporate environments where system
/// proxy detection is insufficient.
pub(crate) fn build_oauth_http_client(
    user_agent: &str,
    timeout_secs: u64,
    connect_timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs));

    // Explicit proxy override from dedicated env var.
    if let Ok(proxy_url) = std::env::var("AIO_OAUTH_PROXY_URL") {
        let trimmed = proxy_url.trim();
        if !trimmed.is_empty() {
            let masked = mask_oauth_proxy_env_value(trimmed);
            tracing::info!(
                proxy_url = %masked,
                "oauth: using explicit proxy from AIO_OAUTH_PROXY_URL"
            );
            let proxy = reqwest::Proxy::all(trimmed)
                .map_err(|e| format!("invalid AIO_OAUTH_PROXY_URL={masked}: {e}"))?;
            builder = builder.proxy(proxy);
        }
    } else {
        // Log which standard proxy env vars are active for diagnostics.
        for var in [
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "https_proxy",
            "http_proxy",
            "all_proxy",
        ] {
            if let Ok(val) = std::env::var(var) {
                if !val.is_empty() {
                    tracing::debug!(
                        env_var = var,
                        value = %mask_oauth_proxy_env_value(&val),
                        "oauth: detected proxy env var"
                    );
                }
            }
        }
    }

    builder
        .build()
        .map_err(|e| format!("oauth HTTP client init failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvVarRestore {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn reset_oauth_flow_for_test() {
        let mut guard = ACTIVE_FLOW.lock().unwrap_or_else(|err| err.into_inner());
        *guard = None;
    }

    #[test]
    fn oauth_proxy_env_mask_redacts_valid_url_credentials() {
        assert_eq!(
            mask_oauth_proxy_env_value("http://user:secret@proxy.example.com:7890"),
            "http://proxy.example.com:7890"
        );
    }

    #[test]
    fn oauth_proxy_env_mask_redacts_invalid_credential_like_values() {
        assert_eq!(
            mask_oauth_proxy_env_value("http://user:super-secret@"),
            "[redacted]"
        );
    }

    #[test]
    fn explicit_oauth_proxy_error_masks_env_value() {
        let _env_lock = crate::test_support::test_env_lock();
        let _restore = EnvVarRestore::set("AIO_OAUTH_PROXY_URL", "http://user:super-secret@");

        let err = build_oauth_http_client("test-agent", 1, 1)
            .expect_err("invalid explicit proxy should fail")
            .to_string();

        assert!(err.contains("[redacted]"));
        assert!(!err.contains("super-secret"));
        assert!(!err.contains("user:"));
    }

    #[tokio::test]
    async fn oauth_flow_lifecycle_replaces_current_flow() {
        let _flow_lock = super::oauth_flow_test_lock().await;
        reset_oauth_flow_for_test();

        let first = begin_flow_lifecycle();
        assert!(is_current_flow(&first.flow_id));

        let second = begin_flow_lifecycle();
        assert!(!is_current_flow(&first.flow_id));
        assert!(is_current_flow(&second.flow_id));

        assert!(!cancel_flow(&first.flow_id));
        assert!(cancel_flow(&second.flow_id));
        assert!(!is_current_flow(&second.flow_id));
    }

    #[tokio::test]
    async fn oauth_flow_completion_rejects_stale_flow() {
        let _flow_lock = super::oauth_flow_test_lock().await;
        reset_oauth_flow_for_test();

        let first = begin_flow_lifecycle();
        let second = begin_flow_lifecycle();

        let stale = complete_current_flow(&first.flow_id, || {
            Ok::<_, crate::shared::error::AppError>(())
        });
        assert!(stale.is_err());

        let current = complete_current_flow(&second.flow_id, || {
            Ok::<_, crate::shared::error::AppError>(())
        });
        assert!(current.is_ok());
        assert!(!is_current_flow(&second.flow_id));
    }

    #[tokio::test]
    async fn device_flow_binding_is_server_owned_and_expires_fail_closed() {
        let _flow_lock = super::oauth_flow_test_lock().await;
        reset_oauth_flow_for_test();
        let lifecycle = begin_flow_lifecycle();
        let binding = DeviceOAuthFlowBinding {
            provider_id: 41,
            cli_key: "codex".to_string(),
            provider_type: "openai".to_string(),
            device_code: "bound-device".to_string(),
            user_code: "bound-user".to_string(),
            deadline_unix: 200,
        };
        bind_device_flow(&lifecycle.flow_id, binding.clone()).expect("bind device flow");

        assert_eq!(
            current_device_flow(&lifecycle.flow_id, 199).expect("current binding"),
            binding
        );
        assert!(current_device_flow("different-flow", 199).is_err());
        assert!(current_device_flow(&lifecycle.flow_id, 200).is_err());
        assert!(!is_current_flow(&lifecycle.flow_id));
    }

    #[tokio::test]
    async fn device_flow_completion_rechecks_binding_and_never_runs_on_failure() {
        let _flow_lock = super::oauth_flow_test_lock().await;
        reset_oauth_flow_for_test();
        let lifecycle = begin_flow_lifecycle();
        let binding = DeviceOAuthFlowBinding {
            provider_id: 41,
            cli_key: "codex".to_string(),
            provider_type: "openai".to_string(),
            device_code: "bound-device".to_string(),
            user_code: "bound-user".to_string(),
            deadline_unix: 200,
        };
        bind_device_flow(&lifecycle.flow_id, binding.clone()).expect("bind device flow");

        let mut wrong_provider = binding.clone();
        wrong_provider.provider_id = 42;
        let mut called = false;
        assert!(
            complete_current_device_flow(&lifecycle.flow_id, &wrong_provider, 199, || {
                called = true;
                Ok::<_, crate::shared::error::AppError>(())
            })
            .is_err()
        );
        assert!(!called);

        let mut wrong_cli = binding.clone();
        wrong_cli.cli_key = "grok".to_string();
        assert!(
            complete_current_device_flow(&lifecycle.flow_id, &wrong_cli, 199, || {
                called = true;
                Ok::<_, crate::shared::error::AppError>(())
            })
            .is_err()
        );
        assert!(!called);

        let mut wrong_codes = binding.clone();
        wrong_codes.device_code = "changed".to_string();
        assert!(
            complete_current_device_flow(&lifecycle.flow_id, &wrong_codes, 199, || {
                called = true;
                Ok::<_, crate::shared::error::AppError>(())
            })
            .is_err()
        );
        assert!(!called);

        assert!(
            complete_current_device_flow(&lifecycle.flow_id, &binding, 200, || {
                called = true;
                Ok::<_, crate::shared::error::AppError>(())
            })
            .is_err()
        );
        assert!(!called);
    }
}
