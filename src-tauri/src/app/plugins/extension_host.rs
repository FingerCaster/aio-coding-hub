//! Usage: Parent-side extension host worker lifecycle and command dispatch.

use super::extension_host_process::{
    ExtensionHostChildProcess, ExtensionHostMethodHandler, ExtensionHostProcessConfig,
};
use super::extension_host_worker::{
    default_extension_host_max_line_bytes, ExtensionHostWorkerConfig,
};
use super::privacy_redaction_service::PrivacyRedactionService;
use crate::db;
use crate::infra::plugins::{repository, runtime_reports};
use crate::plugins::PluginManifest;
use crate::shared::error::{AppError, AppResult};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_EXTENSION_HOST_START_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_EXTENSION_HOST_CALL_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_EXTENSION_HOST_IDLE_RECYCLE: Duration = Duration::from_secs(30);
const PLUGIN_STORAGE_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct ExtensionHostInstance {
    manifest: PluginManifest,
    runtime: ExtensionHostChildProcess,
    startup_timeout: Duration,
    _config_file: ExtensionHostConfigFile,
}

impl ExtensionHostInstance {
    #[allow(dead_code)]
    pub(crate) async fn start(manifest: PluginManifest, plugin_root: PathBuf) -> AppResult<Self> {
        Self::start_with_timeout(manifest, plugin_root, DEFAULT_EXTENSION_HOST_CALL_TIMEOUT).await
    }

    #[cfg(test)]
    pub(crate) async fn start_with_host_api(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        db: db::Db,
    ) -> AppResult<Self> {
        Self::start_with_host_api_and_privacy_redaction(
            manifest,
            plugin_root,
            db,
            Arc::new(PrivacyRedactionService::default()),
        )
        .await
    }

    #[allow(dead_code)]
    pub(crate) async fn start_with_host_api_and_privacy_redaction(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        db: db::Db,
        privacy_redaction: Arc<PrivacyRedactionService>,
    ) -> AppResult<Self> {
        Self::start_with_host_api_and_privacy_redaction_with_timeout(
            manifest,
            plugin_root,
            db,
            privacy_redaction,
            DEFAULT_EXTENSION_HOST_CALL_TIMEOUT,
        )
        .await
    }

    pub(crate) async fn start_with_host_api_and_privacy_redaction_with_timeout(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        db: db::Db,
        privacy_redaction: Arc<PrivacyRedactionService>,
        call_timeout: Duration,
    ) -> AppResult<Self> {
        let handler_plugin_root = plugin_root.clone();
        Self::start_with_timeout_and_host_handler(
            manifest.clone(),
            plugin_root,
            call_timeout,
            Some(Arc::new(ExtensionHostApiHandler {
                db,
                plugin_id: manifest.id,
                plugin_root: handler_plugin_root,
                capabilities: manifest.capabilities.into_iter().collect(),
                privacy_redaction,
            })),
        )
        .await
    }

    #[allow(dead_code)]
    pub(crate) async fn start_with_timeout(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        call_timeout: Duration,
    ) -> AppResult<Self> {
        Self::start_with_timeout_and_host_handler(manifest, plugin_root, call_timeout, None).await
    }

    async fn start_with_timeout_and_host_handler(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        call_timeout: Duration,
        host_handler: Option<Arc<dyn ExtensionHostMethodHandler>>,
    ) -> AppResult<Self> {
        let current_exe = std::env::current_exe().map_err(|err| {
            AppError::new(
                "PLUGIN_EXTENSION_HOST_EXE_UNAVAILABLE",
                format!("failed to resolve current executable: {err}"),
            )
        })?;
        Self::start_with_program(
            manifest,
            plugin_root,
            current_exe,
            call_timeout,
            host_handler,
        )
        .await
    }

