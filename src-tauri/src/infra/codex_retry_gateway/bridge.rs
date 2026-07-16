use crate::infra::codex_retry_gateway::config::{DEFAULT_HEALTH_PATH, DEFAULT_LISTEN_HOST};
use crate::infra::codex_retry_gateway::managed_state::{
    read_manager_state, CodexRetryGatewayManagedProcessRecord, CodexRetryGatewayManagerPaths,
};
use crate::infra::codex_retry_gateway::process::{
    reconcile_runtime_process, CodexRetryGatewayManagedProcess,
};
use crate::infra::codex_retry_gateway::util::now_unix_ms;
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayDetailsSession, CodexRetryGatewayLifecycleCallback,
    CodexRetryGatewayRouteCallbackReason, CodexRetryGatewayRouteCallbackRequest,
    CodexRetryGatewayStatus, CodexRouteMode,
};
use crate::shared::error::{AppError, AppResult};
use axum::body::{to_bytes, Body};
use axum::extract::{OriginalUri, Path as AxumPath, State};
use axum::http::header::{COOKIE, LOCATION, ORIGIN, REFERER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode};
use axum::routing::{any, get};
use axum::Router;
use bytes::Bytes;
use futures_core::Stream;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{oneshot, Mutex, RwLock};

const BRIDGE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_RESPONSE_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_LAUNCH_TOKEN_TTL_MS: u64 = 60 * 1000;
const BRIDGE_COOKIE_NAME_PREFIX: &str = "aio_codex_retry_gateway_session";
const SEC_FETCH_SITE: &str = "sec-fetch-site";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BridgeRuntimeHandle {
    pub(crate) base_origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BridgeDetailsSession {
    pub(crate) handle: BridgeRuntimeHandle,
    pub(crate) session: CodexRetryGatewayDetailsSession,
}

#[derive(Debug, Clone)]
struct BridgeManagedIdentity {
    generation: u64,
    listener: String,
    pid: u32,
    start_identity: Option<u64>,
    source_commit: String,
    instance_nonce: String,
}

#[derive(Debug, Clone)]
struct BridgeLaunchTokenState {
    identity: BridgeManagedIdentity,
    view_id: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone)]
struct BridgeSessionState {
    identity: BridgeManagedIdentity,
    view_id: String,
}

struct BridgeRuntimeInner {
    handle: BridgeRuntimeHandle,
    state: Arc<BridgeRuntimeState>,
    #[cfg_attr(not(test), allow(dead_code))]
    // Test-only reset needs the shutdown handle; runtime keeps it for teardown.
    shutdown: Option<oneshot::Sender<()>>,
    _task: tauri::async_runtime::JoinHandle<()>,
}

struct BridgeRuntimeState {
    base_origin: String,
    cookie_name: String,
    paths: RwLock<CodexRetryGatewayManagerPaths>,
    callback: RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>>,
    launch_tokens: RwLock<HashMap<String, BridgeLaunchTokenState>>,
    sessions: RwLock<HashMap<String, BridgeSessionState>>,
    client: Client,
}

static BRIDGE_RUNTIME: OnceLock<Mutex<Option<BridgeRuntimeInner>>> = OnceLock::new();

fn bridge_runtime_slot() -> &'static Mutex<Option<BridgeRuntimeInner>> {
    BRIDGE_RUNTIME.get_or_init(|| Mutex::new(None))
}

pub(crate) async fn create_bridge_details_session(
    paths: &CodexRetryGatewayManagerPaths,
    generation: u64,
    process: &CodexRetryGatewayManagedProcess,
    callback: Arc<dyn CodexRetryGatewayLifecycleCallback>,
) -> AppResult<BridgeDetailsSession> {
    let handle = ensure_bridge_runtime(paths.clone(), callback).await?;
    let iframe_view_id = crate::infra::codex_retry_gateway::util::random_hex(16);
    let browser_view_id = crate::infra::codex_retry_gateway::util::random_hex(16);
    let iframe_launch_token = crate::infra::codex_retry_gateway::util::random_hex(24);
    let browser_launch_token = crate::infra::codex_retry_gateway::util::random_hex(24);
    let expires_at_ms = now_unix_ms().saturating_add(BRIDGE_LAUNCH_TOKEN_TTL_MS);
    let identity = BridgeManagedIdentity {
        generation,
        listener: process.record.listener.clone(),
        pid: process.record.pid,
        start_identity: process.record.start_identity,
        source_commit: process.record.source_commit.clone(),
        instance_nonce: process.record.instance_nonce.clone(),
    };

    let state = {
        let runtime = bridge_runtime_slot().lock().await;
        runtime
            .as_ref()
            .map(|inner| inner.state.clone())
            .ok_or_else(|| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
                    "bridge runtime is not running",
                )
            })?
    };
    {
        let mut launch_tokens = state.launch_tokens.write().await;
        prune_expired_launch_tokens(&mut launch_tokens);
        for (token, view_id) in [
            (&iframe_launch_token, &iframe_view_id),
            (&browser_launch_token, &browser_view_id),
        ] {
            launch_tokens.insert(
                token.clone(),
                BridgeLaunchTokenState {
                    identity: identity.clone(),
                    view_id: view_id.clone(),
                    expires_at_ms,
                },
            );
        }
    }

    let iframe_url = format!("{}/launch/{}", handle.base_origin, iframe_launch_token);
    let browser_url = format!("{}/launch/{}", handle.base_origin, browser_launch_token);
    Ok(BridgeDetailsSession {
        handle: handle.clone(),
        session: CodexRetryGatewayDetailsSession {
            generation,
            iframe_url,
            browser_url,
            iframe_view_id,
            expires_at_ms,
        },
    })
}

