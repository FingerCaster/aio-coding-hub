//! Integration tests use the real AIO DB and isolated native directories.
use super::*;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, db::Db, NativeTarget) {
    let home = tempfile::tempdir().unwrap();
    let db = db::init_for_tests(&home.path().join("aio.db")).unwrap();
    let target = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Pi),
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    std::fs::write(&target.models_path,br#"{"providers":{"anthropic":{"apiKey":"literal-secret","future":{"keep":true}}},"defaultModel":"untouched"}"#).unwrap();
    (home, db, target)
}
fn action_input(list: &NativeProvidersList, key: &str) -> NativeProviderActionInput {
    let profile = list.providers.iter().find(|p| p.native_key == key).unwrap();
    NativeProviderActionInput {
        target_id: list.target.target_id.clone(),
        native_key: key.into(),
        expected_revision: list.revision.clone().unwrap(),
        expected_node_digest: profile.node_digest.clone(),
        expected_profile_revision: profile.profile_revision.clone(),
    }
}
fn save_input(
    list: &NativeProvidersList,
    key: &str,
    node: Value,
    apply: bool,
) -> NativeProviderSaveInput {
    let profile = list.providers.iter().find(|p| p.native_key == key);
    NativeProviderSaveInput {
        target_id: list.target.target_id.clone(),
        native_key: key.into(),
        display_name: key.into(),
        expected_revision: list.revision.clone().unwrap(),
        expected_node_digest: profile.and_then(|p| p.node_digest.clone()),
        expected_profile_revision: profile.and_then(|p| p.profile_revision.clone()),
        node: Some(node),
        patch: Vec::new(),
        apply,
    }
}
fn read_node(target: &NativeTarget, key: &str) -> Option<Value> {
    native_cli::with_target_lock(target, |session| {
        Ok(session.snapshot()?.providers().get(key).cloned())
    })
    .unwrap()
}

#[test]
fn complete_native_archive_create_apply_remove_delete_lifecycle() {
    let (_home, db, target) = fixture();
    let list = list_for_target(&db, &target).unwrap();
    assert_eq!(list.providers.len(), 1);
    assert_eq!(list.providers[0].native_key, "anthropic");
    let safe_json = serde_json::to_string(&list).unwrap();
    assert!(!safe_json.contains("literal-secret"));
    assert!(!safe_json.contains("future"));
    let draft = json!({"api":"native-extension","apiKey":"!never-executed","models":[{"id":"m","future":true}]});
    let saved = save_for_target(
        &db,
        &target,
        save_input(&list, "draft", draft.clone(), false),
    )
    .unwrap();
    assert_eq!(saved.provider.unwrap().state, NativeProviderState::Archived);
    assert!(read_node(&target, "draft").is_none());
    let list = list_for_target(&db, &target).unwrap();
    let applied = action_for_target(
        &db,
        &target,
        action_input(&list, "draft"),
        NativeAction::Apply,
    )
    .unwrap();
    assert_eq!(
        applied.provider.unwrap().state,
        NativeProviderState::Present
    );
    assert_eq!(read_node(&target, "draft"), Some(draft.clone()));
    let list = list_for_target(&db, &target).unwrap();
    let noop = action_for_target(
        &db,
        &target,
        action_input(&list, "draft"),
        NativeAction::Apply,
    )
    .unwrap();
    assert!(!noop.changed);
    let removed = action_for_target(
        &db,
        &target,
        action_input(&list, "draft"),
        NativeAction::Remove,
    )
    .unwrap();
    assert_eq!(
        removed.provider.unwrap().state,
        NativeProviderState::Archived
    );
    assert!(read_node(&target, "draft").is_none());
    let list = list_for_target(&db, &target).unwrap();
    assert!(list
        .providers
        .iter()
        .find(|p| p.native_key == "draft")
        .unwrap()
        .node_digest
        .is_none());
    action_for_target(
        &db,
        &target,
        action_input(&list, "draft"),
        NativeAction::Delete {
            remove_native: false,
        },
    )
    .unwrap();
    assert_eq!(list_for_target(&db, &target).unwrap().providers.len(), 1);
    assert_eq!(
        read_node(&target, "anthropic").unwrap()["future"]["keep"],
        true
    );
}

