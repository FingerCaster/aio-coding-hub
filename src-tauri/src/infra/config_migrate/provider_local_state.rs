//! Usage: Preserve machine-local provider model/profile state across config v4 imports.

use super::{ProviderExport, CONFIG_BUNDLE_PROVIDER_UUID_MIN_VERSION};
use crate::shared::error::{db_err, AppError, AppResult};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug)]
struct CatalogRow {
    provider_uuid: String,
    protocol: String,
    last_attempt_at: Option<i64>,
    last_success_at: Option<i64>,
    last_error_code: Option<String>,
}

#[derive(Debug)]
struct ModelRow {
    provider_uuid: String,
    model_uuid: String,
    remote_model_id: String,
    source: String,
    last_seen_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    capabilities_configured: bool,
    supported_reasoning_efforts_json: String,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
}

#[derive(Debug)]
struct ProfileRow {
    provider_uuid: String,
    profile_uuid: String,
    profile_name: String,
    profile_name_key: String,
    model_uuid: String,
    codex_home_path: String,
    content_sha256: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Default)]
pub(super) struct LocalProviderState {
    catalogs: Vec<CatalogRow>,
    models: Vec<ModelRow>,
    profiles: Vec<ProfileRow>,
}

impl LocalProviderState {
    pub(super) fn capture(conn: &Connection) -> AppResult<Self> {
        Ok(Self {
            catalogs: capture_catalogs(conn)?,
            models: capture_models(conn)?,
            profiles: capture_profiles(conn)?,
        })
    }

    pub(super) fn validate_rebind(
        &self,
        schema_version: u32,
        providers: &[ProviderExport],
        imported_codex_home: &Path,
    ) -> AppResult<()> {
        if self.profiles.is_empty() {
            return Ok(());
        }

        if schema_version < CONFIG_BUNDLE_PROVIDER_UUID_MIN_VERSION {
            return Err(profile_conflict(
                "legacy config bundles cannot safely rebind local managed Codex profiles",
                self.profiles
                    .iter()
                    .map(|profile| profile.profile_name.as_str()),
            ));
        }

        let imported_by_uuid = providers
            .iter()
            .filter_map(|provider| {
                provider
                    .provider_uuid
                    .as_deref()
                    .map(|provider_uuid| (provider_uuid, provider))
            })
            .collect::<HashMap<_, _>>();
        let imported_home_key = path_key(imported_codex_home)?;
        let mut conflicting = Vec::new();
        for profile in &self.profiles {
            let provider = imported_by_uuid.get(profile.provider_uuid.as_str());
            let is_conflicting = match provider {
                Some(provider) => {
                    provider.cli_key != "codex"
                        || provider.source_provider_id.is_some()
                        || provider.source_provider_uuid.is_some()
                        || provider.bridge_type.is_some()
                        || path_key(Path::new(&profile.codex_home_path))? != imported_home_key
                }
                None => true,
            };
            if is_conflicting {
                conflicting.push(profile.profile_name.as_str());
            }
        }
        if !conflicting.is_empty() {
            return Err(profile_conflict(
                "config import would orphan or relocate local managed Codex profiles",
                conflicting,
            ));
        }
        Ok(())
    }

    pub(super) fn detach(conn: &Connection) -> AppResult<()> {
        for statement in [
            "DELETE FROM codex_managed_profiles",
            "DELETE FROM provider_models",
            "DELETE FROM provider_model_catalogs",
        ] {
            conn.execute(statement, [])
                .map_err(|error| db_err!("failed to detach local provider state: {error}"))?;
        }
        Ok(())
    }

