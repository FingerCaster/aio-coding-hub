//! Scoped OMP settings and Agent documents. Callers hold the shared target lock.
mod agents;
#[cfg(test)]
mod tests;

use super::{document, files, targets, NativeTargetSession};
use crate::domain::native_cli::{NativeClient, NativeFormat, NativeTarget};
use crate::domain::omp_settings::*;
use crate::shared::error::{AppError, AppResult};
pub(crate) use agents::{read_agent, save_agent};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: usize = 1024 * 1024;

fn conflict() -> AppError {
    AppError::new(
        "OMP_SETTINGS_CONFLICT",
        "OMP 配置已被其他程序修改，或文件优先级发生变化。请重新读取后保存",
    )
}

pub(super) fn safe_file(path: &Path) -> AppResult<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if targets::is_link(&meta) || !meta.is_file() => {
            Err(invalid("配置或 Agent 路径不是普通文件，无法安全读取/写入"))
        }
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(invalid("无法检查 OMP 配置文件")),
    }
}

pub(super) fn check_target(target: &NativeTarget, writing: bool) -> AppResult<()> {
    if target.client != NativeClient::Omp || (writing && !target.writable) {
        return Err(invalid("请选择可编辑的 OMP 原生配置目标"));
    }
    let directory = targets::canonical_directory(Path::new(&target.agent_dir))?;
    if targets::path_identity(&directory) != targets::path_identity(Path::new(&target.agent_dir)) {
        return Err(conflict());
    }
    Ok(())
}

fn config_path(target: &NativeTarget) -> AppResult<(PathBuf, bool)> {
    check_target(target, false)?;
    let dir = Path::new(&target.agent_dir);
    for (name, legacy) in [
        ("config.yml", false),
        ("config.yaml", false),
        ("settings.json", true),
    ] {
        let path = dir.join(name);
        if safe_file(&path)? {
            return Ok((path, legacy));
        }
    }
    Ok((dir.join("config.yml"), false))
}

pub(super) fn revision(path: &Path, bytes: Option<&[u8]>) -> String {
    document::digest_bytes(
        format!(
            "{}:{}",
            targets::path_identity(path),
            bytes
                .map(document::digest_bytes)
                .unwrap_or_else(|| "missing".into())
        )
        .as_bytes(),
    )
}

struct ConfigDocument {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    root: Value,
    revision: String,
    legacy: bool,
}

fn load(target: &NativeTarget) -> AppResult<ConfigDocument> {
    let (path, legacy) = config_path(target)?;
    let bytes = crate::shared::fs::read_optional_file_with_max_len(&path, MAX_CONFIG_BYTES)
        .map_err(|_| invalid("无法读取 OMP 设置，或文件超过 1 MiB；未修改原文件"))?;
    let root = match &bytes {
        Some(b) => document::parse_mapping(
            if legacy {
                NativeFormat::LegacyJson
            } else {
                NativeFormat::Yaml
            },
            b,
        )
        .map_err(|_| invalid("OMP 设置存在无效、重复或不支持的 YAML/JSON 语法；未修改原文件"))?,
        None => json!({}),
    };
    Ok(ConfigDocument {
        revision: revision(&path, bytes.as_deref()),
        path,
        bytes,
        root,
        legacy,
    })
}

