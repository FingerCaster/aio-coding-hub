//! Usage: OAuth account related Tauri commands.
//!
//! Uses the OAuthProviderRegistry (adapter pattern) for provider lookup instead of
//! hardcoded enum+match dispatch.

use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::gateway::oauth::provider_trait::{make_redirect_uri, OAuthProvider};
use crate::gateway::oauth::{callback_server, pkce, registry, token_exchange};
use crate::oauth_accounts;
use crate::shared::error::AppResult;
use crate::shared::time::now_unix_seconds;
use rand::RngCore;
use std::process::Command;
use tauri::Emitter;
use tokio::{task, time::Duration};

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OAuthAccountEditable {
    pub id: i64,
    pub cli_key: String,
    pub label: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_uri: Option<String>,
    pub expires_at: Option<i64>,
    pub last_refreshed_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OAuthAccountLimitsSnapshot {
    pub account_id: i64,
    pub cli_key: String,
    pub limit_5h_text: Option<String>,
    pub limit_weekly_text: Option<String>,
    pub fetched_at: i64,
}

fn oauth_http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(format!(
            "aio-coding-hub-oauth-command/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("SYSTEM_ERROR: oauth client init failed: {e}").into())
}

fn parse_status(raw: &str) -> AppResult<oauth_accounts::OAuthAccountStatus> {
    match raw.trim() {
        "active" => Ok(oauth_accounts::OAuthAccountStatus::Active),
        "quota_cooldown" => Ok(oauth_accounts::OAuthAccountStatus::QuotaCooldown),
        "disabled" => Ok(oauth_accounts::OAuthAccountStatus::Disabled),
        "expired" => Ok(oauth_accounts::OAuthAccountStatus::Expired),
        "error" => Ok(oauth_accounts::OAuthAccountStatus::Error),
        _ => Err(
            "SEC_INVALID_INPUT: status must be active|quota_cooldown|disabled|expired|error"
                .to_string()
                .into(),
        ),
    }
}

fn full_to_summary(full: oauth_accounts::OAuthAccount) -> oauth_accounts::OAuthAccountSummary {
    oauth_accounts::OAuthAccountSummary {
        id: full.id,
        cli_key: full.cli_key,
        label: full.label,
        email: full.email,
        provider_type: full.provider_type,
        expires_at: full.expires_at,
        refresh_lead_s: full.refresh_lead_s,
        status: full.status,
        last_error: full.last_error,
        last_refreshed_at: full.last_refreshed_at,
        quota_exceeded: full.quota_exceeded,
        quota_recover_at: full.quota_recover_at,
        created_at: full.created_at,
        updated_at: full.updated_at,
    }
}

fn full_to_editable(full: oauth_accounts::OAuthAccount) -> OAuthAccountEditable {
    OAuthAccountEditable {
        id: full.id,
        cli_key: full.cli_key,
        label: full.label,
        access_token: full.access_token,
        refresh_token: full.refresh_token,
        id_token: full.id_token,
        token_uri: full.token_uri,
        expires_at: full.expires_at,
        last_refreshed_at: full.last_refreshed_at,
    }
}

fn build_oauth_state() -> String {
    use rand::rngs::OsRng;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Build the authorize URL using the adapter pattern.
/// Extra params come from the provider's trait method instead of hardcoded match.
fn build_authorize_url(
    provider: &dyn OAuthProvider,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> AppResult<String> {
    let endpoints = provider.endpoints();
    let mut url = reqwest::Url::parse(endpoints.auth_url)
        .map_err(|e| format!("SYSTEM_ERROR: invalid oauth auth url: {e}"))?;
    {
        let scope = endpoints.scopes.join(" ");
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", endpoints.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("scope", &scope);
        query.append_pair("state", state);
        query.append_pair("code_challenge", code_challenge);
        query.append_pair("code_challenge_method", "S256");

        // Adapter pattern: each provider adds its own extra params via trait dispatch
        for (key, value) in provider.extra_authorize_params() {
            query.append_pair(key, value);
        }
    }
    Ok(url.to_string())
}

fn open_browser(url: &str) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        build_windows_open_browser_command(url)
            .spawn()
            .map_err(|e| format!("SYSTEM_ERROR: failed to open browser: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("SYSTEM_ERROR: failed to open browser: {e}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("SYSTEM_ERROR: failed to open browser: {e}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("SYSTEM_ERROR: browser open is unsupported on this platform"
        .to_string()
        .into())
}

#[cfg(target_os = "windows")]
fn build_windows_open_browser_command(url: &str) -> Command {
    let mut cmd = Command::new("rundll32.exe");
    cmd.arg("url.dll,FileProtocolHandler").arg(url);
    cmd
}

#[tauri::command]
pub(crate) async fn oauth_accounts_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
) -> Result<Vec<oauth_accounts::OAuthAccountSummary>, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("oauth_accounts_list", move || {
        let conn = db.open_connection()?;
        oauth_accounts::list_by_cli(&conn, &cli_key)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn oauth_account_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
) -> Result<OAuthAccountEditable, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run(
        "oauth_account_get",
        move || -> AppResult<OAuthAccountEditable> {
            let conn = db.open_connection()?;
            let full = oauth_accounts::get_by_id(&conn, id)?;
            Ok(full_to_editable(full))
        },
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn oauth_account_upsert(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    account_id: Option<i64>,
    cli_key: String,
    label: String,
    email: Option<String>,
    provider_type: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_uri: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    expires_at: Option<i64>,
    last_refreshed_at: Option<i64>,
    refresh_lead_s: Option<i64>,
    status: Option<String>,
) -> Result<oauth_accounts::OAuthAccountSummary, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("oauth_account_upsert", move || {
        let conn = db.open_connection()?;
        oauth_accounts::upsert(
            &conn,
            account_id,
            &cli_key,
            &label,
            email.as_deref(),
            &provider_type,
            access_token.as_deref(),
            refresh_token.as_deref(),
            id_token.as_deref(),
            token_uri.as_deref(),
            client_id.as_deref(),
            client_secret.as_deref(),
            expires_at,
            refresh_lead_s,
            last_refreshed_at,
            status.as_deref(),
        )
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn oauth_account_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
) -> Result<bool, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run(
        "oauth_account_delete",
        move || -> crate::shared::error::AppResult<bool> {
            let conn = db.open_connection()?;
            oauth_accounts::delete(&conn, id)?;
            Ok(true)
        },
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn oauth_account_set_status(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
    status: String,
    error: Option<String>,
) -> Result<oauth_accounts::OAuthAccountSummary, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run(
        "oauth_account_set_status",
        move || -> AppResult<oauth_accounts::OAuthAccountSummary> {
            let conn = db.open_connection()?;
            oauth_accounts::mark_status(&conn, id, parse_status(&status)?, error.as_deref())?;
            let full = oauth_accounts::get_by_id(&conn, id)?;
            Ok(full_to_summary(full))
        },
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn oauth_account_force_refresh(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
) -> Result<oauth_accounts::OAuthAccountSummary, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;

    let account = blocking::run("oauth_account_force_refresh_load", {
        let db = db.clone();
        move || -> AppResult<oauth_accounts::OAuthAccountForGateway> {
            let conn = db.open_connection()?;
            oauth_accounts::get_for_gateway(&conn, id)
        }
    })
    .await?;

    let client = oauth_http_client().map_err(|e| e.to_string())?;
    let refreshed = crate::gateway::oauth::refresh::refresh_account_access_token(&client, &account)
        .await
        .map_err(|e| e.to_string())?;

    // Adapter pattern: use registry to resolve effective token via trait dispatch
    let reg = registry::global_registry();
    let (effective_access_token, id_token_to_store) =
        if let Some(provider) = reg.get_by_cli_key(&account.cli_key) {
            provider.resolve_effective_token(&refreshed, account.id_token.as_deref())
        } else {
            (refreshed.access_token.clone(), refreshed.id_token.clone())
        };

    let expires_at = refreshed.expires_at.or(account.expires_at);

    blocking::run(
        "oauth_account_force_refresh_save",
        move || -> AppResult<oauth_accounts::OAuthAccountSummary> {
            let conn = db.open_connection()?;
            oauth_accounts::update_tokens(
                &conn,
                id,
                &effective_access_token,
                id_token_to_store.as_deref(),
                expires_at,
                refreshed.refresh_token.as_deref(),
            )?;
            let full = oauth_accounts::get_by_id(&conn, id)?;
            Ok(full_to_summary(full))
        },
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn oauth_account_manual_add(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    cli_key: String,
    label: String,
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_uri: Option<String>,
    expires_at: Option<i64>,
    last_refreshed_at: Option<i64>,
) -> Result<oauth_accounts::OAuthAccountSummary, String> {
    if let Some(value) = last_refreshed_at {
        if value <= 0 {
            return Err("SEC_INVALID_INPUT: last_refreshed_at must be > 0".to_string());
        }
    }
    let db = ensure_db_ready(app, db_state.inner()).await?;

    // Adapter pattern: lookup provider via registry instead of hardcoded config
    let reg = registry::global_registry();
    let provider = reg
        .get_by_cli_key(&cli_key)
        .ok_or_else(|| "SEC_INVALID_INPUT: unsupported cli_key for oauth".to_string())?;

    let endpoints = provider.endpoints();
    let provider_type = provider.provider_type().to_string();
    let token_uri = token_uri.unwrap_or_else(|| endpoints.token_url.to_string());
    let client_id = endpoints.client_id.to_string();
    let client_secret = endpoints.client_secret.map(str::to_string);

    blocking::run("oauth_account_manual_add", move || {
        let conn = db.open_connection()?;
        oauth_accounts::upsert(
            &conn,
            None,
            &cli_key,
            &label,
            None,
            &provider_type,
            Some(&access_token),
            refresh_token.as_deref(),
            id_token.as_deref(),
            Some(&token_uri),
            Some(&client_id),
            client_secret.as_deref(),
            expires_at,
            Some(3600),
            last_refreshed_at,
            Some("active"),
        )
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn oauth_start_login(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    account_id: Option<i64>,
    cli_key: String,
    provider_type: String,
    label: String,
) -> Result<oauth_accounts::OAuthAccountSummary, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;

    // Adapter pattern: lookup provider via registry
    let reg = registry::global_registry();
    let provider = reg
        .get_by_provider_type(&provider_type)
        .ok_or_else(|| "SEC_INVALID_INPUT: unsupported provider_type".to_string())?;
    if provider.cli_key() != cli_key.trim() {
        return Err("SEC_INVALID_INPUT: cli_key/provider_type mismatch".to_string());
    }

    let endpoints = provider.endpoints();
    let pkce_pair = pkce::generate_pkce_pair();
    let state = build_oauth_state();
    let callback_listener =
        callback_server::bind_callback_listener(endpoints.default_callback_port)
            .await
            .map_err(|e| e.to_string())?;
    let callback_port = callback_listener.port();
    let redirect_uri = make_redirect_uri(endpoints, callback_port);
    let auth_url = build_authorize_url(provider, &redirect_uri, &state, &pkce_pair.code_challenge)
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "oauth-login-progress",
        serde_json::json!({ "cli_key": cli_key.as_str(), "step": "waiting_callback" }),
    );
    let callback_state = state.clone();
    let callback_task = task::spawn(async move {
        callback_server::wait_for_callback(
            callback_listener,
            &callback_state,
            Duration::from_secs(300),
        )
        .await
    });
    task::yield_now().await;

    if let Err(err) = open_browser(&auth_url) {
        callback_task.abort();
        let _ = app.emit(
            "oauth-login-progress",
            serde_json::json!({ "cli_key": cli_key.as_str(), "step": "error" }),
        );
        return Err(err.to_string());
    }

    let callback_payload = callback_task
        .await
        .map_err(|e| format!("SYSTEM_ERROR: oauth callback task failed: {e}"))?
        .map_err(|e| e.to_string())?;
    if let Some(err_code) = callback_payload.error.as_deref() {
        let description = callback_payload
            .error_description
            .as_deref()
            .unwrap_or("oauth login failed");
        let _ = app.emit(
            "oauth-login-progress",
            serde_json::json!({ "cli_key": cli_key.as_str(), "step": "error" }),
        );
        return Err(format!(
            "SYSTEM_ERROR: oauth provider returned error={err_code}: {description}"
        ));
    }
    let code = callback_payload
        .code
        .ok_or_else(|| "SYSTEM_ERROR: oauth callback missing code".to_string())?;

    let _ = app.emit(
        "oauth-login-progress",
        serde_json::json!({ "cli_key": cli_key.as_str(), "step": "exchanging" }),
    );

    let client = oauth_http_client().map_err(|e| e.to_string())?;
    let token_set = token_exchange::exchange_authorization_code(
        &client,
        &token_exchange::TokenExchangeRequest {
            token_uri: endpoints.token_url.to_string(),
            client_id: endpoints.client_id.to_string(),
            client_secret: endpoints.client_secret.map(str::to_string),
            code,
            redirect_uri: redirect_uri.clone(),
            code_verifier: pkce_pair.code_verifier,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    // Adapter pattern: resolve effective token via trait dispatch
    let (effective_access_token, id_token_to_store) =
        provider.resolve_effective_token(&token_set, None);

    let cli_key_for_save = cli_key.clone();
    let label_for_save = label.clone();
    let provider_type_for_save = provider_type.clone();
    let token_url = endpoints.token_url.to_string();
    let client_id_save = endpoints.client_id.to_string();
    let client_secret_save = endpoints.client_secret.map(str::to_string);
    let summary = blocking::run("oauth_start_login_save_account", move || {
        let conn = db.open_connection()?;
        oauth_accounts::upsert(
            &conn,
            account_id,
            &cli_key_for_save,
            &label_for_save,
            None,
            &provider_type_for_save,
            Some(&effective_access_token),
            token_set.refresh_token.as_deref(),
            id_token_to_store.as_deref(),
            Some(&token_url),
            Some(&client_id_save),
            client_secret_save.as_deref(),
            token_set.expires_at,
            Some(3600),
            None,
            Some("active"),
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "oauth-login-progress",
        serde_json::json!({ "cli_key": cli_key.as_str(), "step": "done" }),
    );

    Ok(summary)
}

#[tauri::command]
pub(crate) async fn oauth_account_fetch_limits(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    id: i64,
) -> Result<OAuthAccountLimitsSnapshot, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    let account = blocking::run("oauth_account_fetch_limits_load", move || {
        let conn = db.open_connection()?;
        oauth_accounts::get_by_id(&conn, id)
    })
    .await
    .map_err(|e| e.to_string())?;

    let client = oauth_http_client().map_err(|e| e.to_string())?;

    // Adapter pattern: delegate limits fetching to the provider via trait dispatch
    let reg = registry::global_registry();
    let provider = reg.get_by_cli_key(&account.cli_key).ok_or_else(|| {
        "SEC_INVALID_INPUT: unsupported cli_key for oauth limit fetch".to_string()
    })?;

    let limits = provider
        .fetch_limits(&client, &account.access_token)
        .await
        .map_err(|e| e.to_string())?;

    Ok(OAuthAccountLimitsSnapshot {
        account_id: account.id,
        cli_key: account.cli_key,
        limit_5h_text: limits.limit_5h_text,
        limit_weekly_text: limits.limit_weekly_text,
        fetched_at: now_unix_seconds(),
    })
}

#[cfg(all(test, target_os = "windows"))]
mod windows_open_browser_tests {
    use super::build_windows_open_browser_command;
    use std::ffi::OsStr;

    #[test]
    fn windows_browser_command_uses_protocol_handler() {
        let cmd = build_windows_open_browser_command("https://example.com/auth?x=1&y=2");

        assert_eq!(cmd.get_program(), OsStr::new("rundll32.exe"));
        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "url.dll,FileProtocolHandler",
                "https://example.com/auth?x=1&y=2"
            ]
        );
    }
}