pub(crate) async fn revoke_bridge_details_session(view_id: &str) -> AppResult<()> {
    if view_id.len() != 32 || !view_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_INVALID",
            "bridge view id must be an exact 32-character hexadecimal value",
        ));
    }
    let state = {
        let runtime = bridge_runtime_slot().lock().await;
        runtime.as_ref().map(|inner| inner.state.clone())
    };
    let Some(state) = state else {
        return Ok(());
    };
    {
        let mut launch_tokens = state.launch_tokens.write().await;
        launch_tokens.retain(|_, launch| launch.view_id != view_id);
    }
    {
        let mut sessions = state.sessions.write().await;
        sessions.retain(|_, session| session.view_id != view_id);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) async fn reset_bridge_runtime_for_tests() -> AppResult<()> {
    let mut guard = bridge_runtime_slot().lock().await;
    if let Some(mut inner) = guard.take() {
        if let Some(shutdown) = inner.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
    Ok(())
}

async fn ensure_bridge_runtime(
    paths: CodexRetryGatewayManagerPaths,
    callback: Arc<dyn CodexRetryGatewayLifecycleCallback>,
) -> AppResult<BridgeRuntimeHandle> {
    let mut guard = bridge_runtime_slot().lock().await;
    if let Some(inner) = guard.as_mut() {
        *inner.state.paths.write().await = paths;
        *inner.state.callback.write().await = callback;
        return Ok(inner.handle.clone());
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
                format!("failed to bind bridge loopback listener: {err}"),
            )
        })?;
    let addr = listener.local_addr().map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
            format!("failed to read bridge listener address: {err}"),
        )
    })?;
    let base_origin = format!("http://127.0.0.1:{}", addr.port());
    let state = Arc::new(BridgeRuntimeState {
        base_origin: base_origin.clone(),
        cookie_name: bridge_cookie_name(addr.port()),
        paths: RwLock::new(paths),
        callback: RwLock::new(callback),
        launch_tokens: RwLock::new(HashMap::new()),
        sessions: RwLock::new(HashMap::new()),
        client: Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_HTTP_CLIENT_FAILED",
                    format!("failed to build bridge HTTP client: {err}"),
                )
            })?,
    });
    let handle = BridgeRuntimeHandle {
        base_origin: base_origin.clone(),
    };
    let router = Router::new()
        .route("/launch/:session_id", get(launch_session))
        .fallback(any(proxy_request))
        .with_state(state.clone());
    let (shutdown, shutdown_rx) = oneshot::channel::<()>();
    let task = tauri::async_runtime::spawn(async move {
        let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        let _ = serve.await;
    });

    *guard = Some(BridgeRuntimeInner {
        handle: handle.clone(),
        state,
        shutdown: Some(shutdown),
        _task: task,
    });
    Ok(handle)
}

async fn launch_session(
    State(state): State<Arc<BridgeRuntimeState>>,
    AxumPath(launch_token): AxumPath<String>,
) -> Response<Body> {
    let launch = {
        let mut launch_tokens = state.launch_tokens.write().await;
        consume_launch_token(&mut launch_tokens, &launch_token)
    };
    let Some(launch) = launch else {
        return error_response(
            StatusCode::GONE,
            "bridge launch token is missing, expired, or already consumed",
        );
    };
    match validate_managed_identity(&state, &launch.identity).await {
        Ok(_) => {
            let session_id = crate::infra::codex_retry_gateway::util::random_hex(24);
            state.sessions.write().await.insert(
                session_id.clone(),
                BridgeSessionState {
                    identity: launch.identity,
                    view_id: launch.view_id,
                },
            );
            let mut response = Response::builder()
                .status(StatusCode::FOUND)
                .header(LOCATION, "/__codex_retry_gateway/ui")
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
            if let Ok(value) =
                HeaderValue::from_str(&bridge_session_cookie(&state.cookie_name, &session_id))
            {
                response.headers_mut().insert(SET_COOKIE, value);
            }
            response
        }
        Err(error) => error_response(StatusCode::GONE, &error.to_string()),
    }
}

