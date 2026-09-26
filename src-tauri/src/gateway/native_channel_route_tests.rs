// Included under native_protocol_tests to reuse the production-router harness.
mod channel_tests {
    use super::*;
    use crate::domain::native_channels::*;
    use crate::domain::native_cli::{NativeClient, NativeTarget, NativeTargetSelection};
    use crate::domain::native_gateway::GeneratedEntry;

    fn declare(h: &Harness, consumer: &str, id: i64, protocol: GatewayProtocol) {
        let mut conn = h.db.open_connection().unwrap();
        let uuid: String = conn
            .query_row(
                "SELECT provider_uuid FROM providers WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        let tx = conn.transaction().unwrap();
        let current = channel_models_get(&tx, id, &uuid, consumer, protocol).unwrap();
        let spec=serde_json::from_value(serde_json::json!({"requestModelId":MODEL,"displayName":"Explicit source model","input":["text","image"],"contextWindow":64000,"maxTokens":4000,"reasoning":false,"thinking":null,"supportsTools":true})).unwrap();
        channel_models_set(
            &tx,
            id,
            &uuid,
            consumer,
            protocol,
            &current.revision,
            &[spec],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    fn source_provider(
        h: &Harness,
        consumer: &str,
        source: SourceChannel,
        protocol: GatewayProtocol,
        origin: &str,
        oauth: bool,
    ) -> i64 {
        let conn = h.db.open_connection().unwrap();
        let uuid = crate::shared::uuid::new_uuid_v4();
        conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,oauth_provider_type,oauth_access_token,oauth_expires_at,sort_order,created_at,updated_at) VALUES (?1,?2,?1,?3,?4,'source-secret',1,?5,?6,?7,4102444800,(SELECT COALESCE(MAX(sort_order),-1)+1 FROM providers WHERE cli_key=?2),1,1)",rusqlite::params![uuid,source.as_str(),origin,serde_json::json!([origin]).to_string(),if oauth {"oauth"} else {"api_key"},if oauth {Some(format!("{}_oauth",source.as_str()))}else{None},if oauth {Some("source-oauth-token")}else{None}]).unwrap();
        let id = conn.last_insert_rowid();
        drop(conn);
        append_default_route_provider(&h.db, source.as_str(), id);
        declare(h, consumer, id, protocol);
        id
    }
    fn publish(
        h: &Harness,
        consumer: &str,
        source: SourceChannel,
        protocol: GatewayProtocol,
        origin: &str,
    ) -> (NativeTarget, GeneratedEntry, String) {
        let client = if consumer == "pi" {
            NativeClient::Pi
        } else {
            NativeClient::Omp
        };
        let target = crate::infra::native_cli::targets::resolve(
            h._home.path(),
            &NativeTargetSelection::default_for(client),
            None,
            None,
        )
        .unwrap();
        let policy = serde_json::to_string(&h.settings.model_routing_policy).unwrap();
        let c = crate::app::native_channel_service::catalog_for_target(
            &h.db,
            &target,
            Some(origin),
            &policy,
            || Ok(()),
        )
        .unwrap();
        let i = ChannelLifecycleInput {
            target_id: target.target_id.clone(),
            expected_revision: c.revision,
            catalog_revision: c.catalog_revision,
            selections: vec![ChannelSelection {
                source_channel: source,
                protocol,
                model_ids: vec![MODEL.into()],
            }],
            remove_binding_ids: vec![],
        };
        crate::app::native_channel_service::apply_for_target(
            &h.db,
            &target,
            &i,
            Some(origin),
            &policy,
            || Ok(()),
        )
        .unwrap();
        let conn = h.db.open_connection().unwrap();
        let binding = list_bindings(&conn, &target.target_id)
            .unwrap()
            .into_iter()
            .find(|m| {
                m.protocol == protocol && m.channel.as_ref().unwrap().source_channel == source
            })
            .unwrap();
        (
            target,
            binding.payload.desired.unwrap(),
            binding.channel.unwrap().binding_id,
        )
    }
    fn request(
        consumer: &str,
        protocol: GatewayProtocol,
        binding: &str,
        stream: bool,
    ) -> Request<Body> {
        let mut request = native_request(consumer, protocol, stream);
        *request.uri_mut() = format!(
            "/{consumer}/_aio/channel/{binding}{}",
            endpoint(protocol, stream)
        )
        .parse()
        .unwrap();
        request
    }
    async fn send(
        h: &mut Harness,
        req: Request<Body>,
    ) -> (StatusCode, Vec<u8>, request_logs::RequestLogInsert) {
        let response = build_router(h.state.clone()).oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (status, bytes, recv_terminal_request_log(&mut h.logs).await)
    }
    fn assert_binding_log(
        log: &request_logs::RequestLogInsert,
        consumer: &str,
        source: SourceChannel,
        id: &str,
    ) {
        assert_eq!(log.cli_key, consumer);
        let markers: Vec<Value> =
            serde_json::from_str(log.special_settings_json.as_deref().unwrap()).unwrap();
        assert!(markers.iter().any(|v| v["type"] == "channel_binding"
            && v["consumerCli"] == consumer
            && v["sourceChannel"] == source.as_str()
            && v["bindingId"] == id));
    }

    #[tokio::test]
    async fn native_channel_four_sources_five_protocol_groups_keep_consumer_usage_and_source_auth()
    {
        let _lock = crate::test_support::test_env_lock();
        for consumer in ["pi", "omp"] {
            for source in SourceChannel::ALL {
                for &protocol in source.protocols() {
                    for stream in [false, true] {
                        let mut h = Harness::new();
                        let remote = upstream(
                            StatusCode::OK,
                            if stream {
                                "text/event-stream"
                            } else {
                                "application/json"
                            },
                            if stream {
                                sse_success(protocol)
                            } else {
                                json_success(protocol)
                            },
                        )
                        .await;
                        let id =
                            source_provider(&h, consumer, source, protocol, &remote.origin, false);
                        let (_, _, binding) =
                            publish(&h, consumer, source, protocol, "http://127.0.0.1:3711");
                        let (status, body, log) =
                            send(&mut h, request(consumer, protocol, &binding, stream)).await;
                        assert_eq!(
                            status,
                            StatusCode::OK,
                            "{consumer}/{source:?}/{protocol:?}: {}",
                            String::from_utf8_lossy(&body)
                        );
                        assert_usage(&log, consumer, protocol);
                        assert_binding_log(&log, consumer, source, &binding);
                        assert_eq!(attempts(&log)[0]["provider_id"], id);
                        let captured = remote.captures.lock().unwrap();
                        assert_eq!(captured.len(), 1);
                        assert!(!captured[0].text().contains("client-secret"));
                        assert!(!captured[0].head.contains("_aio"));
                        assert!(captured[0].has_header_line(match protocol {
                            GatewayProtocol::AnthropicMessages => "x-api-key: source-secret",
                            GatewayProtocol::GoogleGenerativeAi => "x-goog-api-key: source-secret",
                            _ => "authorization: Bearer source-secret",
                        }));
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn native_channel_oauth_uses_existing_source_adapters_without_relabeling_consumer() {
        let _lock = crate::test_support::test_env_lock();
        for consumer in ["pi", "omp"] {
            for source in [
                SourceChannel::Claude,
                SourceChannel::Codex,
                SourceChannel::Grok,
            ] {
                let protocol = source.protocols()[0];
                let mut h = Harness::new();
                let remote =
                    upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
                h._env
                    .set_var("AIO_CODING_HUB_TEST_SOURCE_OAUTH_BASE_URL", &remote.origin);
                source_provider(&h, consumer, source, protocol, &remote.origin, true);
                let (_, _, binding) =
                    publish(&h, consumer, source, protocol, "http://127.0.0.1:3711");
                let (status, body, log) =
                    send(&mut h, request(consumer, protocol, &binding, true)).await;
                assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
                assert_usage(&log, consumer, protocol);
                assert_binding_log(&log, consumer, source, &binding);
                assert!(remote.captures.lock().unwrap()[0]
                    .has_header_line("authorization: Bearer source-oauth-token"));
            }
        }
    }

    #[tokio::test]
    async fn native_channel_binding_rejects_cross_consumer_forced_source_and_wrong_protocol() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let p = GatewayProtocol::OpenaiResponses;
        let remote = upstream(StatusCode::OK, "application/json", json_success(p)).await;
        source_provider(&h, "pi", SourceChannel::Codex, p, &remote.origin, false);
        let foreign = source_provider(&h, "pi", SourceChannel::Grok, p, &remote.origin, false);
        let (_, _, binding) = publish(&h, "pi", SourceChannel::Codex, p, "http://127.0.0.1:3711");
        for req in [
            request("omp", p, &binding, false),
            request("pi", GatewayProtocol::AnthropicMessages, &binding, false),
            request("pi", p, "unknown-binding", false),
        ] {
            let response = build_router(h.state.clone()).oneshot(req).await.unwrap();
            assert!(response.status().is_client_error());
        }
        let mut forced = request("pi", p, &binding, false);
        forced
            .headers_mut()
            .insert("x-aio-provider-id", foreign.to_string().parse().unwrap());
        let (status, _, _) = send(&mut h, forced).await;
        assert!(!status.is_success());
        assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
    }

    async fn scripted_upstream(
        responses: Vec<(StatusCode, String)>,
        on_request: impl Fn(usize) + Send + Sync + 'static,
    ) -> Upstream {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let captures = Arc::new(Mutex::new(Vec::new()));
        let count = calls.clone();
        let captured = captures.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let raw = read_complete_http_request_bytes(&mut socket).await;
                let index = count.fetch_add(1, Ordering::SeqCst);
                let (end, start) = find_http_head_split(&raw).unwrap();
                captured.lock().unwrap().push(CapturedRawRequest {
                    head: String::from_utf8_lossy(&raw[..end]).into_owned(),
                    body: raw[start..].to_vec(),
                });
                on_request(index);
                let (status, body) = &responses[index.min(responses.len() - 1)];
                let content_type = if body.starts_with("event:") {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                let response=format!("HTTP/1.1 {} {}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",status.as_u16(),status.canonical_reason().unwrap(),body.len());
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

    #[tokio::test]
    async fn native_channel_rechecks_withdrawal_before_same_request_failover() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let p = GatewayProtocol::OpenaiResponses;
        let db = h.db.clone();
        let failed = scripted_upstream(
            vec![(
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":{"message":"fail over"}}"#.into(),
            )],
            move |_| {
                db.open_connection()
                    .unwrap()
                    .execute("DELETE FROM native_channel_bindings", [])
                    .unwrap();
            },
        )
        .await;
        let success = upstream(StatusCode::OK, "application/json", json_success(p)).await;
        let first = source_provider(&h, "pi", SourceChannel::Codex, p, &failed.origin, false);
        let second = source_provider(&h, "pi", SourceChannel::Codex, p, &success.origin, false);
        let (_, _, binding) = publish(&h, "pi", SourceChannel::Codex, p, "http://127.0.0.1:3711");
        let (status, _, log) = send(&mut h, request("pi", p, &binding, false)).await;
        assert!(!status.is_success());
        assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
        assert_eq!(success.calls.load(Ordering::SeqCst), 0);
        assert_eq!(attempts(&log)[0]["provider_id"], first);
        assert_eq!(
            h.state
                .circuit
                .snapshot(second, crate::shared::time::now_unix_seconds())
                .failure_count,
            0
        );
    }

    #[tokio::test]
    async fn native_channel_codex_oauth_refresh_retries_using_source_identity() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let p = GatewayProtocol::OpenaiResponses;
        let remote = scripted_upstream(
            vec![
                (
                    StatusCode::UNAUTHORIZED,
                    r#"{"error":{"message":"expired token"}}"#.into(),
                ),
                (StatusCode::OK, sse_success(p)),
            ],
            |_| {},
        )
        .await;
        h._env
            .set_var("AIO_CODING_HUB_TEST_SOURCE_OAUTH_BASE_URL", &remote.origin);
        let token=upstream(StatusCode::OK,"application/json",r#"{"access_token":"refreshed-source-token","refresh_token":"new-refresh","expires_in":3600,"token_type":"Bearer"}"#.into()).await;
        let id = source_provider(&h, "omp", SourceChannel::Codex, p, &remote.origin, true);
        h.db.open_connection().unwrap().execute("UPDATE providers SET oauth_refresh_token='local-refresh',oauth_token_uri=?1,oauth_client_id='fixture-client' WHERE id=?2",rusqlite::params![format!("{}/token",token.origin),id]).unwrap();
        let (_, _, binding) = publish(&h, "omp", SourceChannel::Codex, p, "http://127.0.0.1:3711");
        let (status, body, log) = send(&mut h, request("omp", p, &binding, true)).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert_usage(&log, "omp", p);
        assert_binding_log(&log, "omp", SourceChannel::Codex, &binding);
        assert_eq!(token.calls.load(Ordering::SeqCst), 1);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 2);
        let captured = remote.captures.lock().unwrap();
        assert!(captured[0].has_header_line("authorization: Bearer source-oauth-token"));
        assert!(captured[1].has_header_line("authorization: Bearer refreshed-source-token"));
    }

    #[tokio::test]
    async fn native_channel_model_mapping_rotation_and_session_isolation_follow_source() {
        let _lock = crate::test_support::test_env_lock();
        let mut h = Harness::new();
        let p = GatewayProtocol::OpenaiResponses;
        let remote = upstream(StatusCode::OK, "application/json", json_success(p)).await;
        let id = source_provider(&h, "pi", SourceChannel::Codex, p, &remote.origin, false);
        let policy = settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![settings::ModelRoutingRule {
                source_model: MODEL.into(),
                target_model: Some("remote-model".into()),
                reasoning_effort: None,
            }],
        };
        h.db.open_connection()
            .unwrap()
            .execute(
                "UPDATE providers SET model_routing_policy_json=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&policy).unwrap(), id],
            )
            .unwrap();
        declare(&h, "pi", id, p);
        declare(&h, "omp", id, p);
        let (_, _, pi_binding) =
            publish(&h, "pi", SourceChannel::Codex, p, "http://127.0.0.1:3711");
        let (_, _, omp_binding) =
            publish(&h, "omp", SourceChannel::Codex, p, "http://127.0.0.1:3711");
        let grok = source_provider(&h, "pi", SourceChannel::Grok, p, &remote.origin, false);
        let (_, _, grok_binding) =
            publish(&h, "pi", SourceChannel::Grok, p, "http://127.0.0.1:3711");
        let mut sessions = std::collections::BTreeSet::new();
        for (consumer, binding, source_id) in [
            ("pi", &pi_binding, id),
            ("omp", &omp_binding, id),
            ("pi", &grok_binding, grok),
        ] {
            let mut req = request(consumer, p, binding, false);
            req.headers_mut()
                .insert("x-session-id", "same-original-session".parse().unwrap());
            let (status, _, log) = send(&mut h, req).await;
            assert_eq!(status, StatusCode::OK);
            let session = log.session_id.clone().unwrap();
            assert_ne!(session, "same-original-session");
            assert!(sessions.insert(session));
            assert_eq!(attempts(&log)[0]["provider_id"], source_id);
        }
        h.db.open_connection()
            .unwrap()
            .execute(
                "UPDATE providers SET api_key_plaintext='rotated-source' WHERE id=?1",
                [id],
            )
            .unwrap();
        let (status, _, log) = send(&mut h, request("pi", p, &pi_binding, false)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(log.requested_model.as_deref(), Some(MODEL));
        {
            let captured = remote.captures.lock().unwrap();
            assert_eq!(captured.len(), 4);
            for index in [0, 1, 3] {
                let body: Value = serde_json::from_slice(&captured[index].body).unwrap();
                assert_eq!(body["model"], "remote-model");
            }
            assert!(captured[3].has_header_line("authorization: Bearer rotated-source"));
        }
        h.db.open_connection()
            .unwrap()
            .execute("DELETE FROM providers WHERE id=?1", [id])
            .unwrap();
        let (status, _, _) = send(&mut h, request("pi", p, &pi_binding, false)).await;
        assert!(!status.is_success());
        assert_eq!(remote.calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires installed Pi and OMP runtimes; explicit local-only integration matrix"]
    async fn native_channel_real_cli_ten_source_protocol_streams() {
        let _lock = crate::test_support::test_env_lock();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut results = Vec::new();
        for consumer in ["pi", "omp"] {
            for source in SourceChannel::ALL {
                for &protocol in source.protocols() {
                    let mut h = Harness::new();
                    let failed = upstream(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "application/json",
                        r#"{"error":{"message":"switch source upstream"}}"#.into(),
                    )
                    .await;
                    let success =
                        upstream(StatusCode::OK, "text/event-stream", sse_success(protocol)).await;
                    let first =
                        source_provider(&h, consumer, source, protocol, &failed.origin, false);
                    let second =
                        source_provider(&h, consumer, source, protocol, &success.origin, false);
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let origin = format!("http://{}", listener.local_addr().unwrap());
                    let (_, entry, binding) = publish(&h, consumer, source, protocol, &origin);
                    let path=root.join(format!(".trellis/.runtime/research/omp-pi/channel-test/entry-{consumer}-{}-{}.json",source.as_str(),protocol.as_str()));
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(&path, serde_json::to_vec_pretty(&entry).unwrap()).unwrap();
                    let router = build_router(h.state.clone());
                    let server =
                        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
                    let output = tokio::time::timeout(
                        Duration::from_secs(60),
                        tokio::process::Command::new("node")
                            .current_dir(root)
                            .arg("scripts/pi-omp-gateway-client.mjs")
                            .args([
                                "--client",
                                consumer,
                                "--protocol",
                                protocol.as_str(),
                                "--base-origin",
                                &origin,
                                "--model",
                                MODEL,
                                "--target",
                                ".trellis/.runtime/research/omp-pi/channel-test",
                                "--expect-text",
                                "W0_LOCAL_OK",
                                "--entry-json",
                                path.to_str().unwrap(),
                            ])
                            .kill_on_drop(true)
                            .output(),
                    )
                    .await
                    .expect("CLI timeout")
                    .expect("start CLI");
                    server.abort();
                    assert!(
                        output.status.success(),
                        "{consumer}/{source:?}/{protocol:?} stdout={} stderr={}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    let mut result: Value = serde_json::from_slice(&output.stdout).unwrap();
                    assert_eq!(result["passed"], true);
                    let log = recv_terminal_request_log(&mut h.logs).await;
                    assert_usage(&log, consumer, protocol);
                    assert_binding_log(&log, consumer, source, &binding);
                    assert_eq!(
                        attempts(&log)
                            .iter()
                            .map(|v| v["provider_id"].as_i64().unwrap())
                            .collect::<Vec<_>>(),
                        vec![first, second]
                    );
                    result["sourceChannel"] = serde_json::json!(source);
                    result["bindingId"] = serde_json::json!(binding);
                    results.push(result);
                }
            }
        }
        let report = root.join(".trellis/.runtime/research/omp-pi/channel-test/results.json");
        std::fs::write(&report, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
        println!(
            "10/10 real CLI channel streams with source failover: {}",
            report.display()
        );
    }
}