pub(crate) fn read(
    target: &NativeTarget,
    session: &NativeTargetSession<'_>,
) -> AppResult<OmpSettingsSnapshot> {
    let doc = load(target)?;
    let mut warnings = Vec::new();
    if doc.legacy {
        warnings.push("发现旧版 settings.json，只读。请先由 OMP 完成原生迁移后刷新。".into());
    }
    if !target.writable {
        warnings.push("当前原生目标只读，请检查目录与目标状态。".into());
    }
    let mut models = Vec::new();
    match session.snapshot() {
        Ok(snapshot) => {
            for (provider, node) in snapshot.providers() {
                if let Some(items) = node.get("models").and_then(Value::as_array) {
                    for item in items {
                        let Some(id) = item.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        let label = item.get("name").and_then(Value::as_str).unwrap_or(id);
                        let levels = item
                            .get("thinking")
                            .map(thinking_levels)
                            .unwrap_or_default();
                        models.push(OmpModelOption {
                            selector: format!("{provider}/{id}"),
                            label: format!("{label} · {provider}"),
                            source: "原生配置 / AIO 入口".into(),
                            thinking_levels: levels,
                        });
                    }
                }
            }
        }
        Err(_) => warnings.push(
            "原生模型文件无法读取。模型选择仅显示内置目录；可保留或手动填写原有模型。".into(),
        ),
    }
    let values = project(&doc.root);
    let mut agents = agents::list_agents(target, &mut warnings)?;
    // Keep settings for absent/plugin agents accessible without executing discovery plugins.
    let mut names: HashSet<String> = agents.iter().map(|a| a.name.clone()).collect();
    for key in RECORD_KEYS.iter().filter(|k| k.starts_with("task.")) {
        if let Some(map) = values.get(*key).and_then(Value::as_object) {
            for name in map.keys() {
                if names.insert(name.clone()) {
                    agents.push(agents::configured_agent(name));
                }
            }
        }
    }
    if let Some(disabled) = values.get("task.disabledAgents").and_then(Value::as_array) {
        for name in disabled.iter().filter_map(Value::as_str) {
            if names.insert(name.into()) {
                agents.push(agents::configured_agent(name));
            }
        }
    }
    Ok(OmpSettingsSnapshot {
        target_id: target.target_id.clone(),
        config_path: doc.path.to_string_lossy().into(),
        agents_dir: Path::new(&target.agent_dir)
            .join("agents")
            .to_string_lossy()
            .into(),
        revision: doc.revision,
        writable: target.writable && !doc.legacy,
        warnings,
        fields: fields(),
        values,
        models,
        agents,
    })
}

fn thinking_levels(thinking: &Value) -> Vec<String> {
    if let Some(levels) = thinking
        .get("efforts")
        .or_else(|| thinking.get("levels"))
        .and_then(Value::as_array)
    {
        return levels
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| EFFORTS.contains(s))
            .map(str::to_owned)
            .collect();
    }
    match (
        thinking.get("minLevel").and_then(Value::as_str),
        thinking.get("maxLevel").and_then(Value::as_str),
    ) {
        (Some(min), Some(max)) => match (
            EFFORTS.iter().position(|s| *s == min),
            EFFORTS.iter().position(|s| *s == max),
        ) {
            (Some(min), Some(max)) if min <= max => {
                EFFORTS[min..=max].iter().map(|s| (*s).into()).collect()
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

pub(crate) fn save(
    target: &NativeTarget,
    input: &OmpSettingsSaveInput,
) -> AppResult<OmpSettingsWriteResult> {
    check_target(target, true)?;
    if input.patches.len() > 512 {
        return Err(invalid("单次设置变更过多"));
    }
    let current = load(target)?;
    if current.legacy {
        return Err(invalid("旧版设置只读，请先使用 OMP 完成迁移"));
    }
    if current.revision != input.expected_revision {
        return Err(conflict());
    }
    let mut root = current.root.clone();
    let mut seen = HashSet::new();
    for patch in &input.patches {
        validate_patch(patch)?;
        if !seen.insert(&patch.path) {
            return Err(invalid("重复的设置变更"));
        }
        patch_value(&mut root, &patch.path, &patch.value)?;
    }
    if root == current.root {
        return Ok(OmpSettingsWriteResult {
            target_id: target.target_id.clone(),
            revision: current.revision,
            changed: false,
            backup_path: None,
        });
    }
    let output = serde_yaml::to_string(&root).map_err(|_| invalid("无法序列化 OMP 设置"))?;
    if output.len() > MAX_CONFIG_BYTES
        || document::parse_mapping(NativeFormat::Yaml, output.as_bytes())? != root
    {
        return Err(invalid("OMP 设置无法安全写回"));
    }
    files::create_directory(Path::new(&target.agent_dir))?;
    let backup = current
        .bytes
        .as_ref()
        .map(|b| files::backup(&current.path, b))
        .transpose()?;
    files::atomic_write(
        &current.path,
        output.as_bytes(),
        current.bytes.is_some(),
        || {
            check_target(target, true)?;
            if load(target)?.revision != current.revision {
                return Err(conflict());
            }
            Ok(())
        },
    )?;
    Ok(OmpSettingsWriteResult {
        target_id: target.target_id.clone(),
        revision: revision(&current.path, Some(output.as_bytes())),
        changed: true,
        backup_path: backup.map(|p| p.to_string_lossy().into()),
    })
}
