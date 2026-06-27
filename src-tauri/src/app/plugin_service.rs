use crate::domain::plugins::{
    permission_risk, validate_manifest, PluginCompatibilitySummary, PluginDetail,
    PluginHookLifecycleSummary, PluginInstallPreview, PluginInstallSource, PluginLifecycleChange,
    PluginLifecycleNotice, PluginManifest, PluginPermissionLifecycleChange,
    PluginPermissionLifecycleSummary, PluginPermissionRisk, PluginRuntime,
    PluginRuntimeLifecycleSummary, PluginStatus, PluginTrustSummary, PluginUpdateDiff,
};
use crate::infra::plugins::{package, repository, signing};
use crate::shared::error::{AppError, AppResult};
use rusqlite::OptionalExtension;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

const OFFICIAL_PRIVACY_FILTER_ID: &str = "official.privacy-filter";

pub(crate) fn list_plugins(db: &crate::db::Db) -> AppResult<Vec<crate::plugins::PluginSummary>> {
    repository::list_plugins(db)
}

pub(crate) fn get_plugin_detail(db: &crate::db::Db, plugin_id: &str) -> AppResult<PluginDetail> {
    detail_with_config_defaults_for_db(db, repository::get_plugin(db, plugin_id)?)
}

pub(crate) fn enabled_plugins_for_gateway(db: &crate::db::Db) -> AppResult<Vec<PluginDetail>> {
    match enabled_plugins_for_gateway_once(db) {
        Ok(plugins) => Ok(plugins),
        Err(err) if is_missing_plugin_table_error(&err) => {
            tracing::warn!(
                error = %err,
                "plugin schema missing while loading gateway plugins; repairing runtime schema"
            );
            crate::db::ensure_runtime_schema(db)?;
            enabled_plugins_for_gateway_once(db)
        }
        Err(err) => Err(err),
    }
}

fn enabled_plugins_for_gateway_once(db: &crate::db::Db) -> AppResult<Vec<PluginDetail>> {
    let mut out = Vec::new();
    for summary in repository::list_plugins(db)? {
        if summary.status != PluginStatus::Enabled {
            continue;
        }
        let detail = detail_with_config_defaults_for_db(
            db,
            repository::get_plugin(db, &summary.plugin_id)?,
        )?;
        if let Err(err) = validate_manifest(&detail.manifest, env!("CARGO_PKG_VERSION")) {
            tracing::warn!(
                plugin_id = %summary.plugin_id,
                error = ?err,
                "skipping enabled plugin with invalid manifest"
            );
            continue;
        }
        out.push(detail);
    }
    Ok(out)
}

fn is_missing_plugin_table_error(err: &AppError) -> bool {
    let message = err.to_string();
    message.contains("no such table: plugins") || message.contains("no such table: plugin_")
}

pub(crate) fn install_official_plugin(
    db: &crate::db::Db,
    plugin_id: &str,
    official_resource_root: &Path,
    host_version: &str,
    installed_root: &Path,
) -> AppResult<PluginDetail> {
    let fixture = crate::app::plugins::official::official_plugin_from_root(
        plugin_id,
        official_resource_root,
    )?;
    let installed_dir = crate::app::plugins::official_assets::materialize_official_plugin(
        plugin_id,
        &fixture.root_dir,
        installed_root,
        &fixture.manifest.version,
    )?;
    install_plugin_manifest(
        db,
        fixture.manifest.clone(),
        PluginInstallSource::Official,
        Some(installed_dir.to_string_lossy().to_string()),
        host_version,
    )?;
    repository::save_plugin_config(
        db,
        plugin_id,
        fixture.manifest.config_version.unwrap_or(1),
        &fixture.default_config,
        &[],
    )?;
    let detail =
        repository::save_plugin_permissions(db, plugin_id, &fixture.manifest.permissions, &[])?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.official.installed",
        "low",
        "Official plugin installed",
        serde_json::json!({ "source": "official" }),
    )?;
    Ok(detail)
}

pub(crate) fn install_plugin_manifest(
    db: &crate::db::Db,
    manifest: PluginManifest,
    install_source: PluginInstallSource,
    installed_dir: Option<String>,
    host_version: &str,
) -> AppResult<PluginDetail> {
    validate_manifest(&manifest, host_version)?;
    validate_reserved_official_source(&manifest, install_source)?;
    let plugin_id = manifest.id.clone();
    let requested_permissions = manifest.permissions.clone();
    let detail = repository::insert_plugin(
        db,
        repository::InsertPluginInput {
            manifest,
            install_source,
            status: PluginStatus::Disabled,
            installed_dir,
        },
    )?;
    let detail = if install_source == PluginInstallSource::Official {
        detail
    } else {
        repository::save_plugin_permissions(db, &plugin_id, &[], &requested_permissions)?
    };
    append_audit(
        db,
        Some(plugin_id.clone()),
        "plugin.installed",
        "low",
        "Plugin installed",
        serde_json::json!({ "source": install_source.as_str() }),
    )?;
    Ok(detail)
}

pub(crate) fn install_plugin_from_local_package(
    db: &crate::db::Db,
    package_path: &Path,
    cache_dir: &Path,
    installed_root: &Path,
    host_version: &str,
) -> AppResult<PluginDetail> {
    install_plugin_from_local_package_with_policy(
        db,
        package_path,
        cache_dir,
        installed_root,
        host_version,
        LocalPackageInstallPolicy::default(),
    )
}

#[derive(Debug, Clone)]
pub(crate) struct LocalPackageInstallPolicy {
    pub(crate) expected_plugin_id: Option<String>,
    pub(crate) expected_checksum: Option<String>,
    pub(crate) signature: Option<String>,
    pub(crate) public_key: Option<String>,
    pub(crate) allow_unsigned: bool,
    pub(crate) developer_mode: bool,
    pub(crate) install_source: PluginInstallSource,
    pub(crate) remote_source_url: Option<String>,
    pub(crate) market_source_url: Option<String>,
}

