use axum::{
    body::Body,
    extract::{Path, State},
    http::Request,
    response::Response,
    routing::{any, get},
    Json, Router,
};
use serde::Serialize;

use super::proxy::proxy_impl;
use super::runtime::GatewayAppState;
use super::util::now_unix_seconds;

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    app: &'static str,
    version: &'static str,
    ts: u64,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        app: "aio-coding-hub",
        version: env!("CARGO_PKG_VERSION"),
        ts: now_unix_seconds(),
    })
}

async fn root() -> &'static str {
    "AIO Coding Hub is running"
}

async fn proxy_cli_any<R>(
    State(state): State<GatewayAppState<R>>,
    Path((cli_key, path)): Path<(String, String)>,
    req: Request<Body>,
) -> Response
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    let forwarded_path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}")
    };
    proxy_impl(state, cli_key, forwarded_path, req).await
}

async fn proxy_cli_with_provider_any<R>(
    State(state): State<GatewayAppState<R>>,
    Path((cli_key, provider_id, path)): Path<(String, i64, String)>,
    mut req: Request<Body>,
) -> Response
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    if let Ok(value) = axum::http::HeaderValue::from_str(&provider_id.to_string()) {
        req.headers_mut().insert("x-aio-provider-id", value);
    }

    let forwarded_path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}")
    };

    proxy_impl(state, cli_key, forwarded_path, req).await
}

async fn proxy_openai_v1_any<R>(
    State(state): State<GatewayAppState<R>>,
    Path(path): Path<String>,
    req: Request<Body>,
) -> Response
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    let forwarded_path = if path.is_empty() {
        "/v1".to_string()
    } else {
        format!("/v1/{path}")
    };
    proxy_impl(state, "codex".to_string(), forwarded_path, req).await
}

async fn proxy_openai_v1_root<R>(
    State(state): State<GatewayAppState<R>>,
    req: Request<Body>,
) -> Response
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    proxy_impl(state, "codex".to_string(), "/v1".to_string(), req).await
}

pub(super) fn build_router<R>(state: GatewayAppState<R>) -> Router
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route(
            "/:cli_key/_aio/provider/:provider_id/*path",
            any(proxy_cli_with_provider_any::<R>),
        )
        .route("/v1", any(proxy_openai_v1_root::<R>))
        .route("/v1/*path", any(proxy_openai_v1_any::<R>))
        .route("/:cli_key/*path", any(proxy_cli_any::<R>))
        .with_state(state)
}

#[cfg(test)]
#[allow(clippy::await_holding_lock, clippy::field_reassign_with_default)]
mod tests {
    use super::build_router;
    use crate::app::plugins::official;
    use crate::domain::plugin_contributions::PluginContributes;
    use crate::domain::plugins::{
        PluginDetail, PluginHook, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use crate::gateway::codex_session_id::CodexSessionIdCache;
    use crate::gateway::plugins::context::{GatewayHookResult, GatewayPluginHookName};
    use crate::gateway::plugins::pipeline::{
        GatewayPluginPipeline, GatewayPluginPipelineConfig, InMemoryGatewayPluginExecutor,
    };
    use crate::gateway::proxy::{ProviderBaseUrlPingCache, RecentErrorCache};
    use crate::gateway::runtime::GatewayAppState;
    use crate::infra::plugins::repository;
    use crate::{circuit_breaker, db, providers, request_logs, session_manager, settings};
    use axum::body::HttpBody;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use futures_core::Stream;
    use serde_json::Value;
    use std::collections::{BTreeMap, HashMap};
    use std::ffi::OsString;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tauri::Manager;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tower::ServiceExt;

    #[derive(Default)]
    struct EnvRestore {
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvRestore {
        fn save_once(&mut self, key: &'static str) {
            if self.saved.iter().any(|(saved, _)| *saved == key) {
                return;
            }
            self.saved.push((key, std::env::var_os(key)));
        }

        fn set_var(&mut self, key: &'static str, value: impl Into<OsString>) {
            self.save_once(key);
            std::env::set_var(key, value.into());
        }

        fn remove_var(&mut self, key: &'static str) {
            self.save_once(key);
            std::env::remove_var(key);
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            settings::clear_cache();
        }
    }

    fn isolate_app_env(home: &std::path::Path) -> EnvRestore {
        let mut env = EnvRestore::default();
        let home_os = home.as_os_str().to_os_string();
        env.set_var("HOME", home_os.clone());
        env.set_var("AIO_CODING_HUB_HOME_DIR", home_os.clone());
        env.set_var("USERPROFILE", home_os);
        env.set_var("AIO_CODING_HUB_DOTDIR_NAME", ".aio-coding-hub-route-test");
        env.remove_var("CODEX_HOME");
        settings::clear_cache();
        env
    }

    async fn spawn_hanging_upstream() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream stub");
        let addr = listener.local_addr().expect("upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_json_upstream(body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind json upstream stub");
        let addr = listener.local_addr().expect("json upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_counting_status_upstream(
        status: StatusCode,
        body: &'static str,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind counting status upstream stub");
        let addr = listener
            .local_addr()
            .expect("counting status upstream addr");
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task_call_count = Arc::clone(&call_count);
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                task_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown"),
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), call_count, task)
    }

    struct GatedCountingStatusUpstream {
        base_url: String,
        call_count: Arc<std::sync::atomic::AtomicUsize>,
        first_request_accepted: Option<tokio::sync::oneshot::Receiver<()>>,
        release_first_response: Option<tokio::sync::oneshot::Sender<()>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl GatedCountingStatusUpstream {
        async fn wait_for_first_request(&mut self) {
            let accepted = self
                .first_request_accepted
                .take()
                .expect("first request acceptance receiver");
            tokio::time::timeout(Duration::from_secs(3), accepted)
                .await
                .expect("first gated upstream request timeout")
                .expect("first gated upstream request signal");
        }

        fn release_first_response(&mut self) {
            self.release_first_response
                .take()
                .expect("first response release sender")
                .send(())
                .expect("release first gated upstream response");
        }

        fn calls(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Drop for GatedCountingStatusUpstream {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn spawn_gated_counting_status_upstream(
        status: StatusCode,
        body: &'static str,
    ) -> GatedCountingStatusUpstream {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind gated counting status upstream stub");
        let addr = listener
            .local_addr()
            .expect("gated counting status upstream addr");
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task_call_count = Arc::clone(&call_count);
        let (first_request_tx, first_request_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut first_request_tx = Some(first_request_tx);
            let mut release_first_rx = Some(release_first_rx);
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let call_number =
                    task_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if call_number == 1 {
                    let _ = first_request_tx
                        .take()
                        .expect("first request acceptance sender")
                        .send(());
                    if release_first_rx
                        .take()
                        .expect("first response release receiver")
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                let response = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown"),
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        GatedCountingStatusUpstream {
            base_url: format!("http://{addr}"),
            call_count,
            first_request_accepted: Some(first_request_rx),
            release_first_response: Some(release_first_tx),
            task,
        }
    }

    async fn spawn_retry_rule_upstream(
        status_line: &'static str,
        error_body: Vec<u8>,
        gzip_error: bool,
        success_body: &'static str,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retry-rule upstream stub");
        let addr = listener.local_addr().expect("retry-rule upstream addr");
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task_call_count = Arc::clone(&call_count);
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                task_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if index == 0 {
                    let content_encoding = if gzip_error {
                        "content-encoding: gzip\r\n"
                    } else {
                        ""
                    };
                    let headers = format!(
                        "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\n{content_encoding}content-length: {}\r\nconnection: close\r\n\r\n",
                        error_body.len()
                    );
                    let _ = socket.write_all(headers.as_bytes()).await;
                    let _ = socket.write_all(&error_body).await;
                } else {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        success_body.len(),
                        success_body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), call_count, task)
    }

    #[derive(Debug)]
    struct CapturedRawRequest {
        head: String,
        body: Vec<u8>,
    }

    impl CapturedRawRequest {
        fn text(&self) -> String {
            let mut out = self.head.clone();
            out.push_str("\r\n\r\n");
            out.push_str(&String::from_utf8_lossy(&self.body));
            out
        }

        fn has_header_line(&self, needle: &str) -> bool {
            self.head
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        }
    }

    fn find_http_head_split(bytes: &[u8]) -> Option<(usize, usize)> {
        let marker = b"\r\n\r\n";
        bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .map(|idx| (idx, idx + marker.len()))
    }

    async fn read_complete_http_request_bytes(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 1024];
        while let Ok(size) = socket.read(&mut chunk).await {
            if size == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..size]);
            if buf.len() > 64 * 1024 {
                break;
            }

            let Some((head_start, body_start)) = find_http_head_split(&buf) else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buf[..head_start]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if buf.len().saturating_sub(body_start) >= content_length {
                break;
            }
        }
        buf
    }

    fn split_raw_http_request(bytes: Vec<u8>) -> CapturedRawRequest {
        let Some((head_start, body_start)) = find_http_head_split(&bytes) else {
            return CapturedRawRequest {
                head: String::from_utf8_lossy(&bytes).to_string(),
                body: Vec::new(),
            };
        };
        CapturedRawRequest {
            head: String::from_utf8_lossy(&bytes[..head_start]).to_string(),
            body: bytes[body_start..].to_vec(),
        }
    }

    async fn read_complete_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let buf = read_complete_http_request_bytes(socket).await;
        String::from_utf8_lossy(&buf).to_string()
    }

