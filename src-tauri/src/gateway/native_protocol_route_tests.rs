// Included in routes::tests to exercise the production router and its existing fixtures.
mod native_protocol_tests {
    use super::*;
    use crate::shared::gateway_protocol::GatewayProtocol;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PROTOCOLS: [GatewayProtocol; 4] = [
        GatewayProtocol::AnthropicMessages,
        GatewayProtocol::OpenaiCompletions,
        GatewayProtocol::OpenaiResponses,
        GatewayProtocol::GoogleGenerativeAi,
    ];
    const MODEL: &str = "w0-model";

    struct Harness {
        _home: tempfile::TempDir,
        _db_dir: tempfile::TempDir,
        _env: EnvRestore,
        _app: tauri::App<tauri::test::MockRuntime>,
        db: db::Db,
        settings: settings::AppSettings,
        state: GatewayAppState<tauri::test::MockRuntime>,
        logs: tokio::sync::mpsc::Receiver<request_logs::RequestLogInsert>,
    }

    impl Harness {
        fn new() -> Self {
            let home = tempfile::tempdir().unwrap();
            let env = isolate_app_env(home.path());
            let app = tauri::test::mock_app();
            let mut settings = settings::AppSettings::default();
            settings.failover_max_attempts_per_provider = 1;
            settings.failover_max_providers_to_try = 2;
            settings.provider_cooldown_seconds = 0;
            disable_upstream_retry_policy(&mut settings);
            settings::write(app.handle(), &settings).unwrap();
            let db_dir = tempfile::tempdir().unwrap();
            let db = db::init_for_tests(&db_dir.path().join("native-routes.sqlite")).unwrap();
            let (log_tx, logs) = tokio::sync::mpsc::channel(64);
            let state = gateway_state(app.handle().clone(), db.clone(), log_tx);
            Self {
                _home: home,
                _db_dir: db_dir,
                _env: env,
                _app: app,
                db,
                settings,
                state,
                logs,
            }
        }

        fn provider(&self, cli: &str, protocol: GatewayProtocol, name: &str, origin: &str) -> i64 {
            let conn = self.db.open_connection().unwrap();
            conn.execute(
                "INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,gateway_protocol,sort_order,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'upstream-native-key',1,'api_key',?6,(SELECT COALESCE(MAX(sort_order),-1)+1 FROM providers WHERE cli_key=?2),1,1)",
                rusqlite::params![crate::shared::uuid::new_uuid_v4(),cli,name,origin,serde_json::json!([origin]).to_string(),protocol.as_str()],
            ).unwrap();
            let id = conn.last_insert_rowid();
            drop(conn);
            append_default_route_provider(&self.db, cli, id);
            crate::domain::native_gateway::seed_native_gateway_for_test(
                &self.db,
                id,
                cli,
                protocol,
                MODEL,
                &self.settings.model_routing_policy,
            )
            .unwrap();
            id
        }

        async fn request(
            &mut self,
            cli: &str,
            protocol: GatewayProtocol,
            stream: bool,
        ) -> (StatusCode, Vec<u8>, request_logs::RequestLogInsert) {
            let request = native_request(cli, protocol, stream);
            let response = build_router(self.state.clone())
                .oneshot(request)
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec();
            let log = recv_terminal_request_log(&mut self.logs).await;
            (status, body, log)
        }
    }

    fn endpoint(protocol: GatewayProtocol, stream: bool) -> String {
        match protocol {
            GatewayProtocol::AnthropicMessages => "/v1/messages?beta=true".into(),
            GatewayProtocol::OpenaiCompletions => "/v1/chat/completions".into(),
            GatewayProtocol::OpenaiResponses => "/v1/responses".into(),
            GatewayProtocol::GoogleGenerativeAi => format!(
                "/v1beta/models/{MODEL}:{}",
                if stream {
                    "streamGenerateContent?alt=sse"
                } else {
                    "generateContent"
                }
            ),
        }
    }

    fn native_request(cli: &str, protocol: GatewayProtocol, stream: bool) -> Request<Body> {
        let path = endpoint(protocol, stream);
        let query_join = if path.contains('?') { '&' } else { '?' };
        Request::builder().method(Method::POST)
            .uri(format!("/{cli}/_protocol/{}{path}{query_join}key=client-secret", protocol.as_str()))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer client-secret")
            .header("x-api-key", "client-secret")
            .header("x-goog-api-key", "client-secret")
            .header("chatgpt-account-id", "client-secret")
            .body(Body::from(serde_json::json!({"model":MODEL,"stream":stream,"max_tokens":1024,"messages":[{"role":"user","content":"hello"}],"input":"hello","contents":[{"role":"user","parts":[{"text":"hello"}]}]}).to_string())).unwrap()
    }

