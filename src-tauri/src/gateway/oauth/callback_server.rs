//! Usage: One-shot localhost HTTP listener for OAuth authorization code callbacks.

use crate::shared::security::constant_time_eq;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener as TokioTcpListener;

#[derive(Debug, Clone)]
pub(crate) struct OAuthCallbackPayload {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub(crate) struct BoundOAuthCallbackListener {
    listener: TokioTcpListener,
    pub port: u16,
}

pub(crate) async fn bind_callback_listener(
    preferred_port: u16,
) -> Result<BoundOAuthCallbackListener, String> {
    // Try preferred port first
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, preferred_port));
    if let Ok(std_listener) = TcpListener::bind(addr) {
        std_listener
            .set_nonblocking(true)
            .map_err(|e| format!("failed to set nonblocking: {e}"))?;
        let listener = TokioTcpListener::from_std(std_listener)
            .map_err(|e| format!("failed to create tokio listener: {e}"))?;
        return Ok(BoundOAuthCallbackListener {
            listener,
            port: preferred_port,
        });
    }

    // Fall back to dynamic port
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0u16));
    let std_listener =
        TcpListener::bind(addr).map_err(|e| format!("failed to bind callback listener: {e}"))?;
    let port = std_listener
        .local_addr()
        .map_err(|e| format!("failed to get local addr: {e}"))?
        .port();
    std_listener
        .set_nonblocking(true)
        .map_err(|e| format!("failed to set nonblocking: {e}"))?;
    let listener = TokioTcpListener::from_std(std_listener)
        .map_err(|e| format!("failed to create tokio listener: {e}"))?;

    tracing::info!(
        preferred_port,
        actual_port = port,
        "callback listener: preferred port unavailable, using dynamic port"
    );

    Ok(BoundOAuthCallbackListener { listener, port })
}

impl BoundOAuthCallbackListener {
    pub(crate) async fn wait_for_callback(
        self,
        expected_state: &str,
        timeout_secs: u64,
    ) -> Result<OAuthCallbackPayload, String> {
        let deadline = std::time::Duration::from_secs(timeout_secs);

        // Loop accepting connections until we get a valid OAuth callback with matching state.
        // Browsers may send favicon.ico, preflight, or other requests before/after the real
        // callback — we must ignore those instead of consuming the only accept slot.
        tokio::time::timeout(deadline, self.accept_matching(expected_state))
            .await
            .map_err(|_| "OAuth callback timeout: no response received".to_string())?
    }

    async fn accept_matching(self, expected_state: &str) -> Result<OAuthCallbackPayload, String> {
        loop {
            let (mut stream, _addr) = self
                .listener
                .accept()
                .await
                .map_err(|e| format!("failed to accept callback connection: {e}"))?;

            let payload = Self::read_and_parse(&mut stream).await;

            match payload {
                Ok(ref p) if p.code.is_some() => {
                    // Looks like a real OAuth callback — validate state.
                    let state_ok = match p.state.as_deref() {
                        Some(s) => constant_time_eq(s.as_bytes(), expected_state.as_bytes()),
                        None => false,
                    };

                    if state_ok {
                        // Check for OAuth error in the callback itself.
                        if p.error.is_some() {
                            let err = p.error.as_deref().unwrap_or("unknown");
                            let desc = p.error_description.as_deref().unwrap_or("");
                            Self::send_response(
                                &mut stream,
                                "400 Bad Request",
                                "Authentication failed. You can close this tab.",
                            )
                            .await;
                            return Err(format!("OAuth callback error: {err}: {desc}"));
                        }
                        Self::send_response(
                            &mut stream,
                            "200 OK",
                            "Authentication successful! You can close this tab.",
                        )
                        .await;
                        return Ok(p.clone());
                    }

                    // State mismatch — could be a stale callback from a prior flow.
                    tracing::warn!(
                        "oauth callback ignored invalid request while waiting for matching state \
                         reason=OAuth state mismatch: possible CSRF attack"
                    );
                    Self::send_response(
                        &mut stream,
                        "400 Bad Request",
                        "State mismatch. Please retry login from the app.",
                    )
                    .await;
                    // Continue looping for the correct callback.
                }
                Ok(ref p) if p.error.is_some() => {
                    let err = p.error.as_deref().unwrap_or("unknown");
                    let desc = p.error_description.as_deref().unwrap_or("");
                    Self::send_response(
                        &mut stream,
                        "400 Bad Request",
                        "Authentication failed. You can close this tab.",
                    )
                    .await;
                    return Err(format!("OAuth callback error: {err}: {desc}"));
                }
                _ => {
                    // Non-OAuth request (favicon.ico, preflight, empty, etc.) — ignore and loop.
                    tracing::debug!("oauth callback listener: ignoring non-OAuth request");
                    Self::send_response(&mut stream, "404 Not Found", "").await;
                }
            }
        }
    }

    async fn read_and_parse(
        stream: &mut tokio::net::TcpStream,
    ) -> Result<OAuthCallbackPayload, String> {
        // Read enough to cover the GET request line + headers.
        // Use a larger buffer and loop until we see the end-of-headers marker or hit the limit.
        let mut buf = Vec::with_capacity(16384);
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream
                .read(&mut tmp)
                .await
                .map_err(|e| format!("failed to read callback request: {e}"))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            // Stop once we have the full headers (double CRLF) or hit size limit.
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 16384 {
                break;
            }
        }

        let request = String::from_utf8_lossy(&buf);
        Ok(parse_callback_request(&request))
    }

    async fn send_response(stream: &mut tokio::net::TcpStream, status: &str, message: &str) {
        let body = if message.is_empty() {
            String::new()
        } else {
            format!(
                "<html><body><h1>{}</h1><p>{message}</p></body></html>",
                if status.starts_with('2') {
                    "Success"
                } else {
                    "Error"
                }
            )
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    }
}

fn parse_callback_request(request: &str) -> OAuthCallbackPayload {
    let mut payload = OAuthCallbackPayload {
        code: None,
        state: None,
        error: None,
        error_description: None,
    };

    // Parse GET request line: "GET /callback?code=xxx&state=yyy HTTP/1.1"
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");

    if let Some(query) = path.split_once('?').map(|(_, q)| q) {
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                let decoded = crate::gateway::util::url_decode_component(value);
                match key {
                    "code" => payload.code = Some(decoded),
                    "state" => payload.state = Some(decoded),
                    "error" => payload.error = Some(decoded),
                    "error_description" => payload.error_description = Some(decoded),
                    _ => {}
                }
            }
        }
    }

    payload
}