impl Default for LocalPackageInstallPolicy {
    fn default() -> Self {
        Self {
            expected_plugin_id: None,
            expected_checksum: None,
            signature: None,
            public_key: None,
            allow_unsigned: false,
            developer_mode: false,
            install_source: PluginInstallSource::Local,
            remote_source_url: None,
            market_source_url: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RemotePackageInstallPolicy {
    pub(crate) install_source: PluginInstallSource,
    pub(crate) expected_plugin_id: String,
    pub(crate) expected_checksum: String,
    pub(crate) signature: Option<String>,
    pub(crate) public_key: Option<String>,
    pub(crate) market_source_url: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct PackageTrust {
    signature_verified: bool,
}

fn lifecycle_notice(
    severity: &str,
    code: &str,
    message: impl Into<String>,
) -> PluginLifecycleNotice {
    PluginLifecycleNotice {
        severity: severity.to_string(),
        code: code.to_string(),
        message: message.into(),
    }
}

fn cleanup_staging_dir(staging_root: &Path, staging_dir: &Path) {
    let _ = std::fs::remove_dir_all(staging_dir);
    let _ = std::fs::remove_dir(staging_root);
}

fn app_error_message(error: &AppError) -> String {
    let rendered = error.to_string();
    rendered
        .split_once(':')
        .map_or(rendered.clone(), |(_, message)| message.trim().to_string())
}

fn compare_version_direction(from: &str, to: &str) -> String {
    match (parse_semver_precedence(from), parse_semver_precedence(to)) {
        (Some(left), Some(right)) => match compare_semver_precedence(&left, &right) {
            Ordering::Less => "upgrade".to_string(),
            Ordering::Greater => "downgrade".to_string(),
            Ordering::Equal => "same".to_string(),
        },
        _ => "unknown".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemverPrecedence {
    core: (u64, u64, u64),
    prerelease: Vec<SemverPrereleaseIdentifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SemverPrereleaseIdentifier {
    Numeric(u64),
    Text(String),
}

fn parse_semver_precedence(version: &str) -> Option<SemverPrecedence> {
    let version = version.trim();
    let version = version.split_once('+').map_or(version, |(left, _)| left);
    let (core, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(core, prerelease)| {
            (core, Some(prerelease))
        });
    let mut core_parts = core.split('.');
    let major = parse_semver_core_number(core_parts.next()?)?;
    let minor = parse_semver_core_number(core_parts.next()?)?;
    let patch = parse_semver_core_number(core_parts.next()?)?;
    if core_parts.next().is_some() {
        return None;
    }
    let prerelease = match prerelease {
        Some(raw) => parse_semver_prerelease(raw)?,
        None => Vec::new(),
    };
    Some(SemverPrecedence {
        core: (major, minor, patch),
        prerelease,
    })
}

fn parse_semver_core_number(value: &str) -> Option<u64> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return None;
    }
    value.parse::<u64>().ok()
}

fn parse_semver_prerelease(raw: &str) -> Option<Vec<SemverPrereleaseIdentifier>> {
    raw.split('.')
        .map(|identifier| {
            if identifier.is_empty()
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return None;
            }
            if identifier.bytes().all(|byte| byte.is_ascii_digit()) {
                if identifier.len() > 1 && identifier.starts_with('0') {
                    return None;
                }
                return identifier
                    .parse::<u64>()
                    .ok()
                    .map(SemverPrereleaseIdentifier::Numeric);
            }
            Some(SemverPrereleaseIdentifier::Text(identifier.to_string()))
        })
        .collect()
}

fn compare_semver_precedence(left: &SemverPrecedence, right: &SemverPrecedence) -> Ordering {
    let core_order = left.core.cmp(&right.core);
    if core_order != Ordering::Equal {
        return core_order;
    }
    match (left.prerelease.is_empty(), right.prerelease.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => compare_prerelease_identifiers(&left.prerelease, &right.prerelease),
    }
}

fn compare_prerelease_identifiers(
    left: &[SemverPrereleaseIdentifier],
    right: &[SemverPrereleaseIdentifier],
) -> Ordering {
    for (left_identifier, right_identifier) in left.iter().zip(right.iter()) {
        let order = match (left_identifier, right_identifier) {
            (
                SemverPrereleaseIdentifier::Numeric(left_number),
                SemverPrereleaseIdentifier::Numeric(right_number),
            ) => left_number.cmp(right_number),
            (SemverPrereleaseIdentifier::Numeric(_), SemverPrereleaseIdentifier::Text(_)) => {
                Ordering::Less
            }
            (SemverPrereleaseIdentifier::Text(_), SemverPrereleaseIdentifier::Numeric(_)) => {
                Ordering::Greater
            }
            (
                SemverPrereleaseIdentifier::Text(left_text),
                SemverPrereleaseIdentifier::Text(right_text),
            ) => left_text.cmp(right_text),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

fn runtime_lifecycle_summary(manifest: &PluginManifest) -> PluginRuntimeLifecycleSummary {
    match &manifest.runtime {
        PluginRuntime::ExtensionHost { .. } => PluginRuntimeLifecycleSummary {
            kind: "extensionHost".to_string(),
            label: "Extension Host".to_string(),
            supported: false,
            blocking_reasons: vec![lifecycle_notice(
                "warn",
                "PLUGIN_EXTENSION_HOST_NOT_WIRED",
                "extension host runtime execution is not wired in this release",
            )],
        },
        PluginRuntime::DeclarativeRules { .. } => PluginRuntimeLifecycleSummary {
            kind: "declarativeRules".to_string(),
            label: "Declarative Rules".to_string(),
            supported: true,
            blocking_reasons: Vec::new(),
        },
        PluginRuntime::Native { engine } if manifest.id == OFFICIAL_PRIVACY_FILTER_ID => {
            PluginRuntimeLifecycleSummary {
                kind: "native".to_string(),
                label: format!("Native ({engine})"),
                supported: true,
                blocking_reasons: Vec::new(),
            }
        }
        PluginRuntime::Native { engine } => PluginRuntimeLifecycleSummary {
            kind: "native".to_string(),
            label: format!("Native ({engine})"),
            supported: false,
            blocking_reasons: vec![lifecycle_notice(
                "error",
                "PLUGIN_NATIVE_RUNTIME_UNSUPPORTED",
                "third-party native plugin runtime is not supported",
            )],
        },
        PluginRuntime::Wasm { .. } => PluginRuntimeLifecycleSummary {
            kind: "wasm".to_string(),
            label: "WASM".to_string(),
            supported: false,
            blocking_reasons: vec![lifecycle_notice(
                "warn",
                "PLUGIN_WASM_POLICY_GATED",
                "WASM plugin execution is policy-gated in this release",
            )],
        },
    }
}

fn hook_lifecycle_summaries(manifest: &PluginManifest) -> Vec<PluginHookLifecycleSummary> {
    manifest
        .hooks
        .iter()
        .map(|hook| PluginHookLifecycleSummary {
            name: hook.name.clone(),
            priority: hook.priority,
            failure_policy: hook.failure_policy.clone(),
        })
        .collect()
}

fn permission_lifecycle_summaries(
    permissions: &[String],
    granted: &[String],
    pending: &[String],
) -> Vec<PluginPermissionLifecycleSummary> {
    permissions
        .iter()
        .map(|permission| PluginPermissionLifecycleSummary {
            permission: permission.clone(),
            risk: permission_risk(permission).unwrap_or(PluginPermissionRisk::Low),
            granted: granted.contains(permission),
            pending: pending.contains(permission),
        })
        .collect()
}

fn compatibility_summary(
    manifest: &PluginManifest,
    host_version: &str,
) -> PluginCompatibilitySummary {
    match validate_manifest(manifest, host_version) {
        Ok(()) => PluginCompatibilitySummary {
            compatible: true,
            host_version: host_version.to_string(),
            app_range: manifest.host_compatibility.app.clone(),
            plugin_api_range: manifest.host_compatibility.plugin_api.clone(),
            platforms: manifest.host_compatibility.platforms.clone(),
            blocking_reasons: Vec::new(),
        },
        Err(error) => PluginCompatibilitySummary {
            compatible: false,
            host_version: host_version.to_string(),
            app_range: manifest.host_compatibility.app.clone(),
            plugin_api_range: manifest.host_compatibility.plugin_api.clone(),
            platforms: manifest.host_compatibility.platforms.clone(),
            blocking_reasons: vec![lifecycle_notice("error", &error.code, error.message)],
        },
    }
}

fn trust_summary(
    extracted: &package::ExtractedPluginPackage,
    policy: &LocalPackageInstallPolicy,
    trust: PackageTrust,
) -> PluginTrustSummary {
    let checksum_verified = policy.expected_checksum.as_deref().is_some_and(|expected| {
        expected
            .trim()
            .eq_ignore_ascii_case(extracted.checksum.as_str())
    });
    PluginTrustSummary {
        checksum: extracted.checksum.clone(),
        expected_checksum: policy.expected_checksum.clone(),
        checksum_verified,
        signature_verified: trust.signature_verified,
        unsigned: !trust.signature_verified,
        developer_mode: policy.developer_mode,
    }
}

pub(crate) fn preview_plugin_from_local_package_with_policy(
    db: &crate::db::Db,
    package_path: &Path,
    cache_dir: &Path,
    host_version: &str,
    policy: LocalPackageInstallPolicy,
) -> AppResult<PluginInstallPreview> {
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        format!(
            "failed to create plugin cache dir {}: {e}",
            cache_dir.display()
        )
    })?;
    let staging_root = cache_dir.join("staging");
    let staging_dir = staging_root.join(format!(
        "preview-{}",
        crate::shared::time::now_unix_seconds()
    ));
    let extracted = match package::extract_plugin_package_for_inspection(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            cleanup_staging_dir(&staging_root, &staging_dir);
            return Err(error);
        }
    };
    let result = build_install_preview(
        db,
        &extracted,
        host_version,
        PluginInstallSource::Local,
        &policy,
    );
    cleanup_staging_dir(&staging_root, &staging_dir);
    result
}

fn build_install_preview(
    db: &crate::db::Db,
    extracted: &package::ExtractedPluginPackage,
    host_version: &str,
    source: PluginInstallSource,
    policy: &LocalPackageInstallPolicy,
) -> AppResult<PluginInstallPreview> {
    let manifest = &extracted.manifest;
    let compatibility = compatibility_summary(manifest, host_version);
    let mut blocking_reasons = compatibility.blocking_reasons.clone();
    let mut warnings = Vec::new();
    let runtime = runtime_lifecycle_summary(manifest);
    blocking_reasons.extend(
        runtime
            .blocking_reasons
            .iter()
            .filter(|notice| notice.severity == "error")
            .cloned(),
    );
    warnings.extend(
        runtime
            .blocking_reasons
            .iter()
            .filter(|notice| notice.severity != "error")
            .cloned(),
    );

    let trust = match verify_local_package(extracted, policy) {
        Ok(trust) => trust,
        Err(error) => {
            blocking_reasons.push(lifecycle_notice(
                "error",
                error.code(),
                app_error_message(&error),
            ));
            PackageTrust {
                signature_verified: false,
            }
        }
    };
    if let Err(error) = enforce_unsigned_install_policy(manifest, policy, trust) {
        blocking_reasons.push(lifecycle_notice(
            "error",
            error.code(),
            app_error_message(&error),
        ));
    }
    if let Err(error) = validate_reserved_official_source(manifest, source) {
        blocking_reasons.push(lifecycle_notice(
            "error",
            error.code(),
            app_error_message(&error),
        ));
    }

    let existing = repository::get_plugin(db, &manifest.id).ok();
    let existing_status = existing.as_ref().map(|detail| detail.summary.status);
    let existing_version = existing
        .as_ref()
        .and_then(|detail| detail.summary.current_version.clone());

    Ok(PluginInstallPreview {
        plugin_id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        source,
        description: manifest.description.clone(),
        author: manifest.author.clone(),
        homepage: manifest.homepage.clone(),
        repository: manifest.repository.clone(),
        license: manifest.license.clone(),
        category: manifest.category.clone(),
        runtime,
        hooks: hook_lifecycle_summaries(manifest),
        permissions: permission_lifecycle_summaries(
            &manifest.permissions,
            &[],
            &manifest.permissions,
        ),
        compatibility,
        trust: trust_summary(extracted, policy, trust),
        existing_status,
        existing_version,
        blocking_reasons,
        warnings,
    })
}

pub(crate) fn preview_plugin_update_from_local_package(
    db: &crate::db::Db,
    package_path: &Path,
    cache_dir: &Path,
    host_version: &str,
    policy: LocalPackageInstallPolicy,
) -> AppResult<PluginUpdateDiff> {
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        format!(
            "failed to create plugin cache dir {}: {e}",
            cache_dir.display()
        )
    })?;
    let staging_root = cache_dir.join("staging");
    let staging_dir = staging_root.join(format!(
        "update-preview-{}",
        crate::shared::time::now_unix_seconds()
    ));
    let extracted = match package::extract_plugin_package_for_inspection(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            cleanup_staging_dir(&staging_root, &staging_dir);
            return Err(error);
        }
    };
    let result = build_update_diff(db, &extracted, host_version, &policy);
    cleanup_staging_dir(&staging_root, &staging_dir);
    result
}

fn build_update_diff(
    db: &crate::db::Db,
    extracted: &package::ExtractedPluginPackage,
    host_version: &str,
    policy: &LocalPackageInstallPolicy,
) -> AppResult<PluginUpdateDiff> {
    let manifest = &extracted.manifest;
    let current = repository::get_plugin(db, &manifest.id)?;
    let compatibility = compatibility_summary(manifest, host_version);
    let mut blocking_reasons = compatibility.blocking_reasons.clone();
    let mut warnings = Vec::new();

    let trust = match verify_local_package(extracted, policy) {
        Ok(trust) => trust,
        Err(error) => {
            blocking_reasons.push(lifecycle_notice(
                "error",
                error.code(),
                app_error_message(&error),
            ));
            PackageTrust {
                signature_verified: false,
            }
        }
    };
    if let Err(error) = enforce_unsigned_install_policy(manifest, policy, trust) {
        blocking_reasons.push(lifecycle_notice(
            "error",
            error.code(),
            app_error_message(&error),
        ));
    }
    if let Err(error) = validate_reserved_official_source(manifest, PluginInstallSource::Local) {
        blocking_reasons.push(lifecycle_notice(
            "error",
            error.code(),
            app_error_message(&error),
        ));
    }

    let current_runtime = runtime_lifecycle_summary(&current.manifest);
    let next_runtime = runtime_lifecycle_summary(manifest);
    blocking_reasons.extend(
        next_runtime
            .blocking_reasons
            .iter()
            .filter(|notice| notice.severity == "error")
            .cloned(),
    );
    warnings.extend(
        next_runtime
            .blocking_reasons
            .iter()
            .filter(|notice| notice.severity != "error")
            .cloned(),
    );
    let runtime_change = (current_runtime.kind != next_runtime.kind
        || current_runtime.label != next_runtime.label
        || current_runtime.supported != next_runtime.supported)
        .then(|| PluginLifecycleChange {
            name: "runtime".to_string(),
            change: "changed".to_string(),
            before: Some(current_runtime.label),
            after: Some(next_runtime.label),
        });

    let from_version = current
        .summary
        .current_version
        .clone()
        .unwrap_or_else(|| current.manifest.version.clone());
    let version_direction = compare_version_direction(&from_version, &manifest.version);
    if version_direction == "downgrade" {
        warnings.push(lifecycle_notice(
            "warn",
            "PLUGIN_UPDATE_DOWNGRADE",
            "selected package version is lower than the installed version",
        ));
    }

    Ok(PluginUpdateDiff {
        plugin_id: manifest.id.clone(),
        from_version: from_version.clone(),
        to_version: manifest.version.clone(),
        version_direction,
        runtime_change,
        hook_changes: diff_hooks(&current.manifest, manifest),
        permission_changes: diff_permissions(&current, manifest),
        config_version_change: config_version_change(&current.manifest, manifest),
        compatibility,
        trust: trust_summary(extracted, policy, trust),
        rollback_available: rollback_available(db, &manifest.id, &from_version),
        blocking_reasons,
        warnings,
    })
}

fn rollback_available(db: &crate::db::Db, plugin_id: &str, version: &str) -> bool {
    rollback_candidate_installed_dir(db, plugin_id, version)
        .is_some_and(|installed_dir| repository::plugin_installed_dir_available(&installed_dir))
}

fn rollback_candidate_installed_dir(
    db: &crate::db::Db,
    plugin_id: &str,
    version: &str,
) -> Option<String> {
    let conn = db.open_connection().ok()?;
    conn.query_row(
        r#"
SELECT installed_dir
FROM plugin_versions
WHERE plugin_id = ?1 AND version = ?2
"#,
        rusqlite::params![plugin_id, version],
        |row| row.get(0),
    )
    .optional()
    .ok()?
    .flatten()
}

fn diff_hooks(before: &PluginManifest, after: &PluginManifest) -> Vec<PluginLifecycleChange> {
    let mut changes = Vec::new();
    for hook in &before.hooks {
        match after.hooks.iter().find(|next| next.name == hook.name) {
            Some(next)
                if next.priority != hook.priority || next.failure_policy != hook.failure_policy =>
            {
                changes.push(PluginLifecycleChange {
                    name: hook.name.clone(),
                    change: "changed".to_string(),
                    before: Some(format!(
                        "priority={}, failurePolicy={}",
                        hook.priority,
                        hook.failure_policy.as_deref().unwrap_or("-")
                    )),
                    after: Some(format!(
                        "priority={}, failurePolicy={}",
                        next.priority,
                        next.failure_policy.as_deref().unwrap_or("-")
                    )),
                });
            }
            Some(_) => {}
            None => changes.push(PluginLifecycleChange {
                name: hook.name.clone(),
                change: "removed".to_string(),
                before: Some("declared".to_string()),
                after: None,
            }),
        }
    }
    for hook in &after.hooks {
        if before.hooks.iter().all(|prev| prev.name != hook.name) {
            changes.push(PluginLifecycleChange {
                name: hook.name.clone(),
                change: "added".to_string(),
                before: None,
                after: Some(format!(
                    "priority={}, failurePolicy={}",
                    hook.priority,
                    hook.failure_policy.as_deref().unwrap_or("-")
                )),
            });
        }
    }
    changes
}

fn diff_permissions(
    current: &PluginDetail,
    next: &PluginManifest,
) -> Vec<PluginPermissionLifecycleChange> {
    let mut all = current.manifest.permissions.clone();
    for permission in &next.permissions {
        if !all.contains(permission) {
            all.push(permission.clone());
        }
    }
    all.sort();

    all.into_iter()
        .map(|permission| {
            let was_requested = current.manifest.permissions.contains(&permission);
            let is_requested = next.permissions.contains(&permission);
            let was_granted = current.granted_permissions.contains(&permission);
            let was_pending = current.pending_permissions.contains(&permission);
            let change = match (was_requested, is_requested, was_granted, was_pending) {
                (true, true, true, _) => "unchanged_granted",
                (true, true, false, true) => "unchanged_pending",
                (true, true, false, false) => "unchanged_requested",
                (false, true, _, _) => "added_pending",
                (true, false, _, _) => "removed",
                (false, false, _, _) => "not_requested",
            };
            let risk = permission_risk(&permission).unwrap_or(PluginPermissionRisk::Low);
            PluginPermissionLifecycleChange {
                permission,
                risk,
                change: change.to_string(),
            }
        })
        .filter(|change| change.change != "not_requested")
        .collect()
}

fn reconcile_permissions_for_manifest(
    current: &PluginDetail,
    manifest: &PluginManifest,
) -> (Vec<String>, Vec<String>) {
    let mut granted: Vec<String> = current
        .granted_permissions
        .iter()
        .filter(|permission| manifest.permissions.contains(*permission))
        .cloned()
        .collect();
    granted.sort();
    granted.dedup();

    let mut pending: Vec<String> = manifest
        .permissions
        .iter()
        .filter(|permission| !granted.contains(permission))
        .cloned()
        .collect();
    pending.sort();
    pending.dedup();

    (granted, pending)
}

fn config_for_manifest_version(
    current: &PluginDetail,
    manifest: &PluginManifest,
) -> serde_json::Value {
    config_with_schema_defaults(manifest.config_schema.as_ref(), current.config.clone())
}

fn config_version_change(before: &PluginManifest, after: &PluginManifest) -> Option<String> {
    let before_version = before.config_version.unwrap_or(1);
    let after_version = after.config_version.unwrap_or(1);
    (before_version != after_version).then(|| format!("{before_version} -> {after_version}"))
}

pub(crate) fn install_plugin_from_local_package_with_policy(
    db: &crate::db::Db,
    package_path: &Path,
    cache_dir: &Path,
    installed_root: &Path,
    host_version: &str,
    policy: LocalPackageInstallPolicy,
) -> AppResult<PluginDetail> {
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        format!(
            "failed to create plugin cache dir {}: {e}",
            cache_dir.display()
        )
    })?;
    std::fs::create_dir_all(installed_root).map_err(|e| {
        format!(
            "failed to create plugin installed dir {}: {e}",
            installed_root.display()
        )
    })?;

    let staging_root = cache_dir.join("staging");
    let staging_dir =
        staging_root.join(format!("local-{}", crate::shared::time::now_unix_seconds()));
    let extracted = match package::extract_plugin_package(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            let _ = std::fs::remove_dir(&staging_root);
            return Err(error);
        }
    };
    let trust = match validate_local_package_install(&extracted, host_version, &policy) {
        Ok(trust) => trust,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            let _ = std::fs::remove_dir(&staging_root);
            return Err(error);
        }
    };

    let plugin_id = extracted.manifest.id.clone();
    let version = extracted.manifest.version.clone();
    let installed_dir = installed_root
        .join(crate::app_paths::plugin_id_path_segment(&plugin_id)?)
        .join(crate::app_paths::plugin_id_path_segment(&version)?);
    let cache_package_path = cache_dir.join(format!(
        "{}-{}-{}.aio-plugin",
        plugin_id,
        version,
        crate::shared::time::now_unix_seconds()
    ));

    let result = (|| -> AppResult<PluginDetail> {
        std::fs::copy(package_path, &cache_package_path).map_err(|e| {
            format!(
                "failed to copy plugin package {} -> {}: {e}",
                package_path.display(),
                cache_package_path.display()
            )
        })?;

        replace_dir(&extracted.root_dir, &installed_dir)?;
        let requested_permissions = extracted.manifest.permissions.clone();
        let detail = repository::with_plugin_transaction(db, |tx| {
            repository::insert_plugin_with_tx(
                tx,
                repository::InsertPluginInput {
                    manifest: extracted.manifest.clone(),
                    install_source: policy.install_source,
                    status: PluginStatus::Disabled,
                    installed_dir: Some(installed_dir.to_string_lossy().to_string()),
                },
            )?;
            let detail = repository::save_plugin_permissions_with_tx(
                tx,
                &plugin_id,
                &[],
                &requested_permissions,
            )?;
            append_audit_with_tx(
                tx,
                Some(plugin_id.clone()),
                install_audit_event_type(policy.install_source),
                "medium",
                install_audit_message(policy.install_source),
                install_audit_details(
                    policy.install_source,
                    policy.remote_source_url.as_deref(),
                    policy.market_source_url.as_deref(),
                    serde_json::json!({
                    "source": "local",
                    "packageChecksum": extracted.checksum,
                    "cachedPackage": cache_package_path.to_string_lossy(),
                    "unsigned": !trust.signature_verified,
                    "signatureVerified": trust.signature_verified,
                    "developerMode": policy.developer_mode,
                    }),
                ),
            )?;
            Ok(detail)
        })?;
        tracing::info!(
            plugin_id,
            version,
            installed_dir = %installed_dir.display(),
            "local plugin package installed"
        );
        repository::get_plugin(db, &plugin_id).or(Ok(detail))
    })();

    let _ = std::fs::remove_dir_all(&staging_dir);
    let _ = std::fs::remove_dir(&staging_root);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&installed_dir);
        let _ = std::fs::remove_file(&cache_package_path);
    }
    result
}

