use super::*;
use crate::infra::settings::{self, AppSettings, CodexHomeMode};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

static TEST_ENV_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct EnvRestore {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn save_once(&mut self, key: &'static str) {
        if self.saved.iter().any(|(k, _)| *k == key) {
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
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct CliProxyTestApp {
    _env: EnvRestore,
    _lock: MutexGuard<'static, ()>,
    #[allow(dead_code)]
    home: tempfile::TempDir,
    app: tauri::App<tauri::test::MockRuntime>,
}

impl CliProxyTestApp {
    fn new() -> Self {
        let lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let seq = TEST_ENV_SEQ.fetch_add(1, Ordering::Relaxed);

        let mut env = EnvRestore::default();
        let home_os = home.path().as_os_str().to_os_string();
        env.set_var("AIO_CODING_HUB_HOME_DIR", home_os.clone());
        // app data 目录也使用每测例唯一 dotdir，避免共享真实 HOME 时读到旧 manifest。
        env.set_var(
            "AIO_CODING_HUB_DOTDIR_NAME",
            format!(".aio-coding-hub-cli-proxy-test-{seq}"),
        );
        crate::test_support::clear_settings_cache();

        Self {
            _lock: lock,
            _env: env,
            home,
            app: tauri::test::mock_app(),
        }
    }

    fn handle(&self) -> tauri::AppHandle<tauri::test::MockRuntime> {
        self.app.handle().clone()
    }
}

#[derive(Clone)]
enum ManagedProviderProbeMode {
    FollowRuntimeState,
    Fixed(String),
}

#[derive(Clone)]
struct ManagedProviderProbeState {
    listen: String,
    listener: String,
    upstream_base_url: String,
    config_path: String,
    state_path: String,
    state_root: String,
    log_path: String,
    instance_nonce: String,
    process_id: u32,
    provider_mode: ManagedProviderProbeMode,
}

impl ManagedProviderProbeState {
    fn provider_name(&self) -> String {
        match &self.provider_mode {
            ManagedProviderProbeMode::FollowRuntimeState => std::fs::read(&self.state_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|state| {
                    state
                        .get("provider_name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "missing".to_string()),
            ManagedProviderProbeMode::Fixed(provider_name) => provider_name.clone(),
        }
    }
}

async fn managed_provider_probe_health(
    State(state): State<ManagedProviderProbeState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "process_id": state.process_id,
        "listen": state.listen,
        "upstream_base_url": state.upstream_base_url,
        "ui_path": "/__codex_retry_gateway/ui"
    }))
}

async fn managed_provider_probe_status(
    State(state): State<ManagedProviderProbeState>,
) -> Json<serde_json::Value> {
    let provider_name = state.provider_name();
    Json(serde_json::json!({
        "ok": true,
        "process_id": state.process_id,
        "listen": state.listen,
        "state": {
            "process_id": state.process_id,
            "original_base_url": state.upstream_base_url,
            "gateway_base_url": state.listener,
            "aio_instance_nonce": state.instance_nonce,
            "provider_name": provider_name
        },
        "paths": {
            "config_path": state.config_path,
            "state_path": state.state_path,
            "state_root": state.state_root,
            "log_path": state.log_path
        }
    }))
}

struct ManagedProviderProbe {
    listener: String,
    _server: tokio::task::JoinHandle<()>,
}

impl Drop for ManagedProviderProbe {
    fn drop(&mut self) {
        self._server.abort();
    }
}

fn managed_relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .expect("managed path under root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn canonical_path_text(path: &Path) -> String {
    std::fs::canonicalize(path)
        .expect("canonical managed path")
        .display()
        .to_string()
}

async fn install_managed_provider_probe(
    handle: &tauri::AppHandle<tauri::test::MockRuntime>,
    aio_origin: &str,
    provider_mode: ManagedProviderProbeMode,
) -> ManagedProviderProbe {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind managed provider probe");
    let port = listener.local_addr().expect("probe address").port();
    let listener_url = format!("http://127.0.0.1:{port}");
    let listen = format!("127.0.0.1:{port}");
    let upstream_base_url = format!("{aio_origin}/v1");
    let paths = crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(handle)
        .expect("gateway paths");
    paths.ensure_dirs().expect("gateway dirs");

    let source_commit = "0123456789abcdef0123456789abcdef01234567";
    let source_dir = paths.source_dir(source_commit).expect("source dir");
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    std::fs::write(&paths.runtime_config_path, b"{}").expect("write runtime config");
    std::fs::write(&paths.runtime_log_path, b"").expect("write runtime log");

    let process_id = std::process::id();
    let start_identity =
        crate::infra::codex_retry_gateway::process_start_identity_for_tests(process_id)
            .expect("current process start identity");
    let instance_nonce = "provider-probe-nonce";
    let runtime_state = crate::infra::codex_retry_gateway::managed_gateway_state(
        crate::infra::codex_retry_gateway::ManagedGatewayStateInput {
            gateway_base_url: &listener_url,
            state_root: &paths.runtime_dir.display().to_string(),
            config_path: &paths.runtime_config_path.display().to_string(),
            log_path: &paths.runtime_log_path.display().to_string(),
            pid_path: &paths.runtime_pid_path.display().to_string(),
            upstream_base_url: &upstream_base_url,
            provider_name: crate::infra::codex_retry_gateway::MANAGED_PROVIDER_AIO,
            instance_nonce,
            process_id: Some(process_id),
            process_start_identity: Some(start_identity),
        },
    );
    std::fs::write(
        &paths.runtime_state_path,
        serde_json::to_vec_pretty(&runtime_state).expect("serialize runtime state"),
    )
    .expect("write runtime state");

    let record = crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord {
        pid: process_id,
        start_identity: Some(start_identity),
        started_at_ms: 1,
        node_executable: canonical_path_text(
            &std::env::current_exe().expect("current test executable"),
        ),
        source_commit: source_commit.to_string(),
        source_dir_rel: managed_relative_path(&source_dir, &paths.root),
        config_path_rel: managed_relative_path(&paths.runtime_config_path, &paths.root),
        state_path_rel: managed_relative_path(&paths.runtime_state_path, &paths.root),
        log_path_rel: managed_relative_path(&paths.runtime_log_path, &paths.root),
        listener: listener_url.clone(),
        upstream_base_url: upstream_base_url.clone(),
        instance_nonce: instance_nonce.to_string(),
        provider_name: crate::infra::codex_retry_gateway::MANAGED_PROVIDER_AIO.to_string(),
    };
    let manager = crate::infra::codex_retry_gateway::CodexRetryGatewayManagerState {
        generation: 10,
        active_commit: Some(source_commit.to_string()),
        effective_port: Some(port),
        process_record: Some(record),
        ..Default::default()
    };
    crate::infra::codex_retry_gateway::write_manager_state(&paths, &manager)
        .expect("write manager state");

    let probe_state = ManagedProviderProbeState {
        listen,
        listener: listener_url.clone(),
        upstream_base_url,
        config_path: canonical_path_text(&paths.runtime_config_path),
        state_path: canonical_path_text(&paths.runtime_state_path),
        state_root: canonical_path_text(&paths.runtime_dir),
        log_path: canonical_path_text(&paths.runtime_log_path),
        instance_nonce: instance_nonce.to_string(),
        process_id,
        provider_mode,
    };
    let router = Router::new()
        .route(
            "/__codex_retry_gateway/health",
            get(managed_provider_probe_health),
        )
        .route(
            "/__codex_retry_gateway/api/status",
            get(managed_provider_probe_status),
        )
        .with_state(probe_state);
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    tokio::task::yield_now().await;
    ManagedProviderProbe {
        listener: listener_url,
        _server: server,
    }
}

fn write_cli_proxy_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    enabled: bool,
    base_origin: Option<&str>,
) {
    write_manifest(
        app,
        cli_key,
        &CliProxyManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            managed_by: MANAGED_BY.to_string(),
            cli_key: cli_key.to_string(),
            enabled,
            base_origin: base_origin.map(str::to_string),
            created_at: 1,
            updated_at: 1,
            files: Vec::new(),
            codex: None,
        },
    )
    .expect("write manifest");
}

fn codex_platform_for_tests() -> CodexConfigPlatform {
    CodexConfigPlatform::current()
}

fn write_codex_proxy_files<R: tauri::Runtime>(app: &tauri::AppHandle<R>, base_origin: &str) {
    let config_path = codex_config_path(app).expect("codex config path");
    let auth_path = codex_auth_path(app).expect("codex auth path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config dir");

    let config = build_codex_config_toml(
        None,
        &format!("{base_origin}/v1"),
        codex_platform_for_tests(),
    )
    .expect("build codex config");
    std::fs::write(&config_path, config).expect("write config");

    let auth = build_codex_auth_json(None).expect("build codex auth");
    std::fs::write(&auth_path, auth).expect("write auth");
}

fn write_codex_oauth_proxy_config<R: tauri::Runtime>(app: &tauri::AppHandle<R>, base_origin: &str) {
    let config_path = codex_config_path(app).expect("codex config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config dir");

    let config = build_codex_config_toml_oauth_compatible(
        None,
        &format!("{base_origin}/v1"),
        codex_platform_for_tests(),
    )
    .expect("build codex oauth config");
    std::fs::write(&config_path, config).expect("write config");
}

fn set_custom_codex_home<R: tauri::Runtime>(app: &tauri::AppHandle<R>, codex_home: &Path) {
    let settings = AppSettings {
        codex_home_mode: CodexHomeMode::Custom,
        codex_home_override: codex_home.display().to_string(),
        ..AppSettings::default()
    };
    settings::write(app, &settings).expect("write settings");
}

fn set_codex_oauth_compatible_proxy_mode<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    enabled: bool,
) {
    let mut settings = settings::read(app).unwrap_or_default();
    settings.codex_oauth_compatible_proxy_mode = enabled;
    settings::write(app, &settings).expect("write settings");
}

fn write_codex_direct_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    config: &str,
    auth: &str,
) {
    let config_path = codex_config_path(app).expect("codex config path");
    let auth_path = codex_auth_path(app).expect("codex auth path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config dir");
    std::fs::write(&config_path, config).expect("write direct config");
    std::fs::write(&auth_path, auth).expect("write direct auth");
}

fn manifest_entry<'a>(manifest: &'a CliProxyManifest, kind: &str) -> &'a BackupFileEntry {
    manifest
        .files
        .iter()
        .find(|entry| entry.kind == kind)
        .unwrap_or_else(|| panic!("missing manifest entry for kind={kind}"))
}

