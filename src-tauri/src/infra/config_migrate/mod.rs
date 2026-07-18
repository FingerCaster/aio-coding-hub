//! Usage: Config export/import for machine migration.

mod export;
mod import;
mod rollback;
pub(crate) mod skill_fs;

#[cfg(test)]
mod tests;

use crate::resident;
use crate::shared::error::{db_err, AppResult};
use crate::{db, settings};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CONFIG_BUNDLE_SCHEMA_VERSION: u32 = 3;
pub const CONFIG_BUNDLE_SCHEMA_VERSION_V1: u32 = 1;
pub const CONFIG_BUNDLE_SCHEMA_VERSION_V2: u32 = 2;
pub(crate) const CONFIG_BUNDLE_FULL_SKILL_PAYLOAD_MIN_VERSION: u32 = 2;
pub(crate) const CONFIG_BUNDLE_ACCOUNT_USAGE_SNAPSHOT_MIN_VERSION: u32 = 3;
/// Shared encoded budget for config export serialization and import file reads.
pub(crate) const CONFIG_BUNDLE_ENCODED_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Compatibility alias for the shared encoded budget.
#[cfg(test)]
pub(crate) const CONFIG_IMPORT_FILE_MAX_BYTES: usize = CONFIG_BUNDLE_ENCODED_MAX_BYTES;
pub(crate) const CONFIG_SKILL_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const CONFIG_SKILL_FILE_MAX_BYTES: usize = CONFIG_SKILL_TOTAL_MAX_BYTES;
pub(crate) const CONFIG_SKILL_FILE_COUNT_MAX: usize = 256;
pub(crate) const CONFIG_SKILL_EXPORT_ENCODED_MAX_BYTES: usize = 56 * 1024 * 1024;
pub(crate) const CONFIG_SKILL_EXPORT_FILE_COUNT_MAX: usize = 2048;
pub(crate) const CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS: usize = 512;
pub(crate) const CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES: usize = 64 * 1024;
pub(crate) const CONFIG_SKILL_MD_MAX_BYTES: usize = 256 * 1024;
const SKILL_MANAGED_MARKER_FILE: &str = ".aio-coding-hub.managed";
const SKILL_SOURCE_MARKER_FILE: &str = ".aio-coding-hub.source.json";

fn default_empty_json_object() -> String {
    "{}".to_string()
}

