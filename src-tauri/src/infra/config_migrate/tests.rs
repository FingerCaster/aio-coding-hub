use super::rollback::set_before_local_skill_dir_create_test_hook;
use super::skill_fs::{
    cli_skills_root, export_skill_dir_files, set_after_skill_export_enumeration_test_hook,
    set_after_skill_export_file_metadata_test_hook, set_write_prepared_skill_files_failpoint,
    ssot_skills_root, validate_installed_skill_key, write_skill_files_to_dir,
    SkillSourceMetadataFile,
};
use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::MutexGuard;
use tauri::Manager;

static TEST_ENV_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct EnvRestore {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn save_once(&mut self, key: &'static str) {
        if self.saved.iter().any(|(saved_key, _)| *saved_key == key) {
            return;
        }
        self.saved.push((key, std::env::var_os(key)));
    }

    fn set_var(&mut self, key: &'static str, value: impl Into<OsString>) {
        self.save_once(key);
        std::env::set_var(key, value.into());
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct ConfigMigrateTestApp {
    _env: EnvRestore,
    _lock: MutexGuard<'static, ()>,
    #[allow(dead_code)]
    home: tempfile::TempDir,
    app: tauri::App<tauri::test::MockRuntime>,
    db: crate::db::Db,
}

impl ConfigMigrateTestApp {
    fn new() -> Self {
        let lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let seq = TEST_ENV_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut env = EnvRestore::default();
        let home_os = home.path().as_os_str().to_os_string();
        env.set_var("AIO_CODING_HUB_HOME_DIR", home_os.clone());
        env.set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(".aio-coding-hub-config-migrate-test-{seq}"),
        );
        crate::test_support::clear_settings_cache();

        let app = tauri::test::mock_app();
        app.manage(crate::resident::ResidentState::default());
        let db = crate::db::init(app.handle()).expect("init db");

        Self {
            _lock: lock,
            _env: env,
            home,
            app,
            db,
        }
    }

    fn handle(&self) -> tauri::AppHandle<tauri::test::MockRuntime> {
        self.app.handle().clone()
    }
}

fn query_workspace(conn: &Connection, cli_key: &str) -> (i64, String) {
    conn.query_row(
        "SELECT id, name FROM workspaces WHERE cli_key = ?1 ORDER BY id ASC LIMIT 1",
        params![cli_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("query workspace")
}

fn write_skill_md(dir: &Path, name: &str, description: &str) {
    std::fs::create_dir_all(dir).expect("create skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n"),
    )
    .expect("write skill md");
}

fn write_grok_config_for_import_matrix(
    app: &tauri::AppHandle<tauri::test::MockRuntime>,
    valid: bool,
) {
    let config_path = crate::grok_config::config_path(app).expect("Grok config path");
    std::fs::create_dir_all(config_path.parent().expect("Grok config parent"))
        .expect("create Grok home");
    let content = if valid {
        b"# matrix valid\n[model.aio]\nmodel = \"grok-build\"\n".as_slice()
    } else {
        b"[mcp_servers\ninvalid = true\n".as_slice()
    };
    std::fs::write(config_path, content).expect("write Grok matrix config");
}

fn synthetic_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 31 + 17) % 251) as u8)
        .collect()
}

fn synthetic_png_like_bytes(len: usize) -> Vec<u8> {
    assert!(len >= 8);
    let mut bytes = synthetic_bytes(len);
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes
}

fn assert_synthetic_bytes(bytes: &[u8]) {
    for (index, byte) in bytes.iter().enumerate() {
        assert_eq!(*byte, ((index * 31 + 17) % 251) as u8, "byte {index}");
    }
}

fn make_test_bundle(schema_version: u32) -> ConfigBundle {
    ConfigBundle {
        schema_version,
        exported_at: "2026-03-29T00:00:00.000Z".to_string(),
        app_version: "0.0.0-test".to_string(),
        settings: serde_json::to_string(&settings::AppSettings::default()).expect("settings"),
        providers: Vec::new(),
        sort_modes: Vec::new(),
        sort_mode_active: HashMap::new(),
        workspaces: vec![WorkspaceExport {
            cli_key: "codex".to_string(),
            name: "Imported".to_string(),
            is_active: true,
            prompts: Vec::new(),
            prompt: None,
        }],
        mcp_servers: Vec::new(),
        skill_repos: Vec::new(),
        installed_skills: (schema_version >= CONFIG_BUNDLE_SCHEMA_VERSION).then(Vec::new),
        local_skills: (schema_version >= CONFIG_BUNDLE_SCHEMA_VERSION).then(Vec::new),
        image_gen_configs: None,
    }
}

fn make_matrix_bundle(
    settings_value: &settings::AppSettings,
    label: &str,
    prompt_content: &str,
) -> ConfigBundle {
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.settings = serde_json::to_string(settings_value).expect("matrix settings");
    bundle.workspaces = vec![WorkspaceExport {
        cli_key: "codex".to_string(),
        name: format!("Matrix {label}"),
        is_active: true,
        prompts: vec![PromptExport {
            name: format!("{label}-prompt"),
            content: prompt_content.to_string(),
            enabled: true,
        }],
        prompt: None,
    }];
    bundle.installed_skills = Some(vec![InstalledSkillExport {
        skill_key: format!("matrix-{label}"),
        name: format!("Matrix {label}"),
        description: "deterministic import matrix skill".to_string(),
        source_git_url: "https://example.invalid/matrix.git".to_string(),
        source_branch: "main".to_string(),
        source_subdir: String::new(),
        enabled_in_workspaces: vec![("codex".to_string(), format!("Matrix {label}"))],
        files: vec![
            SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD.encode(format!(
                    "---\nname: Matrix {label}\ndescription: matrix skill\n---\n"
                )),
            },
            SkillFileExport {
                relative_path: "payload.bin".to_string(),
                content_base64: BASE64_STANDARD.encode(format!("installed-{label}")),
            },
        ],
    }]);
    bundle.local_skills = Some(vec![LocalSkillExport {
        cli_key: "codex".to_string(),
        dir_name: format!("matrix-local-{label}"),
        name: format!("Matrix Local {label}"),
        description: "deterministic local matrix skill".to_string(),
        source_git_url: Some("https://example.invalid/local.git".to_string()),
        source_branch: Some("main".to_string()),
        source_subdir: Some("skills/matrix".to_string()),
        files: vec![
            SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD.encode(format!(
                    "---\nname: Matrix Local {label}\ndescription: local matrix skill\n---\n"
                )),
            },
            SkillFileExport {
                relative_path: "payload.txt".to_string(),
                content_base64: BASE64_STANDARD.encode(format!("local-{label}")),
            },
        ],
    }]);
    bundle
}

fn assert_matrix_state(
    test_app: &ConfigMigrateTestApp,
    expected_settings: &settings::AppSettings,
    label: &str,
    prompt_content: &str,
) {
    let app = test_app.handle();
    let canonical = settings::read(&app).expect("matrix canonical settings");
    assert_eq!(canonical.auto_start, expected_settings.auto_start);
    assert_eq!(
        canonical.log_retention_days,
        expected_settings.log_retention_days
    );

    let conn = test_app.db.open_connection().expect("matrix db");
    let workspace_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspaces WHERE name = ?1",
            rusqlite::params![format!("Matrix {label}")],
            |row| row.get(0),
        )
        .expect("matrix workspace count");
    assert_eq!(workspace_count, 1);
    let prompt_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM prompts WHERE content = ?1",
            rusqlite::params![prompt_content],
            |row| row.get(0),
        )
        .expect("matrix prompt count");
    assert_eq!(prompt_count, 1);

    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("matrix SSOT root");
    assert_eq!(
        std::fs::read(
            ssot_root
                .join(format!("matrix-{label}"))
                .join("payload.bin")
        )
        .expect("matrix installed payload"),
        format!("installed-{label}").as_bytes()
    );
    let local_root = cli_skills_root(&app, "codex").expect("matrix local root");
    assert_eq!(
        std::fs::read(
            local_root
                .join(format!("matrix-local-{label}"))
                .join("payload.txt")
        )
        .expect("matrix local payload"),
        format!("local-{label}").as_bytes()
    );
    let prompt_bytes = crate::prompt_sync::read_target_bytes(&app, "codex")
        .expect("matrix prompt target")
        .expect("matrix prompt target exists");
    assert!(
        String::from_utf8_lossy(&prompt_bytes).contains(prompt_content),
        "runtime prompt must match matrix winner"
    );
}

fn assert_matrix_import_artifacts_clean(app: &tauri::AppHandle<tauri::test::MockRuntime>) {
    let app_data_dir = crate::app_paths::app_data_dir(app).expect("matrix app data dir");
    let leftovers = std::fs::read_dir(&app_data_dir)
        .expect("read matrix app data dir")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with("config-import-skills-stage-")
                || name.starts_with("config-import-skills-backup-")
                || name.starts_with("config-import-local-backup-")
        })
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "import artifacts remain: {leftovers:?}"
    );
}

fn insert_image_gen_config(conn: &Connection, adapter_id: &str, api_key: &str) {
    conn.execute(
        r#"
INSERT INTO image_gen_configs(adapter_id, base_url, model, api_key_plaintext, created_at, updated_at)
VALUES (?1, 'https://img.example.com/v1', 'gpt-image-2', ?2, 1, 1)
"#,
        params![adapter_id, api_key],
    )
    .expect("insert image gen config");
}

#[test]
fn config_export_import_round_trips_image_gen_configs() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    {
        let conn = test_app.db.open_connection().expect("open db");
        insert_image_gen_config(&conn, "gpt-image", "sk-image-secret");
    }

    let bundle = config_export(&app, &test_app.db).expect("export");
    let exported = bundle
        .image_gen_configs
        .as_ref()
        .expect("bundle should carry image_gen_configs");
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].adapter_id, "gpt-image");
    assert_eq!(exported[0].base_url, "https://img.example.com/v1");
    assert_eq!(exported[0].model, "gpt-image-2");
    assert_eq!(exported[0].api_key_plaintext, "sk-image-secret");

    // Simulate "clear then import": wipe the table, then import the bundle.
    {
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute("DELETE FROM image_gen_configs", [])
            .expect("clear image gen configs");
    }

    config_import(&app, &test_app.db, bundle).expect("import");

    let conn = test_app.db.open_connection().expect("open db");
    let (base_url, model, api_key): (String, String, String) = conn
        .query_row(
            "SELECT base_url, model, api_key_plaintext FROM image_gen_configs WHERE adapter_id = 'gpt-image'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read restored image gen config");
    assert_eq!(base_url, "https://img.example.com/v1");
    assert_eq!(model, "gpt-image-2");
    assert_eq!(api_key, "sk-image-secret");
}

#[test]
fn config_import_without_image_gen_configs_keeps_existing_rows() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    {
        let conn = test_app.db.open_connection().expect("open db");
        insert_image_gen_config(&conn, "gpt-image", "sk-keep-me");
    }

    // Legacy bundle without the image_gen_configs field.
    let bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    assert!(bundle.image_gen_configs.is_none());

    config_import(&app, &test_app.db, bundle).expect("import");

    let conn = test_app.db.open_connection().expect("open db");
    let api_key: String = conn
        .query_row(
            "SELECT api_key_plaintext FROM image_gen_configs WHERE adapter_id = 'gpt-image'",
            [],
            |row| row.get(0),
        )
        .expect("existing image gen config should survive legacy import");
    assert_eq!(api_key, "sk-keep-me");
}

#[test]
fn config_import_cas_loser_preserves_winner_without_autostart_side_effect() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous settings");
    let mut imported = previous.clone();
    imported.auto_start = !previous.auto_start;
    imported.log_retention_days = previous.log_retention_days.saturating_add(1);
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.settings = serde_json::to_string(&imported).expect("import settings");

    let winner_retention = imported.log_retention_days.saturating_add(1);
    let hook_app = app.clone();
    set_before_config_import_settings_cas_test_hook(Box::new(move || {
        settings::update(&hook_app, |winner| {
            winner.log_retention_days = winner_retention;
            Ok(())
        })
        .expect("commit concurrent winner");
    }));
    crate::app::autostart::reset_auto_start_sync_test_calls();

    let error = match config_import(&app, &test_app.db, bundle) {
        Ok(_) => panic!("concurrent update must reject import"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SETTINGS_CONCURRENT_UPDATE"));
    let canonical = settings::read(&app).expect("canonical winner");
    assert_eq!(canonical.log_retention_days, winner_retention);
    assert_eq!(canonical.auto_start, previous.auto_start);
    assert_eq!(crate::app::autostart::auto_start_sync_test_calls(), 0);
}

#[test]
fn config_import_serializes_second_import_until_first_finishes() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous");

    let mut first_settings = previous.clone();
    first_settings.auto_start = true;
    first_settings.log_retention_days = previous.log_retention_days.saturating_add(1);
    let mut first_bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    first_bundle.settings = serde_json::to_string(&first_settings).expect("first settings");

    let mut second_settings = previous.clone();
    second_settings.auto_start = false;
    second_settings.log_retention_days = previous.log_retention_days.saturating_add(2);
    let mut second_bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    second_bundle.settings = serde_json::to_string(&second_settings).expect("second settings");

    let barrier = Arc::new(Barrier::new(2));
    let first_at_lock = Arc::new(AtomicBool::new(false));
    let first_at_lock_hook = first_at_lock.clone();
    let barrier_for_hook = barrier.clone();
    set_after_config_import_lock_acquired_test_hook(Box::new(move || {
        first_at_lock_hook.store(true, Ordering::SeqCst);
        barrier_for_hook.wait();
        barrier_for_hook.wait();
    }));
    reset_config_import_lock_attempts_for_test();

    let app_a = app.clone();
    let db_a = test_app.db.clone();
    let first = std::thread::spawn(move || config_import(&app_a, &db_a, first_bundle));

    // Deterministic wait until the first import owns CONFIG_IMPORT_LOCK.
    while !first_at_lock.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    barrier.wait();

    let app_b = app.clone();
    let db_b = test_app.db.clone();
    let second = std::thread::spawn(move || config_import(&app_b, &db_b, second_bundle));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while config_import_lock_attempts_for_test() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "second import must actually attempt CONFIG_IMPORT_LOCK"
        );
        std::thread::yield_now();
    }
    // While first is still paused after lock acquisition, the second thread is waiting.
    assert!(
        !second.is_finished(),
        "second import must wait for first import lifecycle"
    );

    // Release first import to finish CAS/commit/finish.
    barrier.wait();
    first.join().expect("join first").expect("first import");
    second.join().expect("join second").expect("second import");

    let canonical = settings::read(&app).expect("canonical");
    assert_eq!(
        canonical.log_retention_days,
        second_settings.log_retention_days
    );
    assert_eq!(canonical.auto_start, second_settings.auto_start);
}