    async fn start_with_program(
        manifest: PluginManifest,
        plugin_root: PathBuf,
        program: PathBuf,
        call_timeout: Duration,
        host_handler: Option<Arc<dyn ExtensionHostMethodHandler>>,
    ) -> AppResult<Self> {
        let contribution_hash = contribution_hash(&manifest);
        let startup_timeout = DEFAULT_EXTENSION_HOST_START_TIMEOUT;
        let config_file = write_worker_config(
            &plugin_root,
            startup_timeout,
            call_timeout,
            contribution_hash.clone(),
        )?;
        #[cfg(not(test))]
        let args = vec![
            "--extension-host-worker".to_string(),
            "--extension-host-config".to_string(),
            config_file.path().display().to_string(),
        ];
        #[cfg(test)]
        let args = vec![
            "--exact".to_string(),
            "app::plugins::extension_host_worker::extension_host_worker_process_entry_for_tests"
                .to_string(),
            "--nocapture".to_string(),
            "--".to_string(),
            "--extension-host-config".to_string(),
            config_file.path().display().to_string(),
        ];
        let max_line_bytes = default_extension_host_max_line_bytes();
        let runtime = ExtensionHostChildProcess::start(ExtensionHostProcessConfig {
            program: program.display().to_string(),
            args,
            start_timeout: DEFAULT_EXTENSION_HOST_START_TIMEOUT,
            hook_timeout: call_timeout,
            idle_recycle: DEFAULT_EXTENSION_HOST_IDLE_RECYCLE,
            max_line_bytes,
            ready_method: "extension.ready".to_string(),
            allow_startup_noise: cfg!(test),
            host_handler,
        })
        .await
        .map_err(map_extension_host_process_error)?;

        let mut host = Self {
            manifest,
            runtime,
            startup_timeout,
            _config_file: config_file,
        };
        host.handshake().await?;
        Ok(host)
    }

    async fn handshake(&mut self) -> AppResult<()> {
        self.runtime
            .call_method_with_timeout(
                "extension.handshake",
                json!({
                    "pluginId": self.manifest.id,
                    "version": self.manifest.version,
                    "apiVersion": self.manifest.api_version,
                    "contributionHash": contribution_hash(&self.manifest),
                }),
                self.startup_timeout,
            )
            .await
            .map(|_| ())
            .map_err(map_extension_host_process_error)
    }

    #[allow(dead_code)]
    pub(crate) async fn activate(&mut self) -> AppResult<()> {
        self.runtime
            .call_method_with_timeout("extension.activate", Value::Null, self.startup_timeout)
            .await
            .map(|_| ())
            .map_err(map_extension_host_process_error)
    }

    #[allow(dead_code)]
    pub(crate) async fn execute_command(&mut self, command: &str, args: Value) -> AppResult<Value> {
        if !self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == "commands.execute")
        {
            return Err(AppError::new(
                "PLUGIN_EXTENSION_HOST_FORBIDDEN",
                "extension host API requires commands.execute",
            ));
        }
        self.activate().await?;
        self.runtime
            .call_method(
                "commands.execute",
                json!({
                    "command": command,
                    "args": args,
                }),
            )
            .await
            .map_err(map_extension_host_process_error)
    }

    pub(crate) async fn execute_gateway_hook(
        &mut self,
        hook: &str,
        context: Value,
    ) -> AppResult<Value> {
        if !self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability == "gateway.hooks")
        {
            return Err(AppError::new(
                "PLUGIN_EXTENSION_HOST_FORBIDDEN",
                "extension host API requires gateway.hooks",
            ));
        }
        self.activate().await?;
        self.runtime
            .call_method(
                "gatewayHooks.execute",
                json!({
                    "hook": hook,
                    "context": context,
                }),
            )
            .await
            .map_err(map_extension_host_process_error)
    }

    #[cfg(test)]
    async fn execute_command_rpc_for_tests(
        &mut self,
        command: &str,
        args: Value,
    ) -> AppResult<Value> {
        self.runtime
            .call_method(
                "commands.execute",
                json!({
                    "command": command,
                    "args": args,
                }),
            )
            .await
            .map_err(map_extension_host_process_error)
    }

    #[allow(dead_code)]
    pub(crate) fn is_running(&mut self) -> bool {
        self.runtime.is_running()
    }

    #[allow(dead_code)]
    pub(crate) async fn dispose(&mut self) {
        let _ = self
            .runtime
            .call_method_with_timeout("extension.deactivate", Value::Null, self.startup_timeout)
            .await;
        self.runtime.shutdown().await;
    }

    #[cfg(test)]
    async fn start_for_tests(plugin_root: &Path) -> AppResult<Self> {
        let manifest = read_manifest(plugin_root)?;
        Self::start_for_tests_with_manifest(
            manifest,
            plugin_root,
            DEFAULT_EXTENSION_HOST_CALL_TIMEOUT,
        )
        .await
    }

    #[cfg(test)]
    async fn start_for_tests_with_timeout(
        plugin_root: &Path,
        call_timeout: Duration,
    ) -> AppResult<Self> {
        let manifest = read_manifest(plugin_root)?;
        Self::start_for_tests_with_manifest(manifest, plugin_root, call_timeout).await
    }

    #[cfg(test)]
    async fn start_for_tests_with_manifest(
        manifest: PluginManifest,
        plugin_root: &Path,
        call_timeout: Duration,
    ) -> AppResult<Self> {
        let program = std::env::current_exe().map_err(|err| {
            AppError::new(
                "PLUGIN_EXTENSION_HOST_EXE_UNAVAILABLE",
                format!("failed to resolve current test executable: {err}"),
            )
        })?;
        Self::start_with_program(
            manifest,
            plugin_root.to_path_buf(),
            program,
            call_timeout,
            None,
        )
        .await
    }
}

