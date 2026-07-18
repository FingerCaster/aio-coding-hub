//! Import rollback infrastructure: backup/restore CLI runtime, skill FS guard, settings recovery.

use crate::shared::error::AppResult;
use crate::{db, settings};
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::PathBuf;

use super::skill_fs::{
    build_local_skill_source_metadata, cli_skills_root, local_skill_dirs,
    prepare_skill_files_for_write, remove_dir_if_exists, ssot_skills_root,
    validate_installed_skill_key, validate_local_dir_name,
    write_prepared_skill_files_into_existing_dir, write_prepared_skill_files_to_dir,
    PreparedSkillFiles,
};
use super::{InstalledSkillExport, LocalSkillExport};

#[derive(Debug, Clone)]
pub(super) struct CliRuntimeBackup {
    pub(super) cli_key: &'static str,
    pub(super) prompt_target: Option<Vec<u8>>,
    pub(super) prompt_manifest: Option<Vec<u8>>,
    pub(super) mcp_target: Option<Vec<u8>>,
    pub(super) mcp_manifest: Option<Vec<u8>>,
}

#[derive(Debug)]
struct LocalSkillBackup {
    original_path: PathBuf,
    backup_path: PathBuf,
}

#[derive(Debug, Default)]
pub(super) struct SkillFsImportGuard {
    /// Candidate/live SSOT path. This path is not owned until activation is
    /// recorded; merely observing it must never authorize deletion.
    ssot_root: Option<PathBuf>,
    /// Stage directory created by this import and safe to remove on failure.
    ssot_stage_dir: Option<PathBuf>,
    ssot_backup_dir: Option<PathBuf>,
    ssot_root_backed_up: bool,
    ssot_root_activated: bool,
    local_backup_roots: Vec<PathBuf>,
    local_backups: Vec<LocalSkillBackup>,
    /// Directories proven absent immediately before this import attempted to
    /// write them. Only these paths may be removed as new local outputs.
    imported_local_dirs: Vec<PathBuf>,
}