fn default_oauth_refresh_lead_seconds() -> i64 {
    3600
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ConfigBundle {
    pub schema_version: u32,
    pub exported_at: String,
    pub app_version: String,
    pub settings: String,
    pub providers: Vec<ProviderExport>,
    pub sort_modes: Vec<SortModeExport>,
    pub sort_mode_active: HashMap<String, String>,
    pub workspaces: Vec<WorkspaceExport>,
    pub mcp_servers: Vec<McpServerExport>,
    pub skill_repos: Vec<SkillRepoExport>,
    #[serde(default)]
    pub installed_skills: Option<Vec<InstalledSkillExport>>,
    #[serde(default)]
    pub local_skills: Option<Vec<LocalSkillExport>>,
    // Image gen connection configs. None (older bundles) leaves the current
    // configs untouched on import; Some replaces them (providers posture).
    #[serde(default)]
    pub image_gen_configs: Option<Vec<ImageGenConfigExport>>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ProviderExport {
    pub id: Option<i64>,
    pub cli_key: String,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: String,
    pub api_key_plaintext: String,
    pub auth_mode: String,
    pub oauth_provider_type: Option<String>,
    pub oauth_access_token: Option<String>,
    pub oauth_refresh_token: Option<String>,
    #[serde(default)]
    pub oauth_id_token: Option<String>,
    pub oauth_token_expiry: Option<i64>,
    pub oauth_scopes: Option<String>,
    pub oauth_token_uri: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_email: Option<String>,
    #[serde(default = "default_oauth_refresh_lead_seconds")]
    pub oauth_refresh_lead_seconds: i64,
    #[serde(default)]
    pub oauth_last_refreshed_at: Option<i64>,
    #[serde(default)]
    pub oauth_last_error: Option<String>,
    pub claude_models_json: String,
    #[serde(default = "default_empty_json_object")]
    pub supported_models_json: String,
    #[serde(default = "default_empty_json_object")]
    pub model_mapping_json: String,
    pub enabled: bool,
    pub priority: i64,
    pub cost_multiplier: f64,
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub daily_reset_mode: String,
    pub daily_reset_time: String,
    pub tags_json: String,
    pub note: String,
    pub source_provider_id: Option<i64>,
    pub source_provider_cli_key: Option<String>,
    pub bridge_type: Option<String>,
    #[serde(default)]
    pub account_usage_config: Option<serde_json::Value>,
    #[serde(default)]
    pub account_usage_credentials: Option<ProviderAccountUsageCredentialsExport>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ProviderAccountUsageCredentialsExport {
    pub newapi_user_id: Option<String>,
    pub newapi_access_token_plaintext: Option<String>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct SortModeExport {
    pub name: String,
    pub is_default: bool,
    pub providers: Vec<SortModeProviderExport>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct SortModeProviderExport {
    pub cli_key: String,
    pub provider_cli_key: String,
    pub sort_order: i64,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct WorkspaceExport {
    pub cli_key: String,
    pub name: String,
    pub is_active: bool,
    #[serde(default)]
    pub prompts: Vec<PromptExport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptExport>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct PromptExport {
    pub name: String,
    pub content: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct McpServerExport {
    pub server_key: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args_json: String,
    pub env_json: String,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers_json: Option<String>,
    pub enabled_in_workspaces: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct ImageGenConfigExport {
    pub adapter_id: String,
    pub base_url: String,
    pub model: String,
    pub api_key_plaintext: String,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct SkillRepoExport {
    pub git_url: String,
    pub branch: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct InstalledSkillExport {
    pub skill_key: String,
    pub name: String,
    pub description: String,
    pub source_git_url: String,
    pub source_branch: String,
    pub source_subdir: String,
    pub enabled_in_workspaces: Vec<(String, String)>,
    pub files: Vec<SkillFileExport>,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct LocalSkillExport {
    pub cli_key: String,
    pub dir_name: String,
    pub name: String,
    pub description: String,
    pub source_git_url: Option<String>,
    pub source_branch: Option<String>,
    pub source_subdir: Option<String>,
    pub files: Vec<SkillFileExport>,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct SkillFileExport {
    pub relative_path: String,
    pub content_base64: String,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct ConfigImportResult {
    pub providers_imported: u32,
    pub sort_modes_imported: u32,
    pub workspaces_imported: u32,
    pub prompts_imported: u32,
    pub mcp_servers_imported: u32,
    pub skill_repos_imported: u32,
    pub installed_skills_imported: u32,
    pub local_skills_imported: u32,
}

// --- Shared helpers used by multiple submodules ---

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn normalize_oauth_refresh_lead_seconds(value: i64) -> i64 {
    if value > 0 {
        value
    } else {
        default_oauth_refresh_lead_seconds()
    }
}

fn prompts_for_import(
    prompts: Vec<PromptExport>,
    prompt: Option<PromptExport>,
) -> Vec<PromptExport> {
    if prompts.is_empty() {
        prompt.into_iter().collect()
    } else {
        prompts
    }
}

fn validate_bundle_schema_version(schema_version: u32) -> AppResult<()> {
    if !matches!(
        schema_version,
        CONFIG_BUNDLE_SCHEMA_VERSION_V1
            | CONFIG_BUNDLE_SCHEMA_VERSION_V2
            | CONFIG_BUNDLE_SCHEMA_VERSION
    ) {
        return Err(format!(
            "SEC_INVALID_INPUT: unsupported config bundle schema_version={}, expected one of [{}, {}, {}]",
            schema_version,
            CONFIG_BUNDLE_SCHEMA_VERSION_V1,
            CONFIG_BUNDLE_SCHEMA_VERSION_V2,
            CONFIG_BUNDLE_SCHEMA_VERSION
        )
        .into());
    }
    Ok(())
}

// --- Public entry points ---

pub fn config_export<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
) -> AppResult<ConfigBundle> {
    let app_settings = settings::read(app)?;
    let settings_string = serde_json::to_string(&app_settings)
        .map_err(|e| format!("SYSTEM_ERROR: failed to serialize settings: {e}"))?;

    let conn = db.open_connection()?;
    let provider_cli_key_by_id = export::load_provider_cli_key_by_id(&conn)?;
    let mut skill_export_budget = skill_fs::SkillExportBudget::default();

    Ok(ConfigBundle {
        schema_version: CONFIG_BUNDLE_SCHEMA_VERSION,
        exported_at: export::query_exported_at(&conn)?,
        app_version: app.package_info().version.to_string(),
        settings: settings_string,
        providers: export::export_providers(&conn, &provider_cli_key_by_id)?,
        sort_modes: export::export_sort_modes(&conn)?,
        sort_mode_active: export::export_sort_mode_active(&conn)?,
        workspaces: export::export_workspaces(&conn)?,
        mcp_servers: export::export_mcp_servers(&conn)?,
        skill_repos: export::export_skill_repos(&conn)?,
        installed_skills: Some(export::export_installed_skills(
            app,
            &conn,
            &mut skill_export_budget,
        )?),
        local_skills: Some(export::export_local_skills(app, &mut skill_export_budget)?),
        image_gen_configs: Some(export::export_image_gen_configs(&conn)?),
    })
}

fn config_import_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
static CONFIG_IMPORT_LOCK_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
static CONFIG_IMPORT_NOW_OVERRIDE: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(i64::MIN);

#[cfg(test)]
pub(crate) fn reset_config_import_lock_attempts_for_test() {
    CONFIG_IMPORT_LOCK_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn config_import_lock_attempts_for_test() -> usize {
    CONFIG_IMPORT_LOCK_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn set_config_import_now_override_for_test(value: Option<i64>) {
    CONFIG_IMPORT_NOW_OVERRIDE.store(
        value.unwrap_or(i64::MIN),
        std::sync::atomic::Ordering::SeqCst,
    );
}

fn config_import_timestamp() -> i64 {
    #[cfg(test)]
    {
        let value = CONFIG_IMPORT_NOW_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst);
        if value != i64::MIN {
            return value;
        }
    }
    crate::shared::time::now_unix_seconds()
}

/// Pure payload preflight that does not read or mutate current process state.
pub(crate) fn prepare_config_import(bundle: ConfigBundle) -> AppResult<PreparedConfigImport> {
    let bundle_schema_version = bundle.schema_version;
    validate_bundle_schema_version(bundle_schema_version)?;
    let imports_full_skill_payload =
        bundle_schema_version >= CONFIG_BUNDLE_FULL_SKILL_PAYLOAD_MIN_VERSION;
    let imports_account_usage_snapshot =
        bundle_schema_version >= CONFIG_BUNDLE_ACCOUNT_USAGE_SNAPSHOT_MIN_VERSION;

    let ConfigBundle {
        schema_version: _,
        exported_at: _,
        app_version: _,
        settings,
        mut providers,
        sort_modes,
        sort_mode_active,
        workspaces,
        mcp_servers,
        skill_repos,
        installed_skills,
        local_skills,
        image_gen_configs,
    } = bundle;

    if !imports_account_usage_snapshot {
        for provider in &mut providers {
            provider.account_usage_config = None;
            provider.account_usage_credentials = None;
        }
    } else {
        for provider in &mut providers {
            if let Some(config) = provider.account_usage_config.as_mut() {
                *config =
                    crate::domain::provider_account_usage::sanitize_account_usage_extension_value(
                        config,
                    );
            }
        }
    }

    let (installed_skills, local_skills) = import::resolve_skill_payloads_for_import(
        bundle_schema_version,
        installed_skills,
        local_skills,
    )?;
    import::validate_local_skills_for_import(&local_skills)?;
    let prepared_skill_fs = if imports_full_skill_payload {
        Some(rollback::prepare_skill_fs_import(
            &installed_skills,
            &local_skills,
        )?)
    } else {
        None
    };

    let mut settings_to_write: settings::AppSettings = serde_json::from_str(&settings)
        .map_err(|e| format!("SEC_INVALID_INPUT: invalid settings bundle: {e}"))?;
    settings_to_write.schema_version = settings::SCHEMA_VERSION;

    Ok(PreparedConfigImport {
        imports_full_skill_payload,
        imports_account_usage_snapshot,
        settings_to_write,
        providers,
        sort_modes,
        sort_mode_active,
        workspaces,
        mcp_servers,
        skill_repos,
        installed_skills,
        local_skills,
        prepared_skill_fs,
        image_gen_configs,
    })
}

pub(crate) struct PreparedConfigImport {
    imports_full_skill_payload: bool,
    imports_account_usage_snapshot: bool,
    settings_to_write: settings::AppSettings,
    providers: Vec<ProviderExport>,
    sort_modes: Vec<SortModeExport>,
    sort_mode_active: HashMap<String, String>,
    workspaces: Vec<WorkspaceExport>,
    mcp_servers: Vec<McpServerExport>,
    skill_repos: Vec<SkillRepoExport>,
    installed_skills: Vec<InstalledSkillExport>,
    local_skills: Vec<LocalSkillExport>,
    prepared_skill_fs: Option<rollback::PreparedSkillFsImport>,
    image_gen_configs: Option<Vec<ImageGenConfigExport>>,
}

pub fn config_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    bundle: ConfigBundle,
) -> AppResult<ConfigImportResult> {
    // Pure schema/path/Base64/metadata preflight stays outside the process lock.
    let prepared = prepare_config_import(bundle)?;

    #[cfg(test)]
    CONFIG_IMPORT_LOCK_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let _import_guard = config_import_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    #[cfg(test)]
    run_after_config_import_lock_acquired_test_hook();

    let PreparedConfigImport {
        imports_full_skill_payload,
        imports_account_usage_snapshot,
        mut settings_to_write,
        providers,
        sort_modes,
        sort_mode_active,
        workspaces,
        mcp_servers,
        skill_repos,
        installed_skills,
        local_skills,
        mut prepared_skill_fs,
        image_gen_configs,
    } = prepared;

    let previous_settings = settings::read(app)?;
    let runtime_backups = rollback::capture_cli_runtime_backups(app)?;

    let mut conn = db.open_connection()?;
    let now = config_import_timestamp();
    let tx = conn
        .transaction()
        .map_err(|e| db_err!("failed to start transaction: {e}"))?;

    let legacy_skill_state = if imports_full_skill_payload {
        None
    } else {
        Some(import::capture_legacy_skill_state(&tx)?)
    };

    import::clear_existing_config_data(&tx, imports_full_skill_payload)?;

    let result = import::import_into_transaction(
        &tx,
        now,
        providers,
        sort_modes,
        sort_mode_active,
        workspaces,
        mcp_servers,
        skill_repos,
        imports_full_skill_payload,
        imports_account_usage_snapshot,
        &installed_skills,
        &local_skills,
        legacy_skill_state.as_ref(),
    )?;

    if let Some(image_gen_configs) = &image_gen_configs {
        import::replace_image_gen_configs(&tx, now, image_gen_configs)?;
    }

    let mut skill_fs_guard = if imports_full_skill_payload {
        let prepared_skill_fs = prepared_skill_fs
            .take()
            .ok_or_else(|| "SYSTEM_ERROR: prepared Skill FS payload missing".to_string())?;
        Some(rollback::apply_prepared_skill_fs_import(
            app,
            prepared_skill_fs,
        )?)
    } else {
        None
    };

    #[cfg(test)]
    run_before_config_import_settings_cas_test_hook();
    let settings_commit = crate::app::autostart::commit_whole_settings_with_auto_start(
        app,
        &previous_settings,
        &settings_to_write,
    );
    use crate::app::autostart::WholeSettingsCommitResult;
    let auto_start_token = match settings_commit {
        WholeSettingsCommitResult::Committed { settings, token } => {
            settings_to_write = settings;
            token
        }
        WholeSettingsCommitResult::ConcurrentUpdate => {
            // No durable settings write; release DB tx before reopening for rollback.
            drop(tx);
            let recovery = rollback::rollback_after_failed_import(
                app,
                db,
                &previous_settings,
                None,
                runtime_backups,
                skill_fs_guard.as_mut(),
            );
            let base =
                "SETTINGS_CONCURRENT_UPDATE: settings changed during config import".to_string();
            return Err(match recovery {
                Ok(()) => base.into(),
                Err(fs_err) => format!("{base}; {fs_err}").into(),
            });
        }
        WholeSettingsCommitResult::Failed(error) => {
            drop(tx);
            let recovery = rollback::rollback_after_failed_import(
                app,
                db,
                &previous_settings,
                None,
                runtime_backups,
                skill_fs_guard.as_mut(),
            );
            return Err(match recovery {
                Ok(()) => error.into(),
                Err(fs_err) => format!("{error}; {fs_err}").into(),
            });
        }
        WholeSettingsCommitResult::CommitNeedsRollback {
            committed,
            token,
            error,
        } => {
            // Durable settings already changed; must roll back with token + expected.
            drop(tx);
            let recovery = rollback::rollback_after_failed_import_with_auto_start_token(
                app,
                db,
                &previous_settings,
                Some(&committed),
                Some(token),
                runtime_backups,
                skill_fs_guard.as_mut(),
            );
            return Err(match recovery {
                Ok(()) => error.into(),
                Err(fs_err) => format!("{error}; {fs_err}").into(),
            });
        }
    };

    let runtime_sync_error = {
        #[cfg(test)]
        {
            take_config_import_cli_runtime_sync_error()
                .map(|message| message.into())
                .or_else(|| rollback::sync_all_cli_runtime(app, &tx).err())
        }
        #[cfg(not(test))]
        {
            rollback::sync_all_cli_runtime(app, &tx).err()
        }
    };
    if let Some(err) = runtime_sync_error {
        drop(tx);
        let recovery = rollback::rollback_after_failed_import_with_auto_start_token(
            app,
            db,
            &previous_settings,
            Some(&settings_to_write),
            Some(auto_start_token),
            runtime_backups,
            skill_fs_guard.as_mut(),
        );
        return Err(match recovery {
            Ok(()) => err,
            Err(fs_err) => format!("{err}; {fs_err}").into(),
        });
    }

    if let Err(err) = tx.commit() {
        // commit() already consumed the transaction on failure paths.
        let recovery = rollback::rollback_after_failed_import_with_auto_start_token(
            app,
            db,
            &previous_settings,
            Some(&settings_to_write),
            Some(auto_start_token),
            runtime_backups,
            skill_fs_guard.as_mut(),
        );
        let base = format!("failed to commit transaction: {err}");
        return Err(match recovery {
            Ok(()) => db_err!("{base}"),
            Err(fs_err) => format!("{base}; {fs_err}").into(),
        });
    }

    if let Some(guard) = skill_fs_guard.take() {
        if let Err(err) = guard.finish() {
            return Err(format!(
                "CONFIG_IMPORT_RECOVERY_REQUIRED: config import committed but Skill FS backup cleanup failed: {err}"
            )
            .into());
        }
    }
    resident::sync_tray_enabled_from_canonical(app).map_err(|error| {
        format!(
            "CONFIG_IMPORT_RECOVERY_REQUIRED: config import committed but tray runtime convergence failed: {error}"
        )
    })?;

    Ok(result)
}

#[cfg(test)]
type BeforeConfigImportSettingsCasHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
fn before_config_import_settings_cas_test_hook(
) -> &'static std::sync::Mutex<Option<BeforeConfigImportSettingsCasHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<BeforeConfigImportSettingsCasHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn set_before_config_import_settings_cas_test_hook(hook: BeforeConfigImportSettingsCasHook) {
    *before_config_import_settings_cas_test_hook()
        .lock()
        .expect("config import cas hook") = Some(hook);
}

#[cfg(test)]
fn run_before_config_import_settings_cas_test_hook() {
    let hook = before_config_import_settings_cas_test_hook()
        .lock()
        .expect("config import cas hook")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

/// Observability for second-import wait: fired once the process import lock is
/// acquired (after any wait). Used by concurrent-import tests instead of
/// spawn-then-immediately-check-is_finished races.
#[cfg(test)]
type AfterConfigImportLockAcquiredHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
fn after_config_import_lock_acquired_test_hook(
) -> &'static std::sync::Mutex<Option<AfterConfigImportLockAcquiredHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<AfterConfigImportLockAcquiredHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn set_after_config_import_lock_acquired_test_hook(hook: AfterConfigImportLockAcquiredHook) {
    *after_config_import_lock_acquired_test_hook()
        .lock()
        .expect("config import lock acquired hook") = Some(hook);
}

#[cfg(test)]
fn run_after_config_import_lock_acquired_test_hook() {
    let hook = after_config_import_lock_acquired_test_hook()
        .lock()
        .expect("config import lock acquired hook")
        .take();
    if let Some(hook) = hook {
        hook();
    }
}

/// Injected runtime sync failure for import rollback matrix tests.
#[cfg(test)]
type ConfigImportCliRuntimeSyncHook = Box<dyn FnMut() -> Option<String> + Send>;

#[cfg(test)]
fn config_import_cli_runtime_sync_test_hook(
) -> &'static std::sync::Mutex<Option<ConfigImportCliRuntimeSyncHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<ConfigImportCliRuntimeSyncHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn set_config_import_cli_runtime_sync_test_hook(hook: ConfigImportCliRuntimeSyncHook) {
    *config_import_cli_runtime_sync_test_hook()
        .lock()
        .expect("cli runtime sync hook") = Some(hook);
}

#[cfg(test)]
pub(super) fn take_config_import_cli_runtime_sync_error() -> Option<String> {
    let mut guard = config_import_cli_runtime_sync_test_hook()
        .lock()
        .expect("cli runtime sync hook");
    if let Some(hook) = guard.as_mut() {
        return hook();
    }
    None
}

#[cfg(test)]
pub(crate) fn clear_config_import_cli_runtime_sync_test_hook() {
    *config_import_cli_runtime_sync_test_hook()
        .lock()
        .expect("cli runtime sync hook") = None;
}
