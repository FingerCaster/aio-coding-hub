use super::*;
use crate::domain::native_cli::{NativeClient, NativeTargetSelection};
use crate::shared::gateway_protocol::GatewayProtocol;
use rusqlite::params;
use serde_json::{json, Value};

const ORIGIN: &str = "http://127.0.0.1:3711";
fn policy() -> String {
    serde_json::to_string(&crate::settings::ModelRoutingPolicy::default()).unwrap()
}
fn fixture() -> (tempfile::TempDir, db::Db, NativeTarget, i64) {
    let home = tempfile::tempdir().unwrap();
    let db = db::init_for_tests(&home.path().join("aio.db")).unwrap();
    let target = native_cli::targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Pi),
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    std::fs::write(&target.models_path,br#"{"providers":{"original":{"apiKey":"never-copy-this-secret"}},"defaultModel":"leave-unchanged"}"#).unwrap();
    let conn = db.open_connection().unwrap();
    let uuid = crate::shared::uuid::new_uuid_v4();
    conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,gateway_protocol,created_at,updated_at) VALUES (?1,'pi','P','https://upstream.test','[\"https://upstream.test\"]','upstream-secret',1,'api_key','openai-responses',1,1)",[&uuid]).unwrap();
    let id = conn.last_insert_rowid();
    drop(conn);
    crate::providers::default_route_set_order(&db, "pi", vec![id]).unwrap();
    let model=serde_json::from_value(json!({"requestModelId":"public-model","displayName":"Explicit","input":["text","image"],"contextWindow":64000,"maxTokens":4000,"reasoning":false,"thinking":null,"supportsTools":true})).unwrap();
    let snapshot = models_get(&db, id, &uuid).unwrap();
    models_set(&db, id, &uuid, &snapshot.revision, vec![model]).unwrap();
    (home, db, target, id)
}
fn preview(db: &db::Db, target: &NativeTarget) -> GatewayCatalogPreview {
    preview_for_target(db, target, Some(ORIGIN), &policy()).unwrap()
}
fn input(preview: &GatewayCatalogPreview) -> GatewayLifecycleInput {
    GatewayLifecycleInput {
        target_id: preview.target_id.clone(),
        expected_revision: preview.revision.clone(),
        catalog_revision: preview.catalog_revision.clone(),
    }
}
fn document(target: &NativeTarget) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&target.models_path).unwrap()).unwrap()
}

#[test]
fn generated_lifecycle_is_idempotent_preserves_originals_and_revokes_candidates() {
    let (_home, db, target, id) = fixture();
    let initial = preview(&db, &target);
    assert_eq!(initial.entries.len(), 1);
    let applied = mutate_for_target(
        &db,
        &target,
        &input(&initial),
        Some(ORIGIN),
        &policy(),
        false,
    )
    .unwrap();
    assert!(applied.changed);
    let after = preview(&db, &target);
    let key = &after.entries[0].native_key;
    assert_eq!(document(&target)["defaultModel"], "leave-unchanged");
    assert_eq!(
        document(&target)["providers"]["original"]["apiKey"],
        "never-copy-this-secret"
    );
    let serialized = serde_json::to_string(&after).unwrap();
    assert!(!serialized.contains("upstream-secret"));
    assert!(!serialized.contains("upstream.test"));
    assert!(!serialized.contains("never-copy"));
    assert_eq!(
        document(&target)["providers"][key]["apiKey"],
        crate::cli_proxy::PLACEHOLDER_KEY
    );
    assert!(
        !mutate_for_target(&db, &target, &input(&after), Some(ORIGIN), &policy(), false)
            .unwrap()
            .changed
    );
    let conn = db.open_connection().unwrap();
    assert!(candidate_eligible(
        &conn,
        id,
        "pi",
        GatewayProtocol::OpenaiResponses,
        "public-model",
        &settings::ModelRoutingPolicy::default()
    )
    .unwrap());
    drop(conn);
    let removed = mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        true,
    )
    .unwrap();
    assert!(removed.manifests.is_empty());
    assert!(document(&target)["providers"].get(key).is_none());
    let conn = db.open_connection().unwrap();
    assert!(!candidate_eligible(
        &conn,
        id,
        "pi",
        GatewayProtocol::OpenaiResponses,
        "public-model",
        &settings::ModelRoutingPolicy::default()
    )
    .unwrap());
}