#[derive(Default, Clone)]
struct FakeTransitionStore {
    pending:
        Arc<Mutex<Option<crate::infra::codex_retry_gateway::CodexRetryGatewayRouteTransition>>>,
    committed: Arc<Mutex<Vec<(String, u64)>>>,
    cleared: Arc<Mutex<Vec<String>>>,
    snapshots: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl crate::infra::codex_retry_gateway::CodexRetryGatewayTransitionStore for FakeTransitionStore {
    fn load_pending(
        &self,
    ) -> crate::shared::error::AppResult<
        Option<crate::infra::codex_retry_gateway::CodexRetryGatewayRouteTransition>,
    > {
        Ok(self.pending.lock().expect("pending lock").clone())
    }

    fn prepare(
        &self,
        transition: &crate::infra::codex_retry_gateway::CodexRetryGatewayRouteTransition,
        snapshot_bytes: &[Option<Vec<u8>>],
    ) -> crate::shared::error::AppResult<()> {
        if transition.snapshots.len() != snapshot_bytes.len() {
            return Err("snapshot metadata and bytes count differ".into());
        }
        let mut stored = self.snapshots.lock().expect("snapshot lock");
        stored.clear();
        for (snapshot, bytes) in transition.snapshots.iter().zip(snapshot_bytes) {
            if let (Some(backup_rel), Some(bytes)) = (snapshot.backup_rel.as_ref(), bytes.as_ref())
            {
                stored.insert(backup_rel.clone(), bytes.clone());
            }
        }
        *self.pending.lock().expect("pending lock") = Some(transition.clone());
        Ok(())
    }

    fn read_snapshot(
        &self,
        transition: &crate::infra::codex_retry_gateway::CodexRetryGatewayRouteTransition,
        snapshot: &crate::infra::codex_retry_gateway::CodexRetryGatewayRouteSnapshot,
    ) -> crate::shared::error::AppResult<Vec<u8>> {
        let pending = self.pending.lock().expect("pending lock");
        if pending
            .as_ref()
            .map(|pending| pending.operation_id.as_str())
            != Some(transition.operation_id.as_str())
        {
            return Err("pending transition mismatch".into());
        }
        let backup_rel = snapshot
            .backup_rel
            .as_ref()
            .ok_or_else(|| AppError::from("snapshot backup path missing"))?;
        self.snapshots
            .lock()
            .expect("snapshot lock")
            .get(backup_rel)
            .cloned()
            .ok_or_else(|| AppError::from("snapshot bytes missing"))
    }

    fn commit(&self, operation_id: &str, generation: u64) -> crate::shared::error::AppResult<()> {
        self.committed
            .lock()
            .expect("committed lock")
            .push((operation_id.to_string(), generation));
        *self.pending.lock().expect("pending lock") = None;
        self.snapshots.lock().expect("snapshot lock").clear();
        Ok(())
    }

    fn clear(&self, operation_id: &str) -> crate::shared::error::AppResult<()> {
        self.cleared
            .lock()
            .expect("cleared lock")
            .push(operation_id.to_string());
        *self.pending.lock().expect("pending lock") = None;
        self.snapshots.lock().expect("snapshot lock").clear();
        Ok(())
    }
}

#[test]
fn read_manifest_rejects_oversized_file() {
    let test_app = CliProxyTestApp::new();
    let handle = test_app.handle();
    let root = cli_proxy_root_dir(&handle, "codex").expect("cli proxy root");
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(
        cli_proxy_manifest_path(&root),
        vec![b'x'; CLI_PROXY_MANIFEST_MAX_BYTES + 1],
    )
    .expect("write oversized manifest");

    let err = read_manifest(&handle, "codex").expect_err("oversized manifest should fail");

    assert!(err.to_string().contains("too large"));
}

#[test]
fn backup_for_enable_rejects_oversized_target_file() {
    let test_app = CliProxyTestApp::new();
    let handle = test_app.handle();
    let config_path = codex_config_path(&handle).expect("codex config path");
    std::fs::create_dir_all(config_path.parent().expect("config parent"))
        .expect("create config parent");
    std::fs::write(&config_path, vec![b'x'; CLI_PROXY_FILE_MAX_BYTES + 1])
        .expect("write oversized config");

    let err = backup_for_enable(&handle, "codex", "http://127.0.0.1:37123", None)
        .expect_err("oversized target file should fail");

    assert!(err.to_string().contains("too large"));
    assert!(read_manifest(&handle, "codex")
        .expect("read manifest")
        .is_none());
}

#[test]
fn restore_backups_exactly_rejects_oversized_backup_file() {
    let test_app = CliProxyTestApp::new();
    let handle = test_app.handle();
    let config_path = codex_config_path(&handle).expect("codex config path");
    let root = cli_proxy_root_dir(&handle, "codex").expect("cli proxy root");
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir).expect("create files dir");
    std::fs::write(
        files_dir.join("config.toml"),
        vec![b'x'; CLI_PROXY_FILE_MAX_BYTES + 1],
    )
    .expect("write oversized backup");
    let manifest = CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: "codex".to_string(),
        enabled: true,
        base_origin: Some("http://127.0.0.1:37123".to_string()),
        created_at: 1,
        updated_at: 1,
        files: vec![BackupFileEntry {
            kind: "unknown_kind".to_string(),
            path: config_path.to_string_lossy().to_string(),
            existed: true,
            backup_rel: Some("config.toml".to_string()),
        }],
        codex: None,
    };

    let err = restore_backups_exactly_from_manifest(&handle, &manifest)
        .expect_err("oversized backup should fail");

    assert!(err.to_string().contains("too large"));
}

#[test]
fn codex_proxy_preserves_nested_model_provider_tables_and_order() {
    let input = r#"
model_provider = "aio"
preferred_auth_method = "apikey"

[model_providers.aio]
name = "aio"
base_url = "http://old/v1"
wire_api = "responses"
requires_openai_auth = true

[model_providers.aio.projects."C:\\work"]
trust_level = "trusted"

[other]
foo = "bar"
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("base_url = \"http://new/v1\""), "{s}");
    assert!(
        s.contains("[model_providers.aio.projects.\"C:\\\\work\"]"),
        "{s}"
    );
    assert!(s.contains("trust_level = \"trusted\""), "{s}");

    let base_idx = s.find("[model_providers.aio]").expect("base table exists");
    let nested_idx = s
        .find("[model_providers.aio.projects.\"C:\\\\work\"]")
        .expect("nested table exists");
    assert!(base_idx < nested_idx, "base must appear before nested: {s}");
}

#[test]
fn codex_proxy_preserves_extra_keys_in_base_table() {
    let input = r#"
[model_providers.aio]
name = "aio"
base_url = "http://old/v1"
wire_api = "responses"
requires_openai_auth = true
trusted_roots = ["C:\\work"]
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("base_url = \"http://new/v1\""), "{s}");
    assert!(s.contains("trusted_roots = [\"C:\\\\work\"]"), "{s}");
}

#[test]
fn codex_proxy_dedupes_multiple_base_tables() {
    let input = r#"
[model_providers."aio"]
base_url = "http://old-1/v1"

[model_providers.aio]
base_url = "http://old-2/v1"

[model_providers.aio.projects."C:\\work"]
trust_level = "trusted"
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    let count = s.matches("[model_providers.aio]").count()
        + s.matches("[model_providers.\"aio\"]").count()
        + s.matches("[model_providers.'aio']").count();
    assert_eq!(count, 1, "{s}");
    assert!(s.contains("base_url = \"http://new/v1\""), "{s}");
    assert!(
        s.contains("[model_providers.aio.projects.\"C:\\\\work\"]"),
        "{s}"
    );
}

#[test]
fn codex_oauth_compatible_config_removes_aio_owned_preferred_auth_method() {
    let input = r#"
preferred_auth_method = "apikey"
model = "gpt-5-codex"

[model_providers.aio]
base_url = "http://old/v1"
"#;

    let out = build_codex_config_toml_oauth_compatible(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("model_provider = \"aio\""), "{s}");
    assert!(!s.contains("preferred_auth_method"), "{s}");
    assert!(s.contains("model = \"gpt-5-codex\""), "{s}");
    assert!(s.contains("base_url = \"http://new/v1\""), "{s}");
    assert!(s.contains("requires_openai_auth = true"), "{s}");
}

#[test]
fn codex_oauth_compatible_config_preserves_non_aio_preferred_auth_method() {
    let input = r#"
preferred_auth_method = "chatgpt"

[existing]
foo = "bar"
"#;

    let out = build_codex_config_toml_oauth_compatible(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("preferred_auth_method = \"chatgpt\""), "{s}");
    assert!(s.contains("base_url = \"http://new/v1\""), "{s}");
}

#[test]
fn codex_proxy_inserts_base_table_before_nested_when_missing() {
    let input = r#"
[model_providers.aio.projects."C:\\work"]
trust_level = "trusted"
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    let base_idx = s
        .find("[model_providers.aio]")
        .expect("base table inserted");
    let nested_idx = s
        .find("[model_providers.aio.projects.\"C:\\\\work\"]")
        .expect("nested table exists");
    assert!(base_idx < nested_idx, "base must appear before nested: {s}");
}

#[test]
fn codex_proxy_moves_base_table_before_nested_when_out_of_order() {
    let input = r#"
[model_providers.aio.projects."C:\\work"]
trust_level = "trusted"

[model_providers.aio]
name = "aio"
base_url = "http://old/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    let base_idx = s.find("[model_providers.aio]").expect("base table exists");
    let nested_idx = s
        .find("[model_providers.aio.projects.\"C:\\\\work\"]")
        .expect("nested table exists");
    assert!(base_idx < nested_idx, "base must appear before nested: {s}");
}

#[test]
fn codex_proxy_adds_windows_sandbox_only_on_windows() {
    let out = build_codex_config_toml(None, "http://new/v1", CodexConfigPlatform::Windows)
        .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("[windows]"), "{s}");
    assert!(s.contains("sandbox = \"elevated\""), "{s}");
}

#[test]
fn codex_proxy_does_not_add_windows_sandbox_on_non_windows() {
    let out =
        build_codex_config_toml(None, "http://new/v1", CodexConfigPlatform::Other).expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(!s.contains("[windows]"), "{s}");
    assert!(!s.contains("sandbox = \"elevated\""), "{s}");
}

#[test]
fn codex_proxy_preserves_existing_windows_block_on_non_windows() {
    let input = r#"
[windows]
sandbox = "elevated"

[existing]
foo = "bar"
"#;

    let out = build_codex_config_toml(
        Some(input.as_bytes().to_vec()),
        "http://new/v1",
        CodexConfigPlatform::Other,
    )
    .expect("build");
    let s = String::from_utf8(out).expect("utf8");

    assert!(s.contains("[windows]"), "{s}");
    assert!(s.contains("sandbox = \"elevated\""), "{s}");
    assert!(s.contains("[existing]"), "{s}");
    assert!(s.contains("foo = \"bar\""), "{s}");
}

#[test]
fn codex_proxy_auth_json_preserves_existing_oauth_fields() {
    let input = r#"{
  "oauth_access_token": "tok-123",
  "oauth_refresh_token": "ref-456",
  "OPENAI_API_KEY": "old-key"
}"#;

    let out = build_codex_auth_json(Some(input.as_bytes().to_vec())).expect("build auth");
    let value: serde_json::Value = serde_json::from_slice(&out).expect("parse output");

    assert_eq!(
        value.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
        Some("aio-coding-hub")
    );
    assert_eq!(
        value.get("oauth_access_token").and_then(|v| v.as_str()),
        Some("tok-123")
    );
    assert_eq!(
        value.get("oauth_refresh_token").and_then(|v| v.as_str()),
        Some("ref-456")
    );
}

#[test]
fn codex_proxy_auth_json_rejects_non_object_root() {
    let input = r#"["not", "an", "object"]"#;
    let err = build_codex_auth_json(Some(input.as_bytes().to_vec())).expect_err("must fail");
    assert!(err
        .to_string()
        .contains("auth.json root must be a JSON object"));
}

#[test]
fn codex_proxy_enable_does_not_partially_write_config_when_auth_json_is_invalid() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    let original_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://api.openai.com/v1"
"#;
    let invalid_auth = r#"{ "tokens": "#;

    write_codex_direct_files(&handle, original_config, invalid_auth);

    let result = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(!result.ok, "enable must fail: {result:?}");
    assert_eq!(
        result.error_code.as_deref(),
        Some("CLI_PROXY_ENABLE_FAILED")
    );
    assert!(
        result.message.contains("CLI_PROXY_INVALID_AUTH_JSON"),
        "unexpected message: {}",
        result.message
    );

    let config_path = codex_config_path(&handle).expect("codex config path");
    let auth_path = codex_auth_path(&handle).expect("codex auth path");
    assert_eq!(
        std::fs::read_to_string(&config_path).expect("read config"),
        original_config,
        "config.toml must stay untouched when a later target fails to parse"
    );
    assert_eq!(
        std::fs::read_to_string(&auth_path).expect("read auth"),
        invalid_auth,
        "auth.json must stay untouched on parse failure"
    );
    assert_eq!(
        std::fs::read_to_string(auth_path.with_extension("json.invalid-backup"))
            .expect("read invalid backup"),
        invalid_auth
    );

    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest should preserve backup snapshot");
    assert!(
        !manifest.enabled,
        "failed enable should not mark the proxy as enabled"
    );
}

#[test]
fn status_all_skips_gateway_check_when_gateway_not_running_even_if_codex_is_applied() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    write_cli_proxy_manifest(&handle, "codex", true, Some(base_origin));
    write_codex_proxy_files(&handle, base_origin);

    let rows = status_all(&handle, None).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(codex.enabled);
    assert_eq!(codex.base_origin.as_deref(), Some(base_origin));
    assert_eq!(codex.applied_to_current_gateway, None);
}

#[test]
fn status_all_skips_gateway_check_when_gateway_not_running_even_if_codex_has_drifted() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    write_cli_proxy_manifest(&handle, "codex", true, Some(base_origin));
    write_codex_proxy_files(&handle, "http://127.0.0.1:9999");

    let rows = status_all(&handle, None).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(codex.enabled);
    assert_eq!(codex.base_origin.as_deref(), Some(base_origin));
    assert_eq!(codex.applied_to_current_gateway, None);
}