#[allow(dead_code)]
pub(crate) type ExtensionHost = ExtensionHostInstance;

struct ExtensionHostApiHandler {
    db: db::Db,
    plugin_id: String,
    plugin_root: PathBuf,
    capabilities: BTreeSet<String>,
    privacy_redaction: Arc<PrivacyRedactionService>,
}

impl ExtensionHostMethodHandler for ExtensionHostApiHandler {
    fn handle_host_method(&self, method: &str, params: Value) -> AppResult<Value> {
        match method {
            "storage.get" => self.storage_get(params),
            "storage.set" => self.storage_set(params),
            "diagnostics.getRuntimeReports" => self.diagnostics_get_runtime_reports(params),
            "privacy.redactText" => self.privacy_redact_text(params),
            "privacy.redactRequestBody" => self.privacy_redact_request_body(params),
            other => Err(AppError::new(
                "PLUGIN_EXTENSION_HOST_METHOD_NOT_FOUND",
                format!("unsupported extension host API method: {other}"),
            )),
        }
    }
}

impl ExtensionHostApiHandler {
    fn require_capability(&self, capability: &str) -> AppResult<()> {
        if self.capabilities.contains(capability) {
            return Ok(());
        }
        Err(AppError::new(
            "PLUGIN_EXTENSION_HOST_FORBIDDEN",
            format!("extension host API requires {capability}"),
        ))
    }

