//! Resolve native target identity through model documents. Settings use this identity
//! but own their separate config/Agent paths. Never read auth or .env here.
use super::document::digest_bytes;
use crate::domain::native_cli::{
    NativeClient, NativeFormat, NativeTarget, NativeTargetMode, NativeTargetSelection,
};
use crate::shared::error::{AppError, AppResult};
use std::path::{Component, Path, PathBuf};

fn invalid(message: &str) -> AppError {
    AppError::new("NATIVE_TARGET_INVALID", message)
}

pub(crate) fn validate_selection(selection: &NativeTargetSelection) -> AppResult<()> {
    match selection.mode {
        NativeTargetMode::Default
            if selection.agent_dir.is_none() && selection.profile.is_none() =>
        {
            Ok(())
        }
        NativeTargetMode::Custom if selection.profile.is_none() => {
            let path = selection
                .agent_dir
                .as_deref()
                .ok_or_else(|| invalid("Custom target requires an absolute agent directory"))?;
            validate_absolute(Path::new(path))
        }
        NativeTargetMode::Profile
            if selection.client == NativeClient::Omp && selection.agent_dir.is_none() =>
        {
            validate_profile(
                selection
                    .profile
                    .as_deref()
                    .ok_or_else(|| invalid("Choose an OMP profile"))?,
            )
        }
        _ => Err(invalid("Directory mode and profile parameters disagree")),
    }
}

pub(crate) fn validate_profile(profile: &str) -> AppResult<()> {
    let first = profile.as_bytes().first().copied().unwrap_or(0);
    let base = profile.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit());
    if profile == "default"
        || profile.len() > 64
        || !first.is_ascii_lowercase() && !first.is_ascii_digit()
        || !profile
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        || profile.ends_with('.')
        || reserved
    {
        return Err(invalid(
            "Invalid OMP profile; choose Default for the default profile",
        ));
    }
    Ok(())
}

fn validate_absolute(path: &Path) -> AppResult<()> {
    if !path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || path.as_os_str().to_string_lossy().contains('\0')
    {
        return Err(invalid(
            "Agent directory must be absolute and contain no parent traversal",
        ));
    }
    #[cfg(windows)]
    if let Some(Component::Prefix(prefix)) = path.components().next() {
        if !matches!(
            prefix.kind(),
            std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
        ) {
            return Err(invalid("Network/device targets are not supported"));
        }
    }
    Ok(())
}

pub(crate) fn canonical_directory(path: &Path) -> AppResult<PathBuf> {
    validate_absolute(path)?;
    let mut missing = Vec::new();
    let mut anchor = path;
    loop {
        match std::fs::symlink_metadata(anchor) {
            Ok(meta) => {
                if !meta.is_dir() && !meta.file_type().is_symlink() {
                    return Err(invalid("Agent path has a non-directory ancestor"));
                }
                let mut canonical = std::fs::canonicalize(anchor)
                    .map_err(|_| invalid("Cannot resolve agent directory"))?;
                if !canonical.is_dir() {
                    return Err(invalid("Agent directory is not a directory"));
                }
                for part in missing.iter().rev() {
                    canonical.push(part);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    anchor
                        .file_name()
                        .ok_or_else(|| invalid("Cannot resolve agent directory"))?
                        .to_os_string(),
                );
                anchor = anchor
                    .parent()
                    .ok_or_else(|| invalid("Cannot resolve agent directory"))?;
            }
            Err(_) => return Err(invalid("Cannot inspect agent directory")),
        }
    }
}

fn expanded_env_dir(home: &Path, raw: &str) -> AppResult<PathBuf> {
    let path = if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    };
    validate_absolute(&path)?;
    Ok(path)
}

pub(crate) fn omp_root(home: &Path, config_dir: Option<&str>) -> AppResult<PathBuf> {
    let directory = config_dir.filter(|s| !s.is_empty()).unwrap_or(".omp");
    let path = Path::new(directory);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid(
            "PI_CONFIG_DIR must be an unambiguous home-relative directory",
        ));
    }
    Ok(home.join(path))
}

