//! Project rule-editor candidates from the authoritative base Codex catalog.

use super::{CodexModelCatalogIssue, CodexModelCatalogSnapshot, CodexModelCatalogStatus};
use crate::shared::error::{AppError, AppResult};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

const MAX_CANDIDATE_MODEL_COUNT: usize = 1_000;
const MAX_CANDIDATE_MODEL_ID_BYTES: usize = 256;
const MAX_CANDIDATE_DISPLAY_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
pub struct CodexModelContextCandidate {
    pub model_id: String,
    pub display_name: String,
    pub hidden: bool,
    pub base_context_window: Option<i64>,
    pub base_max_context_window: Option<i64>,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
pub struct CodexModelContextCandidatesState {
    pub status: CodexModelCatalogStatus,
    pub issue: Option<CodexModelCatalogIssue>,
    pub snapshot: CodexModelCatalogSnapshot,
    pub models: Vec<CodexModelContextCandidate>,
}

pub(crate) fn codex_model_context_candidates_get_locked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexModelContextCandidatesState> {
    let (snapshot, _) = super::catalog_snapshot(app)?;
    let base = match super::managed::read_original_base_catalog_locked(app) {
        Ok(base) => base,
        Err(error) => {
            let Some((status, issue)) = expected_base_read_issue(&error) else {
                return Err(error);
            };
            return Ok(candidate_state(snapshot, status, Some(issue), Vec::new()));
        }
    };

    match parse_candidates(&base.bytes) {
        Ok(models) if models.is_empty() => Ok(candidate_state(
            snapshot,
            CodexModelCatalogStatus::Degraded,
            Some(CodexModelCatalogIssue::EmptyCatalog),
            models,
        )),
        Ok(models) => Ok(candidate_state(
            snapshot,
            CodexModelCatalogStatus::Ready,
            None,
            models,
        )),
        Err(()) => Ok(candidate_state(
            snapshot,
            CodexModelCatalogStatus::Degraded,
            Some(CodexModelCatalogIssue::ProtocolError),
            Vec::new(),
        )),
    }
}

fn candidate_state(
    snapshot: CodexModelCatalogSnapshot,
    status: CodexModelCatalogStatus,
    issue: Option<CodexModelCatalogIssue>,
    models: Vec<CodexModelContextCandidate>,
) -> CodexModelContextCandidatesState {
    CodexModelContextCandidatesState {
        status,
        issue,
        snapshot,
        models,
    }
}

fn expected_base_read_issue(
    error: &AppError,
) -> Option<(CodexModelCatalogStatus, CodexModelCatalogIssue)> {
    match error.code() {
        "CODEX_MANAGED_MODEL_CLI_NOT_FOUND" => Some((
            CodexModelCatalogStatus::Unavailable,
            CodexModelCatalogIssue::CliNotFound,
        )),
        "CODEX_MANAGED_MODEL_BUNDLED_TIMEOUT" => Some((
            CodexModelCatalogStatus::Degraded,
            CodexModelCatalogIssue::Timeout,
        )),
        "CODEX_MANAGED_MODEL_BUNDLED_UNAVAILABLE" => Some((
            CodexModelCatalogStatus::Degraded,
            CodexModelCatalogIssue::AppServerUnavailable,
        )),
        "CODEX_MANAGED_MODEL_BUNDLED_INVALID"
        | "CODEX_MANAGED_MODEL_BASE_CATALOG_UNAVAILABLE"
        | "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID" => Some((
            CodexModelCatalogStatus::Degraded,
            CodexModelCatalogIssue::ProtocolError,
        )),
        _ => None,
    }
}

fn parse_candidates(bytes: &[u8]) -> Result<Vec<CodexModelContextCandidate>, ()> {
    let root: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let models = root
        .as_object()
        .and_then(|object| object.get("models"))
        .and_then(Value::as_array)
        .ok_or(())?;
    if models.len() > MAX_CANDIDATE_MODEL_COUNT {
        return Err(());
    }

    let mut seen = HashSet::with_capacity(models.len());
    let mut candidates = Vec::with_capacity(models.len());
    for model in models {
        let object = model.as_object().ok_or(())?;
        let slug = object
            .get("slug")
            .and_then(Value::as_str)
            .filter(|slug| {
                !slug.is_empty()
                    && slug.len() <= MAX_CANDIDATE_MODEL_ID_BYTES
                    && !slug.chars().any(char::is_control)
            })
            .ok_or(())?;
        if !seen.insert(slug) {
            return Err(());
        }
        if slug.starts_with("aio/") {
            continue;
        }

        let display_name = object
            .get("display_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= MAX_CANDIDATE_DISPLAY_NAME_BYTES
                    && !value.chars().any(char::is_control)
            })
            .unwrap_or(slug)
            .to_string();
        candidates.push(CodexModelContextCandidate {
            model_id: slug.to_string(),
            display_name,
            hidden: object.get("visibility").and_then(Value::as_str) != Some("list"),
            base_context_window: optional_non_negative_i64(object.get("context_window")),
            base_max_context_window: optional_non_negative_i64(object.get("max_context_window")),
        });
    }
    Ok(candidates)
}