#[derive(Debug, Default)]
pub(super) struct SkillFsRollbackReport {
    pub(super) live_root_errors: Vec<String>,
    pub(super) cleanup_errors: Vec<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkillFsImportFailpoint {
    StageWrite,
    SsotBackupRename,
    SsotActivation,
}

#[cfg(test)]
thread_local! {
    static SKILL_FS_IMPORT_FAILPOINT: std::cell::Cell<Option<SkillFsImportFailpoint>> = const { std::cell::Cell::new(None) };
    static BEFORE_LOCAL_SKILL_DIR_CREATE_TEST_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce() + Send>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_skill_fs_import_failpoint(failpoint: SkillFsImportFailpoint) {
    SKILL_FS_IMPORT_FAILPOINT.with(|current| current.set(Some(failpoint)));
}

#[cfg(test)]
fn run_skill_fs_import_failpoint(expected: SkillFsImportFailpoint) -> AppResult<()> {
    let triggered = SKILL_FS_IMPORT_FAILPOINT.with(|current| {
        if current.get() == Some(expected) {
            current.set(None);
            true
        } else {
            false
        }
    });
    if triggered {
        let message = match expected {
            SkillFsImportFailpoint::StageWrite => "injected stage write failure",
            SkillFsImportFailpoint::SsotBackupRename => "injected SSOT backup rename failure",
            SkillFsImportFailpoint::SsotActivation => "injected SSOT activation failure",
        };
        return Err(format!("SYSTEM_ERROR: {message}").into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn set_before_local_skill_dir_create_test_hook(hook: Box<dyn FnOnce() + Send>) {
    BEFORE_LOCAL_SKILL_DIR_CREATE_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_local_skill_dir_create_test_hook() {
    let hook = BEFORE_LOCAL_SKILL_DIR_CREATE_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

impl SkillFsRollbackReport {
    pub(super) fn into_recovery_error(self) -> Option<String> {
        if self.live_root_errors.is_empty() && self.cleanup_errors.is_empty() {
            return None;
        }
        let mut parts = self.live_root_errors;
        parts.extend(self.cleanup_errors);
        Some(format!(
            "SKILL_FS_RECOVERY_REQUIRED: skill filesystem rollback failed: {}",
            parts.join("; ")
        ))
    }
}

impl SkillFsImportGuard {
    /// Restore skill roots owned by this import. Live-root restore failures are
    /// aggregated and must be escalated by the caller while still holding
    /// CONFIG_IMPORT_LOCK. Backup cleanup failures are recorded separately.
    pub(super) fn rollback(&mut self) -> SkillFsRollbackReport {
        let mut report = SkillFsRollbackReport::default();

        if let Some(stage_dir) = self.ssot_stage_dir.take() {
            if let Err(err) = remove_dir_if_exists(&stage_dir) {
                report.cleanup_errors.push(format!(
                    "remove imported SSOT stage {}: {err}",
                    stage_dir.display()
                ));
            }
        }

        for path in self.imported_local_dirs.iter().rev() {
            if let Err(err) = remove_dir_if_exists(path) {
                report
                    .live_root_errors
                    .push(format!("remove imported local {}: {err}", path.display()));
            }
        }

        for backup in self.local_backups.iter().rev() {
            if let Err(err) = remove_dir_if_exists(&backup.original_path) {
                report.live_root_errors.push(format!(
                    "clear local original {}: {err}",
                    backup.original_path.display()
                ));
            }
            if let Err(err) = std::fs::rename(&backup.backup_path, &backup.original_path) {
                report.live_root_errors.push(format!(
                    "restore local backup {} -> {}: {err}",
                    backup.backup_path.display(),
                    backup.original_path.display()
                ));
            }
        }

        if let Some(ssot_root) = &self.ssot_root {
            // A candidate path is not destructive authority. Only an activated
            // replacement belongs to this guard and may be removed.
            if self.ssot_root_activated {
                if let Err(err) = remove_dir_if_exists(ssot_root) {
                    report.live_root_errors.push(format!(
                        "remove imported ssot root {}: {err}",
                        ssot_root.display()
                    ));
                }
            }
            if self.ssot_root_backed_up {
                match self.ssot_backup_dir.as_ref() {
                    Some(backup_dir) => {
                        if let Err(err) = std::fs::rename(backup_dir, ssot_root) {
                            report.live_root_errors.push(format!(
                                "restore ssot backup {} -> {}: {err}",
                                backup_dir.display(),
                                ssot_root.display()
                            ));
                        }
                    }
                    None => report.live_root_errors.push(format!(
                        "SSOT backup ownership was recorded without a backup path for {}",
                        ssot_root.display()
                    )),
                }
            }
        }

        for backup_root in self.local_backup_roots.iter().rev() {
            if let Err(err) = remove_dir_if_exists(backup_root) {
                report.cleanup_errors.push(format!(
                    "cleanup local backup root {}: {err}",
                    backup_root.display()
                ));
            }
        }

        report
    }

    pub(super) fn finish(self) -> AppResult<()> {
        let mut errors = Vec::new();
        if let Some(stage_dir) = self.ssot_stage_dir {
            if let Err(err) = remove_dir_if_exists(&stage_dir) {
                errors.push(format!("cleanup SSOT stage {}: {err}", stage_dir.display()));
            }
        }
        if let Some(backup_dir) = self.ssot_backup_dir {
            if let Err(err) = remove_dir_if_exists(&backup_dir) {
                errors.push(format!(
                    "cleanup SSOT backup {}: {err}",
                    backup_dir.display()
                ));
            }
        }
        for backup_root in self.local_backup_roots {
            if let Err(err) = remove_dir_if_exists(&backup_root) {
                errors.push(format!(
                    "cleanup local backup root {}: {err}",
                    backup_root.display()
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "SKILL_FS_RECOVERY_REQUIRED: skill backup cleanup failed: {}",
                errors.join("; ")
            )
            .into())
        }
    }

    #[cfg(test)]
    pub(super) fn test_guard_with_ssot_root(path: PathBuf) -> Self {
        Self {
            ssot_root: Some(path),
            ssot_root_activated: true,
            ..Self::default()
        }
    }
}

pub(super) fn capture_cli_runtime_backups<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Vec<CliRuntimeBackup>> {
    let mut backups = Vec::new();
    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Workspaces)
    {
        backups.push(CliRuntimeBackup {
            cli_key,
            prompt_target: crate::prompt_sync::read_target_bytes(app, cli_key)?,
            prompt_manifest: crate::prompt_sync::read_manifest_bytes(app, cli_key)?,
            mcp_target: crate::mcp_sync::read_target_bytes(app, cli_key)?,
            mcp_manifest: crate::mcp_sync::read_manifest_bytes(app, cli_key)?,
        });
    }
    Ok(backups)
}

fn restore_cli_runtime_backups<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    backups: Vec<CliRuntimeBackup>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for backup in backups {
        if let Err(err) =
            crate::prompt_sync::restore_target_bytes(app, backup.cli_key, backup.prompt_target)
        {
            errors.push(format!(
                "{}/prompt target restore failed: {err}",
                backup.cli_key
            ));
        }
        if let Err(err) =
            crate::prompt_sync::restore_manifest_bytes(app, backup.cli_key, backup.prompt_manifest)
        {
            errors.push(format!(
                "{}/prompt manifest restore failed: {err}",
                backup.cli_key
            ));
        }
        if let Err(err) =
            crate::mcp_sync::restore_target_bytes(app, backup.cli_key, backup.mcp_target)
        {
            errors.push(format!(
                "{}/mcp target restore failed: {err}",
                backup.cli_key
            ));
        }
        if let Err(err) =
            crate::mcp_sync::restore_manifest_bytes(app, backup.cli_key, backup.mcp_manifest)
        {
            errors.push(format!(
                "{}/mcp manifest restore failed: {err}",
                backup.cli_key
            ));
        }
    }
    errors
}

pub(super) fn sync_all_cli_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    conn: &Connection,
) -> AppResult<()> {
    #[cfg(test)]
    if let Some(error) = super::take_config_import_cli_runtime_sync_error() {
        return Err(error.into());
    }

    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Workspaces)
    {
        crate::prompts::sync_one_cli(app, conn, cli_key)?;
        crate::mcp::sync_one_cli(app, conn, cli_key)?;
        crate::skills::sync_one_cli(app, conn, cli_key)?;
    }
    Ok(())
}

enum SettingsRestoreResult {
    Restored,
    ConcurrentWinner,
    Failed(String),
}

fn restore_settings_after_failed_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_settings: &settings::AppSettings,
    committed_settings: Option<&settings::AppSettings>,
) -> SettingsRestoreResult {
    let Some(committed_settings) = committed_settings else {
        return SettingsRestoreResult::ConcurrentWinner;
    };
    match settings::compare_and_swap(app, committed_settings, previous_settings) {
        Ok((_, true)) => SettingsRestoreResult::Restored,
        Ok((_, false)) => {
            tracing::warn!(
                "config import rollback: settings changed concurrently; preserving the newer snapshot"
            );
            SettingsRestoreResult::ConcurrentWinner
        }
        Err(err) => {
            tracing::warn!(error = %err, "config import rollback: failed to restore settings");
            SettingsRestoreResult::Failed(err.to_string())
        }
    }
}

pub(super) fn rollback_after_failed_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    previous_settings: &settings::AppSettings,
    committed_settings: Option<&settings::AppSettings>,
    runtime_backups: Vec<CliRuntimeBackup>,
    skill_fs_guard: Option<&mut SkillFsImportGuard>,
) -> AppResult<()> {
    rollback_after_failed_import_with_auto_start_token(
        app,
        db,
        previous_settings,
        committed_settings,
        None,
        runtime_backups,
        skill_fs_guard,
    )
}

pub(super) fn rollback_after_failed_import_with_auto_start_token<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    previous_settings: &settings::AppSettings,
    committed_settings: Option<&settings::AppSettings>,
    auto_start_token: Option<crate::app::autostart::AutoStartCommitToken>,
    runtime_backups: Vec<CliRuntimeBackup>,
    skill_fs_guard: Option<&mut SkillFsImportGuard>,
) -> AppResult<()> {
    let mut recovery_parts: Vec<String> = Vec::new();

    match (auto_start_token, committed_settings) {
        (Some(token), Some(committed)) => {
            // Atomic whole-snapshot + generation check under AUTO_START -> SETTINGS.
            // Never mutate auto_start first and then CAS the old expected snapshot.
            match crate::app::autostart::rollback_whole_settings_with_auto_start_token(
                app,
                previous_settings,
                committed,
                token,
            ) {
                crate::app::autostart::OwnedRollbackResult::Restored => {}
                crate::app::autostart::OwnedRollbackResult::ConcurrentWinner(_) => {
                    // The coordinator already converged OS to the winner.
                    tracing::warn!(
                        "config import rollback: settings changed concurrently; preserving the newer snapshot"
                    );
                }
                crate::app::autostart::OwnedRollbackResult::Failed(err) => {
                    tracing::warn!(error = %err, "config import rollback: failed to restore settings");
                    recovery_parts.push(format!("settings/autostart could not be restored: {err}"));
                }
            }
        }
        (None, Some(committed)) => {
            match restore_settings_after_failed_import(app, previous_settings, Some(committed)) {
                SettingsRestoreResult::Restored => {
                    if let Err(err) = crate::app::autostart::restore_auto_start_best_effort(
                        app,
                        previous_settings.auto_start,
                    ) {
                        recovery_parts.push(format!(
                            "settings restored but autostart convergence failed: {err}"
                        ));
                    }
                }
                SettingsRestoreResult::ConcurrentWinner => {
                    // CAS loser with concurrent winner is not RECOVERY_REQUIRED.
                    if let Err(err) = crate::app::autostart::converge_auto_start_to_canonical(app) {
                        recovery_parts.push(format!(
                            "autostart canonical convergence failed after settings CAS loser: {err}"
                        ));
                    }
                }
                SettingsRestoreResult::Failed(err) => {
                    recovery_parts.push(format!(
                        "settings restoration failed during import rollback: {err}"
                    ));
                    if let Err(convergence_error) =
                        crate::app::autostart::converge_auto_start_to_canonical(app)
                    {
                        recovery_parts.push(format!(
                            "autostart canonical convergence failed after settings restore failure: {convergence_error}"
                        ));
                    }
                }
            }
        }
        (Some(token), None) => {
            match crate::app::autostart::rollback_auto_start_with_token_checked(app, token) {
                crate::app::autostart::OwnedRollbackResult::Restored
                | crate::app::autostart::OwnedRollbackResult::ConcurrentWinner(_) => {}
                crate::app::autostart::OwnedRollbackResult::Failed(err) => {
                    recovery_parts.push(format!("autostart rollback/convergence failed: {err}"))
                }
            }
        }
        (None, None) => {}
    }

    if let Some(guard) = skill_fs_guard {
        let report = guard.rollback();
        for cleanup in &report.cleanup_errors {
            tracing::warn!(error = %cleanup, "config import rollback: skill backup cleanup failed");
        }
        if let Some(skill_recovery) = report.into_recovery_error() {
            tracing::error!(error = %skill_recovery, "config import rollback: skill live-root recovery failed");
            recovery_parts.push(skill_recovery);
        }
    }

    let mut cli_runtime_recovery: Option<String> = None;
    match db.open_connection() {
        Ok(conn) => {
            if let Err(err) = sync_all_cli_runtime(app, &conn) {
                tracing::warn!(error = %err, "config import rollback: failed to resync runtime from restored db state");
                let backup_errors = restore_cli_runtime_backups(app, runtime_backups);
                // Backup restore is best-effort; still escalate so callers know
                // CLI runtime may not match restored DB/settings.
                let mut details = vec![format!(
                    "CLI runtime resync failed after import rollback: {err}"
                )];
                details.extend(backup_errors);
                cli_runtime_recovery = Some(details.join("; "));
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "config import rollback: failed to reopen database");
            let backup_errors = restore_cli_runtime_backups(app, runtime_backups);
            let mut details = vec![format!(
                "CLI runtime backup restore path used because DB reopen failed: {err}"
            )];
            details.extend(backup_errors);
            cli_runtime_recovery = Some(details.join("; "));
        }
    }
    if let Some(cli) = cli_runtime_recovery {
        recovery_parts.push(cli);
    }

    // Aggregate settings/autostart + CLI resync/backup + Skill live-root into one
    // RECOVERY_REQUIRED. Concurrent winner that converged is not included.
    if recovery_parts.is_empty() {
        return Ok(());
    }
    Err(format!(
        "CONFIG_IMPORT_RECOVERY_REQUIRED: {}",
        recovery_parts.join("; ")
    )
    .into())
}

#[cfg(test)]
pub(super) fn apply_skill_fs_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    installed_skills: &[InstalledSkillExport],
    local_skills: &[LocalSkillExport],
) -> AppResult<SkillFsImportGuard> {
    let prepared = prepare_skill_fs_import(installed_skills, local_skills)?;
    apply_prepared_skill_fs_import(app, prepared)
}