pub(crate) fn install_plugin_from_remote_package_bytes(
    db: &crate::db::Db,
    package_bytes: Vec<u8>,
    source_url: &str,
    cache_dir: &Path,
    installed_root: &Path,
    host_version: &str,
    policy: RemotePackageInstallPolicy,
) -> AppResult<PluginDetail> {
    if !matches!(
        policy.install_source,
        PluginInstallSource::Market | PluginInstallSource::GithubRelease
    ) {
        return Err(AppError::new(
            "PLUGIN_REMOTE_INSTALL_SOURCE_INVALID",
            "remote plugin install source must be market or GitHub release",
        ));
    }
    if policy.expected_checksum.trim().is_empty() {
        return Err(AppError::new(
            "PLUGIN_REMOTE_CHECKSUM_REQUIRED",
            "remote plugin installation requires a package checksum",
        ));
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        format!(
            "failed to create plugin cache dir {}: {e}",
            cache_dir.display()
        )
    })?;
    let package_path = cache_dir.join(format!(
        "remote-{}-{}.aio-plugin",
        crate::app_paths::plugin_id_path_segment(&policy.expected_plugin_id)?,
        crate::shared::time::now_unix_seconds()
    ));
    std::fs::write(&package_path, &package_bytes).map_err(|e| {
        format!(
            "failed to write remote plugin package cache {}: {e}",
            package_path.display()
        )
    })?;

    let install_source = policy.install_source;
    let expected_plugin_id = policy.expected_plugin_id.clone();
    let expected_checksum = policy.expected_checksum.clone();
    let signature = policy.signature.clone();
    let market_source_url = (install_source == PluginInstallSource::Market)
        .then(|| policy.market_source_url.clone())
        .flatten();
    let public_key = remote_package_trusted_public_key(db, source_url, &policy)?;
    let result = install_plugin_from_local_package_with_policy(
        db,
        &package_path,
        cache_dir,
        installed_root,
        host_version,
        LocalPackageInstallPolicy {
            expected_plugin_id: Some(expected_plugin_id),
            expected_checksum: Some(expected_checksum),
            signature,
            public_key,
            allow_unsigned: false,
            developer_mode: false,
            install_source,
            remote_source_url: Some(source_url.to_string()),
            market_source_url,
        },
    )
    .inspect(|detail| {
        tracing::info!(
            plugin_id = %detail.summary.plugin_id,
            source = install_source.as_str(),
            "remote plugin package installed"
        );
    });

    let _ = std::fs::remove_file(&package_path);
    result
}

fn remote_package_trusted_public_key(
    db: &crate::db::Db,
    source_url: &str,
    policy: &RemotePackageInstallPolicy,
) -> AppResult<Option<String>> {
    match policy.install_source {
        PluginInstallSource::Market => {
            if policy.signature.is_none() {
                return Ok(None);
            }
            let trust_source_url = policy.market_source_url.as_deref().unwrap_or(source_url);
            repository::trusted_market_public_key_for_url(db, trust_source_url)?
                .ok_or_else(|| {
                    AppError::new(
                    "PLUGIN_MARKET_TRUSTED_PUBLIC_KEY_REQUIRED",
                    "signed market plugin installation requires a trusted market source public key",
                )
                })
                .map(Some)
        }
        PluginInstallSource::GithubRelease => Ok(policy.public_key.clone()),
        _ => Err(AppError::new(
            "PLUGIN_REMOTE_INSTALL_SOURCE_INVALID",
            "remote plugin install source must be market or GitHub release",
        )),
    }
}

pub(crate) fn update_plugin_from_local_package(
    db: &crate::db::Db,
    package_path: &Path,
    cache_dir: &Path,
    installed_root: &Path,
    host_version: &str,
    policy: LocalPackageInstallPolicy,
) -> AppResult<PluginDetail> {
    std::fs::create_dir_all(cache_dir).map_err(|e| {
        format!(
            "failed to create plugin cache dir {}: {e}",
            cache_dir.display()
        )
    })?;
    std::fs::create_dir_all(installed_root).map_err(|e| {
        format!(
            "failed to create plugin installed dir {}: {e}",
            installed_root.display()
        )
    })?;
    let staging_root = cache_dir.join("staging");
    let staging_dir = staging_root.join(format!(
        "update-{}",
        crate::shared::time::now_unix_seconds()
    ));
    let extracted = match package::extract_plugin_package(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            let _ = std::fs::remove_dir(&staging_root);
            return Err(error);
        }
    };
    let trust = match validate_local_package_install(&extracted, host_version, &policy) {
        Ok(trust) => trust,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            let _ = std::fs::remove_dir(&staging_root);
            return Err(error);
        }
    };

    let plugin_id = extracted.manifest.id.clone();
    let current = repository::get_plugin(db, &plugin_id)?;
    let installed_dir = installed_root
        .join(crate::app_paths::plugin_id_path_segment(&plugin_id)?)
        .join(crate::app_paths::plugin_id_path_segment(
            &extracted.manifest.version,
        )?);

    let result = (|| -> AppResult<PluginDetail> {
        replace_dir(&extracted.root_dir, &installed_dir)?;
        let (granted, pending) = reconcile_permissions_for_manifest(&current, &extracted.manifest);
        let next_config = config_for_manifest_version(&current, &extracted.manifest);
        let detail = repository::with_plugin_transaction(db, |tx| {
            repository::update_plugin_manifest_with_tx(
                tx,
                extracted.manifest.clone(),
                Some(installed_dir.to_string_lossy().to_string()),
            )?;
            repository::save_plugin_config_with_tx(
                tx,
                &plugin_id,
                extracted.manifest.config_version.unwrap_or(1),
                &next_config,
                &[],
            )?;
            let detail =
                repository::save_plugin_permissions_with_tx(tx, &plugin_id, &granted, &pending)?;
            append_audit_with_tx(
                tx,
                Some(plugin_id.clone()),
                "plugin.updated",
                "high",
                "Plugin updated from local package",
                serde_json::json!({
                    "fromVersion": current.summary.current_version,
                    "toVersion": extracted.manifest.version,
                    "pendingPermissions": pending,
                    "unsigned": !trust.signature_verified,
                    "signatureVerified": trust.signature_verified,
                    "developerMode": policy.developer_mode,
                }),
            )?;
            Ok(detail)
        })?;
        tracing::info!(
            plugin_id,
            version = extracted.manifest.version,
            "local plugin package updated"
        );
        Ok(detail)
    })();

    let _ = std::fs::remove_dir_all(&staging_dir);
    let _ = std::fs::remove_dir(&staging_root);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&installed_dir);
    }
    result
}

pub(crate) fn rollback_plugin_to_version(
    db: &crate::db::Db,
    plugin_id: &str,
    version: &str,
) -> AppResult<PluginDetail> {
    let (manifest, installed_dir) = repository::get_plugin_version(db, plugin_id, version)?;
    let installed_dir_value = installed_dir.as_deref().ok_or_else(|| {
        AppError::new(
            "PLUGIN_ROLLBACK_UNAVAILABLE",
            format!("plugin version {version} has no install directory"),
        )
    })?;
    if !repository::plugin_installed_dir_available(installed_dir_value) {
        return Err(AppError::new(
            "PLUGIN_ROLLBACK_UNAVAILABLE",
            format!("plugin version {version} install directory is unavailable"),
        ));
    }
    let current = repository::get_plugin(db, plugin_id)?;
    let (granted, pending) = reconcile_permissions_for_manifest(&current, &manifest);
    let next_config = config_for_manifest_version(&current, &manifest);
    let config_version = manifest.config_version.unwrap_or(1);
    let detail = repository::with_plugin_transaction(db, |tx| {
        repository::update_plugin_manifest_with_tx(tx, manifest, installed_dir)?;
        repository::save_plugin_config_with_tx(tx, plugin_id, config_version, &next_config, &[])?;
        let detail =
            repository::save_plugin_permissions_with_tx(tx, plugin_id, &granted, &pending)?;
        append_audit_with_tx(
            tx,
            Some(plugin_id.to_string()),
            "plugin.rollback",
            "high",
            "Plugin rolled back",
            serde_json::json!({
                "version": version,
                "grantedPermissions": granted,
                "pendingPermissions": pending,
                "configVersion": config_version,
            }),
        )?;
        Ok(detail)
    })?;
    tracing::warn!(plugin_id, version, "plugin rolled back to previous version");
    Ok(detail)
}

pub(crate) fn quarantine_revoked_plugin(
    db: &crate::db::Db,
    plugin_id: &str,
    reason: &str,
) -> AppResult<PluginDetail> {
    let detail =
        repository::update_plugin_status(db, plugin_id, PluginStatus::Quarantined, Some(reason))?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.quarantined",
        "critical",
        "Plugin quarantined",
        serde_json::json!({ "reason": reason, "source": "market_revoked" }),
    )?;
    tracing::warn!(plugin_id, reason, "plugin quarantined by market revocation");
    repository::get_plugin(db, plugin_id).or(Ok(detail))
}

fn enforce_unsigned_install_policy(
    manifest: &PluginManifest,
    policy: &LocalPackageInstallPolicy,
    trust: PackageTrust,
) -> AppResult<()> {
    if trust.signature_verified {
        return Ok(());
    }
    if !policy.allow_unsigned || !policy.developer_mode {
        for permission in &manifest.permissions {
            if matches!(
                permission_risk(permission),
                Some(PluginPermissionRisk::High | PluginPermissionRisk::Critical)
            ) {
                return Err(AppError::new(
                    "PLUGIN_UNSIGNED_HIGH_RISK_PERMISSION",
                    format!("unsigned plugin cannot request high-risk permission: {permission}"),
                ));
            }
        }
    }
    Ok(())
}