async fn proxy_request(
    State(state): State<Arc<BridgeRuntimeState>>,
    method: Method,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    body: Body,
) -> Response<Body> {
    let path = uri.path().to_string();
    let query = uri
        .query()
        .map(|value| format!("?{value}"))
        .unwrap_or_default();

    if !path_allowed(&method, &path) {
        return error_response(StatusCode::NOT_FOUND, "path is not allowlisted");
    }
    if !request_origin_allowed(&method, &path, &headers, &state.base_origin) {
        return error_response(
            StatusCode::FORBIDDEN,
            "bridge API requests require a verified same-origin browser context",
        );
    }

    let Some(session_id) = read_session_cookie(&headers, &state.cookie_name) else {
        return error_response(StatusCode::UNAUTHORIZED, "bridge session cookie is missing");
    };
    let validated = match validate_session(&state, &session_id).await {
        Ok(validated) => validated,
        Err(error) => return error_response(StatusCode::UNAUTHORIZED, &error.to_string()),
    };

    if path == "/__codex_retry_gateway/api/restore" {
        return handle_restore(&state, validated.generation).await;
    }

    let body_bytes = match to_bytes(body, BRIDGE_BODY_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!("request body exceeds {BRIDGE_BODY_LIMIT_BYTES} bytes: {err}"),
            )
        }
    };
    let forward_body = if path == "/__codex_retry_gateway/api/config" && method == Method::POST {
        match protected_config_body(&validated.record, &body_bytes) {
            Ok(bytes) => bytes,
            Err(error) => return error_response(StatusCode::BAD_REQUEST, &error.to_string()),
        }
    } else {
        body_bytes.to_vec()
    };

    let target = format!("{}{}{}", validated.record.listener, path, query);
    let forward_headers = copy_forward_headers(&headers);
    let request = state
        .client
        .request(method.clone(), &target)
        .headers(forward_headers)
        .body(forward_body);

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("bridge proxy request failed: {err}"),
            )
        }
    };
    if path == "/__codex_retry_gateway/api/config"
        && method == Method::POST
        && response.status().is_success()
    {
        if let Err(error) = validate_session(&state, &session_id).await {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("managed gateway config write could not be revalidated: {error}"),
            );
        }
    }
    let status = response.status();
    let upstream_headers = response.headers().clone();
    let should_overlay_status = response_requires_status_overlay(&method, &path, status);
    if should_overlay_status {
        let bytes = match read_bounded_response_bytes(response, BRIDGE_RESPONSE_LIMIT_BYTES).await {
            Ok(bytes) => bytes,
            Err(error) if error.code() == "CODEX_RETRY_GATEWAY_BRIDGE_RESPONSE_TOO_LARGE" => {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("upstream response exceeds {BRIDGE_RESPONSE_LIMIT_BYTES} bytes"),
                )
            }
            Err(error) => return error_response(StatusCode::BAD_GATEWAY, &error.to_string()),
        };
        let callback = state.callback.read().await.clone();
        let authoritative = match callback.current_gateway_status().await {
            Ok(status) => status,
            Err(error) => {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("AIO authoritative gateway status is unavailable: {error}"),
                )
            }
        };
        if authoritative.generation != validated.generation {
            return error_response(
                StatusCode::GONE,
                "bridge session generation changed while reading status",
            );
        }
        let bytes = match overlay_authoritative_status(&bytes, &validated.record, &authoritative) {
            Ok(bytes) => bytes,
            Err(error) => return error_response(StatusCode::BAD_GATEWAY, &error.to_string()),
        };
        let mut builder = Response::builder().status(status);
        if let Some(headers_mut) = builder.headers_mut() {
            copy_response_headers(&upstream_headers, headers_mut);
        }
        return builder.body(Body::from(bytes)).unwrap_or_else(|_| {
            error_response(StatusCode::BAD_GATEWAY, "failed to build bridge response")
        });
    }

    if response
        .content_length()
        .is_some_and(|length| length > BRIDGE_RESPONSE_LIMIT_BYTES as u64)
    {
        return error_response(
            StatusCode::BAD_GATEWAY,
            &format!("upstream response exceeds {BRIDGE_RESPONSE_LIMIT_BYTES} bytes"),
        );
    }
    let mut builder = Response::builder().status(status);
    if let Some(headers_mut) = builder.headers_mut() {
        copy_response_headers(&upstream_headers, headers_mut);
    }
    let stream =
        BoundedBridgeResponseStream::new(response.bytes_stream(), BRIDGE_RESPONSE_LIMIT_BYTES);
    builder.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        error_response(StatusCode::BAD_GATEWAY, "failed to build bridge response")
    })
}

fn request_origin_allowed(
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    base_origin: &str,
) -> bool {
    // The one-time launch route redirects a Tauri iframe to the bridge UI.
    // WebView2 preserves the top-level cross-site fetch metadata across that
    // redirect, so the document navigation cannot prove bridge origin yet.
    // It is read-only and is still authenticated by the session cookie below;
    // API requests retain the stricter browser-origin checks.
    if !path.starts_with("/__codex_retry_gateway/api/") {
        return matches!(*method, Method::GET | Method::HEAD);
    }

    let fetch_site = headers
        .get(SEC_FETCH_SITE)
        .and_then(|value| value.to_str().ok());
    let exact_origin = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == base_origin);
    let same_origin_referer = headers
        .get(REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(|referer| reqwest::Url::parse(referer).ok())
        .is_some_and(|referer| referer.origin().ascii_serialization() == base_origin);
    if fetch_site.is_some_and(|site| site != "same-origin") && !exact_origin && !same_origin_referer
    {
        return false;
    }
    if !matches!(*method, Method::GET | Method::HEAD) {
        return exact_origin;
    }
    fetch_site == Some("same-origin") || exact_origin || same_origin_referer
}

