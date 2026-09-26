use super::*;

#[test]
fn update_comparison_never_downgrades_or_invents_unknown_versions() {
    assert!(newer_version(Some("0.9.9"), "0.10.0"));
    assert!(!newer_version(Some("1.0.0"), "1.0.0"));
    assert!(!newer_version(Some("2.0.0"), "1.9.9"));
    assert!(!newer_version(None, "1.0.0"));
    assert!(!newer_version(Some("unknown"), "1.0.0"));
    for bad in [
        "latest",
        "1.0.0-beta.1",
        "1.0.0+build",
        "1.0.0;whoami",
        "../1.0.0",
    ] {
        assert!(stable_version(bad).is_err());
    }
}

#[test]
fn plans_are_client_bound_expiring_and_single_use() {
    let plan = Plan {
        client: NativeClient::Pi,
        installation: Installation {
            executable: None,
            version: None,
            method: None,
            blocked_reason: None,
        },
        version: "1.2.3".into(),
        checksum: None,
        created: Instant::now(),
    };
    let id = save_plan(plan.clone()).unwrap();
    assert!(take_plan(&id, NativeClient::Omp).is_err());
    assert!(take_plan(&id, NativeClient::Pi).is_ok());
    assert!(take_plan(&id, NativeClient::Pi).is_err());
    let expired = save_plan(Plan {
        created: Instant::now()
            .checked_sub(PLAN_LIFETIME)
            .expect("plan expiry fixture must fit in the monotonic clock range"),
        ..plan
    })
    .unwrap();
    assert!(take_plan(&expired, NativeClient::Pi).is_err());
}

#[test]
fn unknown_installations_cannot_be_silently_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("omp.cmd");
    std::fs::write(&exe, "@echo omp/18.3.2").unwrap();
    let detected = identify_installation(
        NativeClient::Omp,
        Some(exe),
        Some("18.3.2".into()),
        dir.path(),
        None,
        None,
        None,
    );
    assert!(detected.method.is_none());
    assert!(detected.blocked_reason.is_some());
}

#[test]
fn missing_omp_chooses_official_binary_without_creating_directories() {
    let dir = tempfile::tempdir().unwrap();
    let detected =
        identify_installation(NativeClient::Omp, None, None, dir.path(), None, None, None);
    assert!(matches!(
        detected.method,
        Some(InstallMethod::Standalone { .. })
    ));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn npm_install_preserves_the_existing_prefix_and_package_identity() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("custom prefix");
    let bin = if cfg!(windows) {
        prefix.clone()
    } else {
        prefix.join("bin")
    };
    let modules = if cfg!(windows) {
        prefix.join("node_modules")
    } else {
        prefix.join("lib/node_modules")
    };
    let manifest = modules.join(PI_LEGACY_PACKAGE).join("package.json");
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        manifest,
        format!(r#"{{"name":"{PI_LEGACY_PACKAGE}","version":"0.50.0"}}"#),
    )
    .unwrap();
    let npm = bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    let script = bin.join("node_modules/npm/bin/npm-cli.js");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(script, "").unwrap();
    let node = bin.join(if cfg!(windows) { "node.exe" } else { "node" });
    std::fs::write(&node, "").unwrap();
    let detected = identify_installation(
        NativeClient::Pi,
        Some(bin.join("pi")),
        Some("0.50.0".into()),
        dir.path(),
        Some(&npm),
        Some(&node),
        None,
    );
    assert!(
        matches!(detected.method, Some(InstallMethod::Npm { prefix: actual, package, .. }) if actual == prefix && package == PI_LEGACY_PACKAGE)
    );
}

#[test]
fn bun_global_install_is_not_misclassified_as_standalone() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let manifest = dir
        .path()
        .join("install/global/node_modules")
        .join(OMP_PACKAGE)
        .join("package.json");
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        manifest,
        format!(r#"{{"name":"{OMP_PACKAGE}","version":"18.3.2"}}"#),
    )
    .unwrap();
    let detected = identify_installation(
        NativeClient::Omp,
        Some(bin.join("omp.exe")),
        None,
        dir.path(),
        None,
        None,
        Some(&bin.join("bun.exe")),
    );
    assert_eq!(detected.version.as_deref(), Some("18.3.2"));
    assert!(matches!(detected.method, Some(InstallMethod::Bun { .. })));
}

#[tokio::test]
async fn concurrent_updates_are_rejected() {
    let _first = INSTALL_LOCK.lock().await;
    assert!(INSTALL_LOCK.try_lock().is_err());
}

#[test]
fn retired_pi_package_is_not_reported_as_latest_or_migrated_implicitly() {
    let mut installation = Installation {
        executable: Some("pi".into()),
        version: Some("0.73.1".into()),
        method: Some(InstallMethod::Npm {
            node: "node".into(),
            script: "npm-cli.js".into(),
            prefix: "prefix".into(),
            package: PI_LEGACY_PACKAGE.into(),
        }),
        blocked_reason: None,
    };
    assert!(block_legacy_migration(&mut installation));
    assert!(installation.blocked_reason.unwrap().contains("手动迁移"));
}
