//! Usage: Resolve Codex user-level paths (supports $CODEX_HOME).

use crate::settings;
use std::path::{Path, PathBuf};

const ENV_CODEX_HOME: &str = "CODEX_HOME";

fn home_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    crate::app_paths::home_dir(app)
}

fn expand_tilde(home: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return Some(home.to_path_buf());
    }

    let rest = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"));

    rest.map(|suffix| home.join(suffix))
}

fn resolve_under_home(home: &Path, raw: &str) -> PathBuf {
    if let Some(p) = expand_tilde(home, raw) {
        return p;
    }

    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return candidate;
    }

    home.join(candidate)
}

fn normalize_codex_home_path(path: PathBuf) -> PathBuf {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return path;
    };

    if !file_name.eq_ignore_ascii_case("config.toml") {
        return path;
    }

    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or(path)
}

pub(crate) fn configured_codex_home_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<PathBuf> {
    settings::read(app)
        .ok()
        .filter(|settings| settings.codex_home_mode == settings::CodexHomeMode::Custom)
        .and_then(|settings| configured_codex_home_dir_for_settings(app, &settings).ok())
        .flatten()
}

fn configured_codex_home_dir_for_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_settings: &settings::AppSettings,
) -> crate::shared::error::AppResult<Option<PathBuf>> {
    let value = app_settings.codex_home_override.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let home = home_dir(app)?;
    Ok(Some(normalize_codex_home_path(resolve_under_home(
        &home, value,
    ))))
}

pub fn codex_home_dir_user_default<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(home_dir(app)?.join(".codex"))
}

pub fn codex_home_dir_follow_env_or_default<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    let home = home_dir(app)?;
    let raw = std::env::var(ENV_CODEX_HOME)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    Ok(match raw {
        Some(v) => normalize_codex_home_path(resolve_under_home(&home, &v)),
        None => codex_home_dir_user_default(app)?,
    })
}

pub fn codex_home_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    let app_settings = settings::read(app).unwrap_or_default();
    codex_home_dir_for_settings(app, &app_settings)
}

pub(crate) fn codex_home_dir_for_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_settings: &settings::AppSettings,
) -> crate::shared::error::AppResult<PathBuf> {
    match app_settings.codex_home_mode {
        settings::CodexHomeMode::UserHomeDefault => codex_home_dir_user_default(app),
        settings::CodexHomeMode::FollowCodexHome => codex_home_dir_follow_env_or_default(app),
        settings::CodexHomeMode::Custom => {
            configured_codex_home_dir_for_settings(app, app_settings)?
                .map(Ok)
                .unwrap_or_else(|| codex_home_dir_user_default(app))
        }
    }
}

pub fn codex_config_toml_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(codex_home_dir(app)?.join("config.toml"))
}

pub fn codex_auth_json_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(codex_home_dir(app)?.join("auth.json"))
}

pub fn codex_agents_md_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(codex_home_dir(app)?.join("AGENTS.md"))
}

pub fn codex_skills_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(codex_home_dir(app)?.join("skills"))
}

pub fn codex_sessions_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(codex_home_dir(app)?.join("sessions"))
}
