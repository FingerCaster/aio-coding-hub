//! Usage: OAuth token exchange (authorization_code grant) and refresh (refresh_token grant).

use super::provider_trait::OAuthTokenSet;
use crate::shared::http_body::read_text_with_limit;
use crate::shared::time::now_unix_seconds;

const OAUTH_TOKEN_RESPONSE_BODY_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy)]
enum TokenResponseContext {
    AuthorizationCode,
    RefreshToken,
}

impl TokenResponseContext {
    fn invalid_grant_error(self) -> &'static str {
        match self {
            Self::AuthorizationCode => {
                "AUTHORIZATION_CODE_INVALID: authorization code is invalid or expired"
            }
            Self::RefreshToken => "AUTH_RELOGIN_REQUIRED: refresh token is invalid or expired",
        }
    }
}

fn safe_token_request_error(operation: &str, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("{operation} request timed out")
    } else {
        format!("{operation} request failed")
    }
}

pub(crate) struct TokenExchangeRequest {
    pub token_uri: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub code: String,
    pub redirect_uri: String,
    pub code_verifier: String,
    pub state: Option<String>,
}

pub(crate) struct TokenRefreshRequest {
    pub token_uri: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: String,
}

pub(crate) async fn exchange_authorization_code(
    client: &reqwest::Client,
    req: &TokenExchangeRequest,
) -> Result<OAuthTokenSet, String> {
    tracing::info!("exchanging authorization code for tokens");

    // Anthropic requires JSON body, others use form-encoded
    let is_anthropic = is_anthropic_oauth_token_uri(&req.token_uri);

    let resp = if is_anthropic {
        let missing_state = req
            .state
            .as_ref()
            .map(|state| state.trim().is_empty())
            .unwrap_or(true);
        if missing_state {
            return Err(
                "SEC_INVALID_INPUT: Anthropic token exchange requires non-empty OAuth state"
                    .to_string(),
            );
        }

        let body = build_anthropic_exchange_json(req);

        client
            .post(&req.token_uri)
            .json(&body)
            .send()
            .await
            .map_err(|error| safe_token_request_error("token exchange", &error))?
    } else {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", &req.code),
            ("redirect_uri", &req.redirect_uri),
            ("client_id", &req.client_id),
            ("code_verifier", &req.code_verifier),
        ];

        let secret_ref;
        if let Some(ref secret) = req.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        // grok-build attaches x-grok-client-version on authorization_code exchange.
        let mut request = client.post(&req.token_uri).form(&form);
        if is_xai_oauth_token_uri(&req.token_uri) {
            let version = crate::gateway::oauth::adapters::grok::grok_client_version();
            request = request.header("x-grok-client-version", version);
        }

        request
            .send()
            .await
            .map_err(|error| safe_token_request_error("token exchange", &error))?
    };

    parse_token_response(resp, TokenResponseContext::AuthorizationCode).await
}

fn build_anthropic_exchange_json(req: &TokenExchangeRequest) -> serde_json::Value {
    let mut body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": req.code,
        "redirect_uri": req.redirect_uri,
        "client_id": req.client_id,
        "code_verifier": req.code_verifier,
    });

    if let Some(ref state) = req.state {
        body["state"] = serde_json::json!(state);
    }

    if let Some(ref secret) = req.client_secret {
        body["client_secret"] = serde_json::json!(secret);
    }

    body
}

pub(crate) async fn refresh_access_token(
    client: &reqwest::Client,
    req: &TokenRefreshRequest,
) -> Result<OAuthTokenSet, String> {
    tracing::debug!("refreshing access token");

    // Anthropic requires JSON body, others use form-encoded
    let is_anthropic = is_anthropic_oauth_token_uri(&req.token_uri);

    let resp = if is_anthropic {
        let body = build_anthropic_refresh_json(req);

        client
            .post(&req.token_uri)
            .json(&body)
            .send()
            .await
            .map_err(|error| safe_token_request_error("token refresh", &error))?
    } else {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", &req.refresh_token),
            ("client_id", &req.client_id),
        ];

        let secret_ref;
        if let Some(ref secret) = req.client_secret {
            secret_ref = secret.clone();
            form.push(("client_secret", &secret_ref));
        }

        client
            .post(&req.token_uri)
            .form(&form)
            .send()
            .await
            .map_err(|error| safe_token_request_error("token refresh", &error))?
    };

    parse_token_response(resp, TokenResponseContext::RefreshToken).await
}