#[test]
fn config_import_true_then_settings_false_converges_to_settings_winner() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous");

    let mut imported = previous.clone();
    imported.auto_start = true;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.settings = serde_json::to_string(&imported).expect("import settings");
    config_import(&app, &test_app.db, bundle).expect("import true");

    let after_import = settings::read(&app).expect("after import");
    assert!(after_import.auto_start);

    // Ordinary settings writer flips auto_start false after import succeeded.
    settings::update(&app, |latest| {
        latest.auto_start = false;
        latest.log_retention_days = after_import.log_retention_days.saturating_add(1);
        Ok(())
    })
    .expect("settings false");
    let _ = crate::app::autostart::converge_auto_start_to_canonical(&app);

    let winner = settings::read(&app).expect("winner");
    assert!(!winner.auto_start);
    assert_eq!(
        winner.log_retention_days,
        after_import.log_retention_days.saturating_add(1)
    );
}

#[test]
fn config_import_post_cas_barrier_waits_for_real_settings_writer_and_converges_os() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous settings");

    let mut imported = previous.clone();
    imported.auto_start = true;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.settings = serde_json::to_string(&imported).expect("import settings");

    let cas_reached = Arc::new(AtomicBool::new(false));
    let release_import = Arc::new(Barrier::new(2));
    let cas_reached_for_hook = cas_reached.clone();
    let release_for_hook = release_import.clone();
    crate::app::autostart::set_after_whole_settings_cas_test_hook(Box::new(move || {
        cas_reached_for_hook.store(true, Ordering::SeqCst);
        release_for_hook.wait();
    }));
    crate::app::autostart::reset_auto_start_lock_attempts_for_test();
    crate::app::autostart::reset_auto_start_sync_test_calls();

    let app_for_import = app.clone();
    let db_for_import = test_app.db.clone();
    let import_thread =
        std::thread::spawn(move || config_import(&app_for_import, &db_for_import, bundle));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !cas_reached.load(Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < deadline,
            "import must reach the post-CAS barrier"
        );
        std::thread::yield_now();
    }

    let after_cas = settings::read(&app).expect("post-CAS canonical settings");
    assert!(
        after_cas.auto_start,
        "import must be durably committed at the barrier"
    );

    let settings_update =
        serde_json::from_value::<crate::commands::settings::SettingsUpdate>(serde_json::json!({
            "preferredPort": previous.preferred_port,
            "autoStart": false,
            "logRetentionDays": previous.log_retention_days.saturating_add(7),
            "failoverMaxAttemptsPerProvider": previous.failover_max_attempts_per_provider,
            "failoverMaxProvidersToTry": previous.failover_max_providers_to_try
        }))
        .expect("real settings update payload");
    let app_for_settings = app.clone();
    let settings_thread = std::thread::spawn(move || {
        tauri::async_runtime::block_on(crate::app::settings_service::settings_set_impl_for_test(
            app_for_settings,
            settings_update,
        ))
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while crate::app::autostart::auto_start_lock_attempts_for_test() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "real settings writer must actually attempt the autostart lock"
        );
        std::thread::yield_now();
    }
    assert!(
        !settings_thread.is_finished(),
        "settings writer must still wait at post-CAS barrier"
    );

    release_import.wait();
    import_thread
        .join()
        .expect("join import")
        .expect("post-CAS import succeeds");
    settings_thread
        .join()
        .expect("join settings writer")
        .expect("real settings writer succeeds");

    let canonical = settings::read(&app).expect("canonical winner");
    assert!(!canonical.auto_start);
    assert_eq!(
        canonical.log_retention_days,
        previous.log_retention_days.saturating_add(7)
    );
    assert_eq!(
        crate::app::autostart::auto_start_sync_test_targets()
            .last()
            .copied(),
        Some(false)
    );
}

#[test]
fn config_import_tail_preserves_concurrent_tray_winner_and_resident_state() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous settings");
    assert!(previous.tray_enabled);

    let mut imported = previous.clone();
    imported.tray_enabled = false;
    imported.log_retention_days = previous.log_retention_days.saturating_add(4);
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.settings = serde_json::to_string(&imported).expect("import settings");

    let winner_handle = app.clone();
    crate::app::autostart::set_after_whole_settings_cas_test_hook(Box::new(move || {
        settings::update(&winner_handle, |latest| {
            latest.tray_enabled = true;
            Ok(())
        })
        .expect("B tray winner");
        winner_handle
            .state::<crate::resident::ResidentState>()
            .set_tray_enabled(true);
    }));

    config_import(&app, &test_app.db, bundle).expect("import with tray winner");

    let canonical = settings::read(&app).expect("canonical tray winner");
    assert!(canonical.tray_enabled);
    assert!(
        app.state::<crate::resident::ResidentState>().tray_enabled(),
        "import tail must not hide the resident state using stale imported tray value"
    );
}

#[test]
fn whole_import_rollback_restores_owned_fields_without_overwriting_private_winner() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous settings");
    let mut imported = previous.clone();
    imported.auto_start = true;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    imported.image_gen_storage_dir = Some("A-import-root".to_string());

    let winner_handle = app.clone();
    crate::app::autostart::set_after_whole_settings_cas_test_hook(Box::new(move || {
        settings::update(&winner_handle, |latest| {
            latest.image_gen_storage_dir = Some("B-private-root".to_string());
            Ok(())
        })
        .expect("B private writer");
    }));
    let mut sync_calls = 0usize;
    crate::app::autostart::set_auto_start_sync_failure_hook(Box::new(move |_| {
        sync_calls += 1;
        (sync_calls == 1).then(|| "forced import OS failure".to_string())
    }));

    let (returned, token) = match crate::app::autostart::commit_whole_settings_with_auto_start(
        &app, &previous, &imported,
    ) {
        crate::app::autostart::WholeSettingsCommitResult::Committed { settings, token } => {
            (settings, token)
        }
        other => panic!("import correction should commit with effective auto_start: {other:?}"),
    };
    crate::app::autostart::reset_auto_start_sync_test_calls();

    assert_eq!(
        returned.image_gen_storage_dir.as_deref(),
        Some("A-import-root"),
        "correction token must not absorb B's private field"
    );
    assert_eq!(
        settings::read(&app)
            .expect("canonical after correction")
            .image_gen_storage_dir
            .as_deref(),
        Some("B-private-root")
    );

    let rollback = crate::app::autostart::rollback_whole_settings_with_auto_start_token(
        &app, &previous, &returned, token,
    );
    assert!(
        matches!(
            &rollback,
            crate::app::autostart::OwnedRollbackResult::Restored
                | crate::app::autostart::OwnedRollbackResult::ConcurrentWinner(_)
        ),
        "owner-aware rollback should complete: {rollback:?}"
    );
    let canonical = settings::read(&app).expect("canonical after rollback");
    assert_eq!(canonical.log_retention_days, previous.log_retention_days);
    assert_eq!(
        canonical.image_gen_storage_dir.as_deref(),
        Some("B-private-root")
    );
}

#[test]
fn config_import_rejects_skill_payload_before_import_lock_and_db_write() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.installed_skills = Some(vec![InstalledSkillExport {
        skill_key: "invalid-preflight".to_string(),
        name: "Invalid preflight".to_string(),
        description: String::new(),
        source_git_url: String::new(),
        source_branch: String::new(),
        source_subdir: String::new(),
        enabled_in_workspaces: Vec::new(),
        files: vec![
            SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD.encode(b"---\nname: invalid\n---\n"),
            },
            SkillFileExport {
                relative_path: "payload.bin".to_string(),
                content_base64: "%%%invalid-base64%%%".to_string(),
            },
        ],
    }]);
    reset_config_import_lock_attempts_for_test();

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("invalid Skill payload must fail before destructive import");
    assert!(error.to_string().contains("invalid base64"), "{error}");
    assert_eq!(
        config_import_lock_attempts_for_test(),
        0,
        "payload preflight must fail before CONFIG_IMPORT_LOCK"
    );
    let skill_count: i64 = test_app
        .db
        .open_connection()
        .expect("open db")
        .query_row("SELECT COUNT(1) FROM skills", [], |row| row.get(0))
        .expect("skill count");
    assert_eq!(skill_count, 0);
}

#[test]
fn config_import_rejects_near_64_mib_skill_base64_before_import_lock() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    let mut oversized_base64 = "A".repeat(CONFIG_BUNDLE_ENCODED_MAX_BYTES - 1024);
    oversized_base64.push('!');
    bundle.installed_skills = Some(vec![InstalledSkillExport {
        skill_key: "near-64-mib-preflight".to_string(),
        name: "Near 64 MiB preflight".to_string(),
        description: String::new(),
        source_git_url: String::new(),
        source_branch: String::new(),
        source_subdir: String::new(),
        enabled_in_workspaces: Vec::new(),
        files: vec![SkillFileExport {
            relative_path: "payload.bin".to_string(),
            content_base64: oversized_base64,
        }],
    }]);
    reset_config_import_lock_attempts_for_test();

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("near-64 MiB invalid payload must fail during preflight");
    assert!(error.to_string().contains("too large"), "{error}");
    assert_eq!(
        config_import_lock_attempts_for_test(),
        0,
        "near-limit payload must fail before CONFIG_IMPORT_LOCK"
    );
    let skill_count: i64 = test_app
        .db
        .open_connection()
        .expect("open db")
        .query_row("SELECT COUNT(1) FROM skills", [], |row| row.get(0))
        .expect("skill count");
    assert_eq!(skill_count, 0);
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("near-64-mib-preflight")
        .exists());
}

#[test]
fn config_import_rejects_conflicting_skill_paths_before_import_lock() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.installed_skills = Some(vec![InstalledSkillExport {
        skill_key: "conflicting-path-preflight".to_string(),
        name: "Conflicting path preflight".to_string(),
        description: String::new(),
        source_git_url: String::new(),
        source_branch: String::new(),
        source_subdir: String::new(),
        enabled_in_workspaces: Vec::new(),
        files: vec![
            SkillFileExport {
                relative_path: "nested".to_string(),
                content_base64: BASE64_STANDARD.encode(b"file"),
            },
            SkillFileExport {
                relative_path: "nested/file".to_string(),
                content_base64: BASE64_STANDARD.encode(b"conflict"),
            },
        ],
    }]);
    reset_config_import_lock_attempts_for_test();

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("conflicting path graph must fail during preflight");
    assert!(
        error.to_string().contains("conflicting skill file paths"),
        "{error}"
    );
    assert_eq!(
        config_import_lock_attempts_for_test(),
        0,
        "path conflict must fail before CONFIG_IMPORT_LOCK"
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("conflicting-path-preflight")
        .exists());
}

#[test]
fn local_skill_absence_to_create_race_does_not_delete_external_directory() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let local_root = cli_skills_root(&app, "codex").expect("local root");
    let target = local_root.join("external-race");
    let target_for_hook = target.clone();
    set_before_local_skill_dir_create_test_hook(Box::new(move || {
        write_skill_md(&target_for_hook, "External", "created by external tool");
        std::fs::write(target_for_hook.join("sentinel.bin"), b"external-bytes")
            .expect("external sentinel");
    }));
    let local = LocalSkillExport {
        cli_key: "codex".to_string(),
        dir_name: "external-race".to_string(),
        name: "External race".to_string(),
        description: String::new(),
        source_git_url: None,
        source_branch: None,
        source_subdir: None,
        files: vec![SkillFileExport {
            relative_path: "SKILL.md".to_string(),
            content_base64: BASE64_STANDARD.encode(b"---\nname: import\n---\n"),
        }],
    };

    let error = rollback::apply_skill_fs_import(&app, &[], &[local])
        .expect_err("external directory must win the atomic create race");
    assert!(
        error
            .to_string()
            .contains("SKILL_IMPORT_DIR_ALREADY_EXISTS"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(target.join("sentinel.bin")).expect("external sentinel survives"),
        b"external-bytes"
    );
}

