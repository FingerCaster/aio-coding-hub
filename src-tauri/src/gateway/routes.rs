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

    async fn spawn_repeating_json_upstream(
        body: &'static str,
        response_count: usize,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind repeating json upstream stub");
        let addr = listener.local_addr().expect("repeating json upstream addr");
        let hit_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hit_count_for_task = Arc::clone(&hit_count);
        let task = tokio::spawn(async move {
            for _ in 0..response_count {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let _ = read_complete_http_request(&mut socket).await;
                hit_count_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), hit_count, task)
    }

    async fn spawn_sequence_json_upstream(
        bodies: Vec<&'static str>,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sequence json upstream stub");
        let addr = listener.local_addr().expect("sequence json upstream addr");
        let hit_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hit_count_for_task = Arc::clone(&hit_count);
        let task = tokio::spawn(async move {
            for body in bodies {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let _ = read_complete_http_request(&mut socket).await;
                hit_count_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), hit_count, task)
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
        body: &'static str,
    ) -> (
        String,
        tokio::sync::oneshot::Receiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
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

    async fn spawn_codex_previous_response_retry_upstream() -> (
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
                    (
                        "200 OK",
                        r#"{"id":"stub-ok","object":"response","output":[]}"#,
                    )
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

    fn gunzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut decoder = flate2::read::GzDecoder::new(input);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut out).expect("gzip read");
        out
    }

    async fn spawn_status_json_upstream(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind status json upstream stub");
        let addr = listener.local_addr().expect("status json upstream addr");
        let task = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), task)
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

    async fn spawn_sequence_capturing_sse_upstream(
        bodies: Vec<&'static str>,
    ) -> (
        String,
        tokio::sync::mpsc::Receiver<CapturedRawRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sequence capturing sse upstream stub");
        let addr = listener
            .local_addr()
            .expect("sequence capturing sse upstream addr");
        let (tx, rx) = tokio::sync::mpsc::channel(bodies.len().max(1));
        let task = tokio::spawn(async move {
            for body in bodies {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let request =
                    split_raw_http_request(read_complete_http_request_bytes(&mut socket).await);
                let _ = tx.send(request).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{addr}"), rx, task)
    }

    async fn spawn_sequence_then_keepalive_capturing_sse_upstream(
        first_body: &'static str,
        keepalive_interval: Duration,
    ) -> (
        String,
        tokio::sync::mpsc::Receiver<CapturedRawRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sequence keepalive sse upstream stub");
        let addr = listener
            .local_addr()
            .expect("sequence keepalive sse upstream addr");
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        let task = tokio::spawn(async move {
            let Ok((mut first_socket, _)) = listener.accept().await else {
                return;
            };
            let first_request =
                split_raw_http_request(read_complete_http_request_bytes(&mut first_socket).await);
            let _ = tx.send(first_request).await;
            let first_response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                first_body.len(),
                first_body
            );
            let _ = first_socket.write_all(first_response.as_bytes()).await;
            let _ = first_socket.shutdown().await;

            let Ok((mut second_socket, _)) = listener.accept().await else {
                return;
            };
            let second_request =
                split_raw_http_request(read_complete_http_request_bytes(&mut second_socket).await);
            let _ = tx.send(second_request).await;
            let headers = concat!(
                "HTTP/1.1 200 OK\r\n",
                "content-type: text/event-stream; charset=utf-8\r\n",
                "transfer-encoding: chunked\r\n",
                "connection: keep-alive\r\n",
                "\r\n"
            );
            let _ = second_socket.write_all(headers.as_bytes()).await;
            loop {
                let chunk = ": keepalive\n\n";
                let encoded = format!("{:X}\r\n{}\r\n", chunk.len(), chunk);
                if second_socket.write_all(encoded.as_bytes()).await.is_err() {
                    break;
                }
                tokio::time::sleep(keepalive_interval).await;
            }
        });

        (format!("http://{addr}"), rx, task)
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

    fn insert_provider_with_priority(
        db: &db::Db,
        cli_key: &str,
        name: &str,
        base_url: String,
        priority: i64,
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
                model_mapping: None,
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
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
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

    fn insert_codex_provider_with_stream_idle_timeout(
        db: &db::Db,
        name: &str,
        base_url: String,
        priority: i64,
        stream_idle_timeout_seconds: u32,
    ) -> i64 {
        let provider_id = insert_codex_provider_with_priority(db, name, base_url, priority);
        db.open_connection()
            .expect("open test db")
            .execute(
                "UPDATE providers SET stream_idle_timeout_seconds = ?1 WHERE id = ?2",
                rusqlite::params![stream_idle_timeout_seconds, provider_id],
            )
            .expect("override provider stream idle timeout in test fixture");
        provider_id
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
                model_mapping: None,
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
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
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
                model_mapping: None,
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
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
            },
        )
        .expect("insert cx2cc bridge provider")
        .id;
        append_default_route_provider(db, "claude", provider_id);
        provider_id
    }

    fn insert_codex_bridge_provider(
        db: &db::Db,
        bridge_type: &str,
        source_provider_id: i64,
        priority: i64,
    ) -> i64 {
        let provider_id = providers::upsert(
            db,
            providers::ProviderUpsertParams {
                provider_id: None,
                cli_key: "codex".to_string(),
                name: format!("Codex Bridge Stub {bridge_type}"),
                base_urls: vec![],
                base_url_mode: providers::ProviderBaseUrlMode::Order,
                auth_mode: None,
                api_key: None,
                enabled: true,
                cost_multiplier: 1.0,
                priority: Some(priority),
                claude_models: None,
                model_mapping: None,
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
                bridge_type: Some(bridge_type.to_string()),
                stream_idle_timeout_seconds: None,
                extension_values: None,
                upstream_retry_policy_override: None,
                upstream_retry_policy_override_specified: false,
            },
        )
        .expect("insert codex bridge provider")
        .id;
        append_default_route_provider(db, "codex", provider_id);
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

    fn assert_no_bplus_continuation_fields(entry: &Value) {
        for key in [
            "clientContractVersion",
            "reconstructionStatus",
            "visibleAssemblyKind",
            "canonicalResponseId",
            "canonicalResponseIdContinuity",
            "rounds",
            "clientUsageKind",
            "providerUsageKind",
            "clientUsage",
            "providerRepairUsage",
            "nonVisiblePolicy",
            "phase0SampleAudit",
            "timing",
            "timeoutPolicy",
            "timeoutSource",
            "fallbackAction",
            "repairWallClockBudget",
            "downstreamHeadersCommittedDuringRepair",
            "aggregateRawBytes",
        ] {
            assert!(
                entry.get(key).is_none(),
                "stable continuation setting must not include B+ field {key}: {entry}"
            );
        }
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
    async fn official_privacy_filter_redacts_gzipped_codex_responses_before_upstream() {
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

        assert!(captured.has_header_line("content-encoding: gzip"));
        let decoded_body = gunzip_bytes(&captured.body);
        let decoded_body_text = String::from_utf8_lossy(&decoded_body);
        assert!(decoded_body_text.contains("[电话]"));
        assert!(!decoded_body_text.contains("13344441520"));

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
            spawn_codex_previous_response_retry_upstream().await;
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
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
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
                .to_string(),
            ))
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
        assert!(!String::from_utf8_lossy(&first.body).contains("13344441520"));
        assert!(String::from_utf8_lossy(&first.body).contains("[电话]"));

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
    async fn gateway_preserves_gzipped_codex_request_when_plugins_do_not_mutate_body() {
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
        let db = db::init_for_tests(&db_dir.path().join("gateway-gzip-passthrough-test.sqlite"))
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
            .body(Body::from(compressed_body.clone()))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");

        assert!(captured.has_header_line("content-encoding: gzip"));
        assert_eq!(captured.body, compressed_body);
        assert!(!captured.text().contains("13344441520"));
        assert!(!captured.text().contains("[电话]"));

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
    async fn gateway_plugin_response_after_rewrites_non_stream_body() {
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
                    r#"{"id":"rewritten","object":"chat.completion","choices":[]}"#.to_string(),
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
                r#"{"model":"gpt-plugin","messages":[{"role":"user","content":"hello"}]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload.get("id").and_then(Value::as_str), Some("rewritten"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
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
    async fn gateway_plugin_response_chunk_rewrites_stream_body() {
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
        let (upstream_base_url, upstream_task) =
            spawn_sse_upstream("data: secret-stream\n\n").await;
        let provider_id = insert_codex_provider(&db, upstream_base_url);

        let executor =
            InMemoryGatewayPluginExecutor::new().with_stream_handler("test.stream-chunk", |ctx| {
                let chunk = ctx.stream.chunk.expect("visible stream chunk");
                assert!(chunk.contains("secret-stream"));
                GatewayHookResult {
                    stream_chunk: Some(chunk.replace("secret-stream", "redacted-stream")),
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
                r#"{"model":"gpt-plugin","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
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

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
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
    async fn codex_reasoning_guard_non_stream_exhausts_budget_with_terminal_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-budget-return.sqlite"))
            .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let (upstream_base_url, hit_count, upstream_task) =
            spawn_repeating_json_upstream(guard_body, 2).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Guard Return Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-return","input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );
        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_retry")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("retry_same_provider")
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );
        assert_eq!(
            attempts[1].get("decision").and_then(Value::as_str),
            Some("abort")
        );
        assert!(attempts.iter().all(|attempt| {
            attempt.get("error_code").and_then(Value::as_str) == Some("GW_CODEX_REASONING_GUARD")
                && attempt.get("circuit_failure_count").and_then(Value::as_u64) == Some(0)
        }));

        let special_settings: Value = serde_json::from_str(
            log.special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let special_settings = special_settings.as_array().expect("special settings array");
        let guard_settings: Vec<_> = special_settings
            .iter()
            .filter(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
            })
            .collect();
        assert_eq!(guard_settings.len(), 2);
        assert_eq!(
            guard_settings[0]
                .get("guardRetryPhase")
                .and_then(Value::as_str),
            Some("immediate")
        );
        assert_eq!(
            guard_settings[1].get("actionTaken").and_then(Value::as_str),
            Some("return_guard_error_no_circuit")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_default_non_stream_continuation_strategy_returns_unsupported() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-default-unsupported.sqlite"))
            .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let (upstream_base_url, hit_count, upstream_task) =
            spawn_repeating_json_upstream(guard_body, 2).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Guard Default Unsupported Stub",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-default","input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        let special_settings: Value = serde_json::from_str(
            log.special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let guard_setting = special_settings
            .as_array()
            .expect("special settings array")
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
            })
            .expect("guard setting");
        assert_eq!(
            guard_setting
                .get("guardPostMatchStrategy")
                .and_then(Value::as_str),
            Some("continuation_repair")
        );
        assert_eq!(
            guard_setting
                .get("guardStrategyOutcome")
                .and_then(Value::as_str),
            Some("unsupported")
        );
        assert_eq!(
            guard_setting
                .get("continuationSentRounds")
                .and_then(Value::as_u64),
            Some(0)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_switch_model_terminal_error_logs_fallback_model() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchModel;
        app_settings.codex_reasoning_guard_model_fallbacks = vec!["gpt-5.4".to_string()];
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-guard-switch-model-fake-200.sqlite"),
        )
        .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let fake_200_body =
            r#"{"error":{"message":"fallback synthetic failure","type":"upstream_error"}}"#;
        let (upstream_base_url, hit_count, upstream_task) =
            spawn_sequence_json_upstream(vec![guard_body, fake_200_body]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Guard Switch Model Fake 200 Stub",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"gpt-5.5","input":"hello"}"#))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("fallback synthetic failure"));
        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 2);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
        assert_eq!(log.requested_model.as_deref(), Some("gpt-5.4"));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_switch_model")
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_switch_model_next_provider_keeps_fallback_model() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchModel;
        app_settings.codex_reasoning_guard_model_fallbacks = vec!["gpt-5.4".to_string()];
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-guard-switch-model-next-provider.sqlite"),
        )
        .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let quota_body = r#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#;
        let success_body = r#"{"id":"resp-ok","object":"response","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#;
        let (guard_base_url, guard_hits, guard_task) =
            spawn_sequence_json_upstream(vec![guard_body, quota_body]).await;
        let (success_base_url, captured_success_body, success_task) =
            spawn_capturing_json_upstream(success_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Switch Model A", guard_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Guard Switch Model B", success_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"gpt-5.5","input":"hello"}"#))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-ok"));
        assert_eq!(guard_hits.load(std::sync::atomic::Ordering::SeqCst), 2);

        let captured_body = captured_success_body.await.expect("captured success body");
        let captured_json: Value =
            serde_json::from_str(&captured_body).expect("captured request json");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some("gpt-5.4")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert_eq!(log.requested_model.as_deref(), Some("gpt-5.4"));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 3);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_switch_model")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[1].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(
            attempts[2].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        guard_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_switch_model_next_provider_uses_fallback_model_template_rules() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchModel;
        app_settings.codex_reasoning_guard_model_fallbacks = vec!["gpt-5.4".to_string()];
        app_settings.codex_reasoning_guard_active_template_id = "custom-model-rules".to_string();
        app_settings.codex_reasoning_guard_custom_templates =
            vec![settings::CodexReasoningGuardRuleTemplate {
                id: "custom-model-rules".to_string(),
                name: "Custom model rules".to_string(),
                description: String::new(),
                rules: vec![
                    settings::CodexReasoningGuardTemplateRule {
                        id: "gpt-55-token-516".to_string(),
                        name: "gpt-5.5 reasoning_tokens == 516".to_string(),
                        reasoning_tokens: Some(516),
                        reasoning_tokens_formula: None,
                        action: settings::CodexReasoningGuardTemplateRuleAction::Intercept,
                        logic: settings::CodexReasoningGuardTemplateRuleLogic::And,
                        filters: vec![settings::CodexReasoningGuardTemplateFilter {
                            id: "requested-model-gpt-55".to_string(),
                            field: settings::CodexReasoningGuardTemplateFilterField::RequestedModel,
                            operator: settings::CodexReasoningGuardTemplateFilterOperator::Equals,
                            number_value: None,
                            bool_value: None,
                            string_value: Some("gpt-5.5".to_string()),
                            string_values: Vec::new(),
                        }],
                    },
                    settings::CodexReasoningGuardTemplateRule {
                        id: "gpt-54-token-999".to_string(),
                        name: "gpt-5.4 reasoning_tokens == 999".to_string(),
                        reasoning_tokens: Some(999),
                        reasoning_tokens_formula: None,
                        action: settings::CodexReasoningGuardTemplateRuleAction::Intercept,
                        logic: settings::CodexReasoningGuardTemplateRuleLogic::And,
                        filters: vec![settings::CodexReasoningGuardTemplateFilter {
                            id: "requested-model-gpt-54".to_string(),
                            field: settings::CodexReasoningGuardTemplateFilterField::RequestedModel,
                            operator: settings::CodexReasoningGuardTemplateFilterOperator::Equals,
                            number_value: None,
                            bool_value: None,
                            string_value: Some("gpt-5.4".to_string()),
                            string_values: Vec::new(),
                        }],
                    },
                ],
            }];
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-guard-switch-model-next-provider-rules.sqlite"),
        )
        .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let quota_body = r#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#;
        let fallback_rule_non_match_body = r#"{"id":"resp-fallback-rule-pass","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#;
        let (guard_base_url, guard_hits, guard_task) =
            spawn_sequence_json_upstream(vec![guard_body, quota_body]).await;
        let (success_base_url, captured_success_body, success_task) =
            spawn_capturing_json_upstream(fallback_rule_non_match_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Switch Rule A", guard_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Guard Switch Rule B", success_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"gpt-5.5","input":"hello"}"#))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-fallback-rule-pass"));
        assert_eq!(guard_hits.load(std::sync::atomic::Ordering::SeqCst), 2);

        let captured_body = captured_success_body.await.expect("captured success body");
        let captured_json: Value =
            serde_json::from_str(&captured_body).expect("captured request json");
        assert_eq!(
            captured_json.get("model").and_then(Value::as_str),
            Some("gpt-5.4")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert_eq!(log.requested_model.as_deref(), Some("gpt-5.4"));

        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 3);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_switch_model")
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("body_error: code=GW_FAKE_200")
        );
        assert_eq!(
            attempts[2].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert_eq!(
            attempts[2].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        guard_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_disabled_non_stream_emits_passive_features_only() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_enabled = false;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-disabled-passive.sqlite"))
            .expect("init test db");
        let passive_body = r#"{"id":"resp-passive","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"secret-answer"}]}]}"#;
        let (upstream_base_url, upstream_task) = spawn_json_upstream(passive_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Guard Disabled Passive",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"model":"gpt-passive","input":"hello"}"#))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-passive"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning feature sample");
        assert_eq!(
            feature_entry.get("ruleMode").and_then(Value::as_str),
            Some("reasoning_tokens")
        );
        assert_eq!(
            feature_entry
                .get("responseClassification")
                .and_then(Value::as_str),
            Some("complete")
        );
        assert_eq!(
            feature_entry.get("reasoningTokens").and_then(Value::as_i64),
            Some(516)
        );
        assert_eq!(
            feature_entry
                .get("finalAnswerOnly")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(!log
            .special_settings_json
            .as_deref()
            .unwrap_or_default()
            .contains("secret-answer"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_beta_remote_compaction_turn_is_not_exempt() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-beta-turn-guard.sqlite"))
            .expect("init test db");
        let guard_body = r#"{"id":"resp-beta-turn","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[{"type":"reasoning","summary":[]}]}"#;
        let (upstream_base_url, upstream_task) = spawn_json_upstream(guard_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Beta Turn Guard", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-codex-beta-features", "remote_compaction_v2")
            .header("x-codex-turn-metadata", r#"{"request_kind":"turn"}"#)
            .body(Body::from(
                r#"{"model":"gpt-beta-turn","reasoning":{"effort":"xhigh"},"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("reasoningTokens").and_then(Value::as_i64) == Some(516)
        }));
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning feature sample");
        assert_eq!(feature_entry.get("requestKind"), Some(&Value::Null));
        assert_eq!(
            feature_entry.get("interceptExemptReason"),
            Some(&Value::Null)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_feature_mode_non_stream_exhausts_final_answer_only_high() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        app_settings.codex_reasoning_guard_active_template_id =
            settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-feature-mode-return.sqlite"))
            .expect("init test db");
        let final_only_body = r#"{"id":"resp-final-only","object":"response","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"secret-final"}]}]}"#;
        let (upstream_base_url, upstream_task) = spawn_json_upstream(final_only_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Feature Mode Final Only",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-feature","reasoning":{"effort":"high"},"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let payload: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body"),
        )
        .expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings = parse_special_settings(&log);
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning feature sample");
        assert_eq!(
            feature_entry.get("ruleMode").and_then(Value::as_str),
            Some("final_answer_only_high_xhigh")
        );
        assert_eq!(
            feature_entry
                .get("requestReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            feature_entry
                .get("finalAnswerOnly")
                .and_then(Value::as_bool),
            Some(true)
        );
        let guard_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
            })
            .expect("codex reasoning guard sample");
        assert_eq!(
            guard_entry.get("hitSource").and_then(Value::as_str),
            Some("final_answer_only_high_xhigh")
        );
        assert_eq!(
            guard_entry
                .get("requestReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            guard_entry.get("finalAnswerOnly").and_then(Value::as_bool),
            Some(true)
        );
        assert!(!log
            .special_settings_json
            .as_deref()
            .unwrap_or_default()
            .contains("secret-final"));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_feature_mode_non_stream_observes_zero_final_only() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        app_settings.codex_reasoning_guard_active_template_id =
            settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-feature-mode-zero.sqlite"))
            .expect("init test db");
        let final_only_body = r#"{"id":"resp-final-only-zero","object":"response","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"secret-final-zero"}]}],"usage":{"output_tokens_details":{"reasoning_tokens":0}}}"#;
        let (upstream_base_url, upstream_task) = spawn_json_upstream(final_only_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Feature Mode Final Only Zero",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-feature","reasoning":{"effort":"high"},"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-final-only-zero"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning feature sample");
        assert_eq!(
            feature_entry.get("ruleMode").and_then(Value::as_str),
            Some("final_answer_only_high_xhigh")
        );
        assert_eq!(
            feature_entry
                .get("requestReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            feature_entry.get("reasoningTokens").and_then(Value::as_i64),
            Some(0)
        );
        assert_eq!(
            feature_entry
                .get("finalAnswerOnly")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            feature_entry.get("interceptExemptReason"),
            Some(&Value::Null)
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_feature_mode_non_stream_exempts_compaction() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        app_settings.codex_reasoning_guard_active_template_id =
            settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-feature-mode-compaction.sqlite"))
            .expect("init test db");
        let final_only_body = r#"{"id":"resp-compaction","object":"response","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"secret-compaction"}]}],"usage":{"output_tokens_details":{"reasoning_tokens":0}}}"#;
        let (upstream_base_url, upstream_task) = spawn_json_upstream(final_only_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Feature Mode Compaction",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-feature","reasoning":{"effort":"xhigh"},"request_kind":"context_compaction","input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-compaction"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning feature sample");
        assert_eq!(
            feature_entry.get("ruleMode").and_then(Value::as_str),
            Some("final_answer_only_high_xhigh")
        );
        assert_eq!(
            feature_entry.get("requestKind").and_then(Value::as_str),
            Some("context_compaction")
        );
        assert_eq!(
            feature_entry
                .get("interceptExemptReason")
                .and_then(Value::as_str),
            Some("context_compaction")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_switch_provider_uses_fresh_next_provider_budget() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-switch-provider.sqlite"))
            .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let success_body = r#"{"id":"resp-ok","object":"response","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#;
        let (guard_base_url, guard_task) = spawn_json_upstream(guard_body).await;
        let (success_base_url, success_task) = spawn_json_upstream(success_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Switch Stub", guard_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Guard Success Stub", success_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-switch","input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-ok"));

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
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_switch_provider")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(
            attempts[0]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        let special_settings: Value = serde_json::from_str(
            log.special_settings_json
                .as_deref()
                .expect("special settings json"),
        )
        .expect("special settings json parses");
        let guard_entry = special_settings
            .as_array()
            .expect("special settings array")
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
            })
            .expect("guard special setting");
        assert_eq!(
            guard_entry
                .get("guardExhaustedAction")
                .and_then(Value::as_str),
            Some("switch_provider")
        );
        assert_eq!(
            guard_entry.get("actionTaken").and_then(Value::as_str),
            Some("switch_provider_no_circuit")
        );

        guard_task.abort();
        success_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_switch_provider_all_exhausted_preserves_guard_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-all-exhausted.sqlite"))
            .expect("init test db");
        let guard_body = r#"{"id":"resp-guard","object":"response","usage":{"output_tokens_details":{"reasoning_tokens":516}},"output":[]}"#;
        let (guard_a_base_url, guard_a_hits, guard_a_task) =
            spawn_repeating_json_upstream(guard_body, 1).await;
        let (guard_b_base_url, guard_b_hits, guard_b_task) =
            spawn_repeating_json_upstream(guard_body, 1).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Exhaust A", guard_a_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Guard Exhaust B", guard_b_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-all-exhausted","input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );
        assert_eq!(
            payload.get("message").and_then(Value::as_str),
            Some("Codex reasoning guard retry budget exhausted for all attempted providers")
        );
        assert_eq!(guard_a_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(guard_b_hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert!(attempts.iter().all(|attempt| {
            attempt.get("outcome").and_then(Value::as_str)
                == Some("codex_reasoning_guard_switch_provider")
                && attempt.get("decision").and_then(Value::as_str) == Some("switch")
                && attempt.get("error_code").and_then(Value::as_str)
                    == Some("GW_CODEX_REASONING_GUARD")
                && attempt.get("circuit_failure_count").and_then(Value::as_u64) == Some(0)
        }));

        guard_a_task.abort();
        guard_b_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_include_retry_strategy_forwards_stream_body_unchanged() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-include-retry.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        insert_codex_provider_with_priority(&db, "Include Retry Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-disabled","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");
        let forwarded: Value = serde_json::from_slice(&captured.body).expect("forwarded json");
        assert_eq!(forwarded.get("include"), None);

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_include_strategy_ignores_legacy_boolean() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_continuation_repair_enabled = false;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-include-enabled.sqlite"))
            .expect("init test db");
        let (upstream_base_url, captured_rx, upstream_task) =
            spawn_capturing_raw_upstream(r#"{"id":"stub-ok","object":"response","output":[]}"#)
                .await;
        insert_codex_provider_with_priority(&db, "Include Enabled Stub", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-enabled","stream":true,"include":["foo"],"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
            .await
            .expect("captured upstream request")
            .expect("captured request");
        let forwarded: Value = serde_json::from_slice(&captured.body).expect("forwarded json");
        assert_eq!(
            forwarded.get("include"),
            Some(&serde_json::json!(["foo", "reasoning.encrypted_content"]))
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_records_unified_guard_hit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-repair-success.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cont-1\",\"status\":\"completed\",\"model\":\"gpt-cont\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-cont-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"final after continuation\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final after continuation\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cont-2\",\"status\":\"completed\",\"model\":\"gpt-cont\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Continuation Success", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont","stream":true,"input":"hello"}"#,
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
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("event: response.created"));
        assert!(body_text.contains("event: response.output_item.done"));
        assert_eq!(body_text.matches("event: response.completed").count(), 1);
        assert!(!body_text.contains("event: response.output_text.delta"));
        assert!(body_text.contains("final after continuation"));
        assert!(body_text.contains("resp-cont-2"));

        let first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let no_more = tokio::time::timeout(Duration::from_millis(100), captured_rx.recv()).await;
        assert!(
            matches!(no_more, Ok(None) | Err(_)),
            "successful continuation should send exactly one follow-up request"
        );
        let first_body: Value = serde_json::from_slice(&first.body).expect("first body json");
        assert_eq!(
            first_body.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );
        let second_body: Value = serde_json::from_slice(&second.body).expect("second body json");
        assert_eq!(
            second_body.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );
        let second_input = second_body
            .get("input")
            .and_then(Value::as_array)
            .expect("continuation input array");
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("reasoning")
                && item.get("encrypted_content").and_then(Value::as_str) == Some("enc_1")
        }));
        assert!(second_input.iter().any(|item| {
            item.get("phase").and_then(Value::as_str) == Some("commentary")
                && item.pointer("/content/0/text").and_then(Value::as_str)
                    == Some("Continue thinking...")
        }));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        assert_eq!(log.output_tokens, Some(13));
        let logged_usage: Value = serde_json::from_str(
            log.usage_json
                .as_deref()
                .expect("provider repair usage json"),
        )
        .expect("usage json");
        assert_eq!(
            logged_usage.get("output_tokens").and_then(Value::as_i64),
            Some(13)
        );
        assert_eq!(
            logged_usage
                .pointer("/output_tokens_details/reasoning_tokens")
                .and_then(Value::as_i64),
            Some(518)
        );
        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("repaired")
            })
            .expect("continuation setting");
        assert_no_bplus_continuation_fields(continuation_entry);
        assert_eq!(
            continuation_entry.get("sentRounds").and_then(Value::as_u64),
            Some(1)
        );
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("post-repair feature sample");
        assert_eq!(
            feature_entry.get("reasoningTokens").and_then(Value::as_i64),
            Some(518)
        );
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("continuation_repaired")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(1)
        }));
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

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_records_bplus_contract() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-repair-exp.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cont-exp-1\",\"status\":\"completed\",\"model\":\"gpt-cont-exp\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-cont-exp-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont-exp\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"final after experimental continuation\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final after experimental continuation\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cont-exp-2\",\"status\":\"completed\",\"model\":\"gpt-cont-exp\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        insert_codex_provider_with_priority(&db, "Continuation Experimental", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-exp","stream":true,"input":"hello"}"#,
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
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("response.output_text.delta"));
        assert!(body_text.contains("final after experimental continuation"));
        assert!(body_text.contains("resp-cont-exp-2"));

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let no_more = tokio::time::timeout(Duration::from_millis(100), captured_rx.recv()).await;
        assert!(
            matches!(no_more, Ok(None) | Err(_)),
            "successful experimental continuation should send exactly one follow-up request"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("repaired")
            })
            .expect("continuation setting");
        assert_eq!(
            continuation_entry
                .get("clientContractVersion")
                .and_then(Value::as_str),
            Some("bplus_protocol_reconstruction_v8")
        );
        assert_eq!(
            continuation_entry
                .get("reconstructionStatus")
                .and_then(Value::as_str),
            Some("final_full_passthrough")
        );
        assert_eq!(
            continuation_entry
                .get("visibleAssemblyKind")
                .and_then(Value::as_str),
            Some("empty_prior")
        );
        assert_eq!(
            continuation_entry
                .get("canonicalResponseId")
                .and_then(Value::as_str),
            Some("resp-cont-exp-2")
        );
        assert_eq!(
            continuation_entry
                .get("canonicalResponseIdContinuity")
                .and_then(Value::as_str),
            Some("final_raw_response_id_validated")
        );
        assert!(
            continuation_entry.get("repairWallClockBudget").is_none(),
            "experimental continuation logs must not expose a private wall-clock cap budget"
        );
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("none")
        );
        assert!(
            continuation_entry
                .get("timeoutSource")
                .is_some_and(Value::is_null),
            "successful continuation should not record a timeout source"
        );
        let timeout_policy = continuation_entry
            .get("timeoutPolicy")
            .and_then(Value::as_object)
            .expect("experimental timeout policy diagnostics");
        assert_eq!(
            timeout_policy.get("source").and_then(Value::as_str),
            Some("global_upstream_timeout_settings")
        );
        assert_eq!(
            timeout_policy
                .get("firstByteTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(30)
        );
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSource")
                .and_then(Value::as_str),
            Some("global")
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSource")
                .and_then(Value::as_str),
            Some("global")
        );
        assert!(timeout_policy
            .get("privateRoundTimeoutSeconds")
            .is_some_and(Value::is_null));
        assert!(timeout_policy
            .get("privateRepairWallClockCapSeconds")
            .is_some_and(Value::is_null));
        let timing = continuation_entry
            .get("timing")
            .and_then(Value::as_object)
            .expect("experimental timing diagnostics");
        assert!(timing
            .get("observedCumulativeDurationMs")
            .is_some_and(Value::is_number));
        assert!(timing
            .get("observedRepairDurationMs")
            .is_some_and(Value::is_number));
        assert_eq!(
            timing
                .get("observedRoundDurationsMs")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("round trace");
        assert_eq!(rounds.len(), 2);
        assert_eq!(
            rounds[0]
                .get("hasVisibleClientOutput")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            rounds[1]
                .get("hasVisibleClientOutput")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            continuation_entry
                .pointer("/clientUsage/output_tokens")
                .and_then(Value::as_i64),
            Some(3)
        );
        assert_eq!(
            continuation_entry
                .pointer("/clientUsage/output_tokens_details/reasoning_tokens")
                .and_then(Value::as_i64),
            Some(2)
        );
        assert_eq!(
            continuation_entry
                .pointer("/providerRepairUsage/output_tokens")
                .and_then(Value::as_i64),
            Some(13)
        );
        assert_eq!(
            continuation_entry
                .pointer("/providerRepairUsage/output_tokens_details/reasoning_tokens")
                .and_then(Value::as_i64),
            Some(518)
        );
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair_experimental")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("continuation_repaired")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(1)
        }));

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_folded_preserves_distinct_visible_text() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-distinct-folded.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_early\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"early visible answer\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-folded-distinct-1\",\"status\":\"completed\",\"model\":\"gpt-cont-folded\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-folded-distinct-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont-folded\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"different final answer\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-folded-distinct-2\",\"status\":\"completed\",\"model\":\"gpt-cont-folded\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Distinct Folded",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-folded","stream":true,"input":"hello"}"#,
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
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("early visible answer"));
        assert!(body_text.contains("different final answer"));

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let no_more = tokio::time::timeout(Duration::from_millis(100), captured_rx.recv()).await;
        assert!(
            matches!(no_more, Ok(None) | Err(_)),
            "stable folded continuation should send exactly one follow-up request"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("repaired")
            })
            .expect("continuation setting");
        assert_no_bplus_continuation_fields(continuation_entry);
        assert_eq!(
            continuation_entry.get("sentRounds").and_then(Value::as_u64),
            Some(1)
        );
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("continuation_repaired")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(1)
        }));
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

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_distinct_visible_rounds_fail_closed_without_exposing_partial_text(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-distinct-unsafe.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_early\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"early visible answer\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-distinct-1\",\"status\":\"completed\",\"model\":\"gpt-cont\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-distinct-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"different final answer\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-distinct-2\",\"status\":\"completed\",\"model\":\"gpt-cont\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Distinct Unsafe",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(!body_text.contains("early visible answer"));
        assert!(!body_text.contains("different final answer"));
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("clientContractVersion")
                .and_then(Value::as_str),
            Some("bplus_protocol_reconstruction_v8")
        );
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("bplus_reconstruction")
        );
        assert!(continuation_entry
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("non-prefix")));
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        assert!(
            continuation_entry
                .get("timeoutSource")
                .is_some_and(Value::is_null),
            "non-timeout failure should not record a timeout source"
        );
        let timeout_policy = continuation_entry
            .get("timeoutPolicy")
            .and_then(Value::as_object)
            .expect("experimental timeout policy diagnostics");
        assert_eq!(
            timeout_policy
                .get("firstByteTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(30)
        );
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("sanitized failure round trace");
        assert_eq!(rounds.len(), 2);
        assert_eq!(
            rounds[0].get("visibleTextLen").and_then(Value::as_u64),
            Some("early visible answer".len() as u64)
        );
        assert!(rounds[0].get("visibleTextHash").is_some());
        assert_eq!(
            rounds[1].get("visibleTextLen").and_then(Value::as_u64),
            Some("different final answer".len() as u64)
        );
        assert!(continuation_entry
            .get("aggregateRawBytes")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0));
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair_experimental")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str) == Some("failed")
        }));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_tool_call_failure_fails_closed_without_raw_leak(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-tool-failed.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_tool_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_tool_secret\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_partial\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"partial answer before tool failure\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-tool-1\",\"status\":\"completed\",\"model\":\"gpt-cont-tool\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-tool-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont-tool\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"call_tool_1\",\"delta\":\"{\\\"message\\\":\\\"Tool call failed\\\",\\\"secret\\\":\\\"raw-tool-arg-42\\\"}\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"call_tool_1\",\"type\":\"function_call\",\"name\":\"lookup\",\"call_id\":\"call_tool_1\",\"arguments\":\"{\\\"message\\\":\\\"Tool call failed\\\",\\\"secret\\\":\\\"raw-tool-arg-42\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-tool-2\",\"status\":\"completed\",\"model\":\"gpt-cont-tool\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Tool Failure",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-tool","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings_raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        for forbidden in [
            "Tool call failed",
            "partial answer before tool failure",
            "raw-tool-arg-42",
            "enc_tool_secret",
            "encrypted_content",
            "event: response",
            "response.function_call_arguments.delta",
            "data: {",
        ] {
            assert!(
                !body_text.contains(forbidden),
                "client response leaked forbidden fragment {forbidden}: {body_text}"
            );
            assert!(
                !special_settings_raw.contains(forbidden),
                "special settings leaked forbidden fragment {forbidden}: {special_settings_raw}"
            );
        }

        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("bplus_reconstruction")
        );
        assert!(continuation_entry
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("tool/function call")));
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        let timeout_policy = continuation_entry
            .get("timeoutPolicy")
            .and_then(Value::as_object)
            .expect("experimental timeout policy diagnostics");
        assert_eq!(
            timeout_policy
                .get("firstByteTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(30)
        );
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(300)
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("sanitized failure round trace");
        assert_eq!(rounds.len(), 2);
        assert_eq!(
            rounds[0].get("visibleTextLen").and_then(Value::as_u64),
            Some("partial answer before tool failure".len() as u64)
        );
        assert!(rounds[0].get("visibleTextHash").is_some());
        assert_eq!(
            rounds[1].get("hasToolCall").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            rounds[1].get("visibleTextLen").and_then(Value::as_u64),
            Some(0)
        );
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair_experimental")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str) == Some("failed")
        }));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_aggregation_failure_sanitizes_raw_error_message(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-aggregate-error.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_agg_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_agg_secret\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-agg-1\",\"status\":\"completed\",\"model\":\"gpt-cont-agg\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp-agg-2\",\"error\":{\"message\":\"Tool call failed raw-agg-secret-42\"}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Aggregate Error",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-agg","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings_raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        for forbidden in [
            "Tool call failed",
            "raw-agg-secret-42",
            "enc_agg_secret",
            "encrypted_content",
            "response.failed",
            "event: response",
            "data: {",
        ] {
            assert!(
                !body_text.contains(forbidden),
                "client response leaked forbidden fragment {forbidden}: {body_text}"
            );
            assert!(
                !special_settings_raw.contains(forbidden),
                "special settings leaked forbidden fragment {forbidden}: {special_settings_raw}"
            );
        }

        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("aggregate")
        );
        assert_eq!(
            continuation_entry.get("reason").and_then(Value::as_str),
            Some("failed_to_aggregate_continuation_event_stream")
        );
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        assert_eq!(
            continuation_entry.get("sentRounds").and_then(Value::as_u64),
            Some(0)
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("sanitized failure round trace");
        assert_eq!(rounds.len(), 1);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_unknown_raw_event_sanitizes_event_name() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-unknown-raw-event.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_unknown_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_unknown_secret\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-unknown-1\",\"status\":\"completed\",\"model\":\"gpt-cont-unknown\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-unknown-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont-unknown\"}}\n\n",
            "event: response.secret_route_event_name_abc.delta\n",
            "data: {\"type\":\"response.secret_route_event_name_abc.delta\",\"delta\":\"raw-route-unknown-event-secret-42\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_unknown_final\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final text should not leak after unknown raw event\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-unknown-2\",\"status\":\"completed\",\"model\":\"gpt-cont-unknown\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Unknown Raw Event",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-unknown","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings_raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        for forbidden in [
            "response.secret_route_event_name_abc.delta",
            "raw-route-unknown-event-secret-42",
            "final text should not leak after unknown raw event",
            "enc_unknown_secret",
            "encrypted_content",
            "event: response",
            "data: {",
        ] {
            assert!(
                !body_text.contains(forbidden),
                "client response leaked forbidden fragment {forbidden}: {body_text}"
            );
            assert!(
                !special_settings_raw.contains(forbidden),
                "special settings leaked forbidden fragment {forbidden}: {special_settings_raw}"
            );
        }

        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("bplus_reconstruction")
        );
        assert!(continuation_entry
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("unknown pre-completed event")));
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_keepalive_only_followup_hits_terminal_event_timeout(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-keepalive-timeout.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_keepalive_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_keepalive_secret\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-keepalive-1\",\"status\":\"completed\",\"model\":\"gpt-cont-keepalive\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_then_keepalive_capturing_sse_upstream(
                first_sse,
                Duration::from_millis(100),
            )
            .await;
        let provider_id = insert_codex_provider_with_stream_idle_timeout(
            &db,
            "Continuation Keepalive Timeout",
            upstream_base_url,
            0,
            1,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-keepalive","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(3), router.oneshot(request))
            .await
            .expect("continuation should fail closed before outer timeout")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings_raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        for forbidden in [
            "enc_keepalive_secret",
            "encrypted_content",
            ": keepalive",
            "event: response",
            "data: {",
        ] {
            assert!(
                !body_text.contains(forbidden),
                "client response leaked forbidden fragment {forbidden}: {body_text}"
            );
            assert!(
                !special_settings_raw.contains(forbidden),
                "special settings leaked forbidden fragment {forbidden}: {special_settings_raw}"
            );
        }

        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("upstream_stream")
        );
        assert!(continuation_entry
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("terminal event timeout")));
        assert_eq!(
            continuation_entry
                .get("timeoutSource")
                .and_then(Value::as_str),
            Some("terminal_event")
        );
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        assert_eq!(
            continuation_entry.get("sentRounds").and_then(Value::as_u64),
            Some(0)
        );
        let timeout_policy = continuation_entry
            .get("timeoutPolicy")
            .and_then(Value::as_object)
            .expect("experimental timeout policy diagnostics");
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSource")
                .and_then(Value::as_str),
            Some("provider")
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSeconds")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSource")
                .and_then(Value::as_str),
            Some("provider")
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("sanitized failure round trace");
        assert_eq!(rounds.len(), 1);
        assert_eq!(
            rounds[0].get("hasReasoning").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            rounds[0].get("visibleTextLen").and_then(Value::as_u64),
            Some(0)
        );
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_repair_experimental_disabled_stream_timeout_fails_closed() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.upstream_stream_idle_timeout_seconds = 0;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        app_settings.codex_reasoning_guard_post_match_strategy =
            settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-disabled-timeout.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_disabled_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_disabled_secret\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-disabled-1\",\"status\":\"completed\",\"model\":\"gpt-cont-disabled\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-disabled-2\",\"status\":\"in_progress\",\"model\":\"gpt-cont-disabled\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_disabled\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final text should not leak with disabled timeout\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-disabled-2\",\"status\":\"completed\",\"model\":\"gpt-cont-disabled\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Disabled Timeout",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-disabled","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = tokio::time::timeout(Duration::from_secs(2), router.oneshot(request))
            .await
            .expect("continuation should fail closed before outer timeout")
            .expect("route response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings_raw = log
            .special_settings_json
            .as_deref()
            .expect("special settings json");
        for forbidden in [
            "enc_disabled_secret",
            "encrypted_content",
            "final text should not leak",
            "event: response",
            "data: {",
        ] {
            assert!(
                !body_text.contains(forbidden),
                "client response leaked forbidden fragment {forbidden}: {body_text}"
            );
            assert!(
                !special_settings_raw.contains(forbidden),
                "special settings leaked forbidden fragment {forbidden}: {special_settings_raw}"
            );
        }

        let special_settings = parse_special_settings(&log);
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                    && entry.get("status").and_then(Value::as_str) == Some("failed")
            })
            .expect("failed continuation setting");
        assert_eq!(
            continuation_entry
                .get("failureKind")
                .and_then(Value::as_str),
            Some("upstream_stream")
        );
        assert!(continuation_entry
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("terminal event timeout disabled")));
        assert_eq!(
            continuation_entry
                .get("timeoutSource")
                .and_then(Value::as_str),
            Some("terminal_event_disabled")
        );
        let timeout_policy = continuation_entry
            .get("timeoutPolicy")
            .and_then(Value::as_object)
            .expect("experimental timeout policy diagnostics");
        assert!(timeout_policy
            .get("streamIdleTimeoutSeconds")
            .is_some_and(Value::is_null));
        assert_eq!(
            timeout_policy
                .get("streamIdleTimeoutSource")
                .and_then(Value::as_str),
            Some("global")
        );
        assert!(timeout_policy
            .get("terminalEventTimeoutSeconds")
            .is_some_and(Value::is_null));
        assert_eq!(
            timeout_policy
                .get("terminalEventTimeoutSource")
                .and_then(Value::as_str),
            Some("global")
        );
        assert_eq!(
            continuation_entry
                .get("fallbackAction")
                .and_then(Value::as_str),
            Some("existing_guard_failover_or_exhausted")
        );
        assert_eq!(
            continuation_entry.get("sentRounds").and_then(Value::as_u64),
            Some(0)
        );
        let rounds = continuation_entry
            .get("rounds")
            .and_then(Value::as_array)
            .expect("sanitized failure round trace");
        assert_eq!(rounds.len(), 1);
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_uses_immediate_budget_not_legacy_max_rounds() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 2;
        app_settings.codex_reasoning_guard_continuation_max_rounds = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-immediate-budget.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-budget-1\",\"status\":\"completed\",\"model\":\"gpt-cont-budget\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_2\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_2\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-budget-2\",\"status\":\"completed\",\"model\":\"gpt-cont-budget\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":1034}}}}\n\n"
        );
        let third_sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-budget-3\",\"status\":\"in_progress\",\"model\":\"gpt-cont-budget\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final after second continuation\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-budget-3\",\"status\":\"completed\",\"model\":\"gpt-cont-budget\",\"usage\":{\"output_tokens\":3,\"output_tokens_details\":{\"reasoning_tokens\":2}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse, third_sse]).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Continuation Budget", upstream_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-budget","stream":true,"input":"hello"}"#,
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
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("final after second continuation"));
        assert!(body_text.contains("resp-budget-3"));

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let _third = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("third captured request")
            .expect("third request");
        let no_more = tokio::time::timeout(Duration::from_millis(100), captured_rx.recv()).await;
        assert!(
            matches!(no_more, Ok(None) | Err(_)),
            "immediate retry budget should allow exactly two continuation follow-ups"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                && entry.get("status").and_then(Value::as_str) == Some("repaired")
                && entry.get("sentRounds").and_then(Value::as_u64) == Some(2)
        }));
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("continuation_repaired")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(2)
        }));
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

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_output_cap_stops_still_matched_chain_as_one_guard_hit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_enabled = true;
        app_settings.codex_reasoning_guard_continuation_max_output_tokens = 12;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 3;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-output-cap.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cap-1\",\"status\":\"completed\",\"model\":\"gpt-cont-cap\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_2\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_2\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cap-2\",\"status\":\"completed\",\"model\":\"gpt-cont-cap\",\"usage\":{\"output_tokens\":5,\"output_tokens_details\":{\"reasoning_tokens\":1034}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Output Cap",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-cap","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let _first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let _second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let no_more = tokio::time::timeout(Duration::from_millis(100), captured_rx.recv()).await;
        assert!(
            matches!(no_more, Ok(None) | Err(_)),
            "output-cap fallback should stop before sending another follow-up"
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                && entry.get("status").and_then(Value::as_str) == Some("capped_max_output_tokens")
                && entry.get("failureKind").and_then(Value::as_str)
                    == Some("capped_max_output_tokens")
                && entry.get("sentRounds").and_then(Value::as_u64) == Some(1)
        }));
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
            })
            .expect("continuation setting");
        assert_no_bplus_continuation_fields(continuation_entry);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("capped_max_output_tokens")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(1)
        }));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_missing_encrypted_records_failure_kind_and_one_guard_hit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-missing-encrypted.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-cont-missing\",\"status\":\"completed\",\"model\":\"gpt-cont-missing\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let (upstream_base_url, upstream_task) = spawn_sse_upstream(sse_body).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Missing Encrypted",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-missing","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                && entry.get("status").and_then(Value::as_str) == Some("missing_encrypted")
                && entry.get("failureKind").and_then(Value::as_str) == Some("missing_encrypted")
                && entry.get("sentRounds").and_then(Value::as_u64) == Some(0)
        }));
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
            })
            .expect("continuation setting");
        assert_no_bplus_continuation_fields(continuation_entry);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("missing_encrypted")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(0)
        }));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_continuation_still_matched_uses_one_guard_hit() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_enabled = true;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 1;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-cont-still-matched.sqlite"))
            .expect("init test db");
        let first_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_1\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-still-1\",\"status\":\"completed\",\"model\":\"gpt-cont-still\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let second_sse = concat!(
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"rs_2\",\"type\":\"reasoning\",\"encrypted_content\":\"enc_2\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-still-2\",\"status\":\"completed\",\"model\":\"gpt-cont-still\",\"usage\":{\"output_tokens\":10,\"output_tokens_details\":{\"reasoning_tokens\":1034}}}}\n\n"
        );
        let (upstream_base_url, mut captured_rx, upstream_task) =
            spawn_sequence_capturing_sse_upstream(vec![first_sse, second_sse]).await;
        let provider_id = insert_codex_provider_with_priority(
            &db,
            "Continuation Still Matched",
            upstream_base_url,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-cont-still","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let first = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("first captured request")
            .expect("first request");
        let second = tokio::time::timeout(Duration::from_secs(2), captured_rx.recv())
            .await
            .expect("second captured request")
            .expect("second request");
        let first_body: Value = serde_json::from_slice(&first.body).expect("first body json");
        let second_body: Value = serde_json::from_slice(&second.body).expect("second body json");
        assert_eq!(
            first_body.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );
        assert_eq!(
            second_body.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
                && entry.get("status").and_then(Value::as_str) == Some("still_matched")
                && entry.get("failureKind").and_then(Value::as_str) == Some("still_matched")
                && entry.get("sentRounds").and_then(Value::as_u64) == Some(1)
        }));
        let continuation_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_continuation")
            })
            .expect("continuation setting");
        assert_no_bplus_continuation_fields(continuation_entry);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("guardPostMatchStrategy").and_then(Value::as_str)
                    == Some("continuation_repair")
                && entry.get("guardStrategyOutcome").and_then(Value::as_str)
                    == Some("still_matched")
                && entry.get("continuationSentRounds").and_then(Value::as_u64) == Some(1)
        }));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        upstream_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_stream_normal_response_returns_full_body() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-stream-default-pass.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-stream-pass\",\"status\":\"in_progress\",\"model\":\"gpt-stream-pass\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"stream-full-body-one\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"stream-full-body-two\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-stream-pass\",\"status\":\"completed\",\"model\":\"gpt-stream-pass\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"stream-full-body-one stream-full-body-two\"}]}],\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":2048}}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Guard Stream Default Pass", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-stream-pass","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("resp-stream-pass"));
        assert!(body_text.contains("stream-full-body-one"));
        assert!(body_text.contains("stream-full-body-two"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
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
    async fn codex_reasoning_guard_bridge_stream_bypasses_native_guard_buffering() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        app_settings.codex_reasoning_guard_active_template_id =
            settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-bridge-stream.sqlite"))
            .expect("init test db");
        let chat_sse_body = concat!(
            "data: {\"id\":\"chatcmpl-bridge-stream\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-bridge\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"bridge-stream-one \"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-bridge-stream\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-bridge\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"bridge-stream-two\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-bridge-stream\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-bridge\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n"
        );
        let (source_base_url, source_task) = spawn_sse_upstream(chat_sse_body).await;
        let source_provider_id =
            insert_codex_provider_with_priority(&db, "Bridge Stream Source", source_base_url, 0);
        let provider_id = insert_codex_bridge_provider(
            &db,
            providers::CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
            source_provider_id,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-bridge","stream":true,"reasoning_effort":"high","input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("response.output_text.delta"));
        assert!(body_text.contains("bridge-stream-one"));
        assert!(body_text.contains("bridge-stream-two"));
        assert!(body_text.contains("response.completed"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
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
        assert!(!attempts.iter().any(|attempt| {
            attempt
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|outcome| outcome.starts_with("codex_reasoning_guard"))
        }));

        source_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_bridge_non_stream_bypasses_active_guard() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        app_settings.codex_reasoning_guard_active_template_id =
            settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-bridge-non-stream.sqlite"))
            .expect("init test db");
        let chat_body = r#"{"id":"chatcmpl-bridge-non-stream","object":"chat.completion","model":"gpt-bridge","choices":[{"index":0,"message":{"role":"assistant","content":"bridge non-stream final answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":5}}"#;
        let (source_base_url, source_task) = spawn_json_upstream(chat_body).await;
        let source_provider_id = insert_codex_provider_with_priority(
            &db,
            "Bridge Non Stream Source",
            source_base_url,
            0,
        );
        let provider_id = insert_codex_bridge_provider(
            &db,
            providers::CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
            source_provider_id,
            0,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/codex/_aio/provider/{provider_id}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-bridge","stream":false,"reasoning_effort":"high","input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("chatcmpl-bridge-non-stream"));
        assert!(body_text.contains("bridge non-stream final answer"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
        }));
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
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
        assert!(!attempts.iter().any(|attempt| {
            attempt
                .get("outcome")
                .and_then(Value::as_str)
                .is_some_and(|outcome| outcome.starts_with("codex_reasoning_guard"))
        }));

        source_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_stream_exhausts_budget_with_terminal_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-stream-return.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-guard-stream\",\"status\":\"completed\",\"model\":\"gpt-guard-stream\",\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Guard Stream Stub", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-stream","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );
        assert_eq!(
            attempts[0]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(0)
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_v1_codex_responses_stream_exhausts_budget_with_terminal_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-guard-v1-codex-stream-return.sqlite"),
        )
        .expect("init test db");
        let sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-v1-codex-guard-stream\",\"status\":\"completed\",\"model\":\"gpt-v1-codex-guard-stream\",\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "V1 Codex Guard Stream", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/codex/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-v1-codex-guard-stream","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_chat_stream_exhausts_budget_with_terminal_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::ReturnError;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-chat-stream-return.sqlite"))
            .expect("init test db");
        let sse_body = concat!(
            "data: {\"id\":\"chatcmpl-guard-stream\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-chat-guard-stream\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-guard-stream\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-chat-guard-stream\",\"choices\":[],\"usage\":{\"completion_tokens\":516,\"completion_tokens_details\":{\"reasoning_tokens\":516}}}\n\n",
            "data: [DONE]\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Chat Guard Stream", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-chat-guard-stream","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        assert_eq!(log.path, "/v1/chat/completions");
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_id)
        );
        assert_eq!(
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_exhausted")
        );
        let special_settings = parse_special_settings(&log);
        assert!(special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
                && entry.get("reasoningTokens").and_then(Value::as_i64) == Some(516)
        }));

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_stream_switches_provider_after_exhaustion() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-guard-stream-switch.sqlite"))
            .expect("init test db");
        let guard_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-guard-stream-switch\",\"status\":\"completed\",\"model\":\"gpt-guard-stream-switch\",\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let success_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-ok-stream-switch\",\"status\":\"completed\",\"model\":\"gpt-guard-stream-switch\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}]}}\n\n"
        );
        let (guard_sse_base_url, guard_task) = spawn_sse_upstream(guard_sse_body).await;
        let (success_sse_base_url, success_task) = spawn_sse_upstream(success_sse_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Stream Switch", guard_sse_base_url, 0);
        let provider_b = insert_codex_provider_with_priority(
            &db,
            "Guard Stream Success",
            success_sse_base_url,
            1,
        );

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-stream-switch","stream":true,"input":"hello"}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("resp-ok-stream-switch"));

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
            attempts[0].get("outcome").and_then(Value::as_str),
            Some("codex_reasoning_guard_switch_provider")
        );
        assert_eq!(
            attempts[0]
                .get("circuit_failure_count")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert_eq!(
            attempts[1].get("outcome").and_then(Value::as_str),
            Some("success")
        );

        guard_task.abort();
        success_task.abort();
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
        app_settings.codex_reasoning_guard_enabled = false;
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
        app_settings.codex_reasoning_guard_enabled = false;
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
    async fn codex_responses_sse_fake_200_keeps_fake_200_error_code() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_enabled = false;
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("codex-responses-sse-fake-200.sqlite"))
            .expect("init test db");
        let fake_200_body = concat!(
            "event: response.error\n",
            "data: {\"type\":\"response.error\",\"error\":{\"message\":\"quota exhausted\",\"type\":\"insufficient_quota\"},\"usage\":{\"input_tokens\":11,\"output_tokens\":0,\"total_tokens\":11}}\n\n"
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
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("error_code").and_then(Value::as_str),
            Some("GW_FAKE_200")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_FAKE_200"));
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
            Some("GW_FAKE_200")
        );
        assert_eq!(
            attempts[0].get("decision").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(circuit.snapshot(provider_id, 0).failure_count, 1);
        assert_eq!(session.get_bound_provider("codex", session_id, 0), None);

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
        app_settings.codex_reasoning_guard_enabled = false;
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
        assert!(payload_error_code.is_some());
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
            Some("switch")
        );
        assert_eq!(attempt.get("circuit_failure_count"), Some(&Value::Null));
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
        app_settings.codex_reasoning_guard_enabled = false;
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
        app_settings.codex_reasoning_guard_enabled = false;
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

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_disabled_unbuffered_stream_compaction_emits_request_only_features(
    ) {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 1;
        app_settings.codex_reasoning_guard_enabled = false;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-disabled-unbuffered-compaction.sqlite"),
        )
        .expect("init test db");
        let sse_body = concat!(
            "data: {\"id\":\"chatcmpl-compaction\",\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (sse_base_url, sse_task) = spawn_sse_upstream(sse_body).await;
        let provider_id =
            insert_codex_provider_with_priority(&db, "Disabled Unbuffered Stream", sse_base_url, 0);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/codex/_aio/provider/{provider_id}/v1/chat/completions"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-compaction-stream","stream":true,"request_kind":"context_compaction","messages":[]}"#,
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert!(String::from_utf8_lossy(&body).contains("chatcmpl-compaction"));

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code, None);
        let special_settings = parse_special_settings(&log);
        assert!(!special_settings.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_guard")
        }));
        let feature_entry = special_settings
            .iter()
            .find(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("codex_reasoning_features")
            })
            .expect("codex reasoning request-only feature sample");
        assert_eq!(
            feature_entry
                .get("responseClassification")
                .and_then(Value::as_str),
            Some("request_only")
        );
        assert_eq!(
            feature_entry
                .get("classificationSkippedReason")
                .and_then(Value::as_str),
            Some("guard_disabled_stream_not_buffered")
        );
        assert_eq!(
            feature_entry.get("requestKind").and_then(Value::as_str),
            Some("context_compaction")
        );
        // request-only samples cannot confirm reasoning_tokens == 0, so they are
        // never marked exempt; only the request kind is recorded.
        assert_eq!(
            feature_entry.get("interceptExemptReason"),
            Some(&Value::Null)
        );
        assert_eq!(feature_entry.get("hasFinalAnswer"), Some(&Value::Null));

        sse_task.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_reasoning_guard_stream_switch_provider_all_exhausted_preserves_guard_error() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home dir");
        let _env = isolate_app_env(home.path());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let mut app_settings = settings::AppSettings::default();
        app_settings.failover_max_attempts_per_provider = 1;
        app_settings.failover_max_providers_to_try = 2;
        app_settings.codex_reasoning_guard_immediate_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_budget = 0;
        app_settings.codex_reasoning_guard_delayed_retry_ms = 0;
        app_settings.codex_reasoning_guard_exhausted_action =
            settings::CodexReasoningGuardExhaustedAction::SwitchProvider;
        disable_upstream_retry_policy(&mut app_settings);
        settings::write(&app_handle, &app_settings).expect("write settings");
        crate::cli_proxy::set_enabled(&app_handle, "codex", true, "http://127.0.0.1:37123")
            .expect("enable codex cli proxy");

        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(
            &db_dir
                .path()
                .join("codex-guard-stream-all-exhausted.sqlite"),
        )
        .expect("init test db");
        let guard_sse_body = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-guard-stream-all-exhausted\",\"status\":\"completed\",\"model\":\"gpt-guard-stream-all-exhausted\",\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n"
        );
        let (guard_a_base_url, guard_a_task) = spawn_sse_upstream(guard_sse_body).await;
        let (guard_b_base_url, guard_b_task) = spawn_sse_upstream(guard_sse_body).await;
        let provider_a =
            insert_codex_provider_with_priority(&db, "Guard Stream Exhaust A", guard_a_base_url, 0);
        let provider_b =
            insert_codex_provider_with_priority(&db, "Guard Stream Exhaust B", guard_b_base_url, 1);

        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(8);
        let router = build_router(gateway_state(app_handle, db, log_tx));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/codex/v1/responses")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"model":"gpt-guard-stream-all-exhausted","stream":true,"input":"hello"}"#,
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
            Some("GW_CODEX_REASONING_GUARD")
        );
        assert_eq!(
            payload.get("message").and_then(Value::as_str),
            Some("Codex reasoning guard retry budget exhausted for all attempted providers")
        );

        let log = recv_terminal_request_log(&mut log_rx).await;
        assert_eq!(log.status, Some(502));
        assert_eq!(log.error_code.as_deref(), Some("GW_CODEX_REASONING_GUARD"));
        let attempts: Value = serde_json::from_str(&log.attempts_json).expect("attempts json");
        let attempts = attempts.as_array().expect("attempt array");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0].get("provider_id").and_then(Value::as_i64),
            Some(provider_a)
        );
        assert_eq!(
            attempts[1].get("provider_id").and_then(Value::as_i64),
            Some(provider_b)
        );
        assert!(attempts.iter().all(|attempt| {
            attempt.get("outcome").and_then(Value::as_str)
                == Some("codex_reasoning_guard_switch_provider")
                && attempt.get("decision").and_then(Value::as_str) == Some("switch")
                && attempt.get("error_code").and_then(Value::as_str)
                    == Some("GW_CODEX_REASONING_GUARD")
                && attempt.get("circuit_failure_count").and_then(Value::as_u64) == Some(0)
        }));

        guard_a_task.abort();
        guard_b_task.abort();
    }
}