#[test]
fn exact_key_collision_external_edit_and_stale_revision_are_never_overwritten() {
    let (_home, db, target, _) = fixture();
    let first = preview(&db, &target);
    let key = &first.entries[0].native_key;
    let mut doc = document(&target);
    doc["providers"][key] = json!({"apiKey":"foreign"});
    std::fs::write(&target.models_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(
        mutate_for_target(&db, &target, &input(&first), Some(ORIGIN), &policy(), false).is_err()
    );
    let fresh = preview(&db, &target);
    assert!(
        mutate_for_target(&db, &target, &input(&fresh), Some(ORIGIN), &policy(), false).is_err()
    );
    assert_eq!(document(&target)["providers"][key]["apiKey"], "foreign");
    doc["providers"].as_object_mut().unwrap().remove(key);
    std::fs::write(&target.models_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        false,
    )
    .unwrap();
    let mut doc = document(&target);
    doc["providers"][key]["baseUrl"] = json!("https://foreign.test");
    std::fs::write(&target.models_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let edited = preview(&db, &target);
    assert!(edited.manifests[0].modified);
    assert!(
        mutate_for_target(&db, &target, &input(&edited), Some(ORIGIN), &policy(), true).is_err()
    );
    assert_eq!(
        document(&target)["providers"][key]["baseUrl"],
        "https://foreign.test"
    );
}

#[test]
fn db_failure_after_native_write_compensates_and_retry_recovers_intent() {
    let (_home, db, target, _) = fixture();
    let before = document(&target);
    let conn = db.open_connection().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_manifest BEFORE UPDATE ON native_gateway_manifests WHEN NEW.state='applied' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    drop(conn);
    assert!(mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        false
    )
    .is_err());
    assert_eq!(document(&target), before);
    db.open_connection()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_manifest")
        .unwrap();
    assert!(
        mutate_for_target(
            &db,
            &target,
            &input(&preview(&db, &target)),
            Some(ORIGIN),
            &policy(),
            false
        )
        .unwrap()
        .changed
    );
}

#[test]
fn interrupted_applied_intent_recovers_without_touching_unrelated_nodes() {
    let (_home, db, target, _) = fixture();
    let view = preview(&db, &target);
    let entry = view.entries[0].clone();
    let conn = db.open_connection().unwrap();
    let intent = Manifest {
        channel: None,
        target_id: target.target_id.clone(),
        cli_key: "pi".into(),
        protocol: entry.protocol,
        native_key: entry.native_key.clone(),
        node_digest: node_digest(&entry.node),
        catalog_revision: view.catalog_revision.clone(),
        generation: 1,
        state: "intent_apply".into(),
        payload: ManifestPayload {
            desired: Some(entry.clone()),
            previous: None,
            policy_hash: hash(policy()),
        },
    };
    put_manifest(&conn, &intent).unwrap();
    drop(conn);
    native_cli::with_target_lock(&target, |session| {
        session.patch(
            &view.revision,
            &[NativeNodePatch {
                native_key: entry.native_key.clone(),
                expected_digest: None,
                replacement: Some(entry.node.clone()),
            }],
        )
    })
    .unwrap();
    let mut doc = document(&target);
    doc["externalLater"] = json!(true);
    std::fs::write(&target.models_path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let result = mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        false,
    )
    .unwrap();
    assert_eq!(result.manifests[0].state, "applied");
    assert_eq!(document(&target)["externalLater"], true);
}

#[test]
fn publication_requires_ready_listener_and_current_catalog() {
    let (_home, db, target, id) = fixture();
    let view = preview(&db, &target);
    assert!(mutate_for_target(&db, &target, &input(&view), None, &policy(), false).is_err());
    let conn = db.open_connection().unwrap();
    conn.execute("UPDATE providers SET enabled=0 WHERE id=?1", params![id])
        .unwrap();
    drop(conn);
    assert!(
        mutate_for_target(&db, &target, &input(&view), Some(ORIGIN), &policy(), false).is_err()
    );
}

#[test]
fn listener_change_after_file_write_compensates_only_owned_nodes() {
    let (_home, db, target, _) = fixture();
    let before = document(&target);
    let view = preview(&db, &target);
    let calls = std::cell::Cell::new(0);
    let result = mutate_with_guard(
        &db,
        &target,
        &input(&view),
        Some(ORIGIN),
        &policy(),
        false,
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 3 {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_CATALOG_CONFLICT",
                    "injected listener restart",
                ));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(document(&target), before);
    assert!(managed_native_keys(&db, &target.target_id)
        .unwrap()
        .is_empty());
}

#[test]
fn compensation_conflict_keeps_intent_and_requires_exact_manual_repair() {
    let (_home, db, target, _) = fixture();
    let view = preview(&db, &target);
    let entry = view.entries[0].clone();
    let calls = std::cell::Cell::new(0);
    let result = mutate_with_guard(
        &db,
        &target,
        &input(&view),
        Some(ORIGIN),
        &policy(),
        false,
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 3 {
                let mut doc = document(&target);
                doc["providers"][&entry.native_key]["external"] = json!(true);
                std::fs::write(&target.models_path, serde_json::to_vec(&doc).unwrap()).unwrap();
                return Err(AppError::new(
                    "NATIVE_GATEWAY_DB_ERROR",
                    "injected finalize failure",
                ));
            }
            Ok(())
        },
    );
    assert_eq!(
        result.err().unwrap().code(),
        "NATIVE_GATEWAY_RECOVERY_REQUIRED"
    );
    assert_eq!(
        managed_native_keys(&db, &target.target_id).unwrap(),
        vec![entry.native_key.clone()]
    );
    assert!(mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        false
    )
    .is_err());
    assert_eq!(
        document(&target)["providers"][&entry.native_key]["external"],
        true
    );
    let mut repaired = document(&target);
    repaired["providers"][&entry.native_key] = entry.node;
    std::fs::write(&target.models_path, serde_json::to_vec(&repaired).unwrap()).unwrap();
    let result = mutate_for_target(
        &db,
        &target,
        &input(&preview(&db, &target)),
        Some(ORIGIN),
        &policy(),
        false,
    )
    .unwrap();
    assert_eq!(result.manifests[0].state, "applied");
}

#[test]
fn unreadable_native_file_does_not_prepare_a_manifest() {
    let (_home, db, target, _) = fixture();
    let view = preview(&db, &target);
    std::fs::remove_file(&target.models_path).unwrap();
    std::fs::create_dir(&target.models_path).unwrap();
    assert!(
        mutate_for_target(&db, &target, &input(&view), Some(ORIGIN), &policy(), false).is_err()
    );
    assert!(managed_native_keys(&db, &target.target_id)
        .unwrap()
        .is_empty());
}