    async fn spawn_capturing_json_upstream(
        body: impl Into<String>,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let body = body.into();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind capturing json upstream stub");
        let addr = listener.local_addr().expect("capturing upstream addr");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let request = read_complete_http_request(&mut socket).await;
                let captured_body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, body)| body.to_string())
                    .unwrap_or_default();
                let _ = tx.send(captured_body);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), rx, task)
    }

    async fn spawn_capturing_raw_upstream(
        body: &'static str,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<CapturedRawRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind capturing raw upstream stub");
        let addr = listener.local_addr().expect("capturing raw upstream addr");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let request =
                    split_raw_http_request(read_complete_http_request_bytes(&mut socket).await);
                let _ = tx.send(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), rx, task)
    }

    async fn spawn_previous_response_retry_upstream(
        success_body: &'static str,
    ) -> (
        String,
        tokio::sync::mpsc::Receiver<CapturedRawRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retry upstream stub");
        let addr = listener.local_addr().expect("retry upstream addr");
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let request =
                    split_raw_http_request(read_complete_http_request_bytes(&mut socket).await);
                let _ = tx.send(request).await;
                let (status_line, body) = if index == 0 {
                    (
                        "400 Bad Request",
                        r#"{"error":{"message":"No response found for previous_response_id resp_old","param":"previous_response_id"}}"#,
                    )
                } else {
                    ("200 OK", success_body)
                };
                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), rx, task)
    }

    async fn spawn_previous_response_then_retry_rule_upstream(
        success_body: &'static str,
    ) -> (
        String,
        tokio::sync::mpsc::Receiver<CapturedRawRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind internal-plus-configured retry upstream stub");
        let addr = listener
            .local_addr()
            .expect("internal-plus-configured retry upstream addr");
        let (tx, rx) = tokio::sync::mpsc::channel(3);
        let task = tokio::spawn(async move {
            for index in 0..3 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let request =
                    split_raw_http_request(read_complete_http_request_bytes(&mut socket).await);
                let _ = tx.send(request).await;
                let (status_line, body) = match index {
                    0 => (
                        "400 Bad Request",
                        r#"{"error":{"message":"No response found for previous_response_id resp_old","param":"previous_response_id"}}"#,
                    ),
                    1 => (
                        "503 Service Unavailable",
                        r#"{"error":"temporarily unavailable"}"#,
                    ),
                    _ => ("200 OK", success_body),
                };
                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), rx, task)
    }

    fn gzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    fn zstd_bytes(input: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(input, 3).expect("zstd encode")
    }

    fn brotli_bytes(input: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
            encoder.write_all(input).expect("brotli write");
        }
        output
    }

    async fn spawn_status_upstream(
        status_line: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind status upstream stub");
        let addr = listener.local_addr().expect("status upstream addr");
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_status_json_upstream(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        spawn_status_upstream(status_line, "application/json", body).await
    }

    async fn spawn_large_known_length_error_upstream(
        status_line: &'static str,
        declared_content_length: usize,
        sent_body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind large error upstream stub");
        let addr = listener.local_addr().expect("large error upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let headers = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {declared_content_length}\r\nconnection: keep-alive\r\n\r\n"
                );
                let _ = socket.write_all(headers.as_bytes()).await;
                let _ = socket.write_all(&sent_body).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_unknown_length_json_upstream(
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind unknown-length json upstream stub");
        let addr = listener
            .local_addr()
            .expect("unknown-length json upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{}",
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_sse_upstream(body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sse upstream stub");
        let addr = listener.local_addr().expect("sse upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_retrying_sse_upstream(
        first_body: Vec<u8>,
        gzip_first: bool,
        success_body: &'static str,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retrying sse upstream stub");
        let addr = listener.local_addr().expect("retrying sse upstream addr");
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task_call_count = Arc::clone(&call_count);
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                task_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if index == 0 {
                    let content_encoding = if gzip_first {
                        "content-encoding: gzip\r\n"
                    } else {
                        ""
                    };
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\n{content_encoding}content-length: {}\r\nconnection: close\r\n\r\n",
                        first_body.len()
                    );
                    let _ = socket.write_all(headers.as_bytes()).await;
                    let _ = socket.write_all(&first_body).await;
                } else {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        success_body.len(),
                        success_body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), call_count, task)
    }

    async fn spawn_retrying_chunked_sse_upstream(
        metadata_chunk: &'static str,
        error_chunk: &'static str,
        delay: Duration,
        success_body: &'static str,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retrying chunked sse upstream stub");
        let addr = listener
            .local_addr()
            .expect("retrying chunked sse upstream addr");
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task_call_count = Arc::clone(&call_count);
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                task_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if index == 0 {
                    let headers = concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "content-type: text/event-stream; charset=utf-8\r\n",
                        "transfer-encoding: chunked\r\n",
                        "connection: close\r\n",
                        "\r\n"
                    );
                    let _ = socket.write_all(headers.as_bytes()).await;
                    let metadata = format!("{:X}\r\n{}\r\n", metadata_chunk.len(), metadata_chunk);
                    let _ = socket.write_all(metadata.as_bytes()).await;
                    tokio::time::sleep(delay).await;
                    let error = format!("{:X}\r\n{}\r\n0\r\n\r\n", error_chunk.len(), error_chunk);
                    let _ = socket.write_all(error.as_bytes()).await;
                } else {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        success_body.len(),
                        success_body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), call_count, task)
    }

    async fn spawn_stalling_sse_upstream(
        first_chunk: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalling sse upstream stub");
        let addr = listener.local_addr().expect("stalling sse upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let headers = concat!(
                    "HTTP/1.1 200 OK\r\n",
                    "content-type: text/event-stream; charset=utf-8\r\n",
                    "transfer-encoding: chunked\r\n",
                    "connection: keep-alive\r\n",
                    "\r\n"
                );
                let _ = socket.write_all(headers.as_bytes()).await;
                let chunk = format!("{:X}\r\n{}\r\n", first_chunk.len(), first_chunk);
                let _ = socket.write_all(chunk.as_bytes()).await;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_delayed_chunked_sse_upstream(
        first_chunk: &'static str,
        second_chunk: &'static str,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind delayed sse upstream stub");
        let addr = listener.local_addr().expect("delayed sse upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let headers = concat!(
                    "HTTP/1.1 200 OK\r\n",
                    "content-type: text/event-stream; charset=utf-8\r\n",
                    "transfer-encoding: chunked\r\n",
                    "connection: close\r\n",
                    "\r\n"
                );
                let _ = socket.write_all(headers.as_bytes()).await;
                let first = format!("{:X}\r\n{}\r\n", first_chunk.len(), first_chunk);
                let _ = socket.write_all(first.as_bytes()).await;
                tokio::time::sleep(delay).await;
                let second = format!("{:X}\r\n{}\r\n0\r\n\r\n", second_chunk.len(), second_chunk);
                let _ = socket.write_all(second.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    async fn spawn_delayed_chunked_json_upstream(
        first_chunk: Vec<u8>,
        second_chunk: Vec<u8>,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind delayed chunked json upstream stub");
        let addr = listener
            .local_addr()
            .expect("delayed chunked json upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let headers = concat!(
                    "HTTP/1.1 200 OK\r\n",
                    "content-type: application/json\r\n",
                    "transfer-encoding: chunked\r\n",
                    "connection: close\r\n",
                    "\r\n"
                );
                let _ = socket.write_all(headers.as_bytes()).await;

                let first_len = format!("{:X}\r\n", first_chunk.len());
                let _ = socket.write_all(first_len.as_bytes()).await;
                let _ = socket.write_all(&first_chunk).await;
                let _ = socket.write_all(b"\r\n").await;

                tokio::time::sleep(delay).await;

                let second_len = format!("{:X}\r\n", second_chunk.len());
                let _ = socket.write_all(second_len.as_bytes()).await;
                let _ = socket.write_all(&second_chunk).await;
                let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    fn insert_provider_with_priority(
        db: &db::Db,
        cli_key: &str,
        name: &str,
        base_url: String,
        priority: i64,
    ) -> i64 {
        insert_provider_with_priority_and_extensions(db, cli_key, name, base_url, priority, None)
    }

    fn insert_provider_with_priority_and_extensions(
        db: &db::Db,
        cli_key: &str,
        name: &str,
        base_url: String,
        priority: i64,
        extension_values: Option<Vec<providers::ProviderExtensionValuesInput>>,
    ) -> i64 {
        let provider_id = providers::upsert(
            db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: cli_key.to_string(),
                name: name.to_string(),
                base_urls: vec![base_url],
                base_url_mode: providers::ProviderBaseUrlMode::Order,
                auth_mode: None,
                api_key: Some("sk-test".to_string()),
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(priority),
                claude_models: None,
                availability_test_model: None,
                limit_5h_usd: None,
                limit_daily_usd: None,
                daily_reset_mode: None,
                daily_reset_time: None,
                limit_weekly_usd: None,
                limit_monthly_usd: None,
                limit_total_usd: None,
                tags: None,
                note: None,
                source_provider_id: None,
                bridge_type: None,
                stream_idle_timeout_seconds: None,
                extension_values,
                account_usage_credentials_patch: None,
                account_usage_credentials_copy_from_provider_id: None,
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
                model_routing_policy_override: None,
                model_routing_policy_override_specified: false,
            },
        )
        .expect("insert provider")
        .id;
        append_default_route_provider(db, cli_key, provider_id);
        provider_id
    }

    fn append_default_route_provider(db: &db::Db, cli_key: &str, provider_id: i64) {
        let mut provider_ids: Vec<i64> = providers::default_route_list(db, cli_key)
            .expect("list default route")
            .into_iter()
            .map(|row| row.provider_id)
            .collect();
        provider_ids.push(provider_id);
        providers::default_route_set_order(db, cli_key, provider_ids)
            .expect("append default route provider");
    }

    fn insert_codex_provider_with_priority(
        db: &db::Db,
        name: &str,
        base_url: String,
        priority: i64,
    ) -> i64 {
        insert_provider_with_priority(db, "codex", name, base_url, priority)
    }

    fn insert_codex_provider(db: &db::Db, base_url: String) -> i64 {
        insert_codex_provider_with_priority(db, "Timeout Stub", base_url, 0)
    }

    fn insert_confirmed_custom_provider_with_priority(
        db: &db::Db,
        name: &str,
        model_base_url: String,
        priority: i64,
    ) -> i64 {
        let provider_uuid = crate::shared::uuid::new_uuid_v4();
        let account_base_url = "https://account-usage.example.test/v1";
        let mut extension_values = Some(vec![providers::ProviderExtensionValuesInput {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": "custom",
                "newApiQueryMode": "billing",
                "refreshIntervalSeconds": 300,
                "timedRefreshEnabled": false,
                "routeGateEnabled": true,
                "customScript": "({ request: () => ({}), parse: () => ({ status: 'available' }) })",
                "customAllowedOrigins": [],
                "customTimeoutSeconds": 5,
                "customEnabled": true,
            }),
        }]);
        let scope = crate::domain::provider_account_usage::custom_account_usage_permission_scope(
            &provider_uuid,
            "api_key",
            None,
            account_base_url,
        )
        .expect("custom permission scope");
        let permission =
            crate::domain::provider_account_usage::custom_account_usage_permission_request(
                extension_values.as_deref(),
                &scope,
            )
            .expect("custom permission request")
            .expect("enabled custom adapter requires permission");
        crate::domain::provider_account_usage::add_custom_account_usage_permission_proof(
            &mut extension_values,
            &permission.fingerprint,
            &permission.base_origin,
        )
        .expect("custom permission proof");

        let provider_id = providers::upsert_with_provider_uuid(
            db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: "codex".to_string(),
                name: name.to_string(),
                base_urls: vec![account_base_url.to_string(), model_base_url],
                base_url_mode: providers::ProviderBaseUrlMode::Ping,
                auth_mode: None,
                api_key: Some("sk-test".to_string()),
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(priority),
                claude_models: None,
                availability_test_model: None,
                limit_5h_usd: None,
                limit_daily_usd: None,
                daily_reset_mode: None,
                daily_reset_time: None,
                limit_weekly_usd: None,
                limit_monthly_usd: None,
                limit_total_usd: None,
                tags: None,
                note: None,
                source_provider_id: None,
                bridge_type: None,
                stream_idle_timeout_seconds: None,
                extension_values,
                account_usage_credentials_patch: None,
                account_usage_credentials_copy_from_provider_id: None,
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
                model_routing_policy_override: None,
                model_routing_policy_override_specified: false,
            },
            Some(provider_uuid),
        )
        .expect("insert confirmed custom provider")
        .id;
        append_default_route_provider(db, "codex", provider_id);
        provider_id
    }

    fn account_usage_route_extension() -> Vec<providers::ProviderExtensionValuesInput> {
        vec![providers::ProviderExtensionValuesInput {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": "sub2api",
                "newApiQueryMode": "billing",
                "refreshIntervalSeconds": 300,
                "timedRefreshEnabled": false,
                "routeGateEnabled": true,
            }),
        }]
    }

    fn account_usage_route_result(
        status: crate::domain::provider_account_usage::ProviderAccountUsageStatus,
        balance: Option<f64>,
        last_fetched_at: i64,
    ) -> crate::domain::provider_account_usage::ProviderAccountUsageResult {
        account_usage_route_result_for_adapter(
            crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind::Sub2api,
            status,
            balance,
            last_fetched_at,
        )
    }

    fn account_usage_route_result_for_adapter(
        adapter_kind: crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind,
        status: crate::domain::provider_account_usage::ProviderAccountUsageStatus,
        balance: Option<f64>,
        last_fetched_at: i64,
    ) -> crate::domain::provider_account_usage::ProviderAccountUsageResult {
        crate::domain::provider_account_usage::ProviderAccountUsageResult {
            adapter_kind: Some(adapter_kind),
            status,
            freshness: crate::domain::provider_account_usage::ProviderAccountUsageFreshness::Fresh,
            plan_name: None,
            balance,
            plan_remaining: None,
            used: None,
            total: None,
            unit: None,
            unit_note: None,
            daily_used: None,
            daily_total: None,
            weekly_used: None,
            weekly_total: None,
            monthly_used: None,
            monthly_total: None,
            expires_at: None,
            last_fetched_at: Some(last_fetched_at),
            message: None,
        }
    }

    fn insert_managed_codex_model(db: &db::Db, provider_id: i64, remote_model_id: &str) -> String {
        let conn = db.open_connection().expect("open provider db");
        let provider = crate::providers::get_by_id(&conn, provider_id).expect("load provider");
        drop(conn);
        let catalog = crate::domain::provider_models::manual_upsert(
            db,
            provider_id,
            &provider.provider_uuid,
            remote_model_id,
        )
        .expect("insert managed Codex model");
        let model = catalog
            .models
            .iter()
            .find(|model| model.remote_model_id == remote_model_id)
            .expect("managed model catalog entry");
        format!("aio/{}", model.model_uuid)
    }

    fn insert_managed_codex_profile_alias(
        db: &db::Db,
        provider_id: i64,
        remote_model_id: &str,
        profile_name_key: &str,
    ) -> String {
        let legacy_alias = insert_managed_codex_model(db, provider_id, remote_model_id);
        let model_uuid = legacy_alias
            .strip_prefix("aio/")
            .expect("legacy managed alias");
        let conn = db.open_connection().expect("open provider db");
        conn.execute(
            r#"
INSERT INTO codex_managed_profiles(
  profile_uuid, profile_name, profile_name_key, model_uuid,
  codex_home_path, content_sha256, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, 'C:\codex', ?5, 1, 1)
"#,
            rusqlite::params![
                crate::shared::uuid::new_uuid_v4(),
                profile_name_key,
                profile_name_key,
                model_uuid,
                "a".repeat(64)
            ],
        )
        .expect("insert managed Codex profile alias");
        format!("aio/{profile_name_key}")
    }

    fn disable_upstream_retry_policy(settings: &mut settings::AppSettings) {
        settings.upstream_retry_policy.enabled = false;
    }

    fn insert_codex_oauth_provider_with_priority(db: &db::Db, name: &str, priority: i64) -> i64 {
        insert_codex_oauth_provider_with_base_urls(db, name, Vec::new(), priority)
    }

    fn insert_codex_oauth_provider_with_base_urls(
        db: &db::Db,
        name: &str,
        base_urls: Vec<String>,
        priority: i64,
    ) -> i64 {
        let provider_id = providers::upsert(
            db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: "codex".to_string(),
                name: name.to_string(),
                base_urls,
                base_url_mode: providers::ProviderBaseUrlMode::Order,
                auth_mode: Some(providers::ProviderAuthMode::Oauth),
                api_key: None,
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(priority),
                claude_models: None,
                availability_test_model: None,
                limit_5h_usd: None,
                limit_daily_usd: None,
                daily_reset_mode: None,
                daily_reset_time: None,
                limit_weekly_usd: None,
                limit_monthly_usd: None,
                limit_total_usd: None,
                tags: None,
                note: None,
                source_provider_id: None,
                bridge_type: None,
                stream_idle_timeout_seconds: None,
                extension_values: None,
                account_usage_credentials_patch: None,
                account_usage_credentials_copy_from_provider_id: None,
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
                model_routing_policy_override: None,
                model_routing_policy_override_specified: false,
            },
        )
        .expect("insert oauth provider")
        .id;
        providers::update_oauth_tokens(
            db,
            provider_id,
            "oauth",
            "codex_oauth",
            "access-token",
            None,
            None,
            "https://auth.openai.com/oauth/token",
            "test-client-id",
            None,
            Some(crate::shared::time::now_unix_seconds() + 3_600),
            None,
        )
        .expect("seed oauth token");
        append_default_route_provider(db, "codex", provider_id);
        provider_id
    }

    fn insert_cx2cc_bridge_provider(db: &db::Db, source_provider_id: i64, priority: i64) -> i64 {
        let provider_id = providers::upsert(
            db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: "claude".to_string(),
                name: "CX2CC Bridge Stub".to_string(),
                base_urls: vec![],
                base_url_mode: providers::ProviderBaseUrlMode::Order,
                auth_mode: None,
                api_key: None,
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(priority),
                claude_models: None,
                availability_test_model: None,
                limit_5h_usd: None,
                limit_daily_usd: None,
                daily_reset_mode: None,
                daily_reset_time: None,
                limit_weekly_usd: None,
                limit_monthly_usd: None,
                limit_total_usd: None,
                tags: None,
                note: None,
                source_provider_id: Some(source_provider_id),
                bridge_type: Some("cx2cc".to_string()),
                stream_idle_timeout_seconds: None,
                extension_values: None,
                account_usage_credentials_patch: None,
                account_usage_credentials_copy_from_provider_id: None,
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
                model_routing_policy_override: None,
                model_routing_policy_override_specified: false,
            },
        )
        .expect("insert cx2cc bridge provider")
        .id;
        append_default_route_provider(db, "claude", provider_id);
        provider_id
    }

    async fn recv_terminal_request_log(
        log_rx: &mut tokio::sync::mpsc::Receiver<request_logs::RequestLogInsert>,
    ) -> request_logs::RequestLogInsert {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let log = log_rx.recv().await.expect("request log item");
                if log.status.is_some() {
                    break log;
                }
            }
        })
        .await
        .expect("terminal request log enqueue")
    }

    async fn run_encoded_codex_route(
        db_name: &str,
        forwarded_path: &str,
        content_encoding: &'static str,
        encoded_body: Vec<u8>,
        response_body: &'static str,
    ) -> (CapturedRawRequest, request_logs::RequestLogInsert) {
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join(db_name)).expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(response_body).await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}{forwarded_path}"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, content_encoding)
            .body(Body::from(encoded_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            status,
            StatusCode::OK,
            "response body: {}",
            String::from_utf8_lossy(&body)
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");
        let request_log = recv_terminal_request_log(&mut log_rx).await;
        upstream_task.abort();
        (captured, request_log)
    }

    async fn run_rejected_encoded_codex_route(
        db_name: &str,
        content_encoding: &'static str,
        encoded_body: Vec<u8>,
        max_request_body_mb: Option<&str>,
    ) -> (StatusCode, Value, request_logs::RequestLogInsert) {
        let home = tempfile::tempdir().expect("home dir");
        let mut env = isolate_app_env(home.path());
        if let Some(limit) = max_request_body_mb {
            env.set_var("AIO_GATEWAY_MAX_REQUEST_BODY_MB", limit);
        }
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join(db_name)).expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"must-not-arrive"}"#).await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, content_encoding)
            .body(Body::from(encoded_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let payload = serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("gateway error JSON");
        assert!(
            tokio::time::timeout(Duration::from_millis(150), captured_rx)
                .await
                .is_err(),
            "invalid encoded request unexpectedly reached upstream"
        );
        let request_log = recv_terminal_request_log(&mut log_rx).await;
        upstream_task.abort();
        (status, payload, request_log)
    }

    fn parse_special_settings(log: &request_logs::RequestLogInsert) -> Vec<Value> {
        let raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        match serde_json::from_str::<Value>(raw).expect("special settings json parses") {
            Value::Array(values) => values,
            _ => panic!("special settings json must be an array"),
        }
    }

    fn has_upstream_error_response_rule_marker(log: &request_logs::RequestLogInsert) -> bool {
        let Some(raw) = log.special_settings_json.as_deref() else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return false;
        };
        value.as_array().is_some_and(|settings| {
            settings.iter().any(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("upstream_error_response_rule")
            })
        })
    }

    fn test_upstream_error_response_rule(
        upstream_status: u16,
        status_behavior: settings::UpstreamErrorStatusBehavior,
        message_behavior: settings::UpstreamErrorMessageBehavior,
    ) -> settings::UpstreamErrorResponseRule {
        settings::UpstreamErrorResponseRule {
            id: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51".to_string(),
            name: "route response rule".to_string(),
            description: String::new(),
            enabled: true,
            priority: 10,
            status_codes: vec![upstream_status],
            keywords: Vec::new(),
            match_mode: settings::UpstreamErrorResponseMatchMode::Any,
            cli_keys: vec!["codex".to_string()],
            provider_ids: Vec::new(),
            status_behavior,
            message_behavior,
        }
    }

    struct CodexErrorResponseRuleObservation {
        status: StatusCode,
        response: Value,
        log: request_logs::RequestLogInsert,
        provider_id: i64,
    }

    async fn run_codex_error_response_rule_route(
        upstream_status: StatusCode,
        upstream_body: &'static str,
        rule: settings::UpstreamErrorResponseRule,
    ) -> CodexErrorResponseRuleObservation {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_error_response_rules = vec![rule];
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-error-response-rule.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, call_count, upstream_task) =
            spawn_counting_status_upstream(upstream_status, upstream_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Response Rule Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-response-rule","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let response = serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("response JSON");
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
        upstream_task.abort();

        CodexErrorResponseRuleObservation {
            status,
            response,
            log,
            provider_id,
        }
    }

    fn assert_managed_codex_matched_route_log(
        log: &request_logs::RequestLogInsert,
        canonical_model: &str,
        provider_id: i64,
        remote_model_id: &str,
    ) {
        assert_eq!(log.requested_model.as_deref(), Some(canonical_model));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0]
                .get("requested_upstream_model")
                .and_then(Value::as_str),
            Some(remote_model_id)
        );

        let special_settings = parse_special_settings(log);
        assert!(!special_settings.iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));
        let managed_route = special_settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("aio_managed_model_route")
            })
            .expect("managed route setting");
        assert_eq!(
            managed_route.get("canonicalModel").and_then(Value::as_str),
            Some(canonical_model)
        );
        assert_eq!(
            managed_route.get("providerId").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            managed_route.get("remoteModelId").and_then(Value::as_str),
            Some(remote_model_id)
        );
        assert_eq!(
            managed_route
                .get("requestedUpstreamModel")
                .and_then(Value::as_str),
            Some(remote_model_id)
        );
        assert_eq!(
            managed_route.get("applied").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            managed_route.get("observation").and_then(Value::as_str),
            Some("matched")
        );
    }

    async fn assert_no_additional_terminal_request_log(
        log_rx: &mut tokio::sync::mpsc::Receiver<request_logs::RequestLogInsert>,
    ) {
        let duplicate_terminal = tokio::time::timeout(Duration::from_millis(100), async {
            while let Some(item) = log_rx.recv().await {
                if item.status.is_some() {
                    return Some(item);
                }
            }
            None
        })
        .await;
        assert!(
            !matches!(duplicate_terminal, Ok(Some(_))),
            "managed route must emit exactly one terminal request log"
        );
    }

    fn gateway_state(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
    ) -> GatewayAppState<tauri::test::MockRuntime> {
        gateway_state_with_parts(
            app,
            db,
            log_tx,
            Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig::default(),
                HashMap::new(),
                None,
            )),
            Arc::new(session_manager::SessionManager::new()),
        )
    }

    fn gateway_state_with_parts(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
        circuit: Arc<circuit_breaker::CircuitBreaker>,
        session: Arc<session_manager::SessionManager>,
    ) -> GatewayAppState<tauri::test::MockRuntime> {
        GatewayAppState {
            app,
            db,
            log_tx,
            circuit,
            session,
            codex_session_cache: Arc::new(Mutex::new(CodexSessionIdCache::default())),
            recent_errors: Arc::new(Mutex::new(RecentErrorCache::default())),
            latency_cache: Arc::new(Mutex::new(ProviderBaseUrlPingCache::default())),
            plugin_pipeline: GatewayPluginPipeline::empty_shared(),
            internal_reentry: Arc::new(
                crate::gateway::internal_reentry::InternalReentryRegistry::default(),
            ),
            http_client_override: Some(
                reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("route tests direct http client"),
            ),
            active_requests: Arc::new(
                crate::gateway::active_requests::ActiveRequestRegistry::default(),
            ),
        }
    }

    struct GrokJsonRouteObservation {
        captured: CapturedRawRequest,
        response: Value,
        log: request_logs::RequestLogInsert,
        provider_id: i64,
    }

    struct GrokErrorRouteObservation {
        response: Value,
        log: request_logs::RequestLogInsert,
        provider_id: i64,
    }

    async fn run_grok_json_route(
        route_path: &'static str,
        request_body: &'static str,
        response_body: &'static str,
    ) -> GrokJsonRouteObservation {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        settings::write(&app_handle, &settings::AppSettings::default()).expect("write settings");
        let proxy_result =
            crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
                .expect("enable Grok CLI proxy");
        assert!(proxy_result.ok, "{}", proxy_result.message);

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-json.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(response_body).await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok JSON Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(route_path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer client-placeholder")
            .header("x-api-key", "client-placeholder")
            .header("x-grok-session-id", "grok-session-route")
            .header("x-grok-conv-id", "grok-conversation-route")
            .header("x-grok-req-id", "grok-request-route")
            .body(Body::from(request_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let response = serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("JSON response");
        let captured = captured_rx.await.expect("captured upstream request");
        let log = recv_terminal_request_log(&mut log_rx).await;
        upstream_task.abort();

        GrokJsonRouteObservation {
            captured,
            response,
            log,
            provider_id,
        }
    }

    async fn run_grok_error_route(
        status_line: &'static str,
        content_type: &'static str,
        upstream_body: &'static str,
    ) -> GrokErrorRouteObservation {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.circuit_breaker_failure_threshold = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        let proxy_result =
            crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
                .expect("enable Grok CLI proxy");
        assert!(proxy_result.ok, "{}", proxy_result.message);
        assert_eq!(
            settings::read(&app_handle)
                .expect("read settings after enabling Grok proxy")
                .failover_max_attempts_per_provider,
            1,
            "enabling Grok proxy must preserve unrelated gateway settings"
        );

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-error.sqlite"))
            .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_status_upstream(status_line, content_type, upstream_body).await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok Error Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig {
                    failure_threshold: 1,
                    ..circuit_breaker::CircuitBreakerConfig::default()
                },
                HashMap::new(),
                None,
            )),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/grok/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"grok-error-model","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let response = serde_json::from_slice::<Value>(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("gateway error JSON");
        let log = recv_terminal_request_log(&mut log_rx).await;
        upstream_task.abort();

        GrokErrorRouteObservation {
            response,
            log,
            provider_id,
        }
    }

    fn assert_grok_error_observation(
        observation: &GrokErrorRouteObservation,
        expected_error_code: &'static str,
        expected_preview: &str,
    ) {
        assert_eq!(
            observation
                .response
                .get("error_code")
                .and_then(Value::as_str),
            Some(expected_error_code)
        );
        assert_eq!(observation.log.cli_key, "grok");
        assert_eq!(observation.log.status, Some(502));
        assert_eq!(
            observation.log.error_code.as_deref(),
            Some(expected_error_code)
        );

        let attempts: Value =
            serde_json::from_str(&observation.log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(
            attempts.len(),
            1,
            "unexpected Grok error attempts: {}",
            observation.log.attempts_json
        );
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(observation.provider_id)
        );
        assert!(attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains(expected_preview)));

        let error_details: Value = serde_json::from_str(
            observation
                .log
                .error_details_json
                .as_deref()
                .expect("error details JSON"),
        )
        .expect("valid error details JSON");
        assert!(error_details
            .get("upstream_body_preview")
            .and_then(Value::as_str)
            .is_some_and(|preview| preview.contains(expected_preview)));
    }

    async fn run_grok_sse_route(
        route_path: &'static str,
        request_body: &'static str,
        response_body: &'static str,
    ) -> (String, request_logs::RequestLogInsert, i64) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        settings::write(&app_handle, &settings::AppSettings::default()).expect("write settings");
        let proxy_result =
            crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
                .expect("enable Grok CLI proxy");
        assert!(proxy_result.ok, "{}", proxy_result.message);

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-sse.sqlite"))
            .expect("init test db");
        let (upstream_base_url, upstream_task) = spawn_sse_upstream(response_body).await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok SSE Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(route_path)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-grok-session-id", "grok-session-stream")
            .body(Body::from(request_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body")
                .to_vec(),
        )
        .expect("UTF-8 SSE body");
        let log = recv_terminal_request_log(&mut log_rx).await;
        upstream_task.abort();
        (body, log, provider_id)
    }

    fn assert_single_success_attempt(log: &request_logs::RequestLogInsert, provider_id: i64) {
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_responses_json_is_transparent_and_logged() {
        let request_body =
            r#"{"model":"grok-json-responses","input":"hello","store":false,"stream":false}"#;
        let response_body = r#"{"id":"resp-grok-json","object":"response","model":"grok-json-responses","output":[],"usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18}}"#;
        let observation = run_grok_json_route(
            "/grok/v1/responses?source=grok-test",
            request_body,
            response_body,
        )
        .await;

        assert!(observation
            .captured
            .head
            .starts_with("POST /v1/responses?source=grok-test HTTP/1.1"));
        assert!(observation
            .captured
            .has_header_line("authorization: bearer "));
        assert!(!observation.captured.has_header_line("x-api-key:"));
        assert!(!observation.captured.text().contains("client-placeholder"));
        assert!(observation
            .captured
            .has_header_line("x-grok-session-id: grok-session-route"));
        assert!(observation
            .captured
            .has_header_line("x-grok-conv-id: grok-conversation-route"));
        assert!(observation
            .captured
            .has_header_line("x-grok-req-id: grok-request-route"));
        assert_eq!(
            serde_json::from_slice::<Value>(&observation.captured.body).expect("request JSON"),
            serde_json::from_str::<Value>(request_body).expect("expected request JSON")
        );
        assert_eq!(
            observation.response.get("id").and_then(Value::as_str),
            Some("resp-grok-json")
        );
        assert_eq!(observation.log.cli_key, "grok");
        assert_eq!(observation.log.path, "/v1/responses");
        assert_eq!(observation.log.query.as_deref(), Some("source=grok-test"));
        assert_eq!(
            observation.log.session_id.as_deref(),
            Some("grok-session-route")
        );
        assert_eq!(
            observation.log.requested_model.as_deref(),
            Some("grok-json-responses")
        );
        assert_eq!(observation.log.status, Some(200));
        assert_eq!(observation.log.input_tokens, Some(11));
        assert_eq!(observation.log.output_tokens, Some(7));
        assert_eq!(observation.log.total_tokens, Some(18));
        assert_single_success_attempt(&observation.log, observation.provider_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_previous_response_retry_is_single_and_preserves_usage() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
            .expect("enable Grok CLI proxy");

        let success_body = r#"{"id":"resp-grok-after-retry","object":"response","model":"grok-continuation","output":[],"usage":{"input_tokens":13,"output_tokens":5,"total_tokens":18}}"#;
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_previous_response_retry_upstream(success_body).await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-continuation.sqlite"))
            .expect("init test db");
        let provider_id = insert_provider_with_priority(
            &db,
            "grok",
            "Grok Continuation Stub",
            upstream_base_url,
            0,
        );
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/grok/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"grok-continuation","previous_response_id":"resp_old","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let response: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("response JSON");
        assert_eq!(
            response.get("id").and_then(Value::as_str),
            Some("resp-grok-after-retry")
        );

        let first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first request timeout")
            .expect("first request");
        let second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second request timeout")
            .expect("second request");
        assert!(String::from_utf8_lossy(&first.body).contains("previous_response_id"));
        assert!(!String::from_utf8_lossy(&second.body).contains("previous_response_id"));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
                .await
                .expect("retry upstream should close")
                .is_none()
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.input_tokens, Some(13));
        assert_eq!(log.output_tokens, Some(5));
        assert_eq!(log.total_tokens, Some(18));
        assert!(log.ttfb_ms.is_some());
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert!(attempts.iter().all(|attempt| {
            attempt.get("provider_id").and_then(Value::as_i64) == Some(provider_id)
        }));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_chat_completions_json_is_transparent_and_logged() {
        let request_body = r#"{"model":"grok-json-chat","messages":[{"role":"user","content":"hello"}],"stream":false}"#;
        let response_body = r#"{"id":"chatcmpl-grok-json","object":"chat.completion","model":"grok-json-chat","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#;
        let observation =
            run_grok_json_route("/grok/v1/chat/completions", request_body, response_body).await;

        assert!(observation
            .captured
            .head
            .starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert_eq!(
            serde_json::from_slice::<Value>(&observation.captured.body).expect("request JSON"),
            serde_json::from_str::<Value>(request_body).expect("expected request JSON")
        );
        assert_eq!(observation.log.cli_key, "grok");
        assert_eq!(observation.log.path, "/v1/chat/completions");
        assert_eq!(
            observation.log.requested_model.as_deref(),
            Some("grok-json-chat")
        );
        assert_eq!(observation.log.input_tokens, Some(5));
        assert_eq!(observation.log.output_tokens, Some(3));
        assert_eq!(observation.log.total_tokens, Some(8));
        assert_single_success_attempt(&observation.log, observation.provider_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_responses_sse_is_transparent_and_logged() {
        let sse_body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-grok-sse\",\"status\":\"in_progress\",\"model\":\"grok-sse-responses\",\"usage\":{\"input_tokens\":9,\"output_tokens\":0,\"total_tokens\":9}}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-grok-sse\",\"status\":\"completed\",\"model\":\"grok-sse-responses\",\"output\":[],\"usage\":{\"input_tokens\":9,\"output_tokens\":4,\"total_tokens\":13}}}\n\n"
        );
        let (body, log, provider_id) = run_grok_sse_route(
            "/grok/v1/responses",
            r#"{"model":"grok-sse-responses","input":"hello","stream":true,"store":false}"#,
            sse_body,
        )
        .await;

        assert!(body.contains("event: response.completed"));
        assert_eq!(log.cli_key, "grok");
        assert_eq!(log.path, "/v1/responses");
        assert_eq!(log.session_id.as_deref(), Some("grok-session-stream"));
        assert_eq!(log.input_tokens, Some(9));
        assert_eq!(log.output_tokens, Some(4));
        assert_eq!(log.total_tokens, Some(13));
        assert_single_success_attempt(&log, provider_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_chat_completions_sse_is_transparent_and_logged() {
        let sse_body = concat!(
            "data: {\"id\":\"chatcmpl-grok-sse\",\"object\":\"chat.completion.chunk\",\"model\":\"grok-sse-chat\",\"choices\":[],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2,\"total_tokens\":8}}\n\n",
            "data: [DONE]\n\n"
        );
        let (body, log, provider_id) = run_grok_sse_route(
            "/grok/v1/chat/completions",
            r#"{"model":"grok-sse-chat","messages":[{"role":"user","content":"hello"}],"stream":true}"#,
            sse_body,
        )
        .await;

        assert!(body.contains("data: [DONE]"));
        assert_eq!(log.cli_key, "grok");
        assert_eq!(log.path, "/v1/chat/completions");
        assert_eq!(log.session_id.as_deref(), Some("grok-session-stream"));
        assert_eq!(log.input_tokens, Some(6));
        assert_eq!(log.output_tokens, Some(2));
        assert_eq!(log.total_tokens, Some(8));
        assert_single_success_attempt(&log, provider_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_auth_error_preserves_status_without_body() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        let proxy_result =
            crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
                .expect("enable Grok CLI proxy");
        assert!(proxy_result.ok, "{}", proxy_result.message);

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-error.sqlite"))
            .expect("init test db");
        let upstream_body =
            r#"{"code":"unauthenticated:no-credentials","error":"SYNTHETIC_SECRET"}"#;
        let (upstream_base_url, upstream_task) =
            spawn_status_json_upstream("401 Unauthorized", upstream_body).await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok 401 Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/grok/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"grok-error-model","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let payload: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("gateway error JSON");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream4xx.as_str())
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.cli_key, "grok");
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream4xx.as_str())
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(attempts[0].get("status").and_then(Value::as_i64), Some(401));
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream4xx.as_str())
        );
        assert!(attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason == "status=401"));
        assert!(!log.attempts_json.contains("SYNTHETIC_SECRET"));
        let error_details: Value = serde_json::from_str(
            log.error_details_json
                .as_deref()
                .expect("error details JSON"),
        )
        .expect("valid error details JSON");
        assert!(error_details.get("upstream_body_preview").is_none());
        assert!(!log
            .error_details_json
            .as_deref()
            .unwrap_or_default()
            .contains("SYNTHETIC_SECRET"));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_nested_json_error_preserves_preview() {
        let observation = run_grok_error_route(
            "500 Internal Server Error",
            "application/json",
            r#"{"error":{"message":"nested Grok upstream failure","type":"server_error"}}"#,
        )
        .await;

        assert_grok_error_observation(
            &observation,
            crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str(),
            "nested Grok upstream failure",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_non_json_error_preserves_preview() {
        let observation = run_grok_error_route(
            "502 Bad Gateway",
            "text/plain; charset=utf-8",
            "plain Grok upstream failure",
        )
        .await;

        assert_grok_error_observation(
            &observation,
            crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str(),
            "plain Grok upstream failure",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_fails_over_and_binds_stable_session() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");
        let proxy_result =
            crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
                .expect("enable Grok CLI proxy");
        assert!(proxy_result.ok, "{}", proxy_result.message);

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-grok-failover.sqlite"))
            .expect("init test db");
        let (failed_base_url, failed_task) = spawn_status_json_upstream(
            "401 Unauthorized",
            r#"{"code":"unauthenticated:no-credentials","error":"No credentials presented."}"#,
        )
        .await;
        let (success_base_url, success_task) = spawn_json_upstream(
            r#"{"id":"resp-grok-failover","object":"response","model":"grok-failover-model","output":[],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}"#,
        )
        .await;
        let failed_provider_id =
            insert_provider_with_priority(&db, "grok", "Grok Failed Stub", failed_base_url, 0);
        let success_provider_id =
            insert_provider_with_priority(&db, "grok", "Grok Success Stub", success_base_url, 1);
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig {
                    failure_threshold: 1,
                    ..circuit_breaker::CircuitBreakerConfig::default()
                },
                HashMap::new(),
                None,
            )),
            Arc::clone(&session),
        ));
        let session_id = "grok-session-failover";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/grok/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-grok-session-id", session_id)
            .header("x-grok-req-id", "request-id-must-not-bind")
            .body(Body::from(
                r#"{"model":"grok-failover-model","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.session_id.as_deref(), Some(session_id));
        assert_eq!(log.input_tokens, Some(3));
        assert_eq!(log.output_tokens, Some(2));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(failed_provider_id)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );
        assert_eq!(
            session
                .get_bound_provider("grok", session_id, crate::shared::time::now_unix_seconds(),),
            Some(success_provider_id)
        );
        assert_eq!(
            session.get_bound_provider(
                "grok",
                "request-id-must-not-bind",
                crate::shared::time::now_unix_seconds(),
            ),
            None
        );
        failed_task.abort();
        success_task.abort();
    }

    fn gateway_state_with_plugin_pipeline(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
        plugin_pipeline: Arc<GatewayPluginPipeline>,
    ) -> GatewayAppState<tauri::test::MockRuntime> {
        let mut state = gateway_state(app, db, log_tx);
        state.plugin_pipeline = plugin_pipeline;
        state
    }

    fn request_rewrite_plugin() -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: "test.request-rewrite".to_string(),
                name: "Request Rewrite".to_string(),
                current_version: Some("1.0.0".to_string()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: PluginManifest {
                id: "test.request-rewrite".to_string(),
                name: "Request Rewrite".to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime: PluginRuntime::ExtensionHost {
                    language: "typescript".to_string(),
                },
                hooks: vec![],
                permissions: vec![],
                main: Some("dist/index.js".to_string()),
                activation_events: vec![],
                contributes: Some(PluginContributes {
                    providers: vec![],
                    protocols: vec![],
                    protocol_bridges: vec![],
                    commands: vec![],
                    gateway_hooks: vec![PluginHook {
                        name: GatewayPluginHookName::RequestAfterBodyRead
                            .as_str()
                            .to_string(),
                        priority: 10,
                        failure_policy: Some("fail-open".to_string()),
                        timeout_ms: None,
                    }],
                    ui: BTreeMap::new(),
                }),
                capabilities: vec!["gateway.hooks".to_string()],
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.56.0 <1.0.0".to_string(),
                    plugin_api: "^1.0.0".to_string(),
                    platforms: vec![],
                },
                entry: None,
                config_schema: None,
                config_version: None,
                description: None,
                author: None,
                homepage: None,
                repository: None,
                license: None,
                checksum: None,
                signature: None,
                category: None,
            },
            install_source: PluginInstallSource::Official,
            installed_dir: None,
            config: serde_json::json!({}),
            granted_permissions: vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        }
    }

    fn gateway_hook_mut(plugin: &mut PluginDetail) -> &mut PluginHook {
        plugin
            .manifest
            .contributes
            .as_mut()
            .expect("gateway hook contributions")
            .gateway_hooks
            .first_mut()
            .expect("gateway hook")
    }

    fn set_granted_permissions(plugin: &mut PluginDetail, permissions: &[&str]) {
        plugin.manifest.permissions = vec![];
        plugin.granted_permissions = permissions.iter().map(|item| item.to_string()).collect();
    }

    fn fail_closed(mut plugin: PluginDetail) -> PluginDetail {
        gateway_hook_mut(&mut plugin).failure_policy = Some("fail-closed".to_string());
        plugin
    }

    fn before_send_header_plugin() -> PluginDetail {
        let mut plugin = request_rewrite_plugin();
        plugin.summary.plugin_id = "test.before-send".to_string();
        plugin.summary.name = "Before Send".to_string();
        plugin.manifest.id = "test.before-send".to_string();
        plugin.manifest.name = "Before Send".to_string();
        gateway_hook_mut(&mut plugin).name = GatewayPluginHookName::RequestBeforeSend
            .as_str()
            .to_string();
        set_granted_permissions(&mut plugin, &["request.meta.read", "request.header.write"]);
        plugin
    }

    fn before_send_body_plugin() -> PluginDetail {
        let mut plugin = before_send_header_plugin();
        set_granted_permissions(&mut plugin, &["request.body.read", "request.body.write"]);
        plugin
    }

    fn enable_test_model_route(
        app_settings: &mut settings::AppSettings,
        source_model: &str,
        target_model: &str,
    ) {
        app_settings.model_routing_policy = settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![settings::ModelRoutingRule {
                source_model: source_model.to_string(),
                target_model: Some(target_model.to_string()),
                reasoning_effort: None,
            }],
        };
    }

    fn response_after_plugin() -> PluginDetail {
        let mut plugin = request_rewrite_plugin();
        plugin.summary.plugin_id = "test.response-after".to_string();
        plugin.summary.name = "Response After".to_string();
        plugin.manifest.id = "test.response-after".to_string();
        plugin.manifest.name = "Response After".to_string();
        gateway_hook_mut(&mut plugin).name =
            GatewayPluginHookName::ResponseAfter.as_str().to_string();
        set_granted_permissions(&mut plugin, &["response.body.read", "response.body.write"]);
        plugin
    }

    fn stream_chunk_plugin() -> PluginDetail {
        let mut plugin = request_rewrite_plugin();
        plugin.summary.plugin_id = "test.stream-chunk".to_string();
        plugin.summary.name = "Stream Chunk".to_string();
        plugin.manifest.id = "test.stream-chunk".to_string();
        plugin.manifest.name = "Stream Chunk".to_string();
        gateway_hook_mut(&mut plugin).name =
            GatewayPluginHookName::ResponseChunk.as_str().to_string();
        set_granted_permissions(&mut plugin, &["stream.inspect", "stream.modify"]);
        plugin
    }

    fn log_redaction_plugin() -> PluginDetail {
        let mut plugin = request_rewrite_plugin();
        plugin.summary.plugin_id = "test.log-redaction".to_string();
        plugin.summary.name = "Log Redaction".to_string();
        plugin.manifest.id = "test.log-redaction".to_string();
        plugin.manifest.name = "Log Redaction".to_string();
        gateway_hook_mut(&mut plugin).name =
            GatewayPluginHookName::LogBeforePersist.as_str().to_string();
        set_granted_permissions(&mut plugin, &["log.redact"]);
        plugin
    }

    fn official_privacy_filter_for_tests() -> PluginDetail {
        let fixture = official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        let permissions = fixture.manifest.permissions.clone();
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: fixture.manifest,
            install_source: PluginInstallSource::Official,
            installed_dir: Some(fixture.root_dir.to_string_lossy().to_string()),
            config: fixture.default_config,
            granted_permissions: permissions,
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        }
    }

    fn gateway_error_plugin() -> PluginDetail {
        let mut plugin = request_rewrite_plugin();
        plugin.summary.plugin_id = "test.gateway-error".to_string();
        plugin.summary.name = "Gateway Error".to_string();
        plugin.manifest.id = "test.gateway-error".to_string();
        plugin.manifest.name = "Gateway Error".to_string();
        gateway_hook_mut(&mut plugin).name = GatewayPluginHookName::Error.as_str().to_string();
        set_granted_permissions(
            &mut plugin,
            &[
                "response.body.read",
                "response.body.write",
                "response.header.write",
            ],
        );
        plugin
    }

    fn persist_test_plugin(db: &db::Db, plugin: &PluginDetail) {
        repository::insert_plugin(
            db,
            repository::InsertPluginInput {
                manifest: plugin.manifest.clone(),
                install_source: PluginInstallSource::Official,
                status: PluginStatus::Enabled,
                installed_dir: None,
            },
        )
        .expect("insert test plugin");
        repository::save_plugin_permissions(
            db,
            &plugin.summary.plugin_id,
            &plugin.granted_permissions,
            &[],
        )
        .expect("grant test plugin permissions");
    }

    fn persist_plugin_detail(db: &db::Db, plugin: &PluginDetail) {
        repository::insert_plugin(
            db,
            repository::InsertPluginInput {
                manifest: plugin.manifest.clone(),
                install_source: plugin.install_source,
                status: plugin.summary.status,
                installed_dir: plugin.installed_dir.clone(),
            },
        )
        .expect("insert plugin detail");
        repository::save_plugin_permissions(
            db,
            &plugin.summary.plugin_id,
            &plugin.granted_permissions,
            &plugin.pending_permissions,
        )
        .expect("save plugin detail permissions");
        if let Some(config_version) = plugin.manifest.config_version {
            repository::save_plugin_config(
                db,
                &plugin.summary.plugin_id,
                config_version,
                &plugin.config,
                &[],
            )
            .expect("save plugin detail config");
        }
    }

    fn redact_privacy_filter_body_for_route_test(body: &str) -> String {
        body.replace("sys@example.com", "[邮箱]")
            .replace("13344441520", "[电话]")
            .replace("13344441521", "[电话]")
    }

    fn privacy_filter_route_executor() -> InMemoryGatewayPluginExecutor {
        InMemoryGatewayPluginExecutor::new().with_request_handler(
            "official.privacy-filter",
            |ctx| {
                let Some(body) = ctx.request.body.as_deref() else {
                    return GatewayHookResult::continue_unchanged();
                };
                let redacted = redact_privacy_filter_body_for_route_test(body);
                if redacted == body {
                    GatewayHookResult::continue_unchanged()
                } else {
                    GatewayHookResult {
                        request_body: Some(redacted),
                        ..GatewayHookResult::continue_unchanged()
                    }
                }
            },
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_timeout_stub_returns_bad_gateway_and_emits_request_log() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.upstream_first_byte_timeout_seconds = 1;
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, upstream_task) = spawn_hanging_upstream().await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-timeout","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamTimeout.as_str())
        );

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log enqueue")
            .expect("request log item");
        assert_eq!(log.cli_key, "codex");
        assert_eq!(log.path, "/v1/chat/completions");
        assert_eq!(log.status, Some(524));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamTimeout.as_str())
        );

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamTimeout.as_str())
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("request_timeout: category=SYSTEM_ERROR code=GW_UPSTREAM_TIMEOUT decision=switch timeout_secs=1")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );

        let provider_chain: Value =
            serde_json::from_str(log.provider_chain_json.as_deref().expect("provider chain"))
                .expect("provider chain json");
        assert_eq!(
            provider_chain
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item.get("provider_id"))
                .and_then(Value::as_i64),
            Some(provider_id)
        );

        let error_details: Value =
            serde_json::from_str(log.error_details_json.as_deref().expect("error details"))
                .expect("error details json");
        assert_eq!(
            error_details
                .get("gateway_error_code")
                .and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamTimeout.as_str())
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_request_after_body_read_rewrites_upstream_body() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-plugin-request-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_json_upstream(
            r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor = InMemoryGatewayPluginExecutor::new().with_request_handler(
            "test.request-rewrite",
            |_ctx| GatewayHookResult {
                request_body: Some(
                    r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"rewritten"}]}"#
                        .to_string(),
                ),
                ..GatewayHookResult::continue_unchanged()
            },
        );
        let plugin = request_rewrite_plugin();
        persist_test_plugin(&db, &plugin);
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin.clone()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db.clone(),
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"original"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            status,
            StatusCode::OK,
            "response body: {}",
            String::from_utf8_lossy(&body)
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured body");
        assert!(captured.contains(r#""content":"rewritten""#));
        assert!(!captured.contains(r#""content":"original""#));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        let plugin_detail = repository::get_plugin(&db, &plugin.summary.plugin_id)
            .expect("read persisted plugin detail");
        assert!(plugin_detail.audit_logs.iter().any(|audit| {
            audit.trace_id.as_deref() == Some(request_log.trace_id.as_str())
                && audit.event_type == "plugin.hook.completed"
        }));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn official_privacy_filter_redacts_gzipped_codex_responses_as_identity_upstream() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("privacy-filter-gzip-test.sqlite"))
            .expect("init test db");
        let fixture = official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        let permissions = fixture.manifest.permissions.clone();
        let plugin = PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: fixture.manifest,
            install_source: PluginInstallSource::Official,
            installed_dir: Some(fixture.root_dir.to_string_lossy().to_string()),
            config: fixture.default_config,
            granted_permissions: permissions.clone(),
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        };
        repository::insert_plugin(
            &db,
            repository::InsertPluginInput {
                manifest: plugin.manifest.clone(),
                install_source: PluginInstallSource::Official,
                status: PluginStatus::Enabled,
                installed_dir: plugin.installed_dir.clone(),
            },
        )
        .expect("insert official privacy filter");
        repository::save_plugin_permissions(&db, &plugin.summary.plugin_id, &permissions, &[])
            .expect("grant official privacy filter permissions");

        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin],
            Arc::new(privacy_filter_route_executor()),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let plain_body = serde_json::json!({
            "model": "gpt-plugin",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "你知道 13344441520 是哪里的手机号嘛"
                }]
            }]
        })
        .to_string();
        let compressed_body = gzip_bytes(plain_body.as_bytes());
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(compressed_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            status,
            StatusCode::OK,
            "response body: {}",
            String::from_utf8_lossy(&body)
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        assert!(!captured.has_header_line("content-encoding:"));
        let body_text = String::from_utf8_lossy(&captured.body);
        assert!(body_text.contains("[电话]"));
        assert!(!body_text.contains("13344441520"));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        assert!(!request_log.attempts_json.contains("13344441520"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn official_privacy_filter_redacts_full_codex_responses_payload_before_upstream_and_logs()
    {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("privacy-filter-full-codex-payload-test.sqlite"),
        )
        .expect("init test db");
        let fixture = official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        let permissions = fixture.manifest.permissions.clone();
        let plugin = PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: fixture.manifest,
            install_source: PluginInstallSource::Official,
            installed_dir: Some(fixture.root_dir.to_string_lossy().to_string()),
            config: fixture.default_config,
            granted_permissions: permissions.clone(),
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        };
        repository::insert_plugin(
            &db,
            repository::InsertPluginInput {
                manifest: plugin.manifest.clone(),
                install_source: PluginInstallSource::Official,
                status: PluginStatus::Enabled,
                installed_dir: plugin.installed_dir.clone(),
            },
        )
        .expect("insert official privacy filter");
        repository::save_plugin_permissions(&db, &plugin.summary.plugin_id, &permissions, &[])
            .expect("grant official privacy filter permissions");

        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin],
            Arc::new(privacy_filter_route_executor()),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let plain_body = serde_json::json!({
            "model": "gpt-plugin",
            "instructions": "developer prompt with sys@example.com",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{
                        "type": "input_text",
                        "text": "developer-visible phone 13344441521"
                    }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "你知道 13344441520 是哪里的手机号嘛"
                    }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_123",
                    "name": "lookup_phone",
                    "arguments": "{\"phone\":\"13344441522\"}"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "lookup_phone",
                "description": "Lookup 13344441523",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "phone": {
                            "type": "string",
                            "description": "Phone like 13344441524"
                        }
                    }
                }
            }],
            "tool_choice": "auto",
            "reasoning": { "effort": "xhigh" },
            "client_metadata": {
                "x-codex-window-id": "13344441525"
            }
        })
        .to_string();
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(plain_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            status,
            StatusCode::OK,
            "response body: {}",
            String::from_utf8_lossy(&body)
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        let body_text = String::from_utf8_lossy(&captured.body);
        assert!(body_text.contains("[电话]"));
        assert!(body_text.contains("[邮箱]"));
        assert!(!body_text.contains("13344441520"));
        assert!(!body_text.contains("13344441521"));
        assert!(
            body_text.contains("13344441522"),
            "function_call.arguments should remain unchanged: {body_text}"
        );
        assert!(
            body_text.contains("13344441523"),
            "tool description should remain unchanged: {body_text}"
        );
        assert!(
            body_text.contains("13344441524"),
            "tool parameters should remain unchanged: {body_text}"
        );
        assert!(
            body_text.contains("13344441525"),
            "client_metadata should remain unchanged: {body_text}"
        );

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        assert!(!request_log.attempts_json.contains("13344441520"));
        assert!(!request_log.attempts_json.contains("13344441521"));
        assert!(!request_log
            .provider_chain_json
            .as_deref()
            .unwrap_or_default()
            .contains("13344441520"));
        assert!(!request_log
            .error_details_json
            .as_deref()
            .unwrap_or_default()
            .contains("13344441520"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn official_privacy_filter_before_send_redacts_final_upstream_body() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("privacy-filter-before-send.sqlite"))
            .expect("init test db");
        let mut plugin = official_privacy_filter_for_tests();
        if let Some(contributes) = plugin.manifest.contributes.as_mut() {
            contributes
                .gateway_hooks
                .retain(|hook| hook.name != "gateway.request.afterBodyRead");
        }
        persist_plugin_detail(&db, &plugin);

        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin],
            Arc::new(privacy_filter_route_executor()),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "gpt-plugin",
                    "input": [{
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": "你知道 13344441520 是哪里的手机号嘛"
                        }]
                    }]
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            status,
            StatusCode::OK,
            "response body: {}",
            String::from_utf8_lossy(&body)
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        let body_text = String::from_utf8_lossy(&captured.body);
        assert!(body_text.contains("[电话]"));
        assert!(!body_text.contains("13344441520"));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        assert!(!request_log.attempts_json.contains("13344441520"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_before_send_mutation_survives_codex_internal_retry() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("privacy-filter-retry.sqlite"))
            .expect("init test db");
        let mut plugin = before_send_header_plugin();
        set_granted_permissions(&mut plugin, &["request.body.read", "request.body.write"]);
        persist_plugin_detail(&db, &plugin);

        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_previous_response_retry_upstream(
                r#"{"id":"stub-ok","object":"response","output":[]}"#,
            )
            .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", {
                let call_count = Arc::clone(&call_count);
                move |ctx| {
                    let call = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut result = GatewayHookResult::continue_unchanged();
                    if call == 0 {
                        let body = ctx.request.body.expect("request body visible");
                        result.request_body = Some(body.replace("13344441520", "[电话]"));
                    }
                    result
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let plain_body = serde_json::json!({
            "model": "gpt-plugin",
            "previous_response_id": "resp_old",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "你知道 13344441520 是哪里的手机号嘛"
                }]
            }]
        })
        .to_string();
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzip_bytes(plain_body.as_bytes())))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        assert!(!first.has_header_line("content-encoding:"));
        assert!(!String::from_utf8_lossy(&first.body).contains("13344441520"));
        assert!(String::from_utf8_lossy(&first.body).contains("[电话]"));

        assert!(!second.has_header_line("content-encoding:"));
        let second_body = String::from_utf8_lossy(&second.body);
        assert!(
            second_body.contains("[电话]"),
            "retry request should keep the beforeSend redaction: {second_body}"
        );
        assert!(
            !second_body.contains("13344441520"),
            "retry request leaked the original phone number: {second_body}"
        );
        assert!(!second_body.contains("previous_response_id"));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        assert!(!request_log.attempts_json.contains("13344441520"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_normalizes_gzipped_codex_request_to_identity_upstream() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-gzip-normalization-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let plain_body = serde_json::json!({
            "model": "gpt-plugin",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "你知道 13344441520 是哪里的手机号嘛"
                }]
            }]
        })
        .to_string();
        let compressed_body = gzip_bytes(plain_body.as_bytes());
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(compressed_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        assert!(!captured.has_header_line("content-encoding:"));
        assert_eq!(captured.body, plain_body.as_bytes());
        assert!(captured.text().contains("13344441520"));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_inspects_and_normalizes_zstd_codex_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = true;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-zstd-normalization-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let plain_body = serde_json::json!({
            "model": "gpt-5.6-sol",
            "reasoning": {
                "effort": "max"
            },
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "hello"
                }]
            }]
        })
        .to_string();
        let compressed_body = zstd_bytes(plain_body.as_bytes());
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "zstd")
            .body(Body::from(compressed_body))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        assert!(!captured.has_header_line("content-encoding:"));
        let captured_json: Value =
            serde_json::from_slice(&captured.body).expect("captured request JSON");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some("gpt-5.6-sol")
        );
        let prompt_cache_key = captured_json
            .get("prompt_cache_key")
            .and_then(Value::as_str)
            .expect("completed prompt cache key");
        assert!(captured.has_header_line(&format!("session_id: {prompt_cache_key}")));
        assert!(captured.has_header_line(&format!("x-session-id: {prompt_cache_key}")));

        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        assert_eq!(request_log.requested_model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(request_log.session_id.as_deref(), Some(prompt_cache_key));
        let settings = parse_special_settings(&request_log);
        let effort = settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("codex_reasoning_effort")
            })
            .expect("request reasoning effort setting");
        assert_eq!(effort.get("effort").and_then(Value::as_str), Some("max"));
        assert_eq!(
            effort.get("source").and_then(Value::as_str),
            Some("request")
        );
        assert!(!settings.iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("codex_context_compaction")
        }));
        let session_completion = settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("codex_session_id_completion")
            })
            .expect("session completion setting");
        assert_eq!(
            session_completion
                .get("changedBody")
                .and_then(Value::as_bool),
            Some(true)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_normalizes_compressed_codex_compact_request_with_nested_prefix() {
        let _env_lock = crate::test_support::test_env_lock();
        let plain_body = serde_json::json!({
            "model": "gpt-5.6-sol",
            "input": "compact this context"
        })
        .to_string();
        let (captured, request_log) = run_encoded_codex_route(
            "gateway-gzip-compact-normalization-test.sqlite",
            "/nested/openai/v1/responses/compact/",
            "x-gzip",
            gzip_bytes(plain_body.as_bytes()),
            r#"{"id":"stub-compact","object":"response.compaction","output":[]}"#,
        )
        .await;

        assert!(!captured.has_header_line("content-encoding:"));
        assert_eq!(captured.body, plain_body.as_bytes());
        assert_eq!(request_log.status, Some(200));
        assert_eq!(request_log.requested_model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(request_log.path, "/nested/openai/v1/responses/compact/");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_normalizes_brotli_codex_chat_completions_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let plain_body = serde_json::json!({
            "model": "gpt-5.6-sol",
            "messages": [{ "role": "user", "content": "hello" }]
        })
        .to_string();
        let (captured, request_log) = run_encoded_codex_route(
            "gateway-brotli-chat-normalization-test.sqlite",
            "/v1/chat/completions/",
            "br",
            brotli_bytes(plain_body.as_bytes()),
            r#"{"id":"stub-chat","object":"chat.completion","choices":[]}"#,
        )
        .await;

        assert!(!captured.has_header_line("content-encoding:"));
        assert_eq!(captured.body, plain_body.as_bytes());
        assert_eq!(request_log.status, Some(200));
        assert_eq!(request_log.requested_model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(request_log.path, "/v1/chat/completions/");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn damaged_codex_encoding_returns_400_without_upstream_attempt() {
        let _env_lock = crate::test_support::test_env_lock();
        let sensitive_body =
            br#"{"model":"gpt-5.6-sol","input":"secret-body-must-not-be-logged"}"#.to_vec();
        let (status, payload, request_log) = run_rejected_encoded_codex_route(
            "gateway-invalid-codex-encoding-test.sqlite",
            "zstd",
            sensitive_body,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_INVALID_REQUEST_CONTENT_ENCODING")
        );
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .expect("public error message");
        assert!(message.contains("Content-Encoding"));
        assert!(!message.contains("zstd"));
        assert!(payload
            .get("attempts")
            .and_then(Value::as_array)
            .is_some_and(|attempts| attempts.is_empty()));
        assert!(!payload
            .to_string()
            .contains("secret-body-must-not-be-logged"));
        assert_eq!(request_log.status, Some(400));
        assert_eq!(
            request_log.error_code.as_deref(),
            Some("GW_INVALID_REQUEST_CONTENT_ENCODING")
        );
        assert_eq!(request_log.attempts_json, "[]");
        assert!(!request_log
            .error_details_json
            .as_deref()
            .unwrap_or_default()
            .contains("secret-body-must-not-be-logged"));
        assert!(!request_log
            .special_settings_json
            .as_deref()
            .unwrap_or_default()
            .contains("secret-body-must-not-be-logged"));
        assert!(!request_log
            .provider_chain_json
            .as_deref()
            .unwrap_or_default()
            .contains("secret-body-must-not-be-logged"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn oversized_decoded_codex_body_returns_413_without_upstream_attempt() {
        let _env_lock = crate::test_support::test_env_lock();
        let plain_body = format!(
            r#"{{"model":"gpt-5.6-sol","input":"{}"}}"#,
            "a".repeat(1024 * 1024)
        );
        let encoded_body = zstd_bytes(plain_body.as_bytes());
        assert!(encoded_body.len() < 1024 * 1024);
        let (status, payload, request_log) = run_rejected_encoded_codex_route(
            "gateway-oversized-decoded-codex-body-test.sqlite",
            "zstd",
            encoded_body,
            Some("1"),
        )
        .await;

        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_BODY_TOO_LARGE")
        );
        assert!(payload
            .get("attempts")
            .and_then(Value::as_array)
            .is_some_and(|attempts| attempts.is_empty()));
        assert_eq!(request_log.status, Some(413));
        assert_eq!(request_log.error_code.as_deref(), Some("GW_BODY_TOO_LARGE"));
        assert_eq!(request_log.attempts_json, "[]");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_preserves_non_codex_gzip_request_transport() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
            .expect("enable grok cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-grok-gzip-passthrough.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_raw_upstream(
            r#"{"id":"stub-chat","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok Gzip Stub", upstream_base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let plain_body = r#"{"model":"grok-build","messages":[{"role":"user","content":"hello"}]}"#;
        let encoded_body = gzip_bytes(plain_body.as_bytes());
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/grok/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(encoded_body.clone()))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");
        assert!(captured.has_header_line("content-encoding: gzip"));
        assert_eq!(captured.body, encoded_body);
        let request_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(request_log.status, Some(200));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_request_after_body_read_fail_closed_error_stops_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-after-body-fail-closed-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_json_upstream(
            r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor = InMemoryGatewayPluginExecutor::new().with_request_handler(
            "test.request-rewrite",
            |_ctx| {
                let mut result = GatewayHookResult::continue_unchanged();
                result
                    .headers
                    .insert("x-aio-forbidden".to_string(), "1".to_string());
                result
            },
        );
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![fail_closed(request_rewrite_plugin())],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"original"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), captured_rx)
                .await
                .is_err(),
            "fail-closed afterBodyRead should not send the request upstream"
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_request_before_send_adds_upstream_header() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-plugin-before-send-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_raw_upstream(
            r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", |_ctx| {
                let mut result = GatewayHookResult::continue_unchanged();
                result
                    .headers
                    .insert("x-plugin-before-send".to_string(), "applied".to_string());
                result
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![before_send_header_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured raw request");
        assert!(
            captured
                .text()
                .to_ascii_lowercase()
                .contains("x-plugin-before-send: applied"),
            "captured upstream request did not include plugin header:\n{}",
            captured.text()
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_request_before_send_fail_closed_error_stops_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-before-send-fail-closed-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_raw_upstream(
            r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", |_ctx| {
                let mut result = GatewayHookResult::continue_unchanged();
                result
                    .headers
                    .insert("x-aio-forbidden".to_string(), "1".to_string());
                result
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![fail_closed(before_send_header_plugin())],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), captured_rx)
                .await
                .is_err(),
            "fail-closed beforeSend should not send the request upstream"
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_model_route_runs_after_gzip_and_before_send_plugin() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_codex_session_id_completion = false;
        disable_upstream_retry_policy(&mut app_settings);
        enable_test_model_route(&mut app_settings, "route-source", "route-target");
        app_settings.model_routing_policy.rules[0].reasoning_effort = Some("low".to_string());
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("configured-route-ordering.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) = spawn_capturing_raw_upstream(
            r#"{"id":"route-ok","object":"response","model":"route-target","output":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", |_ctx| {
                GatewayHookResult {
                    request_body: Some(
                        r#"{"model":"plugin-target","input":"hello","stream":false,"pluginTouched":true,"reasoning":{"effort":"high"}}"#
                            .to_string(),
                    ),
                    ..GatewayHookResult::continue_unchanged()
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![before_send_body_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let plain_body = r#"{"model":"route-source","input":"hello","stream":false,"reasoning":{"effort":"max"}}"#;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gzip_bytes(plain_body.as_bytes())))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured raw request");
        assert!(!captured.has_header_line("content-encoding:"));
        let upstream_body: Value =
            serde_json::from_slice(&captured.body).expect("captured request JSON");
        assert_eq!(
            upstream_body.get("model").and_then(Value::as_str),
            Some("route-target")
        );
        assert_eq!(
            upstream_body.get("pluginTouched").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            upstream_body
                .get("reasoning")
                .and_then(|value| value.get("effort"))
                .and_then(Value::as_str),
            Some("low")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.requested_model.as_deref(), Some("route-source"));
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0]["provider_id"].as_i64(), Some(provider_id));
        assert_eq!(attempts[0]["outcome"].as_str(), Some("success"));
        assert_eq!(
            attempts[0]["requested_upstream_model"].as_str(),
            Some("route-target")
        );

        let markers = parse_special_settings(&log);
        let configured_route = markers
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("configured_model_route")
            })
            .expect("configured route marker");
        assert_eq!(
            configured_route.get("providerId").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            configured_route.get("sourceModel").and_then(Value::as_str),
            Some("route-source")
        );
        assert_eq!(
            configured_route.get("targetModel").and_then(Value::as_str),
            Some("route-target")
        );
        assert_eq!(
            configured_route
                .get("effectiveModel")
                .and_then(Value::as_str),
            Some("route-target")
        );
        assert_eq!(
            configured_route
                .get("reasoningEffort")
                .and_then(Value::as_str),
            Some("low")
        );
        assert_eq!(
            configured_route
                .get("reasoningEffortApplied")
                .and_then(Value::as_bool),
            Some(true)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_disabled_between_configured_retries_blocks_later_send() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule::status_only(503)],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("provider-disable-during-retry.sqlite"))
            .expect("init test db");
        let mut first_upstream = spawn_gated_counting_status_upstream(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"message":"retry this response"}}"#,
        )
        .await;
        let success_body =
            r#"{"id":"disable-failover-ok","object":"response","status":"completed","output":[]}"#;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_provider_id = insert_codex_provider_with_priority(
            &db,
            "Disable During Retry",
            first_upstream.base_url.clone(),
            0,
        );
        let second_provider_id =
            insert_codex_provider_with_priority(&db, "Enabled Fallback", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            Arc::clone(&circuit),
            Arc::clone(&session),
        ));
        let session_id = "provider-disable-during-retry-session";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-disable-during-retry","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response_task = tokio::spawn(router.oneshot(request));
        first_upstream.wait_for_first_request().await;
        providers::set_enabled(&db, first_provider_id, false)
            .expect("disable provider after first send");
        first_upstream.release_first_response();

        let response = response_task
            .await
            .expect("route task")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            first_upstream.calls(),
            1,
            "the disabled Provider must not receive its configured retry"
        );
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 3);
        assert_eq!(attempts[0]["provider_id"].as_i64(), Some(first_provider_id));
        assert_eq!(attempts[0]["decision"].as_str(), Some("retry"));
        assert_eq!(attempts[1]["provider_id"].as_i64(), Some(first_provider_id));
        assert_eq!(attempts[1]["outcome"].as_str(), Some("skipped"));
        assert_eq!(
            attempts[1]["reason_code"].as_str(),
            Some("provider_disabled")
        );
        assert_eq!(attempts[1]["provider_index"], Value::Null);
        assert_eq!(attempts[1]["retry_index"], Value::Null);
        assert_eq!(
            attempts[2]["provider_id"].as_i64(),
            Some(second_provider_id)
        );
        assert_eq!(attempts[2]["outcome"].as_str(), Some("success"));
        assert_eq!(circuit.snapshot(first_provider_id, 0).failure_count, 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, 0),
            Some(second_provider_id)
        );

        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bridge_source_disabled_between_retries_blocks_later_send() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule::status_only(503)],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "claude", true, "http://127.0.0.1:37123")
            .expect("enable claude cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("bridge-source-disable-during-retry.sqlite"),
        )
        .expect("init test db");
        let mut source_upstream = spawn_gated_counting_status_upstream(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"message":"retry this bridged response"}}"#,
        )
        .await;
        let source_provider_id = insert_provider_with_priority(
            &db,
            "codex",
            "Bridge Source Disable During Retry",
            source_upstream.base_url.clone(),
            0,
        );
        let bridge_provider_id = insert_cx2cc_bridge_provider(&db, source_provider_id, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            Arc::clone(&circuit),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/claude/_aio/provider/{bridge_provider_id}/v1/messages"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":128,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response_task = tokio::spawn(router.oneshot(request));
        source_upstream.wait_for_first_request().await;
        providers::set_enabled(&db, source_provider_id, false)
            .expect("disable bridge source after first send");
        source_upstream.release_first_response();

        let response = response_task
            .await
            .expect("route task")
            .expect("route response");
        assert!(!response.status().is_success());
        assert_eq!(
            source_upstream.calls(),
            1,
            "the disabled bridge source must not receive its configured retry"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0]["provider_id"].as_i64(),
            Some(bridge_provider_id)
        );
        assert_eq!(attempts[0]["decision"].as_str(), Some("retry"));
        assert_eq!(
            attempts[1]["provider_id"].as_i64(),
            Some(bridge_provider_id)
        );
        assert_eq!(attempts[1]["outcome"].as_str(), Some("skipped"));
        assert_eq!(
            attempts[1]["reason_code"].as_str(),
            Some("provider_disabled")
        );
        assert!(attempts[1]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains(&source_provider_id.to_string())));
        assert_eq!(circuit.snapshot(bridge_provider_id, 0).failure_count, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_provider_specific_route_does_not_fall_back() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("disabled-provider-specific-route.sqlite"),
        )
        .expect("init test db");
        let (disabled_url, disabled_calls, disabled_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"disabled-provider-should-not-send"}"#,
        )
        .await;
        let (fallback_url, fallback_calls, fallback_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"forced-route-must-not-fall-back"}"#,
        )
        .await;
        let disabled_provider_id =
            insert_codex_provider_with_priority(&db, "Disabled Forced Provider", disabled_url, 0);
        insert_codex_provider_with_priority(&db, "Enabled But Not Forced", fallback_url, 1);
        providers::set_enabled(&db, disabled_provider_id, false).expect("disable forced provider");

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{disabled_provider_id}/v1/responses"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-disabled-forced","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("response JSON");
        assert_eq!(
            body.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::NoEnabledProvider.as_str())
        );
        assert_eq!(disabled_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(fallback_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::NoEnabledProvider.as_str())
        );
        assert!(request_log_attempts(&log).is_empty());

        disabled_task.abort();
        fallback_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn provider_self_loop_switches_without_circuit_or_session_pollution() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");
        crate::gateway::http_client::sync_runtime_context(37123, "127.0.0.1", "localhost");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("provider-self-loop-route.sqlite"))
            .expect("init test db");
        let success_body = r#"{"id":"self-loop-failover-ok","object":"response","status":"completed","output":[]}"#;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let self_loop_provider_id = insert_codex_provider_with_priority(
            &db,
            "Self Loop",
            "http://127.0.0.1:37123".to_string(),
            0,
        );
        let second_provider_id =
            insert_codex_provider_with_priority(&db, "Remote Fallback", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::clone(&session),
        ));
        let session_id = "provider-self-loop-session";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-self-loop","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0]["provider_id"].as_i64(),
            Some(self_loop_provider_id)
        );
        assert_eq!(attempts[0]["outcome"].as_str(), Some("skipped"));
        assert_eq!(
            attempts[0]["reason_code"].as_str(),
            Some("provider_target_self_loop")
        );
        assert_eq!(attempts[0]["circuit_state_before"], Value::Null);
        assert_eq!(attempts[0]["circuit_state_after"], Value::Null);
        assert_eq!(
            attempts[1]["provider_id"].as_i64(),
            Some(second_provider_id)
        );
        assert_eq!(attempts[1]["outcome"].as_str(), Some("success"));
        assert_eq!(circuit.snapshot(self_loop_provider_id, 0).failure_count, 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, 0),
            Some(second_provider_id)
        );

        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_model_route_apply_failure_switches_provider_without_runtime_pollution() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 3;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        disable_upstream_retry_policy(&mut app_settings);
        enable_test_model_route(&mut app_settings, "route-source", "route-target");
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("configured-route-failover.sqlite"))
            .expect("init test db");
        let response_body = r#"{"id":"route-ok","object":"response","model":"route-target","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::OK, response_body).await;
        let (second_url, captured_rx, second_task) =
            spawn_capturing_json_upstream(response_body).await;
        let first_provider_id =
            insert_codex_provider_with_priority(&db, "Route Apply Fails", first_url, 0);
        let second_provider_id =
            insert_codex_provider_with_priority(&db, "Route Apply Succeeds", second_url, 1);

        let hook_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", {
                let hook_calls = Arc::clone(&hook_calls);
                move |_ctx| {
                    let call = hook_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    GatewayHookResult {
                        request_body: (call == 0).then(|| "not-json".to_string()),
                        ..GatewayHookResult::continue_unchanged()
                    }
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![before_send_body_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let mut state = gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::clone(&session),
        );
        state.plugin_pipeline = plugin_pipeline;
        let router = build_router(state);
        let session_id = "configured-route-failover-session";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"route-source","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            hook_calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "route apply failure must switch provider without retrying the same provider"
        );
        assert_eq!(
            first_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "failed route application must not reach the first upstream"
        );
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured second upstream request")
            .expect("captured request body");
        let captured: Value = serde_json::from_str(&captured).expect("captured request JSON");
        assert_eq!(
            captured.get("model").and_then(Value::as_str),
            Some("route-target")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.requested_model.as_deref(), Some("route-source"));
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["provider_id"].as_i64(), Some(first_provider_id));
        assert_eq!(
            attempts[0]["error_code"].as_str(),
            Some(crate::gateway::proxy::GatewayErrorCode::ConfiguredModelRouteApplyFailed.as_str())
        );
        assert_eq!(attempts[0]["decision"].as_str(), Some("switch"));
        assert_eq!(attempts[0]["circuit_state_before"].as_str(), Some("CLOSED"));
        assert_eq!(attempts[0]["circuit_state_after"].as_str(), Some("CLOSED"));
        assert_eq!(
            attempts[1]["provider_id"].as_i64(),
            Some(second_provider_id)
        );
        assert_eq!(attempts[1]["outcome"].as_str(), Some("success"));
        assert_eq!(
            attempts[1]["requested_upstream_model"].as_str(),
            Some("route-target")
        );

        let markers = parse_special_settings(&log);
        let configured_route = markers
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("configured_model_route")
            })
            .expect("configured route marker");
        assert_eq!(
            configured_route.get("providerId").and_then(Value::as_i64),
            Some(second_provider_id)
        );
        assert_eq!(
            configured_route.get("sourceModel").and_then(Value::as_str),
            Some("route-source")
        );
        assert_eq!(
            configured_route
                .get("effectiveModel")
                .and_then(Value::as_str),
            Some("route-target")
        );
        assert_eq!(circuit.snapshot(first_provider_id, 0).failure_count, 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, 0),
            Some(second_provider_id)
        );

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_configured_model_route_apply_failures_return_dedicated_502_without_side_effects() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 3;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        disable_upstream_retry_policy(&mut app_settings);
        enable_test_model_route(&mut app_settings, "route-source", "route-target");
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("configured-route-all-failed.sqlite"))
            .expect("init test db");
        let response_body = r#"{"id":"must-not-run","object":"response","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::OK, response_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, response_body).await;
        let first_provider_id =
            insert_codex_provider_with_priority(&db, "Route Apply Fails 1", first_url, 0);
        let second_provider_id =
            insert_codex_provider_with_priority(&db, "Route Apply Fails 2", second_url, 1);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", |_ctx| {
                GatewayHookResult {
                    request_body: Some("not-json".to_string()),
                    ..GatewayHookResult::continue_unchanged()
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![before_send_body_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let mut state = gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::clone(&session),
        );
        state.plugin_pipeline = plugin_pipeline;
        let router = build_router(state);
        let session_id = "configured-route-all-failed-session";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"route-source","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let payload: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("gateway error JSON");
        let expected_error_code =
            crate::gateway::proxy::GatewayErrorCode::ConfiguredModelRouteApplyFailed.as_str();
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(expected_error_code)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some(expected_error_code));
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 2);
        for (attempt, provider_id) in attempts.iter().zip([first_provider_id, second_provider_id]) {
            assert_eq!(attempt["provider_id"].as_i64(), Some(provider_id));
            assert_eq!(attempt["error_code"].as_str(), Some(expected_error_code));
            assert_eq!(attempt["decision"].as_str(), Some("switch"));
            assert_eq!(attempt["circuit_state_before"].as_str(), Some("CLOSED"));
            assert_eq!(attempt["circuit_state_after"].as_str(), Some("CLOSED"));
        }
        assert!(!parse_special_settings(&log).iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("configured_model_route")
        }));
        assert_eq!(circuit.snapshot(first_provider_id, 0).failure_count, 0);
        assert_eq!(circuit.snapshot(second_provider_id, 0).failure_count, 0);
        assert_eq!(session.get_bound_provider("codex", session_id, 0), None);

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_model_route_apply_failure_does_not_override_prior_upstream_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        enable_test_model_route(&mut app_settings, "route-source", "route-target");
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("configured-route-error-priority.sqlite"))
            .expect("init test db");
        let (first_url, first_calls, first_task) = spawn_counting_status_upstream(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"message":"upstream unavailable"}}"#,
        )
        .await;
        let (second_url, second_calls, second_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"must-not-run","object":"response","output":[]}"#,
        )
        .await;
        let first_provider_id =
            insert_codex_provider_with_priority(&db, "Upstream Fails", first_url, 0);
        let second_provider_id =
            insert_codex_provider_with_priority(&db, "Route Apply Fails", second_url, 1);

        let hook_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", {
                let hook_calls = Arc::clone(&hook_calls);
                move |_ctx| {
                    let call = hook_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    GatewayHookResult {
                        request_body: (call == 1).then(|| "not-json".to_string()),
                        ..GatewayHookResult::continue_unchanged()
                    }
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![before_send_body_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let mut state = gateway_state(app_handle, db, log_tx);
        state.plugin_pipeline = plugin_pipeline;
        let router = build_router(state);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"route-source","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let payload: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("gateway error JSON");
        let expected_error_code = crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str();
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(expected_error_code)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.error_code.as_deref(), Some(expected_error_code));
        let attempts = request_log_attempts(&log);
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["provider_id"].as_i64(), Some(first_provider_id));
        assert_eq!(
            attempts[0]["error_code"].as_str(),
            Some(expected_error_code)
        );
        assert_eq!(
            attempts[1]["provider_id"].as_i64(),
            Some(second_provider_id)
        );
        assert_eq!(
            attempts[1]["error_code"].as_str(),
            Some(crate::gateway::proxy::GatewayErrorCode::ConfiguredModelRouteApplyFailed.as_str())
        );

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_alias_routes_only_to_its_bound_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-codex-alias-route.sqlite"))
            .expect("init test db");
        let response_body = r#"{"id":"resp-managed","object":"response","model":"grok-4.5","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":3,"output_tokens":1,"total_tokens":4}}"#;
        let (bound_url, captured_rx, bound_task) =
            spawn_capturing_json_upstream(response_body).await;
        let (other_url, other_calls, other_task) =
            spawn_counting_status_upstream(StatusCode::OK, response_body).await;
        let bound_provider_id =
            insert_codex_provider_with_priority(&db, "Managed Bound", bound_url, 0);
        let other_provider_id =
            insert_codex_provider_with_priority(&db, "Managed Other", other_url, 1);
        let canonical_model =
            insert_managed_codex_profile_alias(&db, bound_provider_id, "grok-4.5", "grok-profile");
        let _other_canonical = insert_managed_codex_model(&db, other_provider_id, "grok-4.5");
        let bound_provider_uuid = {
            let conn = db.open_connection().expect("open db");
            providers::get_by_id(&conn, bound_provider_id)
                .expect("read bound provider")
                .provider_uuid
        };

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured_body = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("bound upstream request")
            .expect("captured request body");
        let captured_json: Value =
            serde_json::from_str(&captured_body).expect("captured JSON body");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some("grok-4.5")
        );
        assert_eq!(
            other_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "same-name model on another provider must not be called"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(
            log.requested_model.as_deref(),
            Some(canonical_model.as_str())
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(bound_provider_id)
        );
        assert_eq!(
            attempts[0]
                .get("requested_upstream_model")
                .and_then(Value::as_str),
            Some("grok-4.5")
        );
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));
        let managed_route = special_settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("aio_managed_model_route")
            })
            .expect("managed route setting");
        assert_eq!(
            managed_route.get("providerId").and_then(Value::as_i64),
            Some(bound_provider_id)
        );
        assert_eq!(
            managed_route.get("providerUuid").and_then(Value::as_str),
            Some(bound_provider_uuid.as_str())
        );
        assert_eq!(
            managed_route.get("observation").and_then(Value::as_str),
            Some("matched")
        );

        bound_task.abort();
        other_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_alias_accepts_256_byte_model_id_at_utf8_boundary() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        // The byte cap lies inside this character at byte 200. This is a
        // valid 256-byte catalog entry, so final wire validation must keep it
        // intact instead of slicing it or treating it as a mutation.
        let remote_model_id = format!("{}模{}", "a".repeat(199), "b".repeat(54));
        assert_eq!(remote_model_id.len(), 256);
        let response_body = serde_json::json!({
            "id": "resp-managed-256",
            "object": "response",
            "model": remote_model_id.clone(),
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "ok" }]
            }],
            "usage": { "input_tokens": 3, "output_tokens": 1, "total_tokens": 4 }
        })
        .to_string();

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-codex-256-model.sqlite"))
            .expect("init test db");
        let (upstream_url, captured_rx, upstream_task) =
            spawn_capturing_json_upstream(response_body).await;
        let provider_id = insert_codex_provider(&db, upstream_url);
        let canonical_model = insert_managed_codex_model(&db, provider_id, &remote_model_id);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured_body = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("upstream request")
            .expect("captured request body");
        let captured_json: Value = serde_json::from_str(&captured_body).expect("captured JSON");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some(remote_model_id.as_str())
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));
        let managed_route = special_settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("aio_managed_model_route")
            })
            .expect("managed route setting");
        assert_eq!(
            managed_route.get("remoteModelId").and_then(Value::as_str),
            Some(remote_model_id.as_str())
        );
        assert_eq!(
            managed_route
                .get("requestedUpstreamModel")
                .and_then(Value::as_str),
            Some(remote_model_id.as_str())
        );
        assert_eq!(
            managed_route.get("observation").and_then(Value::as_str),
            Some("matched")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_alias_failure_never_fails_over_to_another_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-codex-no-failover.sqlite"))
            .expect("init test db");
        let (bound_url, bound_calls, bound_task) = spawn_counting_status_upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"synthetic failure"}}"#,
        )
        .await;
        let (other_url, other_calls, other_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"must-not-run","object":"response","model":"grok-4.5","output":[]}"#,
        )
        .await;
        let bound_provider_id =
            insert_codex_provider_with_priority(&db, "Managed Failing", bound_url, 0);
        let other_provider_id =
            insert_codex_provider_with_priority(&db, "Managed Success", other_url, 1);
        let canonical_model = insert_managed_codex_model(&db, bound_provider_id, "grok-4.5");
        let _other_canonical = insert_managed_codex_model(&db, other_provider_id, "grok-4.5");

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(bound_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            other_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "managed route must not cross provider boundaries"
        );
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.requested_model.as_deref(),
            Some(canonical_model.as_str())
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(bound_provider_id)
        );

        bound_task.abort();
        other_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_before_send_model_mutation_has_one_terminal_log_and_zero_upstream_calls()
    {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-before-send-mutation.sqlite"))
            .expect("init test db");
        let (upstream_url, upstream_calls, upstream_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"must-not-run","object":"response","model":"grok-4.5","output":[]}"#,
        )
        .await;
        let provider_id = insert_codex_provider(&db, upstream_url);
        let canonical_model = insert_managed_codex_model(&db, provider_id, "grok-4.5");

        let mut plugin = before_send_header_plugin();
        set_granted_permissions(&mut plugin, &["request.body.read", "request.body.write"]);
        let executor =
            InMemoryGatewayPluginExecutor::new().with_request_handler("test.before-send", |ctx| {
                let mut body: Value = serde_json::from_str(
                    ctx.request.body.as_deref().expect("request body visible"),
                )
                .expect("request JSON");
                body["model"] = Value::String("tampered-model".to_string());
                GatewayHookResult {
                    request_body: Some(body.to_string()),
                    ..GatewayHookResult::continue_unchanged()
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![plugin],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let state = gateway_state_with_plugin_pipeline(app_handle, db, log_tx, plugin_pipeline);
        let active_requests = state.active_requests.clone();
        let router = build_router(state);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("response JSON");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::ManagedModelInvalid.as_str())
        );
        assert_eq!(
            upstream_calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "mutated managed model must fail before network I/O"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(400));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::ManagedModelInvalid.as_str())
        );
        assert_eq!(
            log.requested_model.as_deref(),
            Some(canonical_model.as_str())
        );
        let error_details: Value = serde_json::from_str(
            log.error_details_json
                .as_deref()
                .expect("error details JSON"),
        )
        .expect("parse error details");
        assert_eq!(
            error_details.get("error_category").and_then(Value::as_str),
            Some("NON_RETRYABLE_CLIENT_ERROR")
        );
        assert!(active_requests.snapshot().is_empty());

        let duplicate_terminal = tokio::time::timeout(Duration::from_millis(100), async {
            while let Some(item) = log_rx.recv().await {
                if item.status.is_some() {
                    return Some(item);
                }
            }
            None
        })
        .await;
        assert!(
            !matches!(duplicate_terminal, Ok(Some(_))),
            "managed model rejection must emit exactly one terminal request log"
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_alias_body_buffer_fake_200_keeps_matched_route_observation() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-body-fake-200.sqlite"))
            .expect("init test db");
        let fake_200_body = r#"{"model":"grok-4.5","error":{"message":"synthetic failure","type":"synthetic_error"}}"#;
        let (upstream_url, captured_rx, upstream_task) =
            spawn_capturing_json_upstream(fake_200_body).await;
        let provider_id = insert_codex_provider(&db, upstream_url);
        let canonical_model = insert_managed_codex_model(&db, provider_id, "grok-4.5");

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("response JSON");
        assert_eq!(
            payload.get("model").and_then(Value::as_str),
            Some("grok-4.5")
        );
        assert_eq!(
            payload.pointer("/error/type").and_then(Value::as_str),
            Some("synthetic_error")
        );

        let captured_body = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("upstream request")
            .expect("captured request body");
        let captured_json: Value =
            serde_json::from_str(&captured_body).expect("captured JSON body");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some("grok-4.5")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_managed_codex_matched_route_log(
            &log,
            canonical_model.as_str(),
            provider_id,
            "grok-4.5",
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_no_additional_terminal_request_log(&mut log_rx).await;

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_alias_completed_sse_keeps_matched_route_observation() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-completed-sse.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-managed-sse\",\"status\":\"completed\",\"model\":\"grok-4.5\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n"
        );
        let (upstream_url, upstream_task) = spawn_sse_upstream(sse_body).await;
        let provider_id = insert_codex_provider(&db, upstream_url);
        let canonical_model = insert_managed_codex_model(&db, provider_id, "grok-4.5");

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": true,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("response.output_text.delta"));
        assert!(body_text.contains("response.completed"));
        assert!(body_text.contains("resp-managed-sse"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert_managed_codex_matched_route_log(
            &log,
            canonical_model.as_str(),
            provider_id,
            "grok-4.5",
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_no_additional_terminal_request_log(&mut log_rx).await;

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_codex_retry_disabled_incomplete_sse_is_sanitized_and_keeps_route_observation()
    {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("managed-incomplete-sse.sqlite"))
            .expect("init test db");
        let incomplete_sse_body = concat!(
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp-managed-incomplete\",\"status\":\"incomplete\",\"model\":\"grok-4.5\",\"output\":[]}}\n\n"
        );
        let (upstream_url, upstream_task) = spawn_sse_upstream(incomplete_sse_body).await;
        let provider_id = insert_codex_provider(&db, upstream_url);
        let canonical_model = insert_managed_codex_model(&db, provider_id, "grok-4.5");

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": canonical_model,
                    "stream": true,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(!body_text.contains("response.incomplete"));
        assert!(!body_text.contains("resp-managed-incomplete"));
        let payload: Value = serde_json::from_slice(&body).expect("gateway error JSON");
        assert_eq!(payload["error_code"], "GW_FAKE_200");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_managed_codex_matched_route_log(
            &log,
            canonical_model.as_str(),
            provider_id,
            "grok-4.5",
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts[0]["decision"], "abort");
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"],
            "unknown"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"],
            "sanitized_before_commit"
        );
        assert_no_additional_terminal_request_log(&mut log_rx).await;

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_response_after_cannot_inject_non_stream_route_mapping() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-plugin-response-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_json_upstream(r#"{"id":"original","object":"chat.completion","choices":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor = InMemoryGatewayPluginExecutor::new().with_response_handler(
            "test.response-after",
            |_ctx| GatewayHookResult {
                response_body: Some(
                    r#"{"id":"rewritten","object":"chat.completion","model":"gpt-injected","reasoning":{"effort":"medium"},"choices":[]}"#.to_string(),
                ),
                ..GatewayHookResult::continue_unchanged()
            },
        );
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![response_after_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","reasoning":{"effort":"high"},"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload.get("id").and_then(Value::as_str), Some("rewritten"));
        assert_eq!(
            payload.get("model").and_then(Value::as_str),
            Some("gpt-injected")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_response_after_fail_closed_error_replaces_upstream_success() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-response-fail-closed-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_json_upstream(r#"{"id":"original","object":"chat.completion","choices":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor = InMemoryGatewayPluginExecutor::new().with_response_handler(
            "test.response-after",
            |_ctx| {
                let mut result = GatewayHookResult::continue_unchanged();
                result
                    .headers
                    .insert("x-aio-forbidden".to_string(), "1".to_string());
                result
            },
        );
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![fail_closed(response_after_plugin())],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let state = gateway_state_with_plugin_pipeline(app_handle, db, log_tx, plugin_pipeline);
        let active_requests = state.active_requests.clone();
        let router = build_router(state);
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );
        assert_ne!(payload.get("id").and_then(Value::as_str), Some("original"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );
        assert!(active_requests.snapshot().is_empty());
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_response_after_block_writes_terminal_log_and_clears_active_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-response-block-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_json_upstream(r#"{"id":"original","object":"chat.completion","choices":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor = InMemoryGatewayPluginExecutor::new().with_response_handler(
            "test.response-after",
            |_ctx| {
                let mut result = GatewayHookResult::continue_unchanged();
                result.action = crate::gateway::plugins::context::GatewayHookAction::Block;
                result.reason = Some("response blocked after upstream success".to_string());
                result
            },
        );
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![response_after_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let state = gateway_state_with_plugin_pipeline(app_handle, db, log_tx, plugin_pipeline);
        let active_requests = state.active_requests.clone();
        let router = build_router(state);
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::InternalError.as_str())
        );
        assert!(active_requests.snapshot().is_empty());
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_response_chunk_rewrites_stream_body_without_hiding_upstream_route() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-plugin-stream-test.sqlite"))
            .expect("init test db");
        let upstream_body = concat!(
            "data: {\"id\":\"chatcmpl-route\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-5.4-mini\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"secret-stream\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let (upstream_base_url, upstream_task) = spawn_sse_upstream(upstream_body).await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_stream_handler("test.stream-chunk", |ctx| {
                let chunk = ctx.stream.chunk.expect("visible stream chunk");
                assert!(chunk.contains("secret-stream"));
                GatewayHookResult {
                    stream_chunk: Some(
                        chunk
                            .replace("secret-stream", "redacted-stream")
                            .replace("gpt-5.4-mini", "gpt-5.5"),
                    ),
                    ..GatewayHookResult::continue_unchanged()
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![stream_chunk_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-5.5","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(
            body.contains("redacted-stream"),
            "stream body was not rewritten: {body}"
        );
        assert!(
            !body.contains("secret-stream"),
            "stream body leaked secret: {body}"
        );
        assert!(
            body.contains("gpt-5.5"),
            "stream model was not rewritten: {body}"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        let settings: Vec<Value> = serde_json::from_str(
            log.special_settings_json
                .as_deref()
                .expect("route mapping settings"),
        )
        .expect("valid route mapping settings");
        let mapping = settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
            })
            .expect("model route mapping");
        assert_eq!(
            mapping.get("requestedModel").and_then(Value::as_str),
            Some("gpt-5.5")
        );
        assert_eq!(
            mapping.get("actualModel").and_then(Value::as_str),
            Some("gpt-5.4-mini")
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_response_chunk_block_emits_stream_error_event() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-stream-block-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_sse_upstream("data: dangerous-command\n\n").await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_stream_handler("test.stream-chunk", |ctx| {
                assert!(ctx
                    .stream
                    .chunk
                    .as_deref()
                    .is_some_and(|chunk| chunk.contains("dangerous-command")));
                let mut result = GatewayHookResult::continue_unchanged();
                result.action = crate::gateway::plugins::context::GatewayHookAction::Block;
                result.reason = Some("dangerous command detected".to_string());
                result
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![stream_chunk_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(
            body.contains("event: error"),
            "stream block did not emit error event: {body}"
        );
        assert!(
            body.contains("plugin_blocked"),
            "stream block reason missing: {body}"
        );
        assert!(
            !body.contains("dangerous-command"),
            "blocked stream leaked chunk: {body}"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::Fake200.as_str())
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plugin_log_redaction_rewrites_request_log_before_enqueue() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-plugin-log-redaction-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, upstream_task) =
            spawn_json_upstream(r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#)
                .await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_log_handler("test.log-redaction", |ctx| {
                let message = ctx.log.message.expect("visible log message");
                assert!(message.contains("secret-query"));
                GatewayHookResult {
                    log_message: Some(message.replace("secret-query", "[REDACTED]")),
                    ..GatewayHookResult::continue_unchanged()
                }
            });
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![log_redaction_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions?token=secret-query"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.query.as_deref(), Some("token=[REDACTED]"));
        assert_ne!(log.query.as_deref(), Some("token=secret-query"));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_plugin_error_hook_rewrites_gateway_error_response() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-plugin-error-test.sqlite"))
            .expect("init test db");

        let executor = InMemoryGatewayPluginExecutor::new().with_response_handler(
            "test.gateway-error",
            |ctx| {
                assert_eq!(ctx.hook_name, "gateway.error");
                assert_eq!(ctx.response.status, Some(503));
                assert!(ctx
                    .response
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("GW_NO_ENABLED_PROVIDER")));
                let mut result = GatewayHookResult {
                    response_body: Some(
                        r#"{"error_code":"GW_NO_ENABLED_PROVIDER","message":"plugin-friendly error","attempts":[]}"#
                            .to_string(),
                    ),
                    ..GatewayHookResult::continue_unchanged()
                };
                result
                    .headers
                    .insert("x-plugin-error".to_string(), "rewritten".to_string());
                result
            },
        );
        let plugin_pipeline = GatewayPluginPipeline::for_tests_shared(
            vec![gateway_error_plugin()],
            Arc::new(executor),
            GatewayPluginPipelineConfig::default(),
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_plugin_pipeline(
            app_handle,
            db,
            log_tx,
            plugin_pipeline,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get("x-plugin-error")
                .and_then(|value| value.to_str().ok()),
            Some("rewritten")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("message").and_then(Value::as_str),
            Some("plugin-friendly error")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(503));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::NoEnabledProvider.as_str())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_fails_over_from_timeout_to_second_provider_success() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.upstream_first_byte_timeout_seconds = 1;
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-failover-test.sqlite"))
            .expect("init test db");
        let (timeout_base_url, timeout_task) = spawn_hanging_upstream().await;
        let success_body = r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let timeout_provider_id =
            insert_codex_provider_with_priority(&db, "Timeout Stub", timeout_base_url, 0);
        let success_provider_id =
            insert_codex_provider_with_priority(&db, "Success Stub", success_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-failover","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload.get("id").and_then(Value::as_str), Some("stub-ok"));

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log enqueue")
            .expect("request log item");
        assert_eq!(log.cli_key, "codex");
        assert_eq!(log.path, "/v1/chat/completions");
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert_eq!(log.requested_model.as_deref(), Some("gpt-route-failover"));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(timeout_provider_id)
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamTimeout.as_str())
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("request_timeout: category=SYSTEM_ERROR code=GW_UPSTREAM_TIMEOUT decision=switch timeout_secs=1")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let provider_chain: Value =
            serde_json::from_str(log.provider_chain_json.as_deref().expect("provider chain"))
                .expect("provider chain json");
        let chain = provider_chain.as_array().expect("provider chain array");
        assert_eq!(chain.len(), 2);
        assert_eq!(
            chain[0].get("provider_id").and_then(Value::as_i64),
            Some(timeout_provider_id)
        );
        assert_eq!(
            chain[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );

        timeout_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_429_quota_fails_over_without_same_provider_retry() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 5;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 30;
        app_settings.upstream_error_response_rules = vec![test_upstream_error_response_rule(
            429,
            settings::UpstreamErrorStatusBehavior::Override { status_code: 503 },
            settings::UpstreamErrorMessageBehavior::Override {
                message: "must not leak after success".to_string(),
            },
        )];
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-429-quota-test.sqlite"))
            .expect("init test db");
        let quota_body = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#;
        let success_body = r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#;
        let (quota_base_url, quota_task) =
            spawn_status_json_upstream("429 Too Many Requests", quota_body).await;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let quota_provider_id =
            insert_codex_provider_with_priority(&db, "429 Quota Stub", quota_base_url, 0);
        let success_provider_id =
            insert_codex_provider_with_priority(&db, "Success Stub", success_base_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-429-quota","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert!(!has_upstream_error_response_rule_marker(&log));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(quota_provider_id)
        );
        assert_eq!(
            attempts[0].get("retry_index").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert!(attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("rule=quota_exhausted")));
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let circuit_snapshot = circuit.snapshot(quota_provider_id, 0);
        assert!(circuit_snapshot.cooldown_until.is_some());

        quota_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_error_response_rule_rewrites_direct_abort_after_original_attempt_audit() {
        let observation = run_codex_error_response_rule_route(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"upstream request detail"}}"#,
            test_upstream_error_response_rule(
                400,
                settings::UpstreamErrorStatusBehavior::Override { status_code: 422 },
                settings::UpstreamErrorMessageBehavior::Passthrough,
            ),
        )
        .await;

        assert_eq!(observation.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            observation.response["error"]["message"].as_str(),
            Some("upstream request detail")
        );
        assert_eq!(observation.log.status, Some(422));
        let attempts: Value =
            serde_json::from_str(&observation.log.attempts_json).expect("attempts json");
        assert_eq!(attempts[0]["status"].as_u64(), Some(400));
        assert_eq!(
            attempts[0]["provider_id"].as_i64(),
            Some(observation.provider_id)
        );
        assert!(attempts[0]["reason"]
            .as_str()
            .is_some_and(|reason| !reason.contains("upstream request detail")));

        let marker = parse_special_settings(&observation.log)
            .into_iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("upstream_error_response_rule")
            })
            .expect("response rule marker");
        assert_eq!(marker["providerId"].as_i64(), Some(observation.provider_id));
        assert_eq!(marker["upstreamStatus"].as_u64(), Some(400));
        assert_eq!(marker["clientStatus"].as_u64(), Some(422));
        assert_eq!(marker["messageMode"].as_str(), Some("passthrough"));
        let special_settings_json = observation
            .log
            .special_settings_json
            .as_deref()
            .expect("response rule special settings");
        assert!(!special_settings_json.contains("upstream request detail"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_error_response_rule_rewrites_last_all_failed_attempt() {
        let observation = run_codex_error_response_rule_route(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"raw provider failure"}}"#,
            test_upstream_error_response_rule(
                500,
                settings::UpstreamErrorStatusBehavior::Override { status_code: 503 },
                settings::UpstreamErrorMessageBehavior::Override {
                    message: "service temporarily unavailable".to_string(),
                },
            ),
        )
        .await;

        assert_eq!(observation.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            observation.response["error"]["message"].as_str(),
            Some("service temporarily unavailable")
        );
        assert_eq!(observation.log.status, Some(503));
        let attempts: Value =
            serde_json::from_str(&observation.log.attempts_json).expect("attempts json");
        assert_eq!(attempts[0]["status"].as_u64(), Some(500));
        assert_eq!(
            attempts[0]["provider_id"].as_i64(),
            Some(observation.provider_id)
        );
        assert!(has_upstream_error_response_rule_marker(&observation.log));
        let special_settings_json = observation
            .log
            .special_settings_json
            .as_deref()
            .expect("response rule special settings");
        assert!(!special_settings_json.contains("service temporarily unavailable"));
        assert!(!special_settings_json.contains("raw provider failure"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_gzip_body_rule_retries_same_provider_and_records_safe_rule_reason() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 503,
                body_contains: vec!["synthetic_body_match".to_string()],
                description: "temporary upstream".to_string(),
            }],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-gzip.sqlite"))
            .expect("init test db");
        let error_body =
            gzip_bytes(br#"{"error":{"message":"SYNTHETIC_BODY_MATCH SYNTHETIC_BODY_SECRET"}}"#);
        let success_body = r#"{"id":"retry-rule-ok","object":"chat.completion","choices":[]}"#;
        let (base_url, call_count, upstream_task) =
            spawn_retry_rule_upstream("503 Service Unavailable", error_body, true, success_body)
                .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Gzip Retry Rule Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-rule","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);
        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("retry")
        );
        let reason = attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .expect("rule reason");
        assert!(reason.contains("retry_rule=1"));
        assert!(reason.contains("retry_rule_description=temporary upstream"));
        assert!(!reason.contains("SYNTHETIC_BODY_MATCH"));
        assert!(!reason.contains("SYNTHETIC_BODY_SECRET"));
        assert!(!log.attempts_json.contains("SYNTHETIC_BODY_SECRET"));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unmatched_http_rule_does_not_expand_the_baseline_provider_budget() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 503,
                body_contains: vec!["required marker".to_string()],
                description: String::new(),
            }],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-unmatched.sqlite"))
            .expect("init test db");
        let (base_url, call_count, upstream_task) = spawn_retry_rule_upstream(
            "503 Service Unavailable",
            br#"{"error":"different marker"}"#.to_vec(),
            false,
            r#"{"id":"must-not-retry"}"#,
        )
        .await;
        insert_codex_provider_with_priority(&db, "Unmatched Retry Rule Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-rule-unmatched","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert!(!attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("retry_rule="));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_auth_body_rule_retries_without_persisting_auth_body() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 401,
                body_contains: vec!["synthetic_auth_match".to_string()],
                description: "auth retry".to_string(),
            }],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-auth.sqlite"))
            .expect("init test db");
        let success_body = r#"{"id":"auth-retry-ok","object":"chat.completion","choices":[]}"#;
        let (base_url, call_count, upstream_task) = spawn_retry_rule_upstream(
            "401 Unauthorized",
            br#"{"error":"SYNTHETIC_AUTH_MATCH SYNTHETIC_AUTH_SECRET"}"#.to_vec(),
            false,
            success_body,
        )
        .await;
        insert_codex_provider_with_priority(&db, "Auth Retry Rule Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-auth-retry","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert!(log.attempts_json.contains("retry_rule=1"));
        assert!(!log.attempts_json.contains("SYNTHETIC_AUTH_MATCH"));
        assert!(!log.attempts_json.contains("SYNTHETIC_AUTH_SECRET"));
        assert!(!log
            .error_details_json
            .as_deref()
            .unwrap_or_default()
            .contains("SYNTHETIC_AUTH_SECRET"));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn internal_repair_does_not_consume_the_configured_retry_budget() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.enable_codex_session_id_completion = false;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule::status_only(503)],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let success_body =
            r#"{"id":"configured-retry-after-repair","object":"response","output":[]}"#;
        let (base_url, mut captured_rx, upstream_task) =
            spawn_previous_response_then_retry_rule_upstream(success_body).await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-retry-rule-after-internal-repair.sqlite"),
        )
        .expect("init test db");
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Internal Then Configured Retry Stub",
            base_url,
            0,
        );
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-after-repair","previous_response_id":"resp_old","input":"hello","stream":false}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let response: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("response JSON");
        assert_eq!(
            response.get("id").and_then(Value::as_str),
            Some("configured-retry-after-repair")
        );

        let first = captured_rx.recv().await.expect("first request");
        let second = captured_rx.recv().await.expect("second request");
        let third = captured_rx.recv().await.expect("third request");
        assert!(String::from_utf8_lossy(&first.body).contains("previous_response_id"));
        assert!(!String::from_utf8_lossy(&second.body).contains("previous_response_id"));
        assert!(!String::from_utf8_lossy(&third.body).contains("previous_response_id"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("retry")
        );
        assert!(attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("retry_rule=1")));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configured_gzip_body_rule_does_not_scan_beyond_decoded_prefix() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 400,
                body_contains: vec!["after_prefix_marker".to_string()],
                description: String::new(),
            }],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-gzip-prefix.sqlite"))
            .expect("init test db");
        let mut decoded_error_body = vec![b'x'; 64 * 1024];
        decoded_error_body.extend_from_slice(b"AFTER_PREFIX_MARKER");
        let (base_url, call_count, upstream_task) = spawn_retry_rule_upstream(
            "400 Bad Request",
            gzip_bytes(&decoded_error_body),
            true,
            r#"{"id":"must-not-retry"}"#,
        )
        .await;
        insert_codex_provider_with_priority(&db, "Gzip Prefix Rule Stub", base_url, 0);
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-rule-prefix","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "content after the decoded 64 KiB prefix must not trigger a retry"
        );
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exhausted_configured_retry_records_only_the_final_circuit_failure() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.circuit_breaker_failure_threshold = 5;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule::status_only(503)],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: false,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-exhausted.sqlite"))
            .expect("init test db");
        let (base_url, call_count, upstream_task) = spawn_counting_status_upstream(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"still unavailable"}"#,
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Exhausted Retry Rule Stub", base_url, 0);
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-rule-exhausted","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("retry")
        );
        assert!(attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("retry_rule=1")));
        assert_eq!(
            attempts[0]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            attempts[1].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert!(!attempts[1]
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("retry_rule="));
        assert_eq!(
            attempts[1]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 1);
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn circuit_open_switch_is_not_reported_as_an_actual_configured_retry() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.upstream_retry_policy = settings::UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![settings::UpstreamHttpRetryRule::status_only(503)],
            transport_errors: Vec::new(),
            stream_internal_errors: Default::default(),
            max_retries: 1,
            backoff_ms: 0,
            counts_toward_circuit_breaker: true,
        };
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-retry-rule-circuit-open.sqlite"))
            .expect("init test db");
        let (base_url, call_count, upstream_task) = spawn_counting_status_upstream(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":"unavailable"}"#,
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Circuit Open Retry Rule Stub", base_url, 0);
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-retry-rule-circuit-open","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert!(!attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("retry_rule="));
        assert_eq!(
            attempts[0]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 1);
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_skips_exhausted_oauth_snapshot_without_opening_circuit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 30;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-oauth-quota-test.sqlite"))
            .expect("init test db");
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let oauth_provider_id =
            insert_codex_oauth_provider_with_priority(&db, "OAuth Quota Stub", 0);
        crate::domain::provider_oauth_limits::save_exhausted_snapshot(
            &db,
            oauth_provider_id,
            Some(now + 3_600),
        )
        .expect("save oauth exhausted snapshot");

        let success_body = r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let success_provider_id =
            insert_codex_provider_with_priority(&db, "Success Stub", success_base_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-oauth-quota","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(oauth_provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::ProviderRateLimited.as_str())
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let oauth_circuit_snapshot = circuit.snapshot(oauth_provider_id, 0);
        assert_eq!(
            oauth_circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(oauth_circuit_snapshot.failure_count, 0);
        assert!(oauth_circuit_snapshot.cooldown_until.is_none());

        success_task.abort();
    }

    fn all_open_probe_request(session_id: &str, model: &str) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                serde_json::json!({
                    "model": model,
                    "stream": false,
                    "input": [
                        {
                            "type": "message",
                            "role": "user",
                            "content": [{"type": "input_text", "text": "before"}]
                        },
                        {
                            "type": "message",
                            "role": "user",
                            "content": [{"type": "input_text", "text": "now"}]
                        }
                    ]
                })
                .to_string(),
            ))
            .expect("request")
    }

    fn request_log_attempts(log: &request_logs::RequestLogInsert) -> Vec<Value> {
        serde_json::from_str::<Value>(&log.attempts_json)
            .expect("attempts json")
            .as_array()
            .expect("attempt array")
            .clone()
    }

    fn request_log_provider_chain(log: &request_logs::RequestLogInsert) -> Vec<Value> {
        serde_json::from_str::<Value>(log.provider_chain_json.as_deref().expect("provider chain"))
            .expect("provider chain json")
            .as_array()
            .expect("provider chain array")
            .clone()
    }

    fn assert_logged_provider_order(rows: &[Value], expected_provider_ids: &[i64]) {
        let actual_provider_ids: Vec<_> = rows
            .iter()
            .map(|row| {
                row.get("provider_id")
                    .and_then(Value::as_i64)
                    .expect("logged provider id")
            })
            .collect();
        assert_eq!(actual_provider_ids, expected_provider_ids);
    }

    async fn route_ordered_failback_json_response(
        router: axum::Router,
        session_id: &str,
        model: &str,
    ) -> (StatusCode, Value) {
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            router.oneshot(all_open_probe_request(session_id, model)),
        )
        .await
        .expect("route ordered failback response timeout")
        .expect("route response");
        let status = response.status();
        let body = tokio::time::timeout(
            Duration::from_secs(3),
            to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("route ordered failback body timeout")
        .expect("route response body");
        let payload = serde_json::from_slice(&body).expect("route response JSON");
        (status, payload)
    }

    async fn recv_terminal_request_logs_by_session(
        log_rx: &mut tokio::sync::mpsc::Receiver<request_logs::RequestLogInsert>,
        expected_count: usize,
    ) -> HashMap<String, request_logs::RequestLogInsert> {
        let mut logs = HashMap::with_capacity(expected_count);
        for _ in 0..expected_count {
            let log = recv_terminal_request_log(log_rx).await;
            let session_id = log.session_id.clone().expect("request log session id");
            assert!(
                logs.insert(session_id.clone(), log).is_none(),
                "duplicate terminal request log for session {session_id}"
            );
        }
        logs
    }

    fn assert_direct_attempt_without_probe_metadata(attempt: &Value) {
        assert_ne!(attempt.get("probe").and_then(Value::as_bool), Some(true));
        assert_ne!(
            attempt.get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(attempt.get("probe_trigger").and_then(Value::as_str), None);
        assert_eq!(attempt.get("probe_result").and_then(Value::as_str), None);
        assert_eq!(
            attempt.get("probe_generation").and_then(Value::as_u64),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_closed_p1_failure_then_open_p2_success_precedes_current_p3(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 3;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-p1-p2-p3.sqlite"),
        )
        .expect("init test db");
        let failed_body = r#"{"error":{"message":"p1 failed"}}"#;
        let p2_body = r#"{"id":"ordered-p2-ok","object":"response","status":"completed","model":"gpt-ordered-p1-p2-p3","output":[]}"#;
        let p3_body = r#"{"id":"current-p3-must-not-run","object":"response","status":"completed","model":"gpt-ordered-p1-p2-p3","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failed_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, p2_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, p3_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Ordered P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Ordered P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Current P3", p3_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p2_id, now.saturating_sub(31), None);
        assert_eq!(
            circuit.snapshot(p2_id, now).state,
            circuit_breaker::CircuitState::Open
        );

        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000201";
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p3_id]), now);
        session.bind_success("codex", session_id, p3_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            router.oneshot(all_open_probe_request(session_id, "gpt-ordered-p1-p2-p3")),
        )
        .await
        .expect("route response timeout")
        .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("ordered-p2-ok")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.session_id.as_deref(), Some(session_id));
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
        assert_eq!(attempts[0].get("status").and_then(Value::as_i64), Some(500));
        assert_ne!(
            attempts[0].get("probe").and_then(Value::as_bool),
            Some(true)
        );
        assert_ne!(
            attempts[0].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[0].get("probe_trigger").and_then(Value::as_str),
            None
        );
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            None
        );
        assert_eq!(
            attempts[1].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[1].get("probe_trigger").and_then(Value::as_str),
            Some("route_changed")
        );
        assert_eq!(
            attempts[1].get("probe_result").and_then(Value::as_str),
            Some("success")
        );
        assert!(attempts[1]
            .get("probe_generation")
            .and_then(Value::as_u64)
            .is_some());
        assert_logged_provider_order(&request_log_provider_chain(&log), &[p1_id, p2_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p2_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id])
        );
        assert_eq!(
            circuit.snapshot(p2_id, now).state,
            circuit_breaker::CircuitState::Closed
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_five_provider_prefix_reaches_p4_before_current_p5()
    {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 5;
        app_settings.circuit_breaker_failure_threshold = 5;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-five-provider.sqlite"),
        )
        .expect("init test db");
        let failure_body = r#"{"error":{"message":"ordered target failed"}}"#;
        let success_body = r#"{"id":"ordered-p4-ok","object":"response","status":"completed","model":"gpt-ordered-five","output":[]}"#;
        let unused_body = r#"{"id":"current-p5-must-not-run","object":"response","status":"completed","model":"gpt-ordered-five","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p4_url, p4_calls, p4_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (p5_url, p5_calls, p5_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Dynamic P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Dynamic P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Dynamic P3", p3_url, 2);
        let p4_id = insert_codex_provider_with_priority(&db, "Dynamic P4", p4_url, 3);
        let p5_id = insert_codex_provider_with_priority(&db, "Current P5", p5_url, 4);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                provider_cooldown_secs: 0,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000202";
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p5_id]), now);
        session.bind_success("codex", session_id, p5_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-ordered-five"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("ordered-p4-ok")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        let expected_order = [p1_id, p2_id, p3_id, p4_id];
        assert_logged_provider_order(&attempts, &expected_order);
        for attempt in &attempts[..3] {
            assert_eq!(attempt.get("status").and_then(Value::as_i64), Some(500));
        }
        assert_eq!(
            attempts[3].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_logged_provider_order(&request_log_provider_chain(&log), &expected_order);
        for call_count in [&p1_calls, &p2_calls, &p3_calls, &p4_calls] {
            assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
        assert_eq!(p5_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p4_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id, p4_id, p5_id])
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
        p4_task.abort();
        p5_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_intermediate_success_short_circuits_later_targets_and_current(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 5;
        app_settings.circuit_breaker_failure_threshold = 5;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-short-circuit.sqlite"),
        )
        .expect("init test db");
        let failure_body = r#"{"error":{"message":"p1 failed"}}"#;
        let success_body = r#"{"id":"short-circuit-p2-ok","object":"response","status":"completed","model":"gpt-ordered-short-circuit","output":[]}"#;
        let unused_body = r#"{"id":"must-not-run","object":"response","status":"completed","model":"gpt-ordered-short-circuit","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p4_url, p4_calls, p4_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p5_url, p5_calls, p5_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Short P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Short P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Short P3", p3_url, 2);
        let p4_id = insert_codex_provider_with_priority(&db, "Short P4", p4_url, 3);
        let p5_id = insert_codex_provider_with_priority(&db, "Current P5", p5_url, 4);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                provider_cooldown_secs: 0,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000203";
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p5_id]), now);
        session.bind_success("codex", session_id, p5_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-ordered-short-circuit",
            ))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("short-circuit-p2-ok")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
        assert_logged_provider_order(&request_log_provider_chain(&log), &[p1_id, p2_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        for call_count in [&p3_calls, &p4_calls, &p5_calls] {
            assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        }
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p2_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id, p4_id, p5_id])
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
        p4_task.abort();
        p5_task.abort();
    }

    #[derive(Clone, Copy)]
    enum OrderedFailbackGateBlock {
        Cooldown,
        InFlight,
    }

    async fn assert_ordered_failback_gate_skip_continues(block: OrderedFailbackGateBlock) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let (label, expected_probe_result, session_id) = match block {
            OrderedFailbackGateBlock::Cooldown => (
                "cooldown",
                "cooldown",
                "0190c0de-0000-7000-8000-000000000204",
            ),
            OrderedFailbackGateBlock::InFlight => (
                "in-flight",
                "in_flight",
                "0190c0de-0000-7000-8000-000000000205",
            ),
        };
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join(format!(
            "gateway-route-ordered-failback-{label}-skip.sqlite"
        )))
        .expect("init test db");
        let unused_body =
            r#"{"id":"p1-must-not-run","object":"response","status":"completed","output":[]}"#;
        let success_body = r#"{"id":"skip-then-p2-ok","object":"response","status":"completed","model":"gpt-ordered-skip","output":[]}"#;
        let current_body =
            r#"{"id":"p3-must-not-run","object":"response","status":"completed","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, current_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Skipped P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Ready P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Current P3", p3_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let opened_at = match block {
            OrderedFailbackGateBlock::Cooldown => now,
            OrderedFailbackGateBlock::InFlight => now.saturating_sub(31),
        };
        circuit.record_failure(p1_id, opened_at, None);
        let _existing_probe = match block {
            OrderedFailbackGateBlock::Cooldown => None,
            OrderedFailbackGateBlock::InFlight => {
                let token = match circuit.try_acquire_probe(
                    p1_id,
                    "existing-ordered-probe",
                    circuit_breaker::ProbeTrigger::RouteChanged,
                    now,
                ) {
                    circuit_breaker::ProbeAcquireResult::Acquired { token, .. } => token,
                    other => panic!("expected existing probe lease, got {other:?}"),
                };
                Some(circuit_breaker::ProbeLeaseGuard::new(
                    circuit.clone(),
                    token,
                ))
            }
        };

        let session = Arc::new(session_manager::SessionManager::new());
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p3_id]), now);
        session.bind_success("codex", session_id, p3_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-ordered-skip"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("skip-then-p2-ok")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[0].get("probe_trigger").and_then(Value::as_str),
            Some("route_changed")
        );
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            Some(expected_probe_result)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_ne!(
            attempts[1].get("probe").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            attempts[1].get("probe_trigger").and_then(Value::as_str),
            None
        );
        assert_logged_provider_order(&request_log_provider_chain(&log), &[p1_id, p2_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p2_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id])
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_cooldown_skip_continues_without_consuming_ready_cap_or_reservation(
    ) {
        assert_ordered_failback_gate_skip_continues(OrderedFailbackGateBlock::Cooldown).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_in_flight_skip_continues_without_consuming_ready_cap_or_reservation(
    ) {
        assert_ordered_failback_gate_skip_continues(OrderedFailbackGateBlock::InFlight).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_natural_not_triggered_p1_does_not_block_due_p2_probe() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-not-triggered.sqlite"),
        )
        .expect("init test db");
        let unused_body =
            r#"{"id":"p1-must-not-run","object":"response","status":"completed","output":[]}"#;
        let success_body = r#"{"id":"natural-p2-ok","object":"response","status":"completed","model":"gpt-natural-ordered","output":[]}"#;
        let current_body =
            r#"{"id":"p3-must-not-run","object":"response","status":"completed","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, current_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Healthy P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Due P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Current P3", p3_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 30,
                provider_cooldown_secs: 30,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p2_id, now.saturating_sub(31), None);
        let p2_before = circuit.snapshot(p2_id, now);
        assert_eq!(p2_before.state, circuit_breaker::CircuitState::Open);
        assert!(p2_before.open_until.is_some_and(|until| until <= now));

        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000206";
        let latest_route = vec![p1_id, p2_id, p3_id];
        session.bind_sort_mode("codex", session_id, None, Some(latest_route.clone()), now);
        session.bind_success("codex", session_id, p3_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-natural-ordered"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(attempts[0].get("provider_index"), Some(&Value::Null));
        assert_eq!(attempts[0].get("retry_index"), Some(&Value::Null));
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            Some("not_triggered")
        );
        assert_eq!(
            attempts[1].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[1].get("probe_trigger").and_then(Value::as_str),
            Some("max_open_wait")
        );
        assert_eq!(
            attempts[1].get("probe_result").and_then(Value::as_str),
            Some("success")
        );
        assert_logged_provider_order(&request_log_provider_chain(&log), &[p2_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            circuit.snapshot(p2_id, now).state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p2_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(latest_route)
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum NaturalFailbackWinnerOutcome {
        Success,
        Failure,
    }

    async fn assert_route_ordered_failback_natural_multi_session_convergence(
        winner_outcome: NaturalFailbackWinnerOutcome,
    ) {
        const MODEL: &str = "gpt-natural-session-convergence";
        const FOLLOWER_COUNT: usize = 4;

        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        app_settings.natural_probe_max_wait_seconds = 60;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let (label, p1_status, p1_body) = match winner_outcome {
            NaturalFailbackWinnerOutcome::Success => (
                "success",
                StatusCode::OK,
                r#"{"id":"recovered-p1-ok","object":"response","status":"completed","model":"gpt-natural-session-convergence","output":[]}"#,
            ),
            NaturalFailbackWinnerOutcome::Failure => (
                "failure",
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":{"message":"natural probe failed"}}"#,
            ),
        };
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join(format!(
            "gateway-route-ordered-failback-session-convergence-{label}.sqlite"
        )))
        .expect("init test db");
        let mut p1_upstream = spawn_gated_counting_status_upstream(p1_status, p1_body).await;
        let current_body = r#"{"id":"current-p2-ok","object":"response","status":"completed","model":"gpt-natural-session-convergence","output":[]}"#;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, current_body).await;
        let p1_id = insert_codex_provider_with_priority(
            &db,
            "Recovering P1",
            p1_upstream.base_url.clone(),
            0,
        );
        let p2_id = insert_codex_provider_with_priority(&db, "Current P2", p2_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                natural_probe_max_wait_secs: 60,
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p1_id, now.saturating_sub(61), None);
        let due_snapshot = circuit.snapshot(p1_id, now);
        assert_eq!(due_snapshot.state, circuit_breaker::CircuitState::Open);
        assert!(
            due_snapshot
                .natural_probe_due_at
                .is_some_and(|due| due <= now),
            "P1 natural 60-second deadline must already be due"
        );
        assert!(
            due_snapshot.open_until.is_some_and(|until| until > now),
            "natural max-wait, not OPEN expiry, must trigger the winner"
        );
        assert!(
            due_snapshot.cooldown_until.is_none_or(|until| until <= now),
            "P1 provider cooldown must already be over"
        );

        let winner_session_id = "0190c0de-0000-7000-8000-000000000300".to_string();
        let follower_session_ids: Vec<String> = (0..FOLLOWER_COUNT)
            .map(|index| format!("0190c0de-0000-7000-8000-{:012x}", 0x301_u64 + index as u64))
            .collect();
        assert!(follower_session_ids.len() >= 3);
        let initial_recovery_epoch = circuit.recovery_epoch();
        let latest_route = vec![p1_id, p2_id];
        let session = Arc::new(session_manager::SessionManager::new());
        for session_id in std::iter::once(&winner_session_id).chain(&follower_session_ids) {
            let binding_request = session
                .begin_binding_request()
                .expect("initial session binding request");
            assert!(session.bind_sort_mode_with_recovery_epoch(
                "codex",
                session_id,
                session_manager::SessionBindingCreation::new(
                    None,
                    Some(latest_route.clone()),
                    initial_recovery_epoch,
                    0,
                    binding_request,
                ),
                now,
            ));
            session.bind_success("codex", session_id, p2_id, None, now);
        }

        let expected_log_count = 1 + follower_session_ids.len() * 2;
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(expected_log_count + 2);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));

        let winner_task = {
            let router = router.clone();
            let session_id = winner_session_id.clone();
            tokio::spawn(async move {
                let response =
                    route_ordered_failback_json_response(router, &session_id, MODEL).await;
                (session_id, response)
            })
        };
        p1_upstream.wait_for_first_request().await;
        assert_eq!(p1_upstream.calls(), 1);

        let first_wave_tasks: Vec<_> = follower_session_ids
            .iter()
            .map(|session_id| {
                let session_id = session_id.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    let response =
                        route_ordered_failback_json_response(router, &session_id, MODEL).await;
                    (session_id, response)
                })
            })
            .collect();
        for task in first_wave_tasks {
            let (session_id, (status, payload)) = task.await.expect("first-wave follower task");
            assert_eq!(status, StatusCode::OK, "first-wave follower {session_id}");
            assert_eq!(
                payload.get("id").and_then(Value::as_str),
                Some("current-p2-ok"),
                "first-wave follower {session_id} must continue on current P2"
            );
        }
        assert_eq!(
            p1_upstream.calls(),
            1,
            "the winner must be the only first-wave P1 network call"
        );
        assert_eq!(
            p2_calls.load(std::sync::atomic::Ordering::SeqCst),
            follower_session_ids.len(),
            "every first-wave follower must continue on current P2"
        );

        let first_wave_logs =
            recv_terminal_request_logs_by_session(&mut log_rx, follower_session_ids.len()).await;
        for session_id in &follower_session_ids {
            let log = first_wave_logs
                .get(session_id)
                .expect("first-wave follower log");
            assert_eq!(log.status, Some(200));
            let attempts = request_log_attempts(log);
            assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
            assert_eq!(
                attempts[0].get("outcome").and_then(Value::as_str),
                Some("skipped")
            );
            assert_eq!(
                attempts[0].get("probe_result").and_then(Value::as_str),
                Some("in_flight")
            );
            assert_eq!(
                attempts[1].get("outcome").and_then(Value::as_str),
                Some("success")
            );
            assert_direct_attempt_without_probe_metadata(&attempts[1]);
            assert_logged_provider_order(&request_log_provider_chain(log), &[p1_id, p2_id]);
            assert_eq!(
                session.get_bound_provider("codex", session_id, now),
                Some(p2_id)
            );
        }

        p1_upstream.release_first_response();
        let (completed_winner_session_id, (winner_status, winner_payload)) =
            winner_task.await.expect("winner task");
        assert_eq!(completed_winner_session_id, winner_session_id);
        assert_eq!(winner_status, StatusCode::OK);
        let winner_log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(
            winner_log.session_id.as_deref(),
            Some(winner_session_id.as_str())
        );
        let winner_attempts = request_log_attempts(&winner_log);

        match winner_outcome {
            NaturalFailbackWinnerOutcome::Success => {
                assert_eq!(
                    winner_payload.get("id").and_then(Value::as_str),
                    Some("recovered-p1-ok")
                );
                assert_logged_provider_order(&winner_attempts, &[p1_id]);
                assert_eq!(
                    winner_attempts[0]
                        .get("selection_method")
                        .and_then(Value::as_str),
                    Some("circuit_probe")
                );
                assert_eq!(
                    winner_attempts[0]
                        .get("probe_trigger")
                        .and_then(Value::as_str),
                    Some("natural_max_wait")
                );
                assert_eq!(
                    winner_attempts[0]
                        .get("probe_result")
                        .and_then(Value::as_str),
                    Some("success")
                );
                assert!(winner_attempts[0]
                    .get("probe_generation")
                    .and_then(Value::as_u64)
                    .is_some());
                assert_logged_provider_order(&request_log_provider_chain(&winner_log), &[p1_id]);
                assert_eq!(
                    session.get_bound_provider("codex", &winner_session_id, now),
                    Some(p1_id)
                );
                let recovered_snapshot = circuit.snapshot(p1_id, now);
                assert_eq!(
                    recovered_snapshot.state,
                    circuit_breaker::CircuitState::Closed
                );
                assert!(recovered_snapshot.natural_probe_due_at.is_none());
                assert!(recovered_snapshot.recovery_epoch > initial_recovery_epoch);
                assert_eq!(circuit.recovery_epoch(), recovered_snapshot.recovery_epoch);
            }
            NaturalFailbackWinnerOutcome::Failure => {
                assert_eq!(
                    winner_payload.get("id").and_then(Value::as_str),
                    Some("current-p2-ok")
                );
                assert_logged_provider_order(&winner_attempts, &[p1_id, p2_id]);
                assert_eq!(
                    winner_attempts[0]
                        .get("selection_method")
                        .and_then(Value::as_str),
                    Some("circuit_probe")
                );
                assert_eq!(
                    winner_attempts[0]
                        .get("probe_trigger")
                        .and_then(Value::as_str),
                    Some("natural_max_wait")
                );
                assert_eq!(
                    winner_attempts[0]
                        .get("probe_result")
                        .and_then(Value::as_str),
                    Some("failed")
                );
                assert_eq!(
                    winner_attempts[1].get("outcome").and_then(Value::as_str),
                    Some("success")
                );
                assert_direct_attempt_without_probe_metadata(&winner_attempts[1]);
                assert_logged_provider_order(
                    &request_log_provider_chain(&winner_log),
                    &[p1_id, p2_id],
                );
                assert_eq!(
                    session.get_bound_provider("codex", &winner_session_id, now),
                    Some(p2_id)
                );
                let failed_at = crate::gateway::util::now_unix_seconds() as i64;
                let failed_snapshot = circuit.snapshot(p1_id, failed_at);
                assert_eq!(failed_snapshot.state, circuit_breaker::CircuitState::Open);
                assert_eq!(failed_snapshot.recovery_epoch, initial_recovery_epoch);
                assert_eq!(circuit.recovery_epoch(), initial_recovery_epoch);
                assert!(
                    failed_snapshot
                        .natural_probe_due_at
                        .is_some_and(|due| due > failed_at),
                    "failed winner must rearm, not publish, the recovery deadline"
                );
            }
        }

        let second_wave_tasks: Vec<_> = follower_session_ids
            .iter()
            .map(|session_id| {
                let session_id = session_id.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    let response =
                        route_ordered_failback_json_response(router, &session_id, MODEL).await;
                    (session_id, response)
                })
            })
            .collect();
        for task in second_wave_tasks {
            let (session_id, (status, payload)) = task.await.expect("second-wave follower task");
            assert_eq!(status, StatusCode::OK, "second-wave follower {session_id}");
            let expected_id = match winner_outcome {
                NaturalFailbackWinnerOutcome::Success => "recovered-p1-ok",
                NaturalFailbackWinnerOutcome::Failure => "current-p2-ok",
            };
            assert_eq!(
                payload.get("id").and_then(Value::as_str),
                Some(expected_id),
                "second-wave follower {session_id} chose the wrong Provider"
            );
        }

        let second_wave_logs =
            recv_terminal_request_logs_by_session(&mut log_rx, follower_session_ids.len()).await;
        match winner_outcome {
            NaturalFailbackWinnerOutcome::Success => {
                assert_eq!(p1_upstream.calls(), 1 + follower_session_ids.len());
                assert_eq!(
                    p2_calls.load(std::sync::atomic::Ordering::SeqCst),
                    follower_session_ids.len(),
                    "direct follower convergence must not grow current P2 calls"
                );
                for session_id in &follower_session_ids {
                    let log = second_wave_logs
                        .get(session_id)
                        .expect("second-wave follower log");
                    let attempts = request_log_attempts(log);
                    assert_logged_provider_order(&attempts, &[p1_id]);
                    assert_eq!(
                        attempts[0].get("outcome").and_then(Value::as_str),
                        Some("success")
                    );
                    assert_direct_attempt_without_probe_metadata(&attempts[0]);
                    assert_logged_provider_order(&request_log_provider_chain(log), &[p1_id]);
                    assert_eq!(
                        session.get_bound_provider("codex", session_id, now),
                        Some(p1_id)
                    );
                }
            }
            NaturalFailbackWinnerOutcome::Failure => {
                assert_eq!(
                    p1_upstream.calls(),
                    1,
                    "failed winner must not create a direct recovery fact"
                );
                assert_eq!(
                    p2_calls.load(std::sync::atomic::Ordering::SeqCst),
                    1 + follower_session_ids.len() * 2
                );
                for session_id in &follower_session_ids {
                    let log = second_wave_logs
                        .get(session_id)
                        .expect("second-wave follower log");
                    let attempts = request_log_attempts(log);
                    assert_logged_provider_order(&attempts, &[p1_id, p2_id]);
                    assert_eq!(
                        attempts[0].get("outcome").and_then(Value::as_str),
                        Some("skipped")
                    );
                    assert_eq!(attempts[0].get("provider_index"), Some(&Value::Null));
                    assert_eq!(attempts[0].get("retry_index"), Some(&Value::Null));
                    assert_eq!(
                        attempts[0].get("probe_result").and_then(Value::as_str),
                        Some("not_triggered")
                    );
                    assert_eq!(
                        attempts[1].get("outcome").and_then(Value::as_str),
                        Some("success")
                    );
                    assert_direct_attempt_without_probe_metadata(&attempts[1]);
                    assert_logged_provider_order(&request_log_provider_chain(log), &[p2_id]);
                    assert_eq!(
                        session.get_bound_provider("codex", session_id, now),
                        Some(p2_id)
                    );
                }
            }
        }

        p2_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_natural_single_flight_success_converges_dynamic_follower_sessions_directly(
    ) {
        assert_route_ordered_failback_natural_multi_session_convergence(
            NaturalFailbackWinnerOutcome::Success,
        )
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_natural_failed_winner_does_not_publish_follower_recovery() {
        assert_route_ordered_failback_natural_multi_session_convergence(
            NaturalFailbackWinnerOutcome::Failure,
        )
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_late_old_session_success_cannot_reverse_newer_convergence() {
        const MODEL: &str = "gpt-session-binding-order";
        const WINNER_SESSION: &str = "0190c0de-0000-7000-8000-000000000401";
        const TARGET_SESSION: &str = "0190c0de-0000-7000-8000-000000000402";

        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        app_settings.natural_probe_max_wait_seconds = 60;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-session-binding-order.sqlite"))
            .expect("init test db");
        let recovered_body = r#"{"id":"recovered-p1-ok","object":"response","status":"completed","model":"gpt-session-binding-order","output":[]}"#;
        let old_body = r#"{"id":"old-p2-ok","object":"response","status":"completed","model":"gpt-session-binding-order","output":[]}"#;
        let mut p1_upstream =
            spawn_gated_counting_status_upstream(StatusCode::OK, recovered_body).await;
        let mut p2_upstream = spawn_gated_counting_status_upstream(StatusCode::OK, old_body).await;
        let p1_id = insert_codex_provider_with_priority(
            &db,
            "Recovering P1",
            p1_upstream.base_url.clone(),
            0,
        );
        let p2_id =
            insert_codex_provider_with_priority(&db, "Current P2", p2_upstream.base_url.clone(), 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                natural_probe_max_wait_secs: 60,
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p1_id, now.saturating_sub(61), None);
        let initial_recovery_epoch = circuit.recovery_epoch();
        let latest_route = vec![p1_id, p2_id];
        let session = Arc::new(session_manager::SessionManager::new());
        for session_id in [WINNER_SESSION, TARGET_SESSION] {
            let binding_request = session
                .begin_binding_request()
                .expect("initial session binding request");
            assert!(session.bind_sort_mode_with_recovery_epoch(
                "codex",
                session_id,
                session_manager::SessionBindingCreation::new(
                    None,
                    Some(latest_route.clone()),
                    initial_recovery_epoch,
                    0,
                    binding_request,
                ),
                now,
            ));
            session.bind_success("codex", session_id, p2_id, None, now);
        }

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));

        let winner_task = {
            let router = router.clone();
            tokio::spawn(async move {
                route_ordered_failback_json_response(router, WINNER_SESSION, MODEL).await
            })
        };
        p1_upstream.wait_for_first_request().await;

        let older_target_task = {
            let router = router.clone();
            tokio::spawn(async move {
                route_ordered_failback_json_response(router, TARGET_SESSION, MODEL).await
            })
        };
        p2_upstream.wait_for_first_request().await;
        assert_eq!(p1_upstream.calls(), 1);
        assert_eq!(p2_upstream.calls(), 1);

        p1_upstream.release_first_response();
        let (winner_status, winner_payload) = winner_task.await.expect("winner request task");
        assert_eq!(winner_status, StatusCode::OK);
        assert_eq!(
            winner_payload.get("id").and_then(Value::as_str),
            Some("recovered-p1-ok")
        );
        assert!(circuit.snapshot(p1_id, now).recovery_epoch > initial_recovery_epoch);

        let (newer_status, newer_payload) =
            route_ordered_failback_json_response(router.clone(), TARGET_SESSION, MODEL).await;
        assert_eq!(newer_status, StatusCode::OK);
        assert_eq!(
            newer_payload.get("id").and_then(Value::as_str),
            Some("recovered-p1-ok")
        );
        assert_eq!(p1_upstream.calls(), 2);
        assert_eq!(
            session.get_bound_provider("codex", TARGET_SESSION, now),
            Some(p1_id)
        );

        p2_upstream.release_first_response();
        let (older_status, older_payload) =
            older_target_task.await.expect("older target request task");
        assert_eq!(older_status, StatusCode::OK);
        assert_eq!(
            older_payload.get("id").and_then(Value::as_str),
            Some("old-p2-ok")
        );
        assert_eq!(
            session.get_bound_provider("codex", TARGET_SESSION, now),
            Some(p1_id),
            "the older P2 completion must not overwrite the newer P1 binding"
        );

        let mut saw_old_p2 = false;
        let mut saw_new_direct_p1 = false;
        for _ in 0..3 {
            let log = recv_terminal_request_log(&mut log_rx).await;
            if log.session_id.as_deref() != Some(TARGET_SESSION) {
                continue;
            }
            let attempts = request_log_attempts(&log);
            let provider_ids: Vec<_> = attempts
                .iter()
                .filter_map(|attempt| attempt.get("provider_id").and_then(Value::as_i64))
                .collect();
            if provider_ids == [p1_id] {
                assert_direct_attempt_without_probe_metadata(&attempts[0]);
                saw_new_direct_p1 = true;
            } else if provider_ids == [p1_id, p2_id] {
                assert_eq!(
                    attempts[0].get("probe_result").and_then(Value::as_str),
                    Some("in_flight")
                );
                assert_direct_attempt_without_probe_metadata(&attempts[1]);
                saw_old_p2 = true;
            }
        }
        assert!(saw_old_p2, "missing the older in-flight -> P2 request log");
        assert!(
            saw_new_direct_p1,
            "missing the newer direct P1 convergence request log"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_exhausts_all_higher_targets_before_returning_to_current_provider(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 3;
        app_settings.circuit_breaker_failure_threshold = 5;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-current-fallback.sqlite"),
        )
        .expect("init test db");
        let failure_body = r#"{"error":{"message":"higher target failed"}}"#;
        let current_body = r#"{"id":"current-p3-ok","object":"response","status":"completed","model":"gpt-ordered-current","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, current_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Failed P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Failed P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Current P3", p3_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                provider_cooldown_secs: 0,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000207";
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p3_id]), now);
        session.bind_success("codex", session_id, p3_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-ordered-current"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("current-p3-ok")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let expected_order = [p1_id, p2_id, p3_id];
        assert_logged_provider_order(&request_log_attempts(&log), &expected_order);
        assert_logged_provider_order(&request_log_provider_chain(&log), &expected_order);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p3_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id])
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_ready_cap_keeps_later_probe_skip_visible_and_stops_before_current(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-ready-cap.sqlite"),
        )
        .expect("init test db");
        let failure_body = r#"{"error":{"message":"ready target failed"}}"#;
        let unused_body =
            r#"{"id":"must-not-run","object":"response","status":"completed","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p4_url, p4_calls, p4_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Ready P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Ready P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Cooling P3", p3_url, 2);
        let p4_id = insert_codex_provider_with_priority(&db, "Current P4", p4_url, 3);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p3_id, now, None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000208";
        session.bind_sort_mode("codex", session_id, None, Some(vec![p1_id, p4_id]), now);
        session.bind_success("codex", session_id, p4_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-ordered-ready-cap"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[p1_id, p2_id, p3_id]);
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[2].get("probe_trigger").and_then(Value::as_str),
            Some("route_changed")
        );
        assert_eq!(
            attempts[2].get("probe_result").and_then(Value::as_str),
            Some("cooldown")
        );
        assert_logged_provider_order(&request_log_provider_chain(&log), &[p1_id, p2_id, p3_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p4_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p4_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(vec![p1_id, p2_id, p3_id, p4_id])
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
        p4_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn route_ordered_failback_route_change_zero_target_sends_release_reservation_for_next_request(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.circuit_breaker_failure_threshold = 1;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ordered-failback-reservation-release.sqlite"),
        )
        .expect("init test db");
        let p1_body = r#"{"id":"second-request-p1-ok","object":"response","status":"completed","model":"gpt-reservation-release","output":[]}"#;
        let unused_body =
            r#"{"id":"p2-must-not-run","object":"response","status":"completed","output":[]}"#;
        let current_body = r#"{"id":"first-request-current-ok","object":"response","status":"completed","model":"gpt-reservation-release","output":[]}"#;
        let (p1_url, p1_calls, p1_task) =
            spawn_counting_status_upstream(StatusCode::OK, p1_body).await;
        let (p2_url, p2_calls, p2_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (p3_url, p3_calls, p3_task) =
            spawn_counting_status_upstream(StatusCode::OK, current_body).await;
        let p1_id = insert_codex_provider_with_priority(&db, "Retry P1", p1_url, 0);
        let p2_id = insert_codex_provider_with_priority(&db, "Cooling P2", p2_url, 1);
        let p3_id = insert_codex_provider_with_priority(&db, "Current P3", p3_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                provider_cooldown_secs: 30,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(p1_id, now, None);
        circuit.record_failure(p2_id, now, None);
        let old_route = vec![p1_id, p3_id];
        let latest_route = vec![p1_id, p2_id, p3_id];
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000209";
        session.bind_sort_mode("codex", session_id, None, Some(old_route.clone()), now);
        session.bind_success("codex", session_id, p3_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let first_response = router
            .clone()
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-reservation-release",
            ))
            .await
            .expect("first route response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_body = to_bytes(first_response.into_body(), usize::MAX)
            .await
            .expect("first response body");
        let first_payload: Value = serde_json::from_slice(&first_body).expect("first json body");
        assert_eq!(
            first_payload.get("id").and_then(Value::as_str),
            Some("first-request-current-ok")
        );
        let first_log = recv_terminal_request_log(&mut log_rx).await;
        let first_attempts = request_log_attempts(&first_log);
        assert_logged_provider_order(&first_attempts, &[p1_id, p2_id, p3_id]);
        assert_eq!(
            first_attempts[0]
                .get("probe_result")
                .and_then(Value::as_str),
            Some("cooldown")
        );
        assert_eq!(
            first_attempts[1]
                .get("probe_result")
                .and_then(Value::as_str),
            Some("cooldown")
        );
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(old_route)
        );

        circuit.reset(p1_id, now);
        let second_response = router
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-reservation-release",
            ))
            .await
            .expect("second route response");
        assert_eq!(second_response.status(), StatusCode::OK);
        let second_body = to_bytes(second_response.into_body(), usize::MAX)
            .await
            .expect("second response body");
        let second_payload: Value = serde_json::from_slice(&second_body).expect("second json body");
        assert_eq!(
            second_payload.get("id").and_then(Value::as_str),
            Some("second-request-p1-ok")
        );
        let second_log = recv_terminal_request_log(&mut log_rx).await;
        assert_logged_provider_order(&request_log_attempts(&second_log), &[p1_id]);
        assert_logged_provider_order(&request_log_provider_chain(&second_log), &[p1_id]);
        assert_eq!(p1_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(p2_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(p3_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(p1_id)
        );
        assert_eq!(
            session.get_bound_provider_order("codex", session_id, now),
            Some(latest_route)
        );

        p1_task.abort();
        p2_task.abort();
        p3_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_open_persisted_first_binding_probes_first_provider_as_new_unbound_session() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-all-open-probe.sqlite"))
            .expect("init test db");
        let success_body = r#"{"id":"probe-ok","object":"response","status":"completed","model":"gpt-all-open-probe","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Open", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Open", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let eligible_opened_at = now.saturating_sub(31);
        circuit.record_failure(first_id, eligible_opened_at, None);
        circuit.record_failure(second_id, eligible_opened_at, None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000101";
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id]),
            now,
        );
        session.bind_success("codex", session_id, first_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-all-open-probe"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[0].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[0].get("probe_trigger").and_then(Value::as_str),
            Some("new_unbound_session")
        );
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            circuit.snapshot(first_id, now).state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(
            circuit.snapshot(second_id, now).state,
            circuit_breaker::CircuitState::Open
        );
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(first_id)
        );

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn balance_blocked_prefix_still_probes_final_open_provider_in_same_request() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let account_usage_runtime =
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState::default();
        app.manage(account_usage_runtime.clone());

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-balance-prefix-final-open-probe.sqlite"),
        )
        .expect("init test db");
        let blocked_body = r#"{"error":{"message":"balance-blocked provider must not run"}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, blocked_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, blocked_body).await;
        let success_body = r#"{"id":"final-probe-ok","object":"response","status":"completed","model":"gpt-balance-prefix-final-probe","output":[]}"#;
        let (third_url, third_calls, third_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_provider_with_priority_and_extensions(
            &db,
            "codex",
            "First Balance Blocked",
            first_url,
            0,
            Some(account_usage_route_extension()),
        );
        let second_id = insert_provider_with_priority_and_extensions(
            &db,
            "codex",
            "Second Balance Blocked",
            second_url,
            1,
            Some(account_usage_route_extension()),
        );
        let third_id = insert_codex_provider_with_priority(&db, "Final Circuit Open", third_url, 2);

        let now = crate::gateway::util::now_unix_seconds() as i64;
        for provider_id in [first_id, second_id] {
            let context = {
                let connection = db.open_connection().expect("open provider db");
                providers::get_account_usage_fetch_context(&connection, provider_id)
                    .expect("load account usage context")
            };
            let target = crate::app::provider_account_usage_runtime::
                ProviderAccountUsageTarget::from_gateway_fetch_context(provider_id, &context)
                .expect("route-gated target");
            account_usage_runtime.seed_gateway_route_snapshot_for_tests(
                &target,
                account_usage_route_result(
                    crate::domain::provider_account_usage::ProviderAccountUsageStatus::ZeroBalance,
                    Some(0.0),
                    now,
                ),
                std::time::Instant::now(),
                now,
            );
        }

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        circuit.record_failure(third_id, now.saturating_sub(31), None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000109";
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id, third_id]),
            now,
        );
        session.bind_success("codex", session_id, third_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-balance-prefix-final-probe",
            ))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts = request_log_attempts(&log);
        assert_logged_provider_order(&attempts, &[first_id, second_id, third_id]);
        assert_eq!(attempts.len(), 3);
        for attempt in &attempts[..2] {
            assert_eq!(
                attempt.get("outcome").and_then(Value::as_str),
                Some("skipped")
            );
            assert_eq!(
                attempt.get("reason_code").and_then(Value::as_str),
                Some("account_usage_zero_balance")
            );
            assert_eq!(attempt.get("probe").and_then(Value::as_bool), None);
            assert_eq!(attempt.get("probe_trigger").and_then(Value::as_str), None);
        }
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(
            attempts[2].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[2].get("probe_trigger").and_then(Value::as_str),
            Some("new_unbound_session")
        );
        assert_eq!(
            attempts[2].get("probe_result").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(third_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            circuit.snapshot(third_id, now).state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(third_id)
        );

        first_task.abort();
        second_task.abort();
        third_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn natural_max_wait_directly_returns_to_closed_higher_priority_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.natural_probe_max_wait_seconds = 60;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-natural-closed-direct.sqlite"),
        )
        .expect("init test db");
        let success_body = r#"{"id":"natural-closed-ok","object":"response","status":"completed","model":"gpt-natural-closed","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Healthy", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Stable Fallback", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                natural_probe_max_wait_secs: 60,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        // One counted failure arms the natural deadline but does not open P1.
        circuit.record_failure(first_id, now.saturating_sub(61), None);
        let pending = circuit.snapshot(first_id, now);
        assert_eq!(pending.state, circuit_breaker::CircuitState::Closed);
        assert!(pending.natural_probe_due_at.is_some_and(|due| due <= now));

        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000106";
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id]),
            now,
        );
        session.bind_success("codex", session_id, second_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(session_id, "gpt-natural-closed"))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(first_id)
        );
        let after = circuit.snapshot(first_id, now);
        assert_eq!(after.state, circuit_breaker::CircuitState::Closed);
        assert!(after.natural_probe_due_at.is_none());

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn natural_max_wait_direct_failback_failure_rearms_deadline_before_fallback() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.natural_probe_max_wait_seconds = 60;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-natural-closed-failure.sqlite"),
        )
        .expect("init test db");
        let failed_body = r#"{"error":{"message":"natural failback failed"}}"#;
        let success_body = r#"{"id":"natural-fallback-ok","object":"response","status":"completed","model":"gpt-natural-closed-failure","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failed_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Unstable", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Stable Fallback", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                natural_probe_max_wait_secs: 60,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(first_id, now.saturating_sub(61), None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000107";
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id]),
            now,
        );
        session.bind_success("codex", session_id, second_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-natural-closed-failure",
            ))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(second_id)
        );
        let after = circuit.snapshot(first_id, now);
        assert_eq!(after.state, circuit_breaker::CircuitState::Closed);
        assert!(after
            .natural_probe_due_at
            .is_some_and(|due| due >= now + 59));

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_open_failed_probe_advances_to_second_open_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-all-open-probe-failure.sqlite"),
        )
        .expect("init test db");
        let failed_body = r#"{"error":{"message":"probe failed"}}"#;
        let success_body = r#"{"id":"second-probe-ok","object":"response","status":"completed","model":"gpt-all-open-probe-failure","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failed_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Open", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Open", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let eligible_opened_at = now.saturating_sub(31);
        circuit.record_failure(first_id, eligible_opened_at, None);
        circuit.record_failure(second_id, eligible_opened_at, None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000104";
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id]),
            now,
        );
        session.bind_success("codex", session_id, first_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(
                session_id,
                "gpt-all-open-probe-failure",
            ))
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[0].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[0].get("probe_trigger").and_then(Value::as_str),
            Some("new_unbound_session")
        );
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            Some("failed")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(
            attempts[1].get("probe").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            attempts[1].get("selection_method").and_then(Value::as_str),
            Some("circuit_probe")
        );
        assert_eq!(
            attempts[1].get("probe_trigger").and_then(Value::as_str),
            Some("new_unbound_session")
        );
        assert_eq!(
            attempts[1].get("probe_result").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(second_id)
        );
        assert_eq!(
            circuit.snapshot(first_id, now).state,
            circuit_breaker::CircuitState::Open
        );
        assert_eq!(
            circuit.snapshot(second_id, now).state,
            circuit_breaker::CircuitState::Closed
        );

        first_task.abort();
        second_task.abort();
    }

    #[derive(Clone, Copy)]
    enum AllOpenProbeGateBlock {
        FirstCooldown,
        FirstInFlight,
        AllCooldown,
    }

    async fn assert_all_open_probe_gate_behavior(block: AllOpenProbeGateBlock) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let (label, expected_first_probe_result, expect_recovery) = match block {
            AllOpenProbeGateBlock::FirstCooldown => ("first-cooldown", "cooldown", true),
            AllOpenProbeGateBlock::FirstInFlight => ("first-in-flight", "in_flight", true),
            AllOpenProbeGateBlock::AllCooldown => ("all-cooldown", "cooldown", false),
        };
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join(format!("gateway-route-all-open-probe-{label}.sqlite")),
        )
        .expect("init test db");
        let unused_body = r#"{"id":"must-not-run","object":"response","output":[]}"#;
        let success_body = r#"{"id":"second-probe-ok","object":"response","status":"completed","model":"gpt-all-open-gate","output":[]}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::OK, unused_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Open", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Open", second_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        let first_opened_at = match block {
            AllOpenProbeGateBlock::FirstCooldown | AllOpenProbeGateBlock::AllCooldown => now,
            AllOpenProbeGateBlock::FirstInFlight => now.saturating_sub(31),
        };
        let second_opened_at = match block {
            AllOpenProbeGateBlock::AllCooldown => now,
            AllOpenProbeGateBlock::FirstCooldown | AllOpenProbeGateBlock::FirstInFlight => {
                now.saturating_sub(31)
            }
        };
        circuit.record_failure(first_id, first_opened_at, None);
        circuit.record_failure(second_id, second_opened_at, None);
        let _existing_probe = match block {
            AllOpenProbeGateBlock::FirstCooldown | AllOpenProbeGateBlock::AllCooldown => None,
            AllOpenProbeGateBlock::FirstInFlight => {
                let token = match circuit.try_acquire_probe(
                    first_id,
                    "existing-probe",
                    circuit_breaker::ProbeTrigger::NewUnboundSession,
                    now,
                ) {
                    circuit_breaker::ProbeAcquireResult::Acquired { token, .. } => token,
                    other => panic!("expected existing probe lease, got {other:?}"),
                };
                Some(circuit_breaker::ProbeLeaseGuard::new(
                    circuit.clone(),
                    token,
                ))
            }
        };
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = match block {
            AllOpenProbeGateBlock::FirstCooldown => "0190c0de-0000-7000-8000-000000000102",
            AllOpenProbeGateBlock::FirstInFlight => "0190c0de-0000-7000-8000-000000000103",
            AllOpenProbeGateBlock::AllCooldown => "0190c0de-0000-7000-8000-000000000105",
        };
        session.bind_sort_mode(
            "codex",
            session_id,
            None,
            Some(vec![first_id, second_id]),
            now,
        );
        session.bind_success("codex", session_id, first_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let response = router
            .oneshot(all_open_probe_request(
                session_id,
                &format!("gpt-all-open-{label}"),
            ))
            .await
            .expect("route response");
        if expect_recovery {
            assert_eq!(response.status(), StatusCode::OK);
        } else {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body");
            let payload: Value = serde_json::from_slice(&body).expect("json body");
            assert_eq!(
                payload.get("error_code").and_then(Value::as_str),
                Some(crate::gateway::proxy::GatewayErrorCode::AllProvidersUnavailable.as_str())
            );
        }

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[0].get("probe_trigger").and_then(Value::as_str),
            Some("new_unbound_session")
        );
        assert_eq!(
            attempts[0].get("probe_result").and_then(Value::as_str),
            Some(expected_first_probe_result)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        if expect_recovery {
            assert_eq!(
                attempts[1].get("probe_trigger").and_then(Value::as_str),
                Some("new_unbound_session")
            );
            assert_eq!(
                attempts[1].get("probe_result").and_then(Value::as_str),
                Some("success")
            );
            assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(
                circuit.snapshot(second_id, now).state,
                circuit_breaker::CircuitState::Closed
            );
            assert_eq!(
                session.get_bound_provider("codex", session_id, now),
                Some(second_id)
            );
        } else {
            assert_eq!(
                attempts[1].get("probe_result").and_then(Value::as_str),
                Some("cooldown")
            );
            assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert_eq!(
                circuit.snapshot(second_id, now).state,
                circuit_breaker::CircuitState::Open
            );
        }

        first_task.abort();
        second_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_open_first_probe_cooldown_advances_to_second_provider() {
        assert_all_open_probe_gate_behavior(AllOpenProbeGateBlock::FirstCooldown).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_open_first_probe_in_flight_advances_to_second_provider() {
        assert_all_open_probe_gate_behavior(AllOpenProbeGateBlock::FirstInFlight).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn all_open_all_probe_cooldowns_return_unavailable_without_network() {
        assert_all_open_probe_gate_behavior(AllOpenProbeGateBlock::AllCooldown).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decision_a_records_all_session_bound_gate_skips_and_final_503_diagnostics() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-all-gate-skips.sqlite"))
            .expect("init test db");
        let unavailable_body = r#"{"error":{"message":"must not be called"}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, unavailable_body)
                .await;
        let (bound_url, bound_calls, bound_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, unavailable_body)
                .await;
        let (third_url, third_calls, third_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, unavailable_body)
                .await;
        let first_id = insert_codex_provider_with_priority(&db, "First Open", first_url, 0);
        let bound_id = insert_codex_provider_with_priority(&db, "Bound Open", bound_url, 1);
        let third_id = insert_codex_provider_with_priority(&db, "Third Open", third_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        for provider_id in [first_id, bound_id, third_id] {
            circuit.record_failure(provider_id, now, None);
        }
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000001";
        session.bind_success("codex", session_id, bound_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-all-gate-skips","messages":[{"role":"user","content":"hello"},{"role":"assistant","content":"hi"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::AllProvidersUnavailable.as_str())
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 3);
        assert!(attempts
            .iter()
            .all(|attempt| attempt.get("outcome").and_then(Value::as_str) == Some("skipped")));
        let mut attempted_provider_ids: Vec<i64> = attempts
            .iter()
            .filter_map(|attempt| attempt.get("provider_id").and_then(Value::as_i64))
            .collect();
        attempted_provider_ids.sort_unstable();
        let mut expected_provider_ids = vec![first_id, bound_id, third_id];
        expected_provider_ids.sort_unstable();
        assert_eq!(attempted_provider_ids, expected_provider_ids);

        let provider_chain: Value =
            serde_json::from_str(log.provider_chain_json.as_deref().expect("provider chain"))
                .expect("provider chain json");
        let chain = provider_chain.as_array().expect("provider chain array");
        assert_eq!(chain.len(), 3);
        assert!(chain
            .iter()
            .all(|hop| hop.get("outcome").and_then(Value::as_str) == Some("skipped")));
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(bound_id)
        );
        for call_count in [&first_calls, &bound_calls, &third_calls] {
            assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        }

        first_task.abort();
        bound_task.abort();
        third_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decision_a_session_bound_gate_skip_continues_without_consuming_ready_cap() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-skips-ready-cap.sqlite"))
            .expect("init test db");
        let unavailable_body = r#"{"error":{"message":"must not be called"}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, unavailable_body)
                .await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, unavailable_body)
                .await;
        let success_body = r#"{"id":"third-ok","object":"chat.completion","choices":[]}"#;
        let (third_url, third_calls, third_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Open", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Open", second_url, 1);
        let third_id = insert_codex_provider_with_priority(&db, "Third Ready", third_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(first_id, now, None);
        circuit.record_failure(second_id, now, None);
        let session = Arc::new(session_manager::SessionManager::new());
        let session_id = "0190c0de-0000-7000-8000-000000000002";
        session.bind_success("codex", session_id, second_id, None, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            session.clone(),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("session_id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-skips-ready-cap","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 3);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[2].get("provider_id").and_then(Value::as_i64),
            Some(third_id)
        );
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(third_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Gate denial itself does not clear the binding; the later successful
        // fallback legitimately advances it to the provider that served the session.
        assert_eq!(
            session.get_bound_provider("codex", session_id, now),
            Some(third_id)
        );

        first_task.abort();
        second_task.abort();
        third_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn account_usage_gate_precedes_probe_and_ready_budget_with_zero_upstream_calls() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let account_usage_runtime =
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState::default();
        app.manage(account_usage_runtime.clone());

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-account-usage-gate.sqlite"))
            .expect("init test db");
        let (blocked_url, blocked_calls, blocked_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"must-not-run","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let (ready_url, ready_calls, ready_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"ready-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let blocked_id = insert_provider_with_priority_and_extensions(
            &db,
            "codex",
            "Balance Blocked",
            blocked_url,
            0,
            Some(account_usage_route_extension()),
        );
        let ready_id = insert_codex_provider_with_priority(&db, "Ready Fallback", ready_url, 1);
        let managed_model =
            insert_managed_codex_model(&db, blocked_id, "gpt-account-managed-upstream");

        let context = {
            let connection = db.open_connection().expect("open provider db");
            providers::get_account_usage_fetch_context(&connection, blocked_id)
                .expect("load account usage context")
        };
        let target = crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget::
            from_gateway_fetch_context(blocked_id, &context)
            .expect("route-gated target");
        let now = crate::gateway::util::now_unix_seconds() as i64;
        account_usage_runtime.seed_gateway_route_snapshot_for_tests(
            &target,
            account_usage_route_result(
                crate::domain::provider_account_usage::ProviderAccountUsageStatus::ZeroBalance,
                Some(0.0),
                now,
            ),
            std::time::Instant::now(),
            now,
        );

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        circuit.record_failure(blocked_id, now, None);
        let circuit_before = circuit.snapshot(blocked_id, now);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(12);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            Arc::new(session_manager::SessionManager::new()),
        ));

        let forced_stream_request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{blocked_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-account-forced","stream":true,"input":"hello"}"#,
            ))
            .expect("forced stream request");
        let forced_stream_response = router
            .clone()
            .oneshot(forced_stream_request)
            .await
            .expect("forced stream response");
        assert_eq!(
            forced_stream_response.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let forced_stream_log = recv_terminal_request_log(&mut log_rx).await;
        let forced_stream_attempts: Value =
            serde_json::from_str(&forced_stream_log.attempts_json).expect("forced attempts json");
        assert_eq!(
            forced_stream_attempts
                .as_array()
                .and_then(|attempts| attempts.first())
                .and_then(|attempt| attempt.get("reason_code"))
                .and_then(Value::as_str),
            Some("account_usage_zero_balance")
        );

        let managed_request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": managed_model,
                    "stream": false,
                    "input": "hello"
                })
                .to_string(),
            ))
            .expect("managed request");
        let managed_response = router
            .clone()
            .oneshot(managed_request)
            .await
            .expect("managed response");
        assert_eq!(managed_response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let managed_log = recv_terminal_request_log(&mut log_rx).await;
        let managed_attempts: Value =
            serde_json::from_str(&managed_log.attempts_json).expect("managed attempts json");
        assert_eq!(managed_attempts.as_array().map(Vec::len), Some(1));
        assert_eq!(
            managed_attempts
                .as_array()
                .and_then(|attempts| attempts.first())
                .and_then(|attempt| attempt.get("reason_code"))
                .and_then(Value::as_str),
            Some("account_usage_zero_balance")
        );

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-account-gate","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router
            .clone()
            .oneshot(request)
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        let blocked = &attempts[0];
        assert_eq!(
            blocked.get("provider_id").and_then(Value::as_i64),
            Some(blocked_id)
        );
        assert_eq!(
            blocked.get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            blocked.get("decision").and_then(Value::as_str),
            Some("skip")
        );
        assert_eq!(
            blocked.get("selection_method").and_then(Value::as_str),
            Some("filtered")
        );
        assert_eq!(
            blocked.get("error_category").and_then(Value::as_str),
            Some("account_usage")
        );
        assert_eq!(
            blocked.get("error_code").and_then(Value::as_str),
            Some("GW_PROVIDER_ACCOUNT_USAGE_BLOCKED")
        );
        assert_eq!(
            blocked.get("reason_code").and_then(Value::as_str),
            Some("account_usage_zero_balance")
        );
        for field in [
            "provider_index",
            "retry_index",
            "circuit_state_before",
            "circuit_state_after",
            "circuit_failure_count",
            "circuit_failure_threshold",
            "probe",
            "probe_trigger",
            "probe_result",
            "probe_generation",
        ] {
            assert!(blocked.get(field).is_none_or(Value::is_null), "{field}");
        }
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(ready_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(blocked_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(ready_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let circuit_after = circuit.snapshot(blocked_id, now);
        assert_eq!(circuit_after.state, circuit_before.state);
        assert_eq!(circuit_after.state_revision, circuit_before.state_revision);
        assert!(!circuit_after.probe_in_flight);

        let discovery_request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?client_version=0.144.2")
            .body(Body::empty())
            .expect("model discovery request");
        let discovery_response = router
            .oneshot(discovery_request)
            .await
            .expect("model discovery response");
        assert_eq!(discovery_response.status(), StatusCode::OK);
        assert_eq!(blocked_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            ready_calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "account gate must not consume the strict model-discovery send budget"
        );

        blocked_task.abort();
        ready_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn account_usage_all_unavailable_retry_after_never_caches_a_mixed_503() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let account_usage_runtime =
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState::default();
        app.manage(account_usage_runtime.clone());

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-account-usage-unavailable-cache.sqlite"),
        )
        .expect("init test db");
        let (blocked_url, blocked_calls, blocked_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"balance-recovered","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let blocked_id = insert_provider_with_priority_and_extensions(
            &db,
            "codex",
            "Balance Blocked",
            blocked_url,
            0,
            Some(account_usage_route_extension()),
        );
        let context = {
            let connection = db.open_connection().expect("open provider db");
            providers::get_account_usage_fetch_context(&connection, blocked_id)
                .expect("load account usage context")
        };
        let target = crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget::
            from_gateway_fetch_context(blocked_id, &context)
            .expect("route-gated target");
        let now = crate::gateway::util::now_unix_seconds() as i64;
        account_usage_runtime.seed_gateway_route_snapshot_for_tests(
            &target,
            account_usage_route_result(
                crate::domain::provider_account_usage::ProviderAccountUsageStatus::ZeroBalance,
                Some(0.0),
                now,
            ),
            std::time::Instant::now(),
            now,
        );

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(12);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            circuit.clone(),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = || {
            Request::builder()
                .method(Method::POST)
                .uri("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"model":"gpt-account-unavailable","messages":[{"role":"user","content":"hello"}]}"#,
                ))
                .expect("request")
        };

        let pure_response = router
            .clone()
            .oneshot(request())
            .await
            .expect("pure account gate response");
        let pure_status = pure_response.status();
        let pure_retry_after = pure_response.headers().get(header::RETRY_AFTER).cloned();
        let pure_body = to_bytes(pure_response.into_body(), usize::MAX)
            .await
            .expect("pure response body");
        let pure_payload: Value = serde_json::from_slice(&pure_body).expect("pure response json");
        assert_eq!(
            pure_status,
            StatusCode::SERVICE_UNAVAILABLE,
            "payload={pure_payload}, upstream_calls={}",
            blocked_calls.load(std::sync::atomic::Ordering::SeqCst),
        );
        assert!(pure_retry_after.is_none());
        assert_eq!(
            pure_payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::AllProvidersUnavailable.as_str())
        );
        let pure_log = recv_terminal_request_log(&mut log_rx).await;
        let pure_attempts: Value =
            serde_json::from_str(&pure_log.attempts_json).expect("pure attempts json");
        assert_eq!(pure_attempts.as_array().map(Vec::len), Some(1));

        let (open_url, open_calls, open_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"must-not-run","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let open_id = insert_codex_provider_with_priority(&db, "Circuit Open", open_url, 1);
        circuit.record_failure(open_id, now, None);

        let mixed_response = router
            .clone()
            .oneshot(request())
            .await
            .expect("mixed gate response");
        assert_eq!(mixed_response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(mixed_response.headers().get(header::RETRY_AFTER).is_some());
        let mixed_payload: Value = serde_json::from_slice(
            &to_bytes(mixed_response.into_body(), usize::MAX)
                .await
                .expect("mixed response body"),
        )
        .expect("mixed response json");
        assert_eq!(
            mixed_payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::AllProvidersUnavailable.as_str())
        );
        let mixed_log = recv_terminal_request_log(&mut log_rx).await;
        let mixed_attempts: Value =
            serde_json::from_str(&mixed_log.attempts_json).expect("mixed attempts json");
        assert_eq!(mixed_attempts.as_array().map(Vec::len), Some(2));

        account_usage_runtime.seed_gateway_route_snapshot_for_tests(
            &target,
            account_usage_route_result(
                crate::domain::provider_account_usage::ProviderAccountUsageStatus::Available,
                Some(10.0),
                now,
            ),
            std::time::Instant::now(),
            now,
        );
        let recovered_response = router
            .oneshot(request())
            .await
            .expect("recovered route response");
        assert_eq!(recovered_response.status(), StatusCode::OK);
        let recovered_log = recv_terminal_request_log(&mut log_rx).await;
        let recovered_attempts: Value =
            serde_json::from_str(&recovered_log.attempts_json).expect("recovered attempts json");
        assert_eq!(recovered_attempts.as_array().map(Vec::len), Some(1));
        assert_eq!(blocked_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(open_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        blocked_task.abort();
        open_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn account_usage_recovery_fails_back_each_live_session_from_its_own_baseline() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let account_usage_runtime =
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState::default();
        app.manage(account_usage_runtime.clone());

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-account-usage-recovery.sqlite"))
            .expect("init test db");
        let (primary_url, primary_calls, primary_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"primary-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let (fallback_url, fallback_calls, fallback_task) = spawn_counting_status_upstream(
            StatusCode::OK,
            r#"{"id":"fallback-ok","object":"chat.completion","choices":[]}"#,
        )
        .await;
        let primary_id = insert_confirmed_custom_provider_with_priority(
            &db,
            "Recovered Custom Primary",
            primary_url,
            0,
        );
        let fallback_id =
            insert_codex_provider_with_priority(&db, "Stable Fallback", fallback_url, 1);
        let context = {
            let connection = db.open_connection().expect("open provider db");
            providers::get_account_usage_fetch_context(&connection, primary_id)
                .expect("load account usage context")
        };
        let target = crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget::
            from_gateway_fetch_context(primary_id, &context)
            .expect("route-gated target");
        assert_eq!(
            target.adapter_kind,
            crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind::Custom
        );
        let now = crate::gateway::util::now_unix_seconds() as i64;
        account_usage_runtime.seed_gateway_route_snapshot_for_tests(
            &target,
            account_usage_route_result_for_adapter(
                crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind::Custom,
                crate::domain::provider_account_usage::ProviderAccountUsageStatus::ZeroBalance,
                Some(0.0),
                now,
            ),
            std::time::Instant::now(),
            now,
        );

        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(12);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig::default(),
                HashMap::new(),
                None,
            )),
            session.clone(),
        ));
        let first_session_id = "0190c0de-0000-7000-8000-000000000003";
        let second_session_id = "0190c0de-0000-7000-8000-000000000004";
        let request = |session_id: &str| {
            Request::builder()
                .method(Method::POST)
                .uri("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .header("session_id", session_id)
                .body(Body::from(
                    r#"{"model":"gpt-account-recovery","messages":[{"role":"user","content":"hello"}]}"#,
                ))
                .expect("request")
        };
        let compaction_request = |session_id: &str| {
            Request::builder()
                .method(Method::POST)
                .uri("/v1/responses")
                .header(header::CONTENT_TYPE, "application/json")
                .header("session_id", session_id)
                .body(Body::from(
                    serde_json::json!({
                        "model": "gpt-account-recovery",
                        "stream": false,
                        "input": [
                            {
                                "type": "compaction",
                                "encrypted_content": "opaque-compaction-state"
                            },
                            {
                                "type": "message",
                                "role": "user",
                                "content": [{"type": "input_text", "text": "hello"}]
                            }
                        ]
                    })
                    .to_string(),
                ))
                .expect("compaction request")
        };

        let first_response = router
            .clone()
            .oneshot(request(first_session_id))
            .await
            .expect("first route response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_log = recv_terminal_request_log(&mut log_rx).await;
        let first_attempts: Value =
            serde_json::from_str(&first_log.attempts_json).expect("first attempts json");
        let first_attempts = first_attempts.as_array().expect("first attempt array");
        assert_eq!(first_attempts.len(), 2);
        assert_eq!(
            first_attempts[0].get("reason_code").and_then(Value::as_str),
            Some("account_usage_zero_balance")
        );
        assert_eq!(
            first_attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(fallback_id)
        );
        assert_eq!(
            session.get_bound_provider("codex", first_session_id, now),
            Some(fallback_id)
        );

        for request_index in 0..2 {
            let repeated_response = router
                .clone()
                .oneshot(compaction_request(first_session_id))
                .await
                .expect("repeated blocked compaction response");
            assert_eq!(repeated_response.status(), StatusCode::OK);
            let repeated_log = recv_terminal_request_log(&mut log_rx).await;
            let repeated_attempts: Value = serde_json::from_str(&repeated_log.attempts_json)
                .expect("repeated blocked compaction attempts json");
            let repeated_attempts = repeated_attempts
                .as_array()
                .expect("repeated blocked compaction attempt array");
            assert_eq!(
                repeated_attempts.len(),
                1,
                "blocked failback target must stay out of steady request {request_index}"
            );
            assert_eq!(
                repeated_attempts[0]
                    .get("provider_id")
                    .and_then(Value::as_i64),
                Some(fallback_id)
            );
            assert_eq!(
                repeated_attempts[0].get("outcome").and_then(Value::as_str),
                Some("success")
            );
        }
        assert_eq!(
            session
                .routing_snapshot("codex", first_session_id, now)
                .expect("first session snapshot")
                .last_codex_compaction_fingerprint,
            None,
            "suppressed balance target must leave compaction pending"
        );

        let other_first_response = router
            .clone()
            .oneshot(request(second_session_id))
            .await
            .expect("other session first route response");
        assert_eq!(other_first_response.status(), StatusCode::OK);
        let other_first_log = recv_terminal_request_log(&mut log_rx).await;
        let other_first_attempts: Value = serde_json::from_str(&other_first_log.attempts_json)
            .expect("other first attempts json");
        let other_first_attempts = other_first_attempts
            .as_array()
            .expect("other first attempt array");
        assert_eq!(
            other_first_attempts[0]
                .get("reason_code")
                .and_then(Value::as_str),
            Some("account_usage_zero_balance")
        );
        assert_eq!(
            session.get_bound_provider("codex", second_session_id, now),
            Some(fallback_id)
        );
        assert_eq!(primary_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(fallback_calls.load(std::sync::atomic::Ordering::SeqCst), 4);

        account_usage_runtime.seed_gateway_route_snapshot_for_tests(
            &target,
            account_usage_route_result_for_adapter(
                crate::domain::provider_account_usage::ProviderAccountUsageAdapterKind::Custom,
                crate::domain::provider_account_usage::ProviderAccountUsageStatus::Available,
                Some(10.0),
                now,
            ),
            std::time::Instant::now(),
            now,
        );
        assert_eq!(account_usage_runtime.global_recovery_epoch(), 1);

        let second_response = router
            .clone()
            .oneshot(compaction_request(first_session_id))
            .await
            .expect("second route response");
        assert_eq!(second_response.status(), StatusCode::OK);
        let second_log = recv_terminal_request_log(&mut log_rx).await;
        let second_attempts: Value =
            serde_json::from_str(&second_log.attempts_json).expect("second attempts json");
        let second_attempts = second_attempts.as_array().expect("second attempt array");
        assert_eq!(second_attempts.len(), 1);
        assert_eq!(
            second_attempts[0]
                .get("provider_id")
                .and_then(Value::as_i64),
            Some(primary_id)
        );
        assert_eq!(
            second_attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert_eq!(
            session.get_bound_provider("codex", first_session_id, now),
            Some(primary_id)
        );
        assert!(
            session
                .routing_snapshot("codex", first_session_id, now)
                .expect("recovered first session snapshot")
                .last_codex_compaction_fingerprint
                .is_some(),
            "real recovered dispatch must consume the pending compaction fingerprint"
        );
        assert_eq!(
            primary_calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "first custom recovery performs one base-URL probe and one model request"
        );
        assert_eq!(fallback_calls.load(std::sync::atomic::Ordering::SeqCst), 4);

        let other_second_response = router
            .oneshot(request(second_session_id))
            .await
            .expect("other session second route response");
        assert_eq!(other_second_response.status(), StatusCode::OK);
        let other_second_log = recv_terminal_request_log(&mut log_rx).await;
        let other_second_attempts: Value = serde_json::from_str(&other_second_log.attempts_json)
            .expect("other second attempts json");
        let other_second_attempts = other_second_attempts
            .as_array()
            .expect("other second attempt array");
        assert_eq!(other_second_attempts.len(), 1);
        assert_eq!(
            other_second_attempts[0]
                .get("provider_id")
                .and_then(Value::as_i64),
            Some(primary_id)
        );
        assert_eq!(
            session.get_bound_provider("codex", second_session_id, now),
            Some(primary_id)
        );
        assert_eq!(
            primary_calls.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "the second session reuses the selected base URL and sends one model request"
        );
        assert_eq!(fallback_calls.load(std::sync::atomic::Ordering::SeqCst), 4);

        primary_task.abort();
        fallback_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decision_a_ready_cap_still_records_later_circuit_gate_skip() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ready-cap-gate-skip.sqlite"),
        )
        .expect("init test db");
        let failed_body = r#"{"error":{"message":"ready provider failed"}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failed_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failed_body).await;
        let (third_url, third_calls, third_task) =
            spawn_counting_status_upstream(StatusCode::OK, r#"{"id":"must-not-run"}"#).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Ready", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Ready", second_url, 1);
        let third_id = insert_codex_provider_with_priority(&db, "Third Circuit Open", third_url, 2);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                open_duration_secs: 3_600,
                ..Default::default()
            },
            HashMap::new(),
            None,
        ));
        let now = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(third_id, now, None);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit,
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-ready-cap-gate-skip","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 3);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(
            attempts[2].get("provider_id").and_then(Value::as_i64),
            Some(third_id)
        );
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            attempts[2].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::ProviderCircuitOpen.as_str())
        );
        let provider_chain: Value =
            serde_json::from_str(log.provider_chain_json.as_deref().expect("provider chain"))
                .expect("provider chain json");
        let chain = provider_chain.as_array().expect("provider chain array");
        assert_eq!(chain.len(), 3);
        assert_eq!(
            chain[2].get("outcome").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(third_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        first_task.abort();
        second_task.abort();
        third_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_error_response_rules = vec![test_upstream_error_response_rule(
            500,
            settings::UpstreamErrorStatusBehavior::Override { status_code: 503 },
            settings::UpstreamErrorMessageBehavior::Override {
                message: "must not survive a later different failure".to_string(),
            },
        )];
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-ready-cap-boundary.sqlite"),
        )
        .expect("init test db");
        let failure_body = r#"{"error":{"message":"upstream failure"}}"#;
        let (first_url, first_calls, first_task) =
            spawn_counting_status_upstream(StatusCode::INTERNAL_SERVER_ERROR, failure_body).await;
        let (second_url, second_calls, second_task) =
            spawn_counting_status_upstream(StatusCode::BAD_GATEWAY, failure_body).await;
        let success_body = r#"{"id":"must-not-run","object":"chat.completion","choices":[]}"#;
        let (third_url, third_calls, third_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let first_id = insert_codex_provider_with_priority(&db, "First Ready", first_url, 0);
        let second_id = insert_codex_provider_with_priority(&db, "Second Ready", second_url, 1);
        insert_codex_provider_with_priority(&db, "Third Ready", third_url, 2);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-ready-cap-boundary","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert!(!has_upstream_error_response_rule_marker(&log));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(first_id)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(second_id)
        );
        assert_eq!(first_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(third_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        first_task.abort();
        second_task.abort();
        third_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_large_known_length_5xx_uses_bounded_error_preview() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-large-5xx-test.sqlite"))
            .expect("init test db");
        let diagnostic = "route-large-5xx-diagnostic-prefix";
        let tail_marker = "route-large-5xx-tail-should-not-appear";
        let mut sent_body = diagnostic.as_bytes().to_vec();
        sent_body.resize(96 * 1024, b'x');
        sent_body.extend_from_slice(tail_marker.as_bytes());
        let declared_content_length = sent_body.len() + 10 * 1024 * 1024;
        let (upstream_base_url, upstream_task) = spawn_large_known_length_error_upstream(
            "500 Internal Server Error",
            declared_content_length,
            sent_body,
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Large Error Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-large-5xx","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(2), router.oneshot(request))
            .await
            .expect("route should not wait for the full declared error body")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str())
        );

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log enqueue")
            .expect("request log item");
        assert_eq!(log.cli_key, "codex");
        assert_eq!(log.path, "/v1/chat/completions");
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str())
        );

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream5xx.as_str())
        );
        let reason = attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .expect("attempt reason");
        assert!(reason.contains(diagnostic));
        assert!(!reason.contains(tail_marker));

        let error_details: Value =
            serde_json::from_str(log.error_details_json.as_deref().expect("error details"))
                .expect("error details json");
        assert_eq!(
            error_details
                .get("upstream_body_preview")
                .and_then(Value::as_str)
                .map(|value| value.contains(diagnostic)),
            Some(true)
        );
        assert_eq!(
            error_details
                .get("upstream_body_preview")
                .and_then(Value::as_str)
                .map(|value| value.contains(tail_marker)),
            Some(false)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_large_known_length_400_rectifier_path_is_bounded() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.enable_thinking_signature_rectifier = true;
        app_settings.enable_thinking_budget_rectifier = true;
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-large-400-rectifier-test.sqlite"),
        )
        .expect("init test db");
        let diagnostic = "route-large-400-rectifier-prefix";
        let tail_marker = "route-large-400-rectifier-tail-should-not-appear";
        let mut sent_body = diagnostic.as_bytes().to_vec();
        sent_body.resize(96 * 1024, b'y');
        sent_body.extend_from_slice(tail_marker.as_bytes());
        let declared_content_length = sent_body.len() + 10 * 1024 * 1024;
        let (upstream_base_url, upstream_task) = spawn_large_known_length_error_upstream(
            "400 Bad Request",
            declared_content_length,
            sent_body,
        )
        .await;
        let provider_id =
            insert_provider_with_priority(&db, "claude", "Large 400 Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/claude/_aio/provider/{provider_id}/v1/messages"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":128,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(2), router.oneshot(request))
            .await
            .expect("rectifier path should not wait for the full declared error body")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains(diagnostic));
        assert!(!body_text.contains(tail_marker));
        assert!(body.len() < declared_content_length);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.cli_key, "claude");
        assert_eq!(log.path, "/v1/messages");
        assert_eq!(log.status, Some(400));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::Upstream4xx.as_str())
        );

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("NON_RETRYABLE_CLIENT_ERROR")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_large_known_length_cx2cc_success_transform_is_bounded() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-large-cx2cc-success-test.sqlite"),
        )
        .expect("init test db");
        let diagnostic = "route-large-cx2cc-success-prefix";
        let mut sent_body = diagnostic.as_bytes().to_vec();
        sent_body.resize(96 * 1024, b'z');
        let declared_content_length = sent_body.len() + 32 * 1024 * 1024;
        let (upstream_base_url, upstream_task) =
            spawn_large_known_length_error_upstream("200 OK", declared_content_length, sent_body)
                .await;
        let source_provider_id =
            insert_provider_with_priority(&db, "codex", "CX2CC Source Stub", upstream_base_url, 0);
        let provider_id = insert_cx2cc_bridge_provider(&db, source_provider_id, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/claude/_aio/provider/{provider_id}/v1/messages"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":128,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(2), router.oneshot(request))
            .await
            .expect("cx2cc transform path should reject the oversized body from headers")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamBodyReadError.as_str())
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.cli_key, "claude");
        assert_eq!(log.path, "/v1/messages");
        assert_eq!(log.status, Some(502));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamBodyReadError.as_str())
        );

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some(crate::gateway::proxy::GatewayErrorCode::UpstreamBodyReadError.as_str())
        );
        let reason = attempts[0]
            .get("reason")
            .and_then(Value::as_str)
            .expect("attempt reason");
        assert!(reason.contains("non-stream transform buffer limit"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_success_log_persists_after_buffered_writer_drain() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-writer-test.sqlite"))
            .expect("init test db");
        let success_body = r#"{"id":"persisted-ok","object":"chat.completion","choices":[]}"#;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Persisted Stub", success_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state(app_handle, db.clone(), log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-persisted","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("id").and_then(Value::as_str),
            Some("persisted-ok")
        );

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        assert_eq!(detail.status, Some(200));
        assert_eq!(detail.error_code, None);
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-persisted")
        );
        assert_eq!(detail.final_provider_id, provider_id);

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_internal_forwarded_codex_response_is_not_logged() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-internal-codex-not-logged-test.sqlite"),
        )
        .expect("init test db");
        let success_body = r#"{"id":"internal-ok","object":"response","model":"gpt-internal"}"#;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        insert_codex_provider_with_priority(&db, "Internal Forward Stub", success_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state(app_handle, db.clone(), log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-aio-gateway-forwarded", "aio-coding-hub")
            .body(Body::from(r#"{"model":"gpt-internal","input":"hello"}"#))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        assert!(request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .is_none());

        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_codex_models_response_is_not_logged() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-codex-models-test.sqlite"))
            .expect("init test db");
        let success_body = r#"{"object":"list","data":[{"id":"gpt-5.5","object":"model"}]}"#;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        insert_codex_provider_with_priority(&db, "Models Stub", success_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state(app_handle, db.clone(), log_tx));
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .body(Body::empty())
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        assert!(request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .is_none());

        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_codex_models_failure_is_single_attempt_and_circuit_neutral() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 5;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.circuit_breaker_failure_threshold = 5;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-codex-models-failure-test.sqlite"),
        )
        .expect("init test db");
        let (failure_base_url, call_count, upstream_task) = spawn_counting_status_upstream(
            StatusCode::BAD_GATEWAY,
            r#"{"error":"account has no Codex backend access token"}"#,
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Models Failure Stub", failure_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 5,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::clone(&circuit),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?client_version=0.144.2")
            .body(Body::empty())
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "Codex model discovery must not retry the same provider"
        );

        let circuit_snapshot =
            circuit.snapshot(provider_id, crate::gateway::util::now_unix_seconds() as i64);
        assert_eq!(
            circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(circuit_snapshot.failure_count, 0);
        assert_eq!(circuit_snapshot.cooldown_until, None);

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_codex_models_fails_over_once_without_mutating_circuits() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 5;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.circuit_breaker_failure_threshold = 2;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-codex-models-failover-test.sqlite"),
        )
        .expect("init test db");
        let (failure_base_url, failure_call_count, failure_task) = spawn_counting_status_upstream(
            StatusCode::BAD_GATEWAY,
            r#"{"error":"account has no Codex backend access token"}"#,
        )
        .await;
        let success_body = r#"{"object":"list","data":[{"id":"gpt-5.5","object":"model"}]}"#;
        let (success_base_url, success_call_count, success_task) =
            spawn_counting_status_upstream(StatusCode::OK, success_body).await;
        let failure_provider_id =
            insert_codex_provider_with_priority(&db, "Models Failure Stub", failure_base_url, 0);
        let success_provider_id =
            insert_codex_provider_with_priority(&db, "Models Success Stub", success_base_url, 1);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 2,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let seeded_at = crate::gateway::util::now_unix_seconds() as i64;
        circuit.record_failure(failure_provider_id, seeded_at, None);
        circuit.record_failure(success_provider_id, seeded_at, None);

        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            Arc::clone(&circuit),
            Arc::new(session_manager::SessionManager::new()),
        ));
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?client_version=0.144.2")
            .body(Body::empty())
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        assert_eq!(
            failure_call_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            success_call_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let checked_at = crate::gateway::util::now_unix_seconds() as i64;
        for provider_id in [failure_provider_id, success_provider_id] {
            let snapshot = circuit.snapshot(provider_id, checked_at);
            assert_eq!(snapshot.state, circuit_breaker::CircuitState::Closed);
            assert_eq!(snapshot.failure_count, 1);
            assert_eq!(snapshot.cooldown_until, None);
        }

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");
        assert!(request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .is_none());

        failure_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_sse_stream_persists_success_after_body_consumed() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-sse-test.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "data: {\"id\":\"chatcmpl-sse\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id = insert_codex_provider_with_priority(&db, "SSE Stub", sse_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state(app_handle, db.clone(), log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-sse","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(body_text.contains("data: [DONE]"));

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        assert_eq!(detail.status, Some(200));
        assert_eq!(detail.error_code, None);
        assert_eq!(detail.requested_model.as_deref(), Some("gpt-route-sse"));
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_some());

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_sse_stream_client_abort_persists_499_log() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-sse-abort-test.sqlite"))
            .expect("init test db");
        let first_chunk = "data: {\"id\":\"chatcmpl-abort\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n";
        let (sse_base_url, sse_task) = spawn_stalling_sse_upstream(first_chunk).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "SSE Abort Stub", sse_base_url, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-route-sse-abort";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-route-sse-abort","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();

        let mut body = Box::pin(response.into_body());
        let first_frame = tokio::time::timeout(
            Duration::from_secs(2),
            std::future::poll_fn(|cx| body.as_mut().poll_frame(cx)),
        )
        .await
        .expect("first stream frame timeout")
        .expect("first stream frame")
        .expect("first stream frame ok");
        let first_chunk = first_frame.into_data().expect("data frame");
        assert!(String::from_utf8_lossy(&first_chunk).contains("hello"));
        drop(body);

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        let logged_session_id = detail
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .expect("logged session id");
        assert_eq!(detail.status, Some(499));
        assert_eq!(detail.error_code.as_deref(), Some("GW_STREAM_ABORTED"));
        assert!(detail.excluded_from_stats);
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-sse-abort")
        );
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_some());

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("stream_error: code=GW_STREAM_ABORTED")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_STREAM_ABORTED")
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("CLIENT_ABORT")
        );

        let special_settings: Value = serde_json::from_str(
            detail
                .special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let special_settings = special_settings.as_array().expect("special settings array");
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("client_abort")
                && entry.get("scope").and_then(Value::as_str) == Some("stream")
        }));

        let error_details: Value = serde_json::from_str(
            detail
                .error_details_json
                .as_deref()
                .expect("error details json"),
        )
        .expect("error details json parses");
        assert_eq!(
            error_details
                .get("gateway_error_code")
                .and_then(Value::as_str),
            Some("GW_STREAM_ABORTED")
        );
        assert_eq!(
            error_details.get("error_category").and_then(Value::as_str),
            Some("CLIENT_ABORT")
        );
        let circuit_snapshot = circuit.snapshot(provider_id, 0);
        assert_eq!(
            circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(circuit_snapshot.failure_count, 0);
        assert_eq!(
            session.get_bound_provider("codex", logged_session_id, 0),
            None
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_grok_responses_abort_does_not_drain_completion() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        settings::write(&app_handle, &settings::AppSettings::default()).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "grok", true, "http://127.0.0.1:37123")
            .expect("enable Grok CLI proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-grok-responses-abort-test.sqlite"),
        )
        .expect("init test db");
        let first_chunk = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
        );
        let completion_chunk = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-grok-abort\",\"status\":\"completed\",\"model\":\"grok-abort-model\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_delayed_chunked_sse_upstream(
            first_chunk,
            completion_chunk,
            Duration::from_millis(100),
        )
        .await;
        let provider_id =
            insert_provider_with_priority(&db, "grok", "Grok Abort Stub", sse_base_url, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            Arc::clone(&circuit),
            Arc::clone(&session),
        ));
        let session_id = "grok-session-abort";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/grok/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-grok-session-id", session_id)
            .body(Body::from(
                r#"{"model":"grok-abort-model","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();

        let mut body = Box::pin(response.into_body());
        let first_frame = tokio::time::timeout(
            Duration::from_secs(2),
            std::future::poll_fn(|cx| body.as_mut().poll_frame(cx)),
        )
        .await
        .expect("first Grok frame timeout")
        .expect("first Grok frame")
        .expect("first Grok frame ok");
        let first_chunk = first_frame.into_data().expect("data frame");
        assert!(String::from_utf8_lossy(&first_chunk).contains("hello"));
        drop(body);

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "grok");
        assert_eq!(detail.path, "/v1/responses");
        assert_eq!(detail.status, Some(499));
        assert_eq!(
            detail.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::StreamAborted.as_str())
        );
        assert!(detail.excluded_from_stats);
        assert_eq!(detail.final_provider_id, provider_id);
        assert_eq!(detail.input_tokens, None);
        assert_eq!(detail.output_tokens, None);

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts JSON");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("stream_error: code=GW_STREAM_ABORTED")
        );

        let special_settings: Value = serde_json::from_str(
            detail
                .special_settings_json
                .as_deref()
                .expect("special settings JSON"),
        )
        .expect("special settings JSON parses");
        let abort_entry = special_settings
            .as_array()
            .expect("special settings array")
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("client_abort")
                    && entry.get("scope").and_then(Value::as_str) == Some("stream")
            })
            .expect("client abort diagnostics");
        assert_eq!(
            abort_entry.get("reason").and_then(Value::as_str),
            Some("stream_finalized_aborted")
        );
        assert_eq!(
            abort_entry.get("detected_by").and_then(Value::as_str),
            Some("stream_finalize")
        );
        assert!(abort_entry.get("completion_seen").is_none());
        assert!(abort_entry.get("drained_chunks").is_none());
        assert_eq!(
            circuit.snapshot(provider_id, 0).state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(session.get_bound_provider("grok", session_id, 0), None);

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_codex_responses_abort_drains_completion_as_success() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-responses-relay-abort-test.sqlite"),
        )
        .expect("init test db");
        let first_chunk = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
        );
        let completion_chunk = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-relay-abort\",\"status\":\"completed\",\"model\":\"gpt-route-responses-relay\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_delayed_chunked_sse_upstream(
            first_chunk,
            completion_chunk,
            Duration::from_millis(500),
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Responses Relay Stub", sse_base_url, 0);

        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state(app_handle, db.clone(), log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-responses-relay","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();

        let mut body_stream = Box::pin(response.into_body().into_data_stream());
        let first_chunk = tokio::time::timeout(
            Duration::from_secs(2),
            std::future::poll_fn(|cx| body_stream.as_mut().poll_next(cx)),
        )
        .await
        .expect("first relay chunk timeout")
        .expect("first relay chunk")
        .expect("first relay chunk ok");
        assert!(String::from_utf8_lossy(&first_chunk).contains("hello"));
        drop(body_stream);

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/responses");
        assert_eq!(detail.status, Some(200));
        assert_eq!(detail.error_code, None);
        assert!(!detail.excluded_from_stats);
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-responses-relay")
        );
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_some());
        assert_eq!(detail.input_tokens, Some(1));
        assert_eq!(detail.output_tokens, Some(2));
        assert_eq!(detail.total_tokens, Some(3));

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let special_settings: Value = serde_json::from_str(
            detail
                .special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let special_settings = special_settings.as_array().expect("special settings array");
        if let Some(abort_entry) = special_settings.iter().find(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("client_abort")
                && entry.get("scope").and_then(Value::as_str) == Some("stream")
        }) {
            assert_eq!(
                abort_entry.get("completion_seen").and_then(Value::as_bool),
                Some(true)
            );
            assert!(abort_entry
                .get("drained_chunks")
                .and_then(Value::as_i64)
                .is_some_and(|count| count >= 1));
        }

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_sse_fake_200_persists_error_without_session_binding() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-sse-fake-200-test.sqlite"))
            .expect("init test db");
        let fake_200_body = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"message\":\"quota exhausted\",\"type\":\"insufficient_quota\"}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(fake_200_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "SSE Fake 200 Stub", sse_base_url, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-route-fake-200";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-route-fake-200","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(body.is_empty());

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        let logged_session_id = detail
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .expect("logged session id");
        assert_eq!(detail.status, Some(502));
        assert_eq!(detail.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-fake-200")
        );
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_some());

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("stream_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );

        let error_details: Value = serde_json::from_str(
            detail
                .error_details_json
                .as_deref()
                .expect("error details json"),
        )
        .expect("error details json parses");
        assert_eq!(
            error_details
                .get("gateway_error_code")
                .and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            error_details.get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            error_details.get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );

        let circuit_snapshot = circuit.snapshot(provider_id, 0);
        assert_eq!(
            circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(circuit_snapshot.failure_count, 1);
        assert_eq!(
            session.get_bound_provider("codex", logged_session_id, 0),
            None
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_json_fake_200_returns_bad_gateway_without_session_binding() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-json-fake-200-test.sqlite"),
        )
        .expect("init test db");
        let fake_200_body =
            r#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#;
        let (json_base_url, json_task) = spawn_json_upstream(fake_200_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "JSON Fake 200 Stub", json_base_url, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-route-json-fake-200";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-route-json-fake-200","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("GW_FAKE_200"));

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        let logged_session_id = detail
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .expect("logged session id");
        assert_eq!(detail.status, Some(502));
        assert_eq!(detail.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-json-fake-200")
        );
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_none());

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );

        let error_details: Value = serde_json::from_str(
            detail
                .error_details_json
                .as_deref()
                .expect("error details json"),
        )
        .expect("error details json parses");
        assert_eq!(
            error_details
                .get("gateway_error_code")
                .and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            error_details.get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            error_details.get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );

        let circuit_snapshot = circuit.snapshot(provider_id, 0);
        assert_eq!(
            circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(circuit_snapshot.failure_count, 1);
        assert!(circuit_snapshot.cooldown_until.is_some());
        assert_eq!(
            session.get_bound_provider("codex", logged_session_id, 0),
            None
        );

        json_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_json_fake_200_quota_fails_over_to_next_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.provider_cooldown_seconds = 30;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-json-fake-200-quota-failover-test.sqlite"),
        )
        .expect("init test db");
        let fake_200_body =
            r#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#;
        let success_body = r#"{"id":"stub-ok","object":"chat.completion","choices":[]}"#;
        let (quota_base_url, quota_task) = spawn_json_upstream(fake_200_body).await;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let quota_provider_id =
            insert_codex_provider_with_priority(&db, "Quota Stub", quota_base_url, 0);
        let success_provider_id =
            insert_codex_provider_with_priority(&db, "Success Stub", success_base_url, 1);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session,
        ));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-route-json-fake-200-quota","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload.get("id").and_then(Value::as_str), Some("stub-ok"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(quota_provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let provider_chain: Value =
            serde_json::from_str(log.provider_chain_json.as_deref().expect("provider chain"))
                .expect("provider chain json");
        let chain = provider_chain.as_array().expect("provider chain array");
        assert_eq!(
            chain[0].get("provider_id").and_then(Value::as_i64),
            Some(quota_provider_id)
        );
        assert_eq!(
            chain[1].get("provider_id").and_then(Value::as_i64),
            Some(success_provider_id)
        );

        let circuit_snapshot = circuit.snapshot(quota_provider_id, 0);
        assert!(circuit_snapshot.cooldown_until.is_some());

        quota_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_unknown_length_json_fake_200_logs_error_without_session_binding() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-unknown-length-json-fake-200-test.sqlite"),
        )
        .expect("init test db");
        let fake_200_body =
            r#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#;
        let (json_base_url, json_task) = spawn_unknown_length_json_upstream(fake_200_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Unknown Length JSON Fake 200 Stub",
            json_base_url,
            0,
        );

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, writer_task) =
            request_logs::start_buffered_writer(app_handle.clone(), db.clone());
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db.clone(),
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-route-unknown-length-json-fake-200";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-route-unknown-length-json-fake-200","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .expect("trace header")
            .to_string();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("quota exhausted"));

        tokio::time::timeout(Duration::from_secs(2), writer_task)
            .await
            .expect("writer drain timeout")
            .expect("writer task joins");

        let detail = request_logs::get_by_trace_id(&db, &trace_id)
            .expect("query request log")
            .expect("persisted request log");
        assert_eq!(detail.cli_key, "codex");
        assert_eq!(detail.path, "/v1/chat/completions");
        let logged_session_id = detail
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .expect("logged session id");
        assert_eq!(detail.status, Some(502));
        assert_eq!(detail.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_eq!(
            detail.requested_model.as_deref(),
            Some("gpt-route-unknown-length-json-fake-200")
        );
        assert_eq!(detail.final_provider_id, provider_id);
        assert!(detail.ttfb_ms.is_none());

        let attempts: Value = serde_json::from_str(&detail.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );

        let circuit_snapshot = circuit.snapshot(provider_id, 0);
        assert_eq!(
            circuit_snapshot.state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(circuit_snapshot.failure_count, 1);
        assert_eq!(
            session.get_bound_provider("codex", logged_session_id, 0),
            None
        );

        json_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn chat_completions_unknown_length_success_streams_before_completion_and_ignores_aggregate_limit(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-unknown-length-json-success-stream.sqlite"),
        )
        .expect("init test db");

        let first_chunk = br#"{"id":"msg_chunked","type":"message","role":"assistant","model":"claude-3-5-sonnet","content":[{"type":"text","text":""#.to_vec();
        let mut second_chunk = vec![b'a'; 20 * 1024 * 1024 + 1024];
        second_chunk.extend_from_slice(
            br#""}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}"#,
        );
        let (json_base_url, json_task) = spawn_delayed_chunked_json_upstream(
            first_chunk.clone(),
            second_chunk,
            Duration::from_secs(3),
        )
        .await;
        let provider_id = insert_provider_with_priority(
            &db,
            "claude",
            "Unknown Length JSON Success Stub",
            json_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/claude/_aio/provider/{provider_id}/v1/messages"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":512,"messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(1), router.oneshot(request))
            .await
            .expect("response returned before delayed body completion")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let mut body_stream = Box::pin(response.into_body().into_data_stream());
        let first = tokio::time::timeout(
            Duration::from_secs(1),
            std::future::poll_fn(|cx| body_stream.as_mut().poll_next(cx)),
        )
        .await
        .expect("first body chunk timeout")
        .expect("first body chunk")
        .expect("first body chunk ok");
        assert!(
            first.starts_with(&first_chunk),
            "first body chunk should stream before upstream completion"
        );

        let mut total_bytes = first.len();
        loop {
            let next = tokio::time::timeout(
                Duration::from_secs(5),
                std::future::poll_fn(|cx| body_stream.as_mut().poll_next(cx)),
            )
            .await
            .expect("body completion timeout");
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.expect("body chunk ok");
            total_bytes += chunk.len();
        }
        assert!(total_bytes > 20 * 1024 * 1024);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        json_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_claude_compact_request_persists_request_kind_special_setting() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let app_settings = settings::AppSettings::default();
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("gateway-route-compact-kind-test.sqlite"))
            .expect("init test db");
        let (upstream_base_url, upstream_task) = spawn_json_upstream(
            r#"{"id":"msg_compact","type":"message","role":"assistant","content":[{"type":"text","text":"summary"}],"model":"claude-3-5-sonnet","usage":{"input_tokens":1,"output_tokens":1}}"#,
        )
        .await;
        let provider_id =
            insert_provider_with_priority(&db, "claude", "Compact Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/claude/_aio/provider/{provider_id}/v1/messages"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":512,"system":[{"type":"text","text":"You are a helpful AI assistant tasked with summarizing conversations. Follow the instructions."}],"messages":[{"role":"user","content":"Your task is to create a detailed summary of the conversation so far."}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.cli_key, "claude");
        assert_eq!(log.path, "/v1/messages");
        assert_eq!(log.status, Some(200));

        let special_settings: Value = serde_json::from_str(
            log.special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let special_settings = special_settings.as_array().expect("special settings array");
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("request_kind")
                && entry.get("kind").and_then(Value::as_str) == Some("compact")
        }));

        upstream_task.abort();
    }

    async fn spawn_delayed_json_upstream(
        body: &'static str,
        first_byte_delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind delayed json upstream stub");
        let addr = listener.local_addr().expect("delayed json upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(first_byte_delay).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_runtime_router_claude_compact_request_survives_first_byte_delay_beyond_configured_timeout(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.upstream_first_byte_timeout_seconds = 1;
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("gateway-route-compact-timeout-test.sqlite"),
        )
        .expect("init test db");
        let (upstream_base_url, upstream_task) = spawn_delayed_json_upstream(
            r#"{"id":"msg_compact_slow","type":"message","role":"assistant","content":[{"type":"text","text":"summary"}],"model":"claude-3-5-sonnet","usage":{"input_tokens":1,"output_tokens":1}}"#,
            Duration::from_secs(2),
        )
        .await;
        let provider_id =
            insert_provider_with_priority(&db, "claude", "Compact Slow Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/claude/_aio/provider/{provider_id}/v1/messages"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"claude-3-5-sonnet","max_tokens":512,"system":[{"type":"text","text":"You are a helpful AI assistant tasked with summarizing conversations. Follow the instructions."}],"messages":[{"role":"user","content":"Your task is to create a detailed summary of the conversation so far."}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_buffers_created_event_until_completion() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-disabled-responses-stream.sqlite"))
            .expect("init test db");
        let first_chunk = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-disabled-stream\",\"status\":\"in_progress\",\"model\":\"gpt-disabled-stream\",\"output\":[]}}\n\n"
        );
        let completion_chunk = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-disabled-stream\",\"status\":\"completed\",\"model\":\"gpt-disabled-stream\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"first visible\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_delayed_chunked_sse_upstream(
            first_chunk,
            completion_chunk,
            Duration::from_secs(3),
        )
        .await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Disabled Responses Stream", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-disabled-stream","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let mut response_future = Box::pin(router.oneshot(request));
        assert!(
            tokio::time::timeout(Duration::from_secs(1), response_future.as_mut())
                .await
                .is_err(),
            "metadata-only prefix must remain buffered before completion"
        );
        let response = tokio::time::timeout(Duration::from_secs(5), response_future)
            .await
            .expect("response returned after delayed completion")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let full_body = String::from_utf8_lossy(&body);
        assert!(full_body.contains("response.created"));
        assert!(full_body.contains("response.completed"));
        assert!(full_body.contains("resp-disabled-stream"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_mismatched_delta_and_final_streams_successfully() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-disabled-delta-mismatch-success.sqlite"),
        )
        .expect("init test db");
        let mismatch_sse_body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello \"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-disabled-mismatch\",\"status\":\"completed\",\"model\":\"gpt-disabled-mismatch\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello world\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        );
        let (mismatch_base_url, mismatch_task) = spawn_sse_upstream(mismatch_sse_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Disabled Mismatch Stream",
            mismatch_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-disabled-mismatch","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("hello "));
        assert!(body_text.contains("hello world"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        mismatch_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_empty_success_stream_returns_bad_gateway_without_session_binding() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-empty-success-stream.sqlite"))
            .expect("init test db");
        let empty_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-empty\",\"status\":\"completed\",\"model\":\"gpt-empty-stream\",\"output\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}}\n\n"
        );
        let (empty_base_url, empty_task) = spawn_sse_upstream(empty_sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Empty Stream Stub", empty_base_url, 0);

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-empty-success";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-empty-stream","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_EMPTY_RESPONSE")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_EMPTY_RESPONSE"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("error_category").and_then(Value::as_str),
            Some("PROVIDER_ERROR")
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_EMPTY_RESPONSE")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 1);
        assert_eq!(session.get_bound_provider("codex", session_id, 0), None);

        empty_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_empty_success_stream_fails_over_to_next_provider() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-empty-success-failover.sqlite"))
            .expect("init test db");
        let empty_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-empty-first\",\"status\":\"completed\",\"model\":\"gpt-empty-failover\",\"output\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}}\n\n"
        );
        let success_sse_body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-ok-after-empty\",\"status\":\"completed\",\"model\":\"gpt-empty-failover\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}],\"usage\":{\"input_tokens\":11,\"output_tokens\":1,\"total_tokens\":12}}}\n\n"
        );
        let (empty_base_url, empty_task) = spawn_sse_upstream(empty_sse_body).await;
        let (success_base_url, success_task) = spawn_sse_upstream(success_sse_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Empty First Stream", empty_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Success Second Stream", success_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-empty-failover","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-ok-after-empty"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[0].get("error_code").and_then(Value::as_str),
            Some("GW_EMPTY_RESPONSE")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        empty_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_split_capacity_error_retries_same_provider_before_commit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy.backoff_ms = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let metadata = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-capacity-first\",\"status\":\"in_progress\"}}\n\n",
            "event: response.in_progress\n",
            "data: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp-capacity-first\",\"status\":\"in_progress\"}}\n\n"
        );
        let capacity_error = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp-capacity-first\",\"status\":\"failed\",\"error\":{\"type\":\"server_error\",\"code\":\"model_at_capacity\",\"message\":\"Selected model is at capacity\"}}}\n\n"
        );
        let success_body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"retry-ok\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-capacity-retry-ok\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry-ok\"}]}],\"usage\":{\"input_tokens\":11,\"output_tokens\":1,\"total_tokens\":12}}}\n\n"
        );
        let (base_url, call_count, upstream_task) = spawn_retrying_chunked_sse_upstream(
            metadata,
            capacity_error,
            Duration::from_millis(25),
            success_body,
        )
        .await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-responses-split-capacity-retry.sqlite"),
        )
        .expect("init test db");
        let provider_id =
            insert_codex_provider_with_priority(&db, "Split Capacity Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-capacity-retry","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("retry-ok"));
        assert!(!body.contains("Selected model is at capacity"));
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0]["provider_id"].as_i64(), Some(provider_id));
        assert_eq!(attempts[0]["error_code"].as_str(), Some("GW_FAKE_200"));
        assert_eq!(attempts[0]["decision"].as_str(), Some("retry"));
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"].as_str(),
            Some("transient_capacity")
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["message"].as_str(),
            Some("Selected model is at capacity")
        );
        assert!(attempts[0]["stream_internal_error"]["matched_keyword"].is_null());
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"].as_str(),
            Some("retry_same_provider")
        );
        assert_eq!(attempts[1]["outcome"].as_str(), Some("success"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_gzip_capacity_error_is_decoded_before_retry_classification() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.provider_cooldown_seconds = 0;
        app_settings.upstream_retry_policy.backoff_ms = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let first_body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-gzip-capacity\",\"status\":\"in_progress\"}}\n\n",
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp-gzip-capacity\",\"status\":\"failed\",\"error\":{\"message\":\"Selected model is at capacity\"}}}\n\n"
        );
        let success_body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"gzip-retry-ok\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-gzip-retry-ok\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"gzip-retry-ok\"}]}],\"usage\":{\"input_tokens\":11,\"output_tokens\":1,\"total_tokens\":12}}}\n\n"
        );
        let (base_url, call_count, upstream_task) =
            spawn_retrying_sse_upstream(gzip_bytes(first_body.as_bytes()), true, success_body)
                .await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-responses-gzip-capacity-retry.sqlite"),
        )
        .expect("init test db");
        insert_codex_provider_with_priority(&db, "Gzip Capacity Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-gzip-capacity","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("gzip-retry-ok"));
        assert!(!body.contains("Selected model is at capacity"));
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        let log = recv_terminal_request_log(&mut log_rx).await;
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"].as_str(),
            Some("transient_capacity")
        );
        assert_eq!(attempts[1]["outcome"].as_str(), Some("success"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_unknown_stream_error_is_sanitized_before_commit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let unknown_body = concat!(
            "event: response.error\n",
            "data: {\"type\":\"response.error\",\"error\":{\"message\":\"SYNTHETIC_UPSTREAM_SCREENSHOT_ERROR\",\"type\":\"vendor_oddity\"}}\n\n"
        );
        let (base_url, upstream_task) = spawn_sse_upstream(unknown_body).await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-responses-unknown-stream-error.sqlite"),
        )
        .expect("init test db");
        insert_codex_provider_with_priority(&db, "Unknown Stream Error Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-unknown-stream-error","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let client_body = String::from_utf8_lossy(&body);
        assert!(!client_body.contains("SYNTHETIC_UPSTREAM_SCREENSHOT_ERROR"));
        assert!(!client_body.contains("vendor_oddity"));
        let payload: Value = serde_json::from_slice(&body).expect("gateway error JSON");
        assert_eq!(payload["error_code"], "GW_FAKE_200");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(
            attempts[0]["stream_internal_error"]["message"],
            "SYNTHETIC_UPSTREAM_SCREENSHOT_ERROR"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"],
            "unknown"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"],
            "sanitized_before_commit"
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_post_commit_terminal_frame_is_dropped_by_relay_firewall() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let visible = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"SYNTHETIC_VISIBLE_OUTPUT\"}\n\n"
        );
        let terminal = concat!(
            "event: response.failed\r\n",
            "data: {\"type\":\"response.failed\",\"error\":{\"code\":\"server_error\",\"message\":\"SYNTHETIC_TERMINAL_SECRET\"}}\r\n\r\n"
        );
        let (base_url, upstream_task) =
            spawn_delayed_chunked_sse_upstream(visible, terminal, Duration::from_millis(600)).await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-post-commit-firewall.sqlite"))
            .expect("init test db");
        insert_codex_provider_with_priority(&db, "Post Commit Firewall Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-post-commit-firewall","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("SYNTHETIC_VISIBLE_OUTPUT"));
        assert!(!body.contains("SYNTHETIC_TERMINAL_SECRET"));
        assert!(!body.contains("response.failed"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(
            attempts[0]["stream_internal_error"]["message"],
            "SYNTHETIC_TERMINAL_SECRET"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"],
            "transient_provider"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"],
            "dropped_after_commit"
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_post_commit_passthrough_exception_preserves_terminal_frame() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.enable_response_fixer = false;
        app_settings
            .upstream_retry_policy
            .stream_internal_errors
            .passthrough_keywords = vec!["SYNTHETIC_PASSTHROUGH_TICKET".to_string()];
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let visible = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"SYNTHETIC_VISIBLE_BEFORE_PASSTHROUGH\"}\n\n"
        );
        let terminal = concat!(
            "event: response.failed\r\n",
            "data: {\"type\":\"response.failed\",\"error\":{\"code\":\"vendor_oddity\",\"message\":\"SYNTHETIC_PASSTHROUGH_TICKET\"}}\r\n\r\n"
        );
        let (base_url, upstream_task) =
            spawn_delayed_chunked_sse_upstream(visible, terminal, Duration::from_millis(600)).await;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-post-commit-passthrough.sqlite"))
            .expect("init test db");
        insert_codex_provider_with_priority(&db, "Post Commit Passthrough Stub", base_url, 0);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-post-commit-passthrough","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("SYNTHETIC_VISIBLE_BEFORE_PASSTHROUGH"));
        assert!(body.contains(terminal));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(
            attempts[0]["stream_internal_error"]["message"],
            "SYNTHETIC_PASSTHROUGH_TICKET"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"],
            "unknown"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"],
            "passthrough_exception"
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_capacity_code_is_raw_passthrough_when_terminal_firewall_is_disabled() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.upstream_retry_policy.enabled = false;
        app_settings
            .upstream_retry_policy
            .stream_internal_errors
            .enabled = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-responses-sse-fake-200.sqlite"))
            .expect("init test db");
        let fake_200_body = concat!(
            "event: response.error\n",
            "data: {\"type\":\"response.error\",\"error\":{\"message\":\"temporary upstream failure\",\"type\":\"server_error\",\"code\":\"SERVER_IS_OVERLOADED\"},\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}\n\n"
        );
        let (fake_200_base_url, fake_200_task) = spawn_sse_upstream(fake_200_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Responses Fake 200 Stub",
            fake_200_base_url,
            0,
        );

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-responses-fake-200";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-fake-200-stream","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let client_body = String::from_utf8_lossy(&body);
        assert!(client_body.contains("SERVER_IS_OVERLOADED"));
        assert!(client_body.contains("temporary upstream failure"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );
        assert!(attempts[0]["error_category"].is_null());
        assert!(attempts[0]["error_code"].is_null());
        assert_eq!(attempts[0]["decision"], "success");
        assert_eq!(
            attempts[0]["stream_internal_error"]["classification"],
            "disabled"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["error_code"],
            "SERVER_IS_OVERLOADED"
        );
        assert_eq!(
            attempts[0]["stream_internal_error"]["disposition"],
            "disabled_passthrough"
        );
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 0);
        assert_eq!(
            session.get_bound_provider("codex", session_id, 0),
            Some(provider_id)
        );

        fake_200_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_responses_sse_fake_200_oauth_quota_skips_circuit_failure() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let mut _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.upstream_retry_policy.max_retries = 0;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-responses-sse-oauth-fake-200-quota.sqlite"),
        )
        .expect("init test db");
        let fake_200_body = concat!(
            "event: response.error\n",
            "data: {\"type\":\"response.error\",\"error\":{\"message\":\"quota exhausted\",\"type\":\"insufficient_quota\"},\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}\n\n"
        );
        let (fake_200_base_url, fake_200_task) = spawn_sse_upstream(fake_200_body).await;
        _env.set_var(
            "AIO_CODING_HUB_TEST_CODEX_OAUTH_BASE_URL",
            fake_200_base_url.clone(),
        );
        let provider_id = insert_codex_oauth_provider_with_base_urls(
            &db,
            "Responses OAuth Quota Stub",
            vec![fake_200_base_url],
            0,
        );

        let circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::new(),
            None,
        ));
        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            circuit.clone(),
            session.clone(),
        ));
        let session_id = "sess-responses-oauth-fake-200";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-oauth-fake-200-stream","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        let payload_error_code = payload.get("error_code").and_then(Value::as_str);
        assert_eq!(payload_error_code, Some("GW_FAKE_200"));
        assert!(!String::from_utf8_lossy(&body).contains("quota exhausted"));
        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.error_code.as_deref(), payload_error_code);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        let attempt = &attempts[0];
        assert_eq!(
            attempt.get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempt.get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );
        assert_eq!(
            attempt.get("decision").and_then(Value::as_str),
            Some("abort")
        );
        assert_eq!(attempt["stream_internal_error"]["classification"], "quota");
        assert_eq!(
            attempt["stream_internal_error"]["disposition"],
            "sanitized_before_commit"
        );
        assert_eq!(attempt.get("circuit_failure_count"), Some(&Value::from(0)));
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 0);
        assert_eq!(session.get_bound_provider("codex", session_id, 0), None);

        fake_200_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_v1_codex_responses_empty_success_is_intercepted() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-v1-codex-empty-success.sqlite"))
            .expect("init test db");
        let empty_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-v1-codex-empty\",\"status\":\"completed\",\"model\":\"gpt-v1-codex-empty\",\"output\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}}\n\n"
        );
        let (empty_base_url, empty_task) = spawn_sse_upstream(empty_sse_body).await;
        insert_codex_provider_with_priority(&db, "V1 Codex Empty Stream", empty_base_url, 0);

        let session = Arc::new(session_manager::SessionManager::new());
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state_with_parts(
            app_handle,
            db,
            log_tx,
            Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig::default(),
                HashMap::new(),
                None,
            )),
            session.clone(),
        ));
        let session_id = "sess-v1-codex-empty-success";
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/codex/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-session-id", session_id)
            .body(Body::from(
                r#"{"model":"gpt-v1-codex-empty","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_EMPTY_RESPONSE")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_EMPTY_RESPONSE"));
        assert_eq!(session.get_bound_provider("codex", session_id, 0), None);

        empty_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_function_call_only_stream_is_not_empty_success() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-function-call-only-stream.sqlite"))
            .expect("init test db");
        let function_call_sse_body = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"call_1\",\"type\":\"function_call\",\"name\":\"lookup\",\"arguments\":\"{}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-tool-only\",\"status\":\"completed\",\"model\":\"gpt-tool-only\",\"output\":[{\"id\":\"call_1\",\"type\":\"function_call\",\"name\":\"lookup\",\"arguments\":\"{}\"}],\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}}\n\n"
        );
        let (function_call_base_url, function_call_task) =
            spawn_sse_upstream(function_call_sse_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Function Call Only Stream",
            function_call_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-tool-only","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-tool-only"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        function_call_task.abort();
    }
}
