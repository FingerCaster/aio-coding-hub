use super::*;
use crate::domain::native_cli::{NativeClient, NativeTargetSelection};
use serde_json::json;

fn fixture(client: NativeClient, bytes: &[u8]) -> (tempfile::TempDir, NativeTarget) {
    let home = tempfile::tempdir().unwrap();
    let target = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(client),
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&target.agent_dir).unwrap();
    std::fs::write(&target.models_path, bytes).unwrap();
    (home, target)
}
fn replacement(key: &str, expected: Option<&Value>, next: Option<Value>) -> NativeNodePatch {
    NativeNodePatch {
        native_key: key.into(),
        expected_digest: expected.map(node_digest),
        replacement: next,
    }
}

#[test]
fn node_patch_preserves_unknown_fields_auth_defaults_and_original_backup() {
    let original = b"{// comment\n\"providers\":{\"builtin\":{\"apiKey\":\"!do-not-run\",\"future\":42},\"other\":{\"headers\":{\"secret\":\"${KEEP}\"}}},\"unknownRoot\":{\"keep\":true},}";
    let (_home, target) = fixture(NativeClient::Pi, original);
    let sentinels = [
        "auth.json",
        "agent.db",
        ".env",
        "settings.json",
        "config.yml",
    ];
    let mut before = Vec::new();
    for name in sentinels {
        let path = Path::new(&target.agent_dir).join(name);
        std::fs::write(&path, format!("sentinel:{name}")).unwrap();
        before.push((
            path.clone(),
            std::fs::read(&path).unwrap(),
            std::fs::metadata(&path).unwrap().modified().unwrap(),
        ));
    }
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        let mut node = snapshot.providers()["builtin"].clone();
        node["name"] = json!("Edited");
        let receipt = session.patch(
            &snapshot.revision,
            &[replacement(
                "builtin",
                snapshot.providers().get("builtin"),
                Some(node),
            )],
        )?;
        assert_eq!(
            std::fs::read(receipt.backup_path.unwrap()).unwrap(),
            original
        );
        let after = session.snapshot()?;
        assert_eq!(after.providers()["other"], snapshot.providers()["other"]);
        assert_eq!(after.root["unknownRoot"], snapshot.root["unknownRoot"]);
        assert_eq!(after.providers()["builtin"]["future"], 42);
        assert_eq!(after.providers()["builtin"]["apiKey"], "!do-not-run");
        Ok(())
    })
    .unwrap();
    for (path, content, modified) in before {
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert_eq!(
            std::fs::metadata(path).unwrap().modified().unwrap(),
            modified
        );
    }
}

#[test]
fn stale_revision_and_new_key_collision_do_not_write() {
    let (_home, target) = fixture(
        NativeClient::Pi,
        br#"{"providers":{"p":{"apiKey":"original"}}}"#,
    );
    let before = std::fs::read(&target.models_path).unwrap();
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        assert_eq!(
            session.patch("stale", &[]).err().unwrap().code(),
            "NATIVE_REVISION_CONFLICT"
        );
        assert_eq!(
            session
                .patch(
                    &snapshot.revision,
                    &[replacement("p", None, Some(json!({"apiKey":"collision"})))]
                )
                .err()
                .unwrap()
                .code(),
            "NATIVE_NODE_CONFLICT"
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&target.models_path).unwrap(), before);
    assert!(!Path::new(&target.agent_dir)
        .join(".aio-native-backups")
        .exists());
}