    fn storage_get(&self, params: Value) -> AppResult<Value> {
        self.require_capability("storage.plugin")?;
        let plugin_id = self.host_api_plugin_id(&params)?;
        let key = required_string(&params, "key")?;
        let detail = repository::get_plugin(&self.db, plugin_id)?;
        Ok(detail
            .config
            .get("storage")
            .and_then(Value::as_object)
            .and_then(|storage| storage.get(key))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn storage_set(&self, params: Value) -> AppResult<Value> {
        self.require_capability("storage.plugin")?;
        let plugin_id = self.host_api_plugin_id(&params)?.to_string();
        let key = required_string(&params, "key")?.to_string();
        let value = params.get("value").cloned().unwrap_or(Value::Null);
        let detail = repository::get_plugin(&self.db, &plugin_id)?;
        let mut config = detail.config;
        if !config.is_object() {
            config = json!({});
        }
        let object = config.as_object_mut().ok_or_else(|| {
            AppError::new(
                "PLUGIN_STORAGE_INVALID",
                "plugin config storage root must be an object",
            )
        })?;
        let storage_value = object
            .entry("storage".to_string())
            .or_insert_with(|| json!({}));
        if !storage_value.is_object() {
            *storage_value = json!({});
        }
        storage_value
            .as_object_mut()
            .expect("storage object")
            .insert(key, value);
        let storage_bytes = serde_json::to_vec(storage_value).map_err(|err| {
            AppError::new(
                "PLUGIN_STORAGE_INVALID",
                format!("failed to encode plugin storage: {err}"),
            )
        })?;
        if storage_bytes.len() > PLUGIN_STORAGE_MAX_BYTES {
            return Err(AppError::new(
                "PLUGIN_STORAGE_LIMIT_EXCEEDED",
                "plugin storage exceeded 64 KiB",
            ));
        }
        let config_version = detail.manifest.config_version.unwrap_or(1);
        repository::save_plugin_config(&self.db, &plugin_id, config_version, &config, &[])?;
        Ok(json!({ "ok": true }))
    }

    fn diagnostics_get_runtime_reports(&self, params: Value) -> AppResult<Value> {
        self.require_capability("diagnostics.read")?;
        let plugin_id = self.host_api_plugin_id(&params)?;
        let limit = params
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 100) as usize;
        let reports = runtime_reports::list_extension_execution_reports(
            &self.db,
            Some(plugin_id),
            None,
            None,
            None,
            limit,
        )?;
        serde_json::to_value(reports).map_err(|err| {
            AppError::new(
                "PLUGIN_DIAGNOSTICS_ENCODE_FAILED",
                format!("failed to encode runtime reports: {err}"),
            )
        })
    }

    fn privacy_redact_text(&self, params: Value) -> AppResult<Value> {
        self.require_capability("privacy.redact")?;
        let plugin_id = self.host_api_plugin_id(&params)?;
        let text = required_string(&params, "text")?;
        let options = params.get("options").cloned().unwrap_or_else(|| json!({}));
        let plugin = self.plugin_detail_with_root(plugin_id)?;
        let output = self
            .privacy_redaction
            .redact_text(&plugin, text, &options)
            .map_err(|err| {
                AppError::new(
                    "PLUGIN_PRIVACY_REDACTION_FAILED",
                    format!("privacy redaction failed: {err}"),
                )
            })?;
        serde_json::to_value(output).map_err(|err| {
            AppError::new(
                "PLUGIN_PRIVACY_REDACTION_ENCODE_FAILED",
                format!("failed to encode privacy redaction result: {err}"),
            )
        })
    }

    fn privacy_redact_request_body(&self, params: Value) -> AppResult<Value> {
        self.require_capability("privacy.redact")?;
        let plugin_id = self.host_api_plugin_id(&params)?;
        let body = required_string(&params, "body")?;
        let options = params.get("options").cloned().unwrap_or_else(|| json!({}));
        let plugin = self.plugin_detail_with_root(plugin_id)?;
        let output = self
            .privacy_redaction
            .redact_request_body(&plugin, body, &options)
            .map_err(|err| {
                AppError::new(
                    "PLUGIN_PRIVACY_REDACTION_FAILED",
                    format!("privacy request body redaction failed: {err}"),
                )
            })?;
        serde_json::to_value(output).map_err(|err| {
            AppError::new(
                "PLUGIN_PRIVACY_REDACTION_ENCODE_FAILED",
                format!("failed to encode privacy redaction result: {err}"),
            )
        })
    }

    fn plugin_detail_with_root(&self, plugin_id: &str) -> AppResult<crate::plugins::PluginDetail> {
        let mut detail = repository::get_plugin(&self.db, plugin_id)?;
        detail.installed_dir = Some(self.plugin_root.to_string_lossy().to_string());
        Ok(detail)
    }

    fn host_api_plugin_id<'a>(&self, params: &'a Value) -> AppResult<&'a str> {
        let plugin_id = required_string(params, "pluginId")?;
        if plugin_id != self.plugin_id {
            return Err(AppError::new(
                "PLUGIN_EXTENSION_HOST_FORBIDDEN",
                "extension host API pluginId did not match owning plugin",
            ));
        }
        Ok(plugin_id)
    }
}