#[test]
fn config_import_failure_then_success_converges_db_settings_fs_and_runtime() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, false);
    let previous_settings = settings::read(&app).expect("previous settings");
    let previous_prompt =
        crate::prompt_sync::read_target_bytes(&app, "codex").expect("read previous prompt target");

    let mut failed_settings = previous_settings.clone();
    failed_settings.log_retention_days = previous_settings.log_retention_days.saturating_add(1);
    let failed_bundle = make_matrix_bundle(&failed_settings, "failure", "failure-prompt");
    let error = config_import(&app, &test_app.db, failed_bundle)
        .expect_err("real Grok runtime failure must reject import");
    assert!(
        error.to_string().contains("GROK_CONFIG_INVALID_TOML"),
        "unexpected failure: {error}"
    );
    assert_eq!(
        serde_json::to_value(settings::read(&app).expect("settings after failed import"))
            .expect("serialize settings after failure"),
        serde_json::to_value(&previous_settings).expect("serialize previous settings")
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-failure")
        .exists());
    assert!(!cli_skills_root(&app, "codex")
        .expect("local root")
        .join("matrix-local-failure")
        .exists());
    assert_eq!(
        crate::prompt_sync::read_target_bytes(&app, "codex").expect("read restored prompt target"),
        previous_prompt
    );
    assert_matrix_import_artifacts_clean(&app);

    write_grok_config_for_import_matrix(&app, true);
    let mut success_settings = previous_settings.clone();
    success_settings.log_retention_days = previous_settings.log_retention_days.saturating_add(2);
    let success_bundle = make_matrix_bundle(&success_settings, "success", "success-prompt");
    config_import(&app, &test_app.db, success_bundle).expect("retry after failure");
    assert_matrix_state(&test_app, &success_settings, "success", "success-prompt");
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-failure")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn config_import_success_then_failure_restores_the_successful_winner() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, true);
    let previous_settings = settings::read(&app).expect("previous settings");

    let mut first_settings = previous_settings.clone();
    first_settings.log_retention_days = previous_settings.log_retention_days.saturating_add(3);
    let first_bundle = make_matrix_bundle(&first_settings, "winner", "winner-prompt");
    config_import(&app, &test_app.db, first_bundle).expect("first import succeeds");
    assert_matrix_state(&test_app, &first_settings, "winner", "winner-prompt");
    let winner_prompt =
        crate::prompt_sync::read_target_bytes(&app, "codex").expect("read winner prompt target");

    write_grok_config_for_import_matrix(&app, false);
    let mut failed_settings = first_settings.clone();
    failed_settings.auto_start = true;
    failed_settings.log_retention_days = first_settings.log_retention_days.saturating_add(4);
    let failed_bundle = make_matrix_bundle(&failed_settings, "loser", "loser-prompt");
    let error = config_import(&app, &test_app.db, failed_bundle)
        .expect_err("second real Grok runtime failure must reject import");
    assert!(
        error.to_string().contains("GROK_CONFIG_INVALID_TOML"),
        "unexpected failure: {error}"
    );
    assert_matrix_state(&test_app, &first_settings, "winner", "winner-prompt");
    assert_eq!(
        crate::prompt_sync::read_target_bytes(&app, "codex").expect("read restored winner prompt"),
        winner_prompt
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-loser")
        .exists());
    assert!(!cli_skills_root(&app, "codex")
        .expect("local root")
        .join("matrix-local-loser")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn config_import_failure_field_rollback_preserves_ordinary_and_private_winners() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, true);
    let previous = settings::read(&app).expect("previous settings");
    let a_storage = tempfile::tempdir().expect("A Image Gen storage root");
    let a_storage_path = std::fs::canonicalize(&a_storage)
        .expect("canonicalize A Image Gen storage root")
        .to_string_lossy()
        .to_string();

    let mut imported = previous.clone();
    imported.preferred_port = previous.preferred_port.saturating_add(1);
    imported.tray_enabled = !previous.tray_enabled;
    imported.auto_start = !previous.auto_start;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    imported.image_gen_storage_dir = Some(a_storage_path);
    let mut bundle = make_matrix_bundle(&imported, "field-rollback", "field-rollback-prompt");
    bundle.settings = serde_json::to_string(&imported).expect("field rollback settings");

    let b_retention = previous.log_retention_days.saturating_add(31);
    let b_update =
        serde_json::from_value::<crate::commands::settings::SettingsUpdate>(serde_json::json!({
            "preferredPort": imported.preferred_port,
            "autoStart": previous.auto_start,
            "logRetentionDays": b_retention,
            "failoverMaxAttemptsPerProvider": previous.failover_max_attempts_per_provider,
            "failoverMaxProvidersToTry": previous.failover_max_providers_to_try
        }))
        .expect("B ordinary settings update");
    let b_handle = app.clone();
    let b_db = test_app.db.clone();
    let b_storage = tempfile::tempdir().expect("B Image Gen storage root");
    let b_storage_path = b_storage.path().to_string_lossy().to_string();
    let b_storage_for_thread = b_storage_path.clone();
    let mut b_started = false;
    let mut b_update = Some(b_update);
    set_config_import_cli_runtime_sync_test_hook(Box::new(move || {
        if b_started {
            return None;
        }
        b_started = true;
        let b_handle = b_handle.clone();
        let b_db = b_db.clone();
        let b_storage_for_thread = b_storage_for_thread.clone();
        let b_update = b_update.take().expect("B update only runs once");
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(
                crate::app::settings_service::settings_set_impl_for_test(
                    b_handle.clone(),
                    b_update,
                ),
            )
            .expect("B ordinary production settings writer");
            crate::app::image_gen_service::commit_image_gen_storage_dir_settings(
                &b_handle,
                &b_db,
                &b_storage_for_thread,
            )
            .expect("B Image Gen dedicated production writer");
        })
        .join()
        .expect("join B production writers");
        Some("forced import runtime failure after B winner".to_string())
    }));

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("import must fail after B commits during runtime sync");
    clear_config_import_cli_runtime_sync_test_hook();
    assert!(
        error
            .to_string()
            .contains("forced import runtime failure after B winner"),
        "unexpected import failure: {error}"
    );

    let canonical = settings::read(&app).expect("canonical after field-aware rollback");
    let b_storage_canonical = std::fs::canonicalize(&b_storage)
        .expect("canonicalize B Image Gen storage root")
        .to_string_lossy()
        .to_string();
    assert_eq!(
        canonical.preferred_port, previous.preferred_port,
        "A-owned preferred_port must be restored"
    );
    assert_eq!(
        canonical.tray_enabled, previous.tray_enabled,
        "A-owned tray_enabled must be restored"
    );
    assert_eq!(
        canonical.log_retention_days, b_retention,
        "B ordinary winner must survive rollback"
    );
    assert_eq!(
        canonical.auto_start, previous.auto_start,
        "B auto_start winner must survive rollback"
    );
    assert_eq!(
        canonical.image_gen_storage_dir.as_deref(),
        Some(b_storage_canonical.as_str()),
        "B Image Gen private winner must survive rollback"
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-field-rollback")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn config_import_runtime_failure_same_value_auto_start_aba_skips_whole_snapshot_restore() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, true);
    let previous = settings::read(&app).expect("previous settings");
    let mut imported = previous.clone();
    imported.auto_start = !previous.auto_start;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    let mut bundle = make_matrix_bundle(&imported, "same-auto-whole", "same-auto-whole-prompt");
    bundle.settings = serde_json::to_string(&imported).expect("whole ABA settings");

    let b_update =
        serde_json::from_value::<crate::commands::settings::SettingsUpdate>(serde_json::json!({
            "preferredPort": imported.preferred_port,
            "autoStart": imported.auto_start,
            "logRetentionDays": imported.log_retention_days,
            "failoverMaxAttemptsPerProvider": imported.failover_max_attempts_per_provider,
            "failoverMaxProvidersToTry": imported.failover_max_providers_to_try
        }))
        .expect("same-value B ordinary settings update");
    let b_handle = app.clone();
    let mut b_started = false;
    let mut b_update = Some(b_update);
    crate::app::autostart::reset_auto_start_sync_test_calls();
    set_config_import_cli_runtime_sync_test_hook(Box::new(move || {
        if b_started {
            return None;
        }
        b_started = true;
        let b_handle = b_handle.clone();
        let b_update = b_update.take().expect("B update only runs once");
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(
                crate::app::settings_service::settings_set_impl_for_test(b_handle, b_update),
            )
            .expect("same-value B ordinary production writer");
        })
        .join()
        .expect("join same-value B writer");
        Some("forced import runtime failure after same-value whole ABA".to_string())
    }));

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("runtime failure must roll back after same-value whole ABA");
    clear_config_import_cli_runtime_sync_test_hook();
    assert!(
        error
            .to_string()
            .contains("forced import runtime failure after same-value whole ABA"),
        "unexpected import failure: {error}"
    );

    let canonical = settings::read(&app).expect("canonical after whole ABA rollback");
    assert_eq!(
        canonical.auto_start, imported.auto_start,
        "newer same-value generation must retain auto_start"
    );
    assert_eq!(
        canonical.log_retention_days, previous.log_retention_days,
        "import-owned ordinary field must still roll back"
    );
    assert_eq!(
        crate::app::autostart::auto_start_sync_test_targets()
            .last()
            .copied(),
        Some(imported.auto_start),
        "OS autostart must converge to the same-value generation winner"
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-same-auto-whole")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn config_import_runtime_failure_same_value_auto_start_aba_keeps_field_winner() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, true);
    let previous = settings::read(&app).expect("previous settings");
    let mut imported = previous.clone();
    imported.preferred_port = previous.preferred_port.saturating_add(1);
    imported.tray_enabled = !previous.tray_enabled;
    imported.auto_start = !previous.auto_start;
    imported.log_retention_days = previous.log_retention_days.saturating_add(5);
    let mut bundle = make_matrix_bundle(&imported, "same-auto-field", "same-auto-field-prompt");
    bundle.settings = serde_json::to_string(&imported).expect("field ABA settings");

    let b_retention = previous.log_retention_days.saturating_add(31);
    let b_update =
        serde_json::from_value::<crate::commands::settings::SettingsUpdate>(serde_json::json!({
            "preferredPort": imported.preferred_port,
            "autoStart": imported.auto_start,
            "logRetentionDays": b_retention,
            "failoverMaxAttemptsPerProvider": imported.failover_max_attempts_per_provider,
            "failoverMaxProvidersToTry": imported.failover_max_providers_to_try
        }))
        .expect("same-value field B ordinary settings update");
    let b_handle = app.clone();
    let mut b_started = false;
    let mut b_update = Some(b_update);
    crate::app::autostart::reset_auto_start_sync_test_calls();
    set_config_import_cli_runtime_sync_test_hook(Box::new(move || {
        if b_started {
            return None;
        }
        b_started = true;
        let b_handle = b_handle.clone();
        let b_update = b_update.take().expect("B update only runs once");
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(
                crate::app::settings_service::settings_set_impl_for_test(b_handle, b_update),
            )
            .expect("same-value field B ordinary production writer");
        })
        .join()
        .expect("join same-value field B writer");
        Some("forced import runtime failure after same-value field ABA".to_string())
    }));

    let error = config_import(&app, &test_app.db, bundle)
        .expect_err("runtime failure must roll back after same-value field ABA");
    clear_config_import_cli_runtime_sync_test_hook();
    assert!(
        error
            .to_string()
            .contains("forced import runtime failure after same-value field ABA"),
        "unexpected import failure: {error}"
    );

    let canonical = settings::read(&app).expect("canonical after field ABA rollback");
    assert_eq!(canonical.preferred_port, previous.preferred_port);
    assert_eq!(canonical.tray_enabled, previous.tray_enabled);
    assert_eq!(canonical.log_retention_days, b_retention);
    assert_eq!(
        canonical.auto_start, imported.auto_start,
        "newer same-value generation must retain auto_start"
    );
    assert_eq!(
        crate::app::autostart::auto_start_sync_test_targets()
            .last()
            .copied(),
        Some(imported.auto_start),
        "OS autostart must converge to the field-aware same-value winner"
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-same-auto-field")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn config_import_same_second_success_replacements_use_unique_artifacts_and_second_wins() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    write_grok_config_for_import_matrix(&app, true);
    crate::infra::config_migrate::set_config_import_now_override_for_test(Some(1_700_000_000));
    struct ImportTimestampReset;
    impl Drop for ImportTimestampReset {
        fn drop(&mut self) {
            crate::infra::config_migrate::set_config_import_now_override_for_test(None);
        }
    }
    let _timestamp_reset = ImportTimestampReset;

    let previous = settings::read(&app).expect("previous settings");
    let mut first_settings = previous.clone();
    first_settings.log_retention_days = previous.log_retention_days.saturating_add(5);
    config_import(
        &app,
        &test_app.db,
        make_matrix_bundle(&first_settings, "same-first", "same-first-prompt"),
    )
    .expect("first same-second import");

    let mut second_settings = first_settings.clone();
    second_settings.log_retention_days = first_settings.log_retention_days.saturating_add(1);
    config_import(
        &app,
        &test_app.db,
        make_matrix_bundle(&second_settings, "same-second", "same-second-prompt"),
    )
    .expect("second same-second import");

    assert_matrix_state(
        &test_app,
        &second_settings,
        "same-second",
        "same-second-prompt",
    );
    assert!(!ssot_skills_root(&app)
        .expect("SSOT root")
        .join("matrix-same-first")
        .exists());
    assert!(!cli_skills_root(&app, "codex")
        .expect("local root")
        .join("matrix-local-same-first")
        .exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn rollback_aggregates_settings_autostart_runtime_and_live_root_failures() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let previous = settings::read(&app).expect("previous settings");
    let mut committed = previous.clone();
    committed.auto_start = !previous.auto_start;
    committed.log_retention_days = previous.log_retention_days.saturating_add(1);

    let token = match crate::app::autostart::commit_whole_settings_with_auto_start(
        &app, &previous, &committed,
    ) {
        crate::app::autostart::WholeSettingsCommitResult::Committed { token, .. } => token,
        other => panic!("rollback aggregation setup failed: {other:?}"),
    };

    let live_root_file = crate::app_paths::app_data_dir(&app)
        .expect("app data dir")
        .join("rollback-live-root-file");
    std::fs::write(&live_root_file, b"live-root must not disappear silently")
        .expect("write live-root failure fixture");
    let mut skill_fs_guard =
        rollback::SkillFsImportGuard::test_guard_with_ssot_root(live_root_file.clone());

    crate::app::autostart::set_auto_start_sync_failure_hook(Box::new(|_| {
        Some("forced autostart rollback failure".to_string())
    }));
    set_config_import_cli_runtime_sync_test_hook(Box::new(|| {
        Some("forced CLI runtime rollback failure".to_string())
    }));

    let error = rollback::rollback_after_failed_import_with_auto_start_token(
        &app,
        &test_app.db,
        &previous,
        Some(&committed),
        Some(token),
        Vec::new(),
        Some(&mut skill_fs_guard),
    )
    .expect_err("all injected rollback failures must aggregate as recovery required");

    crate::app::autostart::reset_auto_start_sync_test_calls();
    clear_config_import_cli_runtime_sync_test_hook();

    let message = error.to_string();
    assert!(
        message.contains("CONFIG_IMPORT_RECOVERY_REQUIRED"),
        "{message}"
    );
    assert!(
        message.contains("settings/autostart could not be restored"),
        "settings/autostart failure missing: {message}"
    );
    assert!(
        message.contains("CLI runtime resync failed after import rollback"),
        "runtime failure missing: {message}"
    );
    assert!(
        message.contains("SKILL_FS_RECOVERY_REQUIRED"),
        "live-root failure missing: {message}"
    );
    assert!(
        live_root_file.exists(),
        "live-root failure must remain observable for recovery"
    );
    std::fs::remove_file(&live_root_file).expect("remove recovery fixture");
}

fn imported_skill_for_rollback_test(skill_key: &str) -> InstalledSkillExport {
    InstalledSkillExport {
        skill_key: skill_key.to_string(),
        name: "Rollback test skill".to_string(),
        description: "rollback test".to_string(),
        source_git_url: "https://example.invalid/rollback.git".to_string(),
        source_branch: "main".to_string(),
        source_subdir: String::new(),
        enabled_in_workspaces: Vec::new(),
        files: vec![SkillFileExport {
            relative_path: "SKILL.md".to_string(),
            content_base64: BASE64_STANDARD.encode(b"---\nname: rollback\n---\n"),
        }],
    }
}

#[test]
fn skill_fs_stage_failure_preserves_existing_ssot_root_and_cleans_stage() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let old_skill = ssot_root.join("old-stage-sentinel");
    write_skill_md(&old_skill, "Old stage", "must survive stage failure");
    let sentinel = old_skill.join("sentinel.bin");
    std::fs::write(&sentinel, b"old-ssot-stage-bytes").expect("old sentinel");

    rollback::set_skill_fs_import_failpoint(rollback::SkillFsImportFailpoint::StageWrite);
    let error = rollback::apply_skill_fs_import(
        &app,
        &[imported_skill_for_rollback_test("new-stage")],
        &[],
    )
    .expect_err("stage failpoint must reject the import");
    assert!(
        error.to_string().contains("stage"),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("old SSOT sentinel"),
        b"old-ssot-stage-bytes"
    );
    assert!(!ssot_root.join("new-stage").exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn skill_fs_backup_rename_failure_preserves_existing_ssot_root_and_cleans_stage() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let old_skill = ssot_root.join("old-rename-sentinel");
    write_skill_md(
        &old_skill,
        "Old rename",
        "must survive backup rename failure",
    );
    let sentinel = old_skill.join("sentinel.bin");
    std::fs::write(&sentinel, b"old-ssot-rename-bytes").expect("old sentinel");

    rollback::set_skill_fs_import_failpoint(rollback::SkillFsImportFailpoint::SsotBackupRename);
    let error = rollback::apply_skill_fs_import(
        &app,
        &[imported_skill_for_rollback_test("new-rename")],
        &[],
    )
    .expect_err("backup rename failpoint must reject the import");
    assert!(
        error.to_string().contains("backup"),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("old SSOT sentinel"),
        b"old-ssot-rename-bytes"
    );
    assert!(!ssot_root.join("new-rename").exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[test]
fn skill_fs_local_mid_write_failure_cleans_half_built_dir_without_touching_existing_dir() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    write_skill_md(
        &ssot_root.join("old-installed"),
        "Old installed",
        "keep installed",
    );

    let local_root = cli_skills_root(&app, "codex").expect("codex local root");
    let existing = local_root.join("existing-local");
    write_skill_md(&existing, "Existing local", "keep local");
    let sentinel = existing.join("sentinel.bin");
    std::fs::write(&sentinel, b"preexisting-local-bytes").expect("existing sentinel");

    let local = LocalSkillExport {
        cli_key: "codex".to_string(),
        dir_name: "new-half-written".to_string(),
        name: "Half written".to_string(),
        description: "mid-write failure".to_string(),
        source_git_url: None,
        source_branch: None,
        source_subdir: None,
        files: vec![
            SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD.encode(b"---\nname: half\n---\n"),
            },
            SkillFileExport {
                relative_path: "payload.bin".to_string(),
                content_base64: BASE64_STANDARD.encode(b"second file"),
            },
        ],
    };
    set_write_prepared_skill_files_failpoint(Some(1));
    let error = rollback::apply_skill_fs_import(&app, &[], &[local])
        .expect_err("mid-write failpoint must reject the import");
    assert!(
        error.to_string().contains("mid-write"),
        "unexpected error: {error}"
    );
    assert!(!local_root.join("new-half-written").exists());
    assert_eq!(
        std::fs::read(&sentinel).expect("preexisting local sentinel"),
        b"preexisting-local-bytes"
    );
    assert!(ssot_root.join("old-installed").join("SKILL.md").exists());
    assert_matrix_import_artifacts_clean(&app);
}

#[cfg(unix)]
fn create_file_symlink(src: &Path, dst: &Path) {
    std::os::unix::fs::symlink(src, dst).expect("create symlink");
}

#[cfg(unix)]
fn create_dir_link(src: &Path, dst: &Path) {
    std::os::unix::fs::symlink(src, dst).expect("create directory symlink");
}

#[cfg(windows)]
fn create_dir_link(src: &Path, dst: &Path) {
    junction::create(src, dst).expect("create directory junction");
}

fn insert_installed_skill(conn: &Connection, skill_key: &str) {
    conn.execute(
        r#"
INSERT INTO skills(
  skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir,
  created_at, updated_at
) VALUES (?1, 'Handle Skill', 'handle-skill', 'Handle-bound export',
          'https://example.test/repo.git', 'main', 'skills/handle', 1, 1)
"#,
        params![skill_key],
    )
    .expect("insert installed skill");
}

fn insert_distinct_installed_skill(conn: &Connection, skill_key: &str) {
    conn.execute(
        r#"
INSERT INTO skills(
  skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir,
  created_at, updated_at
) VALUES (?1, ?1, ?1, 'Aggregate export fixture',
          'https://example.test/repo.git', 'main', 'skills/aggregate', 1, 1)
"#,
        params![skill_key],
    )
    .expect("insert distinct installed skill");
}

#[test]
fn validate_bundle_schema_version_accepts_current_version() {
    assert!(super::validate_bundle_schema_version(CONFIG_BUNDLE_SCHEMA_VERSION).is_ok());
    assert!(super::validate_bundle_schema_version(CONFIG_BUNDLE_SCHEMA_VERSION_V1).is_ok());
}

#[test]
fn validate_bundle_schema_version_rejects_mismatch() {
    let err = super::validate_bundle_schema_version(CONFIG_BUNDLE_SCHEMA_VERSION + 1)
        .expect_err("schema version should fail");
    assert!(err
        .to_string()
        .contains("SEC_INVALID_INPUT: unsupported config bundle schema_version"));
}

#[test]
fn config_import_rejects_invalid_workspace_boundary_values() {
    {
        let test_app = ConfigMigrateTestApp::new();
        let app = test_app.handle();
        let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
        bundle.workspaces[0].name = "x".repeat(129);

        let Err(err) = config_import(&app, &test_app.db, bundle) else {
            panic!("oversized workspace name should fail");
        };
        assert!(err.to_string().contains("workspace name is too long"));
    }

    {
        let test_app = ConfigMigrateTestApp::new();
        let app = test_app.handle();
        let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
        bundle.workspaces[0].cli_key = "opencode".to_string();

        let Err(err) = config_import(&app, &test_app.db, bundle) else {
            panic!("invalid workspace cli key should fail");
        };
        assert!(err.to_string().contains("unknown cli_key=opencode"));
    }
}

#[test]
fn config_export_includes_full_prompts_provider_and_skill_payload() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let conn = test_app.db.open_connection().expect("open db");
    let (codex_workspace_id, codex_workspace_name) = query_workspace(&conn, "codex");

    conn.execute(
        r#"
INSERT INTO providers(
  cli_key, name, base_url, base_urls_json, base_url_mode, auth_mode,
  claude_models_json, supported_models_json, model_mapping_json, api_key_plaintext,
  enabled, priority, sort_order, cost_multiplier, limit_5h_usd, limit_daily_usd,
  daily_reset_mode, daily_reset_time, limit_weekly_usd, limit_monthly_usd, limit_total_usd,
  tags_json, note, oauth_provider_type, oauth_access_token, oauth_refresh_token, oauth_id_token,
  oauth_token_uri, oauth_client_id, oauth_client_secret, oauth_expires_at, oauth_email,
  oauth_refresh_lead_s, oauth_last_refreshed_at, oauth_last_error, created_at, updated_at
) VALUES (
  'codex', 'oauth-provider', 'https://api.example.com', '["https://api.example.com","https://backup.example.com"]',
  'order', 'oauth', '{"main":"gpt-5.4"}', '{"gpt-5.4":true}', '{"gpt-5.4":"gpt-5.4"}', '',
  1, 100, 0, 1.25, 1.0, 2.0, 'fixed', '00:00:00', 3.0, 4.0, 5.0, '["team"]', 'note',
  'openai', 'access-token', 'refresh-token', 'id-token', 'https://auth.example.com/token',
  'client-id', 'client-secret', 2000000000, 'dev@example.com', 7200, 1999999999, 'last error',
  1, 1
)
"#,
        [],
    )
    .expect("insert provider");

    conn.execute(
        r#"
INSERT INTO prompts(workspace_id, name, content, enabled, created_at, updated_at)
VALUES (?1, 'default', 'prompt one', 1, 1, 1),
       (?1, 'review', 'prompt two', 0, 1, 1)
"#,
        params![codex_workspace_id],
    )
    .expect("insert prompts");

    conn.execute(
        r#"
INSERT INTO skill_repos(git_url, branch, enabled, created_at, updated_at)
VALUES ('https://example.com/repo.git', 'main', 1, 1, 1)
"#,
        [],
    )
    .expect("insert skill repo");

    conn.execute(
        r#"
INSERT INTO skills(
  skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir,
  created_at, updated_at
) VALUES (
  'review-skill', 'Review Skill', 'review-skill', 'Installed review skill',
  'https://example.com/repo.git', 'main', 'skills/review', 1, 1
)
"#,
        [],
    )
    .expect("insert skill");
    let skill_id = conn.last_insert_rowid();
    conn.execute(
        r#"
INSERT INTO workspace_skill_enabled(workspace_id, skill_id, created_at, updated_at)
VALUES (?1, ?2, 1, 1)
"#,
        params![codex_workspace_id, skill_id],
    )
    .expect("enable skill");

    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let installed_skill_dir = ssot_root.join("review-skill");
    write_skill_md(
        &installed_skill_dir,
        "Review Skill",
        "Installed review skill",
    );
    std::fs::write(installed_skill_dir.join("README.md"), "installed").expect("write readme");

    let local_root = cli_skills_root(&app, "codex").expect("local root");
    let local_skill_dir = local_root.join("local-review");
    write_skill_md(&local_skill_dir, "Local Review", "Local review skill");
    std::fs::write(local_skill_dir.join("notes.txt"), "local").expect("write local file");
    let source_metadata = skill_fs::SkillSourceMetadataFile {
        source_git_url: "https://example.com/local.git".to_string(),
        source_branch: "main".to_string(),
        source_subdir: "skills/local-review".to_string(),
    };
    std::fs::write(
        local_skill_dir.join(SKILL_SOURCE_MARKER_FILE),
        serde_json::to_vec_pretty(&source_metadata).expect("serialize source"),
    )
    .expect("write source metadata");

    let bundle = config_export(&app, &test_app.db).expect("config export");

    let provider = bundle
        .providers
        .iter()
        .find(|provider| provider.name == "oauth-provider")
        .expect("provider export");
    assert_eq!(
        provider.base_urls,
        vec![
            "https://api.example.com".to_string(),
            "https://backup.example.com".to_string()
        ]
    );
    assert_eq!(provider.oauth_id_token.as_deref(), Some("id-token"));
    assert_eq!(provider.oauth_refresh_lead_seconds, 7200);
    assert_eq!(provider.oauth_last_refreshed_at, Some(1999999999));
    assert_eq!(provider.oauth_last_error.as_deref(), Some("last error"));
    assert_eq!(provider.supported_models_json, "{\"gpt-5.4\":true}");
    assert_eq!(provider.model_mapping_json, "{\"gpt-5.4\":\"gpt-5.4\"}");

    let codex_workspace = bundle
        .workspaces
        .iter()
        .find(|workspace| workspace.cli_key == "codex" && workspace.name == codex_workspace_name)
        .expect("codex workspace export");
    assert_eq!(codex_workspace.prompts.len(), 2);
    assert!(codex_workspace.prompt.is_none());

    let installed_skill = bundle
        .installed_skills
        .as_ref()
        .expect("installed skills export")
        .iter()
        .find(|skill| skill.skill_key == "review-skill")
        .expect("installed skill export");
    assert_eq!(installed_skill.enabled_in_workspaces.len(), 1);
    assert_eq!(
        installed_skill.enabled_in_workspaces[0],
        ("codex".to_string(), codex_workspace_name.clone())
    );
    assert!(installed_skill
        .files
        .iter()
        .any(|file| file.relative_path == "SKILL.md"));

    let local_skill = bundle
        .local_skills
        .as_ref()
        .expect("local skills export")
        .iter()
        .find(|skill| skill.cli_key == "codex" && skill.dir_name == "local-review")
        .expect("local skill export");
    assert_eq!(
        local_skill.source_git_url.as_deref(),
        Some("https://example.com/local.git")
    );
    assert!(local_skill
        .files
        .iter()
        .any(|file| file.relative_path == "notes.txt"));
}