fn build_anthropic_refresh_json(req: &TokenRefreshRequest) -> serde_json::Value {
    let mut body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": req.refresh_token,
        "client_id": req.client_id,
    });

    if let Some(ref secret) = req.client_secret {
        body["client_secret"] = serde_json::json!(secret);
    }

    body
}

fn is_anthropic_oauth_token_uri(token_uri: &str) -> bool {
    let uri = token_uri.trim().to_ascii_lowercase();
    uri.contains("api.anthropic.com/v1/oauth/token")
        || uri.contains("platform.claude.com/v1/oauth/token")
        || (uri.contains("/v1/oauth/token")
            && (uri.contains("anthropic.com") || uri.contains("claude.com")))
}

fn is_xai_oauth_token_uri(token_uri: &str) -> bool {
    let uri = token_uri.trim().to_ascii_lowercase();
    uri.contains("auth.x.ai/oauth2/token") || uri.contains("://auth.x.ai/")
}

async fn parse_token_response(
    resp: reqwest::Response,
    context: TokenResponseContext,
) -> Result<OAuthTokenSet, String> {
    let status = resp.status();
    let body = read_text_with_limit(resp, OAUTH_TOKEN_RESPONSE_BODY_LIMIT, "token response")
        .await
        .map_err(|_| "failed to read token response body".to_string())?;

    if !status.is_success() {
        // Try to parse error details
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            // Anthropic uses nested error structure: {"type":"error","error":{"type":"...","message":"..."}}
            let error = if let Some(error_obj) = json.get("error").and_then(|v| v.as_object()) {
                // Nested structure (Anthropic format)
                error_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            } else {
                // Flat structure (standard OAuth format)
                json.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            };

            if error == "invalid_grant" {
                return Err(context.invalid_grant_error().to_string());
            }

            return Err(format!(
                "token endpoint returned {status} (JSON error response)"
            ));
        }
        tracing::warn!(
            %status,
            "token endpoint returned a non-JSON error response"
        );
        return Err(format!(
            "token endpoint returned {status} (non-JSON error response)"
        ));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| "failed to parse token response JSON".to_string())?;

    let access_token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("token response missing access_token")?
        .to_string();

    let refresh_token = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let id_token = json
        .get("id_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let expires_at = json
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .map(|secs| now_unix_seconds() + secs);

    Ok(OAuthTokenSet {
        access_token,
        refresh_token,
        expires_at,
        id_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct LogCapture(Arc<Mutex<Vec<u8>>>);

    impl LogCapture {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("capture lock").clone())
                .expect("captured logs are UTF-8")
        }
    }

    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| std::io::Error::other("capture lock poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for LogCapture {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.0.clone())
        }
    }

    async fn spawn_token_endpoint(
        status: &str,
        body: &str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind token endpoint");
        let address = listener.local_addr().expect("token endpoint address");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept token request");
            let mut request = vec![0_u8; 4096];
            let _ = socket.read(&mut request).await.expect("read token request");
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write token response");
        });
        (format!("http://{address}/oauth/token"), task)
    }

    async fn spawn_stalled_token_endpoint() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind stalled token endpoint");
        let address = listener
            .local_addr()
            .expect("stalled token endpoint address");
        let task = tokio::spawn(async move {
            let (_socket, _) = listener
                .accept()
                .await
                .expect("accept stalled token request");
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        });
        (format!("http://{address}/oauth/token"), task)
    }

    #[test]
    fn refresh_error_does_not_log_or_return_remote_secrets() {
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .with_writer(capture.clone())
            .finish();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        let (response_error, send_error) = tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(async {
                let (token_uri, server) =
                    spawn_token_endpoint("400 Bad Request", "SYNTHETIC_SECRET_REMOTE_BODY").await;
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .expect("test client");
                let request = TokenRefreshRequest {
                    token_uri: format!("{token_uri}?query=SYNTHETIC_SECRET_URL"),
                    client_id: "SYNTHETIC_SECRET_CLIENT".to_string(),
                    client_secret: Some("SYNTHETIC_SECRET_CLIENT_SECRET".to_string()),
                    refresh_token: "SYNTHETIC_SECRET_REFRESH_TOKEN".to_string(),
                };
                let error = refresh_access_token(&client, &request)
                    .await
                    .expect_err("non-JSON error response must fail");
                server.await.expect("token endpoint task");

                let invalid_request = TokenRefreshRequest {
                    token_uri: "http://example.invalid:invalid/SYNTHETIC_SECRET_URL".to_string(),
                    client_id: "SYNTHETIC_SECRET_CLIENT".to_string(),
                    client_secret: Some("SYNTHETIC_SECRET_CLIENT_SECRET".to_string()),
                    refresh_token: "SYNTHETIC_SECRET_REFRESH_TOKEN".to_string(),
                };
                let send_error = refresh_access_token(&client, &invalid_request)
                    .await
                    .expect_err("invalid endpoint must fail");
                (error, send_error)
            })
        });

        assert!(
            !response_error.contains("SYNTHETIC_SECRET"),
            "secret leaked in response error: {response_error}"
        );
        assert_eq!(send_error, "token refresh request failed");
        let logs = capture.contents();
        assert!(
            !logs.contains("SYNTHETIC_SECRET"),
            "secret leaked in logs: {logs}"
        );
        assert!(logs.contains("400 Bad Request"));
        assert!(logs.contains("non-JSON error response"));
    }

    #[test]
    fn refresh_invalid_grant_preserves_relogin_classification_without_exposing_description() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        for body in [
            r#"{"error":"invalid_grant"}"#,
            r#"{"error":"invalid_grant","error_description":"arbitrary SYNTHETIC_SECRET"}"#,
        ] {
            let error = runtime.block_on(async {
                let (token_uri, server) = spawn_token_endpoint("400 Bad Request", body).await;
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .expect("test client");
                let error = refresh_access_token(
                    &client,
                    &TokenRefreshRequest {
                        token_uri,
                        client_id: "test-client".to_string(),
                        client_secret: None,
                        refresh_token: "test-refresh-token".to_string(),
                    },
                )
                .await
                .expect_err("invalid grant must fail");
                server.await.expect("token endpoint task");
                error
            });

            assert_eq!(
                error,
                "AUTH_RELOGIN_REQUIRED: refresh token is invalid or expired"
            );
            assert!(!error.contains("SYNTHETIC_SECRET"));
        }
    }

    #[test]
    fn authorization_code_invalid_grant_does_not_claim_refresh_token_failure() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let error = runtime.block_on(async {
            let (token_uri, server) = spawn_token_endpoint(
                "400 Bad Request",
                r#"{"error":"invalid_grant","error_description":"SYNTHETIC_SECRET"}"#,
            )
            .await;
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("test client");
            let error = exchange_authorization_code(
                &client,
                &TokenExchangeRequest {
                    token_uri,
                    client_id: "test-client".to_string(),
                    client_secret: None,
                    code: "test-code".to_string(),
                    redirect_uri: "http://127.0.0.1/callback".to_string(),
                    code_verifier: "test-verifier".to_string(),
                    state: None,
                },
            )
            .await
            .expect_err("invalid authorization code must fail");
            server.await.expect("token endpoint task");
            error
        });

        assert_eq!(
            error,
            "AUTHORIZATION_CODE_INVALID: authorization code is invalid or expired"
        );
        assert!(!error.contains("refresh token"));
        assert!(!error.contains("SYNTHETIC_SECRET"));
    }

    #[test]
    fn refresh_timeout_keeps_a_safe_stable_classification() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let error = runtime.block_on(async {
            let (token_uri, server) = spawn_stalled_token_endpoint().await;
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_millis(30))
                .build()
                .expect("test client");
            let error = refresh_access_token(
                &client,
                &TokenRefreshRequest {
                    token_uri,
                    client_id: "test-client".to_string(),
                    client_secret: None,
                    refresh_token: "SYNTHETIC_SECRET_REFRESH_TOKEN".to_string(),
                },
            )
            .await
            .expect_err("stalled endpoint must time out");
            server.await.expect("stalled token endpoint task");
            error
        });

        assert_eq!(error, "token refresh request timed out");
        assert!(!error.contains("SYNTHETIC_SECRET"));
    }
}
