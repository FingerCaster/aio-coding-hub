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
};
use crate::shared::error::{AppError, AppResult};
use axum::body::{to_bytes, Body};
use axum::extract::{OriginalUri, Path as AxumPath, State};
use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode};
use axum::routing::{any, get};
use axum::Router;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{oneshot, Mutex, RwLock};

const BRIDGE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_RESPONSE_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const BRIDGE_SESSION_TTL_MS: u64 = 15 * 60 * 1000;
const BRIDGE_COOKIE_NAME: &str = "aio_codex_retry_gateway_session";

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
struct BridgeSessionState {
    generation: u64,
    listener: String,
    pid: u32,
    start_identity: Option<u64>,
    source_commit: String,
    instance_nonce: String,
    expires_at_ms: u64,
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
    paths: RwLock<CodexRetryGatewayManagerPaths>,
    callback: RwLock<Arc<dyn CodexRetryGatewayLifecycleCallback>>,
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
    let session_id = crate::infra::codex_retry_gateway::util::random_hex(16);
    let expires_at_ms = now_unix_ms().saturating_add(BRIDGE_SESSION_TTL_MS);

    let runtime = bridge_runtime_slot().lock().await;
    let Some(inner) = runtime.as_ref() else {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_UNAVAILABLE",
            "bridge runtime is not running",
        ));
    };
    {
        let mut sessions = inner.state.sessions.write().await;
        prune_expired_sessions(&mut sessions);
        sessions.insert(
            session_id.clone(),
            BridgeSessionState {
                generation,
                listener: process.record.listener.clone(),
                pid: process.record.pid,
                start_identity: process.record.start_identity,
                source_commit: process.record.source_commit.clone(),
                instance_nonce: process.record.instance_nonce.clone(),
                expires_at_ms,
            },
        );
    }
    drop(runtime);

    let launch_url = format!("{}/launch/{}", handle.base_origin, session_id);
    Ok(BridgeDetailsSession {
        handle: handle.clone(),
        session: CodexRetryGatewayDetailsSession {
            generation,
            iframe_url: launch_url.clone(),
            browser_url: launch_url,
            expires_at_ms,
        },
    })
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

    let state = Arc::new(BridgeRuntimeState {
        paths: RwLock::new(paths),
        callback: RwLock::new(callback),
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
    AxumPath(session_id): AxumPath<String>,
) -> Response<Body> {
    match validate_session(&state, &session_id).await {
        Ok(_) => {
            let mut response = Response::builder()
                .status(StatusCode::FOUND)
                .header(LOCATION, "/__codex_retry_gateway/ui")
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
            if let Ok(value) = HeaderValue::from_str(&format!(
                "{BRIDGE_COOKIE_NAME}={session_id}; HttpOnly; SameSite=Strict; Path=/"
            )) {
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

    let Some(session_id) = read_session_cookie(&headers) else {
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
    let mut builder = Response::builder().status(status);
    if let Some(headers_mut) = builder.headers_mut() {
        copy_response_headers(&upstream_headers, headers_mut);
    }
    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        error_response(StatusCode::BAD_GATEWAY, "failed to build bridge response")
    })
}

async fn handle_restore(state: &Arc<BridgeRuntimeState>, generation: u64) -> Response<Body> {
    let callback = state.callback.read().await.clone();
    match callback.request_gateway_disable(CodexRetryGatewayRouteCallbackRequest {
        generation,
        reason: CodexRetryGatewayRouteCallbackReason::ExternalRestore,
    }) {
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
        let mut sessions = state.sessions.write().await;
        prune_expired_sessions(&mut sessions);
        sessions.get(session_id).cloned().ok_or_else(|| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_INVALID",
                "bridge session is missing or expired",
            )
        })?
    };

    let paths = state.paths.read().await.clone();
    let manager = read_manager_state(&paths)?;
    if manager.generation != session.generation {
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
    if record.pid != session.pid
        || record.listener != session.listener
        || record.instance_nonce != session.instance_nonce
        || record.source_commit != session.source_commit
        || record.start_identity != session.start_identity
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_BRIDGE_SESSION_STALE",
            "managed gateway process identity changed",
        ));
    }

    Ok(ValidatedBridgeRequest {
        generation: session.generation,
        record,
    })
}

fn read_session_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name.trim() == BRIDGE_COOKIE_NAME).then(|| value.trim().to_string())
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

fn prune_expired_sessions(sessions: &mut HashMap<String, BridgeSessionState>) {
    let now = now_unix_ms();
    sessions.retain(|_, session| session.expires_at_ms > now);
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        let record = CodexRetryGatewayManagedProcessRecord {
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
        };
        let err = protected_config_body(
            &record,
            br#"{"listen_port":4620,"upstream_base_url":"http://127.0.0.1:1/v1"}"#,
        )
        .expect_err("protected config should reject managed field changes");
        assert!(err.to_string().contains("managed by AIO"));
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