#[test]
fn production_config_export_to_path_round_trips_real_disk_skills_and_db() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let conn = test_app.db.open_connection().expect("open db");
    let (workspace_id, workspace_name) = query_workspace(&conn, "codex");
    conn.execute(
        r#"
INSERT INTO prompts(workspace_id, name, content, enabled, created_at, updated_at)
VALUES (?1, 'roundtrip-prompt', ?2, 1, 1, 1)
"#,
        params![workspace_id, "db-byte-exact-prompt-\0-utf8"],
    )
    .expect("insert round-trip prompt");
    conn.execute(
        r#"
INSERT INTO skills(
  skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir,
  created_at, updated_at
) VALUES (
  'roundtrip-installed', 'Roundtrip Installed', 'roundtrip-installed',
  'DB byte exact description', 'https://example.test/roundtrip.git', 'main', 'skills/roundtrip', 1, 1
)
"#,
        [],
    )
    .expect("insert round-trip skill");
    let skill_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO workspace_skill_enabled(workspace_id, skill_id, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
        params![workspace_id, skill_id],
    )
    .expect("enable round-trip skill");
    drop(conn);

    let installed_payload = b"installed\0bytes\xffwith-no-filter";
    let installed_dir = ssot_skills_root(&app)
        .expect("ssot root")
        .join("roundtrip-installed");
    write_skill_md(
        &installed_dir,
        "Roundtrip Installed",
        "DB byte exact description",
    );
    std::fs::write(installed_dir.join("payload.bin"), installed_payload)
        .expect("write installed payload");

    let local_payload = b"local\0bytes\xfeexact";
    let local_dir = cli_skills_root(&app, "codex")
        .expect("codex local root")
        .join("roundtrip-local");
    write_skill_md(&local_dir, "Roundtrip Local", "local exact description");
    std::fs::write(local_dir.join("payload.bin"), local_payload).expect("write local payload");

    let temp = tempfile::tempdir().expect("export tempdir");
    let export_path = temp.path().join("production-export.json");
    crate::commands::config_migrate::config_export_to_path(&app, &test_app.db, &export_path)
        .expect("production config_export_to_path");
    let exported_bytes = std::fs::read(&export_path).expect("read production export");
    assert!(!exported_bytes.is_empty());
    assert!(exported_bytes.len() <= CONFIG_BUNDLE_ENCODED_MAX_BYTES);

    let reloaded = crate::commands::config_migrate::read_config_import_bundle(
        export_path.to_str().expect("utf8 export path"),
    )
    .expect("bounded reader accepts production export");
    let exported_skill = reloaded
        .installed_skills
        .as_ref()
        .expect("installed payload")
        .iter()
        .find(|skill| skill.skill_key == "roundtrip-installed")
        .expect("installed skill in export");
    assert_eq!(
        BASE64_STANDARD
            .decode(
                exported_skill
                    .files
                    .iter()
                    .find(|file| file.relative_path == "payload.bin")
                    .expect("installed payload file")
                    .content_base64
                    .as_bytes(),
            )
            .expect("decode installed payload"),
        installed_payload
    );
    let exported_local = reloaded
        .local_skills
        .as_ref()
        .expect("local payload")
        .iter()
        .find(|skill| skill.cli_key == "codex" && skill.dir_name == "roundtrip-local")
        .expect("local skill in export");
    assert_eq!(
        BASE64_STANDARD
            .decode(
                exported_local
                    .files
                    .iter()
                    .find(|file| file.relative_path == "payload.bin")
                    .expect("local payload file")
                    .content_base64
                    .as_bytes(),
            )
            .expect("decode local payload"),
        local_payload
    );
    let exported_prompt = reloaded
        .workspaces
        .iter()
        .find(|workspace| workspace.cli_key == "codex" && workspace.name == workspace_name)
        .expect("workspace in export")
        .prompts
        .iter()
        .find(|prompt| prompt.name == "roundtrip-prompt")
        .expect("prompt in export");
    assert_eq!(
        exported_prompt.content.as_bytes(),
        b"db-byte-exact-prompt-\0-utf8"
    );

    config_import(&app, &test_app.db, reloaded).expect("production export import");

    assert_eq!(
        std::fs::read(installed_dir.join("payload.bin")).expect("restored installed payload"),
        installed_payload
    );
    assert_eq!(
        std::fs::read(local_dir.join("payload.bin")).expect("restored local payload"),
        local_payload
    );
    let conn = test_app.db.open_connection().expect("open restored db");
    let prompt_content: String = conn
        .query_row(
            "SELECT content FROM prompts WHERE name = 'roundtrip-prompt'",
            [],
            |row| row.get(0),
        )
        .expect("restored prompt");
    assert_eq!(prompt_content.as_bytes(), b"db-byte-exact-prompt-\0-utf8");
    let skill: (String, String, String, String, String) = conn
        .query_row(
            "SELECT skill_key, description, source_git_url, source_branch, source_subdir FROM skills WHERE skill_key = 'roundtrip-installed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("restored skill DB row");
    assert_eq!(
        skill,
        (
            "roundtrip-installed".to_string(),
            "DB byte exact description".to_string(),
            "https://example.test/roundtrip.git".to_string(),
            "main".to_string(),
            "skills/roundtrip".to_string(),
        )
    );
    let enabled: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspace_skill_enabled wse JOIN skills s ON s.id = wse.skill_id WHERE s.skill_key = 'roundtrip-installed'",
            [],
            |row| row.get(0),
        )
        .expect("restored skill enablement");
    assert_eq!(enabled, 1);
}