#[test]
fn status_all_skips_gateway_application_check_for_disabled_codex() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    write_cli_proxy_manifest(&handle, "codex", false, Some(base_origin));
    write_codex_proxy_files(&handle, base_origin);

    let rows = status_all(&handle, None).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(!codex.enabled);
    assert_eq!(codex.base_origin.as_deref(), Some(base_origin));
    assert_eq!(codex.applied_to_current_gateway, None);
}

#[test]
fn status_all_reports_codex_route_metadata_when_proxy_is_enabled() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    let rows = status_all(&handle, Some(base_origin)).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert_eq!(codex.route_mode, Some(CodexRouteMode::DirectAio));
    assert_eq!(codex.desired_enabled, Some(false));
    assert_eq!(codex.aio_origin.as_deref(), Some(base_origin));
    assert_eq!(codex.effective_origin.as_deref(), Some(base_origin));
    assert!(codex.generation.is_some_and(|generation| generation > 0));
}

#[test]
fn current_canonical_codex_auth_bytes_strip_proxy_overlay_and_keep_user_fields() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    write_codex_direct_files(
        &handle,
        "[model_providers.openai]\nname = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\n",
        r#"{
  "tokens": { "access": "tok-123" },
  "profile": "direct"
}"#,
    );

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    let auth_path = codex_auth_path(&handle).expect("codex auth path");
    std::fs::write(
        &auth_path,
        br#"{
  "OPENAI_API_KEY": "aio-coding-hub",
  "auth_mode": "apikey",
  "user_added": "keep-me"
}"#,
    )
    .expect("write live auth");

    let state = codex_cli_proxy_state(&handle)
        .expect("codex state")
        .expect("managed codex state");
    let canonical = current_canonical_codex_auth_bytes(&handle, &state)
        .expect("canonical auth bytes")
        .expect("canonical auth exists");
    let value: serde_json::Value =
        serde_json::from_slice(&canonical).expect("parse canonical auth");

    assert!(value.get("OPENAI_API_KEY").is_none(), "{value}");
    assert!(value.get("auth_mode").is_none(), "{value}");
    assert_eq!(
        value.get("user_added").and_then(|entry| entry.as_str()),
        Some("keep-me")
    );
    assert!(value.get("tokens").is_some(), "{value}");
}

#[test]
fn codex_route_transition_helpers_reject_stale_generation_and_hash() {
    let err = reject_stale_codex_route_state(7, "sha256:abc", 8, "sha256:abc")
        .expect_err("generation mismatch should fail");
    assert!(err.to_string().contains("CLI_PROXY_STALE_ROUTE_GENERATION"));

    let err = reject_stale_codex_route_state(7, "sha256:abc", 7, "sha256:def")
        .expect_err("hash mismatch should fail");
    assert!(err.to_string().contains("CLI_PROXY_STALE_ROUTE_HASH"));
}

#[test]
fn codex_route_transition_helpers_prepare_commit_and_reconcile_pending_state() {
    let store = FakeTransitionStore::default();
    let prior = CodexCliProxyManifestState {
        generation: 4,
        route_mode: CodexRouteMode::DirectAio,
        desired_enabled: false,
        aio_origin: Some("http://127.0.0.1:37123".to_string()),
        guarded_origin: Some("http://127.0.0.1:4610".to_string()),
        canonical_config_sha256: Some("sha256:old-canonical".to_string()),
        live_config_sha256: Some("sha256:old-live".to_string()),
    };

    let transition = prepare_codex_route_transition(
        &store,
        CodexRouteTransitionPreparation {
            operation_kind:
                crate::infra::codex_retry_gateway::CodexRetryGatewayOperationKind::Recover,
            prior_state: &prior,
            target_mode: CodexRouteMode::Guarded,
            prior_canonical_config_sha256: "sha256:old-canonical".to_string(),
            prior_live_config_sha256: "sha256:old-live".to_string(),
            canonical_config_sha256: "sha256:new-canonical".to_string(),
            live_config_sha256: "sha256:new-live".to_string(),
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
            snapshots: Vec::new(),
            snapshot_bytes: Vec::new(),
        },
    )
    .expect("prepare transition");

    assert_eq!(transition.prior_generation, 4);
    assert_eq!(transition.target_generation, 5);
    assert_eq!(transition.prior_mode, CodexRouteMode::DirectAio);
    assert_eq!(transition.target_mode, CodexRouteMode::Guarded);
    assert!(store.load_pending().expect("load pending").is_some());

    let reconciled = reconcile_codex_route_transition(
        &store,
        5,
        CodexRouteMode::Guarded,
        "sha256:new-canonical",
        "sha256:new-live",
    )
    .expect("reconcile pending");
    assert_eq!(reconciled, Some(true));
    assert_eq!(store.pending.lock().expect("pending lock").clone(), None);
    assert_eq!(store.committed.lock().expect("committed lock").len(), 1);

    let second = prepare_codex_route_transition(
        &store,
        CodexRouteTransitionPreparation {
            operation_kind:
                crate::infra::codex_retry_gateway::CodexRetryGatewayOperationKind::Update,
            prior_state: &prior,
            target_mode: CodexRouteMode::Guarded,
            prior_canonical_config_sha256: "sha256:old-canonical".to_string(),
            prior_live_config_sha256: "sha256:old-live".to_string(),
            canonical_config_sha256: "sha256:new-canonical".to_string(),
            live_config_sha256: "sha256:new-live".to_string(),
            source_commit: None,
            process_should_run: true,
            snapshots: Vec::new(),
            snapshot_bytes: Vec::new(),
        },
    )
    .expect("prepare second transition");
    let reconciled = reconcile_codex_route_transition(
        &store,
        4,
        CodexRouteMode::DirectAio,
        "sha256:old-canonical",
        "sha256:old-live",
    )
    .expect("reconcile mismatch");
    assert_eq!(reconciled, Some(false));
    assert!(store.pending.lock().expect("pending lock").is_some());
    clear_codex_route_transition(&store, &second).expect("clear mismatched transition");
    assert_eq!(
        store.cleared.lock().expect("cleared lock").last().cloned(),
        Some(second.operation_id)
    );

    let third = prepare_codex_route_transition(
        &store,
        CodexRouteTransitionPreparation {
            operation_kind:
                crate::infra::codex_retry_gateway::CodexRetryGatewayOperationKind::DisableGateway,
            prior_state: &prior,
            target_mode: CodexRouteMode::DirectAio,
            prior_canonical_config_sha256: "sha256:old-canonical".to_string(),
            prior_live_config_sha256: "sha256:old-live".to_string(),
            canonical_config_sha256: "sha256:old-canonical".to_string(),
            live_config_sha256: "sha256:old-live".to_string(),
            source_commit: None,
            process_should_run: false,
            snapshots: Vec::new(),
            snapshot_bytes: Vec::new(),
        },
    )
    .expect("prepare third transition");
    clear_codex_route_transition(&store, &third).expect("clear third transition");
    assert!(store.pending.lock().expect("pending lock").is_none());
}

#[test]
fn interrupted_route_transaction_restores_persistent_snapshots_before_clearing_journal() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let original_config = r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.example/v1"
"#;
    let original_auth = r#"{"OPENAI_API_KEY":"user-key","auth_mode":"apikey"}"#;
    write_codex_direct_files(&handle, original_config, original_auth);

    let CodexRouteContext {
        state: _,
        route_state: prior,
        canonical_config: canonical_bytes,
        canonical_auth: _,
        canonical_sha256: prior_canonical_sha256,
    } = codex_route_context(&handle, Some("http://127.0.0.1:37123")).expect("route context");
    let prior_live_bytes = read_cli_proxy_file(&codex_config_path(&handle).expect("config path"))
        .expect("read prior live config");
    let captured = capture_current_target_state(&handle, "codex").expect("capture targets");
    let target_snapshots = snapshot_target_files(&captured).expect("target snapshots");
    let backup_snapshots =
        snapshot_backup_files(&handle, "codex", &captured).expect("backup snapshots");
    let manifest_path = codex_route_manifest_path(&handle).expect("manifest path");
    let manifest_snapshot = snapshot_file(&manifest_path).expect("manifest snapshot");
    let mut route_snapshots = target_snapshots;
    route_snapshots.extend(backup_snapshots);
    route_snapshots.push(manifest_snapshot);
    let persistent_snapshots = build_persistent_codex_route_snapshots(&handle, &route_snapshots)
        .expect("persistent snapshots");

    let manager_paths =
        crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&handle)
            .expect("manager paths");
    manager_paths.ensure_dirs().expect("manager dirs");
    let store = crate::infra::codex_retry_gateway::FileCodexRetryGatewayTransitionStore::new(
        manager_paths.transition_path.clone(),
    );
    let transition = prepare_codex_route_transition(
        &store,
        CodexRouteTransitionPreparation {
            operation_kind:
                crate::infra::codex_retry_gateway::CodexRetryGatewayOperationKind::Enable,
            prior_state: &prior,
            target_mode: CodexRouteMode::DirectAio,
            prior_canonical_config_sha256: prior_canonical_sha256.clone(),
            prior_live_config_sha256: sha256_hex(&prior_live_bytes),
            canonical_config_sha256: sha256_hex(&canonical_bytes),
            live_config_sha256: sha256_hex(b"partial-live-config"),
            source_commit: None,
            process_should_run: true,
            snapshots: persistent_snapshots.metadata,
            snapshot_bytes: persistent_snapshots.bytes,
        },
    )
    .expect("prepare persistent route transaction");

    let config_path = codex_config_path(&handle).expect("config path");
    let auth_path = codex_auth_path(&handle).expect("auth path");
    write_cli_proxy_file_atomic(&config_path, b"partial-live-config")
        .expect("write partial config");
    write_cli_proxy_file_atomic(&auth_path, b"partial-auth").expect("write partial auth");
    let files_dir =
        cli_proxy_files_dir(&cli_proxy_root_dir(&handle, "codex").expect("cli proxy root"));
    std::fs::create_dir_all(&files_dir).expect("create backup files dir");
    std::fs::write(files_dir.join("config.toml"), b"partial-config-backup")
        .expect("write partial config backup");
    std::fs::write(files_dir.join("auth.json"), b"partial-auth-backup")
        .expect("write partial auth backup");
    std::fs::write(&manifest_path, b"{").expect("write malformed partial manifest");

    let reconciled = reconcile_pending_route(&handle, &store).expect("reconcile interrupted route");
    assert_eq!(reconciled.pending_transition_reconciled, Some(false));
    assert_eq!(reconciled.route.generation, transition.prior_generation);
    assert_eq!(reconciled.route.route_mode, transition.prior_mode);
    assert_eq!(
        std::fs::read(&config_path).expect("restored config"),
        original_config.as_bytes()
    );
    assert_eq!(
        std::fs::read(&auth_path).expect("restored auth"),
        original_auth.as_bytes()
    );
    assert!(!files_dir.join("config.toml").exists());
    assert!(!files_dir.join("auth.json").exists());
    assert!(!manifest_path.exists());
    assert!(!manager_paths.transition_path.exists());
}