/// An explicit default selection must not inherit the agent path propagated by
/// OMP setProfile. Named profiles are selected by the user, never guessed from
/// the launching shell. Pi does not interpret OMP/PI profile variables.
pub(crate) fn resolve_with_profiles(
    home: &Path,
    selection: &NativeTargetSelection,
    agent_env: Option<&str>,
    config_env: Option<&str>,
    omp_profile_env: Option<&str>,
    pi_profile_env: Option<&str>,
) -> AppResult<NativeTarget> {
    fn profile(raw: Option<&str>) -> AppResult<Option<&str>> {
        match raw
            .map(str::trim)
            .filter(|p| !p.is_empty() && *p != "default")
        {
            Some(name) => {
                validate_profile(name)?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }
    let mut baseline = agent_env;
    if selection.client == NativeClient::Omp && selection.mode == NativeTargetMode::Default {
        if let Some(agent) = agent_env.filter(|s| !s.is_empty()) {
            let active = profile(omp_profile_env.or(pi_profile_env))?;
            let inherited = active.or(profile(pi_profile_env).ok().flatten());
            if let Some(name) = inherited {
                let inherited_dir = omp_root(home, config_env)?
                    .join("profiles")
                    .join(name)
                    .join("agent");
                if path_identity(&canonical_directory(&expanded_env_dir(home, agent)?)?)
                    == path_identity(&canonical_directory(&inherited_dir)?)
                {
                    baseline = None;
                }
            }
        }
    }
    resolve(home, selection, baseline, config_env)
}

pub(crate) fn resolve(
    home: &Path,
    selection: &NativeTargetSelection,
    agent_env: Option<&str>,
    config_env: Option<&str>,
) -> AppResult<NativeTarget> {
    validate_selection(selection)?;
    let (directory, source) = match selection.mode {
        NativeTargetMode::Custom => (
            PathBuf::from(selection.agent_dir.as_ref().expect("validated")),
            "custom",
        ),
        NativeTargetMode::Profile => (
            omp_root(home, config_env)?
                .join("profiles")
                .join(selection.profile.as_ref().expect("validated"))
                .join("agent"),
            "profile",
        ),
        NativeTargetMode::Default => {
            if let Some(directory) = agent_env.filter(|s| !s.is_empty()) {
                (expanded_env_dir(home, directory)?, "environment")
            } else {
                (
                    match selection.client {
                        NativeClient::Pi => home.join(".pi").join("agent"),
                        NativeClient::Omp => omp_root(home, config_env)?.join("agent"),
                    },
                    "default",
                )
            }
        }
    };
    let agent_dir = canonical_directory(&directory)?;
    let (models_path, format, shadowed_files) = selected_model_file(selection.client, &agent_dir)?;
    let stable_path = path_identity(&agent_dir);
    let identity = format!(
        "{}\nnative\n{}\n{}\n{}",
        selection.client.as_str(),
        selection.profile.as_deref().unwrap_or(""),
        stable_path,
        path_identity(&models_path)
    );
    let mut target = NativeTarget {
        target_id: digest_bytes(identity.as_bytes()), client: selection.client, environment: "native".into(),
        profile: selection.profile.clone(), agent_dir: agent_dir.to_string_lossy().into_owned(), models_path: models_path.to_string_lossy().into_owned(),
        source: source.into(), format, selected: false, writable: format != NativeFormat::LegacyJson,
        issue: (format == NativeFormat::LegacyJson).then(|| "NATIVE_LEGACY_READ_ONLY: Migrate OMP models.json with the native CLI before editing".into()), shadowed_files
    };
    if let Ok(metadata) = std::fs::symlink_metadata(&models_path) {
        if is_link(&metadata) || !metadata.is_file() {
            target.writable = false;
            target.issue =
                Some("NATIVE_TARGET_UNSAFE: Model target is not an ordinary file".into());
        }
    }
    Ok(target)
}

pub(crate) fn path_identity(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        text.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        text
    }
}

pub(crate) fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

pub(crate) fn selected_model_file(
    client: NativeClient,
    agent_dir: &Path,
) -> AppResult<(PathBuf, NativeFormat, Vec<String>)> {
    let candidates: &[(&str, NativeFormat)] = match client {
        NativeClient::Pi => &[("models.json", NativeFormat::Jsonc)],
        NativeClient::Omp => &[
            ("models.yml", NativeFormat::Yaml),
            ("models.yaml", NativeFormat::Yaml),
            ("models.json", NativeFormat::LegacyJson),
        ],
    };
    let mut found = Vec::new();
    for (name, format) in candidates {
        let path = agent_dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => found.push((path, *format)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid("Cannot inspect native model file priority")),
        }
    }
    if found.is_empty() {
        return Ok((agent_dir.join(candidates[0].0), candidates[0].1, Vec::new()));
    }
    let (selected, format) = found.remove(0);
    Ok((
        selected,
        format,
        found
            .into_iter()
            .map(|(p, _)| p.to_string_lossy().into_owned())
            .collect(),
    ))
}