fn verify_local_package(
    extracted: &package::ExtractedPluginPackage,
    policy: &LocalPackageInstallPolicy,
) -> AppResult<PackageTrust> {
    if let Some(expected_plugin_id) = policy.expected_plugin_id.as_deref() {
        if extracted.manifest.id != expected_plugin_id {
            return Err(AppError::new(
                "PLUGIN_REMOTE_PLUGIN_ID_MISMATCH",
                format!(
                    "remote package plugin id mismatch: expected {}, got {}",
                    expected_plugin_id, extracted.manifest.id
                ),
            ));
        }
    }

    if let Some(expected) = policy.expected_checksum.as_deref() {
        signing::verify_checksum(&extracted.package_bytes, expected)?;
    }

    match (policy.signature.as_deref(), policy.public_key.as_deref()) {
        (Some(signature), Some(public_key)) => {
            signing::verify_ed25519_signature(&extracted.package_bytes, signature, public_key)?;
            Ok(PackageTrust {
                signature_verified: true,
            })
        }
        (Some(_), None) | (None, Some(_)) => Err(AppError::new(
            "PLUGIN_SIGNATURE_POLICY_INCOMPLETE",
            "plugin signature verification requires both signature and public key",
        )),
        (None, None) => Ok(PackageTrust {
            signature_verified: false,
        }),
    }
}

fn validate_local_package_install(
    extracted: &package::ExtractedPluginPackage,
    host_version: &str,
    policy: &LocalPackageInstallPolicy,
) -> AppResult<PackageTrust> {
    validate_manifest(&extracted.manifest, host_version)?;
    validate_reserved_official_source(&extracted.manifest, policy.install_source)?;
    let trust = verify_local_package(extracted, policy)?;
    enforce_unsigned_install_policy(&extracted.manifest, policy, trust)?;
    Ok(trust)
}

fn install_audit_event_type(source: PluginInstallSource) -> &'static str {
    match source {
        PluginInstallSource::Market | PluginInstallSource::GithubRelease => {
            "plugin.remote.installed"
        }
        _ => "plugin.installed",
    }
}

fn install_audit_message(source: PluginInstallSource) -> &'static str {
    match source {
        PluginInstallSource::Market | PluginInstallSource::GithubRelease => {
            "Remote plugin package installed"
        }
        _ => "Local plugin package installed",
    }
}

fn install_audit_details(
    source: PluginInstallSource,
    source_url: Option<&str>,
    market_source_url: Option<&str>,
    mut details: serde_json::Value,
) -> serde_json::Value {
    if let serde_json::Value::Object(object) = &mut details {
        object.insert(
            "source".to_string(),
            serde_json::Value::String(source.as_str().to_string()),
        );
        if let Some(source_url) = source_url {
            object.insert(
                "sourceUrl".to_string(),
                serde_json::Value::String(source_url.to_string()),
            );
        }
        if let Some(market_source_url) = market_source_url {
            object.insert(
                "marketSourceUrl".to_string(),
                serde_json::Value::String(market_source_url.to_string()),
            );
        }
    }
    details
}

fn validate_reserved_official_source(
    manifest: &PluginManifest,
    install_source: PluginInstallSource,
) -> AppResult<()> {
    if manifest.id.starts_with("official.") && install_source != PluginInstallSource::Official {
        return Err(AppError::new(
            "PLUGIN_RESERVED_OFFICIAL_ID",
            "official plugin ids are reserved for built-in official plugins",
        ));
    }
    Ok(())
}

pub(crate) fn enable_plugin(
    db: &crate::db::Db,
    plugin_id: &str,
    host_version: &str,
) -> AppResult<PluginDetail> {
    let detail = detail_with_config_defaults_for_db(db, repository::get_plugin(db, plugin_id)?)?;
    if !matches!(
        detail.summary.status,
        PluginStatus::Disabled | PluginStatus::Installed
    ) {
        return Err(AppError::new(
            "PLUGIN_INVALID_STATUS",
            format!(
                "plugin {plugin_id} cannot be enabled from status {}",
                detail.summary.status.as_str()
            ),
        ));
    }
    validate_manifest(&detail.manifest, host_version)?;
    ensure_runtime_enabled(&detail.manifest)?;
    ensure_required_permissions_granted(&detail)?;
    validate_config_against_schema(detail.manifest.config_schema.as_ref(), &detail.config)?;
    let next = repository::update_plugin_status(db, plugin_id, PluginStatus::Enabled, None)?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.enabled",
        "low",
        "Plugin enabled",
        serde_json::json!({}),
    )?;
    tracing::info!(plugin_id, "plugin enabled");
    detail_with_config_defaults_for_db(db, next)
}

pub(crate) fn disable_plugin(db: &crate::db::Db, plugin_id: &str) -> AppResult<PluginDetail> {
    let next = repository::update_plugin_status(db, plugin_id, PluginStatus::Disabled, None)?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.disabled",
        "low",
        "Plugin disabled",
        serde_json::json!({}),
    )?;
    tracing::info!(plugin_id, "plugin disabled");
    Ok(next)
}

pub(crate) fn uninstall_plugin(db: &crate::db::Db, plugin_id: &str) -> AppResult<PluginDetail> {
    let next = repository::update_plugin_status(db, plugin_id, PluginStatus::Uninstalled, None)?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.uninstalled",
        "medium",
        "Plugin uninstalled",
        serde_json::json!({ "auditRetained": true }),
    )?;
    tracing::info!(plugin_id, "plugin uninstalled");
    repository::get_plugin(db, plugin_id).or(Ok(next))
}

pub(crate) fn save_plugin_config(
    db: &crate::db::Db,
    plugin_id: &str,
    config: serde_json::Value,
) -> AppResult<PluginDetail> {
    let detail = detail_with_config_defaults_for_db(db, repository::get_plugin(db, plugin_id)?)?;
    let config = config_with_schema_defaults(detail.manifest.config_schema.as_ref(), config);
    validate_config_against_schema(detail.manifest.config_schema.as_ref(), &config)?;
    let config_version = detail.manifest.config_version.unwrap_or(1);
    let next = repository::save_plugin_config(db, plugin_id, config_version, &config, &[])?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.config.saved",
        "medium",
        "Plugin config saved",
        serde_json::json!({ "configVersion": config_version }),
    )?;
    detail_with_config_defaults_for_db(db, next)
}

pub(crate) fn grant_plugin_permissions(
    db: &crate::db::Db,
    plugin_id: &str,
    permissions: Vec<String>,
) -> AppResult<PluginDetail> {
    let detail = repository::get_plugin(db, plugin_id)?;
    let mut granted = detail.granted_permissions;
    for permission in permissions {
        if !detail.manifest.permissions.contains(&permission) {
            return Err(AppError::new(
                "PLUGIN_PERMISSION_NOT_REQUESTED",
                format!("plugin did not request permission: {permission}"),
            ));
        }
        if !granted.contains(&permission) {
            granted.push(permission);
        }
    }
    granted.sort();
    let next = repository::save_plugin_permissions(db, plugin_id, &granted, &[])?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.permissions.granted",
        "high",
        "Plugin permissions granted",
        serde_json::json!({ "permissions": granted }),
    )?;
    Ok(next)
}

pub(crate) fn revoke_plugin_permission(
    db: &crate::db::Db,
    plugin_id: &str,
    permission: &str,
) -> AppResult<PluginDetail> {
    let detail = repository::get_plugin(db, plugin_id)?;
    let granted: Vec<String> = detail
        .granted_permissions
        .into_iter()
        .filter(|item| item != permission)
        .collect();
    let next = repository::save_plugin_permissions(db, plugin_id, &granted, &[])?;
    append_audit(
        db,
        Some(plugin_id.to_string()),
        "plugin.permissions.revoked",
        "medium",
        "Plugin permission revoked",
        serde_json::json!({ "permission": permission }),
    )?;
    repository::get_plugin(db, plugin_id).or(Ok(next))
}

fn ensure_required_permissions_granted(detail: &PluginDetail) -> AppResult<()> {
    let missing: Vec<&str> = detail
        .manifest
        .permissions
        .iter()
        .map(String::as_str)
        .filter(|permission| {
            !detail
                .granted_permissions
                .iter()
                .any(|item| item == permission)
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::new(
            "PLUGIN_PERMISSION_REQUIRED",
            format!(
                "missing required plugin permissions: {}",
                missing.join(", ")
            ),
        ))
    }
}

fn ensure_runtime_enabled(manifest: &PluginManifest) -> AppResult<()> {
    match &manifest.runtime {
        PluginRuntime::ExtensionHost { .. } => Err(AppError::new(
            "PLUGIN_RUNTIME_DISABLED",
            "extension host runtime execution is not wired in this release",
        )),
        PluginRuntime::DeclarativeRules { .. } => Ok(()),
        PluginRuntime::Native { engine }
            if manifest.id == "official.privacy-filter" && engine == "privacyFilter" =>
        {
            Ok(())
        }
        PluginRuntime::Wasm { .. } => Err(AppError::new(
            "PLUGIN_RUNTIME_DISABLED",
            "wasm runtime execution is disabled by host policy",
        )),
        PluginRuntime::Native { .. } => Err(AppError::new(
            "PLUGIN_UNSUPPORTED_RUNTIME",
            "native runtime is reserved for official plugins",
        )),
    }
}

fn detail_with_config_defaults_for_db(
    db: &crate::db::Db,
    detail: PluginDetail,
) -> AppResult<PluginDetail> {
    let stored_config_version = repository::plugin_config_version(db, &detail.summary.plugin_id)?;
    Ok(detail_with_config_defaults(detail, stored_config_version))
}

fn detail_with_config_defaults(
    mut detail: PluginDetail,
    stored_config_version: Option<u32>,
) -> PluginDetail {
    detail = merge_packaged_official_manifest(detail);
    detail.config =
        config_with_schema_defaults(detail.manifest.config_schema.as_ref(), detail.config);
    migrate_legacy_official_privacy_filter_config(&mut detail, stored_config_version);
    detail
}

fn merge_packaged_official_manifest(mut detail: PluginDetail) -> PluginDetail {
    if detail.install_source != PluginInstallSource::Official
        || detail.summary.plugin_id != OFFICIAL_PRIVACY_FILTER_ID
    {
        return detail;
    }

    let Ok(fixture) = crate::app::plugins::official::official_plugin(&detail.summary.plugin_id)
    else {
        return detail;
    };

    detail.manifest = fixture.manifest;
    detail
}

fn migrate_legacy_official_privacy_filter_config(
    detail: &mut PluginDetail,
    stored_config_version: Option<u32>,
) {
    if detail.install_source != PluginInstallSource::Official
        || detail.summary.plugin_id != OFFICIAL_PRIVACY_FILTER_ID
    {
        return;
    }
    let current_version = detail.manifest.config_version.unwrap_or(1);
    if stored_config_version.unwrap_or(0) >= current_version {
        return;
    }
    let default_sensitive_types = detail
        .manifest
        .config_schema
        .as_ref()
        .and_then(|schema| schema.pointer("/properties/sensitiveTypes/default"))
        .and_then(serde_json::Value::as_array);
    let default_redaction_scopes = detail
        .manifest
        .config_schema
        .as_ref()
        .and_then(|schema| schema.pointer("/properties/redactionScopes/default"))
        .cloned();
    let Some(config) = detail.config.as_object_mut() else {
        return;
    };

    if let Some(default_sensitive_types) = default_sensitive_types {
        if let Some(sensitive_types) = config
            .get_mut("sensitiveTypes")
            .and_then(serde_json::Value::as_array_mut)
        {
            for item in default_sensitive_types {
                if !sensitive_types.iter().any(|existing| existing == item) {
                    sensitive_types.push(item.clone());
                }
            }
        }
    }

    if !config.contains_key("redactionScopes") {
        if let Some(default_redaction_scopes) = default_redaction_scopes {
            config.insert("redactionScopes".to_string(), default_redaction_scopes);
        }
    }
}

fn config_with_schema_defaults(
    schema: Option<&serde_json::Value>,
    mut config: serde_json::Value,
) -> serde_json::Value {
    if let Some(schema) = schema {
        apply_schema_defaults(&mut config, schema);
    }
    config
}

fn apply_schema_defaults(value: &mut serde_json::Value, schema: &serde_json::Value) {
    match schema.get("type").and_then(serde_json::Value::as_str) {
        Some("object") => {
            if !value.is_object() {
                if let Some(default) = schema.get("default") {
                    *value = default.clone();
                }
            }
            let Some(object) = value.as_object_mut() else {
                return;
            };
            let Some(properties) = schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
            else {
                return;
            };
            for (key, child_schema) in properties {
                if !object.contains_key(key) {
                    if let Some(default) = child_schema.get("default") {
                        object.insert(key.clone(), default.clone());
                    }
                }
                if let Some(child_value) = object.get_mut(key) {
                    apply_schema_defaults(child_value, child_schema);
                }
            }
        }
        Some("array") => {
            if !value.is_array() {
                if let Some(default) = schema.get("default") {
                    *value = default.clone();
                }
            }
        }
        Some(_) | None => {
            if value.is_null() {
                if let Some(default) = schema.get("default") {
                    *value = default.clone();
                }
            }
        }
    }
}