#[test]
fn corrupt_route_journal_is_quarantined_and_no_longer_blocks_direct_aio_fail_open() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"
"#,
        r#"{"profile":"local"}"#,
    );
    let manager_paths =
        crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&handle)
            .expect("manager paths");
    manager_paths.ensure_dirs().expect("manager dirs");
    let store = crate::infra::codex_retry_gateway::FileCodexRetryGatewayTransitionStore::new(
        manager_paths.transition_path.clone(),
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("guarded plan");
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("apply guarded route");
    assert_eq!(guarded.route.route_mode, CodexRouteMode::Guarded);

    std::fs::write(&manager_paths.transition_path, b"{").expect("corrupt route journal");
    let reconciled = reconcile_pending_route(&handle, &store).expect("quarantine corrupt journal");
    assert!(
        reconciled
            .recovery_warning
            .as_deref()
            .is_some_and(|warning| warning.contains("TRANSITION_CORRUPT")),
        "{reconciled:?}"
    );
    assert!(!manager_paths.transition_path.exists());

    let current = verify_route(&handle).expect("verify guarded route before fail-open");
    let direct = apply_direct_aio_route(
        &handle,
        &store,
        CodexDirectAioRouteApplyRequest {
            expected_generation: current.generation,
            expected_canonical_sha256: current.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("corrupt journal must not block direct-AIO fail-open");
    assert_eq!(direct.route.route_mode, CodexRouteMode::DirectAio);
    assert!(direct.route.live_matches_projection);
    assert!(direct.route.auth_matches_projection);
}

#[test]
fn route_snapshot_recovery_preflights_every_backup_before_writing_targets() {
    for corruption in ["missing", "hash"] {
        let dir = tempfile::tempdir().expect("transition dir");
        let store = crate::infra::codex_retry_gateway::FileCodexRetryGatewayTransitionStore::new(
            dir.path().join("transition.json"),
        );
        let snapshots = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .enumerate()
            .map(|(index, bytes)| CodexRetryGatewayRouteSnapshot {
                root: CodexRetryGatewayRouteSnapshotRoot::CodexHome,
                root_path_sha256: format!("sha256:{}", "a".repeat(64)),
                target_rel: format!("target-{index}.json"),
                existed: true,
                backup_rel: Some(format!("files/{index:08}.bin")),
                backup_sha256: Some(sha256_hex(bytes)),
            })
            .collect::<Vec<_>>();
        let transition = CodexRetryGatewayRouteTransition {
            schema_version: CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION,
            operation_id: format!("preflight-{corruption}"),
            operation_kind:
                crate::infra::codex_retry_gateway::CodexRetryGatewayOperationKind::Recover,
            prior_generation: 1,
            target_generation: 2,
            prior_mode: CodexRouteMode::Guarded,
            target_mode: CodexRouteMode::DirectAio,
            prior_canonical_config_sha256: format!("sha256:{}", "b".repeat(64)),
            prior_live_config_sha256: format!("sha256:{}", "c".repeat(64)),
            canonical_config_sha256: format!("sha256:{}", "d".repeat(64)),
            live_config_sha256: format!("sha256:{}", "e".repeat(64)),
            source_commit: None,
            process_should_run: true,
            snapshots,
        };
        store
            .prepare(
                &transition,
                &[Some(b"first".to_vec()), Some(b"second".to_vec())],
            )
            .expect("prepare route snapshots");
        let second_backup = dir.path().join("transition-snapshots/files/00000001.bin");
        if corruption == "missing" {
            std::fs::remove_file(&second_backup).expect("remove second backup");
        } else {
            std::fs::write(&second_backup, b"corrupt").expect("corrupt second backup");
        }

        let error = preflight_persistent_codex_route_snapshot_bytes(&store, &transition)
            .expect_err("every corrupt backup must fail preflight");
        assert!(
            error.to_string().contains("snapshot"),
            "{corruption}: {error}"
        );
    }
}

#[test]
fn verify_route_rejects_commented_expected_projection_and_wrong_auth() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let aio_origin = "http://127.0.0.1:37123";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"
"#,
        r#"{"profile":"local"}"#,
    );
    set_enabled(&handle, "codex", true, aio_origin).expect("enable direct-AIO route");

    let config_path = codex_config_path(&handle).expect("config path");
    std::fs::write(
        &config_path,
        format!(
            "# model_provider = \"aio\"\n# [model_providers.aio]\n# base_url = \"{aio_origin}/v1\"\nmodel_provider = \"other\"\n[model_providers.other]\nname = \"other\"\nbase_url = \"https://example.invalid/v1\"\n"
        ),
    )
    .expect("write drifted config with misleading comments");
    std::fs::write(
        codex_auth_path(&handle).expect("auth path"),
        br#"{"OPENAI_API_KEY":"wrong-key"}"#,
    )
    .expect("write wrong auth projection");

    let verified = verify_route(&handle).expect("route verification remains observable");
    assert_eq!(verified.route_mode, CodexRouteMode::DirectAio);
    assert!(!verified.live_matches_projection, "{verified:?}");
    assert!(!verified.auth_matches_projection, "{verified:?}");
}

#[test]
fn sync_enabled_skips_codex_while_provider_sync_recovery_is_pending() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let original_origin = "http://127.0.0.1:37123";
    let next_origin = "http://127.0.0.1:37124";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"
"#,
        r#"{"profile":"local"}"#,
    );
    set_enabled(&handle, "codex", true, original_origin).expect("enable codex proxy");
    let config_path = codex::codex_config_path(&handle).expect("Codex config path");
    let config_before = std::fs::read(&config_path).expect("read config before pending recovery");

    let codex_home = crate::codex_paths::codex_home_dir(&handle).expect("Codex home");
    let transaction_root = codex_home.join("tmp/provider-sync-transaction");
    std::fs::create_dir_all(&transaction_root).expect("create pending transaction root");
    std::fs::write(
        transaction_root.join("journal.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "operation_id": "interrupted-route",
            "phase": "prepared",
            "target_provider": "OpenAI",
            "target_config_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "route": null,
            "backup_dir_rel": "backups_state/provider-sync/interrupted",
            "backup_root_existed": true,
            "snapshots": []
        }))
        .expect("serialize pending transaction"),
    )
    .expect("write pending transaction");

    let results = sync_enabled(&handle, next_origin, true).expect("sync remains available");
    let codex = results
        .iter()
        .find(|result| result.cli_key == "codex")
        .expect("Codex result");

    assert!(!codex.ok, "{codex:?}");
    assert_eq!(
        codex.error_code.as_deref(),
        Some("CLI_PROXY_PROVIDER_SYNC_RECOVERY_REQUIRED")
    );
    assert_eq!(
        std::fs::read(&config_path).expect("read unchanged config"),
        config_before
    );
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("Codex manifest");
    assert_eq!(manifest.base_origin.as_deref(), Some(original_origin));
}

#[test]
fn plan_external_enable_reports_cli_proxy_and_provider_sync_requirements() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();

    write_codex_direct_files(
        &handle,
        r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, "http://127.0.0.1:37123", "http://127.0.0.1:4610")
        .expect("plan external enable");

    assert_eq!(plan.current_route_mode, CodexRouteMode::Unproxied);
    assert!(plan.cli_proxy_enable_required, "{plan:?}");
    assert!(plan.route_change_required, "{plan:?}");
    assert_eq!(
        plan.provider_sync.current_provider.as_deref(),
        Some("Anthropic")
    );
    assert_eq!(plan.provider_sync.target_provider, "aio");
    assert!(plan.provider_sync.change_required, "{plan:?}");
    assert!(plan.provider_sync.codex_must_be_closed, "{plan:?}");
}

#[test]
fn apply_guarded_route_rolls_back_when_provider_sync_requires_closed_codex() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let config = r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#;
    let auth = r#"{
  "profile": "local"
}"#;
    write_codex_direct_files(&handle, config, auth);

    let plan = plan_external_enable(&handle, "http://127.0.0.1:37123", "http://127.0.0.1:4610")
        .expect("plan external enable");

    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(true));
    let err = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: "http://127.0.0.1:37123".to_string(),
            guarded_origin: "http://127.0.0.1:4610".to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect_err("running codex should block provider sync");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    assert!(err
        .to_string()
        .contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"));
    assert_eq!(
        std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
            .expect("read config"),
        config
    );
    assert_eq!(
        std::fs::read_to_string(codex_auth_path(&handle).expect("auth path")).expect("read auth"),
        auth
    );
    assert!(read_manifest(&handle, "codex")
        .expect("read manifest")
        .is_none());
    assert!(store.load_pending().expect("load pending").is_none());
}

#[test]
fn restore_unproxied_route_requires_closed_codex_when_canonical_provider_changes() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    let original_config = r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#;
    write_codex_direct_files(
        &handle,
        original_config,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("apply guarded route");

    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(true));
    let err = restore_unproxied_route(
        &handle,
        &store,
        CodexRestoreUnproxiedRouteRequest {
            expected_generation: guarded.route.generation,
            expected_canonical_sha256: guarded.route.canonical_config_sha256.clone(),
            aio_origin: Some(aio_origin.to_string()),
            desired_enabled: false,
            keep_cli_proxy_enabled: false,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect_err("running codex should block restore provider sync");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    assert!(err
        .to_string()
        .contains("CODEX_PROVIDER_SYNC_PROCESS_RUNNING"));
    let verified = verify_route(&handle).expect("verify guarded route after failed restore");
    assert_eq!(verified.route_mode, CodexRouteMode::Guarded);
    assert_eq!(verified.effective_origin.as_deref(), Some(guarded_origin));
    assert!(verified.live_matches_projection, "{verified:?}");
    assert!(verified.auth_matches_projection, "{verified:?}");
    assert!(store.load_pending().expect("load pending").is_none());
}

#[test]
fn restore_unproxied_route_fails_closed_when_canonical_provider_missing() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: true,
        },
    )
    .expect("apply guarded route");
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let backup_path = backup_file_path_for_manifest(&handle, &manifest, "codex_config_toml")
        .expect("config backup path lookup")
        .expect("config backup path");
    std::fs::write(&backup_path, "approval_policy = \"on-request\"\n")
        .expect("overwrite canonical backup");
    let state = codex_cli_proxy_state(&handle)
        .expect("load codex route state")
        .expect("managed codex route state");
    let canonical_sha256 = sha256_hex(
        &current_canonical_codex_config_bytes(&handle, &state)
            .expect("current canonical config bytes after backup mutation"),
    );

    let err = restore_unproxied_route(
        &handle,
        &store,
        CodexRestoreUnproxiedRouteRequest {
            expected_generation: guarded.route.generation,
            expected_canonical_sha256: canonical_sha256,
            aio_origin: Some(aio_origin.to_string()),
            desired_enabled: false,
            keep_cli_proxy_enabled: false,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect_err("missing canonical provider should fail closed");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_INVALID_TARGET"),
        "{err_text}"
    );
    let verified = verify_route(&handle).expect("verify guarded route after failed restore");
    assert_eq!(verified.route_mode, CodexRouteMode::Guarded);
    assert_eq!(verified.effective_origin.as_deref(), Some(guarded_origin));
    assert!(store.load_pending().expect("load pending").is_none());
    let current_config = std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
        .expect("read current config after failed restore");
    assert!(current_config.contains(&format!("base_url = \"{guarded_origin}/v1\"")));
}

