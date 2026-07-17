use super::skill_fs::{
    cli_skills_root, export_skill_dir_files, ssot_skills_root, validate_installed_skill_key,
    write_skill_files_to_dir, SkillSourceMetadataFile,
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

#[cfg(unix)]
fn create_file_symlink(src: &Path, dst: &Path) {
    std::os::unix::fs::symlink(src, dst).expect("create symlink");
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