fn response_requires_status_overlay(method: &Method, path: &str, status: StatusCode) -> bool {
    status.is_success()
        && ((path == "/__codex_retry_gateway/api/status" && method == Method::GET)
            || (path == "/__codex_retry_gateway/api/config" && method == Method::POST))
}

fn overlay_authoritative_status(
    bytes: &[u8],
    record: &CodexRetryGatewayManagedProcessRecord,
    status: &CodexRetryGatewayStatus,
) -> AppResult<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_STATUS_INVALID",
            format!("failed to parse external status response: {err}"),
        )
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_STATUS_INVALID",
            "external status response must be an object",
        )
    })?;
    let config = object
        .entry("config".to_string())
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_STATUS_INVALID",
                "external status config must be an object",
            )
        })?;
    config.insert(
        "listen_host".to_string(),
        Value::String(DEFAULT_LISTEN_HOST.to_string()),
    );
    config.insert(
        "upstream_base_url".to_string(),
        Value::String(record.upstream_base_url.clone()),
    );
    config.insert(
        "health_path".to_string(),
        Value::String(DEFAULT_HEALTH_PATH.to_string()),
    );
    if let Ok(listener) = reqwest::Url::parse(&record.listener) {
        if let Some(port) = listener.port_or_known_default() {
            config.insert("listen_port".to_string(), Value::from(port));
            object.insert(
                "listen".to_string(),
                Value::String(format!("{DEFAULT_LISTEN_HOST}:{port}")),
            );
        }
    }

    let external_state = object
        .entry("state".to_string())
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_STATUS_INVALID",
                "external status state must be an object",
            )
        })?;
    external_state.insert("process_id".to_string(), Value::from(record.pid));
    external_state.insert(
        "original_base_url".to_string(),
        Value::String(record.upstream_base_url.clone()),
    );
    external_state.insert(
        "gateway_base_url".to_string(),
        Value::String(record.listener.clone()),
    );
    external_state.insert(
        "aio_instance_nonce".to_string(),
        Value::String(record.instance_nonce.clone()),
    );
    external_state.insert(
        "provider_name".to_string(),
        Value::String(record.provider_name.clone()),
    );
    match status.route_mode {
        CodexRouteMode::Guarded => {
            external_state.insert(
                "codex_current_base_url".to_string(),
                Value::String(format!("{}/v1", record.listener.trim_end_matches('/'))),
            );
        }
        CodexRouteMode::DirectAio => {
            external_state.insert(
                "codex_current_base_url".to_string(),
                Value::String(record.upstream_base_url.clone()),
            );
        }
        CodexRouteMode::Unproxied => {}
    }

    object.insert("process_id".to_string(), Value::from(record.pid));
    object.insert(
        "aio".to_string(),
        serde_json::json!({
            "managed": true,
            "generation": status.generation,
            "desired_enabled": status.desired_enabled,
            "runtime_phase": status.runtime_phase,
            "route_mode": status.route_mode,
            "cli_proxy_enabled": status.cli_proxy_enabled,
            "cli_proxy_applied": status.cli_proxy_applied,
            "selected_commit": status.selected_commit,
            "active_commit": status.active_commit,
            "previous_commit": status.previous_commit,
            "source_commit": record.source_commit,
            "provider_name": record.provider_name,
            "listener": record.listener,
            "upstream_base_url": record.upstream_base_url,
        }),
    );
    serde_json::to_vec(&value).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_STATUS_INVALID",
            format!("failed to serialize overlaid status response: {err}"),
        )
    })
}

async fn handle_restore(state: &Arc<BridgeRuntimeState>, generation: u64) -> Response<Body> {
    let callback = state.callback.read().await.clone();
    match callback
        .request_gateway_disable(CodexRetryGatewayRouteCallbackRequest {
            generation,
            reason: CodexRetryGatewayRouteCallbackReason::ExternalRestore,
        })
        .await
    {
        Ok(()) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "ok": true,
                "handled_by": "aio",
                "reason": "external_restore"
            }),
        ),
        Err(error) => error_response(StatusCode::SERVICE_UNAVAILABLE, &error.to_string()),
    }
}

async fn read_bounded_response_bytes(
    mut response: reqwest::Response,
    limit: usize,
) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_RESPONSE_READ_FAILED",
            format!("failed to read upstream response: {err}"),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_RESPONSE_TOO_LARGE",
                format!("upstream response exceeds {limit} bytes"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

struct ValidatedBridgeRequest {
    generation: u64,
    record: CodexRetryGatewayManagedProcessRecord,
}

async fn validate_session(
    state: &Arc<BridgeRuntimeState>,
    session_id: &str,
) -> AppResult<ValidatedBridgeRequest> {
    let session = {
        let sessions = state.sessions.read().await;
        sessions.get(session_id).cloned().ok_or_else(|| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_INVALID",
                "bridge session is missing or revoked",
            )
        })?
    };

    match validate_managed_identity(state, &session.identity).await {
        Ok(validated) => Ok(validated),
        Err(error) => {
            state.sessions.write().await.remove(session_id);
            Err(error)
        }
    }
}