#[test]
fn restore_unproxied_route_fails_closed_when_canonical_provider_empty() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: true,
        },
    )
    .expect("apply guarded route");
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let backup_path = backup_file_path_for_manifest(&handle, &manifest, "codex_config_toml")
        .expect("config backup path lookup")
        .expect("config backup path");
    std::fs::write(&backup_path, "model_provider = \"\"\n").expect("overwrite canonical backup");
    let state = codex_cli_proxy_state(&handle)
        .expect("load codex route state")
        .expect("managed codex route state");
    let canonical_sha256 = sha256_hex(
        &current_canonical_codex_config_bytes(&handle, &state)
            .expect("current canonical config bytes after backup mutation"),
    );

    let err = restore_unproxied_route(
        &handle,
        &store,
        CodexRestoreUnproxiedRouteRequest {
            expected_generation: guarded.route.generation,
            expected_canonical_sha256: canonical_sha256,
            aio_origin: Some(aio_origin.to_string()),
            desired_enabled: false,
            keep_cli_proxy_enabled: false,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect_err("empty canonical provider should fail closed");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    let err_text = err.to_string();
    assert!(
        err_text.contains("CODEX_PROVIDER_SYNC_INVALID_TARGET"),
        "{err_text}"
    );
    let verified = verify_route(&handle).expect("verify guarded route after failed restore");
    assert_eq!(verified.route_mode, CodexRouteMode::Guarded);
    assert_eq!(verified.effective_origin.as_deref(), Some(guarded_origin));
    assert!(store.load_pending().expect("load pending").is_none());
    let current_config = std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
        .expect("read current config after failed restore");
    assert!(current_config.contains(&format!("base_url = \"{guarded_origin}/v1\"")));
}

#[test]
fn restore_unproxied_route_preserves_remote_compaction_openai_behavior() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    let original_config = r#"model_provider = "OpenAI"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "https://api.openai.com/v1"

[features]
remote_compaction = true
"#;
    write_codex_direct_files(
        &handle,
        original_config,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    assert_eq!(
        plan.provider_sync.current_provider.as_deref(),
        Some("OpenAI")
    );
    assert_eq!(plan.provider_sync.target_provider, "OpenAI");
    assert!(!plan.provider_sync.change_required, "{plan:?}");

    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: true,
        },
    )
    .expect("apply guarded route");
    assert!(guarded.provider_sync.is_none(), "{guarded:?}");

    let restore = restore_unproxied_route(
        &handle,
        &store,
        CodexRestoreUnproxiedRouteRequest {
            expected_generation: guarded.route.generation,
            expected_canonical_sha256: guarded.route.canonical_config_sha256.clone(),
            aio_origin: Some(aio_origin.to_string()),
            desired_enabled: false,
            keep_cli_proxy_enabled: false,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect("restore unproxied route");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    assert_eq!(restore.route.route_mode, CodexRouteMode::Unproxied);
    assert!(restore.provider_sync.is_none(), "{restore:?}");
    let final_config = std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
        .expect("read final config");
    assert!(
        final_config.contains("model_provider = \"OpenAI\""),
        "{final_config}"
    );
    assert!(
        final_config.contains("remote_compaction = true"),
        "{final_config}"
    );
    assert!(
        final_config.contains("base_url = \"https://api.openai.com/v1\""),
        "{final_config}"
    );
    assert!(!final_config.contains(guarded_origin), "{final_config}");
}

#[test]
fn codex_route_coordinator_applies_guarded_direct_and_unproxied_transitions() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";
    let original_config = r#"model_provider = "Anthropic"

[model_providers.Anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
"#;
    write_codex_direct_files(
        &handle,
        original_config,
        r#"{
  "profile": "local"
}"#,
    );

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("apply guarded route");

    assert_eq!(guarded.route.route_mode, CodexRouteMode::Guarded);
    assert_eq!(
        guarded.route.effective_origin.as_deref(),
        Some(guarded_origin)
    );
    assert!(guarded.route.live_matches_projection, "{guarded:?}");
    assert!(guarded.route.auth_matches_projection, "{guarded:?}");
    assert!(guarded.provider_sync.is_some(), "{guarded:?}");

    let verify_guarded = verify_route(&handle).expect("verify guarded");
    assert_eq!(verify_guarded.route_mode, CodexRouteMode::Guarded);
    assert_eq!(
        verify_guarded.effective_origin.as_deref(),
        Some(guarded_origin)
    );

    let direct = apply_direct_aio_route(
        &handle,
        &store,
        CodexDirectAioRouteApplyRequest {
            expected_generation: guarded.route.generation,
            expected_canonical_sha256: guarded.route.canonical_config_sha256.clone(),
            aio_origin: aio_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect("apply direct aio route");

    assert_eq!(direct.route.route_mode, CodexRouteMode::DirectAio);
    assert_eq!(direct.route.effective_origin.as_deref(), Some(aio_origin));
    assert!(direct.route.live_matches_projection, "{direct:?}");
    assert!(direct.route.auth_matches_projection, "{direct:?}");

    let restore = restore_unproxied_route(
        &handle,
        &store,
        CodexRestoreUnproxiedRouteRequest {
            expected_generation: direct.route.generation,
            expected_canonical_sha256: direct.route.canonical_config_sha256.clone(),
            aio_origin: Some(aio_origin.to_string()),
            desired_enabled: false,
            keep_cli_proxy_enabled: false,
            source_commit: None,
            process_should_run: false,
        },
    )
    .expect("restore unproxied route");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);

    assert_eq!(restore.route.route_mode, CodexRouteMode::Unproxied);
    assert!(!restore.route.cli_proxy_enabled, "{restore:?}");
    assert!(restore.route.live_matches_projection, "{restore:?}");
    assert!(restore.route.auth_matches_projection, "{restore:?}");
    assert_eq!(
        std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
            .expect("read final config")
            .trim_end(),
        original_config.trim_end()
    );
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    assert!(!manifest.enabled);
    let codex = manifest.codex.expect("codex metadata");
    assert_eq!(codex.route_mode, CodexRouteMode::Unproxied);
    assert!(!codex.desired_enabled);
}

#[tokio::test(flavor = "current_thread")]
async fn managed_remote_compaction_keeps_runtime_provider_identity_in_guarded_and_direct_modes() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"

[features]
remote_compaction = false
"#,
        r#"{"profile":"local"}"#,
    );

    let probe = install_managed_provider_probe(
        &handle,
        aio_origin,
        ManagedProviderProbeMode::FollowRuntimeState,
    )
    .await;
    let guarded_origin = probe.listener.as_str();

    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    let guarded = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("guarded route");
    assert_eq!(guarded.route.route_mode, CodexRouteMode::Guarded);

    let paths = crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&handle)
        .expect("gateway paths");

    let patch: crate::infra::codex_config::CodexConfigPatch =
        serde_json::from_value(serde_json::json!({ "features_remote_compaction": true }))
            .expect("remote compaction patch");
    let handle_for_mutation = handle.clone();
    crate::commands::cli_manager::run_codex_config_stage(
        handle.clone(),
        "test_enable_remote_compaction_while_guarded",
        move || crate::infra::codex_config::codex_config_set_staged(&handle_for_mutation, patch),
    )
    .await
    .expect("enable remote compaction while guarded");

    let guarded_after = verify_route(&handle).expect("verify guarded after provider rename");
    assert_eq!(guarded_after.route_mode, CodexRouteMode::Guarded);
    assert_eq!(
        guarded_after.effective_origin.as_deref(),
        Some(guarded_origin)
    );
    assert!(guarded_after.live_matches_projection);
    let manager_after = crate::infra::codex_retry_gateway::read_manager_state(&paths)
        .expect("manager after guarded rename");
    assert_eq!(
        manager_after
            .process_record
            .as_ref()
            .map(|record| record.provider_name.as_str()),
        Some(crate::infra::codex_retry_gateway::MANAGED_PROVIDER_OPENAI)
    );
    let state_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&paths.runtime_state_path).expect("runtime state after guarded rename"),
    )
    .expect("runtime state JSON");
    assert_eq!(state_after["provider_name"], "OpenAI");

    let direct = apply_direct_aio_route(
        &handle,
        &store,
        CodexDirectAioRouteApplyRequest {
            expected_generation: guarded_after.generation,
            expected_canonical_sha256: guarded_after.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: true,
        },
    )
    .expect("direct AIO route");
    assert_eq!(direct.route.route_mode, CodexRouteMode::DirectAio);
    let raw = crate::infra::codex_config::codex_config_toml_get_raw(&handle)
        .expect("canonical raw config")
        .toml
        .replace("model_provider = \"OpenAI\"", "model_provider = \"aio\"")
        .replace("[model_providers.OpenAI]", "[model_providers.aio]")
        .replace("name = \"OpenAI\"", "name = \"aio\"")
        .replace("remote_compaction = true", "remote_compaction = false");
    let handle_for_mutation = handle.clone();
    crate::commands::cli_manager::run_codex_config_stage(
        handle.clone(),
        "test_disable_remote_compaction_while_direct",
        move || {
            crate::infra::codex_config::codex_config_toml_set_raw_staged(&handle_for_mutation, raw)
        },
    )
    .await
    .expect("disable remote compaction while direct");

    let direct_after = verify_route(&handle).expect("verify direct after provider rename");
    assert_eq!(direct_after.route_mode, CodexRouteMode::DirectAio);
    assert_eq!(direct_after.effective_origin.as_deref(), Some(aio_origin));
    assert!(direct_after.live_matches_projection);
    let manager_after = crate::infra::codex_retry_gateway::read_manager_state(&paths)
        .expect("manager after direct rename");
    assert_eq!(
        manager_after
            .process_record
            .as_ref()
            .map(|record| record.provider_name.as_str()),
        Some(crate::infra::codex_retry_gateway::MANAGED_PROVIDER_AIO)
    );
    let state_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&paths.runtime_state_path).expect("runtime state after direct rename"),
    )
    .expect("runtime state JSON");
    assert_eq!(state_after["provider_name"], "aio");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);
}

#[tokio::test(flavor = "current_thread")]
async fn managed_provider_probe_failure_restores_every_mutated_projection() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"

[features]
remote_compaction = false
"#,
        r#"{"profile":"local"}"#,
    );

    let probe = install_managed_provider_probe(
        &handle,
        aio_origin,
        ManagedProviderProbeMode::Fixed(
            crate::infra::codex_retry_gateway::MANAGED_PROVIDER_AIO.to_string(),
        ),
    )
    .await;
    let guarded_origin = probe.listener.as_str();
    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("guarded route");

    let codex_home = crate::codex_paths::codex_home_dir(&handle).expect("Codex home");
    let rollout_path = codex_home
        .join("sessions")
        .join("2026")
        .join("rollout-provider-rollback.jsonl");
    std::fs::create_dir_all(rollout_path.parent().expect("rollout parent"))
        .expect("create rollout parent");
    std::fs::write(
        &rollout_path,
        b"{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"aio\"}}\n",
    )
    .expect("write rollout");

    let paths = crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&handle)
        .expect("gateway paths");
    let provider_sync_backup_root =
        codex_home.join(crate::infra::codex_provider_sync::PROVIDER_SYNC_BACKUP_ROOT);
    let backup_root_existed_before = provider_sync_backup_root.exists();
    let backup_entries_before = if backup_root_existed_before {
        std::fs::read_dir(&provider_sync_backup_root)
            .expect("read provider sync backups before mutation")
            .map(|entry| {
                entry
                    .expect("provider sync backup entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        std::collections::BTreeSet::new()
    };
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let canonical_backup_path =
        backup_file_path_for_manifest(&handle, &manifest, "codex_config_toml")
            .expect("backup lookup")
            .expect("canonical backup exists");
    let manifest_path =
        cli_proxy_manifest_path(&cli_proxy_root_dir(&handle, "codex").expect("CLI proxy root"));
    let live_config_path = codex_config_path(&handle).expect("live config path");
    let snapshots = [
        ("live config", live_config_path),
        ("rollout", rollout_path),
        ("CLI manifest", manifest_path),
        ("canonical backup", canonical_backup_path),
        ("manager state", paths.manager_path.clone()),
        ("runtime state", paths.runtime_state_path.clone()),
    ]
    .map(|(label, path)| {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("read {label} snapshot {}: {error}", path.display()));
        (label, path, bytes)
    });

    let patch: crate::infra::codex_config::CodexConfigPatch =
        serde_json::from_value(serde_json::json!({ "features_remote_compaction": true }))
            .expect("remote compaction patch");
    let handle_for_mutation = handle.clone();
    let error = crate::commands::cli_manager::run_codex_config_stage(
        handle.clone(),
        "test_reject_stale_managed_provider",
        move || crate::infra::codex_config::codex_config_set_staged(&handle_for_mutation, patch),
    )
    .await
    .expect_err("stale provider health must reject the mutation");
    assert!(
        error.contains("CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED"),
        "{error}"
    );

    for (label, path, expected) in snapshots {
        assert_eq!(
            std::fs::read(&path).unwrap_or_else(|read_error| panic!(
                "read {label} {}: {read_error}",
                path.display()
            )),
            expected,
            "{label} must be restored byte-for-byte"
        );
    }
    let backup_root_existed_after = provider_sync_backup_root.exists();
    let backup_entries_after = if backup_root_existed_after {
        std::fs::read_dir(&provider_sync_backup_root)
            .expect("read provider sync backups after rollback")
            .map(|entry| {
                entry
                    .expect("provider sync backup entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        std::collections::BTreeSet::new()
    };
    assert_eq!(backup_root_existed_after, backup_root_existed_before);
    assert_eq!(backup_entries_after, backup_entries_before);
    let verified = verify_route(&handle).expect("verify guarded route after rollback");
    assert_eq!(verified.route_mode, CodexRouteMode::Guarded);
    assert_eq!(verified.effective_origin.as_deref(), Some(guarded_origin));
    assert!(verified.live_matches_projection, "{verified:?}");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_provider_sync_transaction_is_quarantined_before_route_recovery() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    write_codex_direct_files(
        &handle,
        r#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://api.openai.com/v1"

[features]
remote_compaction = false
"#,
        r#"{"profile":"local"}"#,
    );
    let probe = install_managed_provider_probe(
        &handle,
        aio_origin,
        ManagedProviderProbeMode::Fixed(
            crate::infra::codex_retry_gateway::MANAGED_PROVIDER_AIO.to_string(),
        ),
    )
    .await;
    let plan = plan_external_enable(&handle, aio_origin, &probe.listener).expect("plan");
    crate::test_support::codex_provider_sync_set_running_override_for_tests(Some(false));
    apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            guarded_origin: probe.listener.clone(),
            desired_enabled: true,
            source_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            process_should_run: true,
        },
    )
    .expect("guarded route");

    let paths = crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&handle)
        .expect("gateway paths");
    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let backup_path = backup_file_path_for_manifest(&handle, &manifest, "codex_config_toml")
        .expect("backup lookup")
        .expect("canonical backup");
    let manifest_path = codex_route_manifest_path(&handle).expect("manifest path");
    let config_path = codex_config_path(&handle).expect("config path");
    let prior_files = [
        config_path.clone(),
        manifest_path.clone(),
        backup_path.clone(),
        paths.manager_path.clone(),
        paths.runtime_state_path.clone(),
    ]
    .map(|path| {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("read prior {}: {error}", path.display()));
        (path, bytes)
    });

    let patch: crate::infra::codex_config::CodexConfigPatch =
        serde_json::from_value(serde_json::json!({ "features_remote_compaction": true }))
            .expect("remote compaction patch");
    let mut staged = crate::infra::codex_config::codex_config_set_staged(&handle, patch)
        .expect("stage managed provider change");
    let transaction = staged.transaction.take().expect("managed transaction");
    std::mem::forget(transaction);
    assert!(
        paths.transition_path.exists(),
        "route journal must remain pending"
    );
    assert!(
        crate::infra::codex_provider_sync::has_pending_provider_sync_recovery(&handle)
            .expect("provider journal status")
    );
    let codex_home = crate::codex_paths::codex_home_dir(&handle).expect("Codex home");
    let provider_transaction_root = codex_home.join("tmp/provider-sync-transaction");
    std::fs::write(provider_transaction_root.join("journal.json"), b"{")
        .expect("corrupt provider sync journal");
    std::fs::write(&manifest_path, b"{").expect("corrupt partial manifest");

    let file_store = crate::infra::codex_retry_gateway::FileCodexRetryGatewayTransitionStore::new(
        paths.transition_path.clone(),
    );
    let reconciled =
        reconcile_pending_route(&handle, &file_store).expect("recover interrupted provider change");
    assert_eq!(reconciled.pending_transition_reconciled, Some(false));
    assert!(
        reconciled
            .recovery_warning
            .as_deref()
            .is_some_and(|warning| warning.contains("CODEX_PROVIDER_SYNC_TRANSITION_CORRUPT")),
        "{reconciled:?}"
    );
    assert_eq!(reconciled.route.route_mode, CodexRouteMode::Guarded);
    assert!(reconciled.route.live_matches_projection);
    assert!(reconciled.route.auth_matches_projection);
    for (path, expected) in prior_files {
        assert_eq!(
            std::fs::read(&path)
                .unwrap_or_else(|error| panic!("read restored {}: {error}", path.display())),
            expected,
            "{} must be restored byte-for-byte",
            path.display()
        );
    }
    assert!(!paths.transition_path.exists());
    assert!(
        !crate::infra::codex_provider_sync::has_pending_provider_sync_recovery(&handle)
            .expect("provider journal cleared")
    );
    let quarantine_root = codex_home.join("backups_state/provider-sync-quarantine");
    let quarantine = std::fs::read_dir(&quarantine_root)
        .expect("read provider sync quarantine")
        .next()
        .expect("provider sync quarantine entry")
        .expect("read provider sync quarantine entry")
        .path();
    assert!(quarantine.join("transaction").is_dir());
    assert!(quarantine.join("quarantine.json").is_file());
    crate::test_support::codex_provider_sync_set_running_override_for_tests(None);
}