pub(super) fn apply_prepared_skill_fs_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prepared: PreparedSkillFsImport,
) -> AppResult<SkillFsImportGuard> {
    let PreparedSkillFsImport {
        installed,
        mut local,
    } = prepared;
    let app_data_dir = crate::app_paths::app_data_dir(app)?;
    let import_token = allocate_import_token();
    let ssot_root = ssot_skills_root(app)?;
    let ssot_stage_dir =
        create_unique_import_dir(&app_data_dir, "config-import-skills-stage", &import_token)?;
    let ssot_backup_dir =
        allocate_unique_import_path(&app_data_dir, "config-import-skills-backup", &import_token);
    let mut guard = SkillFsImportGuard {
        ssot_root: Some(ssot_root.clone()),
        ssot_stage_dir: Some(ssot_stage_dir.clone()),
        ..SkillFsImportGuard::default()
    };

    let apply_result = (|| -> AppResult<()> {
        // Stage was created with create-new semantics; only ensure parents exist.

        #[cfg(test)]
        run_skill_fs_import_failpoint(SkillFsImportFailpoint::StageWrite)?;

        for (skill_key, files) in installed {
            let skill_dir = ssot_stage_dir.join(&skill_key);
            write_prepared_skill_files_to_dir(&skill_dir, files)?;
            if !skill_dir.join("SKILL.md").exists() {
                return Err(format!(
                    "SEC_INVALID_INPUT: installed skill missing SKILL.md: {skill_key}"
                )
                .into());
            }
        }

        if ssot_root.exists() {
            #[cfg(test)]
            run_skill_fs_import_failpoint(SkillFsImportFailpoint::SsotBackupRename)?;
            std::fs::rename(&ssot_root, &ssot_backup_dir).map_err(|e| {
                format!(
                    "failed to backup installed skills dir {} -> {}: {e}",
                    ssot_root.display(),
                    ssot_backup_dir.display()
                )
            })?;
            guard.ssot_backup_dir = Some(ssot_backup_dir.clone());
            guard.ssot_root_backed_up = true;
        }

        #[cfg(test)]
        run_skill_fs_import_failpoint(SkillFsImportFailpoint::SsotActivation)?;
        std::fs::rename(&ssot_stage_dir, &ssot_root).map_err(|e| {
            format!(
                "failed to activate installed skills dir {} -> {}: {e}",
                ssot_stage_dir.display(),
                ssot_root.display()
            )
        })?;
        guard.ssot_root_activated = true;

        for cli_key in
            crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
        {
            let root = cli_skills_root(app, cli_key)?;
            std::fs::create_dir_all(&root)
                .map_err(|e| format!("failed to create {}: {e}", root.display()))?;

            let existing_local_dirs = local_skill_dirs(&root)?;
            let backup_root = if existing_local_dirs.is_empty() {
                None
            } else {
                Some(create_unique_import_dir(
                    &app_data_dir,
                    &format!("config-import-local-backup-{cli_key}"),
                    &import_token,
                )?)
            };
            if let Some(backup_root) = &backup_root {
                guard.local_backup_roots.push(backup_root.clone());
            }

            for dir in existing_local_dirs {
                let dir_name = dir
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        format!(
                            "SKILL_IMPORT_INVALID_LOCAL_DIR_NAME: local skill dir name invalid: {}",
                            dir.display()
                        )
                    })?;
                let backup_root = backup_root
                    .as_ref()
                    .ok_or_else(|| "SYSTEM_ERROR: local skill backup root missing".to_string())?;
                let backup_path = backup_root.join(dir_name);
                std::fs::rename(&dir, &backup_path).map_err(|e| {
                    format!(
                        "failed to backup local skill dir {} -> {}: {e}",
                        dir.display(),
                        backup_path.display()
                    )
                })?;
                guard.local_backups.push(LocalSkillBackup {
                    original_path: dir,
                    backup_path,
                });
            }

            for local_skill in local.iter_mut().filter(|value| value.cli_key == cli_key) {
                let dir_name = &local_skill.dir_name;
                let target_dir = root.join(dir_name);
                #[cfg(test)]
                run_before_local_skill_dir_create_test_hook();

                let files = local_skill.files.take().ok_or_else(|| {
                    "SYSTEM_ERROR: prepared local skill payload already consumed".to_string()
                })?;

                // The directory itself is the ownership claim. `create_dir`
                // must win atomically before the path is registered, so an
                // external creator can never become an import-owned output.
                std::fs::create_dir(&target_dir).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        format!(
                            "SKILL_IMPORT_DIR_ALREADY_EXISTS: target local skill dir already exists: {}",
                            target_dir.display()
                        )
                    } else {
                        format!("failed to create {}: {error}", target_dir.display())
                    }
                })?;
                guard.imported_local_dirs.push(target_dir.clone());
                write_prepared_skill_files_into_existing_dir(&target_dir, files)?;
                if !target_dir.join("SKILL.md").exists() {
                    return Err(format!(
                        "SEC_INVALID_INPUT: local skill missing SKILL.md: cli_key={cli_key}, dir_name={dir_name}"
                    )
                    .into());
                }
            }
        }

        Ok(())
    })();

    if let Err(err) = apply_result {
        let report = guard.rollback();
        for cleanup in &report.cleanup_errors {
            tracing::warn!(error = %cleanup, "skill fs apply failure cleanup warning");
        }
        let mut recovery_parts = Vec::new();
        if let Some(recovery) = report.into_recovery_error() {
            recovery_parts.push(recovery);
        }
        if !recovery_parts.is_empty() {
            return Err(format!("{err}; {}", recovery_parts.join("; ")).into());
        }
        return Err(err);
    }

    Ok(guard)
}