fn required_string<'a>(params: &'a Value, key: &str) -> AppResult<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(
                "PLUGIN_EXTENSION_HOST_INVALID_REQUEST",
                format!("extension host API requires {key}"),
            )
        })
}

#[derive(Debug)]
struct ExtensionHostConfigFile {
    path: PathBuf,
}

impl ExtensionHostConfigFile {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ExtensionHostConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_worker_config(
    plugin_root: &Path,
    startup_timeout: Duration,
    call_timeout: Duration,
    contribution_hash: String,
) -> AppResult<ExtensionHostConfigFile> {
    let config = ExtensionHostWorkerConfig {
        plugin_root: plugin_root.to_path_buf(),
        contribution_hash: Some(contribution_hash),
        max_line_bytes: default_extension_host_max_line_bytes(),
        startup_js_timeout_ms: startup_timeout.as_millis().try_into().unwrap_or(u64::MAX),
        js_timeout_ms: call_timeout.as_millis().try_into().unwrap_or(u64::MAX),
    };
    let mut nonce = [0_u8; 8];
    rand::thread_rng().fill_bytes(&mut nonce);
    let path = std::env::temp_dir().join(format!(
        "aio-extension-host-{}-{:016x}.json",
        std::process::id(),
        u64::from_le_bytes(nonce)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|err| {
            AppError::new(
                "PLUGIN_EXTENSION_HOST_CONFIG_CREATE_FAILED",
                format!("failed to create extension host config file: {err}"),
            )
        })?;
    let bytes = serde_json::to_vec(&config).map_err(|err| {
        AppError::new(
            "PLUGIN_EXTENSION_HOST_CONFIG_ENCODE_FAILED",
            format!("failed to encode extension host config: {err}"),
        )
    })?;
    file.write_all(&bytes).map_err(|err| {
        AppError::new(
            "PLUGIN_EXTENSION_HOST_CONFIG_WRITE_FAILED",
            format!("failed to write extension host config: {err}"),
        )
    })?;
    file.flush().map_err(|err| {
        AppError::new(
            "PLUGIN_EXTENSION_HOST_CONFIG_WRITE_FAILED",
            format!("failed to flush extension host config: {err}"),
        )
    })?;
    Ok(ExtensionHostConfigFile { path })
}

fn contribution_hash(manifest: &PluginManifest) -> String {
    let bytes = serde_json::to_vec(&json!({
        "runtime": manifest.runtime,
        "main": manifest.main,
        "activationEvents": manifest.activation_events,
        "contributes": manifest.contributes,
        "capabilities": manifest.capabilities,
        "permissions": manifest.permissions,
    }))
    .unwrap_or_default();
    format!("{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(test)]
fn read_manifest(plugin_root: &Path) -> AppResult<PluginManifest> {
    let path = plugin_root.join("plugin.json");
    let bytes = std::fs::read(&path).map_err(|err| {
        AppError::new(
            "PLUGIN_EXTENSION_HOST_MANIFEST_READ_FAILED",
            format!(
                "failed to read extension host manifest {}: {err}",
                path.display()
            ),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        AppError::new(
            "PLUGIN_EXTENSION_HOST_MANIFEST_DECODE_FAILED",
            format!(
                "failed to decode extension host manifest {}: {err}",
                path.display()
            ),
        )
    })
}

fn map_extension_host_process_error(err: AppError) -> AppError {
    match err.code() {
        "PLUGIN_EXTENSION_HOST_PROCESS_HOOK_TIMEOUT" => AppError::new(
            "PLUGIN_EXTENSION_CALL_TIMEOUT",
            "extension host call timed out",
        ),
        "PLUGIN_EXTENSION_HOST_TIMEOUT" => AppError::new(
            "PLUGIN_EXTENSION_CALL_TIMEOUT",
            "extension host call timed out",
        ),
        "PLUGIN_EXTENSION_HOST_PROCESS_START_TIMEOUT" => AppError::new(
            "PLUGIN_EXTENSION_START_TIMEOUT",
            "extension host worker did not become ready before startup timeout",
        ),
        "PLUGIN_EXTENSION_HOST_PROCESS_REQUEST_TOO_LARGE" => AppError::new(
            "PLUGIN_EXTENSION_REQUEST_TOO_LARGE",
            "extension host request exceeded max line bytes",
        ),
        "PLUGIN_EXTENSION_HOST_PROCESS_RESPONSE_TOO_LARGE" => AppError::new(
            "PLUGIN_EXTENSION_RESPONSE_TOO_LARGE",
            "extension host response exceeded max line bytes",
        ),
        _ => err,
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::plugins::{PluginInstallSource, PluginManifest, PluginStatus};
    use crate::infra::plugins::repository::{self, InsertPluginInput};
    use serde_json::json;
    use std::path::Path;
    use std::time::Duration;

    fn write_extension_plugin(root: &Path, extension_js: &str) {
        write_extension_plugin_with_capabilities(root, extension_js, &["commands.execute"]);
    }

    fn write_extension_plugin_with_capabilities(
        root: &Path,
        extension_js: &str,
        capabilities: &[&str],
    ) {
        write_extension_plugin_with_manifest_overrides(
            root,
            extension_js,
            capabilities,
            serde_json::json!({
                "commands": [
                    { "command": "acme.echo", "title": "Echo" },
                    { "command": "acme.never", "title": "Never" }
                ]
            }),
            vec!["onCommand:acme.echo", "onCommand:acme.never"],
        );
    }

    fn write_gateway_extension_plugin(root: &Path, extension_js: &str, hook_name: &str) {
        write_extension_plugin_with_manifest_overrides(
            root,
            extension_js,
            &["gateway.hooks"],
            serde_json::json!({
                "gatewayHooks": [
                    { "name": hook_name, "priority": 10, "failurePolicy": "fail-open" }
                ]
            }),
            Vec::new(),
        );
    }

    fn write_extension_plugin_with_manifest_overrides(
        root: &Path,
        extension_js: &str,
        capabilities: &[&str],
        contributes: serde_json::Value,
        activation_events: Vec<&str>,
    ) {
        std::fs::create_dir_all(root.join("dist")).expect("create dist");
        let manifest = json!({
            "id": "acme.echo",
            "name": "Acme Echo",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "main": "dist/extension.js",
            "activationEvents": activation_events,
            "contributes": contributes,
            "capabilities": capabilities,
            "hostCompatibility": { "app": ">=0.60.0", "pluginApi": "^1.0.0" }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .expect("write plugin.json");
        std::fs::write(root.join("dist/extension.js"), extension_js).expect("write extension.js");
    }

    fn install_extension_plugin(db: &crate::db::Db, root: &Path) -> PluginManifest {
        let manifest = super::read_manifest(root).expect("manifest");
        repository::insert_plugin(
            db,
            InsertPluginInput {
                manifest: manifest.clone(),
                install_source: PluginInstallSource::Local,
                status: PluginStatus::Enabled,
                installed_dir: Some(root.to_string_lossy().to_string()),
            },
        )
        .expect("insert plugin");
        manifest
    }

    fn init_test_db(root: &Path) -> crate::db::Db {
        crate::db::init_for_tests(&root.join("plugins.db")).expect("init db")
    }

    #[tokio::test]
    async fn extension_host_activates_and_dispatches_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function(args) {
                return { ok: true, echo: args.text };
              });
            };
            "#,
        );

        let mut host = super::ExtensionHost::start_for_tests(temp.path())
            .await
            .expect("start extension host");

        let result = host
            .execute_command("acme.echo", json!({ "text": "hello" }))
            .await
            .expect("execute command");

        assert_eq!(result, json!({ "ok": true, "echo": "hello" }));
        assert!(host.is_running());
        host.dispose().await;
        assert!(!host.is_running());
    }

    #[tokio::test]
    async fn extension_host_executes_gateway_hook() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.gateway.registerHook("gateway.request.afterBodyRead", function(context) {
                return {
                  action: "replace",
                  requestBody: context.request.body.replace("hello", "hi")
                };
              });
            };
            "#,
            "gateway.request.afterBodyRead",
        );

        let mut host = super::ExtensionHost::start_for_tests(temp.path())
            .await
            .expect("start extension host");

        let result = host
            .execute_gateway_hook(
                "gateway.request.afterBodyRead",
                json!({
                    "hookName": "gateway.request.afterBodyRead",
                    "traceId": "trace-gateway",
                    "request": { "body": "hello" },
                    "response": {},
                    "stream": {},
                    "log": {}
                }),
            )
            .await
            .expect("execute gateway hook");

        assert_eq!(result, json!({ "action": "replace", "requestBody": "hi" }));
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_gateway_hook_registration_requires_manifest_declaration() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.gateway.registerHook("gateway.response.after", function() {
                return { action: "continue" };
              });
            };
            "#,
            "gateway.request.afterBodyRead",
        );