pub(crate) fn discover_profiles(
    home: &Path,
    config_env: Option<&str>,
) -> AppResult<Vec<NativeTargetSelection>> {
    let root = omp_root(home, config_env)?.join("profiles");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(invalid("Cannot enumerate OMP profiles")),
    };
    let mut profiles = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= 256 {
            return Err(invalid("Too many OMP profiles to enumerate safely"));
        }
        let entry = entry.map_err(|_| invalid("Cannot inspect OMP profile"))?;
        let kind = entry
            .file_type()
            .map_err(|_| invalid("Cannot inspect OMP profile"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() && validate_profile(&name).is_ok() {
            profiles.push(NativeTargetSelection {
                client: NativeClient::Omp,
                mode: NativeTargetMode::Profile,
                agent_dir: None,
                profile: Some(name),
            });
        }
    }
    profiles.sort_by(|a, b| a.profile.cmp(&b.profile));
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_custom_profile_and_yaml_priority_are_distinct() {
        let home = tempfile::tempdir().unwrap();
        let pi = resolve(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Pi),
            None,
            None,
        )
        .unwrap();
        let omp = resolve(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Omp),
            None,
            None,
        )
        .unwrap();
        assert_ne!(pi.target_id, omp.target_id);
        assert!(Path::new(&pi.models_path).ends_with(".pi/agent/models.json"));
        std::fs::create_dir_all(&omp.agent_dir).unwrap();
        std::fs::write(Path::new(&omp.agent_dir).join("models.json"), b"{}").unwrap();
        assert_eq!(
            resolve(
                home.path(),
                &NativeTargetSelection::default_for(NativeClient::Omp),
                None,
                None
            )
            .unwrap()
            .format,
            NativeFormat::LegacyJson
        );
        std::fs::write(Path::new(&omp.agent_dir).join("models.yaml"), b"{}").unwrap();
        let yaml = resolve(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Omp),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.models_path.ends_with("models.yaml"));
        std::fs::write(Path::new(&omp.agent_dir).join("models.yml"), b"{}").unwrap();
        let yml = resolve(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Omp),
            None,
            None,
        )
        .unwrap();
        assert!(yml.models_path.ends_with("models.yml"));
        assert_eq!(yml.shadowed_files.len(), 2);
        let profile = NativeTargetSelection {
            client: NativeClient::Omp,
            mode: NativeTargetMode::Profile,
            agent_dir: None,
            profile: Some("work".into()),
        };
        let target = resolve(
            home.path(),
            &profile,
            Some("ignored-relative-override"),
            Some(".custom-omp"),
        )
        .unwrap();
        assert!(Path::new(&target.agent_dir).ends_with(".custom-omp/profiles/work/agent"));
    }
    #[test]
    fn rejects_ambiguous_targets_and_reserved_profiles() {
        for name in ["", "default", "../x", "con", "lpt0.txt", "UPPER", "name."] {
            assert!(validate_profile(name).is_err());
        }
        let home = tempfile::tempdir().unwrap();
        assert!(resolve(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Pi),
            Some("relative"),
            None
        )
        .is_err());
        assert!(omp_root(home.path(), Some("../escape")).is_err());
    }
    #[test]
    fn explicit_default_drops_only_inherited_profile_agent_override() {
        let home = tempfile::tempdir().unwrap();
        let inherited = home.path().join(".custom-omp/profiles/work/agent");
        let inherited = inherited.to_str().unwrap();
        let selection = NativeTargetSelection::default_for(NativeClient::Omp);
        for (omp, pi) in [
            (Some("work"), None),
            (None, Some("work")),
            (Some(""), Some("work")),
            (Some("default"), Some("work")),
        ] {
            let target = resolve_with_profiles(
                home.path(),
                &selection,
                Some(inherited),
                Some(".custom-omp"),
                omp,
                pi,
            )
            .unwrap();
            assert!(Path::new(&target.agent_dir).ends_with(".custom-omp/agent"));
            assert_eq!(target.source, "default");
            assert_eq!(target.profile, None);
        }
        let custom = home.path().join("custom-agent");
        let target = resolve_with_profiles(
            home.path(),
            &selection,
            custom.to_str(),
            None,
            Some("work"),
            None,
        )
        .unwrap();
        assert!(Path::new(&target.agent_dir).ends_with("custom-agent"));
        assert_eq!(target.source, "environment");
        let pi = resolve_with_profiles(
            home.path(),
            &NativeTargetSelection::default_for(NativeClient::Pi),
            Some(inherited),
            Some(".custom-omp"),
            Some("work"),
            None,
        )
        .unwrap();
        assert!(Path::new(&pi.agent_dir).ends_with("profiles/work/agent"));
    }
}