#[test]
fn apply_guarded_route_rejects_stale_generation_after_managed_config_edit() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let store = FakeTransitionStore::default();
    let aio_origin = "http://127.0.0.1:37123";
    let guarded_origin = "http://127.0.0.1:4610";

    let enabled = set_enabled(&handle, "codex", true, aio_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");
    let plan = plan_external_enable(&handle, aio_origin, guarded_origin).expect("plan");

    crate::infra::codex_config::codex_config_toml_set_raw(
        &handle,
        "approval_policy = \"on-request\"\n".to_string(),
    )
    .expect("update managed canonical config");

    let err = apply_guarded_route(
        &handle,
        &store,
        CodexGuardedRouteApplyRequest {
            expected_generation: plan.generation,
            expected_canonical_sha256: plan.canonical_config_sha256,
            aio_origin: aio_origin.to_string(),
            guarded_origin: guarded_origin.to_string(),
            desired_enabled: true,
            source_commit: None,
            process_should_run: true,
        },
    )
    .expect_err("stale generation/hash should fail");

    let err_text = err.to_string();
    assert!(
        err_text.contains("CLI_PROXY_STALE_ROUTE_GENERATION")
            || err_text.contains("CLI_PROXY_STALE_ROUTE_HASH"),
        "{err_text}"
    );
}

#[test]
fn status_all_prefers_current_gateway_origin_when_available() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let manifest_origin = "http://127.0.0.1:37123";
    let current_origin = "http://127.0.0.1:37125";

    write_cli_proxy_manifest(&handle, "codex", true, Some(manifest_origin));
    write_codex_proxy_files(&handle, current_origin);

    let rows = status_all(&handle, Some(current_origin)).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(codex.enabled);
    assert_eq!(codex.base_origin.as_deref(), Some(manifest_origin));
    assert_eq!(
        codex.current_gateway_origin.as_deref(),
        Some(current_origin)
    );
    assert_eq!(codex.applied_to_current_gateway, Some(true));
}

#[test]
fn status_all_reports_drift_against_current_gateway_origin() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let manifest_origin = "http://127.0.0.1:37123";
    let current_origin = "http://127.0.0.1:37125";

    write_cli_proxy_manifest(&handle, "codex", true, Some(manifest_origin));
    write_codex_proxy_files(&handle, manifest_origin);

    let rows = status_all(&handle, Some(current_origin)).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(codex.enabled);
    assert_eq!(codex.base_origin.as_deref(), Some(manifest_origin));
    assert_eq!(
        codex.current_gateway_origin.as_deref(),
        Some(current_origin)
    );
    assert_eq!(codex.applied_to_current_gateway, Some(false));
}

#[test]
fn enabling_codex_oauth_compatible_proxy_writes_config_only_and_does_not_create_auth() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    let result = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(result.ok, "{result:?}");

    let config_path = codex_config_path(&handle).expect("config path");
    let auth_path = codex_auth_path(&handle).expect("auth path");
    let config = std::fs::read_to_string(config_path).expect("read config");

    assert!(config.contains("model_provider = \"aio\""), "{config}");
    assert!(
        config.contains(&format!("base_url = \"{base_origin}/v1\"")),
        "{config}"
    );
    assert!(config.contains("requires_openai_auth = true"), "{config}");
    assert!(!config.contains("preferred_auth_method"), "{config}");
    assert!(
        !auth_path.exists(),
        "oauth compatible proxy must not create auth.json at {}",
        auth_path.display()
    );

    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    assert!(manifest
        .files
        .iter()
        .any(|entry| entry.kind == "codex_config_toml"));
    assert!(
        !manifest
            .files
            .iter()
            .any(|entry| entry.kind == "codex_auth_json"),
        "oauth compatible manifest should not target auth.json: {manifest:?}"
    );
}

#[test]
fn enabling_codex_oauth_compatible_proxy_preserves_existing_auth_json() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    let config = r#"
preferred_auth_method = "apikey"

[existing]
foo = "bar"
"#;
    let auth = r#"{
  "tokens": { "access": "oauth-token" },
  "last_refresh": 123,
  "profile": "teacher"
}"#;
    write_codex_direct_files(&handle, config, auth);
    let auth_path = codex_auth_path(&handle).expect("auth path");
    let before_auth = std::fs::read_to_string(&auth_path).expect("read auth before");

    let result = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(result.ok, "{result:?}");

    let after_auth = std::fs::read_to_string(&auth_path).expect("read auth after");
    let config_path = codex_config_path(&handle).expect("config path");
    let after_config = std::fs::read_to_string(config_path).expect("read config after");

    assert_eq!(after_auth, before_auth);
    assert!(
        !after_config.contains("preferred_auth_method"),
        "{after_config}"
    );
    assert!(after_config.contains(&format!("base_url = \"{base_origin}/v1\"")));
}

#[test]
fn codex_oauth_compatible_status_uses_config_without_auth_json() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    write_cli_proxy_manifest(&handle, "codex", true, Some(base_origin));
    write_codex_oauth_proxy_config(&handle, base_origin);

    let rows = status_all(&handle, Some(base_origin)).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert!(codex.enabled);
    assert_eq!(codex.applied_to_current_gateway, Some(true));
}

#[test]
fn codex_oauth_compatible_status_reports_drift_when_old_apikey_preference_remains() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    write_cli_proxy_manifest(&handle, "codex", true, Some(base_origin));
    write_codex_proxy_files(&handle, base_origin);

    let rows = status_all(&handle, Some(base_origin)).expect("status_all");
    let codex = rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex row");

    assert_eq!(codex.applied_to_current_gateway, Some(false));
}

#[test]
fn disabling_codex_oauth_compatible_proxy_restores_config_and_leaves_auth_json() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    let config = r#"[existing]
foo = "bar"
"#;
    let auth = r#"{
  "tokens": { "access": "oauth-token" },
  "profile": "teacher"
}"#;
    write_codex_direct_files(&handle, config, auth);
    let auth_path = codex_auth_path(&handle).expect("auth path");

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    let user_changed_auth = r#"{
  "tokens": { "access": "new-oauth-token" },
  "profile": "teacher",
  "user_added": true
}"#;
    std::fs::write(&auth_path, user_changed_auth).expect("write auth user change");

    let disabled = set_enabled(&handle, "codex", false, base_origin).expect("disable codex");
    assert!(disabled.ok, "{disabled:?}");

    let config_after = std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
        .expect("read config after disable");
    let auth_after = std::fs::read_to_string(auth_path).expect("read auth after disable");

    assert!(config_after.contains("[existing]"), "{config_after}");
    assert!(
        !config_after.contains("model_provider = \"aio\""),
        "{config_after}"
    );
    assert_eq!(auth_after, user_changed_auth);
}

#[test]
fn switching_codex_oauth_compatible_proxy_to_normal_mode_adds_auth_backup_and_writes_auth() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";
    set_codex_oauth_compatible_proxy_mode(&handle, true);

    let config = r#"[existing]
foo = "bar"
"#;
    let auth = r#"{
  "tokens": { "access": "oauth-token" },
  "last_refresh": 123,
  "profile": "teacher"
}"#;
    write_codex_direct_files(&handle, config, auth);
    let auth_path = codex_auth_path(&handle).expect("auth path");

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex oauth");
    assert!(enabled.ok, "{enabled:?}");

    let oauth_manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    assert!(
        !oauth_manifest
            .files
            .iter()
            .any(|entry| entry.kind == "codex_auth_json"),
        "oauth compatible manifest should not backup auth: {oauth_manifest:?}"
    );

    set_codex_oauth_compatible_proxy_mode(&handle, false);
    let sync_rows = sync_enabled(&handle, base_origin, true).expect("sync after mode switch");
    let codex_row = sync_rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex sync row");
    assert!(codex_row.ok, "{codex_row:?}");

    let config_after_sync =
        std::fs::read_to_string(codex_config_path(&handle).expect("config path"))
            .expect("read config after sync");
    assert!(
        config_after_sync.contains("preferred_auth_method = \"apikey\""),
        "{config_after_sync}"
    );

    let auth_after_sync = std::fs::read_to_string(&auth_path).expect("read auth after sync");
    let auth_after_sync_json: serde_json::Value =
        serde_json::from_str(&auth_after_sync).expect("parse auth after sync");
    assert_eq!(
        auth_after_sync_json
            .get("OPENAI_API_KEY")
            .and_then(|value| value.as_str()),
        Some(PLACEHOLDER_KEY)
    );
    assert_eq!(
        auth_after_sync_json
            .get("auth_mode")
            .and_then(|value| value.as_str()),
        Some("apikey")
    );
    assert!(auth_after_sync_json.get("tokens").is_none());

    let normal_manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let auth_entry = manifest_entry(&normal_manifest, "codex_auth_json");
    assert!(auth_entry.existed);
    let root = cli_proxy_root_dir(&handle, "codex").expect("codex root");
    let backup_rel = auth_entry.backup_rel.as_ref().expect("auth backup rel");
    let auth_backup =
        std::fs::read_to_string(cli_proxy_files_dir(&root).join(backup_rel)).expect("read backup");
    let auth_backup_json: serde_json::Value =
        serde_json::from_str(&auth_backup).expect("parse auth backup");
    assert_eq!(
        auth_backup_json
            .get("tokens")
            .and_then(|tokens| tokens.get("access"))
            .and_then(|value| value.as_str()),
        Some("oauth-token")
    );

    let disabled = set_enabled(&handle, "codex", false, base_origin).expect("disable codex");
    assert!(disabled.ok, "{disabled:?}");

    let auth_after_disable = std::fs::read_to_string(auth_path).expect("read auth after disable");
    let auth_after_disable_json: serde_json::Value =
        serde_json::from_str(&auth_after_disable).expect("parse auth after disable");
    assert_eq!(
        auth_after_disable_json
            .get("tokens")
            .and_then(|tokens| tokens.get("access"))
            .and_then(|value| value.as_str()),
        Some("oauth-token")
    );
    assert!(auth_after_disable_json.get("OPENAI_API_KEY").is_none());
    assert!(auth_after_disable_json.get("auth_mode").is_none());
}