#[test]
fn config_import_v2_restores_full_prompt_and_skill_payload() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let bundle = ConfigBundle {
        providers: vec![ProviderExport {
            id: Some(1),
            cli_key: "codex".to_string(),
            name: "oauth-provider".to_string(),
            base_urls: vec![
                "https://api.example.com".to_string(),
                "https://backup.example.com".to_string(),
            ],
            base_url_mode: "order".to_string(),
            api_key_plaintext: String::new(),
            auth_mode: "oauth".to_string(),
            oauth_provider_type: Some("openai".to_string()),
            oauth_access_token: Some("access-token".to_string()),
            oauth_refresh_token: Some("refresh-token".to_string()),
            oauth_id_token: Some("id-token".to_string()),
            oauth_token_expiry: Some(2_000_000_000),
            oauth_scopes: None,
            oauth_token_uri: Some("https://auth.example.com/token".to_string()),
            oauth_client_id: Some("client-id".to_string()),
            oauth_client_secret: Some("client-secret".to_string()),
            oauth_email: Some("dev@example.com".to_string()),
            oauth_refresh_lead_seconds: 7200,
            oauth_last_refreshed_at: Some(1_999_999_999),
            oauth_last_error: Some("last error".to_string()),
            claude_models_json: "{\"main\":\"gpt-5.4\"}".to_string(),
            supported_models_json: "{\"gpt-5.4\":true}".to_string(),
            model_mapping_json: "{\"gpt-5.4\":\"gpt-5.4\"}".to_string(),
            enabled: true,
            priority: 100,
            cost_multiplier: 1.25,
            limit_5h_usd: Some(1.0),
            limit_daily_usd: Some(2.0),
            limit_weekly_usd: Some(3.0),
            limit_monthly_usd: Some(4.0),
            limit_total_usd: Some(5.0),
            daily_reset_mode: "fixed".to_string(),
            daily_reset_time: "00:00:00".to_string(),
            tags_json: "[\"team\"]".to_string(),
            note: "note".to_string(),
            source_provider_id: None,
            source_provider_cli_key: None,
            bridge_type: None,
        }],
        sort_modes: Vec::new(),
        sort_mode_active: HashMap::new(),
        workspaces: vec![WorkspaceExport {
            cli_key: "codex".to_string(),
            name: "Imported".to_string(),
            is_active: true,
            prompts: vec![
                PromptExport {
                    name: "default".to_string(),
                    content: "prompt one".to_string(),
                    enabled: true,
                },
                PromptExport {
                    name: "review".to_string(),
                    content: "prompt two".to_string(),
                    enabled: false,
                },
            ],
            prompt: None,
        }],
        skill_repos: vec![SkillRepoExport {
            git_url: "https://example.com/repo.git".to_string(),
            branch: "main".to_string(),
            enabled: true,
        }],
        installed_skills: Some(vec![InstalledSkillExport {
            skill_key: "review-skill".to_string(),
            name: "Review Skill".to_string(),
            description: "Installed review skill".to_string(),
            source_git_url: "https://example.com/repo.git".to_string(),
            source_branch: "main".to_string(),
            source_subdir: "skills/review".to_string(),
            enabled_in_workspaces: vec![("codex".to_string(), "Imported".to_string())],
            files: vec![
                SkillFileExport {
                    relative_path: "SKILL.md".to_string(),
                    content_base64: BASE64_STANDARD.encode(
                        b"---\nname: Review Skill\ndescription: Installed review skill\n---\n",
                    ),
                },
                SkillFileExport {
                    relative_path: "README.md".to_string(),
                    content_base64: BASE64_STANDARD.encode(b"installed"),
                },
            ],
        }]),
        local_skills: Some(vec![LocalSkillExport {
            cli_key: "codex".to_string(),
            dir_name: "local-review".to_string(),
            name: "Local Review".to_string(),
            description: "Local review skill".to_string(),
            source_git_url: Some("https://example.com/local.git".to_string()),
            source_branch: Some("main".to_string()),
            source_subdir: Some("skills/local-review".to_string()),
            files: vec![
                SkillFileExport {
                    relative_path: "SKILL.md".to_string(),
                    content_base64: BASE64_STANDARD
                        .encode(b"---\nname: Local Review\ndescription: Local review skill\n---\n"),
                },
                SkillFileExport {
                    relative_path: "notes.txt".to_string(),
                    content_base64: BASE64_STANDARD.encode(b"local"),
                },
            ],
        }]),
        ..make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION)
    };

    let result = config_import(&app, &test_app.db, bundle).expect("config import");
    assert_eq!(result.providers_imported, 1);
    assert_eq!(result.prompts_imported, 2);
    assert_eq!(result.installed_skills_imported, 1);
    assert_eq!(result.local_skills_imported, 1);

    let conn = test_app.db.open_connection().expect("open db");
    let prompt_count: i64 = conn
        .query_row("SELECT COUNT(1) FROM prompts", [], |row| row.get(0))
        .expect("prompt count");
    assert_eq!(prompt_count, 2);

    let oauth_id_token: Option<String> = conn
        .query_row(
            "SELECT oauth_id_token FROM providers WHERE name = 'oauth-provider'",
            [],
            |row| row.get(0),
        )
        .expect("oauth id token");
    assert_eq!(oauth_id_token.as_deref(), Some("id-token"));

    let skill_enabled_count: i64 = conn
        .query_row("SELECT COUNT(1) FROM workspace_skill_enabled", [], |row| {
            row.get(0)
        })
        .expect("skill enabled count");
    assert_eq!(skill_enabled_count, 1);

    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    assert!(ssot_root.join("review-skill").join("README.md").exists());

    let local_root = cli_skills_root(&app, "codex").expect("local root");
    assert!(local_root.join("local-review").join("notes.txt").exists());
    assert!(local_root.join("review-skill").join("SKILL.md").exists());

    let prompt_bytes = crate::prompt_sync::read_target_bytes(&app, "codex")
        .expect("read prompt target")
        .expect("prompt target exists");
    assert_eq!(
        String::from_utf8(prompt_bytes)
            .expect("utf8")
            .trim_end_matches('\n'),
        "prompt one"
    );
}

#[test]
fn config_import_failure_restores_grok_runtime_and_skill_files() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let config_path = crate::grok_config::config_path(&app).expect("Grok config path");
    std::fs::create_dir_all(config_path.parent().expect("Grok config parent"))
        .expect("create Grok home");
    std::fs::write(
        &config_path,
        b"# original\n[model.aio]\nmodel = \"grok-build\"\n",
    )
    .expect("write initial Grok config");
    crate::mcp_sync::sync_cli(&app, "grok", &[]).expect("seed Grok MCP manifest");
    crate::prompt_sync::apply_enabled_prompt(&app, "grok", 42, "original instructions")
        .expect("seed Grok prompt runtime");

    let invalid_config = b"[mcp_servers\ninvalid = true\n";
    std::fs::write(&config_path, invalid_config).expect("write invalid Grok config");
    let prompt_target_before =
        crate::prompt_sync::read_target_bytes(&app, "grok").expect("read Grok prompt target");
    let prompt_manifest_before =
        crate::prompt_sync::read_manifest_bytes(&app, "grok").expect("read Grok prompt manifest");
    let mcp_manifest_before =
        crate::mcp_sync::read_manifest_bytes(&app, "grok").expect("read Grok MCP manifest");

    let ssot_root = ssot_skills_root(&app).expect("SSOT root");
    write_skill_md(
        &ssot_root.join("existing-installed"),
        "Existing Installed",
        "Existing installed skill",
    );
    let grok_skills_root = cli_skills_root(&app, "grok").expect("Grok skills root");
    write_skill_md(
        &grok_skills_root.join("existing-local"),
        "Existing Local",
        "Existing local skill",
    );
    std::fs::write(
        grok_skills_root.join("existing-local").join("notes.txt"),
        "keep me",
    )
    .expect("write existing local skill file");

    let bundle = ConfigBundle {
        installed_skills: Some(vec![InstalledSkillExport {
            skill_key: "imported-installed".to_string(),
            name: "Imported Installed".to_string(),
            description: "Imported installed skill".to_string(),
            source_git_url: "https://example.test/imported.git".to_string(),
            source_branch: "main".to_string(),
            source_subdir: "skills/imported".to_string(),
            enabled_in_workspaces: Vec::new(),
            files: vec![SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD.encode(
                    b"---\nname: Imported Installed\ndescription: Imported installed skill\n---\n",
                ),
            }],
        }]),
        local_skills: Some(vec![LocalSkillExport {
            cli_key: "grok".to_string(),
            dir_name: "imported-local".to_string(),
            name: "Imported Local".to_string(),
            description: "Imported local skill".to_string(),
            source_git_url: None,
            source_branch: None,
            source_subdir: None,
            files: vec![SkillFileExport {
                relative_path: "SKILL.md".to_string(),
                content_base64: BASE64_STANDARD
                    .encode(b"---\nname: Imported Local\ndescription: Imported local skill\n---\n"),
            }],
        }]),
        ..make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION)
    };

    let Err(error) = config_import(&app, &test_app.db, bundle) else {
        panic!("invalid Grok TOML must fail runtime sync");
    };

    assert!(error.to_string().contains("GROK_CONFIG_INVALID_TOML"));
    assert_eq!(
        std::fs::read(&config_path).expect("read restored Grok config"),
        invalid_config
    );
    assert_eq!(
        crate::prompt_sync::read_target_bytes(&app, "grok")
            .expect("read restored Grok prompt target"),
        prompt_target_before
    );
    assert_eq!(
        crate::prompt_sync::read_manifest_bytes(&app, "grok")
            .expect("read restored Grok prompt manifest"),
        prompt_manifest_before
    );
    assert_eq!(
        crate::mcp_sync::read_manifest_bytes(&app, "grok")
            .expect("read restored Grok MCP manifest"),
        mcp_manifest_before
    );
    assert!(ssot_root
        .join("existing-installed")
        .join("SKILL.md")
        .exists());
    assert!(!ssot_root.join("imported-installed").exists());
    assert_eq!(
        std::fs::read_to_string(grok_skills_root.join("existing-local").join("notes.txt"))
            .expect("read restored local skill"),
        "keep me"
    );
    assert!(!grok_skills_root.join("imported-local").exists());

    let conn = test_app.db.open_connection().expect("open restored db");
    let imported_workspace_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM workspaces WHERE name = 'Imported'",
            [],
            |row| row.get(0),
        )
        .expect("count imported workspaces");
    assert_eq!(imported_workspace_count, 0);
}