#[test]
fn compensation_changes_only_owned_nodes_and_detects_external_edit() {
    let (_home, target) = fixture(
        NativeClient::Pi,
        br#"{"providers":{"p":{"apiKey":"original"},"other":{}}}"#,
    );
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        let receipt = session.patch(
            &snapshot.revision,
            &[replacement(
                "p",
                snapshot.providers().get("p"),
                Some(json!({"apiKey":"aio-write"})),
            )],
        )?;
        let mut external = session.snapshot()?.root;
        external["providers"]["other"] = json!({"external":true});
        external["externalRoot"] = json!(42);
        std::fs::write(&target.models_path, serde_json::to_vec(&external).unwrap()).unwrap();
        session.compensate(&receipt)?;
        let after = session.snapshot()?;
        assert_eq!(after.providers()["p"], snapshot.providers()["p"]);
        assert_eq!(after.providers()["other"], external["providers"]["other"]);
        assert_eq!(after.root["externalRoot"], 42);
        let receipt = session.patch(
            &after.revision,
            &[replacement(
                "p",
                after.providers().get("p"),
                Some(json!({"apiKey":"second-aio-write"})),
            )],
        )?;
        let mut external = session.snapshot()?.root;
        external["providers"]["p"]["apiKey"] = json!("external-winner");
        let bytes = serde_json::to_vec(&external).unwrap();
        std::fs::write(&target.models_path, &bytes).unwrap();
        assert_eq!(
            session.compensate(&receipt).unwrap_err().code(),
            "NATIVE_COMPENSATION_CONFLICT"
        );
        assert_eq!(std::fs::read(&target.models_path).unwrap(), bytes);
        Ok(())
    })
    .unwrap();
}

#[test]
fn external_write_between_staging_and_replace_survives() {
    let (_home, target) = fixture(NativeClient::Pi, br#"{"providers":{}}"#);
    let external = br#"{"providers":{"external":{"future":true}}}"#;
    with_target_lock(&target, |session| {
        let before = session.snapshot()?;
        let err = session
            .patch_with_hook(
                &before.revision,
                &[replacement("new", None, Some(json!({})))],
                || {
                    std::fs::write(&target.models_path, external).unwrap();
                    Ok(())
                },
            )
            .err()
            .unwrap();
        assert_eq!(err.code(), "NATIVE_REVISION_CONFLICT");
        assert_eq!(std::fs::read(&target.models_path).unwrap(), external);
        Ok(())
    })
    .unwrap();
}

#[test]
fn invalid_or_duplicate_yaml_stays_byte_identical() {
    for original in [
        b"providers: {a: {auth: none}, a: {auth: none}}".as_slice(),
        b"providers: {a: &value {auth: none}, b: *value}",
        b"providers: [broken",
    ] {
        let (_home, target) = fixture(NativeClient::Omp, original);
        with_target_lock(&target, |session| {
            assert!(session.snapshot().is_err());
            assert!(session.patch("missing", &[]).is_err());
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&target.models_path).unwrap(), original);
    }
}

#[test]
fn unsupported_pi_block_comments_remain_untouched() {
    let original = br#"{/* block */"providers":{}}"#;
    let (_home, target) = fixture(NativeClient::Pi, original);
    with_target_lock(&target, |session| {
        assert!(session.snapshot().is_err());
        assert!(session.patch("missing", &[]).is_err());
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&target.models_path).unwrap(), original);
    assert!(!Path::new(&target.agent_dir)
        .join(".aio-native-backups")
        .exists());
}

#[test]
fn legacy_json_is_preview_only_and_priority_change_invalidates_target() {
    let home = tempfile::tempdir().unwrap();
    let original = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Omp),
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&original.agent_dir).unwrap();
    std::fs::write(
        Path::new(&original.agent_dir).join("models.json"),
        br#"{"providers":{"p":{"auth":"none"}}}"#,
    )
    .unwrap();
    let legacy = targets::resolve(
        home.path(),
        &NativeTargetSelection::default_for(NativeClient::Omp),
        None,
        None,
    )
    .unwrap();
    with_target_lock(&legacy, |session| {
        assert!(session.snapshot()?.providers().contains_key("p"));
        assert_eq!(
            session.patch("missing", &[]).err().unwrap().code(),
            "NATIVE_TARGET_READ_ONLY"
        );
        Ok(())
    })
    .unwrap();
    assert!(!Path::new(&legacy.agent_dir).join("models.yml").exists());
    assert!(with_target_lock(&original, |session| session.snapshot()).is_err());
}