#[test]
fn sync_enabled_rebases_codex_manifest_when_codex_home_changes() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    let old_codex_home = app.home.path().join("codex-old");
    let new_codex_home = app.home.path().join("codex-new");

    let old_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://old.example/v1"

[old_section]
marker = "old"
"#;
    let old_auth = r#"{
  "tokens": { "access": "old-token" },
  "profile": "old"
}"#;
    let new_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://new.example/v1"

[new_section]
marker = "new"
"#;
    let new_auth = r#"{
  "tokens": { "access": "new-token" },
  "profile": "new"
}"#;

    set_custom_codex_home(&handle, &old_codex_home);
    write_codex_direct_files(&handle, old_config, old_auth);
    let old_config_path = codex_config_path(&handle).expect("old config path");
    let old_auth_path = codex_auth_path(&handle).expect("old auth path");

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    set_custom_codex_home(&handle, &new_codex_home);
    write_codex_direct_files(&handle, new_config, new_auth);
    let new_config_path = codex_config_path(&handle).expect("new config path");
    let new_auth_path = codex_auth_path(&handle).expect("new auth path");

    assert_ne!(old_config_path, new_config_path);
    assert_ne!(old_auth_path, new_auth_path);

    let sync_rows = sync_enabled(&handle, base_origin, false).expect("sync enabled");
    let codex_row = sync_rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex sync result");
    assert!(codex_row.ok, "{codex_row:?}");
    assert_eq!(codex_row.message, "已重绑 Codex 目录基线，待网关启动后接管");

    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    let config_entry = manifest_entry(&manifest, "codex_config_toml");
    let auth_entry = manifest_entry(&manifest, "codex_auth_json");

    assert_eq!(manifest.base_origin.as_deref(), Some(base_origin));
    assert_eq!(PathBuf::from(&config_entry.path), new_config_path);
    assert_eq!(PathBuf::from(&auth_entry.path), new_auth_path);

    let rebound_config = std::fs::read_to_string(&new_config_path).expect("read rebound config");
    let rebound_auth = std::fs::read_to_string(&new_auth_path).expect("read rebound auth");
    let rebound_auth_json: serde_json::Value =
        serde_json::from_str(&rebound_auth).expect("parse rebound auth");

    assert!(
        rebound_config.contains("[new_section]"),
        "offline rebind should keep direct config in target file: {rebound_config}"
    );
    assert!(
        !rebound_config.contains("model_provider = \"aio\""),
        "offline rebind should not rewrite target config to proxy: {rebound_config}"
    );
    assert_eq!(
        rebound_auth_json
            .get("profile")
            .and_then(|value| value.as_str()),
        Some("new"),
        "offline rebind should keep direct auth in target file: {rebound_auth}"
    );
    assert!(
        rebound_auth_json.get("OPENAI_API_KEY").is_none(),
        "offline rebind should not inject proxy auth into target file: {rebound_auth}"
    );

    let root = cli_proxy_root_dir(&handle, "codex").expect("codex root");
    let files_dir = cli_proxy_files_dir(&root);
    let config_backup =
        std::fs::read_to_string(files_dir.join("config.toml")).expect("read config backup");
    let auth_backup =
        std::fs::read_to_string(files_dir.join("auth.json")).expect("read auth backup");
    let auth_backup_json: serde_json::Value =
        serde_json::from_str(&auth_backup).expect("parse auth backup");

    assert!(
        config_backup.contains("[new_section]"),
        "config backup should be rebound to new baseline: {config_backup}"
    );
    assert!(
        !config_backup.contains("[old_section]"),
        "config backup should stop using old baseline: {config_backup}"
    );
    assert_eq!(
        auth_backup_json
            .get("profile")
            .and_then(|value| value.as_str()),
        Some("new"),
        "auth backup should be rebound to new baseline: {auth_backup}"
    );
}

#[test]
fn sync_enabled_rebinds_and_applies_proxy_when_apply_live_true() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    let old_codex_home = app.home.path().join("codex-old");
    let new_codex_home = app.home.path().join("codex-new");

    let old_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://old.example/v1"

[old_section]
marker = "old"
"#;
    let old_auth = r#"{
  "tokens": { "access": "old-token" },
  "profile": "old"
}"#;
    let new_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://new.example/v1"

[new_section]
marker = "new"
"#;
    let new_auth = r#"{
  "tokens": { "access": "new-token" },
  "profile": "new"
}"#;

    set_custom_codex_home(&handle, &old_codex_home);
    write_codex_direct_files(&handle, old_config, old_auth);

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    set_custom_codex_home(&handle, &new_codex_home);
    write_codex_direct_files(&handle, new_config, new_auth);
    let new_config_path = codex_config_path(&handle).expect("new config path");
    let new_auth_path = codex_auth_path(&handle).expect("new auth path");

    let sync_rows = sync_enabled(&handle, base_origin, true).expect("sync enabled");
    let codex_row = sync_rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex sync result");
    assert!(codex_row.ok, "{codex_row:?}");
    assert_eq!(codex_row.message, "已重绑 Codex 目录并写入当前网关配置");

    let rebound_config = std::fs::read_to_string(&new_config_path).expect("read rebound config");
    let rebound_auth = std::fs::read_to_string(&new_auth_path).expect("read rebound auth");
    let rebound_auth_json: serde_json::Value =
        serde_json::from_str(&rebound_auth).expect("parse rebound auth");

    assert!(
        rebound_config.contains("model_provider = \"aio\""),
        "live rebind should rewrite target config to proxy: {rebound_config}"
    );
    assert!(
        rebound_config.contains(&format!("{base_origin}/v1")),
        "live rebind should point target config to current gateway: {rebound_config}"
    );
    assert_eq!(
        rebound_auth_json
            .get("OPENAI_API_KEY")
            .and_then(|value| value.as_str()),
        Some("aio-coding-hub"),
        "live rebind should inject proxy auth into target file: {rebound_auth}"
    );
    assert_eq!(
        rebound_auth_json
            .get("auth_mode")
            .and_then(|value| value.as_str()),
        Some("apikey"),
        "live rebind should mark auth mode for gateway auth: {rebound_auth}"
    );

    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(manifest.base_origin.as_deref(), Some(base_origin));
    assert_eq!(
        PathBuf::from(&manifest_entry(&manifest, "codex_config_toml").path),
        new_config_path
    );
    assert_eq!(
        PathBuf::from(&manifest_entry(&manifest, "codex_auth_json").path),
        new_auth_path
    );
}

#[test]
fn disabling_codex_after_rebind_restores_new_target_path_only() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    let old_codex_home = app.home.path().join("codex-old");
    let new_codex_home = app.home.path().join("codex-new");

    let old_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://old.example/v1"

[old_section]
marker = "old"
"#;
    let old_auth = r#"{
  "tokens": { "access": "old-token" },
  "profile": "old"
}"#;
    let new_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://new.example/v1"

[new_section]
marker = "new"
"#;
    let new_auth = r#"{
  "tokens": { "access": "new-token" },
  "profile": "new"
}"#;

    set_custom_codex_home(&handle, &old_codex_home);
    write_codex_direct_files(&handle, old_config, old_auth);
    let old_config_path = codex_config_path(&handle).expect("old config path");
    let old_auth_path = codex_auth_path(&handle).expect("old auth path");

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    set_custom_codex_home(&handle, &new_codex_home);
    write_codex_direct_files(&handle, new_config, new_auth);
    let new_config_path = codex_config_path(&handle).expect("new config path");
    let new_auth_path = codex_auth_path(&handle).expect("new auth path");

    let sync_rows = sync_enabled(&handle, base_origin, false).expect("sync enabled");
    let codex_row = sync_rows
        .into_iter()
        .find(|row| row.cli_key == "codex")
        .expect("codex sync result");
    assert!(codex_row.ok, "{codex_row:?}");
    assert_eq!(codex_row.message, "已重绑 Codex 目录基线，待网关启动后接管");

    let old_config_before_disable =
        std::fs::read_to_string(&old_config_path).expect("read old config before disable");
    let old_auth_before_disable =
        std::fs::read_to_string(&old_auth_path).expect("read old auth before disable");

    let disabled = set_enabled(&handle, "codex", false, base_origin).expect("disable codex");
    assert!(disabled.ok, "{disabled:?}");

    let old_config_after_disable =
        std::fs::read_to_string(&old_config_path).expect("read old config after disable");
    let old_auth_after_disable =
        std::fs::read_to_string(&old_auth_path).expect("read old auth after disable");
    let new_config_after_disable =
        std::fs::read_to_string(&new_config_path).expect("read new config after disable");
    let new_auth_after_disable =
        std::fs::read_to_string(&new_auth_path).expect("read new auth after disable");
    let new_auth_json: serde_json::Value =
        serde_json::from_str(&new_auth_after_disable).expect("parse new auth after disable");

    assert_eq!(
        old_config_after_disable, old_config_before_disable,
        "old codex_home config should stay untouched after rebind disable"
    );
    assert_eq!(
        old_auth_after_disable, old_auth_before_disable,
        "old codex_home auth should stay untouched after rebind disable"
    );

    assert!(
        new_config_after_disable.contains("[new_section]"),
        "new codex_home config should restore new baseline: {new_config_after_disable}"
    );
    assert!(
        !new_config_after_disable.contains("model_provider = \"aio\""),
        "new codex_home config should no longer point to proxy: {new_config_after_disable}"
    );
    assert_eq!(
        new_auth_json
            .get("profile")
            .and_then(|value| value.as_str()),
        Some("new"),
        "new codex_home auth should restore new baseline: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("tokens").is_some(),
        "new codex_home auth should restore direct tokens: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("OPENAI_API_KEY").is_none(),
        "new codex_home auth should remove proxy API key: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("auth_mode").is_none(),
        "new codex_home auth should remove proxy auth mode: {new_auth_after_disable}"
    );
}

#[test]
fn rebind_codex_home_adopts_existing_proxy_target_and_disable_restores_new_target_path() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    let old_codex_home = app.home.path().join("codex-old");
    let new_codex_home = app.home.path().join("codex-new");

    let old_config = r#"[model_providers.openai]
name = "openai"
base_url = "https://old.example/v1"