        let mut host = super::ExtensionHost::start_for_tests(temp.path())
            .await
            .expect("start extension host");

        let err = host
            .execute_gateway_hook("gateway.request.afterBodyRead", json!({}))
            .await
            .expect_err("undeclared gateway hook registration should fail activation");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_UNDECLARED_GATEWAY_HOOK");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_gateway_hook_call_timeout_does_not_cover_activation() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              const start = Date.now();
              while (Date.now() - start < 100) {}
              api.gateway.registerHook("gateway.request.afterBodyRead", function() {
                return { action: "continue" };
              });
            };
            "#,
            "gateway.request.afterBodyRead",
        );

        let mut host = super::ExtensionHost::start_for_tests_with_timeout(
            temp.path(),
            Duration::from_millis(50),
        )
        .await
        .expect("start extension host");

        let result = host
            .execute_gateway_hook("gateway.request.afterBodyRead", json!({}))
            .await
            .expect("activation should use startup budget, not hook invocation budget");

        assert_eq!(result, json!({ "action": "continue" }));
        assert!(host.is_running());
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_gateway_hook_call_timeout_does_not_cover_module_load() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            r#"
            const start = Date.now();
            while (Date.now() - start < 100) {}
            module.exports.activate = function(api) {
              api.gateway.registerHook("gateway.request.afterBodyRead", function() {
                return { action: "continue" };
              });
            };
            "#,
            "gateway.request.afterBodyRead",
        );

        let mut host = super::ExtensionHost::start_for_tests_with_timeout(
            temp.path(),
            Duration::from_millis(50),
        )
        .await
        .expect("module load should use startup budget, not hook invocation budget");

        let result = host
            .execute_gateway_hook("gateway.request.afterBodyRead", json!({}))
            .await
            .expect("execute gateway hook");

        assert_eq!(result, json!({ "action": "continue" }));
        assert!(host.is_running());
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_gateway_hook_call_timeout_covers_handler_execution() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.gateway.registerHook("gateway.request.afterBodyRead", function() {
                while (true) {}
              });
            };
            "#,
            "gateway.request.afterBodyRead",
        );

        let mut host = super::ExtensionHost::start_for_tests_with_timeout(
            temp.path(),
            Duration::from_millis(50),
        )
        .await
        .expect("start extension host");

        let started = std::time::Instant::now();
        let err = host
            .execute_gateway_hook("gateway.request.afterBodyRead", json!({}))
            .await
            .expect_err("gateway hook handler should be inside hook timeout");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_CALL_TIMEOUT");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "gateway hook timeout should cover handler execution, elapsed {:?}",
            started.elapsed()
        );
        assert!(!host.is_running());
    }

    #[tokio::test]
    async fn extension_host_storage_api_allows_storage_plugin_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin_with_capabilities(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                api.storage.set("key", { ok: true });
                return api.storage.get("key");
              });
            };
            "#,
            &["commands.execute", "storage.plugin"],
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let result = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect("execute storage command");

        assert_eq!(result, json!({ "ok": true }));
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_storage_api_rejects_missing_storage_plugin_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return globalThis.__aioHostApi(
                  "storage.get",
                  { pluginId: "acme.echo", key: "key" }
                );
              });
            };
            "#,
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let err = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect_err("storage API without capability should fail");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_FORBIDDEN");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_diagnostics_api_allows_diagnostics_read_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin_with_capabilities(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return api.diagnostics.getRuntimeReports(5);
              });
            };
            "#,
            &["commands.execute", "diagnostics.read"],
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let result = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect("execute diagnostics command");

        assert!(result.as_array().is_some());
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_diagnostics_api_rejects_missing_diagnostics_read_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return globalThis.__aioHostApi(
                  "diagnostics.getRuntimeReports",
                  { pluginId: "acme.echo", limit: 5 }
                );
              });
            };
            "#,
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let err = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect_err("diagnostics API without capability should fail");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_FORBIDDEN");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_privacy_api_redacts_text_with_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("rules")).expect("rules dir");
        std::fs::write(temp.path().join("rules/gitleaks.toml"), "").expect("rules file");
        write_extension_plugin_with_capabilities(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return api.privacy.redactText("email a@example.com", { sensitiveTypes: ["email"] });
              });
            };
            "#,
            &["commands.execute", "privacy.redact"],
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let result = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect("execute privacy command");

        assert_eq!(
            result.get("hit").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            result.get("redacted").and_then(serde_json::Value::as_str),
            Some("email [邮箱]")
        );
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_privacy_api_rejects_missing_capability() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("rules")).expect("rules dir");
        std::fs::write(temp.path().join("rules/gitleaks.toml"), "").expect("rules file");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return globalThis.__aioHostApi(
                  "privacy.redactText",
                  { pluginId: "acme.echo", text: "email a@example.com", options: { sensitiveTypes: ["email"] } }
                );
              });
            };
            "#,
        );
        let db = init_test_db(temp.path());
        let manifest = install_extension_plugin(&db, temp.path());

        let mut host =
            super::ExtensionHost::start_with_host_api(manifest, temp.path().to_path_buf(), db)
                .await
                .expect("start extension host");

        let err = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect_err("privacy API without capability should fail");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_FORBIDDEN");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_command_dispatch_requires_commands_execute_before_activation() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin_with_capabilities(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function() {
                return { executed: true };
              });
            };
            "#,
            &[],
        );

        let mut host = super::ExtensionHost::start_for_tests(temp.path())
            .await
            .expect("start extension host");

        let err = host
            .execute_command("acme.echo", json!({}))
            .await
            .expect_err("missing commands capability should fail before activation");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_FORBIDDEN");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_worker_rpc_rejects_registry_mutation_without_commands_execute() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin_with_capabilities(
            temp.path(),
            r#"
            globalThis.__aioCommands["acme.echo"] = function() {
                return { executed: true };
            };
            module.exports.activate = function() {};
            "#,
            &[],
        );

        let mut host = super::ExtensionHost::start_for_tests(temp.path())
            .await
            .expect("start extension host");

        let err = host
            .execute_command_rpc_for_tests("acme.echo", json!({}))
            .await
            .expect_err("missing commands capability should fail before command execution");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_FORBIDDEN");
        host.dispose().await;
    }

    #[tokio::test]
    async fn extension_host_timeout_kills_worker() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.never", function() {
                while (true) {}
              });
            };
            "#,
        );

        let mut host = super::ExtensionHost::start_for_tests_with_timeout(
            temp.path(),
            Duration::from_millis(50),
        )
        .await
        .expect("start extension host");

        let err = host
            .execute_command("acme.never", json!({}))
            .await
            .expect_err("command timeout fails");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_CALL_TIMEOUT");
        assert!(!host.is_running());
    }

    #[tokio::test]
    async fn extension_host_rejects_manifest_contribution_hash_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_extension_plugin(
            temp.path(),
            r#"
            module.exports.activate = function(api) {
              api.commands.registerCommand("acme.echo", function(args) {
                return args;
              });
            };
            "#,
        );
        let mut manifest = super::read_manifest(temp.path()).expect("manifest");
        manifest.contributes = None;

        let err = super::ExtensionHost::start_for_tests_with_manifest(
            manifest,
            temp.path(),
            Duration::from_millis(50),
        )
        .await
        .expect_err("hash mismatch should fail handshake");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_PROCESS_CRASHED");
    }
}