    fn json_success(protocol: GatewayProtocol) -> String {
        match protocol {
            GatewayProtocol::AnthropicMessages => serde_json::json!({"id":"message_native","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":"native-ok"}],"stop_reason":"end_turn","usage":{"input_tokens":7,"output_tokens":4,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}}),
            GatewayProtocol::OpenaiCompletions => serde_json::json!({"id":"chat_native","model":MODEL,"choices":[{"message":{"role":"assistant","content":"native-ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":4,"total_tokens":11,"prompt_tokens_details":{"cached_tokens":3}}}),
            GatewayProtocol::OpenaiResponses => serde_json::json!({"id":"response_native","model":MODEL,"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"native-ok"}]}],"usage":{"input_tokens":7,"output_tokens":4,"total_tokens":11,"input_tokens_details":{"cached_tokens":3}}}),
            GatewayProtocol::GoogleGenerativeAi => serde_json::json!({"modelVersion":MODEL,"candidates":[{"content":{"role":"model","parts":[{"text":"native-ok"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":4,"totalTokenCount":11,"cachedContentTokenCount":3}}),
        }.to_string()
    }

    fn data(value: Value) -> String {
        format!("data: {value}\n\n")
    }
    fn event(value: Value) -> String {
        format!(
            "event: {}\ndata: {value}\n\n",
            value["type"].as_str().unwrap()
        )
    }

    fn sse_success(protocol: GatewayProtocol) -> String {
        let text = "W0_LOCAL_OK";
        match protocol {
            GatewayProtocol::AnthropicMessages => [
                serde_json::json!({"type":"message_start","message":{"id":"msg_w0","type":"message","role":"assistant","model":MODEL,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":7,"output_tokens":0,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}}}),
                serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
                serde_json::json!({"type":"content_block_stop","index":0}),
                serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":4}}),
                serde_json::json!({"type":"message_stop"}),
            ].into_iter().map(event).collect(),
            GatewayProtocol::OpenaiCompletions => [
                data(serde_json::json!({"id":"chatcmpl_w0","object":"chat.completion.chunk","created":1,"model":MODEL,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]})),
                data(serde_json::json!({"id":"chatcmpl_w0","object":"chat.completion.chunk","created":1,"model":MODEL,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})),
                data(serde_json::json!({"id":"chatcmpl_w0","object":"chat.completion.chunk","model":MODEL,"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":4,"total_tokens":11,"prompt_tokens_details":{"cached_tokens":3}}})),
                "data: [DONE]\n\n".into(),
            ].concat(),
            GatewayProtocol::OpenaiResponses => {
                let message = serde_json::json!({"id":"msg_w0","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]});
                let response = serde_json::json!({"id":"resp_w0","object":"response","created_at":1,"status":"completed","model":MODEL,"output":[message.clone()],"usage":{"input_tokens":7,"output_tokens":4,"total_tokens":11,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":0}}});
                let mut events = vec![
                    serde_json::json!({"type":"response.created","response":{"id":"resp_w0","object":"response","status":"in_progress","model":MODEL,"output":[]}}),
                    serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_w0","type":"message","status":"in_progress","role":"assistant","content":[]}}),
                    serde_json::json!({"type":"response.content_part.added","item_id":"msg_w0","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
                    serde_json::json!({"type":"response.output_text.delta","item_id":"msg_w0","output_index":0,"content_index":0,"delta":text}),
                    serde_json::json!({"type":"response.output_text.done","item_id":"msg_w0","output_index":0,"content_index":0,"text":text}),
                    serde_json::json!({"type":"response.content_part.done","item_id":"msg_w0","output_index":0,"content_index":0,"part":message["content"][0]}),
                    serde_json::json!({"type":"response.output_item.done","output_index":0,"item":message}),
                    serde_json::json!({"type":"response.completed","response":response}),
                ];
                for (index, event) in events.iter_mut().enumerate() { event["sequence_number"] = serde_json::json!(index); }
                events.into_iter().map(event).collect()
            }
            GatewayProtocol::GoogleGenerativeAi => data(serde_json::json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":text}]},"finishReason":"STOP"}],"modelVersion":MODEL,"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":4,"totalTokenCount":11,"cachedContentTokenCount":3}})),
        }
    }

    struct Upstream {
        origin: String,
        calls: Arc<AtomicUsize>,
        captures: Arc<Mutex<Vec<CapturedRawRequest>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Upstream {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn upstream(status: StatusCode, content_type: &'static str, body: String) -> Upstream {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let captures = Arc::new(Mutex::new(Vec::new()));
        let count = calls.clone();
        let captured = captures.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let raw = read_complete_http_request_bytes(&mut socket).await;
                count.fetch_add(1, Ordering::SeqCst);
                let (head_end, body_start) = find_http_head_split(&raw).unwrap();
                captured.lock().unwrap().push(CapturedRawRequest {
                    head: String::from_utf8_lossy(&raw[..head_end]).into_owned(),
                    body: raw[body_start..].to_vec(),
                });
                let response = format!("HTTP/1.1 {} {}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", status.as_u16(), status.canonical_reason().unwrap(), body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        Upstream {
            origin,
            calls,
            captures,
            task,
        }
    }

    fn attempts(log: &request_logs::RequestLogInsert) -> Vec<Value> {
        serde_json::from_str(&log.attempts_json).unwrap()
    }
    fn assert_usage(log: &request_logs::RequestLogInsert, cli: &str, protocol: GatewayProtocol) {
        assert_eq!(log.cli_key, cli);
        assert_eq!(log.requested_model.as_deref(), Some(MODEL));
        assert_eq!(log.error_code, None);
        let input = if protocol == GatewayProtocol::AnthropicMessages {
            7
        } else {
            4
        };
        assert_eq!(log.input_tokens, Some(input));
        assert_eq!(log.output_tokens, Some(4));
        assert_eq!(log.cache_read_input_tokens, Some(3));
        if protocol == GatewayProtocol::AnthropicMessages {
            assert_eq!(log.cache_creation_input_tokens, Some(2));
        }
        let markers: Vec<Value> =
            serde_json::from_str(log.special_settings_json.as_deref().unwrap()).unwrap();
        assert!(markers
            .iter()
            .any(|value| value["type"] == "gateway_protocol"
                && value["sourceCli"] == cli
                && value["protocol"] == protocol.as_str()
                && value["input_semantics"] == "exclusive"
                && value["wire_input_tokens"] == 7));
        assert!(!markers.iter().any(|value| value["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("codex_"))));
    }

    #[tokio::test]
    async fn native_json_routes_preserve_source_protocol_auth_model_and_usage() {
        let _lock = crate::test_support::test_env_lock();
        for cli in ["pi", "omp"] {
            for protocol in PROTOCOLS {
                let mut h = Harness::new();
                let remote =
                    upstream(StatusCode::OK, "application/json", json_success(protocol)).await;
                let id = h.provider(cli, protocol, "native-json", &remote.origin);
                let (status, body, log) = h.request(cli, protocol, false).await;
                assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
                assert_usage(&log, cli, protocol);
                assert_eq!(attempts(&log).len(), 1);
                assert_eq!(attempts(&log)[0]["provider_id"], id);
                assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
                let captures = remote.captures.lock().unwrap();
                let request = &captures[0];
                assert!(request
                    .head
                    .starts_with(&format!("POST {} HTTP/", endpoint(protocol, false))));
                assert!(!request.text().contains("client-secret"));
                assert!(!request.head.contains("_protocol"));
                let header = match protocol {
                    GatewayProtocol::AnthropicMessages => "x-api-key: upstream-native-key",
                    GatewayProtocol::GoogleGenerativeAi => "x-goog-api-key: upstream-native-key",
                    _ => "authorization: Bearer upstream-native-key",
                };
                assert!(request.has_header_line(header));
                assert_eq!(
                    serde_json::from_slice::<Value>(&request.body).unwrap()["model"],
                    MODEL
                );
            }
        }
    }

    #[tokio::test]
    async fn native_sse_routes_keep_late_usage_and_protocol_completion() {
        let _lock = crate::test_support::test_env_lock();
        for cli in ["pi", "omp"] {
            for protocol in PROTOCOLS {
                let mut h = Harness::new();
                let remote =
                    upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
                h.provider(cli, protocol, "native-sse", &remote.origin);
                let (status, body, log) = h.request(cli, protocol, true).await;
                assert_eq!(status, StatusCode::OK);
                assert!(String::from_utf8_lossy(&body).contains("W0_LOCAL_OK"));
                assert_usage(&log, cli, protocol);
                assert_eq!(attempts(&log).len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn native_incompatible_candidates_have_zero_send_attempt_and_health_penalty() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let remote = upstream(StatusCode::OK, "application/json", "{}".into()).await;
        let cross_protocol = h.provider(
            "pi",
            GatewayProtocol::AnthropicMessages,
            "wrong-protocol",
            &remote.origin,
        );
        let cross_client = h.provider(
            "omp",
            GatewayProtocol::OpenaiResponses,
            "wrong-client",
            &remote.origin,
        );
        let undeclared = h.provider(
            "pi",
            GatewayProtocol::OpenaiResponses,
            "undeclared-model",
            &remote.origin,
        );
        h.db.open_connection()
            .unwrap()
            .execute(
                "DELETE FROM native_gateway_model_specs WHERE provider_id=?1",
                [undeclared],
            )
            .unwrap();
        let (status, body, log) = h
            .request("pi", GatewayProtocol::OpenaiResponses, false)
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(serde_json::from_slice::<Value>(&body)
            .unwrap()
            .get("error")
            .is_some());
        assert!(attempts(&log).is_empty());
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
        for id in [cross_protocol, cross_client, undeclared] {
            assert_eq!(h.state.circuit.snapshot(id, 1).failure_count, 0);
        }
    }

    #[tokio::test]
    async fn native_same_protocol_failover_skips_foreign_protocol_without_using_budget() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let wrong = upstream(StatusCode::OK, "application/json", "{}".into()).await;
        let failed = upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            "application/json",
            r#"{"error":{"type":"server_error","message":"temporary"}}"#.into(),
        )
        .await;
        let success = upstream(
            StatusCode::OK,
            "application/json",
            json_success(GatewayProtocol::OpenaiResponses),
        )
        .await;
        let wrong_id = h.provider(
            "pi",
            GatewayProtocol::AnthropicMessages,
            "foreign-protocol",
            &wrong.origin,
        );
        let first_id = h.provider(
            "pi",
            GatewayProtocol::OpenaiResponses,
            "first",
            &failed.origin,
        );
        let second_id = h.provider(
            "pi",
            GatewayProtocol::OpenaiResponses,
            "second",
            &success.origin,
        );
        let (status, _, log) = h
            .request("pi", GatewayProtocol::OpenaiResponses, false)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            attempts(&log)
                .iter()
                .map(|v| v["provider_id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![first_id, second_id]
        );
        assert_eq!(wrong.calls.load(Ordering::SeqCst), 0);
        assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
        assert_eq!(success.calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.state.circuit.snapshot(wrong_id, 1).failure_count, 0);
        assert_usage(&log, "pi", GatewayProtocol::OpenaiResponses);
    }

    fn metadata_frame(protocol: GatewayProtocol) -> String {
        match protocol {
            GatewayProtocol::AnthropicMessages => event(
                serde_json::json!({"type":"message_start","message":{"model":MODEL,"content":[],"usage":{"input_tokens":7,"output_tokens":0}}}),
            ),
            GatewayProtocol::OpenaiCompletions => data(
                serde_json::json!({"model":MODEL,"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
            ),
            GatewayProtocol::OpenaiResponses => event(
                serde_json::json!({"type":"response.created","response":{"id":"resp_metadata","model":MODEL,"status":"in_progress","output":[]}}),
            ),
            GatewayProtocol::GoogleGenerativeAi => {
                data(serde_json::json!({"modelVersion":MODEL,"candidates":[]}))
            }
        }
    }

    fn visible_frame(protocol: GatewayProtocol) -> String {
        match protocol {
            GatewayProtocol::AnthropicMessages => event(
                serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"visible-before-error"}}),
            ),
            GatewayProtocol::OpenaiCompletions => data(
                serde_json::json!({"model":MODEL,"choices":[{"index":0,"delta":{"content":"visible-before-error"},"finish_reason":null}]}),
            ),
            GatewayProtocol::OpenaiResponses => event(
                serde_json::json!({"type":"response.output_text.delta","delta":"visible-before-error"}),
            ),
            GatewayProtocol::GoogleGenerativeAi => data(
                serde_json::json!({"modelVersion":MODEL,"candidates":[{"content":{"parts":[{"text":"visible-before-error"}]}}]}),
            ),
        }
    }

    #[tokio::test]
    async fn native_metadata_then_error_fails_over_before_commit_for_every_protocol() {
        let _lock = crate::test_support::test_env_lock();
        for protocol in PROTOCOLS {
            let mut h = Harness::new();
            let error = crate::gateway::proxy::protocol::error_frame(
                protocol,
                500,
                "temporary provider failure",
            );
            let failed = upstream(
                StatusCode::OK,
                "text/event-stream",
                metadata_frame(protocol) + std::str::from_utf8(&error).unwrap(),
            )
            .await;
            let success =
                upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
            let first = h.provider("omp", protocol, "pre-commit-failure", &failed.origin);
            let second = h.provider("omp", protocol, "fallback", &success.origin);
            let (status, body, log) = h.request("omp", protocol, true).await;
            assert_eq!(status, StatusCode::OK);
            let body = String::from_utf8(body).unwrap();
            assert!(body.contains("W0_LOCAL_OK"));
            assert!(!body.contains("temporary provider failure"));
            assert_eq!(
                attempts(&log)
                    .iter()
                    .map(|value| value["provider_id"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                vec![first, second]
            );
            assert_eq!(attempts(&log)[0]["error_code"], "GW_FAKE_200");
            assert_usage(&log, "omp", protocol);
        }
    }

    #[tokio::test]
    async fn native_error_after_committed_content_never_replays_to_another_provider() {
        let _lock = crate::test_support::test_env_lock();
        for protocol in PROTOCOLS {
            let mut h = Harness::new();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let (release, resume) = tokio::sync::oneshot::channel::<()>();
            let task = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let _ = read_complete_http_request_bytes(&mut socket).await;
                let content = visible_frame(protocol);
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n{:x}\r\n{}\r\n", content.len(), content).as_bytes()).await.unwrap();
                resume.await.unwrap();
                let error = crate::gateway::proxy::protocol::error_frame(
                    protocol,
                    500,
                    "terminal-native-error",
                );
                socket
                    .write_all(
                        format!(
                            "{:x}\r\n{}\r\n0\r\n\r\n",
                            error.len(),
                            std::str::from_utf8(&error).unwrap()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                let _ = socket.shutdown().await;
            });
            let fallback =
                upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
            let first = h.provider("pi", protocol, "committed", &origin);
            h.provider("pi", protocol, "unused-fallback", &fallback.origin);
            let response = tokio::time::timeout(
                Duration::from_secs(3),
                build_router(h.state.clone()).oneshot(native_request("pi", protocol, true)),
            )
            .await
            .expect("native content commits without Codex guard")
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = Box::pin(response.into_body());
            let first_chunk = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx))
                .await
                .unwrap()
                .unwrap()
                .into_data()
                .unwrap();
            assert!(String::from_utf8_lossy(&first_chunk).contains("visible-before-error"));
            release.send(()).unwrap();
            let tail = to_bytes(Body::new(body), 1024 * 1024).await.unwrap();
            assert!(String::from_utf8_lossy(&tail).contains("terminal-native-error"));
            let log = recv_terminal_request_log(&mut h.logs).await;
            assert!(log.error_code.is_some());
            assert_eq!(attempts(&log).len(), 1);
            assert_eq!(attempts(&log)[0]["provider_id"], first);
            assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn native_chat_eof_without_done_is_a_terminal_failure_without_replay() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let protocol = GatewayProtocol::OpenaiCompletions;
        let remote = upstream(StatusCode::OK, "text/event-stream", visible_frame(protocol)).await;
        let fallback = upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
        h.provider("pi", protocol, "incomplete", &remote.origin);
        h.provider("pi", protocol, "unused", &fallback.origin);
        let (_, body, log) = h.request("pi", protocol, true).await;
        assert!(String::from_utf8_lossy(&body).contains("GW_STREAM_ERROR"));
        assert_eq!(log.error_code.as_deref(), Some("GW_STREAM_ERROR"));
        assert_eq!(attempts(&log).len(), 1);
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn native_real_wire_tools_images_and_thinking_survive_actual_router() {
        let _lock = crate::test_support::test_env_lock();
        // W0 Pi 0.87.1 / OMP 18.3.2 captures, including the tool-result turn.
        // Generated environment/system prompts are omitted; feature fields and
        // protocol responses preserve the SDK's actual wire shapes.
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/native_wire_features.json")).unwrap();
        assert_eq!(fixtures.len(), 32);
        for fixture in fixtures {
            let cli = fixture["client"].as_str().unwrap();
            let protocol = GatewayProtocol::parse(fixture["protocol"].as_str().unwrap()).unwrap();
            let mut h = Harness::new();
            let response_wire = fixture["response"].as_str().unwrap();
            let remote = upstream(StatusCode::OK, "text/event-stream", response_wire.into()).await;
            h.provider(cli, protocol, "real-feature-wire", &remote.origin);
            let request = Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/{cli}/_protocol/{}{}",
                    protocol.as_str(),
                    fixture["path"].as_str().unwrap()
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(fixture["body"].to_string()))
                .unwrap();
            let response = build_router(h.state.clone())
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{cli} {protocol:?} {}",
                fixture["scenario"]
            );
            let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            assert_eq!(
                body.as_ref(),
                response_wire.as_bytes(),
                "feature SSE changed: {cli} {protocol:?} {}",
                fixture["scenario"]
            );
            let log = recv_terminal_request_log(&mut h.logs).await;
            assert_eq!(log.cli_key, cli);
            assert_eq!(
                log.error_code, None,
                "{cli} {protocol:?} {}",
                fixture["scenario"]
            );
            assert_eq!(attempts(&log).len(), 1);
            let captures = remote.captures.lock().unwrap();
            assert_eq!(captures.len(), 1);
            let forwarded: Value = serde_json::from_slice(&captures[0].body).unwrap();
            assert_eq!(
                forwarded, fixture["body"],
                "tools/image/thinking request changed: {cli} {protocol:?} {} turn {}",
                fixture["scenario"], fixture["turn"]
            );
        }
    }

    #[tokio::test]
    async fn native_client_cancel_after_commit_does_not_replay_or_penalize_provider() {
        let _lock = crate::test_support::test_env_lock();
        for cli in ["pi", "omp"] {
            for protocol in PROTOCOLS {
                let mut h = Harness::new();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let task = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let _ = read_complete_http_request_bytes(&mut socket).await;
                    let content = visible_frame(protocol);
                    socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n{:x}\r\n{}\r\n",content.len(),content).as_bytes()).await.unwrap();
                    // Keep upstream open until gateway observes downstream cancellation.
                    let mut probe = [0u8; 1];
                    let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut probe).await;
                });
                let fallback =
                    upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
                let first = h.provider(cli, protocol, "cancelled", &origin);
                let second = h.provider(cli, protocol, "unused-fallback", &fallback.origin);
                let response = tokio::time::timeout(
                    Duration::from_secs(3),
                    build_router(h.state.clone()).oneshot(native_request(cli, protocol, true)),
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let mut body = Box::pin(response.into_body());
                let chunk = std::future::poll_fn(|cx| body.as_mut().poll_frame(cx))
                    .await
                    .unwrap()
                    .unwrap()
                    .into_data()
                    .unwrap();
                assert!(String::from_utf8_lossy(&chunk).contains("visible-before-error"));
                drop(body);
                let log = recv_terminal_request_log(&mut h.logs).await;
                assert_eq!(log.cli_key, cli);
                assert_eq!(log.error_code.as_deref(), Some("GW_STREAM_ABORTED"));
                assert_eq!(attempts(&log).len(), 1);
                assert_eq!(attempts(&log)[0]["provider_id"], first);
                assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
                assert_eq!(h.state.circuit.snapshot(first, 1).failure_count, 0);
                assert_eq!(h.state.circuit.snapshot(second, 1).failure_count, 0);
                task.abort();
            }
        }
    }

    #[tokio::test]
    async fn native_legacy_auxiliary_and_mismatched_routes_are_rejected_without_send() {
        let _lock = crate::test_support::test_env_lock();
        let h = Harness::new();
        let remote = upstream(StatusCode::OK, "application/json", "{}".into()).await;
        h.provider(
            "pi",
            GatewayProtocol::OpenaiResponses,
            "never-sent",
            &remote.origin,
        );
        for path in [
            "/pi/v1/responses",
            "/pi/_protocol/openai-responses/v1/responses/compact",
            "/pi/_protocol/openai-responses/v1/chat/completions",
            "/pi/_protocol/anthropic-messages/v1/messages/count_tokens",
            "/codex/_protocol/openai-responses/v1/responses",
        ] {
            let request = Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"model":"w0-model"}"#))
                .unwrap();
            let response = build_router(h.state.clone())
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        }
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    /// Opt-in: requires the pinned CLI packages installed by the W0 bootstrap.
    /// Every CLI enters the actual Axum gateway, skips an incompatible provider,
    /// fails over from HTTP 500 and consumes the second provider's protocol stream.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires W0 pinned Pi/OMP runtimes; run explicitly with --ignored --nocapture"]
    async fn native_real_cli_eight_protocol_streams_through_actual_gateway() {
        let _lock = crate::test_support::test_env_lock();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut results = Vec::new();
        for cli in ["pi", "omp"] {
            for protocol in PROTOCOLS {
                let mut h = Harness::new();
                let incompatible_protocol = if protocol == GatewayProtocol::AnthropicMessages {
                    GatewayProtocol::OpenaiResponses
                } else {
                    GatewayProtocol::AnthropicMessages
                };
                let wrong = upstream(StatusCode::OK, "application/json", "{}".into()).await;
                let failed = upstream(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "application/json",
                    r#"{"error":{"type":"server_error","message":"retry other provider"}}"#.into(),
                )
                .await;
                let success =
                    upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
                let wrong_id =
                    h.provider(cli, incompatible_protocol, "foreign-wire", &wrong.origin);
                let first = h.provider(cli, protocol, "fails-once", &failed.origin);
                let second = h.provider(cli, protocol, "returns-cli-stream", &success.origin);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let (_, groups) = crate::domain::native_gateway::catalog(
                    &h.db.open_connection().unwrap(),
                    cli,
                    Some(&origin),
                    &serde_json::to_string(&h.settings.model_routing_policy).unwrap(),
                )
                .unwrap();
                let entry = crate::domain::native_gateway::generate_entries(cli, &origin, &groups)
                    .unwrap()
                    .into_iter()
                    .find(|entry| entry.protocol == protocol)
                    .unwrap();
                let entry_path = root.join(format!(
                    ".trellis/.runtime/research/omp-pi/gateway-test/entry-{cli}-{}.json",
                    protocol.as_str()
                ));
                std::fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
                std::fs::write(&entry_path, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
                let router = build_router(h.state.clone());
                let server = tokio::spawn(async move {
                    axum::serve(listener, router).await.unwrap();
                });
                let output = tokio::time::timeout(
                    Duration::from_secs(50),
                    tokio::process::Command::new("node")
                        .current_dir(root)
                        .arg("scripts/pi-omp-gateway-client.mjs")
                        .args([
                            "--client",
                            cli,
                            "--protocol",
                            protocol.as_str(),
                            "--base-origin",
                            &origin,
                            "--model",
                            MODEL,
                            "--target",
                            ".trellis/.runtime/research/omp-pi/gateway-test",
                            "--expect-text",
                            "W0_LOCAL_OK",
                            "--entry-json",
                            entry_path.to_str().unwrap(),
                        ])
                        .kill_on_drop(true)
                        .output(),
                )
                .await
                .expect("real CLI timeout")
                .expect("start W0 gateway client");
                server.abort();
                assert!(
                    output.status.success(),
                    "{cli}/{} stdout={} stderr={}",
                    protocol.as_str(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let result: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(result["passed"], true);
                let log = recv_terminal_request_log(&mut h.logs).await;
                assert_usage(&log, cli, protocol);
                assert_eq!(
                    attempts(&log)
                        .iter()
                        .map(|v| v["provider_id"].as_i64().unwrap())
                        .collect::<Vec<_>>(),
                    vec![first, second]
                );
                assert_eq!(wrong.calls.load(Ordering::SeqCst), 0);
                assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
                assert_eq!(success.calls.load(Ordering::SeqCst), 1);
                assert_eq!(h.state.circuit.snapshot(wrong_id, 1).failure_count, 0);
                results.push(result);
            }
        }
        let report =
            root.join(".trellis/.runtime/research/omp-pi/gateway-test/aio-gateway-results.json");
        std::fs::create_dir_all(report.parent().unwrap()).unwrap();
        std::fs::write(&report, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
        println!(
            "8/8 real CLI streams traversed AIO gateway with same-protocol failover: {}",
            report.display()
        );
    }
    include!("native_channel_route_tests.rs");
}