async fn validate_managed_identity(
    state: &Arc<BridgeRuntimeState>,
    identity: &BridgeManagedIdentity,
) -> AppResult<ValidatedBridgeRequest> {
    let paths = state.paths.read().await.clone();
    let manager = read_manager_state(&paths)?;
    if manager.generation != identity.generation {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_STALE",
            "bridge session generation no longer matches the managed runtime state",
        ));
    }
    let Some(record) = manager.process_record else {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_STALE",
            "managed gateway process is no longer recorded",
        ));
    };
    let reconciled =
        reconcile_runtime_process(&paths, Some(&record), manager.effective_port).await?;
    let Some(process) = reconciled.managed else {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_STALE",
            "managed gateway process is no longer owned and healthy",
        ));
    };
    let record = process.record;
    if record.pid != identity.pid
        || record.listener != identity.listener
        || record.instance_nonce != identity.instance_nonce
        || record.source_commit != identity.source_commit
        || record.start_identity != identity.start_identity
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_STALE",
            "managed gateway process identity changed",
        ));
    }

    Ok(ValidatedBridgeRequest {
        generation: identity.generation,
        record,
    })
}

fn bridge_cookie_name(port: u16) -> String {
    format!("{BRIDGE_COOKIE_NAME_PREFIX}_{port}")
}

fn bridge_session_cookie(cookie_name: &str, session_id: &str) -> String {
    // The Tauri page and loopback bridge are cross-site. Chromium/WebView2
    // require SameSite=None plus Secure for the iframe to send this cookie.
    format!("{cookie_name}={session_id}; HttpOnly; SameSite=None; Secure; Path=/")
}

fn read_session_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name.trim() == cookie_name).then(|| value.trim().to_string())
    })
}

fn path_allowed(method: &Method, path: &str) -> bool {
    let segments = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["favicon.ico"] => matches!(*method, Method::GET),
        ["__codex_retry_gateway", "ui"] => matches!(*method, Method::GET),
        ["__codex_retry_gateway", "api", "status"] => matches!(*method, Method::GET),
        ["__codex_retry_gateway", "api", "logs"] => matches!(*method, Method::GET),
        ["__codex_retry_gateway", "api", "config"] => matches!(*method, Method::POST),
        ["__codex_retry_gateway", "api", "probe", "run"] => matches!(*method, Method::POST),
        ["__codex_retry_gateway", "api", "restore"] => matches!(*method, Method::POST),
        ["__codex_retry_gateway", "api", "analytics", "reasoning"] => {
            matches!(*method, Method::GET)
        }
        ["__codex_retry_gateway", "api", "analytics", "reasoning", "analyze"] => {
            matches!(*method, Method::POST)
        }
        ["__codex_retry_gateway", "api", "analytics", "reasoning", "export"] => {
            matches!(*method, Method::GET)
        }
        ["__codex_retry_gateway", "api", "analytics", "reasoning", "export", "jobs", job_id] => {
            matches!(*method, Method::GET) && !job_id.is_empty()
        }
        ["__codex_retry_gateway", "api", "analytics", "reasoning", "export", "jobs", job_id, "download"] => {
            matches!(*method, Method::GET) && !job_id.is_empty()
        }
        ["__codex_retry_gateway", "api", "analytics", "imports"] => {
            matches!(*method, Method::GET)
        }
        ["__codex_retry_gateway", "api", "analytics", "imports", "run"] => {
            matches!(*method, Method::POST)
        }
        ["__codex_retry_gateway", "api", "analytics", "imports", "analyze"] => {
            matches!(*method, Method::POST)
        }
        ["__codex_retry_gateway", "api", "analytics", "imports", "latest"] => {
            matches!(*method, Method::GET)
        }
        ["__codex_retry_gateway", "api", "analytics", "imports", "jobs", job_id] => {
            matches!(*method, Method::GET) && !job_id.is_empty()
        }
        _ => false,
    }
}