#[test]
fn failed_backup_and_finalize_preserve_original_bytes() {
    let original = br#"{"providers":{}}"#;
    let (_home, target) = fixture(NativeClient::Pi, original);
    let block = Path::new(&target.agent_dir).join(".aio-native-backups");
    std::fs::write(&block, b"not-directory").unwrap();
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        assert!(session
            .patch(
                &snapshot.revision,
                &[replacement("new", None, Some(json!({})))]
            )
            .is_err());
        std::fs::remove_file(&block).unwrap();
        assert!(session
            .patch_with_hook(
                &snapshot.revision,
                &[replacement("new", None, Some(json!({})))],
                || Err(AppError::new(
                    "NATIVE_TEST_FAILURE",
                    "injected write failure"
                ))
            )
            .is_err());
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&target.models_path).unwrap(), original);
    assert!(!std::fs::read_dir(&target.agent_dir).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
}

#[test]
fn idempotent_apply_keeps_file_mtime_and_avoids_backup() {
    let (_home, target) = fixture(NativeClient::Pi, br#"{"providers":{"p":{}}}"#);
    let mtime = std::fs::metadata(&target.models_path)
        .unwrap()
        .modified()
        .unwrap();
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        let receipt = session.patch(
            &snapshot.revision,
            &[replacement(
                "p",
                snapshot.providers().get("p"),
                Some(json!({})),
            )],
        )?;
        assert!(!receipt.changed);
        assert!(receipt.backup_path.is_none());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        std::fs::metadata(&target.models_path)
            .unwrap()
            .modified()
            .unwrap(),
        mtime
    );
}

#[cfg(windows)]
#[test]
fn windows_locked_destination_preserves_data_and_private_backup() {
    use std::os::windows::fs::OpenOptionsExt;
    let original = br#"{"providers":{}}"#;
    let (_home, target) = fixture(NativeClient::Pi, original);
    let _external_handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&target.models_path)
        .unwrap();
    with_target_lock(&target, |session| {
        let snapshot = session.snapshot()?;
        assert!(session
            .patch(
                &snapshot.revision,
                &[replacement("p", None, Some(json!({})))]
            )
            .is_err());
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&target.models_path).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn private_native_output_and_backups_are_mode_600() {
    use std::os::unix::fs::PermissionsExt;
    let (_home, target) = fixture(NativeClient::Pi, br#"{"providers":{}}"#);
    let receipt = with_target_lock(&target, |session| {
        session.patch(
            &session.snapshot()?.revision,
            &[replacement("p", None, Some(json!({})))],
        )
    })
    .unwrap();
    for path in [&target.models_path, receipt.backup_path.as_ref().unwrap()] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(windows)]
#[test]
fn private_native_output_has_protected_owner_acl() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;
    use windows_sys::Win32::Security::{GetKernelObjectSecurity, DACL_SECURITY_INFORMATION};
    let (_home, target) = fixture(NativeClient::Pi, br#"{"providers":{}}"#);
    let receipt = with_target_lock(&target, |session| {
        session.patch(
            &session.snapshot()?.revision,
            &[replacement("p", None, Some(json!({})))],
        )
    })
    .unwrap();
    for path in [&target.models_path, receipt.backup_path.as_ref().unwrap()] {
        let file = std::fs::File::open(path).unwrap();
        let mut size = 0;
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut size,
            );
        }
        let mut descriptor = vec![0u8; size as usize];
        let mut text = std::ptr::null_mut();
        unsafe {
            assert_ne!(
                GetKernelObjectSecurity(
                    file.as_raw_handle(),
                    DACL_SECURITY_INFORMATION,
                    descriptor.as_mut_ptr().cast(),
                    size,
                    &mut size
                ),
                0
            );
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor.as_mut_ptr().cast(),
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    std::ptr::null_mut()
                ),
                0
            );
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            assert!(sddl.starts_with("D:P"));
            assert!(sddl.contains(";;;OW)"));
            assert!(!sddl.contains(";;;WD)"));
            assert!(!sddl.contains(";;;BU)"));
        }
    }
}
