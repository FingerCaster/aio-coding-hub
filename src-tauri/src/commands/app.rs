//! Usage: App-level Tauri commands (about info, lifecycle, etc.).

use regex::Regex;
use std::sync::LazyLock;
use tauri::utils::config::BundleType;
use tauri::Manager;

static FRONTEND_ERROR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bhttps?://[^\s<>\"']+"#).expect("valid frontend-error URL regex")
});
static FRONTEND_ERROR_AUTHORIZATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)([\"']?\b(?:proxy[-_])?authorization[\"']?\s*[:=]\s*[\"']?(?:(?:bearer|basic)\s+)?)([^\s,;\"']+)"#,
    )
    .expect("valid frontend-error authorization regex")
});
static FRONTEND_ERROR_BEARER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\bbearer\s+)([^\s,;\"']+)"#).expect("valid frontend-error bearer regex")
});
static FRONTEND_ERROR_SECRET_ASSIGNMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)([\"']?(?:[a-z0-9_-]*token|client[_-]?secret|private[_-]?key|api[_-]?key|authorization|password|passwd|secret|credential|cookie|flow[_-]?id|device[_-]?code|user[_-]?code|code[_-]?verifier|nonce|[a-z0-9_-]*capability[a-z0-9_-]*)[\"']?\s*[:=]\s*[\"']?)([^\s,;&\"']+)"#,
    )
    .expect("valid frontend-error secret assignment regex")
});
static FRONTEND_ERROR_KEYLIKE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:sk|sk-proj|ghp|gho|ghu|ghs|ghr|github_pat|xox[baprs])[-_][a-z0-9_-]{8,}\b",
    )
    .expect("valid frontend-error key regex")
});

fn normalize_frontend_error_url(value: &str) -> Option<(String, bool)> {
    let mut url = reqwest::Url::parse(value.trim()).ok()?;
    let removed_sensitive_parts = !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    Some((url.to_string(), removed_sensitive_parts))
}

fn redact_frontend_error_text(value: &str) -> String {
    let urls = FRONTEND_ERROR_URL_RE.replace_all(value, |captures: &regex::Captures<'_>| {
        let raw_url = captures.get(0).map_or("", |matched| matched.as_str());
        match normalize_frontend_error_url(raw_url) {
            Some((url, true)) => format!("{url} [REDACTED]"),
            Some((url, false)) => url,
            None => "[REDACTED]".to_string(),
        }
    });
    let authorization = FRONTEND_ERROR_AUTHORIZATION_RE.replace_all(&urls, "$1[REDACTED]");
    let bearer = FRONTEND_ERROR_BEARER_RE.replace_all(&authorization, "$1[REDACTED]");
    let assigned = FRONTEND_ERROR_SECRET_ASSIGNMENT_RE.replace_all(&bearer, "$1[REDACTED]");
    FRONTEND_ERROR_KEYLIKE_RE
        .replace_all(&assigned, "[REDACTED]")
        .into_owned()
}

fn sanitize_frontend_error_text(input: Option<String>, max_len: usize) -> Option<String> {
    let value = input?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        redact_frontend_error_text(trimmed)
            .chars()
            .take(max_len)
            .collect(),
    )
}

fn sanitize_frontend_error_href(input: Option<String>, max_len: usize) -> Option<String> {
    let value = input?;
    let (url, _) = normalize_frontend_error_url(&value)?;
    Some(url.chars().take(max_len).collect())
}

