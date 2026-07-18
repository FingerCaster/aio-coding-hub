use crate::app_state::{ensure_db_ready, DbInitState};
use crate::blocking;
use crate::infra::config_migrate;
use crate::shared::error::AppError;
use crate::shared::fs::read_file_with_max_len;
use crate::shared::ipc_confirm::RiskyIpcConfirm;
use std::path::Path;

fn map_config_import_read_error(err: AppError) -> String {
    let message = err.to_string();
    if message.starts_with("SEC_INVALID_INPUT:") {
        message
    } else if let Some(message) = message.strip_prefix("INTERNAL_ERROR: ") {
        format!("SYSTEM_ERROR: failed to read config import file: {message}")
    } else {
        format!("SYSTEM_ERROR: failed to read config import file: {message}")
    }
}

fn read_config_import_bundle_with_max_len(
    file_path: &str,
    max_len: usize,
) -> Result<config_migrate::ConfigBundle, String> {
    let bytes = read_file_with_max_len(Path::new(file_path), max_len)
        .map_err(map_config_import_read_error)?;
    let raw = String::from_utf8(bytes)
        .map_err(|err| format!("SEC_INVALID_INPUT: config import file must be UTF-8: {err}"))?;
    serde_json::from_str(&raw)
        .map_err(|err| format!("SEC_INVALID_INPUT: invalid config import json: {err}"))
}

pub(crate) fn read_config_import_bundle(
    file_path: &str,
) -> Result<config_migrate::ConfigBundle, String> {
    read_config_import_bundle_with_max_len(
        file_path,
        config_migrate::CONFIG_BUNDLE_ENCODED_MAX_BYTES,
    )
}

struct CappedJsonWriter {
    max_len: usize,
    buffer: Vec<u8>,
}

impl CappedJsonWriter {
    fn new(max_len: usize) -> Self {
        Self {
            max_len,
            buffer: Vec::new(),
        }
    }

    fn into_string(self) -> Result<String, String> {
        if self.buffer.len() > self.max_len {
            return Err(format!(
                "SEC_INVALID_INPUT: config export exceeds max encoded size (max {} bytes)",
                self.max_len
            ));
        }
        String::from_utf8(self.buffer).map_err(|err| {
            format!("SYSTEM_ERROR: config export serialization produced invalid UTF-8: {err}")
        })
    }
}

impl std::io::Write for CappedJsonWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next_len = self
            .buffer
            .len()
            .checked_add(buf.len())
            .ok_or_else(|| std::io::Error::other("config export size overflow"))?;
        // Allow writing one extra byte so callers can distinguish exact overflow.
        if next_len > self.max_len.saturating_add(1) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "SEC_INVALID_INPUT: config export exceeds max encoded size (max {} bytes)",
                    self.max_len
                ),
            ));
        }
        self.buffer.extend_from_slice(buf);
        if self.buffer.len() > self.max_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "SEC_INVALID_INPUT: config export exceeds max encoded size (max {} bytes)",
                    self.max_len
                ),
            ));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialize_config_bundle_pretty_capped(
    bundle: &config_migrate::ConfigBundle,
    max_len: usize,
) -> Result<String, String> {
    let mut writer = CappedJsonWriter::new(max_len);
    serde_json::to_writer_pretty(&mut writer, bundle).map_err(|err| {
        let message = err.to_string();
        if message.contains("SEC_INVALID_INPUT:") {
            message
        } else {
            format!("SYSTEM_ERROR: failed to serialize config export: {message}")
        }
    })?;
    writer.into_string()
}

/// Production export path: capped pretty JSON then atomic write.
/// Shared by the Tauri command and regression tests so overflow never
/// overwrites the target.
pub(crate) fn write_config_export_bundle_to_path(
    file_path: &Path,
    bundle: &config_migrate::ConfigBundle,
) -> Result<(), String> {
    let content = serialize_config_bundle_pretty_capped(
        bundle,
        config_migrate::CONFIG_BUNDLE_ENCODED_MAX_BYTES,
    )?;
    crate::shared::fs::write_file_atomic(file_path, content.as_bytes())
        .map_err(|err| format!("SYSTEM_ERROR: failed to write config export file: {err}"))
}