#[test]
fn config_import_v1_keeps_existing_skill_state() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let conn = test_app.db.open_connection().expect("open db");
    let (codex_workspace_id, _) = query_workspace(&conn, "codex");

    conn.execute(
        r#"
INSERT INTO skills(
  skill_key, name, normalized_name, description, source_git_url, source_branch, source_subdir,
  created_at, updated_at
) VALUES (
  'existing-skill', 'Existing Skill', 'existing-skill', 'Existing skill',
  'https://example.com/existing.git', 'main', 'skills/existing', 1, 1
)
"#,
        [],
    )
    .expect("insert existing skill");
    let existing_skill_id = conn.last_insert_rowid();
    conn.execute(
        r#"
INSERT INTO workspace_skill_enabled(workspace_id, skill_id, created_at, updated_at)
VALUES (?1, ?2, 1, 1)
"#,
        params![codex_workspace_id, existing_skill_id],
    )
    .expect("enable existing skill");

    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    write_skill_md(
        &ssot_root.join("existing-skill"),
        "Existing Skill",
        "Existing skill",
    );

    let local_root = cli_skills_root(&app, "codex").expect("local root");
    write_skill_md(
        &local_root.join("existing-local"),
        "Existing Local",
        "Local skill",
    );

    let bundle = ConfigBundle {
        workspaces: vec![WorkspaceExport {
            cli_key: "codex".to_string(),
            name: "Imported".to_string(),
            is_active: true,
            prompts: Vec::new(),
            prompt: Some(PromptExport {
                name: "default".to_string(),
                content: "legacy prompt".to_string(),
                enabled: true,
            }),
        }],
        installed_skills: None,
        local_skills: None,
        ..make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION_V1)
    };

    let result = config_import(&app, &test_app.db, bundle).expect("config import");
    assert_eq!(result.prompts_imported, 1);
    assert_eq!(result.installed_skills_imported, 0);
    assert_eq!(result.local_skills_imported, 0);

    let conn = test_app.db.open_connection().expect("open db");
    let skill_count: i64 = conn
        .query_row("SELECT COUNT(1) FROM skills", [], |row| row.get(0))
        .expect("skill count");
    assert_eq!(skill_count, 1);
    let restored_enabled_count: i64 = conn
        .query_row(
            r#"
SELECT COUNT(1)
FROM workspace_skill_enabled e
JOIN skills s ON s.id = e.skill_id
WHERE s.skill_key = 'existing-skill'
"#,
            [],
            |row| row.get(0),
        )
        .expect("restored enabled skill count");
    assert_eq!(restored_enabled_count, 1);
    assert!(ssot_root.join("existing-skill").join("SKILL.md").exists());
    assert!(local_root.join("existing-local").join("SKILL.md").exists());
    assert!(local_root.join("existing-skill").join("SKILL.md").exists());
}

#[test]
fn config_import_v2_rejects_missing_installed_skills_payload() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.installed_skills = None;

    let Err(err) = config_import(&app, &test_app.db, bundle) else {
        panic!("missing installed_skills");
    };
    assert!(err
        .to_string()
        .contains("SEC_INVALID_INPUT: config bundle missing installed_skills"));
}

#[test]
fn config_import_v2_rejects_missing_local_skills_payload() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.local_skills = None;

    let Err(err) = config_import(&app, &test_app.db, bundle) else {
        panic!("missing local_skills");
    };
    assert!(err
        .to_string()
        .contains("SEC_INVALID_INPUT: config bundle missing local_skills"));
}

#[test]
fn validate_local_skills_for_import_rejects_unknown_cli_key() {
    let err = import::validate_local_skills_for_import(&[LocalSkillExport {
        cli_key: "cursor".to_string(),
        dir_name: "local-review".to_string(),
        name: "Local Review".to_string(),
        description: "Local review skill".to_string(),
        source_git_url: None,
        source_branch: None,
        source_subdir: None,
        files: vec![SkillFileExport {
            relative_path: "SKILL.md".to_string(),
            content_base64: BASE64_STANDARD
                .encode(b"---\nname: Local Review\ndescription: Local review skill\n---\n"),
        }],
    }])
    .expect_err("unknown cli key should fail");
    assert!(err
        .to_string()
        .contains("SEC_INVALID_INPUT: unknown local skill cli_key=cursor"));
}

#[test]
fn installed_skill_key_requires_one_portable_normal_component() {
    assert_eq!(
        validate_installed_skill_key(" valid-skill ").expect("valid key"),
        "valid-skill"
    );
    for invalid in [
        "",
        ".",
        "..",
        "../escape",
        "nested/escape",
        "nested\\escape",
        "/absolute",
        "C:\\absolute",
        "C:/absolute",
        "\\\\server\\share",
    ] {
        assert!(
            validate_installed_skill_key(invalid).is_err(),
            "accepted invalid key {invalid:?}"
        );
    }
}

#[test]
fn config_import_rejects_traversal_skill_key_without_fs_or_db_activation() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    write_skill_md(&ssot_root.join("existing"), "Existing", "keep");
    let app_data = crate::app_paths::app_data_dir(&app).expect("app data");
    let escaped = app_data.join("escape");

    let mut bundle = make_test_bundle(CONFIG_BUNDLE_SCHEMA_VERSION);
    bundle.installed_skills = Some(vec![InstalledSkillExport {
        skill_key: "../escape".to_string(),
        name: "Escape".to_string(),
        description: String::new(),
        source_git_url: "https://example.test/repo.git".to_string(),
        source_branch: "main".to_string(),
        source_subdir: "skills/escape".to_string(),
        enabled_in_workspaces: Vec::new(),
        files: vec![SkillFileExport {
            relative_path: "SKILL.md".to_string(),
            content_base64: BASE64_STANDARD.encode(b"---\nname: Escape\n---\n"),
        }],
    }]);
    bundle.local_skills = Some(Vec::new());

    let Err(error) = config_import(&app, &test_app.db, bundle) else {
        panic!("traversal must fail");
    };
    assert!(error.to_string().contains("invalid installed skill_key"));
    assert!(!escaped.exists());
    assert!(ssot_root.join("existing").join("SKILL.md").exists());
    assert_eq!(
        test_app
            .db
            .open_connection()
            .expect("open db")
            .query_row("SELECT COUNT(1) FROM skills", [], |row| row
                .get::<_, i64>(0))
            .expect("skill count"),
        0
    );
}

#[cfg(unix)]
#[test]
fn export_skill_dir_files_rejects_symlink_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("local-review");
    write_skill_md(&skill_dir, "Local Review", "Local review skill");

    let outside_file = temp.path().join("outside.txt");
    std::fs::write(&outside_file, "secret").expect("write outside file");
    create_file_symlink(&outside_file, &skill_dir.join("escape.txt"));

    let Err(err) = export_skill_dir_files(&skill_dir, true) else {
        panic!("symlink escape should fail");
    };
    assert!(err
        .to_string()
        .contains("SKILL_EXPORT_BLOCKED_SYMLINK_ESCAPE"));
}

#[test]
fn config_export_rejects_ssot_top_level_directory_link() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let outside = tempfile::tempdir().expect("outside tempdir");
    write_skill_md(outside.path(), "Outside", "Must not be exported");
    std::fs::write(
        outside.path().join("opaque.bin"),
        b"random-outside-capability-bytes",
    )
    .expect("outside bytes");

    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    std::fs::create_dir_all(&ssot_root).expect("create ssot root");
    create_dir_link(outside.path(), &ssot_root.join("linked-skill"));
    let conn = test_app.db.open_connection().expect("open db");
    insert_installed_skill(&conn, "linked-skill");

    let error = match config_export(&app, &test_app.db) {
        Ok(_) => panic!("top-level link must fail closed"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("trusted directory"));
}

#[test]
fn config_export_ignores_local_top_level_directory_link() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let cli_key =
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
            .next()
            .expect("skills CLI");
    let local_root = cli_skills_root(&app, cli_key).expect("local root");
    std::fs::create_dir_all(&local_root).expect("create local root");
    let outside = tempfile::tempdir().expect("outside tempdir");
    write_skill_md(outside.path(), "Outside Local", "Must not be exported");
    std::fs::write(
        outside.path().join("opaque.bin"),
        b"random-outside-capability-bytes",
    )
    .expect("outside bytes");
    create_dir_link(outside.path(), &local_root.join("linked-local"));

    let bundle = config_export(&app, &test_app.db).expect("linked local is not authority");
    assert!(bundle
        .local_skills
        .expect("local skill payload")
        .iter()
        .all(|skill| skill.dir_name != "linked-local"));
}

#[test]
fn config_export_aggregate_payload_budget_is_shared_by_installed_and_local_skills() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let conn = test_app.db.open_connection().expect("open db");
    let installed_key = "aggregate-installed";
    insert_distinct_installed_skill(&conn, installed_key);
    let installed_dir = ssot_root.join(installed_key);
    write_skill_md(&installed_dir, installed_key, "aggregate payload boundary");
    let installed_md_len = std::fs::metadata(installed_dir.join("SKILL.md"))
        .expect("installed SKILL.md metadata")
        .len() as usize;
    std::fs::write(
        installed_dir.join("payload.bin"),
        vec![0x11; CONFIG_SKILL_TOTAL_MAX_BYTES - installed_md_len],
    )
    .expect("write installed aggregate payload");
    drop(conn);

    let cli_key =
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
            .next()
            .expect("skills CLI");
    let local_root = cli_skills_root(&app, cli_key).expect("local root");
    for index in 0..4 {
        let local_dir = local_root.join(format!("aggregate-local-{index}"));
        write_skill_md(&local_dir, "Aggregate local", "aggregate payload boundary");
        let skill_md_len = std::fs::metadata(local_dir.join("SKILL.md"))
            .expect("local SKILL.md metadata")
            .len() as usize;
        std::fs::write(
            local_dir.join("payload.bin"),
            vec![index as u8; CONFIG_SKILL_TOTAL_MAX_BYTES - skill_md_len],
        )
        .expect("write local aggregate payload");
    }
    let local_dir = local_root.join("aggregate-local-z-overflow");
    write_skill_md(
        &local_dir,
        "Aggregate local overflow",
        "cross-root aggregate boundary",
    );
    std::fs::write(local_dir.join("payload.bin"), vec![0xA5; 3 * 1024 * 1024])
        .expect("write local overflow payload");

    let target = test_app.home.path().join("aggregate-payload-export.json");
    std::fs::write(&target, b"SENTINEL-PAYLOAD").expect("write payload sentinel");
    let error = crate::commands::config_migrate::config_export_to_path(&app, &test_app.db, &target)
        .expect_err("installed plus local encoded payload must exceed aggregate budget");

    assert!(
        error.contains("SEC_INVALID_INPUT: skill export aggregate encoded payload"),
        "unexpected aggregate payload error: {error}"
    );
    assert_eq!(
        std::fs::read(&target).expect("read payload sentinel"),
        b"SENTINEL-PAYLOAD"
    );
}

#[test]
fn config_export_aggregate_file_budget_is_shared_by_installed_and_local_skills() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let conn = test_app.db.open_connection().expect("open db");
    let installed_key = "aggregate-files-installed";
    insert_distinct_installed_skill(&conn, installed_key);
    let installed_dir = ssot_root.join(installed_key);
    write_skill_md(&installed_dir, installed_key, "aggregate file boundary");
    for file_index in 0..255 {
        std::fs::write(
            installed_dir.join(format!("file-{file_index:03}.bin")),
            [0x11, file_index as u8],
        )
        .expect("write installed aggregate file");
    }
    drop(conn);

    let cli_key =
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
            .next()
            .expect("skills CLI");
    let local_root = cli_skills_root(&app, cli_key).expect("local root");
    for skill_index in 0..7 {
        let local_dir = local_root.join(format!("aggregate-files-local-{skill_index}"));
        write_skill_md(
            &local_dir,
            "Aggregate local files",
            "aggregate file boundary",
        );
        for file_index in 0..255 {
            std::fs::write(
                local_dir.join(format!("file-{file_index:03}.bin")),
                [skill_index as u8, file_index as u8],
            )
            .expect("write local aggregate file");
        }
    }
    let local_dir = local_root.join("aggregate-files-z-overflow");
    write_skill_md(&local_dir, "Aggregate file overflow", "2049th export file");

    let target = test_app.home.path().join("aggregate-files-export.json");
    std::fs::write(&target, b"SENTINEL-FILES").expect("write file-count sentinel");
    let error = crate::commands::config_migrate::config_export_to_path(&app, &test_app.db, &target)
        .expect_err("installed plus local files must exceed aggregate file budget");

    assert!(
        error.contains("SEC_INVALID_INPUT: too many skill export aggregate files"),
        "unexpected aggregate file error: {error}"
    );
    assert_eq!(
        std::fs::read(&target).expect("read file-count sentinel"),
        b"SENTINEL-FILES"
    );
}