fn normalize_frontend_error_source(input: String) -> String {
    match input.trim() {
        "error" | "unhandledrejection" | "render" => input.trim().to_string(),
        _ => "unknown".to_string(),
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct AppAboutInfo {
    os: String,
    arch: String,
    profile: String,
    app_version: String,
    bundle_type: Option<String>,
    run_mode: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontendErrorReportInput {
    source: String,
    message: String,
    stack: Option<String>,
    details_json: Option<String>,
    href: Option<String>,
    user_agent: Option<String>,
}

pub(crate) use crate::app::startup_state::AppStartupStatus;

#[tauri::command]
#[specta::specta]
pub(crate) fn app_about_get() -> AppAboutInfo {
    let bundle_type = tauri::utils::platform::bundle_type();
    let run_mode = match bundle_type {
        Some(BundleType::Nsis | BundleType::Msi | BundleType::Deb | BundleType::Rpm) => "installer",
        Some(BundleType::AppImage) => "portable",
        Some(BundleType::App | BundleType::Dmg) => "unknown",
        None => {
            // On Windows, BundleType::None means the exe is NOT running from an
            // MSI or NSIS install, so it must be a portable (ZIP) deployment.
            #[cfg(windows)]
            {
                "portable"
            }
            #[cfg(not(windows))]
            {
                "unknown"
            }
        }
    }
    .to_string();

    AppAboutInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        profile: if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        bundle_type: bundle_type.map(|t| t.to_string()),
        run_mode,
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn app_exit(app: tauri::AppHandle) -> Result<bool, String> {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.state::<crate::app::resident::ResidentState>()
            .begin_exit();
        app.exit(0);
    });
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn app_restart(app: tauri::AppHandle) -> Result<bool, String> {
    crate::app::maintenance::ensure_normal_operation(&app)?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.state::<crate::app::resident::ResidentState>()
            .begin_restart();
        tauri::async_runtime::block_on(crate::app::cleanup::cleanup_before_exit(&app));
        app.request_restart();
    });
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn app_heartbeat_pong(app: tauri::AppHandle) -> Result<bool, String> {
    let watchdog = app.state::<crate::app::heartbeat_watchdog::HeartbeatWatchdogState>();
    watchdog.record_pong();
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn app_startup_status_get(app: tauri::AppHandle) -> AppStartupStatus {
    crate::app::startup_state::startup_status_snapshot(&app)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn app_startup_retry(app: tauri::AppHandle) -> AppStartupStatus {
    let status = crate::app::startup_state::startup_status_snapshot(&app);
    if status.maintenance_mode {
        if crate::app::maintenance::retry_pending_reset(app.clone()).await {
            crate::app::bootstrap::start_normal_runtime(&app);
        }
    } else {
        let _ = crate::app::startup_tasks::spawn(app.clone());
    }
    crate::app::startup_state::startup_status_snapshot(&app)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn app_frontend_error_report(input: FrontendErrorReportInput) -> Result<bool, String> {
    let source = normalize_frontend_error_source(input.source);
    let message = sanitize_frontend_error_text(Some(input.message), 4096)
        .unwrap_or_else(|| "unknown".to_string());
    let stack = sanitize_frontend_error_text(input.stack, 16_384);
    let details_json = sanitize_frontend_error_text(input.details_json, 16_384);
    let href = sanitize_frontend_error_href(input.href, 2_048);
    let user_agent = sanitize_frontend_error_text(input.user_agent, 1_024);

    tracing::error!(
        target: "frontend",
        source = %source,
        href = %href.as_deref().unwrap_or_default(),
        user_agent = %user_agent.as_deref().unwrap_or_default(),
        stack = %stack.as_deref().unwrap_or_default(),
        details_json = %details_json.as_deref().unwrap_or_default(),
        "frontend runtime error: {}",
        message
    );

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_frontend_error_text_returns_none_for_none_input() {
        assert!(sanitize_frontend_error_text(None, 100).is_none());
    }

    #[test]
    fn sanitize_frontend_error_text_returns_none_for_empty_string() {
        assert!(sanitize_frontend_error_text(Some("".to_string()), 100).is_none());
    }

    #[test]
    fn sanitize_frontend_error_text_returns_none_for_whitespace_only() {
        assert!(sanitize_frontend_error_text(Some("   \t\n  ".to_string()), 100).is_none());
    }

    #[test]
    fn sanitize_frontend_error_text_trims_whitespace() {
        assert_eq!(
            sanitize_frontend_error_text(Some("  hello  ".to_string()), 100),
            Some("hello".to_string())
        );
    }

    #[test]
    fn sanitize_frontend_error_text_truncates_to_max_len() {
        assert_eq!(
            sanitize_frontend_error_text(Some("abcdefgh".to_string()), 3),
            Some("abc".to_string())
        );
    }

    #[test]
    fn sanitize_frontend_error_text_truncates_after_trimming() {
        // Whitespace is trimmed first, then truncation applies to the trimmed result.
        assert_eq!(
            sanitize_frontend_error_text(Some("  abcdefgh  ".to_string()), 5),
            Some("abcde".to_string())
        );
    }

    #[test]
    fn sanitize_frontend_error_text_handles_multibyte_chars_by_char_count() {
        // Truncation is by char count, not byte count.
        let cjk = Some("\u{4f60}\u{597d}\u{4e16}\u{754c}".to_string()); // 4 CJK chars
        assert_eq!(
            sanitize_frontend_error_text(cjk, 2),
            Some("\u{4f60}\u{597d}".to_string())
        );
    }

    #[test]
    fn sanitize_frontend_error_text_returns_full_string_when_within_limit() {
        assert_eq!(
            sanitize_frontend_error_text(Some("short".to_string()), 100),
            Some("short".to_string())
        );
    }

    #[test]
    fn frontend_error_diagnostics_redact_secrets_before_tracing() {
        let secret = "sentinel-0123456789abcdef0123456789abcdef";
        let source = normalize_frontend_error_source(format!("Authorization: Bearer {secret}"));
        let message =
            sanitize_frontend_error_text(Some(format!("Authorization: Bearer {secret}")), 4096)
                .expect("message");
        let stack =
            sanitize_frontend_error_text(Some(format!("api_key={secret}")), 16_384).expect("stack");
        let details = sanitize_frontend_error_text(
            Some(format!(
                r#"{{"password":"{secret}","token":"{secret}","session_token":"{secret}"}}"#
            )),
            16_384,
        )
        .expect("details");
        let user_agent =
            sanitize_frontend_error_text(Some(format!("test-agent secret={secret}")), 1024)
                .expect("user agent");
        let href = sanitize_frontend_error_href(
            Some(format!(
                "https://user:{secret}@example.test/path?token={secret}#{secret}"
            )),
            2048,
        )
        .expect("href");

        let diagnostics = format!("{source}\n{message}\n{stack}\n{details}\n{user_agent}\n{href}");
        assert!(!diagnostics.contains(secret));
        assert!(diagnostics.contains("[REDACTED]"));
        assert_eq!(source, "unknown");
        assert_eq!(href, "https://example.test/path");
    }
}
