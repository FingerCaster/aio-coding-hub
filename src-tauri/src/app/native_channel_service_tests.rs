use super::*;
use crate::domain::native_cli::{NativeClient, NativeTargetSelection};
use crate::domain::native_gateway::{self, Manifest, ManifestPayload, ManifestVersion};
use rusqlite::params;
use serde_json::{json, Value};
const ORIGIN: &str = "http://127.0.0.1:3711";
fn policy() -> String {
    serde_json::to_string(&crate::settings::ModelRoutingPolicy::default()).unwrap()
}
fn model(id: &str) -> NativeModelSpec {
    serde_json::from_value(json!({"requestModelId":id,"displayName":id,"input":["text","image"],"contextWindow":64000,"maxTokens":4000,"reasoning":false,"thinking":null,"supportsTools":true})).unwrap()
}
fn provider(db: &db::Db, consumer: &str, source: SourceChannel, protocol: GatewayProtocol) -> i64 {
    let mut conn = db.open_connection().unwrap();
    let uuid = crate::shared::uuid::new_uuid_v4();
    conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,created_at,updated_at) VALUES (?1,?2,?1,'https://upstream.test','[\"https://upstream.test\"]','do-not-copy-real-key',1,'api_key',1,1)",params![uuid,source.as_str()]).unwrap();
    let id = conn.last_insert_rowid();
    let tx = conn.transaction().unwrap();
    let old = channel_models_get(&tx, id, &uuid, consumer, protocol).unwrap();
    channel_models_set(
        &tx,
        id,
        &uuid,
        consumer,
        protocol,
        &old.revision,
        &[model("public-model")],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(conn);
    let mut ids = crate::providers::list_enabled_for_gateway_using_active_mode(db, source.as_str())
        .unwrap()
        .providers
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    if !ids.contains(&id) {
        ids.push(id)
    }
    crate::providers::default_route_set_order(db, source.as_str(), ids).unwrap();
    id
}
fn fixture(client: NativeClient) -> (tempfile::TempDir, db::Db, NativeTarget) {
    let home = tempfile::tempdir().unwrap();
    let db = db::init_for_tests(&home.path().join("aio.db")).unwrap();
    let target = native_cli::targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(client),
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    std::fs::write(
        &target.models_path,
        br#"{"providers":{"original":{"apiKey":"native-secret"}},"defaultModel":"untouched"}"#,
    )
    .unwrap();
    (home, db, target)
}
fn catalog(db: &db::Db, t: &NativeTarget) -> ChannelCatalog {
    catalog_for_target(db, t, Some(ORIGIN), &policy(), || Ok(())).unwrap()
}
fn input(
    c: &ChannelCatalog,
    source: SourceChannel,
    protocol: GatewayProtocol,
) -> ChannelLifecycleInput {
    ChannelLifecycleInput {
        target_id: c.target_id.clone(),
        expected_revision: c.revision.clone(),
        catalog_revision: c.catalog_revision.clone(),
        selections: vec![ChannelSelection {
            source_channel: source,
            protocol,
            model_ids: vec!["public-model".into()],
        }],
        remove_binding_ids: vec![],
    }
}
fn apply(
    db: &db::Db,
    t: &NativeTarget,
    i: &ChannelLifecycleInput,
) -> AppResult<ChannelMutationResult> {
    apply_for_target(db, t, i, Some(ORIGIN), &policy(), || Ok(()))
}
fn doc(t: &NativeTarget) -> Value {
    let bytes = std::fs::read(&t.models_path).unwrap();
    match t.client {
        NativeClient::Pi => serde_json::from_slice(&bytes).unwrap(),
        NativeClient::Omp => serde_yaml::from_slice(&bytes).unwrap(),
    }
}