#[test]
fn config_export_large_legal_payload_round_trips_without_content_filtering() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let conn = test_app.db.open_connection().expect("open db");
    insert_distinct_installed_skill(&conn, "large-legal-roundtrip");
    drop(conn);
    let skill_dir = ssot_skills_root(&app)
        .expect("ssot root")
        .join("large-legal-roundtrip");
    write_skill_md(&skill_dir, "Large legal roundtrip", "arbitrary bytes");
    let mut payload = vec![0xA5; 1024 * 1024 + 17];
    let sensitive_looking = b"SYNTHETIC_SECRET\0\xff";
    payload[..sensitive_looking.len()].copy_from_slice(sensitive_looking);
    std::fs::write(skill_dir.join("opaque.bin"), &payload).expect("write legal payload");

    let target = test_app.home.path().join("large-legal-export.json");
    crate::commands::config_migrate::config_export_to_path(&app, &test_app.db, &target)
        .expect("large legal production export");
    let bundle: ConfigBundle =
        serde_json::from_slice(&std::fs::read(&target).expect("read exported bundle"))
            .expect("parse exported bundle");
    drop(test_app);

    let imported_app = ConfigMigrateTestApp::new();
    let imported_handle = imported_app.handle();
    config_import(&imported_handle, &imported_app.db, bundle).expect("import exported bundle");
    assert_eq!(
        std::fs::read(
            ssot_skills_root(&imported_handle)
                .expect("imported ssot root")
                .join("large-legal-roundtrip/opaque.bin")
        )
        .expect("read imported payload"),
        payload
    );
}

#[test]
fn config_export_rejects_ssot_top_level_rebind_after_enumeration() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let ssot_root = ssot_skills_root(&app).expect("ssot root");
    let skill_dir = ssot_root.join("raced-skill");
    write_skill_md(&skill_dir, "Trusted", "Original directory");
    let moved = ssot_root.join("raced-skill-original");
    let outside = tempfile::tempdir().expect("outside tempdir");
    write_skill_md(outside.path(), "Outside", "Must not be exported");
    std::fs::write(outside.path().join("opaque.bin"), b"outside-race-bytes")
        .expect("outside bytes");
    let hook_skill = skill_dir.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.path().to_path_buf();
    set_after_skill_export_enumeration_test_hook(Box::new(move || {
        std::fs::rename(&hook_skill, &hook_moved).expect("move enumerated Skill");
        create_dir_link(&hook_outside, &hook_skill);
    }));
    let conn = test_app.db.open_connection().expect("open db");
    insert_installed_skill(&conn, "raced-skill");

    let error = match config_export(&app, &test_app.db) {
        Ok(_) => panic!("rebind must fail closed"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
}

#[test]
fn config_export_rejects_local_top_level_rebind_after_enumeration() {
    let test_app = ConfigMigrateTestApp::new();
    let app = test_app.handle();
    let cli_key =
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
            .next()
            .expect("skills CLI");
    let local_root = cli_skills_root(&app, cli_key).expect("local root");
    let skill_dir = local_root.join("raced-local");
    write_skill_md(&skill_dir, "Trusted Local", "Original directory");
    let moved = local_root.join("raced-local-original");
    let outside = tempfile::tempdir().expect("outside tempdir");
    write_skill_md(outside.path(), "Outside Local", "Must not be exported");
    std::fs::write(outside.path().join("opaque.bin"), b"outside-race-bytes")
        .expect("outside bytes");
    let hook_skill = skill_dir.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.path().to_path_buf();
    set_after_skill_export_enumeration_test_hook(Box::new(move || {
        std::fs::rename(&hook_skill, &hook_moved).expect("move enumerated local Skill");
        create_dir_link(&hook_outside, &hook_skill);
    }));

    let error = match config_export(&app, &test_app.db) {
        Ok(_) => panic!("rebind must fail closed"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
}

#[test]
fn export_skill_dir_files_rejects_hardlink_swap_after_handle_enumeration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("skill");
    write_skill_md(&skill_dir, "Handle Bound", "Handle bound export");
    let payload = skill_dir.join("payload.bin");
    let moved = skill_dir.join("payload-original.bin");
    let outside = temp.path().join("outside.bin");
    std::fs::write(&payload, b"trusted-root-bytes").expect("trusted payload");
    std::fs::write(&outside, b"SYNTHETIC_SECRET_OUTSIDE").expect("outside payload");
    let hook_payload = payload.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.clone();
    set_after_skill_export_enumeration_test_hook(Box::new(move || {
        std::fs::rename(&hook_payload, &hook_moved).expect("move enumerated payload");
        std::fs::hard_link(&hook_outside, &hook_payload).expect("replace with hardlink");
    }));

    let error = match export_skill_dir_files(&skill_dir, true) {
        Ok(_) => panic!("enumerated file identity replacement must fail closed"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
    assert_eq!(
        std::fs::read(&outside).expect("outside remains unchanged"),
        b"SYNTHETIC_SECRET_OUTSIDE"
    );
}

#[test]
fn export_skill_dir_files_rejects_directory_rebind_after_handle_enumeration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("skill");
    let nested = skill_dir.join("assets");
    write_skill_md(&skill_dir, "Handle Bound", "Handle bound export");
    std::fs::create_dir_all(&nested).expect("nested");
    std::fs::write(nested.join("inside.bin"), b"trusted-root-bytes").expect("inside");
    let moved = skill_dir.join("assets-original");
    let outside = temp.path().join("outside-dir");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("outside.bin"), b"SYNTHETIC_SECRET_OUTSIDE").expect("outside");
    let hook_nested = nested.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.clone();
    set_after_skill_export_enumeration_test_hook(Box::new(move || {
        std::fs::rename(&hook_nested, &hook_moved).expect("move enumerated directory");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&hook_outside, &hook_nested).expect("directory symlink");
        #[cfg(windows)]
        junction::create(&hook_outside, &hook_nested).expect("directory junction");
    }));

    let error = match export_skill_dir_files(&skill_dir, true) {
        Ok(_) => panic!("enumerated directory replacement must fail closed"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
    assert_eq!(
        std::fs::read(outside.join("outside.bin")).expect("outside remains unchanged"),
        b"SYNTHETIC_SECRET_OUTSIDE"
    );
}

#[test]
fn skill_root_sensitive_looking_bytes_round_trip_without_content_filtering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("skill");
    write_skill_md(&skill_dir, "Opaque Bytes", "No content filtering");
    let expected = b"prefix\0SYNTHETIC_SECRET\xffcredential-looking-suffix";
    std::fs::write(skill_dir.join("opaque.bin"), expected).expect("opaque bytes");

    let files = export_skill_dir_files(&skill_dir, true).expect("export root-owned bytes");
    let target = temp.path().join("imported");
    write_skill_files_to_dir(&target, &files, None).expect("import root-owned bytes");
    assert_eq!(
        std::fs::read(target.join("opaque.bin")).expect("round-tripped bytes"),
        expected
    );
}

#[cfg(unix)]
#[test]
fn config_export_fifo_replacement_fails_closed_under_external_watchdog() {
    const TEST_FILTER: &str = "config_export_fifo_replacement_fails_closed_under_external_watchdog";
    if std::env::var_os("AIO_CONFIG_EXPORT_FIFO_WATCHDOG_CHILD").is_some() {
        use std::os::unix::ffi::OsStrExt as _;

        let temp = tempfile::tempdir().expect("watchdog tempdir");
        let skill_dir = temp.path().join("fifo-race-skill");
        write_skill_md(&skill_dir, "FIFO race", "regular file before replacement");
        let regular = skill_dir.join("race.bin");
        let moved = skill_dir.join("race-original.bin");
        std::fs::write(&regular, b"regular-before-fifo").expect("regular fixture");
        let hook_regular = regular.clone();
        let hook_moved = moved.clone();
        set_after_skill_export_enumeration_test_hook(Box::new(move || {
            std::fs::rename(&hook_regular, &hook_moved).expect("move enumerated regular file");
            let c_path =
                std::ffi::CString::new(hook_regular.as_os_str().as_bytes()).expect("fifo path");
            assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        }));

        let error = export_skill_dir_files(&skill_dir, true)
            .expect_err("FIFO replacement must fail closed");
        assert!(error.to_string().contains("SEC_INVALID_INPUT"), "{error}");
        return;
    }

    let mut child =
        std::process::Command::new(std::env::current_exe().expect("current test executable"))
            .arg(TEST_FILTER)
            .arg("--nocapture")
            .env("AIO_CONFIG_EXPORT_FIFO_WATCHDOG_CHILD", "1")
            .spawn()
            .expect("spawn export watchdog child");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait().expect("poll export watchdog child") {
            Some(status) => {
                assert!(status.success(), "FIFO child failed: {status}");
                break;
            }
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("production config export did not fail closed before watchdog deadline");
            }
            None => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

#[test]
fn config_skill_file_budget_matches_existing_safety_budgets() {
    assert_eq!(CONFIG_SKILL_FILE_MAX_BYTES, CONFIG_SKILL_TOTAL_MAX_BYTES);
    assert_eq!(CONFIG_SKILL_TOTAL_MAX_BYTES, 8 * 1024 * 1024);
    assert_eq!(CONFIG_SKILL_FILE_COUNT_MAX, 256);
    assert_eq!(CONFIG_IMPORT_FILE_MAX_BYTES, 64 * 1024 * 1024);
    assert_eq!(CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES, 64 * 1024);
    assert_eq!(CONFIG_SKILL_MD_MAX_BYTES, 256 * 1024);
}

#[test]
fn export_skill_dir_files_accepts_file_above_legacy_one_mib_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("local-review");
    write_skill_md(&skill_dir, "Local Review", "Local review skill");
    let legacy_limit = 1024 * 1024;
    std::fs::write(
        skill_dir.join("large.bin"),
        synthetic_bytes(legacy_limit + 1),
    )
    .expect("write large file");

    let files = export_skill_dir_files(&skill_dir, true).expect("export skill files");
    let file = files
        .iter()
        .find(|file| file.relative_path == "large.bin")
        .expect("large file export");

    assert_eq!(
        BASE64_STANDARD
            .decode(file.content_base64.as_bytes())
            .expect("decode large file"),
        synthetic_bytes(legacy_limit + 1)
    );
}

#[test]
fn export_skill_dir_files_rejects_growth_after_metadata_via_handle_reader() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("growth-skill");
    write_skill_md(&skill_dir, "Growth", "growth skill");
    let path = skill_dir.join("grow.bin");
    std::fs::write(&path, b"abcd").expect("seed");

    let grow_path = path.clone();
    set_after_skill_export_file_metadata_test_hook(Box::new(move || {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&grow_path)
            .expect("append open");
        // Grow well past any residual SKILL.md/source budgets so the ordinary
        // file hard limit rejects at limit+1 from the same handle.
        file.write_all(&vec![b'x'; CONFIG_SKILL_FILE_MAX_BYTES])
            .expect("append growth");
    }));

    let err = match export_skill_dir_files(&skill_dir, true) {
        Ok(_) => panic!("growth must fail"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("too large") || err.to_string().contains("SEC_INVALID_INPUT"),
        "unexpected: {err}"
    );
}

#[test]
fn nested_png_like_asset_round_trips_byte_for_byte() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("synthetic-skill");
    let assets_dir = skill_dir.join("assets").join("nested");
    write_skill_md(&skill_dir, "Synthetic Skill", "Synthetic fixture");
    std::fs::create_dir_all(&assets_dir).expect("create assets dir");
    let expected = synthetic_png_like_bytes(2 * 1024 * 1024 + 17);
    std::fs::write(assets_dir.join("fixture.png"), &expected).expect("write synthetic asset");

    let files = export_skill_dir_files(&skill_dir, true).expect("export nested asset");
    let target = temp.path().join("imported-skill");
    write_skill_files_to_dir(&target, &files, None).expect("import nested asset");

    let actual = std::fs::read(target.join("assets/nested/fixture.png"))
        .expect("read imported synthetic asset");
    assert_eq!(actual, expected);
}

#[test]
fn atomic_temp_like_skill_names_round_trip_in_both_orders() {
    let temp = tempfile::tempdir().expect("tempdir");
    let metadata = SkillSourceMetadataFile {
        source_git_url: "https://example.invalid/repo.git".to_string(),
        source_branch: "main".to_string(),
        source_subdir: "skills/example".to_string(),
    };
    let make_files = || {
        vec![
            SkillFileExport {
                relative_path: ".aio-coding-hub.source.json.aio-tmp".to_string(),
                content_base64: BASE64_STANDARD.encode(b"payload-marker-temp-name"),
            },
            SkillFileExport {
                relative_path: "a.aio-tmp".to_string(),
                content_base64: BASE64_STANDARD.encode(b"payload-a-temp"),
            },
            SkillFileExport {
                relative_path: "a".to_string(),
                content_base64: BASE64_STANDARD.encode(b"payload-a"),
            },
        ]
    };
    let reversed = make_files().into_iter().rev().collect::<Vec<_>>();

    for (index, ordered) in [make_files(), reversed].into_iter().enumerate() {
        let target = temp.path().join(format!("target-{index}"));
        write_skill_files_to_dir(&target, &ordered, Some(&metadata))
            .expect("temp-like legal payload names must import");
        assert_eq!(
            std::fs::read(target.join(".aio-coding-hub.source.json.aio-tmp"))
                .expect("read marker-like payload"),
            b"payload-marker-temp-name"
        );
        assert_eq!(
            std::fs::read(target.join("a.aio-tmp")).expect("read a temp payload"),
            b"payload-a-temp"
        );
        assert_eq!(
            std::fs::read(target.join("a")).expect("read a payload"),
            b"payload-a"
        );
        assert!(target.join(".aio-coding-hub.source.json").is_file());
    }
}