struct PreparedLocalSkill {
    cli_key: String,
    dir_name: String,
    files: Option<PreparedSkillFiles>,
}

pub(super) struct PreparedSkillFsImport {
    installed: Vec<(String, PreparedSkillFiles)>,
    local: Vec<PreparedLocalSkill>,
}

pub(super) fn prepare_skill_fs_import(
    installed_skills: &[InstalledSkillExport],
    local_skills: &[LocalSkillExport],
) -> AppResult<PreparedSkillFsImport> {
    let mut seen_skill_keys = HashSet::new();
    let mut installed = Vec::with_capacity(installed_skills.len());
    for skill in installed_skills {
        let skill_key = validate_installed_skill_key(&skill.skill_key)?;
        if !seen_skill_keys.insert(skill_key.clone()) {
            return Err(
                format!("SEC_INVALID_INPUT: duplicate installed skill_key={skill_key}").into(),
            );
        }
        let files = prepare_skill_files_for_write(&skill.files, None)?;
        if !skill
            .files
            .iter()
            .any(|file| file.relative_path == "SKILL.md")
        {
            return Err(format!(
                "SEC_INVALID_INPUT: installed skill missing SKILL.md: {skill_key}"
            )
            .into());
        }
        installed.push((skill_key, files));
    }

    let mut seen_local_names = HashSet::new();
    let mut local = Vec::with_capacity(local_skills.len());
    for skill in local_skills {
        let dir_name = validate_local_dir_name(&skill.dir_name)?;
        if !seen_local_names.insert((skill.cli_key.clone(), dir_name.clone())) {
            return Err(format!(
                "SEC_INVALID_INPUT: duplicate local skill dir_name for cli_key={}: {dir_name}",
                skill.cli_key
            )
            .into());
        }
        let source_metadata = build_local_skill_source_metadata(skill)?;
        let files = prepare_skill_files_for_write(&skill.files, source_metadata.as_ref())?;
        if !skill
            .files
            .iter()
            .any(|file| file.relative_path == "SKILL.md")
        {
            return Err(format!(
                "SEC_INVALID_INPUT: local skill missing SKILL.md: cli_key={}, dir_name={dir_name}",
                skill.cli_key
            )
            .into());
        }
        local.push(PreparedLocalSkill {
            cli_key: skill.cli_key.clone(),
            dir_name,
            files: Some(files),
        });
    }
    Ok(PreparedSkillFsImport { installed, local })
}

fn allocate_import_token() -> String {
    use rand::RngCore as _;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "{}-{}",
        std::process::id(),
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn allocate_unique_import_path(
    parent: &std::path::Path,
    prefix: &str,
    import_token: &str,
) -> PathBuf {
    parent.join(format!("{prefix}-{import_token}"))
}

fn create_unique_import_dir(
    parent: &std::path::Path,
    prefix: &str,
    import_token: &str,
) -> AppResult<PathBuf> {
    use rand::RngCore as _;

    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;

    // Prefer the stable token path; on rare collision append extra entropy.
    let mut candidate = allocate_unique_import_path(parent, prefix, import_token);
    for attempt in 0..32 {
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let mut extra = [0u8; 8];
                rand::thread_rng().fill_bytes(&mut extra);
                candidate = parent.join(format!(
                    "{prefix}-{import_token}-{attempt}-{}",
                    extra.iter().map(|b| format!("{b:02x}")).collect::<String>()
                ));
            }
            Err(err) => {
                return Err(format!("failed to create {}: {err}", candidate.display()).into())
            }
        }
    }
    Err(format!(
        "failed to allocate unique import directory under {}",
        parent.display()
    )
    .into())
}