#[test]
fn omp_archive_only_then_yaml_apply_preserves_native_extensions() {
    let home = tempfile::tempdir().unwrap();
    let db = db::init_for_tests(&home.path().join("aio.db")).unwrap();
    let target = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Omp),
        None,
        None,
    )
    .unwrap();
    let list = list_for_target(&db, &target).unwrap();
    assert_eq!(list.parse_status, NativeParseStatus::Missing);
    let node = json!({"api":"google-vertex","auth":"none","baseUrl":"https://example.invalid","models":[{"id":"m","thinking":{"mode":"budget","efforts":["low","high"]},"future":{"keep":true}}],"futureProvider":"kept"});
    save_for_target(
        &db,
        &target,
        save_input(&list, "native", node.clone(), false),
    )
    .unwrap();
    assert!(!Path::new(&target.models_path).exists());
    let list = list_for_target(&db, &target).unwrap();
    let applied = action_for_target(
        &db,
        &target,
        action_input(&list, "native"),
        NativeAction::Apply,
    )
    .unwrap();
    assert!(applied.backup_path.is_none());
    assert_eq!(read_node(&target, "native"), Some(node.clone()));
    let mut external = std::fs::read_to_string(&target.models_path).unwrap();
    external.push_str("futureRoot: keep\n");
    std::fs::write(&target.models_path, &external).unwrap();
    let list = list_for_target(&db, &target).unwrap();
    action_for_target(
        &db,
        &target,
        action_input(&list, "native"),
        NativeAction::Remove,
    )
    .unwrap();
    assert!(std::fs::read_to_string(&target.models_path)
        .unwrap()
        .contains("futureRoot: keep"));
    let list = list_for_target(&db, &target).unwrap();
    action_for_target(
        &db,
        &target,
        action_input(&list, "native"),
        NativeAction::Apply,
    )
    .unwrap();
    assert_eq!(read_node(&target, "native"), Some(node));
    assert!(std::fs::read_to_string(&target.models_path)
        .unwrap()
        .contains("futureRoot: keep"));
}