#[test]
fn channel_incremental_publication_coexists_and_withdraws_exactly_for_both_clients() {
    for client in [NativeClient::Pi, NativeClient::Omp] {
        let (_home, db, t) = fixture(client);
        for source in SourceChannel::ALL {
            for &protocol in source.protocols() {
                provider(&db, client.as_str(), source, protocol);
            }
        }
        let initial = catalog(&db, &t);
        assert_eq!(initial.sources.len(), 5);
        let first = input(
            &initial,
            SourceChannel::Codex,
            GatewayProtocol::OpenaiResponses,
        );
        let before = std::fs::read(&t.models_path).unwrap();
        let preview =
            preview_for_target(&db, &t, &first, Some(ORIGIN), &policy(), || Ok(())).unwrap();
        assert_eq!(std::fs::read(&t.models_path).unwrap(), before);
        let serialized = serde_json::to_string(&preview).unwrap();
        assert!(!serialized.contains("do-not-copy"));
        assert!(!serialized.contains("upstream.test"));
        let result = apply(&db, &t, &first).unwrap();
        assert!(result.changed);
        assert!(result.backup_path.is_some());
        let codex_id = result.bindings[0].binding_id.clone();
        let codex_node = doc(&t)["providers"][&result.bindings[0].native_key].clone();
        assert!(codex_node["baseUrl"]
            .as_str()
            .unwrap()
            .contains(&format!("/_aio/channel/{codex_id}")));
        if client == NativeClient::Omp {
            assert_eq!(codex_node["auth"], "apiKey");
        }
        let c = catalog(&db, &t);
        assert!(
            !apply(
                &db,
                &t,
                &input(&c, SourceChannel::Codex, GatewayProtocol::OpenaiResponses)
            )
            .unwrap()
            .changed
        );
        let c = catalog(&db, &t);
        let mut more = input(&c, SourceChannel::Grok, GatewayProtocol::OpenaiResponses);
        more.selections.push(ChannelSelection {
            source_channel: SourceChannel::Claude,
            protocol: GatewayProtocol::AnthropicMessages,
            model_ids: vec!["public-model".into()],
        });
        let result = apply(&db, &t, &more).unwrap();
        assert_eq!(result.bindings.len(), 3);
        assert_eq!(
            doc(&t)["providers"]["aio-channel-codex-openai-responses"],
            codex_node
        );
        assert_eq!(doc(&t)["defaultModel"], "untouched");
        assert_eq!(doc(&t)["providers"]["original"]["apiKey"], "native-secret");
        assert_eq!(
            native_gateway::managed_native_keys(&db, &t.target_id)
                .unwrap()
                .len(),
            3
        );
        let c = catalog(&db, &t);
        let remove = ChannelLifecycleInput {
            target_id: t.target_id.clone(),
            expected_revision: c.revision,
            catalog_revision: c.catalog_revision,
            selections: vec![],
            remove_binding_ids: vec![codex_id.clone()],
        };
        let result = apply_for_target(
            &db,
            &t,
            &remove,
            None,
            "invalid policy still permits removal",
            || Ok(()),
        )
        .unwrap();
        assert_eq!(result.bindings.len(), 2);
        assert!(doc(&t)["providers"]
            .get("aio-channel-codex-openai-responses")
            .is_none());
        let conn = db.open_connection().unwrap();
        assert!(load_binding(&conn, &codex_id, client.as_str()).is_err());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM native_gateway_manifests", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        drop(conn);
        let mut repeated = remove;
        repeated.expected_revision = result.revision;
        assert!(!apply(&db, &t, &repeated).unwrap().changed);
    }
}