fn append_audit(
    db: &crate::db::Db,
    plugin_id: Option<String>,
    event_type: &str,
    risk_level: &str,
    message: &str,
    details: serde_json::Value,
) -> AppResult<()> {
    repository::append_audit_log(
        db,
        repository::AppendPluginAuditLogInput {
            plugin_id,
            trace_id: None,
            event_type: event_type.to_string(),
            risk_level: risk_level.to_string(),
            message: message.to_string(),
            details,
        },
    )?;
    Ok(())
}

fn append_audit_with_tx(
    conn: &rusqlite::Transaction<'_>,
    plugin_id: Option<String>,
    event_type: &str,
    risk_level: &str,
    message: &str,
    details: serde_json::Value,
) -> AppResult<()> {
    repository::append_audit_log_with_tx(
        conn,
        repository::AppendPluginAuditLogInput {
            plugin_id,
            trace_id: None,
            event_type: event_type.to_string(),
            risk_level: risk_level.to_string(),
            message: message.to_string(),
            details,
        },
    )?;
    Ok(())
}

fn replace_dir(src: &Path, dst: &Path) -> AppResult<()> {
    let Some(parent) = dst.parent() else {
        return Err(AppError::new(
            "PLUGIN_INSTALL_FAILED",
            format!("invalid plugin install dir: {}", dst.display()),
        ));
    };
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    if dst.exists() {
        std::fs::remove_dir_all(dst)
            .map_err(|e| format!("failed to remove existing {}: {e}", dst.display()))?;
    }
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_recursive(src, dst)?;
            std::fs::remove_dir_all(src)
                .map_err(|e| format!("failed to remove staging {}: {e}", src.display()))?;
            Ok(())
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("failed to read {}: {e}", src.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let source_path = entry.path();
        let target_path: PathBuf = dst.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path).map_err(|e| {
                format!(
                    "failed to copy {} -> {}: {e}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn validate_config_against_schema(
    schema: Option<&serde_json::Value>,
    config: &serde_json::Value,
) -> AppResult<()> {
    let Some(schema) = schema else {
        return Ok(());
    };
    validate_value_against_schema("$", schema, config)
}

fn validate_value_against_schema(
    path: &str,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> AppResult<()> {
    let schema_type = schema
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| AppError::new("PLUGIN_INVALID_CONFIG_SCHEMA", "schema type is required"))?;

    match schema_type {
        "object" => {
            let object = value.as_object().ok_or_else(|| {
                AppError::new("PLUGIN_INVALID_CONFIG", format!("{path} must be an object"))
            })?;
            if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
                for key in required.iter().filter_map(serde_json::Value::as_str) {
                    if !object.contains_key(key) {
                        return Err(AppError::new(
                            "PLUGIN_INVALID_CONFIG",
                            format!("{path}.{key} is required"),
                        ));
                    }
                }
            }
            if let Some(properties) = schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
            {
                for (key, child_schema) in properties {
                    if let Some(child_value) = object.get(key) {
                        validate_value_against_schema(
                            &format!("{path}.{key}"),
                            child_schema,
                            child_value,
                        )?;
                    }
                }
            }
            Ok(())
        }
        "array" => {
            let array = value.as_array().ok_or_else(|| {
                AppError::new("PLUGIN_INVALID_CONFIG", format!("{path} must be an array"))
            })?;
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in array.iter().enumerate() {
                    validate_value_against_schema(&format!("{path}[{index}]"), item_schema, item)?;
                }
            }
            Ok(())
        }
        "string" | "password" => {
            if !value.is_string() {
                return Err(AppError::new(
                    "PLUGIN_INVALID_CONFIG",
                    format!("{path} must be a string"),
                ));
            }
            validate_enum(path, schema, value)
        }
        "number" => {
            if !value.is_number() {
                return Err(AppError::new(
                    "PLUGIN_INVALID_CONFIG",
                    format!("{path} must be a number"),
                ));
            }
            validate_enum(path, schema, value)
        }
        "integer" => {
            if value.as_i64().is_none() && value.as_u64().is_none() {
                return Err(AppError::new(
                    "PLUGIN_INVALID_CONFIG",
                    format!("{path} must be an integer"),
                ));
            }
            validate_enum(path, schema, value)
        }
        "boolean" => {
            if !value.is_boolean() {
                return Err(AppError::new(
                    "PLUGIN_INVALID_CONFIG",
                    format!("{path} must be a boolean"),
                ));
            }
            validate_enum(path, schema, value)
        }
        _ => Err(AppError::new(
            "PLUGIN_INVALID_CONFIG_SCHEMA",
            format!("unsupported schema type: {schema_type}"),
        )),
    }
}

fn validate_enum(
    path: &str,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> AppResult<()> {
    let Some(allowed) = schema.get("enum").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };
    if allowed.iter().any(|item| item == value) {
        Ok(())
    } else {
        Err(AppError::new(
            "PLUGIN_INVALID_CONFIG",
            format!("{path} is not an allowed value"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugins::{
        PluginInstallSource, PluginManifest, PluginPermissionRisk, PluginStatus,
    };
    use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayRequestHookInput};
    use crate::gateway::plugins::pipeline::{GatewayPluginPipeline, GatewayPluginPipelineConfig};
    use std::io::Write;
    use std::path::Path;
    use std::sync::Arc;

    fn manifest() -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "id": "community.prompt-helper",
            "name": "Community Prompt Helper",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "runtime": {
                "kind": "declarativeRules",
                "rules": ["rules/main.json"]
            },
            "hooks": [
                {
                    "name": "gateway.request.afterBodyRead",
                    "priority": 100,
                    "failurePolicy": "fail-open"
                }
            ],
            "permissions": ["request.body.read", "request.body.write"],
            "hostCompatibility": {
                "app": ">=0.56.0 <1.0.0",
                "pluginApi": "^1.0.0",
                "platforms": ["macos", "windows", "linux"]
            },
            "configSchema": {
                "type": "object",
                "required": ["mode"],
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["append_instruction", "rewrite_system_message"]
                    }
                }
            }
        }))
        .unwrap()
    }

    fn wasm_manifest(plugin_id: &str) -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "id": plugin_id,
            "name": "WASM Policy Plugin",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "runtime": {
                "kind": "wasm",
                "abiVersion": "1.0.0",
                "memoryLimitBytes": 16777216
            },
            "entry": "plugin.wasm",
            "hooks": [
                {
                    "name": "gateway.request.afterBodyRead",
                    "priority": 100,
                    "failurePolicy": "fail-open"
                }
            ],
            "permissions": ["request.body.read"],
            "hostCompatibility": {
                "app": ">=0.56.0 <1.0.0",
                "pluginApi": "^1.0.0",
                "platforms": ["macos", "windows", "linux"]
            }
        }))
        .unwrap()
    }

    #[test]
    fn service_requires_permissions_before_enable_and_preserves_config_on_disable() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();

        let err =
            enable_plugin(&db, "community.prompt-helper", env!("CARGO_PKG_VERSION")).unwrap_err();
        assert!(err.to_string().starts_with("PLUGIN_PERMISSION_REQUIRED:"));

        save_plugin_config(
            &db,
            "community.prompt-helper",
            serde_json::json!({"mode": "append_instruction"}),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "community.prompt-helper",
            vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
        )
        .unwrap();
        let enabled =
            enable_plugin(&db, "community.prompt-helper", env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(enabled.summary.status, PluginStatus::Enabled);

        let disabled = disable_plugin(&db, "community.prompt-helper").unwrap();
        assert_eq!(disabled.summary.status, PluginStatus::Disabled);
        assert_eq!(disabled.config["mode"], "append_instruction");
    }

    #[test]
    fn local_plugin_install_records_manifest_permissions_as_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        let installed = install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();

        assert_eq!(installed.granted_permissions, Vec::<String>::new());
        assert_eq!(
            installed.pending_permissions,
            vec!["request.body.read", "request.body.write"]
        );
    }

    #[test]
    fn enable_plugin_rejects_quarantined_even_when_ready() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        save_plugin_config(
            &db,
            "community.prompt-helper",
            serde_json::json!({"mode": "append_instruction"}),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "community.prompt-helper",
            vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
        )
        .unwrap();
        quarantine_revoked_plugin(&db, "community.prompt-helper", "revoked").unwrap();

        let err =
            enable_plugin(&db, "community.prompt-helper", env!("CARGO_PKG_VERSION")).unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_INVALID_STATUS:"));
        let detail = get_plugin_detail(&db, "community.prompt-helper").unwrap();
        assert_eq!(detail.summary.status, PluginStatus::Quarantined);
    }

    #[test]
    fn enable_plugin_rejects_uninstalled_even_when_ready() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        save_plugin_config(
            &db,
            "community.prompt-helper",
            serde_json::json!({"mode": "append_instruction"}),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "community.prompt-helper",
            vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
        )
        .unwrap();
        uninstall_plugin(&db, "community.prompt-helper").unwrap();

        let err =
            enable_plugin(&db, "community.prompt-helper", env!("CARGO_PKG_VERSION")).unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_INVALID_STATUS:"));
        let detail = get_plugin_detail(&db, "community.prompt-helper").unwrap();
        assert_eq!(detail.summary.status, PluginStatus::Uninstalled);
    }

    #[test]
    fn enable_plugin_rejects_wasm_when_host_policy_disables_execution() {
        let dir = tempfile::tempdir().expect("db dir");
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).expect("db");
        install_plugin_manifest(
            &db,
            wasm_manifest("acme.wasm-policy"),
            PluginInstallSource::Local,
            Some(dir.path().to_string_lossy().to_string()),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("install");
        grant_plugin_permissions(
            &db,
            "acme.wasm-policy",
            vec!["request.body.read".to_string()],
        )
        .expect("grant");

        let err = enable_plugin(&db, "acme.wasm-policy", env!("CARGO_PKG_VERSION"))
            .expect_err("wasm should not enable without policy");

        assert_eq!(err.code(), "PLUGIN_RUNTIME_DISABLED");
    }

    #[test]
    fn uninstall_keeps_audit_logs() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();

        uninstall_plugin(&db, "community.prompt-helper").unwrap();
        let detail = get_plugin_detail(&db, "community.prompt-helper").unwrap();
        assert_eq!(detail.summary.status, PluginStatus::Uninstalled);
        assert!(!detail.audit_logs.is_empty());
    }

    #[test]
    fn revoke_plugin_permission_removes_grant_and_records_audit() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "community.prompt-helper",
            vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
        )
        .unwrap();

        let detail =
            revoke_plugin_permission(&db, "community.prompt-helper", "request.body.write").unwrap();

        assert_eq!(detail.granted_permissions, vec!["request.body.read"]);
        assert!(detail
            .audit_logs
            .iter()
            .any(|log| log.event_type == "plugin.permissions.revoked"));
    }

    #[test]
    fn official_plugin_install_enable_and_uninstall_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("official-plugins.db")).unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        let installed = install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        assert_eq!(installed.install_source, PluginInstallSource::Official);
        assert_eq!(installed.summary.status, PluginStatus::Disabled);
        assert!(installed
            .installed_dir
            .as_deref()
            .is_some_and(|path| { path.contains("official.privacy-filter") }));
        let installed_dir = std::path::Path::new(installed.installed_dir.as_deref().unwrap());
        assert!(installed_dir.join("plugin.json").exists());
        assert!(installed_dir.join("rules/gitleaks.toml").exists());
        assert!(installed
            .granted_permissions
            .contains(&"log.redact".to_string()));
        assert_eq!(installed.config["redactBeforeUpstream"], true);
        assert_eq!(installed.config["redactLogs"], true);

        let enabled =
            enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();
        assert_eq!(enabled.summary.status, PluginStatus::Enabled);

        let active = enabled_plugins_for_gateway(&db).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].summary.plugin_id, "official.privacy-filter");

        let uninstalled = uninstall_plugin(&db, "official.privacy-filter").unwrap();
        assert_eq!(uninstalled.summary.status, PluginStatus::Uninstalled);
    }

    #[test]
    fn enabled_plugins_for_gateway_repairs_missing_plugin_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("missing-plugin-tables.db")).unwrap();
        {
            let conn = db.open_connection().unwrap();
            conn.execute_batch(
                r#"
DROP TABLE plugin_runtime_failures;
DROP TABLE plugin_market_sources;
DROP TABLE plugin_audit_logs;
DROP TABLE plugin_permissions;
DROP TABLE plugin_configs;
DROP TABLE plugin_versions;
DROP TABLE plugins;
"#,
            )
            .unwrap();
        }

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert!(active.is_empty());
        let conn = db.open_connection().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'plugins' LIMIT 1",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(
            exists,
            "runtime schema repair should recreate plugin tables"
        );
    }

    #[test]
    fn enabled_plugins_for_gateway_skips_manifest_that_no_longer_validates() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("invalid-enabled-plugin.db")).unwrap();

        install_plugin_manifest(
            &db,
            manifest(),
            PluginInstallSource::Local,
            None,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "community.prompt-helper",
            vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
        )
        .unwrap();
        save_plugin_config(
            &db,
            "community.prompt-helper",
            serde_json::json!({"mode": "append_instruction"}),
        )
        .unwrap();
        enable_plugin(&db, "community.prompt-helper", env!("CARGO_PKG_VERSION")).unwrap();
        let mut invalid_manifest = manifest();
        invalid_manifest.api_version = "2.0.0".to_string();
        let invalid_manifest_json = serde_json::to_string(&invalid_manifest).unwrap();
        db.open_connection()
            .unwrap()
            .execute(
                "UPDATE plugins SET manifest_json = ?1 WHERE plugin_id = ?2",
                rusqlite::params![invalid_manifest_json, "community.prompt-helper"],
            )
            .unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert!(active.is_empty());
    }

    #[test]
    fn official_privacy_filter_install_uses_upstream_redaction_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("official-privacy-filter.db")).unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        let installed = install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();

        assert_eq!(installed.install_source, PluginInstallSource::Official);
        assert_eq!(installed.summary.status, PluginStatus::Disabled);
        assert_eq!(installed.summary.runtime, "native:privacyFilter");
        assert_eq!(
            installed.manifest.runtime,
            crate::plugins::PluginRuntime::Native {
                engine: "privacyFilter".to_string()
            }
        );
        assert!(installed
            .installed_dir
            .as_deref()
            .is_some_and(|path| { path.contains("official.privacy-filter") }));
        assert!(installed
            .installed_dir
            .as_deref()
            .is_some_and(|path| path.starts_with(installed_root.to_string_lossy().as_ref())));
        assert_eq!(installed.config["redactBeforeUpstream"], true);
        assert_eq!(installed.config["redactLogs"], true);
        assert_eq!(installed.config["profile"], "balanced");
        assert!(installed
            .granted_permissions
            .contains(&"request.body.write".to_string()));
        assert!(installed
            .granted_permissions
            .contains(&"log.redact".to_string()));
    }

    #[test]
    fn enabled_official_privacy_filter_fills_missing_runtime_config_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("official-privacy-filter.db")).unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        repository::save_plugin_config(
            &db,
            "official.privacy-filter",
            1,
            &serde_json::json!({}),
            &[],
        )
        .unwrap();
        enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].summary.plugin_id, "official.privacy-filter");
        assert_eq!(active[0].config["redactBeforeUpstream"], true);
        assert_eq!(active[0].config["redactLogs"], true);
        assert_eq!(active[0].config["profile"], "balanced");
        assert!(active[0]
            .config
            .get("sensitiveTypes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item == "cn_phone")));
        assert_eq!(
            active[0].config["redactionScopes"],
            serde_json::json!([
                "system_instructions",
                "user_prompts",
                "tool_results",
                "legacy_prompt"
            ])
        );

        let pipeline = GatewayPluginPipeline::for_tests(
            active,
            Arc::new(
                crate::app::plugins::runtime_executor::RuntimeGatewayPluginExecutor::default(),
            ),
            GatewayPluginPipelineConfig::default(),
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let output = rt
            .block_on(
                pipeline.run_request_hook(GatewayRequestHookInput {
                    hook_name: GatewayPluginHookName::RequestAfterBodyRead,
                    trace_id: "trace-privacy-filter-default-config".to_string(),
                    cli_key: "codex".to_string(),
                    method: axum::http::Method::POST,
                    path: "/v1/responses".to_string(),
                    query: None,
                    headers: axum::http::HeaderMap::new(),
                    body: axum::body::Bytes::from(
                        serde_json::json!({
                            "input": [{
                                "type": "message",
                                "role": "user",
                                "content": [{
                                    "type": "input_text",
                                    "text": "你知道 13344441520 是哪里的手机号嘛"
                                }]
                            }]
                        })
                        .to_string(),
                    ),
                    requested_model: Some("gpt-test".to_string()),
                }),
            )
            .unwrap();
        let body = String::from_utf8(output.body.to_vec()).unwrap();
        assert!(body.contains("[电话]"));
        assert!(!body.contains("13344441520"));
    }

    #[test]
    fn enabled_official_privacy_filter_migrates_legacy_sensitive_type_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let db =
            crate::db::init_for_tests(&dir.path().join("official-privacy-filter-legacy-config.db"))
                .unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        repository::save_plugin_config(
            &db,
            "official.privacy-filter",
            1,
            &serde_json::json!({
                "redactBeforeUpstream": true,
                "redactLogs": true,
                "profile": "balanced",
                "sensitiveTypes": ["email"]
            }),
            &[],
        )
        .unwrap();
        enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert_eq!(active.len(), 1);
        assert!(active[0]
            .config
            .get("sensitiveTypes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item == "cn_phone")));
        assert_eq!(
            active[0].config["redactionScopes"],
            serde_json::json!([
                "system_instructions",
                "user_prompts",
                "tool_results",
                "legacy_prompt"
            ])
        );

        let pipeline = GatewayPluginPipeline::for_tests(
            active,
            Arc::new(
                crate::app::plugins::runtime_executor::RuntimeGatewayPluginExecutor::default(),
            ),
            GatewayPluginPipelineConfig::default(),
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let output = rt
            .block_on(
                pipeline.run_request_hook(GatewayRequestHookInput {
                    hook_name: GatewayPluginHookName::RequestBeforeSend,
                    trace_id: "trace-privacy-filter-legacy-config".to_string(),
                    cli_key: "codex".to_string(),
                    method: axum::http::Method::POST,
                    path: "/v1/responses".to_string(),
                    query: None,
                    headers: axum::http::HeaderMap::new(),
                    body: axum::body::Bytes::from(
                        serde_json::json!({
                            "input": [{
                                "type": "message",
                                "role": "user",
                                "content": [{
                                    "type": "input_text",
                                    "text": "你知道 13344441520 是哪里的手机号嘛"
                                }]
                            }]
                        })
                        .to_string(),
                    ),
                    requested_model: Some("gpt-test".to_string()),
                }),
            )
            .unwrap();
        let body = String::from_utf8(output.body.to_vec()).unwrap();
        assert!(body.contains("[电话]"));
        assert!(!body.contains("13344441520"));
    }

    #[test]
    fn enabled_official_privacy_filter_respects_current_config_sensitive_type_choices() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(
            &dir.path().join("official-privacy-filter-current-config.db"),
        )
        .unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        repository::save_plugin_config(
            &db,
            "official.privacy-filter",
            3,
            &serde_json::json!({
                "redactBeforeUpstream": true,
                "redactLogs": true,
                "profile": "balanced",
                "sensitiveTypes": ["email"],
                "redactionScopes": ["user_prompts"]
            }),
            &[],
        )
        .unwrap();
        enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert_eq!(active.len(), 1);
        assert!(active[0]
            .config
            .get("sensitiveTypes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| !items.iter().any(|item| item == "cn_phone")));
        assert_eq!(
            active[0].config["redactionScopes"],
            serde_json::json!(["user_prompts"])
        );
    }

    #[test]
    fn enabled_official_privacy_filter_preserves_existing_redaction_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(
            &dir.path()
                .join("official-privacy-filter-redaction-scopes.db"),
        )
        .unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        repository::save_plugin_config(
            &db,
            "official.privacy-filter",
            2,
            &serde_json::json!({
                "redactBeforeUpstream": true,
                "redactLogs": true,
                "profile": "balanced",
                "sensitiveTypes": ["email"],
                "redactionScopes": ["user_prompts"]
            }),
            &[],
        )
        .unwrap();
        enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].config["redactionScopes"],
            serde_json::json!(["user_prompts"])
        );
    }

    #[test]
    fn enabled_official_privacy_filter_merges_packaged_manifest_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("official-privacy-filter-hooks.db"))
            .unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        let mut legacy = repository::get_plugin(&db, "official.privacy-filter")
            .unwrap()
            .manifest;
        legacy
            .hooks
            .retain(|hook| hook.name != "gateway.request.beforeSend");
        repository::update_plugin_manifest(
            &db,
            legacy,
            Some(installed_root.to_string_lossy().to_string()),
        )
        .unwrap();
        enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();

        let active = enabled_plugins_for_gateway(&db).unwrap();

        assert_eq!(active.len(), 1);
        assert!(active[0]
            .manifest
            .hooks
            .iter()
            .any(|hook| hook.name == "gateway.request.beforeSend"));
    }

    #[test]
    fn official_privacy_filter_detail_and_enable_return_packaged_manifest_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("official-privacy-filter-detail.db"))
            .unwrap();
        let installed_root = dir.path().join("installed");
        let official_root = crate::app::plugins::official::official_resource_root_for_tests();

        install_official_plugin(
            &db,
            "official.privacy-filter",
            &official_root,
            env!("CARGO_PKG_VERSION"),
            &installed_root,
        )
        .unwrap();
        let mut legacy = repository::get_plugin(&db, "official.privacy-filter")
            .unwrap()
            .manifest;
        legacy
            .hooks
            .retain(|hook| hook.name != "gateway.request.beforeSend");
        repository::update_plugin_manifest(
            &db,
            legacy,
            Some(installed_root.to_string_lossy().to_string()),
        )
        .unwrap();

        let enabled =
            enable_plugin(&db, "official.privacy-filter", env!("CARGO_PKG_VERSION")).unwrap();
        let detail = get_plugin_detail(&db, "official.privacy-filter").unwrap();

        for item in [&enabled, &detail] {
            assert!(item
                .manifest
                .hooks
                .iter()
                .any(|hook| hook.name == "gateway.request.beforeSend"));
            assert_eq!(item.config["redactBeforeUpstream"], true);
            assert!(item
                .config
                .get("sensitiveTypes")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| items.iter().any(|item| item == "cn_phone")));
        }
    }

    fn local_package_manifest(plugin_id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({
            "id": plugin_id,
            "name": "Local Package Plugin",
            "version": version,
            "apiVersion": "1.0.0",
            "runtime": {
                "kind": "declarativeRules",
                "rules": ["rules/main.json"]
            },
            "hooks": [
                {
                    "name": "gateway.request.afterBodyRead",
                    "priority": 10,
                    "failurePolicy": "fail-open"
                }
            ],
            "permissions": ["request.meta.read"],
            "hostCompatibility": {
                "app": ">=0.56.0 <1.0.0",
                "pluginApi": "^1.0.0",
                "platforms": ["macos", "windows", "linux"]
            }
        })
    }

    fn write_local_package(path: &Path, manifest: serde_json::Value) {
        let file = std::fs::File::create(path).expect("create package");
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::<()>::default();
        zip.start_file("plugin.json", opts).expect("manifest entry");
        zip.write_all(manifest.to_string().as_bytes())
            .expect("manifest bytes");
        zip.start_file("rules/main.json", opts)
            .expect("rules entry");
        zip.write_all(br#"{"rules":[]}"#).expect("rules bytes");
        zip.finish().expect("finish package");
    }

    fn invalid_checksum() -> String {
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string()
    }

    #[test]
    fn plugin_local_install_preview_reports_identity_risk_and_trust_without_db_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("preview-safe.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("local.preview-safe", "1.0.0"),
        );

        let preview = preview_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(preview.plugin_id, "local.preview-safe");
        assert_eq!(preview.name, "Local Package Plugin");
        assert_eq!(preview.version, "1.0.0");
        assert_eq!(preview.source, PluginInstallSource::Local);
        assert_eq!(preview.runtime.kind, "declarativeRules");
        assert!(preview.runtime.supported);
        assert!(preview.compatibility.compatible);
        assert!(preview.trust.unsigned);
        assert!(!preview.trust.signature_verified);
        assert_eq!(preview.permissions[0].permission, "request.meta.read");
        assert_eq!(preview.permissions[0].risk, PluginPermissionRisk::Low);
        assert!(preview.blocking_reasons.is_empty());
        assert!(repository::get_plugin(&db, "local.preview-safe").is_err());
    }

    #[test]
    fn plugin_local_install_preview_reports_incompatible_manifest_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("preview-incompatible.aio-plugin");
        let mut manifest = local_package_manifest("local.preview-incompatible", "1.0.0");
        manifest["hostCompatibility"] = serde_json::json!({
            "app": ">=999.0.0 <1000.0.0",
            "pluginApi": "^1.0.0",
            "platforms": ["macos", "windows", "linux"]
        });
        write_local_package(&package_path, manifest);

        let preview = preview_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(preview.plugin_id, "local.preview-incompatible");
        assert!(!preview.compatibility.compatible);
        assert!(preview
            .blocking_reasons
            .iter()
            .any(|notice| notice.code == "PLUGIN_INCOMPATIBLE_HOST"));
        assert!(repository::get_plugin(&db, "local.preview-incompatible").is_err());
    }

    #[test]
    fn plugin_local_install_preview_blocks_unsigned_high_risk_with_default_policy() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("preview-risky.aio-plugin");
        let mut manifest = local_package_manifest("local.preview-risky", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.body.read"]);
        write_local_package(&package_path, manifest);

        let preview = preview_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy::default(),
        )
        .unwrap();

        assert!(preview.blocking_reasons.iter().any(|notice| {
            notice.code == "PLUGIN_UNSIGNED_HIGH_RISK_PERMISSION"
                && notice.message.contains("request.body.read")
        }));
        assert!(repository::get_plugin(&db, "local.preview-risky").is_err());
    }

    #[test]
    fn plugin_local_update_preview_reports_permission_runtime_hook_and_config_changes() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("diff-v1.aio-plugin");
        write_local_package(&v1_package, local_package_manifest("local.diff", "1.0.0"));
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        grant_plugin_permissions(&db, "local.diff", vec!["request.meta.read".to_string()]).unwrap();

        let v2_package = dir.path().join("diff-v2.aio-plugin");
        let mut v2_manifest = local_package_manifest("local.diff", "1.1.0");
        v2_manifest["configVersion"] = serde_json::json!(2);
        v2_manifest["hooks"] = serde_json::json!([
            {
                "name": "gateway.request.afterBodyRead",
                "priority": 10,
                "failurePolicy": "fail-open"
            },
            {
                "name": "gateway.request.beforeSend",
                "priority": 20,
                "failurePolicy": "fail-open"
            }
        ]);
        v2_manifest["permissions"] =
            serde_json::json!(["request.meta.read", "request.header.read"]);
        write_local_package(&v2_package, v2_manifest);

        let diff = preview_plugin_update_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(diff.plugin_id, "local.diff");
        assert_eq!(diff.from_version, "1.0.0");
        assert_eq!(diff.to_version, "1.1.0");
        assert_eq!(diff.version_direction, "upgrade");
        assert_eq!(diff.config_version_change.as_deref(), Some("1 -> 2"));
        assert!(diff.rollback_available);
        assert!(diff
            .hook_changes
            .iter()
            .any(|change| change.name == "gateway.request.beforeSend" && change.change == "added"));
        assert!(diff.permission_changes.iter().any(|change| {
            change.permission == "request.meta.read" && change.change == "unchanged_granted"
        }));
        assert!(diff.permission_changes.iter().any(|change| {
            change.permission == "request.header.read" && change.change == "added_pending"
        }));
        assert!(diff.blocking_reasons.is_empty());
    }

    #[test]
    fn plugin_local_update_preview_reports_prerelease_version_direction() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let rc_package = dir.path().join("prerelease-rc.aio-plugin");
        write_local_package(
            &rc_package,
            local_package_manifest("local.prerelease", "1.0.0-rc.1"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &rc_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let release_package = dir.path().join("prerelease-release.aio-plugin");
        write_local_package(
            &release_package,
            local_package_manifest("local.prerelease", "1.0.0"),
        );
        let release_diff = preview_plugin_update_from_local_package(
            &db,
            &release_package,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(release_diff.from_version, "1.0.0-rc.1");
        assert_eq!(release_diff.to_version, "1.0.0");
        assert_eq!(release_diff.version_direction, "upgrade");
        assert!(release_diff
            .warnings
            .iter()
            .all(|notice| notice.code != "PLUGIN_UPDATE_DOWNGRADE"));

        update_plugin_from_local_package(
            &db,
            &release_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let rc_diff = preview_plugin_update_from_local_package(
            &db,
            &rc_package,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(rc_diff.from_version, "1.0.0");
        assert_eq!(rc_diff.to_version, "1.0.0-rc.1");
        assert_eq!(rc_diff.version_direction, "downgrade");
        assert!(rc_diff
            .warnings
            .iter()
            .any(|notice| notice.code == "PLUGIN_UPDATE_DOWNGRADE"));
    }

    #[test]
    fn plugin_local_update_preview_reports_rollback_unavailable_when_current_install_dir_is_missing(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("rollback-v1.aio-plugin");
        let v2_package = dir.path().join("rollback-v2.aio-plugin");
        let v3_package = dir.path().join("rollback-v3.aio-plugin");
        write_local_package(
            &v1_package,
            local_package_manifest("local.rollback-preview", "1.0.0"),
        );
        write_local_package(
            &v2_package,
            local_package_manifest("local.rollback-preview", "1.1.0"),
        );
        write_local_package(
            &v3_package,
            local_package_manifest("local.rollback-preview", "1.2.0"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let diff_with_current_dir = preview_plugin_update_from_local_package(
            &db,
            &v3_package,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert!(diff_with_current_dir.rollback_available);

        let v2_installed_dir = installed_dir.join("local.rollback-preview").join("1.1.0");
        assert!(v2_installed_dir.is_dir());
        std::fs::remove_dir_all(&v2_installed_dir).unwrap();

        let diff_without_current_dir = preview_plugin_update_from_local_package(
            &db,
            &v3_package,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert!(!diff_without_current_dir.rollback_available);
    }

    fn signed_package_policy(
        package_path: &Path,
        key_seed: u8,
    ) -> (String, LocalPackageInstallPolicy) {
        use base64::Engine;
        use ed25519_dalek::Signer;
        use sha2::{Digest, Sha256};

        let package_bytes = std::fs::read(package_path).unwrap();
        let expected_checksum = crate::infra::plugins::signing::verify_checksum(
            &package_bytes,
            &format!("sha256:{:x}", Sha256::digest(&package_bytes)),
        )
        .unwrap();
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[key_seed; 32]);
        let signature = signing_key.sign(&package_bytes);
        let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        let public_key_b64 = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());

        (
            expected_checksum,
            LocalPackageInstallPolicy {
                signature: Some(signature_b64),
                public_key: Some(public_key_b64),
                allow_unsigned: false,
                developer_mode: false,
                ..LocalPackageInstallPolicy::default()
            },
        )
    }

    fn hex_to_bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn plugin_local_install_installs_package_into_cache_and_installed_dir() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("local-safe.aio-plugin");
        write_local_package(&package_path, local_package_manifest("local.safe", "1.0.0"));
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");

        let detail = install_plugin_from_local_package(
            &db,
            &package_path,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();

        assert_eq!(detail.summary.plugin_id, "local.safe");
        assert_eq!(detail.install_source, PluginInstallSource::Local);
        assert_eq!(detail.summary.status, PluginStatus::Disabled);
        assert!(detail
            .installed_dir
            .as_deref()
            .is_some_and(|path| path.ends_with("plugins/installed/local.safe/1.0.0")));
        assert!(installed_dir
            .join("local.safe")
            .join("1.0.0")
            .join("rules/main.json")
            .exists());
        let cached_packages: Vec<_> = std::fs::read_dir(&cache_dir).unwrap().collect();
        assert_eq!(cached_packages.len(), 1);
        assert!(detail
            .audit_logs
            .iter()
            .any(|log| log.event_type == "plugin.installed"));
    }

    #[test]
    fn plugin_local_install_rolls_back_invalid_package_without_db_row_or_install_dir() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("invalid.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("local.bad", "not-semver"),
        );
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");

        let err = install_plugin_from_local_package(
            &db,
            &package_path,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_INVALID_VERSION:"));
        assert!(repository::get_plugin(&db, "local.bad").is_err());
        assert!(!installed_dir.join("local.bad").exists());
        assert!(!cache_dir.join("staging").exists());
    }

    #[test]
    fn plugin_local_install_rejects_reserved_official_privacy_filter_native_package() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("fake-official-privacy-filter.aio-plugin");
        let mut manifest = local_package_manifest("official.privacy-filter", "1.0.0");
        manifest["runtime"] = serde_json::json!({
            "kind": "native",
            "engine": "privacyFilter"
        });
        manifest["hooks"] = serde_json::json!([
            {
                "name": "gateway.request.afterBodyRead",
                "priority": 10,
                "failurePolicy": "fail-open"
            },
            {
                "name": "log.beforePersist",
                "priority": 20,
                "failurePolicy": "fail-closed"
            }
        ]);
        manifest["permissions"] =
            serde_json::json!(["request.body.read", "request.body.write", "log.redact"]);
        write_local_package(&package_path, manifest);

        let err = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_RESERVED_OFFICIAL_ID:"));
        assert!(repository::get_plugin(&db, "official.privacy-filter").is_err());
    }

    #[test]
    fn plugin_package_security_rejects_checksum_mismatch_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("checksum-mismatch.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("local.checksum", "1.0.0"),
        );
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");

        let err = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                expected_checksum: Some(invalid_checksum()),
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_CHECKSUM_MISMATCH:"));
        assert!(repository::get_plugin(&db, "local.checksum").is_err());
        assert!(!installed_dir.join("local.checksum").exists());
        assert!(!cache_dir.join("staging").exists());
    }

    #[test]
    fn plugin_signature_verification_accepts_signed_local_install() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("signed-valid.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("local.signed-valid", "1.0.0"),
        );
        let (expected_checksum, mut policy) = signed_package_policy(&package_path, 7);
        policy.expected_checksum = Some(expected_checksum);

        let detail = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            policy,
        )
        .unwrap();

        assert_eq!(detail.summary.plugin_id, "local.signed-valid");
        assert_eq!(detail.summary.status, PluginStatus::Disabled);
        assert!(detail
            .installed_dir
            .as_deref()
            .is_some_and(|path| path.ends_with("plugins/installed/local.signed-valid/1.0.0")));
    }

    #[test]
    fn plugin_signature_verification_allows_high_risk_signed_local_install() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("signed-risky.aio-plugin");
        let mut manifest = local_package_manifest("local.signed-risky", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.body.read"]);
        write_local_package(&package_path, manifest);
        let (expected_checksum, mut policy) = signed_package_policy(&package_path, 8);
        policy.expected_checksum = Some(expected_checksum);

        let detail = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            policy,
        )
        .unwrap();

        assert_eq!(detail.summary.plugin_id, "local.signed-risky");
        let install_audit = detail
            .audit_logs
            .iter()
            .find(|log| log.event_type == "plugin.installed")
            .unwrap();
        assert_eq!(install_audit.details["unsigned"], false);
    }

    #[test]
    fn plugin_local_package_install_records_manifest_permissions_as_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("signed-pending-permissions.aio-plugin");
        let mut manifest = local_package_manifest("local.pending-permissions", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.body.read", "request.body.write"]);
        write_local_package(&package_path, manifest);
        let (expected_checksum, mut policy) = signed_package_policy(&package_path, 9);
        policy.expected_checksum = Some(expected_checksum);

        let detail = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            policy,
        )
        .unwrap();

        assert_eq!(detail.granted_permissions, Vec::<String>::new());
        assert_eq!(
            detail.pending_permissions,
            vec!["request.body.read", "request.body.write"]
        );
    }

    #[test]
    fn plugin_market_revoked_quarantines_installed_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("revoked-v1.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("community.revoked", "1.0.0"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let detail =
            quarantine_revoked_plugin(&db, "community.revoked", "Plugin revoked by market index")
                .unwrap();

        assert_eq!(detail.summary.status, PluginStatus::Quarantined);
        assert_eq!(
            detail.summary.last_error.as_deref(),
            Some("Plugin revoked by market index")
        );
        assert!(detail
            .audit_logs
            .iter()
            .any(|log| log.event_type == "plugin.quarantined"));
    }

    #[test]
    fn github_release_plugin_install_installs_verified_artifact_bytes() {
        use sha2::Digest;

        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("github-release.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("github.release", "1.0.0"),
        );
        let package_bytes = std::fs::read(&package_path).unwrap();
        let checksum = format!("sha256:{:x}", sha2::Sha256::digest(&package_bytes));
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");

        let detail = install_plugin_from_remote_package_bytes(
            &db,
            package_bytes.clone(),
            "https://github.com/acme/release/releases/download/v1/plugin.aio-plugin",
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::GithubRelease,
                expected_plugin_id: "github.release".to_string(),
                expected_checksum: checksum,
                signature: None,
                public_key: None,
                market_source_url: None,
            },
        )
        .unwrap();

        assert_eq!(detail.install_source, PluginInstallSource::GithubRelease);
        assert_eq!(detail.summary.plugin_id, "github.release");
        assert!(installed_dir
            .join("github.release")
            .join("1.0.0")
            .join("plugin.json")
            .exists());
    }

    #[test]
    fn github_release_plugin_install_ignores_market_source_url_in_audit() {
        use sha2::Digest;

        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("github-release-market-url.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("github.market-url", "1.0.0"),
        );
        let package_bytes = std::fs::read(&package_path).unwrap();
        let checksum = format!("sha256:{:x}", sha2::Sha256::digest(&package_bytes));

        let detail = install_plugin_from_remote_package_bytes(
            &db,
            package_bytes,
            "https://github.com/acme/release/releases/download/v1/plugin.aio-plugin",
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::GithubRelease,
                expected_plugin_id: "github.market-url".to_string(),
                expected_checksum: checksum,
                signature: None,
                public_key: None,
                market_source_url: Some("https://plugins.example.test/index.json".to_string()),
            },
        )
        .unwrap();

        let install_audit = detail
            .audit_logs
            .iter()
            .find(|log| log.event_type == "plugin.remote.installed")
            .unwrap();
        assert_eq!(install_audit.details["source"], "github_release");
        assert!(install_audit.details.get("marketSourceUrl").is_none());
    }

    #[test]
    fn github_release_plugin_install_rejects_checksum_mismatch_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("github-release-bad.aio-plugin");
        write_local_package(&package_path, local_package_manifest("github.bad", "1.0.0"));
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");

        let err = install_plugin_from_remote_package_bytes(
            &db,
            std::fs::read(&package_path).unwrap(),
            "https://github.com/acme/release/releases/download/v1/plugin.aio-plugin",
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::GithubRelease,
                expected_plugin_id: "github.bad".to_string(),
                expected_checksum: invalid_checksum(),
                signature: None,
                public_key: None,
                market_source_url: None,
            },
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_CHECKSUM_MISMATCH:"));
        assert!(repository::get_plugin(&db, "github.bad").is_err());
        assert!(!installed_dir.join("github.bad").exists());
        assert!(!cache_dir.join("staging").exists());
    }

    #[test]
    fn plugin_remote_install_uses_trusted_market_source_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("market-signed-risky.aio-plugin");
        let mut manifest = local_package_manifest("market.signed-risky", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.body.read"]);
        write_local_package(&package_path, manifest);
        let package_bytes = std::fs::read(&package_path).unwrap();
        let (expected_checksum, trusted_policy) = signed_package_policy(&package_path, 9);
        let trusted_public_key = trusted_policy.public_key.clone().unwrap();
        let caller_public_key = signed_package_policy(&package_path, 10)
            .1
            .public_key
            .unwrap();
        let conn = db.open_connection().unwrap();
        conn.execute(
            r#"
INSERT INTO plugin_market_sources(
  name,
  index_url,
  enabled,
  trusted_public_key,
  created_at,
  updated_at
) VALUES (?1, ?2, 1, ?3, 1, 1)
"#,
            rusqlite::params![
                "Community",
                "https://plugins.example.test/index.json",
                trusted_public_key
            ],
        )
        .unwrap();
        drop(conn);

        let detail = install_plugin_from_remote_package_bytes(
            &db,
            package_bytes,
            "https://plugins.example.test/download/market-signed-risky.aio-plugin",
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::Market,
                expected_plugin_id: "market.signed-risky".to_string(),
                expected_checksum,
                signature: trusted_policy.signature,
                public_key: Some(caller_public_key),
                market_source_url: None,
            },
        )
        .unwrap();

        assert_eq!(detail.summary.plugin_id, "market.signed-risky");
        assert_eq!(detail.install_source, PluginInstallSource::Market);
        assert_eq!(detail.granted_permissions, Vec::<String>::new());
        assert_eq!(detail.pending_permissions, vec!["request.body.read"]);
        let install_audit = detail
            .audit_logs
            .iter()
            .find(|log| log.event_type == "plugin.remote.installed")
            .unwrap();
        assert_eq!(install_audit.details["source"], "market");
        assert_eq!(
            install_audit.details["sourceUrl"],
            "https://plugins.example.test/download/market-signed-risky.aio-plugin"
        );
        assert_eq!(install_audit.details["unsigned"], false);
        assert_eq!(install_audit.details["signatureVerified"], true);
    }

    #[test]
    fn plugin_remote_install_uses_market_source_url_when_download_host_differs() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("market-cdn-signed.aio-plugin");
        let mut manifest = local_package_manifest("market.cdn-signed", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.body.read"]);
        write_local_package(&package_path, manifest);
        let package_bytes = std::fs::read(&package_path).unwrap();
        let (expected_checksum, trusted_policy) = signed_package_policy(&package_path, 19);
        let trusted_public_key = trusted_policy.public_key.clone().unwrap();

        let conn = db.open_connection().unwrap();
        conn.execute(
            r#"
INSERT INTO plugin_market_sources(
  name,
  index_url,
  enabled,
  trusted_public_key,
  created_at,
  updated_at
) VALUES (?1, ?2, 1, ?3, 1, 1)
"#,
            rusqlite::params![
                "Community CDN",
                "https://plugins.example.test/index.json",
                trusted_public_key
            ],
        )
        .unwrap();
        drop(conn);

        let detail = install_plugin_from_remote_package_bytes(
            &db,
            package_bytes,
            "https://cdn.example.test/download/market-cdn-signed.aio-plugin",
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::Market,
                expected_plugin_id: "market.cdn-signed".to_string(),
                expected_checksum,
                signature: trusted_policy.signature,
                public_key: None,
                market_source_url: Some("https://plugins.example.test/index.json".to_string()),
            },
        )
        .unwrap();

        assert_eq!(detail.summary.plugin_id, "market.cdn-signed");
        assert_eq!(detail.install_source, PluginInstallSource::Market);
        let install_audit = detail
            .audit_logs
            .iter()
            .find(|log| log.event_type == "plugin.remote.installed")
            .unwrap();
        assert_eq!(
            install_audit.details["sourceUrl"],
            "https://cdn.example.test/download/market-cdn-signed.aio-plugin"
        );
        assert_eq!(
            install_audit.details["marketSourceUrl"],
            "https://plugins.example.test/index.json"
        );
        assert_eq!(install_audit.details["signatureVerified"], true);
    }

    #[test]
    fn plugin_remote_install_rejects_signed_market_package_without_trusted_source() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("market-untrusted.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("market.untrusted", "1.0.0"),
        );
        let package_bytes = std::fs::read(&package_path).unwrap();
        let (expected_checksum, policy) = signed_package_policy(&package_path, 11);

        let err = install_plugin_from_remote_package_bytes(
            &db,
            package_bytes,
            "https://untrusted.example.test/download/market-untrusted.aio-plugin",
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            RemotePackageInstallPolicy {
                install_source: PluginInstallSource::Market,
                expected_plugin_id: "market.untrusted".to_string(),
                expected_checksum,
                signature: policy.signature,
                public_key: policy.public_key,
                market_source_url: None,
            },
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .starts_with("PLUGIN_MARKET_TRUSTED_PUBLIC_KEY_REQUIRED:"));
        assert!(repository::get_plugin(&db, "market.untrusted").is_err());
    }

    #[test]
    fn plugin_unsigned_offline_install_rejects_high_risk_permissions_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("unsigned-risky.aio-plugin");
        let mut manifest = local_package_manifest("local.risky", "1.0.0");
        manifest["permissions"] = serde_json::json!(["request.header.readSensitive"]);
        write_local_package(&package_path, manifest);

        let err = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: false,
                developer_mode: false,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .starts_with("PLUGIN_UNSIGNED_HIGH_RISK_PERMISSION:"));
        assert!(repository::get_plugin(&db, "local.risky").is_err());
    }

    #[test]
    fn plugin_unsigned_offline_install_allows_low_risk_in_developer_mode_as_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let package_path = dir.path().join("unsigned-low-risk.aio-plugin");
        write_local_package(
            &package_path,
            local_package_manifest("local.low-risk", "1.0.0"),
        );

        let detail = install_plugin_from_local_package_with_policy(
            &db,
            &package_path,
            &dir.path().join("plugins/cache"),
            &dir.path().join("plugins/installed"),
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(detail.summary.status, PluginStatus::Disabled);
        assert!(detail
            .audit_logs
            .iter()
            .any(|log| log.details["unsigned"] == true));
    }

    #[test]
    fn plugin_update_rollback_marks_new_permissions_pending_and_keeps_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("plugin-v1.aio-plugin");
        write_local_package(
            &v1_package,
            local_package_manifest("local.updatable", "1.0.0"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        save_plugin_config(&db, "local.updatable", serde_json::json!({"enabled": true})).unwrap();
        grant_plugin_permissions(
            &db,
            "local.updatable",
            vec!["request.meta.read".to_string()],
        )
        .unwrap();

        let v2_package = dir.path().join("plugin-v2.aio-plugin");
        let mut v2_manifest = local_package_manifest("local.updatable", "1.1.0");
        v2_manifest["permissions"] =
            serde_json::json!(["request.meta.read", "request.header.read"]);
        write_local_package(&v2_package, v2_manifest);

        let updated = update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        assert_eq!(updated.summary.current_version.as_deref(), Some("1.1.0"));
        assert_eq!(updated.config["enabled"], true);
        assert_eq!(updated.granted_permissions, vec!["request.meta.read"]);
        assert_eq!(updated.pending_permissions, vec!["request.header.read"]);
    }

    #[test]
    fn plugin_update_rollback_keeps_old_version_when_new_package_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("plugin-v1.aio-plugin");
        write_local_package(
            &v1_package,
            local_package_manifest("local.rollback", "1.0.0"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let bad_package = dir.path().join("plugin-bad.aio-plugin");
        write_local_package(
            &bad_package,
            local_package_manifest("local.rollback", "not-semver"),
        );

        let err = update_plugin_from_local_package(
            &db,
            &bad_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_INVALID_VERSION:"));
        let current = get_plugin_detail(&db, "local.rollback").unwrap();
        assert_eq!(current.summary.current_version.as_deref(), Some("1.0.0"));
        assert!(installed_dir
            .join("local.rollback")
            .join("1.0.0")
            .join("plugin.json")
            .exists());
        assert!(!installed_dir
            .join("local.rollback")
            .join("not-semver")
            .exists());
    }

    #[test]
    fn plugin_update_rollback_rejects_invalid_signature_and_keeps_old_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("signed-plugin-v1.aio-plugin");
        write_local_package(&v1_package, local_package_manifest("local.signed", "1.0.0"));
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let v2_package = dir.path().join("signed-plugin-v2.aio-plugin");
        write_local_package(&v2_package, local_package_manifest("local.signed", "1.1.0"));
        let signature_for_empty_payload = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            hex_to_bytes("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b").as_slice(),
        );

        let err = update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                signature: Some(signature_for_empty_payload),
                public_key: Some("11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=".to_string()),
                allow_unsigned: false,
                developer_mode: false,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_SIGNATURE_INVALID:"));
        let current = get_plugin_detail(&db, "local.signed").unwrap();
        assert_eq!(current.summary.current_version.as_deref(), Some("1.0.0"));
        assert!(installed_dir
            .join("local.signed")
            .join("1.0.0")
            .join("plugin.json")
            .exists());
        assert!(!installed_dir.join("local.signed").join("1.1.0").exists());
    }

    #[test]
    fn plugin_update_rollback_can_manually_restore_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("plugin-v1.aio-plugin");
        let v2_package = dir.path().join("plugin-v2.aio-plugin");
        write_local_package(&v1_package, local_package_manifest("local.manual", "1.0.0"));
        write_local_package(&v2_package, local_package_manifest("local.manual", "1.1.0"));
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let rolled_back = rollback_plugin_to_version(&db, "local.manual", "1.0.0").unwrap();

        assert_eq!(
            rolled_back.summary.current_version.as_deref(),
            Some("1.0.0")
        );
        assert!(rolled_back
            .installed_dir
            .as_deref()
            .is_some_and(|path| path.ends_with("plugins/installed/local.manual/1.0.0")));
    }

    #[test]
    fn plugin_update_rollback_reconciles_permissions_and_config_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("plugin-v1.aio-plugin");
        let v2_package = dir.path().join("plugin-v2.aio-plugin");

        let mut v1_manifest = local_package_manifest("local.rollback-state", "1.0.0");
        v1_manifest["configVersion"] = serde_json::json!(1);
        v1_manifest["permissions"] = serde_json::json!(["request.meta.read"]);
        write_local_package(&v1_package, v1_manifest);

        let mut v2_manifest = local_package_manifest("local.rollback-state", "1.1.0");
        v2_manifest["configVersion"] = serde_json::json!(2);
        v2_manifest["permissions"] =
            serde_json::json!(["request.meta.read", "request.header.read"]);
        write_local_package(&v2_package, v2_manifest);

        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        save_plugin_config(
            &db,
            "local.rollback-state",
            serde_json::json!({"enabled": true, "extra": "kept"}),
        )
        .unwrap();
        grant_plugin_permissions(
            &db,
            "local.rollback-state",
            vec!["request.meta.read".to_string()],
        )
        .unwrap();

        let updated = update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(updated.pending_permissions, vec!["request.header.read"]);

        let rolled_back = rollback_plugin_to_version(&db, "local.rollback-state", "1.0.0").unwrap();

        assert_eq!(
            rolled_back.summary.current_version.as_deref(),
            Some("1.0.0")
        );
        assert_eq!(rolled_back.granted_permissions, vec!["request.meta.read"]);
        assert!(rolled_back.pending_permissions.is_empty());
        assert_eq!(rolled_back.config["enabled"], true);
        assert_eq!(
            repository::plugin_config_version(&db, "local.rollback-state").unwrap(),
            Some(1)
        );
        assert!(enable_plugin(&db, "local.rollback-state", env!("CARGO_PKG_VERSION")).is_ok());
    }

    #[test]
    fn plugin_update_rollback_rejects_missing_historical_install_dir() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("plugins.db")).unwrap();
        let cache_dir = dir.path().join("plugins/cache");
        let installed_dir = dir.path().join("plugins/installed");
        let v1_package = dir.path().join("plugin-v1.aio-plugin");
        let v2_package = dir.path().join("plugin-v2.aio-plugin");
        write_local_package(
            &v1_package,
            local_package_manifest("local.missing-rollback", "1.0.0"),
        );
        write_local_package(
            &v2_package,
            local_package_manifest("local.missing-rollback", "1.1.0"),
        );
        install_plugin_from_local_package_with_policy(
            &db,
            &v1_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();
        update_plugin_from_local_package(
            &db,
            &v2_package,
            &cache_dir,
            &installed_dir,
            env!("CARGO_PKG_VERSION"),
            LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..LocalPackageInstallPolicy::default()
            },
        )
        .unwrap();

        let v1_installed_dir = installed_dir.join("local.missing-rollback").join("1.0.0");
        assert!(v1_installed_dir.is_dir());
        std::fs::remove_dir_all(&v1_installed_dir).unwrap();

        let err = rollback_plugin_to_version(&db, "local.missing-rollback", "1.0.0").unwrap_err();

        assert!(err.to_string().starts_with("PLUGIN_ROLLBACK_UNAVAILABLE:"));
        let current = get_plugin_detail(&db, "local.missing-rollback").unwrap();
        assert_eq!(current.summary.current_version.as_deref(), Some("1.1.0"));
    }
}