fn protected_config_body(
    record: &CodexRetryGatewayManagedProcessRecord,
    bytes: &[u8],
) -> AppResult<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_INVALID",
            format!("failed to parse config request body: {err}"),
        )
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_INVALID",
            "config request body must be a JSON object",
        )
    })?;
    let listener_url = reqwest::Url::parse(&record.listener).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_INVALID",
            format!("managed gateway listener is invalid: {err}"),
        )
    })?;
    let listen_port = listener_url.port_or_known_default().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_INVALID",
            "managed gateway listener is missing its listen port",
        )
    })?;
    if let Some(host) = object.get("listen_host").and_then(|value| value.as_str()) {
        if host.trim() != DEFAULT_LISTEN_HOST {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_PROTECTED",
                "listen_host is managed by AIO and cannot be changed",
            ));
        }
    }
    if let Some(port) = object.get("listen_port").and_then(|value| value.as_u64()) {
        if port != listen_port as u64 {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_PROTECTED",
                "listen_port is managed by AIO and cannot be changed",
            ));
        }
    }
    if let Some(upstream) = object
        .get("upstream_base_url")
        .and_then(|value| value.as_str())
    {
        if upstream.trim() != record.upstream_base_url {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_PROTECTED",
                "upstream_base_url is managed by AIO and cannot be changed",
            ));
        }
    }
    if let Some(health_path) = object.get("health_path").and_then(|value| value.as_str()) {
        if health_path.trim() != DEFAULT_HEALTH_PATH {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_PROTECTED",
                "health_path is managed by AIO and cannot be changed",
            ));
        }
    }
    object.insert(
        "listen_host".to_string(),
        Value::String(DEFAULT_LISTEN_HOST.to_string()),
    );
    object.insert("listen_port".to_string(), Value::from(listen_port));
    object.insert(
        "upstream_base_url".to_string(),
        Value::String(record.upstream_base_url.clone()),
    );
    object.insert(
        "health_path".to_string(),
        Value::String(DEFAULT_HEALTH_PATH.to_string()),
    );
    serde_json::to_vec(&value).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_CONFIG_INVALID",
            format!("failed to serialize protected config body: {err}"),
        )
    })
}

fn copy_forward_headers(source: &HeaderMap) -> HeaderMap {
    let mut target = HeaderMap::new();
    for (name, value) in source {
        if should_drop_request_header(name) {
            continue;
        }
        target.append(name.clone(), value.clone());
    }
    target
}

fn should_drop_request_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "host" | "content-length" | "cookie" | "connection"
    )
}

fn copy_response_headers(source: &HeaderMap, target: &mut HeaderMap) {
    for (name, value) in source {
        if should_drop_response_header(name) {
            continue;
        }
        target.append(name.clone(), value.clone());
    }
}

fn should_drop_response_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "content-length" | "transfer-encoding" | "connection"
    )
}

fn prune_expired_launch_tokens(tokens: &mut HashMap<String, BridgeLaunchTokenState>) {
    let now = now_unix_ms();
    tokens.retain(|_, token| token.expires_at_ms > now);
}

fn consume_launch_token(
    tokens: &mut HashMap<String, BridgeLaunchTokenState>,
    launch_token: &str,
) -> Option<BridgeLaunchTokenState> {
    prune_expired_launch_tokens(tokens);
    tokens.remove(launch_token)
}

struct BoundedBridgeResponseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    limit: usize,
    bytes_seen: usize,
    failed: bool,
}

impl BoundedBridgeResponseStream {
    fn new<S>(stream: S, limit: usize) -> Self
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            limit,
            bytes_seen: 0,
            failed: false,
        }
    }
}