fn optional_non_negative_i64(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| i64::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::ffi::{OsStr, OsString};

    struct EnvRestore {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            crate::settings::clear_cache();
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn parse(value: Value) -> Result<Vec<CodexModelContextCandidate>, ()> {
        parse_candidates(&serde_json::to_vec(&value).expect("serialize catalog"))
    }

    #[test]
    fn candidates_project_exact_slug_display_visibility_and_optional_windows() {
        let candidates = parse(json!({
            "models": [
                {
                    "slug": "gpt-visible",
                    "display_name": "  GPT Visible  ",
                    "visibility": "list",
                    "context_window": 272000,
                    "max_context_window": 300000
                },
                {
                    "slug": "gpt-hidden",
                    "visibility": "hide",
                    "context_window": null,
                    "max_context_window": "272000"
                },
                {
                    "slug": "gpt-unknown-visibility",
                    "visibility": "future",
                    "context_window": -1,
                    "max_context_window": 10000000
                },
                {
                    "slug": "AIO/case-sensitive",
                    "display_name": "",
                    "visibility": "list"
                },
                {
                    "slug": "aio/managed",
                    "display_name": "Filtered",
                    "visibility": "list",
                    "context_window": 512000,
                    "max_context_window": 512000
                }
            ]
        }))
        .expect("valid candidates");

        assert_eq!(
            candidates,
            vec![
                CodexModelContextCandidate {
                    model_id: "gpt-visible".to_string(),
                    display_name: "GPT Visible".to_string(),
                    hidden: false,
                    base_context_window: Some(272000),
                    base_max_context_window: Some(300000),
                },
                CodexModelContextCandidate {
                    model_id: "gpt-hidden".to_string(),
                    display_name: "gpt-hidden".to_string(),
                    hidden: true,
                    base_context_window: None,
                    base_max_context_window: None,
                },
                CodexModelContextCandidate {
                    model_id: "gpt-unknown-visibility".to_string(),
                    display_name: "gpt-unknown-visibility".to_string(),
                    hidden: true,
                    base_context_window: None,
                    base_max_context_window: Some(10000000),
                },
                CodexModelContextCandidate {
                    model_id: "AIO/case-sensitive".to_string(),
                    display_name: "AIO/case-sensitive".to_string(),
                    hidden: false,
                    base_context_window: None,
                    base_max_context_window: None,
                },
            ]
        );
    }

    #[test]
    fn candidates_reject_malformed_or_ambiguous_base_catalogs() {
        assert!(parse(json!([])).is_err());
        assert!(parse(json!({"models": {}})).is_err());
        assert!(parse(json!({"models": [null]})).is_err());
        assert!(parse(json!({"models": [{"slug": ""}]})).is_err());
        assert!(parse(json!({
            "models": [{"slug": "duplicate"}, {"slug": "duplicate"}]
        }))
        .is_err());
        assert!(parse(json!({"models": [{"slug": "control\nslug"}]})).is_err());
    }

    #[test]
    fn candidates_bound_the_base_model_count() {
        let models = (0..=MAX_CANDIDATE_MODEL_COUNT)
            .map(|index| json!({"slug": format!("model-{index}")}))
            .collect::<Vec<_>>();
        assert!(parse(json!({"models": models})).is_err());
    }

    #[test]
    fn candidate_read_uses_original_user_base_without_managed_catalog_writes() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let _home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let test_dotdir = ".aio-candidate-test";
        let _dotdir_restore = EnvRestore::set("AIO_CODING_HUB_DOTDIR_NAME", test_dotdir);
        crate::settings::clear_cache();

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let codex_home = home.path().join(".codex");
        std::fs::create_dir_all(&codex_home).expect("create Codex home");
        let catalog_path = codex_home.join("base-models.json");
        let catalog_bytes = serde_json::to_vec(&json!({
            "models": [{
                "slug": "base-model",
                "display_name": "Base Model",
                "visibility": "list",
                "context_window": 272000,
                "max_context_window": 272000
            }]
        }))
        .expect("serialize catalog");
        std::fs::write(&catalog_path, &catalog_bytes).expect("write catalog");

        let config_path = codex_home.join("config.toml");
        let mut config = toml_edit::DocumentMut::new();
        config["model_catalog_json"] = toml_edit::value(catalog_path.to_string_lossy().to_string());
        let config_bytes = config.to_string().into_bytes();
        std::fs::write(&config_path, &config_bytes).expect("write config");

        let managed_dir = home
            .path()
            .join(test_dotdir)
            .join("cli-proxy")
            .join("codex");
        assert!(!managed_dir.exists());

        let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
        let state = codex_model_context_candidates_get_locked(&handle).expect("read candidates");

        assert_eq!(state.status, CodexModelCatalogStatus::Ready);
        assert_eq!(state.issue, None);
        assert_eq!(state.models.len(), 1);
        assert_eq!(state.models[0].model_id, "base-model");
        assert!(!managed_dir.exists());
        assert_eq!(
            std::fs::read(&config_path).expect("read config"),
            config_bytes
        );
        assert_eq!(
            std::fs::read(&catalog_path).expect("read catalog"),
            catalog_bytes
        );
    }
}