    pub(super) fn restore(
        &self,
        conn: &Connection,
        eligible_provider_uuids: &HashSet<String>,
        provider_id_by_uuid: &HashMap<String, i64>,
    ) -> AppResult<()> {
        for catalog in &self.catalogs {
            let Some(provider_id) = retained_provider_id(
                &catalog.provider_uuid,
                eligible_provider_uuids,
                provider_id_by_uuid,
            ) else {
                continue;
            };
            conn.execute(
                r#"
INSERT INTO provider_model_catalogs(
  provider_id, protocol, stale, last_attempt_at, last_success_at, last_error_code
) VALUES (?1, ?2, 1, ?3, ?4, ?5)
"#,
                params![
                    provider_id,
                    catalog.protocol,
                    catalog.last_attempt_at,
                    catalog.last_success_at,
                    catalog.last_error_code
                ],
            )
            .map_err(|error| db_err!("failed to restore provider model catalog: {error}"))?;
        }

        for model in &self.models {
            let Some(provider_id) = retained_provider_id(
                &model.provider_uuid,
                eligible_provider_uuids,
                provider_id_by_uuid,
            ) else {
                continue;
            };
            let stale = i64::from(model.source == "discovered");
            conn.execute(
                r#"
INSERT INTO provider_models(
  model_uuid, provider_id, remote_model_id, source, stale, last_seen_at, created_at, updated_at,
  capabilities_configured, supported_reasoning_efforts_json, default_reasoning_effort,
  context_window
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
"#,
                params![
                    model.model_uuid,
                    provider_id,
                    model.remote_model_id,
                    model.source,
                    stale,
                    model.last_seen_at,
                    model.created_at,
                    model.updated_at,
                    i64::from(model.capabilities_configured),
                    model.supported_reasoning_efforts_json,
                    model.default_reasoning_effort,
                    model.context_window
                ],
            )
            .map_err(|error| db_err!("failed to restore provider model: {error}"))?;
        }

        for profile in &self.profiles {
            if retained_provider_id(
                &profile.provider_uuid,
                eligible_provider_uuids,
                provider_id_by_uuid,
            )
            .is_none()
            {
                return Err(AppError::new(
                    "CONFIG_IMPORT_MANAGED_PROFILE_CONFLICT",
                    "managed Codex profile provider disappeared during import",
                ));
            }
            conn.execute(
                r#"
INSERT INTO codex_managed_profiles(
  profile_uuid, profile_name, profile_name_key, model_uuid, codex_home_path,
  content_sha256, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
"#,
                params![
                    profile.profile_uuid,
                    profile.profile_name,
                    profile.profile_name_key,
                    profile.model_uuid,
                    profile.codex_home_path,
                    profile.content_sha256,
                    profile.created_at,
                    profile.updated_at
                ],
            )
            .map_err(|error| db_err!("failed to restore managed Codex profile: {error}"))?;
        }
        Ok(())
    }
}

pub(super) fn eligible_provider_uuids(providers: &[ProviderExport]) -> HashSet<String> {
    providers
        .iter()
        .filter(|provider| {
            provider.cli_key == "codex"
                && provider.source_provider_id.is_none()
                && provider.source_provider_uuid.is_none()
                && provider.bridge_type.is_none()
        })
        .filter_map(|provider| provider.provider_uuid.clone())
        .collect()
}

fn retained_provider_id(
    provider_uuid: &str,
    eligible_provider_uuids: &HashSet<String>,
    provider_id_by_uuid: &HashMap<String, i64>,
) -> Option<i64> {
    eligible_provider_uuids
        .contains(provider_uuid)
        .then(|| provider_id_by_uuid.get(provider_uuid).copied())
        .flatten()
}

fn profile_conflict<'a>(
    reason: &str,
    profile_names: impl IntoIterator<Item = &'a str>,
) -> AppError {
    let names = profile_names
        .into_iter()
        .take(20)
        .collect::<Vec<_>>()
        .join(", ");
    AppError::new(
        "CONFIG_IMPORT_MANAGED_PROFILE_CONFLICT",
        format!("{reason}: {names}"),
    )
}

fn path_key(path: &Path) -> AppResult<String> {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = normalized
        .to_str()
        .ok_or_else(|| {
            AppError::new(
                "SEC_INVALID_INPUT",
                "Codex home path must be valid UTF-8 to rebind managed profiles",
            )
        })?
        .replace('\\', "/");
    if cfg!(windows) {
        Ok(value.to_ascii_lowercase())
    } else {
        Ok(value)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;

    #[test]
    fn path_key_rejects_non_utf8_codex_home() {
        let path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        let error = path_key(&path).expect_err("non-UTF-8 paths must fail closed");
        assert_eq!(error.code(), "SEC_INVALID_INPUT");
    }
}

fn capture_catalogs(conn: &Connection) -> AppResult<Vec<CatalogRow>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT provider.provider_uuid, catalog.protocol, catalog.last_attempt_at,
       catalog.last_success_at, catalog.last_error_code
FROM provider_model_catalogs catalog
JOIN providers provider ON provider.id = catalog.provider_id
ORDER BY provider.provider_uuid ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare local catalog capture: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CatalogRow {
                provider_uuid: row.get(0)?,
                protocol: row.get(1)?,
                last_attempt_at: row.get(2)?,
                last_success_at: row.get(3)?,
                last_error_code: row.get(4)?,
            })
        })
        .map_err(|error| db_err!("failed to capture local catalogs: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read local catalog state: {error}"))
}

fn capture_models(conn: &Connection) -> AppResult<Vec<ModelRow>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT provider.provider_uuid, model.model_uuid, model.remote_model_id, model.source,
       model.last_seen_at, model.created_at, model.updated_at,
       model.capabilities_configured, model.supported_reasoning_efforts_json,
       model.default_reasoning_effort, model.context_window
FROM provider_models model
JOIN providers provider ON provider.id = model.provider_id
ORDER BY provider.provider_uuid ASC, model.model_uuid ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare local model capture: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ModelRow {
                provider_uuid: row.get(0)?,
                model_uuid: row.get(1)?,
                remote_model_id: row.get(2)?,
                source: row.get(3)?,
                last_seen_at: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                capabilities_configured: row.get::<_, i64>(7)? != 0,
                supported_reasoning_efforts_json: row.get(8)?,
                default_reasoning_effort: row.get(9)?,
                context_window: row.get(10)?,
            })
        })
        .map_err(|error| db_err!("failed to capture local models: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read local model state: {error}"))
}

fn capture_profiles(conn: &Connection) -> AppResult<Vec<ProfileRow>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT provider.provider_uuid, profile.profile_uuid, profile.profile_name,
       profile.profile_name_key, profile.model_uuid, profile.codex_home_path,
       profile.content_sha256, profile.created_at, profile.updated_at
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
ORDER BY profile.profile_name_key ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare managed profile capture: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ProfileRow {
                provider_uuid: row.get(0)?,
                profile_uuid: row.get(1)?,
                profile_name: row.get(2)?,
                profile_name_key: row.get(3)?,
                model_uuid: row.get(4)?,
                codex_home_path: row.get(5)?,
                content_sha256: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|error| db_err!("failed to capture managed profiles: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read managed profile state: {error}"))
}