impl Stream for BoundedBridgeResponseStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }
        match self.inner.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => {
                let next = self.bytes_seen.saturating_add(chunk.len());
                if next > self.limit {
                    self.failed = true;
                    let error = std::io::Error::other(format!(
                        "Codex retry gateway bridge response exceeds {} bytes",
                        self.limit
                    ));
                    return Poll::Ready(Some(Err(error)));
                }
                self.bytes_seen = next;
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.failed = true;
                let error = std::io::Error::other(format!(
                    "failed to stream Codex retry gateway bridge response: {error}"
                ));
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "error": message,
            })
            .to_string(),
        ))
        .unwrap_or_else(|_| Response::new(Body::from(message.to_string())))
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct AwaitedRestoreCallback {
        completed: Arc<AtomicBool>,
    }

    impl CodexRetryGatewayLifecycleCallback for AwaitedRestoreCallback {
        fn request_gateway_disable(
            &self,
            _request: CodexRetryGatewayRouteCallbackRequest,
        ) -> crate::infra::codex_retry_gateway::CodexRetryGatewayLifecycleFuture {
            let completed = self.completed.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                completed.store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    fn managed_record() -> CodexRetryGatewayManagedProcessRecord {
        CodexRetryGatewayManagedProcessRecord {
            pid: 1,
            start_identity: None,
            started_at_ms: 1,
            node_executable: "node".to_string(),
            source_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            source_dir_rel: "sources/ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            config_path_rel: "runtime/config/config.json".to_string(),
            state_path_rel: "runtime/state.json".to_string(),
            log_path_rel: "runtime/logs/gateway.log".to_string(),
            listener: "http://127.0.0.1:4610".to_string(),
            upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
            instance_nonce: "nonce".to_string(),
            provider_name: crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO
                .to_string(),
        }
    }

    #[test]
    fn path_allowlist_matches_documented_bridge_surface() {
        assert!(path_allowed(&Method::GET, "/__codex_retry_gateway/ui"));
        assert!(path_allowed(&Method::GET, "/favicon.ico"));
        assert!(path_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/config"
        ));
        assert!(path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/reasoning/export/jobs/job-1"
        ));
        assert!(path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/reasoning/export/jobs/job-1/download"
        ));
        assert!(path_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/analytics/imports/run"
        ));
        assert!(path_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/analytics/imports/analyze"
        ));
        assert!(path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/imports/latest"
        ));
        assert!(path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/imports/jobs/job-2"
        ));
        assert!(!path_allowed(
            &Method::DELETE,
            "/__codex_retry_gateway/api/config"
        ));
        assert!(!path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/unknown"
        ));
        assert!(!path_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/imports/jobs/job-2/download"
        ));
    }

    #[test]
    fn protected_config_body_rejects_managed_field_changes() {
        let record = managed_record();
        let err = protected_config_body(
            &record,
            br#"{"listen_port":4620,"upstream_base_url":"http://127.0.0.1:1/v1"}"#,
        )
        .expect_err("protected config should reject managed field changes");
        assert!(err.to_string().contains("managed by AIO"));
    }

    #[test]
    fn bridge_api_requests_require_verified_same_origin_context() {
        let mut headers = HeaderMap::new();
        assert!(request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/ui",
            &headers,
            "http://127.0.0.1:45100"
        ));

        headers.insert(SEC_FETCH_SITE, HeaderValue::from_static("cross-site"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("http://tauri.localhost/cli-manager/codex-gateway"),
        );
        assert!(request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/ui",
            &headers,
            "http://127.0.0.1:45100"
        ));
        assert!(!request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/reasoning/export",
            &headers,
            "http://127.0.0.1:45100"
        ));

        headers.remove(SEC_FETCH_SITE);
        headers.remove(REFERER);
        assert!(!request_origin_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/config",
            &headers,
            "http://127.0.0.1:45100"
        ));
        headers.insert(ORIGIN, HeaderValue::from_static("http://127.0.0.1:45101"));
        assert!(!request_origin_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/config",
            &headers,
            "http://127.0.0.1:45100"
        ));
        headers.insert(ORIGIN, HeaderValue::from_static("http://127.0.0.1:45100"));
        assert!(request_origin_allowed(
            &Method::POST,
            "/__codex_retry_gateway/api/config",
            &headers,
            "http://127.0.0.1:45100"
        ));

        headers.remove(ORIGIN);
        headers.insert(SEC_FETCH_SITE, HeaderValue::from_static("cross-site"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("http://127.0.0.1:45100/__codex_retry_gateway/ui"),
        );
        assert!(request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/reasoning/export",
            &headers,
            "http://127.0.0.1:45100"
        ));
        headers.insert(SEC_FETCH_SITE, HeaderValue::from_static("same-origin"));
        assert!(request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/analytics/reasoning/export",
            &headers,
            "http://127.0.0.1:45100"
        ));
        headers.remove(SEC_FETCH_SITE);
        assert!(request_origin_allowed(
            &Method::GET,
            "/__codex_retry_gateway/api/status",
            &headers,
            "http://127.0.0.1:45100"
        ));
    }

    #[tokio::test]
    async fn cross_site_ui_navigation_reaches_session_validation_but_api_does_not() {
        let paths = CodexRetryGatewayManagerPaths::from_root(
            tempfile::tempdir().unwrap().path().join("gateway"),
        );
        let state = Arc::new(BridgeRuntimeState {
            base_origin: "http://127.0.0.1:45100".to_string(),
            cookie_name: bridge_cookie_name(45100),
            paths: RwLock::new(paths),
            callback: RwLock::new(Arc::new(AwaitedRestoreCallback {
                completed: Arc::new(AtomicBool::new(false)),
            })),
            launch_tokens: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            client: Client::new(),
        });
        let mut headers = HeaderMap::new();
        headers.insert(SEC_FETCH_SITE, HeaderValue::from_static("cross-site"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("http://tauri.localhost/cli-manager/codex-gateway"),
        );

        let ui_response = proxy_request(
            State(state.clone()),
            Method::GET,
            headers.clone(),
            OriginalUri(axum::http::Uri::from_static("/__codex_retry_gateway/ui")),
            Body::empty(),
        )
        .await;
        assert_eq!(ui_response.status(), StatusCode::UNAUTHORIZED);

        let api_response = proxy_request(
            State(state),
            Method::GET,
            headers,
            OriginalUri(axum::http::Uri::from_static(
                "/__codex_retry_gateway/api/status",
            )),
            Body::empty(),
        )
        .await;
        assert_eq!(api_response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn launch_tokens_are_short_lived_and_single_use() {
        let identity = BridgeManagedIdentity {
            generation: 7,
            listener: "http://127.0.0.1:4610".to_string(),
            pid: 1,
            start_identity: Some(2),
            source_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            instance_nonce: "nonce".to_string(),
        };
        let mut tokens = HashMap::from([
            (
                "usable".to_string(),
                BridgeLaunchTokenState {
                    identity: identity.clone(),
                    view_id: "a".repeat(32),
                    expires_at_ms: now_unix_ms().saturating_add(60_000),
                },
            ),
            (
                "expired".to_string(),
                BridgeLaunchTokenState {
                    identity,
                    view_id: "b".repeat(32),
                    expires_at_ms: now_unix_ms().saturating_sub(1),
                },
            ),
        ]);

        assert!(consume_launch_token(&mut tokens, "expired").is_none());
        assert!(consume_launch_token(&mut tokens, "usable").is_some());
        assert!(consume_launch_token(&mut tokens, "usable").is_none());
    }

    #[test]
    fn bridge_session_cookie_name_isolated_by_listener_port() {
        let first = bridge_cookie_name(45100);
        let second = bridge_cookie_name(45101);
        assert_ne!(first, second);

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{first}=first-session; {second}=second-session"))
                .expect("cookie header"),
        );
        assert_eq!(
            read_session_cookie(&headers, &first).as_deref(),
            Some("first-session")
        );
        assert_eq!(
            read_session_cookie(&headers, &second).as_deref(),
            Some("second-session")
        );
    }

    #[test]
    fn bridge_session_cookie_supports_cross_site_iframe_requests() {
        let cookie = bridge_session_cookie("aio_bridge_45100", "session-id");
        assert_eq!(
            cookie,
            "aio_bridge_45100=session-id; HttpOnly; SameSite=None; Secure; Path=/"
        );
    }

    #[test]
    fn successful_status_and_config_responses_receive_aio_overlay() {
        assert!(response_requires_status_overlay(
            &Method::GET,
            "/__codex_retry_gateway/api/status",
            StatusCode::OK
        ));
        assert!(response_requires_status_overlay(
            &Method::POST,
            "/__codex_retry_gateway/api/config",
            StatusCode::OK
        ));
        assert!(!response_requires_status_overlay(
            &Method::POST,
            "/__codex_retry_gateway/api/config",
            StatusCode::BAD_REQUEST
        ));
    }

    #[test]
    fn status_overlay_uses_aio_route_provider_and_commit_state() {
        let record = managed_record();
        let status = CodexRetryGatewayStatus {
            generation: 7,
            desired_enabled: true,
            runtime_phase:
                crate::infra::codex_retry_gateway::CodexRetryGatewayRuntimePhase::Guarded,
            route_mode: CodexRouteMode::Guarded,
            cli_proxy_enabled: true,
            cli_proxy_applied: true,
            selected_commit: record.source_commit.clone(),
            active_commit: Some(record.source_commit.clone()),
            ..CodexRetryGatewayStatus::default()
        };
        let overlaid = overlay_authoritative_status(
            br#"{"ok":true,"config":{"upstream_base_url":"stale"},"state":{"provider_name":"stale"}}"#,
            &record,
            &status,
        )
        .expect("status overlay");
        let value: Value = serde_json::from_slice(&overlaid).expect("status json");

        assert_eq!(
            value["config"]["upstream_base_url"],
            record.upstream_base_url
        );
        assert_eq!(value["state"]["provider_name"], record.provider_name);
        assert_eq!(
            value["state"]["codex_current_base_url"],
            "http://127.0.0.1:4610/v1"
        );
        assert_eq!(value["aio"]["generation"], 7);
        assert_eq!(value["aio"]["route_mode"], "guarded");
        assert_eq!(value["aio"]["source_commit"], record.source_commit);
    }

    #[tokio::test]
    async fn reset_bridge_runtime_is_idempotent() {
        reset_bridge_runtime_for_tests()
            .await
            .expect("first reset succeeds");
        reset_bridge_runtime_for_tests()
            .await
            .expect("second reset succeeds");
    }

    #[tokio::test]
    async fn restore_waits_for_lifecycle_disable_before_success() {
        let completed = Arc::new(AtomicBool::new(false));
        let paths = CodexRetryGatewayManagerPaths::from_root(
            tempfile::tempdir().unwrap().path().join("gateway"),
        );
        let state = Arc::new(BridgeRuntimeState {
            base_origin: "http://127.0.0.1:45100".to_string(),
            cookie_name: bridge_cookie_name(45100),
            paths: RwLock::new(paths),
            callback: RwLock::new(Arc::new(AwaitedRestoreCallback {
                completed: completed.clone(),
            })),
            launch_tokens: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            client: Client::new(),
        });

        let response = handle_restore(&state, 7).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(completed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn bounded_response_reader_rejects_oversized_body() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tauri::async_runtime::spawn(async move {
            let router = Router::new().route(
                "/large",
                get(|| async move { Body::from(vec![b'x'; BRIDGE_RESPONSE_LIMIT_BYTES + 1]) }),
            );
            let _ = axum::serve(listener, router).await;
        });

        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/large"))
            .send()
            .await
            .unwrap();
        let err = read_bounded_response_bytes(response, BRIDGE_RESPONSE_LIMIT_BYTES)
            .await
            .expect_err("oversized upstream response must fail");
        assert!(err.to_string().contains("exceeds"));
        server.abort();
    }

    #[tokio::test]
    async fn bounded_response_reader_rejects_midstream_disconnect() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tauri::async_runtime::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\npartial",
                )
                .await
                .unwrap();
            let _ = socket.shutdown().await;
        });

        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/broken"))
            .send()
            .await
            .unwrap();
        let err = read_bounded_response_bytes(response, BRIDGE_RESPONSE_LIMIT_BYTES)
            .await
            .expect_err("truncated upstream response must fail");
        assert!(err.to_string().contains("failed to read upstream response"));
        server.await.unwrap();
    }
}