#[test]
fn remove_archives_latest_external_content_and_refresh_does_not_import_gateway() {
    let (_home, db, target) = fixture();
    list_for_target(&db, &target).unwrap();
    let count = || {
        db.open_connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM providers", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before_count = count();
    std::fs::write(
        &target.models_path,
        br#"{"providers":{"anthropic":{"apiKey":"external-secret","external":true}}}"#,
    )
    .unwrap();
    let list = list_for_target(&db, &target).unwrap();
    action_for_target(
        &db,
        &target,
        action_input(&list, "anthropic"),
        NativeAction::Remove,
    )
    .unwrap();
    let conn = db.open_connection().unwrap();
    let profile = profiles::get(&conn, &target, "anthropic").unwrap().unwrap();
    assert_eq!(profile.node["apiKey"], "external-secret");
    assert_eq!(profile.node["external"], true);
    drop(conn); // Test DB intentionally has a single-connection pool.
    assert_eq!(count(), before_count);
}

#[test]
fn failed_read_preserves_archives_and_never_marks_them_removed() {
    let (_home, db, target) = fixture();
    let initial = list_for_target(&db, &target).unwrap();
    let malformed = br#"{"providers":{BROKEN secret-value}"#;
    std::fs::write(&target.models_path, malformed).unwrap();
    let unknown = list_for_target(&db, &target).unwrap();
    assert_eq!(unknown.parse_status, NativeParseStatus::Invalid);
    assert!(unknown.revision.is_none());
    assert_eq!(unknown.providers[0].state, NativeProviderState::Unknown);
    assert_eq!(
        unknown.providers[0].profile_revision,
        initial.providers[0].profile_revision
    );
    assert!(!unknown.issue.unwrap().contains("secret-value"));
    assert!(action_for_target(
        &db,
        &target,
        action_input(&initial, "anthropic"),
        NativeAction::Remove
    )
    .is_err());
    assert_eq!(std::fs::read(&target.models_path).unwrap(), malformed);
}

#[test]
fn archive_and_file_revisions_are_independently_guarded() {
    let (_home, db, target) = fixture();
    let initial = list_for_target(&db, &target).unwrap();
    let mut metadata = save_input(
        &initial,
        "anthropic",
        read_node(&target, "anthropic").unwrap(),
        false,
    );
    metadata.display_name = "changed name".into();
    save_for_target(&db, &target, metadata).unwrap();
    assert_eq!(
        action_for_target(
            &db,
            &target,
            action_input(&initial, "anthropic"),
            NativeAction::Remove
        )
        .err()
        .unwrap()
        .code(),
        "NATIVE_ARCHIVE_CONFLICT"
    );
    let list = list_for_target(&db, &target).unwrap();
    std::fs::write(
        &target.models_path,
        br#"{"providers":{"anthropic":{"external":true}}}"#,
    )
    .unwrap();
    assert_eq!(
        action_for_target(
            &db,
            &target,
            action_input(&list, "anthropic"),
            NativeAction::Remove
        )
        .err()
        .unwrap()
        .code(),
        "NATIVE_REVISION_CONFLICT"
    );
}

#[test]
fn database_failure_compensates_only_native_nodes() {
    let (_home, db, target) = fixture();
    let list = list_for_target(&db, &target).unwrap();
    let original = read_node(&target, "anthropic").unwrap();
    db.open_connection().unwrap().execute_batch("CREATE TRIGGER native_fail_update BEFORE UPDATE ON native_cli_provider_profiles BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let input = save_input(
        &list,
        "anthropic",
        json!({"apiKey":"changed","future":{"keep":true}}),
        true,
    );
    let error = save_for_target(&db, &target, input).err().unwrap();
    assert_eq!(error.code(), "NATIVE_ARCHIVE_FAILED");
    assert_eq!(read_node(&target, "anthropic"), Some(original));
    let conn = db.open_connection().unwrap();
    assert_eq!(
        profiles::get(&conn, &target, "anthropic")
            .unwrap()
            .unwrap()
            .revision,
        list.providers[0].profile_revision.clone().unwrap()
    );
}

#[test]
fn delete_requires_explicit_live_removal_and_save_requires_apply() {
    let (_home, db, target) = fixture();
    let list = list_for_target(&db, &target).unwrap();
    assert_eq!(
        action_for_target(
            &db,
            &target,
            action_input(&list, "anthropic"),
            NativeAction::Delete {
                remove_native: false
            }
        )
        .err()
        .unwrap()
        .code(),
        "NATIVE_REMOVE_REQUIRED"
    );
    assert_eq!(
        save_for_target(
            &db,
            &target,
            save_input(&list, "anthropic", json!({"apiKey":"changed"}), false)
        )
        .err()
        .unwrap()
        .code(),
        "NATIVE_APPLY_REQUIRED"
    );
    action_for_target(
        &db,
        &target,
        action_input(&list, "anthropic"),
        NativeAction::Delete {
            remove_native: true,
        },
    )
    .unwrap();
    assert!(list_for_target(&db, &target).unwrap().providers.is_empty());
    assert!(read_node(&target, "anthropic").is_none());
}

#[test]
fn exact_manifest_keys_are_managed_but_prefix_lookalikes_are_native() {
    let (_home, db, target) = fixture();
    std::fs::write(
        &target.models_path,
        br#"{"providers":{"owned":{},"aio-coding-hub-user":{}}}"#,
    )
    .unwrap();
    db.open_connection().unwrap().execute("INSERT INTO native_gateway_manifests(target_id,cli_key,protocol,native_key,node_digest,node_json,catalog_revision,generation,state,updated_at) VALUES (?1,'pi','openai-responses','owned','digest','{}','catalog',1,'applied',1)",[&target.target_id]).unwrap();
    let list = list_for_target(&db, &target).unwrap();
    assert!(
        list.providers
            .iter()
            .find(|p| p.native_key == "owned")
            .unwrap()
            .managed
    );
    assert!(
        !list
            .providers
            .iter()
            .find(|p| p.native_key == "aio-coding-hub-user")
            .unwrap()
            .managed
    );
    let conn = db.open_connection().unwrap();
    assert!(profiles::get(&conn, &target, "owned").unwrap().is_none());
    drop(conn); // The service must be able to acquire its own DB connection.
    assert_eq!(
        action_for_target(
            &db,
            &target,
            action_input(&list, "owned"),
            NativeAction::Remove
        )
        .err()
        .unwrap()
        .code(),
        "NATIVE_MANAGED_NODE"
    );
}

#[test]
fn shared_agent_directory_is_read_only_and_separate_targets_are_independent() {
    let home = tempfile::tempdir().unwrap();
    let shared = home.path().join("shared").to_string_lossy().into_owned();
    let mut pi = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Pi),
        Some(&shared),
        None,
    )
    .unwrap();
    let omp = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Omp),
        Some(&shared),
        None,
    )
    .unwrap();
    mark_collision(&mut pi, Some(&omp));
    assert!(!pi.writable);
    let custom = NativeTargetSelection {
        client: NativeClient::Pi,
        mode: NativeTargetMode::Custom,
        agent_dir: Some(
            home.path()
                .join("independent")
                .to_string_lossy()
                .into_owned(),
        ),
        profile: None,
    };
    let mut pi = targets::resolve(
        home.path(),
        &custom,
        Some("ignored invalid environment"),
        None,
    )
    .unwrap();
    mark_collision(&mut pi, Some(&omp));
    assert!(pi.writable);
    assert_ne!(pi.target_id, omp.target_id);
}