/// Production export path used by the IPC command and end-to-end tests.
/// Bundle extraction stays coupled to the real DB/Skill filesystem exporter;
/// only serialization and atomic replacement are shared with the lower-level
/// writer test.
pub(crate) fn config_export_to_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &crate::db::Db,
    file_path: &Path,
) -> Result<(), String> {
    let bundle = config_migrate::config_export(app, db).map_err(|err| err.to_string())?;
    write_config_export_bundle_to_path(file_path, &bundle)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn config_export(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    file_path: String,
) -> Result<bool, String> {
    let file_path = file_path.trim().to_string();
    if file_path.is_empty() {
        return Err("SEC_INVALID_INPUT: file_path is required".to_string());
    }
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run("config_export", move || {
        config_export_to_path(&app, &db, Path::new(&file_path))?;
        Ok::<bool, String>(true)
    })
    .await;
    result.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn config_import(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    file_path: String,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<config_migrate::ConfigImportResult, String> {
    let file_path = file_path.trim().to_string();
    if file_path.is_empty() {
        return Err("SEC_INVALID_INPUT: file_path is required".to_string());
    }
    RiskyIpcConfirm::require(confirm, "config_import", file_path.clone())?;
    #[cfg(windows)]
    let app_for_wsl = app.clone();
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let result = blocking::run("config_import", move || {
        let bundle = read_config_import_bundle(&file_path)?;
        config_migrate::config_import(&app, &db, bundle)
    })
    .await
    .map_err(|err| -> String { err.into() })?;

    #[cfg(windows)]
    super::wsl::wsl_sync_trigger::trigger(app_for_wsl);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write temp file");
        (dir, path.to_string_lossy().to_string())
    }

    #[test]
    fn read_config_import_bundle_accepts_valid_json() {
        let raw = serde_json::json!({
            "schema_version": config_migrate::CONFIG_BUNDLE_SCHEMA_VERSION,
            "exported_at": "2026-05-19T00:00:00.000Z",
            "app_version": "0.0.0-test",
            "settings": "{}",
            "providers": [],
            "sort_modes": [],
            "sort_mode_active": {},
            "workspaces": [],
            "mcp_servers": [],
            "skill_repos": [],
            "installed_skills": [],
            "local_skills": []
        })
        .to_string();
        let (_dir, path) = write_temp_file("config.json", raw.as_bytes());

        let bundle = read_config_import_bundle_with_max_len(&path, 4096).expect("bundle");

        assert_eq!(
            bundle.schema_version,
            config_migrate::CONFIG_BUNDLE_SCHEMA_VERSION
        );
    }

    #[test]
    fn read_config_import_bundle_rejects_oversized_file() {
        let (_dir, path) = write_temp_file("config.json", b"{\"schema_version\":2}");

        let err = read_config_import_bundle_with_max_len(&path, 4)
            .err()
            .expect("oversized import file should fail");

        assert!(err.contains("SEC_INVALID_INPUT:"));
        assert!(err.contains("too large"));
    }

    #[test]
    fn read_config_import_bundle_rejects_invalid_utf8() {
        let (_dir, path) = write_temp_file("config.json", &[0xff]);

        let err = read_config_import_bundle_with_max_len(&path, 16)
            .err()
            .expect("invalid utf8 should fail");

        assert!(err.contains("SEC_INVALID_INPUT: config import file must be UTF-8"));
    }

    #[test]
    fn capped_export_serializer_accepts_exact_budget_and_rejects_overflow() {
        let bundle = config_migrate::ConfigBundle {
            schema_version: config_migrate::CONFIG_BUNDLE_SCHEMA_VERSION,
            exported_at: "2026-07-17T00:00:00.000Z".to_string(),
            app_version: "0.0.0-test".to_string(),
            settings: "{}".to_string(),
            providers: Vec::new(),
            sort_modes: Vec::new(),
            sort_mode_active: std::collections::HashMap::new(),
            workspaces: Vec::new(),
            mcp_servers: Vec::new(),
            skill_repos: Vec::new(),
            installed_skills: Some(Vec::new()),
            local_skills: Some(Vec::new()),
            image_gen_configs: None,
        };
        let exact = serialize_config_bundle_pretty_capped(&bundle, 10_000).expect("exact");
        assert!(!exact.is_empty());

        let overflow =
            serialize_config_bundle_pretty_capped(&bundle, 16).expect_err("overflow fails");
        assert!(overflow.contains("SEC_INVALID_INPUT:"));
        assert!(overflow.contains("max encoded size"));
    }

    #[test]
    fn production_export_path_preserves_sentinel_when_six_legal_skills_exceed_encoded_budget() {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("export.json");
        std::fs::write(&path, b"SENTINEL-BYTES").expect("write sentinel");

        // Six independently legal skills: each has SKILL.md + one large binary
        // with decoded total <= 8 MiB. Standard Base64 expands ~4/3, so six
        // near-8MiB payloads push pretty JSON past the shared 64 MiB budget.
        let skill_md = b"---\nname: big\ndescription: legal\n---\n";
        let large_raw = vec![b'x'; 8 * 1024 * 1024 - skill_md.len()];
        let large_b64 = BASE64.encode(&large_raw);
        let mut installed = Vec::new();
        for index in 0..6 {
            installed.push(config_migrate::InstalledSkillExport {
                skill_key: format!("skill-{index}"),
                name: format!("Skill {index}"),
                description: "legal large skill".to_string(),
                source_git_url: "https://example.invalid/repo".to_string(),
                source_branch: "main".to_string(),
                source_subdir: String::new(),
                enabled_in_workspaces: Vec::new(),
                files: vec![
                    config_migrate::SkillFileExport {
                        relative_path: "SKILL.md".to_string(),
                        content_base64: BASE64.encode(skill_md),
                    },
                    config_migrate::SkillFileExport {
                        relative_path: "blob.bin".to_string(),
                        content_base64: large_b64.clone(),
                    },
                ],
            });
        }
        let bundle = config_migrate::ConfigBundle {
            schema_version: config_migrate::CONFIG_BUNDLE_SCHEMA_VERSION,
            exported_at: "2026-07-17T00:00:00.000Z".to_string(),
            app_version: "0.0.0-test".to_string(),
            settings: "{}".to_string(),
            providers: Vec::new(),
            sort_modes: Vec::new(),
            sort_mode_active: std::collections::HashMap::new(),
            workspaces: Vec::new(),
            mcp_servers: Vec::new(),
            skill_repos: Vec::new(),
            installed_skills: Some(installed),
            local_skills: Some(Vec::new()),
            image_gen_configs: None,
        };

        let err = write_config_export_bundle_to_path(&path, &bundle)
            .expect_err("six legal large skills must exceed 64 MiB encoded budget");
        assert!(err.contains("SEC_INVALID_INPUT:"), "unexpected: {err}");
        assert_eq!(
            std::fs::read(&path).expect("read sentinel"),
            b"SENTINEL-BYTES"
        );
    }

    #[test]
    fn production_export_file_round_trips_through_bounded_import_reader() {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("export.json");
        let payload = b"arbitrary-legal-bytes\x00\xffsecret";
        let skill_md = b"---\nname: tiny\ndescription: roundtrip\n---\n";
        let installed = vec![config_migrate::InstalledSkillExport {
            skill_key: "tiny-skill".to_string(),
            name: "Tiny".to_string(),
            description: "roundtrip".to_string(),
            source_git_url: "https://example.invalid/repo".to_string(),
            source_branch: "main".to_string(),
            source_subdir: String::new(),
            enabled_in_workspaces: Vec::new(),
            files: vec![
                config_migrate::SkillFileExport {
                    relative_path: "SKILL.md".to_string(),
                    content_base64: BASE64.encode(skill_md),
                },
                config_migrate::SkillFileExport {
                    relative_path: "data.bin".to_string(),
                    content_base64: BASE64.encode(payload),
                },
            ],
        }];
        let bundle = config_migrate::ConfigBundle {
            schema_version: config_migrate::CONFIG_BUNDLE_SCHEMA_VERSION,
            exported_at: "2026-07-17T00:00:00.000Z".to_string(),
            app_version: "0.0.0-test".to_string(),
            settings: "{}".to_string(),
            providers: Vec::new(),
            sort_modes: Vec::new(),
            sort_mode_active: std::collections::HashMap::new(),
            workspaces: Vec::new(),
            mcp_servers: Vec::new(),
            skill_repos: Vec::new(),
            installed_skills: Some(installed),
            local_skills: Some(Vec::new()),
            image_gen_configs: None,
        };

        write_config_export_bundle_to_path(&path, &bundle).expect("export succeeds");
        let reloaded = read_config_import_bundle(path.to_str().expect("utf8 path"))
            .expect("bounded import reader accepts production export");
        let skill = reloaded
            .installed_skills
            .as_ref()
            .expect("skills")
            .iter()
            .find(|s| s.skill_key == "tiny-skill")
            .expect("skill present");
        let data = skill
            .files
            .iter()
            .find(|f| f.relative_path == "data.bin")
            .expect("data file");
        let decoded = BASE64
            .decode(data.content_base64.as_bytes())
            .expect("decode");
        assert_eq!(decoded, payload);
    }
}