[old_section]
marker = "old"
"#;
    let old_auth = r#"{
  "tokens": { "access": "old-token" },
  "profile": "old"
}"#;

    set_custom_codex_home(&handle, &old_codex_home);
    write_codex_direct_files(&handle, old_config, old_auth);
    let old_config_path = codex_config_path(&handle).expect("old config path");
    let old_auth_path = codex_auth_path(&handle).expect("old auth path");

    let enabled = set_enabled(&handle, "codex", true, base_origin).expect("enable codex");
    assert!(enabled.ok, "{enabled:?}");

    let old_proxy_config_before_rebind =
        std::fs::read_to_string(&old_config_path).expect("read old proxy config");
    let old_proxy_auth_before_rebind =
        std::fs::read_to_string(&old_auth_path).expect("read old proxy auth");

    let root = cli_proxy_root_dir(&handle, "codex").expect("codex root");
    let files_dir = cli_proxy_files_dir(&root);
    let config_backup_before_rebind =
        std::fs::read_to_string(files_dir.join("config.toml")).expect("read config backup");
    let auth_backup_before_rebind =
        std::fs::read_to_string(files_dir.join("auth.json")).expect("read auth backup");

    set_custom_codex_home(&handle, &new_codex_home);
    write_codex_proxy_files(&handle, base_origin);
    let new_config_path = codex_config_path(&handle).expect("new config path");
    let new_auth_path = codex_auth_path(&handle).expect("new auth path");

    let rebound = rebind_codex_home_after_change(&handle, base_origin, true).expect("rebind");
    assert!(rebound.ok, "{rebound:?}");
    assert_eq!(rebound.message, "已重绑 Codex 目录并写入当前网关配置");

    let manifest = read_manifest(&handle, "codex")
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(manifest.base_origin.as_deref(), Some(base_origin));
    assert_eq!(
        PathBuf::from(&manifest_entry(&manifest, "codex_config_toml").path),
        new_config_path
    );
    assert_eq!(
        PathBuf::from(&manifest_entry(&manifest, "codex_auth_json").path),
        new_auth_path
    );

    let config_backup_after_rebind =
        std::fs::read_to_string(files_dir.join("config.toml")).expect("read config backup");
    let auth_backup_after_rebind =
        std::fs::read_to_string(files_dir.join("auth.json")).expect("read auth backup");
    assert_eq!(
        config_backup_after_rebind, config_backup_before_rebind,
        "adopting an existing proxy target must keep the original direct config backup"
    );
    assert_eq!(
        auth_backup_after_rebind, auth_backup_before_rebind,
        "adopting an existing proxy target must keep the original direct auth backup"
    );

    let disabled = set_enabled(&handle, "codex", false, base_origin).expect("disable codex");
    assert!(disabled.ok, "{disabled:?}");

    let old_config_after_disable =
        std::fs::read_to_string(&old_config_path).expect("read old config after disable");
    let old_auth_after_disable =
        std::fs::read_to_string(&old_auth_path).expect("read old auth after disable");
    let new_config_after_disable =
        std::fs::read_to_string(&new_config_path).expect("read new config after disable");
    let new_auth_after_disable =
        std::fs::read_to_string(&new_auth_path).expect("read new auth after disable");
    let new_auth_json: serde_json::Value =
        serde_json::from_str(&new_auth_after_disable).expect("parse new auth after disable");

    assert_eq!(
        old_config_after_disable, old_proxy_config_before_rebind,
        "old codex_home config should remain untouched after adopt + disable"
    );
    assert_eq!(
        old_auth_after_disable, old_proxy_auth_before_rebind,
        "old codex_home auth should remain untouched after adopt + disable"
    );
    assert!(
        new_config_after_disable.contains("[old_section]"),
        "new codex_home config should restore the original direct baseline: {new_config_after_disable}"
    );
    assert!(
        !new_config_after_disable.contains("model_provider = \"aio\""),
        "new codex_home config should no longer point to proxy after disable: {new_config_after_disable}"
    );
    assert_eq!(
        new_auth_json
            .get("profile")
            .and_then(|value| value.as_str()),
        Some("old"),
        "new codex_home auth should restore the original direct baseline: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("tokens").is_some(),
        "new codex_home auth should restore direct tokens: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("OPENAI_API_KEY").is_none(),
        "new codex_home auth should remove proxy API key after disable: {new_auth_after_disable}"
    );
    assert!(
        new_auth_json.get("auth_mode").is_none(),
        "new codex_home auth should remove proxy auth mode after disable: {new_auth_after_disable}"
    );
}

#[test]
fn claude_proxy_settings_json_rejects_invalid_json() {
    let input = br#"{"env": "#.to_vec();
    let err = build_claude_settings_json(Some(input), "http://127.0.0.1:1717/claude")
        .expect_err("must fail");
    assert!(err.to_string().contains("CLI_PROXY_INVALID_SETTINGS_JSON"));
}

// ── merge-restore tests ─────────────────────────────────────────────────────

fn write_temp(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn merge_restore_claude_preserves_user_changes() {
    let tmp = tempfile::tempdir().unwrap();

    // Backup: original file before proxy was enabled
    let backup = write_temp(
        tmp.path(),
        "backup.json",
        br#"{ "model": "opus", "permissions": { "allow": ["Read"] } }"#,
    );

    // Current: user added "language" and proxy injected env keys
    let target = write_temp(
        tmp.path(),
        "settings.json",
        br#"{
  "model": "opus",
  "language": "zh-CN",
  "permissions": { "allow": ["Read", "Write"] },
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:37123/claude",
    "ANTHROPIC_AUTH_TOKEN": "aio-coding-hub",
    "MCP_TIMEOUT": "30000"
  }
}"#,
    );

    merge_restore_claude_settings_json(&target, &backup).unwrap();

    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();

    // User's added "language" is preserved
    assert_eq!(result.get("language").unwrap().as_str(), Some("zh-CN"));
    // User's updated permissions are preserved
    let allow = result["permissions"]["allow"].as_array().unwrap();
    assert_eq!(allow.len(), 2);
    // Proxy keys are removed (backup didn't have them)
    let env = result.get("env").unwrap().as_object().unwrap();
    assert!(!env.contains_key("ANTHROPIC_BASE_URL"));
    assert!(!env.contains_key("ANTHROPIC_AUTH_TOKEN"));
    // User's other env keys are preserved
    assert_eq!(env.get("MCP_TIMEOUT").unwrap().as_str(), Some("30000"));
}

#[test]
fn merge_restore_claude_restores_original_env_keys() {
    let tmp = tempfile::tempdir().unwrap();

    // Backup: had original ANTHROPIC_BASE_URL
    let backup = write_temp(
        tmp.path(),
        "backup.json",
        br#"{ "env": { "ANTHROPIC_BASE_URL": "https://api.anthropic.com" } }"#,
    );

    // Current: proxy replaced the URL
    let target = write_temp(
        tmp.path(),
        "settings.json",
        br#"{ "env": { "ANTHROPIC_BASE_URL": "http://127.0.0.1:37123/claude", "ANTHROPIC_AUTH_TOKEN": "aio" } }"#,
    );

    merge_restore_claude_settings_json(&target, &backup).unwrap();

    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    let env = result.get("env").unwrap().as_object().unwrap();
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").unwrap().as_str(),
        Some("https://api.anthropic.com")
    );
    assert!(!env.contains_key("ANTHROPIC_AUTH_TOKEN"));
}

#[test]
fn merge_restore_codex_auth_preserves_user_changes() {
    let tmp = tempfile::tempdir().unwrap();

    // Backup: had OAuth tokens
    let backup = write_temp(
        tmp.path(),
        "backup.json",
        br#"{ "tokens": { "access": "tok-123" }, "last_refresh": 1234, "custom_key": "keep" }"#,
    );

    // Current: proxy replaced auth, user added a new key
    let target = write_temp(
        tmp.path(),
        "auth.json",
        br#"{ "OPENAI_API_KEY": "aio-coding-hub", "auth_mode": "apikey", "user_added": "hello" }"#,
    );

    merge_restore_codex_auth_json(&target, &backup).unwrap();

    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    // Proxy keys removed
    assert!(result.get("OPENAI_API_KEY").is_none());
    assert!(result.get("auth_mode").is_none());
    // OAuth tokens restored from backup
    assert!(result.get("tokens").is_some());
    assert!(result.get("last_refresh").is_some());
    // User's addition preserved
    assert_eq!(result.get("user_added").unwrap().as_str(), Some("hello"));
}

#[test]
fn merge_restore_gemini_env_preserves_user_changes() {
    let tmp = tempfile::tempdir().unwrap();

    // Backup: had original API key
    let backup = write_temp(
        tmp.path(),
        "backup.env",
        b"GEMINI_API_KEY=original-key\nCUSTOM_VAR=keep\n",
    );

    // Current: proxy replaced keys, user added new var
    let target = write_temp(
        tmp.path(),
        ".env",
        b"GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:37123/gemini\nGEMINI_API_KEY=aio-coding-hub\nUSER_VAR=hello\n",
    );

    merge_restore_gemini_env(&target, &backup).unwrap();

    let result = std::fs::read_to_string(&target).unwrap();
    // Proxy base URL removed (backup didn't have it)
    assert!(!result.contains("GOOGLE_GEMINI_BASE_URL"));
    // Original API key restored
    assert!(result.contains("GEMINI_API_KEY=original-key"));
    // User's addition preserved
    assert!(result.contains("USER_VAR=hello"));
}

#[test]
fn merge_restore_codex_config_preserves_user_changes() {
    let tmp = tempfile::tempdir().unwrap();

    // Backup: no proxy config, just user settings
    let backup = write_temp(
        tmp.path(),
        "backup.toml",
        b"[model_providers.openai]\nname = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\n",
    );

    // Current: proxy added its config, user added a new section
    let target = write_temp(
        tmp.path(),
        "config.toml",
        b"model_provider = \"aio\"\npreferred_auth_method = \"apikey\"\n\n[model_providers.openai]\nname = \"openai\"\nbase_url = \"https://api.openai.com/v1\"\n\n[model_providers.aio]\nname = \"aio\"\nbase_url = \"http://127.0.0.1:37123/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n\n[user_section]\nfoo = \"bar\"\n\n[windows]\nsandbox = \"elevated\"\n",
    );

    merge_restore_codex_config_toml(&target, &backup).unwrap();

    let result = std::fs::read_to_string(&target).unwrap();
    // Proxy root keys removed (check for the root-level assignment, not table names)
    assert!(
        !result.contains("model_provider = \"aio\""),
        "model_provider root key should be removed: {result}"
    );
    assert!(
        !result.contains("preferred_auth_method"),
        "preferred_auth_method should be removed: {result}"
    );
    // Proxy provider section removed
    assert!(!result.contains("[model_providers.aio]"));
    // Proxy windows sandbox removed
    assert!(!result.contains("[windows]"));
    assert!(!result.contains("sandbox"));
    // User's openai section preserved
    assert!(result.contains("[model_providers.openai]"));
    assert!(result.contains("base_url = \"https://api.openai.com/v1\""));
    // User's custom section preserved
    assert!(result.contains("[user_section]"));
    assert!(result.contains("foo = \"bar\""));
}

/// Simulates the app lifecycle that causes the "修复" button to appear:
///
/// 1. Enable proxy → config files point to gateway
/// 2. `restore_enabled_keep_state` (exit cleanup) → config files restored to direct mode,
///    but manifest still has `enabled = true`
/// 3. `status_all` with the same gateway origin → `applied_to_current_gateway == false`
///    (this is the drifted state that shows the "修复" button)
/// 4. `sync_enabled` (the fix) → config files re-applied
/// 5. `status_all` → `applied_to_current_gateway == true` (drift resolved)
#[test]
fn sync_enabled_resolves_drift_after_restore_enabled_keep_state() {
    let app = CliProxyTestApp::new();
    let handle = app.handle();
    let base_origin = "http://127.0.0.1:37123";

    // Step 1: Enable the codex proxy so config files point to the gateway.
    let enable_result = set_enabled(&handle, "codex", true, base_origin).expect("enable");
    assert!(enable_result.ok, "enable should succeed: {enable_result:?}");

    // Verify proxy is applied.
    let rows = status_all(&handle, Some(base_origin)).expect("status_all after enable");
    let codex = rows.iter().find(|r| r.cli_key == "codex").expect("codex");
    assert_eq!(codex.applied_to_current_gateway, Some(true));

    // Step 2: Simulate exit cleanup — restores config to direct mode, keeps enabled=true.
    let restore_results = restore_enabled_keep_state(&handle).expect("restore");
    let codex_restore = restore_results
        .iter()
        .find(|r| r.cli_key == "codex")
        .expect("codex restore");
    assert!(
        codex_restore.ok,
        "restore should succeed: {codex_restore:?}"
    );

    // Step 3: After restore, status_all should report drift (the bug scenario).
    let rows = status_all(&handle, Some(base_origin)).expect("status_all after restore");
    let codex = rows.iter().find(|r| r.cli_key == "codex").expect("codex");
    assert!(codex.enabled, "manifest should still be enabled");
    assert_eq!(
        codex.applied_to_current_gateway,
        Some(false),
        "config was restored to direct mode, so drift is expected"
    );

    // Step 4: sync_enabled (the fix applied at autostart) should re-apply proxy config.
    let sync_results = sync_enabled(&handle, base_origin, true).expect("sync");
    let codex_sync = sync_results
        .iter()
        .find(|r| r.cli_key == "codex")
        .expect("codex sync");
    assert!(codex_sync.ok, "sync should succeed: {codex_sync:?}");

    // Step 5: Drift should now be resolved.
    let rows = status_all(&handle, Some(base_origin)).expect("status_all after sync");
    let codex = rows.iter().find(|r| r.cli_key == "codex").expect("codex");
    assert!(codex.enabled);
    assert_eq!(
        codex.applied_to_current_gateway,
        Some(true),
        "sync_enabled should resolve the drift"
    );
}