#[test]
fn queued_save_and_remove_recheck_selection_after_target_lock() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    let (_home, db, target) = fixture();
    let list = list_for_target(&db, &target).unwrap();
    let original = std::fs::read(&target.models_path).unwrap();
    // A selection commit owns the old target lock. The worker has already
    // resolved its preview, and cannot check the live selection until release.
    // Channels establish ordering; no sleeps or scheduler timing assumptions.
    for save in [true, false] {
        let selected = AtomicBool::new(true);
        let (started, ready) = mpsc::channel();
        let db_ref = &db;
        let target_ref = &target;
        let list_ref = &list;
        let selected_ref = &selected;
        let result = std::thread::scope(|scope| {
            let worker = native_cli::with_target_lock(&target, |_| {
                let worker = scope.spawn(move || {
                    started.send(()).unwrap();
                    let check = || {
                        if selected_ref.load(Ordering::SeqCst) {
                            Ok(())
                        } else {
                            Err(AppError::new(
                                "NATIVE_TARGET_CHANGED",
                                "Selection changed while operation was queued",
                            ))
                        }
                    };
                    if save {
                        save_for_target_checked(
                            db_ref,
                            target_ref,
                            save_input(
                                list_ref,
                                "anthropic",
                                json!({"apiKey":"stale-write"}),
                                true,
                            ),
                            check,
                        )
                    } else {
                        action_for_target_checked(
                            db_ref,
                            target_ref,
                            action_input(list_ref, "anthropic"),
                            NativeAction::Remove,
                            check,
                        )
                    }
                });
                ready.recv().unwrap();
                selected.store(false, Ordering::SeqCst);
                Ok(worker)
            })
            .unwrap();
            worker.join().unwrap()
        });
        assert_eq!(result.err().unwrap().code(), "NATIVE_TARGET_CHANGED");
        assert_eq!(std::fs::read(&target.models_path).unwrap(), original);
        assert_eq!(
            profiles::get(&db.open_connection().unwrap(), &target, "anthropic")
                .unwrap()
                .unwrap()
                .revision,
            list.providers[0]
                .profile_revision
                .as_ref()
                .unwrap()
                .as_str()
        );
        assert!(!Path::new(&target.agent_dir)
            .join(".aio-native-backups")
            .exists());
    }
}
