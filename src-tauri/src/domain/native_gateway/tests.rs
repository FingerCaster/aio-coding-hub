use super::*;
use crate::shared::gateway_protocol::GatewayProtocol;
use rusqlite::params;
use serde_json::json;
fn identity(db: &crate::db::Db, id: i64) -> String {
    db.open_connection()
        .unwrap()
        .query_row(
            "SELECT provider_uuid FROM providers WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}
fn models_set(
    db: &crate::db::Db,
    id: i64,
    models: Vec<NativeModelSpec>,
) -> crate::shared::error::AppResult<GatewayModelsSnapshot> {
    let uuid = identity(db, id);
    let snapshot = super::models_get(db, id, &uuid)?;
    super::models_set(db, id, &uuid, &snapshot.revision, models)
}
fn models_get(
    db: &crate::db::Db,
    id: i64,
) -> crate::shared::error::AppResult<Vec<NativeModelSpec>> {
    Ok(super::models_get(db, id, &identity(db, id))?.models)
}
fn candidate_eligible(
    conn: &rusqlite::Connection,
    id: i64,
    client: &str,
    protocol: GatewayProtocol,
    model: &str,
) -> crate::shared::error::AppResult<bool> {
    super::candidate_eligible(
        conn,
        id,
        client,
        protocol,
        model,
        &crate::settings::ModelRoutingPolicy::default(),
    )
}
fn policy_hash() -> String {
    hash(serde_json::to_vec(&crate::settings::ModelRoutingPolicy::default()).unwrap())
}

fn fixture() -> (tempfile::TempDir, crate::db::Db, i64) {
    let home = tempfile::tempdir().unwrap();
    let db = crate::db::init_for_tests(&home.path().join("aio.db")).unwrap();
    let conn = db.open_connection().unwrap();
    conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,gateway_protocol,created_at,updated_at) VALUES (?1,'pi','P','https://example.test','[\"https://example.test\"]','secret',1,'api_key','openai-responses',1,1)",[crate::shared::uuid::new_uuid_v4()]).unwrap();
    let id = conn.last_insert_rowid();
    drop(conn);
    crate::providers::default_route_set_order(&db, "pi", vec![id]).unwrap();
    (home, db, id)
}
fn model(context: u32) -> NativeModelSpec {
    serde_json::from_value(json!({"requestModelId":"public-model","displayName":"Explicit","input":["text"],"contextWindow":context,"maxTokens":1000,"reasoning":false,"thinking":null,"supportsTools":true})).unwrap()
}
fn publish(conn: &rusqlite::Connection, target: &str, spec: NativeModelSpec) {
    let entry = generate_entries(
        "pi",
        "http://127.0.0.1:1234",
        &[GatewayCatalogGroup {
            protocol: GatewayProtocol::OpenaiResponses,
            models: vec![spec],
        }],
    )
    .unwrap()
    .remove(0);
    put_manifest(
        conn,
        &Manifest::applied(
            target,
            "pi",
            ManifestVersion {
                entry,
                catalog_revision: "rev".into(),
                generation: 1,
                policy_hash: policy_hash(),
            },
        ),
    )
    .unwrap();
}
#[test]
fn eligibility_requires_every_published_target_and_current_binding() {
    let (_home, db, id) = fixture();
    models_set(&db, id, vec![model(64000)]).unwrap();
    let conn = db.open_connection().unwrap();
    let check = || {
        candidate_eligible(
            &conn,
            id,
            "pi",
            GatewayProtocol::OpenaiResponses,
            "public-model",
        )
        .unwrap()
    };
    assert!(!check());
    publish(&conn, "target-a", model(32000));
    assert!(check());
    publish(&conn, "target-b", model(128000));
    assert!(!check());
    delete_manifest(&conn, "target-b", GatewayProtocol::OpenaiResponses).unwrap();
    assert!(check());
    assert!(!candidate_eligible(
        &conn,
        id,
        "omp",
        GatewayProtocol::OpenaiResponses,
        "public-model"
    )
    .unwrap());
    assert!(!candidate_eligible(
        &conn,
        id,
        "pi",
        GatewayProtocol::AnthropicMessages,
        "public-model"
    )
    .unwrap());
    conn.execute(
        "UPDATE providers SET base_urls_json='[\"https://changed.test\"]' WHERE id=?1",
        [id],
    )
    .unwrap();
    assert!(!check());
}
#[test]
fn model_changes_fail_closed_and_catalog_intersects_without_mapping_inference() {
    let (_home, db, id) = fixture();
    models_set(&db, id, vec![model(64000)]).unwrap();
    let conn = db.open_connection().unwrap();
    conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,gateway_protocol,created_at,updated_at) SELECT ?1,cli_key,'Second',base_url,base_urls_json,api_key_plaintext,1,auth_mode,gateway_protocol,1,1 FROM providers WHERE id=?2",params![crate::shared::uuid::new_uuid_v4(),id]).unwrap();
    let second = conn.last_insert_rowid();
    super::catalog::test_set_models(&conn, second, &[model(32000)]).unwrap();
    drop(conn);
    crate::providers::default_route_set_order(&db, "pi", vec![id, second]).unwrap();
    let conn = db.open_connection().unwrap();
    let (revision, groups) = catalog(&conn, "pi", Some("http://localhost:1"), "{}").unwrap();
    assert_eq!(groups[0].models[0].context_window, 32000);
    assert_eq!(groups[0].models[0].request_model_id, "public-model");
    assert_ne!(
        revision,
        catalog(&conn, "pi", Some("http://localhost:2"), "{}")
            .unwrap()
            .0
    );
    publish(&conn, "target", model(32000));
    super::catalog::test_set_models(&conn, id, &[model(16000)]).unwrap();
    assert!(!candidate_eligible(
        &conn,
        id,
        "pi",
        GatewayProtocol::OpenaiResponses,
        "public-model"
    )
    .unwrap());
}
#[test]
fn import_is_atomic_disabled_and_never_copies_source_secret() {
    let (_home, db, _) = fixture();
    let node = json!({"api":"openai-responses","baseUrl":"https://remote.test/v1","apiKey":"!echo native-secret","models":[{"id":"m","name":"M","input":["text"],"contextWindow":16000,"maxTokens":1000,"reasoning":false}]});
    let preview = preview_import("pi", "target", "native", "revision", &node).unwrap();
    let mut input:GatewayImportConfirmInput=serde_json::from_value(json!({"targetId":"target","nativeKey":"native","expectedRevision":"revision","credentials":[{"groupId":preview.groups[0].group_id,"apiKey":"aio-user-secret"}]})).unwrap();
    let imported = import_confirm(&db, "pi", &preview, &input).unwrap();
    assert_eq!(imported.len(), 1);
    assert!(!imported[0].enabled);
    assert_eq!(
        models_get(&db, imported[0].id).unwrap()[0].request_model_id,
        "m"
    );
    let conn = db.open_connection().unwrap();
    let key: String = conn
        .query_row(
            "SELECT api_key_plaintext FROM providers WHERE id=?1",
            [imported[0].id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(key, "aio-user-secret");
    input.expected_revision = "stale".into();
    assert!(import_confirm(&db, "pi", &preview, &input).is_err());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM providers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}
#[test]
fn import_transaction_failure_leaves_no_partial_provider() {
    let (_home, db, _) = fixture();
    let node = json!({"api":"openai-responses","baseUrl":"https://remote.test/v1","models":[{"id":"m","input":["text"],"contextWindow":10000,"maxTokens":1000,"reasoning":false}]});
    let preview = preview_import("pi", "target", "native", "rev", &node).unwrap();
    let input=serde_json::from_value(json!({"targetId":"target","nativeKey":"native","expectedRevision":"rev","credentials":[{"groupId":preview.groups[0].group_id,"apiKey":"explicit-key"}]})).unwrap();
    let conn = db.open_connection().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_spec BEFORE INSERT ON native_gateway_model_specs BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(conn);
    assert!(import_confirm(&db, "pi", &preview, &input).is_err());
    let conn = db.open_connection().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM providers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn model_edit_rejects_replaced_identity_and_stale_capability_revision() {
    let (_home, db, id) = fixture();
    let uuid = identity(&db, id);
    let first = super::models_get(&db, id, &uuid).unwrap();
    super::models_set(&db, id, &uuid, &first.revision, vec![model(64000)]).unwrap();
    assert!(super::models_set(&db, id, &uuid, &first.revision, vec![model(32000)]).is_err());
    assert!(super::models_get(&db, id, "replaced-identity").is_err());
    assert_eq!(models_get(&db, id).unwrap()[0].context_window, 64000);
}

#[test]
fn global_policy_change_cannot_reuse_old_publication() {
    let (_home, db, id) = fixture();
    models_set(&db, id, vec![model(64000)]).unwrap();
    let conn = db.open_connection().unwrap();
    publish(&conn, "target", model(32000));
    let mut manifests = list_manifests(&conn, "target").unwrap();
    manifests[0].payload.policy_hash = hash("different-policy");
    put_manifest(&conn, &manifests[0]).unwrap();
    assert!(!candidate_eligible(
        &conn,
        id,
        "pi",
        GatewayProtocol::OpenaiResponses,
        "public-model"
    )
    .unwrap());
}

#[test]
fn publishing_weaker_capability_requires_other_targets_to_be_updated_or_withdrawn() {
    let (_home, db, id) = fixture();
    models_set(&db, id, vec![model(32000)]).unwrap();
    let conn = db.open_connection().unwrap();
    publish(&conn, "old-target", model(64000));
    let groups = vec![GatewayCatalogGroup {
        protocol: GatewayProtocol::OpenaiResponses,
        models: vec![model(32000)],
    }];
    let policy = serde_json::to_string(&crate::settings::ModelRoutingPolicy::default()).unwrap();
    assert!(validate_publication(&conn, "different-target", "pi", &groups, &policy).is_err());
    assert!(validate_publication(&conn, "old-target", "pi", &groups, &policy).is_ok());
    delete_manifest(&conn, "old-target", GatewayProtocol::OpenaiResponses).unwrap();
    assert!(validate_publication(&conn, "different-target", "pi", &groups, &policy).is_ok());
}