#[test]
fn exporter_to_importer_round_trips_atomic_temp_like_skill_names() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).expect("create source");
    let expected = [
        (
            ".aio-coding-hub.source.json.aio-tmp",
            b"exported-marker-temp-name".as_slice(),
        ),
        ("a.aio-tmp", b"exported-a-temp".as_slice()),
        ("a", b"exported-a".as_slice()),
    ];
    for (name, bytes) in expected {
        std::fs::write(source.join(name), bytes).expect("write source payload");
    }

    let exported = export_skill_dir_files(&source, true).expect("export real skill directory");
    let target = temp.path().join("target");
    write_skill_files_to_dir(&target, &exported, None).expect("import exported skill payload");

    for (name, bytes) in expected {
        assert_eq!(
            std::fs::read(target.join(name)).expect("read imported payload"),
            bytes,
            "round-trip bytes for {name}"
        );
    }
}

#[test]
fn exact_eight_mib_single_file_round_trips() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).expect("create source dir");
    std::fs::write(
        source.join("payload.bin"),
        synthetic_bytes(CONFIG_SKILL_FILE_MAX_BYTES),
    )
    .expect("write boundary payload");

    let files = export_skill_dir_files(&source, true).expect("export boundary payload");
    let target = temp.path().join("target");
    write_skill_files_to_dir(&target, &files, None).expect("import boundary payload");
    drop(files);

    let actual = std::fs::read(target.join("payload.bin")).expect("read boundary payload");
    assert_eq!(actual.len(), CONFIG_SKILL_FILE_MAX_BYTES);
    assert_synthetic_bytes(&actual);
}

#[test]
fn eight_mib_plus_one_is_rejected_on_export_and_import_before_writing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).expect("create source dir");
    std::fs::write(
        source.join("payload.bin"),
        synthetic_bytes(CONFIG_SKILL_FILE_MAX_BYTES + 1),
    )
    .expect("write oversized payload");

    let Err(export_err) = export_skill_dir_files(&source, true) else {
        panic!("oversized payload export should fail");
    };
    assert!(export_err.to_string().contains("too large"));

    let target = temp.path().join("target");
    let files = vec![SkillFileExport {
        relative_path: "payload.bin".to_string(),
        content_base64: BASE64_STANDARD.encode(synthetic_bytes(CONFIG_SKILL_FILE_MAX_BYTES + 1)),
    }];
    let import_err = write_skill_files_to_dir(&target, &files, None)
        .expect_err("oversized payload import should fail");
    assert!(import_err.to_string().contains("too large"));
    assert!(!target.exists());
}

#[test]
fn aggregate_payload_above_eight_mib_is_rejected_on_export_and_import() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).expect("create source dir");
    let first_len = CONFIG_SKILL_TOTAL_MAX_BYTES / 2;
    let second_len = first_len + 1;
    std::fs::write(source.join("a.bin"), synthetic_bytes(first_len)).expect("write first file");
    std::fs::write(source.join("b.bin"), synthetic_bytes(second_len)).expect("write second file");

    let Err(export_err) = export_skill_dir_files(&source, true) else {
        panic!("aggregate export payload should fail");
    };
    assert!(export_err.to_string().contains("payload too large"));

    let files = vec![
        SkillFileExport {
            relative_path: "a.bin".to_string(),
            content_base64: BASE64_STANDARD.encode(synthetic_bytes(first_len)),
        },
        SkillFileExport {
            relative_path: "b.bin".to_string(),
            content_base64: BASE64_STANDARD.encode(synthetic_bytes(second_len)),
        },
    ];
    let target = temp.path().join("target");
    let import_err = write_skill_files_to_dir(&target, &files, None)
        .expect_err("aggregate import payload should fail");
    assert!(import_err.to_string().contains("payload too large"));
    assert!(!target.exists());
}

#[test]
fn write_skill_files_to_dir_rejects_too_many_files_before_creating_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("local-review");
    let files: Vec<SkillFileExport> = (0..=CONFIG_SKILL_FILE_COUNT_MAX)
        .map(|index| SkillFileExport {
            relative_path: format!("{index}.txt"),
            content_base64: BASE64_STANDARD.encode(b"x"),
        })
        .collect();

    let err = write_skill_files_to_dir(&target, &files, None)
        .expect_err("too many skill files should fail");

    assert!(err.to_string().contains("too many skill files"));
    assert!(!target.exists());
}

#[test]
fn export_skill_dir_files_rejects_too_many_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("synthetic-source");
    std::fs::create_dir_all(&source).expect("create source dir");
    for index in 0..=CONFIG_SKILL_FILE_COUNT_MAX {
        std::fs::write(source.join(format!("{index:03}.txt")), b"x").expect("write synthetic file");
    }

    let Err(err) = export_skill_dir_files(&source, true) else {
        panic!("too many exported skill files should fail");
    };

    assert!(err.to_string().contains("too many skill files"));
}

#[test]
fn write_skill_files_to_dir_rejects_base64_above_derived_limit_before_creating_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("synthetic-target");
    let derived_base64_limit = CONFIG_SKILL_FILE_MAX_BYTES.div_ceil(3) * 4;
    let files = vec![SkillFileExport {
        relative_path: "large.bin".to_string(),
        content_base64: "A".repeat(derived_base64_limit + 1),
    }];

    let err =
        write_skill_files_to_dir(&target, &files, None).expect_err("oversized base64 should fail");

    assert!(err.to_string().contains("too large"));
    assert!(!target.exists());
}

#[test]
fn write_skill_files_to_dir_rejects_decoded_file_above_limit_before_creating_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("synthetic-target");
    let derived_base64_limit = CONFIG_SKILL_FILE_MAX_BYTES.div_ceil(3) * 4;
    let content_base64 = BASE64_STANDARD.encode(synthetic_bytes(CONFIG_SKILL_FILE_MAX_BYTES + 1));
    assert_eq!(content_base64.len(), derived_base64_limit);
    assert!(content_base64.len() <= derived_base64_limit);
    let files = vec![SkillFileExport {
        relative_path: "large.bin".to_string(),
        content_base64,
    }];

    let err = write_skill_files_to_dir(&target, &files, None)
        .expect_err("oversized skill file should fail");

    assert!(err.to_string().contains("too large"));
    assert!(!target.exists());
}

#[test]
fn write_skill_files_to_dir_rejects_duplicate_traversal_and_long_paths_before_creating_dir() {
    let duplicate = vec![
        SkillFileExport {
            relative_path: "same.txt".to_string(),
            content_base64: BASE64_STANDARD.encode(b"first"),
        },
        SkillFileExport {
            relative_path: "same.txt".to_string(),
            content_base64: BASE64_STANDARD.encode(b"second"),
        },
    ];
    let invalid_cases = vec![
        ("duplicate", duplicate),
        (
            "traversal",
            vec![SkillFileExport {
                relative_path: "../escape.txt".to_string(),
                content_base64: BASE64_STANDARD.encode(b"escape"),
            }],
        ),
        (
            "absolute",
            vec![SkillFileExport {
                relative_path: format!("{}absolute.txt", std::path::MAIN_SEPARATOR),
                content_base64: BASE64_STANDARD.encode(b"absolute"),
            }],
        ),
        (
            "long",
            vec![SkillFileExport {
                relative_path: "x".repeat(CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS + 1),
                content_base64: BASE64_STANDARD.encode(b"long"),
            }],
        ),
    ];

    for (case, files) in invalid_cases {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join(case);
        write_skill_files_to_dir(&target, &files, None)
            .expect_err("invalid path should fail before writing");
        assert!(!target.exists(), "target created for {case}");
    }
}

#[test]
fn write_skill_files_rejects_file_directory_conflicts_in_both_orders() {
    for (case, paths) in [
        ("parent-first", ["assets", "assets/image.png"]),
        ("child-first", ["assets/image.png", "assets"]),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join(case);
        let files = paths
            .into_iter()
            .map(|relative_path| SkillFileExport {
                relative_path: relative_path.to_string(),
                content_base64: BASE64_STANDARD.encode(b"x"),
            })
            .collect::<Vec<_>>();
        let error = write_skill_files_to_dir(&target, &files, None)
            .expect_err("file/directory conflict must fail");
        assert!(error.to_string().contains("conflicting skill file paths"));
        assert!(!target.exists());
    }
}

#[test]
fn write_skill_files_rejects_generated_marker_conflicts_before_creating_dir() {
    for (case, relative_path) in [
        ("managed", SKILL_MANAGED_MARKER_FILE),
        ("source", SKILL_SOURCE_MARKER_FILE),
        ("managed-child", ".aio-coding-hub.managed/payload"),
        ("source-child", ".aio-coding-hub.source.json/payload"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join(case);
        let files = vec![SkillFileExport {
            relative_path: relative_path.to_string(),
            content_base64: BASE64_STANDARD.encode(b"payload"),
        }];
        let error = write_skill_files_to_dir(&target, &files, None)
            .expect_err("generated marker conflict must fail");
        assert!(error.to_string().contains("reserved skill marker path"));
        assert!(!target.exists());
    }
}

#[cfg(windows)]
#[test]
fn write_skill_files_rejects_windows_case_aliases_in_both_orders() {
    for (case, paths) in [
        ("canonical-first", ["SKILL.md", "skill.MD"]),
        ("alias-first", ["skill.MD", "SKILL.md"]),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join(case);
        let files = paths
            .into_iter()
            .map(|relative_path| SkillFileExport {
                relative_path: relative_path.to_string(),
                content_base64: BASE64_STANDARD.encode(b"x"),
            })
            .collect::<Vec<_>>();
        let error = write_skill_files_to_dir(&target, &files, None)
            .expect_err("Windows case aliases must conflict");
        assert!(error.to_string().contains("duplicate skill file path"));
        assert!(!target.exists());
    }
}

#[test]
fn skill_md_case_alias_uses_platform_specific_budget() {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("skill-target");
    let files = vec![SkillFileExport {
        relative_path: "skill.MD".to_string(),
        content_base64: BASE64_STANDARD.encode(vec![b'x'; CONFIG_SKILL_MD_MAX_BYTES + 1]),
    }];

    #[cfg(windows)]
    {
        let error = write_skill_files_to_dir(&target, &files, None)
            .expect_err("Windows SKILL.md alias must use dedicated budget");
        assert!(error.to_string().contains("SKILL.md too large"));
        assert!(!target.exists());
    }
    #[cfg(not(windows))]
    {
        write_skill_files_to_dir(&target, &files, None)
            .expect("case-distinct non-Windows filename uses ordinary file budget");
        assert!(target.join("skill.MD").is_file());
    }
}

#[test]
fn skill_md_and_source_metadata_use_dedicated_prewrite_budgets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).expect("create source");
    std::fs::write(
        source.join("SKILL.md"),
        vec![b'x'; CONFIG_SKILL_MD_MAX_BYTES + 1],
    )
    .expect("write oversized SKILL.md");
    assert!(export_skill_dir_files(&source, true).is_err());

    let files = vec![SkillFileExport {
        relative_path: "SKILL.md".to_string(),
        content_base64: BASE64_STANDARD.encode(vec![b'x'; CONFIG_SKILL_MD_MAX_BYTES + 1]),
    }];
    let skill_target = temp.path().join("skill-target");
    let error = write_skill_files_to_dir(&skill_target, &files, None)
        .expect_err("oversized SKILL.md must fail");
    assert!(error.to_string().contains("SKILL.md too large"));
    assert!(!skill_target.exists());

    let metadata = SkillSourceMetadataFile {
        source_git_url: "x".repeat(CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES),
        source_branch: "main".to_string(),
        source_subdir: "skill".to_string(),
    };
    let metadata_target = temp.path().join("metadata-target");
    let error = write_skill_files_to_dir(
        &metadata_target,
        &[SkillFileExport {
            relative_path: "ordinary.txt".to_string(),
            content_base64: BASE64_STANDARD.encode(b"ordinary"),
        }],
        Some(&metadata),
    )
    .expect_err("oversized metadata must fail");
    assert!(error.to_string().contains("source metadata too large"));
    assert!(!metadata_target.exists());
}

#[cfg(unix)]
#[test]
fn export_skill_dir_files_rejects_non_utf8_path() {
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("synthetic-skill");
    write_skill_md(&skill_dir, "Synthetic Skill", "Synthetic fixture");
    std::fs::write(skill_dir.join(OsString::from_vec(vec![0xff])), b"invalid")
        .expect("write non-utf8 file");

    let err = export_skill_dir_files(&skill_dir, true).expect_err("non-utf8 path should fail");
    assert!(err.to_string().contains("invalid utf-8 skill path"));
}

#[cfg(unix)]
#[test]
fn export_skill_dir_files_handles_symlink_directory_cycle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("synthetic-skill");
    let nested = skill_dir.join("nested");
    write_skill_md(&skill_dir, "Synthetic Skill", "Synthetic fixture");
    std::fs::create_dir_all(&nested).expect("create nested dir");
    std::os::unix::fs::symlink(&skill_dir, nested.join("cycle")).expect("create cycle symlink");

    let files = export_skill_dir_files(&skill_dir, true).expect("cycle should terminate");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, "SKILL.md");
}

#[cfg(unix)]
#[test]
fn export_skill_dir_files_rejects_special_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let skill_dir = temp.path().join("synthetic-skill");
    write_skill_md(&skill_dir, "Synthetic Skill", "Synthetic fixture");
    let _listener = std::os::unix::net::UnixListener::bind(skill_dir.join("special.sock"))
        .expect("create unix socket");

    let err = export_skill_dir_files(&skill_dir, true).expect_err("special file should fail");
    assert!(err
        .to_string()
        .contains("SKILL_EXPORT_BLOCKED_SPECIAL_FILE"));
}