#[test]
fn channel_models_are_cas_bound_to_source_config_not_credentials_or_display_name() {
    let (_home, db, t) = fixture(NativeClient::Pi);
    let id = provider(
        &db,
        "pi",
        SourceChannel::Codex,
        GatewayProtocol::OpenaiResponses,
    );
    let conn = db.open_connection().unwrap();
    let uuid: String = conn
        .query_row(
            "SELECT provider_uuid FROM providers WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let old = channel_models_get(&conn, id, &uuid, "pi", GatewayProtocol::OpenaiResponses).unwrap();
    conn.execute(
        "UPDATE providers SET api_key_plaintext='rotated',name='Renamed' WHERE id=?1",
        [id],
    )
    .unwrap();
    assert_eq!(
        old.revision,
        channel_models_get(&conn, id, &uuid, "pi", GatewayProtocol::OpenaiResponses)
            .unwrap()
            .revision
    );
    conn.execute(
        "UPDATE providers SET base_urls_json='[\"https://different.test\"]' WHERE id=?1",
        [id],
    )
    .unwrap();
    let fresh =
        channel_models_get(&conn, id, &uuid, "pi", GatewayProtocol::OpenaiResponses).unwrap();
    assert!(fresh.stale);
    assert_ne!(fresh.revision, old.revision);
    assert!(channel_models_set(
        &conn,
        id,
        &uuid,
        "pi",
        GatewayProtocol::OpenaiResponses,
        &old.revision,
        &[model("new")]
    )
    .is_err());
    drop(conn);
    assert!(catalog(&db, &t)
        .sources
        .iter()
        .find(|s| s.source_channel == SourceChannel::Codex)
        .unwrap()
        .models
        .is_empty());
}

#[test]
fn channel_runtime_enforces_current_pool_identity_whitelist_and_capability_coverage() {
    let (_home, db, t) = fixture(NativeClient::Pi);
    let protocol = GatewayProtocol::OpenaiResponses;
    let first = provider(&db, "pi", SourceChannel::Codex, protocol);
    let other = provider(&db, "pi", SourceChannel::Grok, protocol);
    let result = apply(
        &db,
        &t,
        &input(&catalog(&db, &t), SourceChannel::Codex, protocol),
    )
    .unwrap();
    let binding = &result.bindings[0].binding_id;
    let p = crate::settings::ModelRoutingPolicy::default();
    let allowed = |id, model| {
        channel_candidate_eligible(&db.open_connection().unwrap(), binding, "pi", id, model, &p)
            .unwrap()
    };
    assert!(allowed(first, "public-model"));
    assert!(!allowed(other, "public-model"));
    assert!(!allowed(first, "unpublished"));
    assert!(load_binding(&db.open_connection().unwrap(), binding, "omp").is_err());
    let second = provider(&db, "pi", SourceChannel::Codex, protocol);
    crate::providers::default_route_set_order(&db, "codex", vec![second]).unwrap();
    assert!(!allowed(first, "public-model"));
    assert!(allowed(second, "public-model"));
    let conn = db.open_connection().unwrap();
    let uuid: String = conn
        .query_row(
            "SELECT provider_uuid FROM providers WHERE id=?1",
            [second],
            |r| r.get(0),
        )
        .unwrap();
    let current = channel_models_get(&conn, second, &uuid, "pi", protocol).unwrap();
    let mut weak = model("public-model");
    weak.context_window = 32000;
    channel_models_set(
        &conn,
        second,
        &uuid,
        "pi",
        protocol,
        &current.revision,
        &[weak],
    )
    .unwrap();
    drop(conn);
    assert!(!allowed(second, "public-model"));
}

#[test]
fn channel_conflicts_and_finalize_failure_leave_foreign_nodes_untouched() {
    let (_home, db, t) = fixture(NativeClient::Pi);
    let p = GatewayProtocol::OpenaiResponses;
    provider(&db, "pi", SourceChannel::Codex, p);
    let before = doc(&t);
    let c = catalog(&db, &t);
    db.open_connection().unwrap().execute_batch("CREATE TRIGGER fail_channel BEFORE UPDATE ON native_channel_bindings WHEN json_extract(NEW.manifest_json,'$.state')='applied' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(apply(&db, &t, &input(&c, SourceChannel::Codex, p)).is_err());
    assert_eq!(doc(&t), before);
    db.open_connection()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_channel")
        .unwrap();
    let c = catalog(&db, &t);
    let result = apply(&db, &t, &input(&c, SourceChannel::Codex, p)).unwrap();
    assert!(apply(&db, &t, &input(&c, SourceChannel::Codex, p)).is_err());
    let key = &result.bindings[0].native_key;
    let mut foreign = doc(&t);
    foreign["providers"][key]["baseUrl"] = json!("https://edited.test");
    std::fs::write(&t.models_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let c = catalog(&db, &t);
    assert!(c.bindings[0].modified);
    assert!(apply(&db, &t, &input(&c, SourceChannel::Codex, p)).is_err());
    assert_eq!(doc(&t), foreign);
}

#[test]
fn channel_pending_apply_and_remove_recover_by_exact_owned_node_digest() {
    let (_home, db, t) = fixture(NativeClient::Pi);
    let p = GatewayProtocol::OpenaiResponses;
    provider(&db, "pi", SourceChannel::Codex, p);
    let c = catalog(&db, &t);
    let i = input(&c, SourceChannel::Codex, p);
    let conn = db.open_connection().unwrap();
    let plan = channel_plan(&conn, "pi", &i, Some(ORIGIN), &policy()).unwrap();
    let (identity, entry) = plan.desired[0].clone();
    let intent = Manifest {
        channel: Some(identity.clone()),
        target_id: t.target_id.clone(),
        cli_key: "pi".into(),
        protocol: p,
        native_key: entry.native_key.clone(),
        node_digest: node_digest(&entry.node),
        catalog_revision: plan.revision.clone(),
        generation: 1,
        state: "intent_apply".into(),
        payload: ManifestPayload {
            desired: Some(entry.clone()),
            previous: None,
            policy_hash: native_gateway::hash(policy()),
        },
    };
    put_binding(&conn, &intent).unwrap();
    drop(conn);
    let pending = catalog(&db, &t);
    assert!(!pending.bindings[0].modified);
    preview_for_target(
        &db,
        &t,
        &input(&pending, SourceChannel::Codex, p),
        Some(ORIGIN),
        &policy(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        list_bindings(&db.open_connection().unwrap(), &t.target_id).unwrap()[0].state,
        "intent_apply"
    );
    let mut d = doc(&t);
    d["providers"][&entry.native_key] = entry.node.clone();
    std::fs::write(&t.models_path, serde_json::to_vec(&d).unwrap()).unwrap();
    let c = catalog(&db, &t);
    assert_eq!(c.bindings[0].state, "intent_apply");
    preview_for_target(
        &db,
        &t,
        &input(&c, SourceChannel::Codex, p),
        Some(ORIGIN),
        &policy(),
        || Ok(()),
    )
    .unwrap();
    assert!(
        apply(&db, &t, &input(&c, SourceChannel::Codex, p))
            .unwrap()
            .changed
    );
    let conn = db.open_connection().unwrap();
    let applied = load_binding(&conn, &identity.binding_id, "pi").unwrap();
    let remove = Manifest {
        state: "intent_remove".into(),
        payload: ManifestPayload {
            desired: None,
            previous: Some(ManifestVersion {
                entry: entry.clone(),
                catalog_revision: applied.catalog_revision.clone(),
                generation: 1,
                policy_hash: native_gateway::hash(policy()),
            }),
            policy_hash: native_gateway::hash(policy()),
        },
        ..applied
    };
    put_binding(&conn, &remove).unwrap();
    drop(conn);
    d["providers"]
        .as_object_mut()
        .unwrap()
        .remove(&entry.native_key);
    std::fs::write(&t.models_path, serde_json::to_vec(&d).unwrap()).unwrap();
    let c = catalog(&db, &t);
    assert!(!c.bindings[0].modified);
    let i = ChannelLifecycleInput {
        target_id: t.target_id.clone(),
        expected_revision: c.revision,
        catalog_revision: c.catalog_revision,
        selections: vec![],
        remove_binding_ids: vec![identity.binding_id],
    };
    preview_for_target(&db, &t, &i, None, &policy(), || Ok(())).unwrap();
    assert_eq!(
        list_bindings(&db.open_connection().unwrap(), &t.target_id).unwrap()[0].state,
        "intent_remove"
    );
    assert!(apply(&db, &t, &i).unwrap().bindings.is_empty());
    assert_eq!(doc(&t), d);
}

#[test]
fn channel_gemini_standard_api_is_available_but_unverified_oauth_is_blocked() {
    let (_home, db, t) = fixture(NativeClient::Pi);
    let p = GatewayProtocol::GoogleGenerativeAi;
    let id = provider(&db, "pi", SourceChannel::Gemini, p);
    let published = apply(&db, &t, &input(&catalog(&db, &t), SourceChannel::Gemini, p)).unwrap();
    let binding_id = &published.bindings[0].binding_id;
    assert!(!catalog(&db, &t)
        .sources
        .into_iter()
        .find(|s| s.source_channel == SourceChannel::Gemini)
        .unwrap()
        .models
        .is_empty());
    db.open_connection().unwrap().execute("UPDATE providers SET auth_mode='oauth',oauth_provider_type='gemini_oauth',oauth_access_token='unverified-token' WHERE id=?1",[id]).unwrap();
    let c = catalog(&db, &t);
    let source = c
        .sources
        .iter()
        .find(|s| s.source_channel == SourceChannel::Gemini)
        .unwrap();
    assert!(source.models.is_empty());
    assert!(source.providers[0]
        .blocked_reason
        .as_ref()
        .unwrap()
        .contains("2026-06-18"));
    assert!(apply(&db, &t, &input(&c, SourceChannel::Gemini, p)).is_err());
    assert!(!channel_candidate_eligible(
        &db.open_connection().unwrap(),
        binding_id,
        "pi",
        id,
        "public-model",
        &crate::settings::ModelRoutingPolicy::default()
    )
    .unwrap());
    // A mixed pool keeps the standard API candidate. No token/project/email is
    // accepted as proof of a Standard/Enterprise subscription.
    let api_id = provider(&db, "pi", SourceChannel::Gemini, p);
    let mixed = catalog(&db, &t);
    let source = mixed
        .sources
        .iter()
        .find(|s| s.source_channel == SourceChannel::Gemini)
        .unwrap();
    assert_eq!(source.models.len(), 1);
    assert_eq!(
        source
            .providers
            .iter()
            .filter(|p| p.blocked_reason.is_none())
            .count(),
        1
    );
    assert!(channel_candidate_eligible(
        &db.open_connection().unwrap(),
        binding_id,
        "pi",
        api_id,
        "public-model",
        &crate::settings::ModelRoutingPolicy::default()
    )
    .unwrap());
}
