# aio-coding-hub 0.62.2 Plugin Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the 0.62.2 plugin distribution and lifecycle release: install preview, update diff, lifecycle state explanation, rollback/quarantine visibility, and tests, without changing Plugin API v1.

**Architecture:** Keep Plugin API v1 stable and add a host-owned lifecycle explanation layer around the existing install/update/rollback/quarantine service. Rust remains the source of truth for package inspection, compatibility, trust, and lifecycle diffs; Tauri commands expose preview/diff models; the React UI renders those models and stops inferring lifecycle state from scattered audit details.

**Tech Stack:** Rust, Tauri 2 commands, Specta generated TypeScript bindings, React 19, TanStack Query, Vitest, Cargo tests, Markdown docs.

---

## Scope Boundaries

- Do not change public `plugin.json` v1 shape.
- Do not add Plugin API v2.
- Do not expose Provider Plugin API.
- Do not enable JS, TypeScript, WebView, or arbitrary marketplace WASM runtime.
- Do not build a full marketplace page, plugin source CRUD, rating/review system, or automatic background updater.
- Preview/diff is an explanation layer only. Install/update must re-run package extraction, compatibility, checksum, signature, runtime, and permission policy checks.

## File Structure

- Modify: `src-tauri/src/domain/plugins.rs`
  - Add lifecycle DTOs exported through Specta.
  - Keep these types data-only; do not put filesystem or DB logic here.
- Modify: `src-tauri/src/infra/plugins/package.rs`
  - Add an inspection extraction path that still enforces archive safety but lets preview report manifest compatibility blockers instead of failing before a preview can be built.
- Modify: `src-tauri/src/app/plugin_service.rs`
  - Add local package install preview and update diff service functions.
  - Reuse existing package extraction, manifest validation, trust policy, compatibility, permission risk, and repository helpers.
  - Keep install/update state mutation in existing install/update functions.
- Modify: `src-tauri/src/commands/plugins.rs`
  - Add Tauri commands for local install preview and local update preview.
  - Keep remote install restrictions and existing install/update commands unchanged.
- Modify: `src-tauri/src/commands/registry.rs`
  - Register new plugin preview commands for runtime and Specta export.
- Modify: `src/generated/bindings.ts`
  - Regenerate through `pnpm tauri:gen-types`; do not hand-edit generated output.
- Modify: `src/services/plugins.ts`
  - Add IPC wrappers and type exports for preview/diff.
- Modify: `src/query/keys.ts`
  - Add query keys for local install preview and local update preview.
- Modify: `src/query/plugins.ts`
  - Add preview/diff query helpers or mutations for selected file paths.
- Create: `src/pages/plugins/PluginInstallPreviewDialog.tsx`
  - Render install preview and confirmation controls.
- Create: `src/pages/plugins/PluginUpdatePreviewDialog.tsx`
  - Render update diff and confirmation controls.
- Create: `src/pages/plugins/PluginLifecyclePanel.tsx`
  - Render status/source/trust/version/quarantine/rollback summary inside plugin detail.
- Modify: `src/pages/PluginsPage.tsx`
  - Wire file selection -> preview dialog -> confirm install/update.
  - Use `PluginLifecyclePanel` in the detail panel.
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`
  - Cover preview, update diff, quarantine, rollback, and pending permission UI.
- Modify: `docs/plugins/developer-guide.md`
  - Document lifecycle preview and update diff in the install/update workflow.
- Modify: `docs/plugins/reference/publishing.md`
  - Document package trust, signature/checksum, update permission delta, rollback, and quarantine behavior.
- Modify: `docs/plugins/reference/compatibility.md`
  - Document 0.62.2 compatibility and lifecycle boundary.

## Task 1: Add Lifecycle DTOs And Rust Failing Tests

**Files:**

- Modify: `src-tauri/src/domain/plugins.rs`
- Modify: `src-tauri/src/infra/plugins/package.rs`
- Modify: `src-tauri/src/app/plugin_service.rs`

- [ ] **Step 1: Add failing service tests for install preview**

Add this test inside `#[cfg(test)] mod tests` in `src-tauri/src/app/plugin_service.rs` near the local package tests:

```rust
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
```

- [ ] **Step 2: Add failing service test for incompatible install preview**

Add this test in the same test module:

```rust
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
```

- [ ] **Step 3: Add failing service tests for update diff**

Add this test in the same test module:

```rust
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
```

- [ ] **Step 4: Run the failing Rust tests**

Run:

```bash
cd src-tauri && cargo test plugin_local_install_preview_reports_identity_risk_and_trust_without_db_mutation --lib
cd src-tauri && cargo test plugin_local_install_preview_reports_incompatible_manifest_without_installing --lib
cd src-tauri && cargo test plugin_local_update_preview_reports_permission_runtime_hook_and_config_changes --lib
```

Expected: all three fail because `preview_plugin_from_local_package_with_policy`, `preview_plugin_update_from_local_package`, the inspection extractor, and lifecycle DTO fields do not exist.

- [ ] **Step 5: Add lifecycle DTOs**

In `src-tauri/src/domain/plugins.rs`, add these data-only types after `PluginRuntimeFailure`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleNotice {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeLifecycleSummary {
    pub kind: String,
    pub label: String,
    pub supported: bool,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookLifecycleSummary {
    pub name: String,
    pub priority: i32,
    pub failure_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionLifecycleSummary {
    pub permission: String,
    pub risk: PluginPermissionRisk,
    pub granted: bool,
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompatibilitySummary {
    pub compatible: bool,
    pub host_version: String,
    pub app_range: String,
    pub plugin_api_range: String,
    pub platforms: Vec<String>,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTrustSummary {
    pub checksum: String,
    pub expected_checksum: Option<String>,
    pub checksum_verified: bool,
    pub signature_verified: bool,
    pub unsigned: bool,
    pub developer_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallPreview {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub source: PluginInstallSource,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub homepage: Option<String>,
    pub repository: Option<serde_json::Value>,
    pub license: Option<String>,
    pub category: Option<String>,
    pub runtime: PluginRuntimeLifecycleSummary,
    pub hooks: Vec<PluginHookLifecycleSummary>,
    pub permissions: Vec<PluginPermissionLifecycleSummary>,
    pub compatibility: PluginCompatibilitySummary,
    pub trust: PluginTrustSummary,
    pub existing_status: Option<PluginStatus>,
    pub existing_version: Option<String>,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
    pub warnings: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleChange {
    pub name: String,
    pub change: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionLifecycleChange {
    pub permission: String,
    pub risk: PluginPermissionRisk,
    pub change: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateDiff {
    pub plugin_id: String,
    pub from_version: String,
    pub to_version: String,
    pub version_direction: String,
    pub runtime_change: Option<PluginLifecycleChange>,
    pub hook_changes: Vec<PluginLifecycleChange>,
    pub permission_changes: Vec<PluginPermissionLifecycleChange>,
    pub config_version_change: Option<String>,
    pub compatibility: PluginCompatibilitySummary,
    pub trust: PluginTrustSummary,
    pub rollback_available: bool,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
    pub warnings: Vec<PluginLifecycleNotice>,
}
```

- [ ] **Step 6: Keep the red tests uncommitted until implementation**

Do not commit after this step. The tests intentionally fail until Task 2 implements package inspection and lifecycle builders. Keep the working tree changes in place and continue directly to Task 2.

Expected working tree paths:

```bash
src-tauri/src/domain/plugins.rs
src-tauri/src/app/plugin_service.rs
```

Expected: no commit yet.

## Task 2: Implement Preview And Update Diff Service

**Files:**

- Modify: `src-tauri/src/infra/plugins/package.rs`
- Modify: `src-tauri/src/app/plugin_service.rs`

- [ ] **Step 1: Import lifecycle DTOs**

Update the existing `use crate::domain::plugins::{ ... }` import at the top of `src-tauri/src/app/plugin_service.rs` to include:

```rust
PluginCompatibilitySummary, PluginHookLifecycleSummary, PluginInstallPreview,
PluginLifecycleChange, PluginLifecycleNotice, PluginPermissionLifecycleChange,
PluginPermissionLifecycleSummary, PluginRuntimeLifecycleSummary, PluginTrustSummary,
PluginUpdateDiff,
```

- [ ] **Step 2: Add package inspection extraction**

In `src-tauri/src/infra/plugins/package.rs`, refactor package extraction so the existing install path still validates the manifest against the host, while preview can inspect incompatible manifests and report compatibility blockers.

Replace the end of `extract_plugin_package`:

```rust
match extract_zip_bytes(bytes, staging_dir, &limits, checksum) {
    Ok(extracted) => Ok(extracted),
    Err(error) => {
        let _ = std::fs::remove_dir_all(staging_dir);
        Err(error)
    }
}
```

with:

```rust
match extract_zip_bytes(bytes, staging_dir, &limits, checksum, true) {
    Ok(extracted) => Ok(extracted),
    Err(error) => {
        let _ = std::fs::remove_dir_all(staging_dir);
        Err(error)
    }
}
```

Change the `extract_zip_bytes` signature to:

```rust
fn extract_zip_bytes(
    bytes: Vec<u8>,
    staging_dir: &Path,
    limits: &PluginPackageLimits,
    checksum: String,
    validate_manifest_for_host: bool,
) -> AppResult<ExtractedPluginPackage> {
```

Then guard the existing host validation at the end of `extract_zip_bytes`:

```rust
if validate_manifest_for_host {
    crate::domain::plugins::validate_manifest(&manifest, env!("CARGO_PKG_VERSION"))?;
}
```

Finally add this public inspection function next to `extract_plugin_package`:

```rust
pub(crate) fn extract_plugin_package_for_inspection(
    package_path: &Path,
    staging_dir: &Path,
    limits: PluginPackageLimits,
) -> AppResult<ExtractedPluginPackage> {
    let metadata = std::fs::metadata(package_path).map_err(|error| {
        AppError::new(
            "PLUGIN_PACKAGE_NOT_FOUND",
            format!("failed to read plugin package metadata: {error}"),
        )
    })?;
    if metadata.len() > limits.max_package_bytes {
        return Err(AppError::new(
            "PLUGIN_PACKAGE_TOO_LARGE",
            format!(
                "plugin package exceeds {} bytes: {}",
                limits.max_package_bytes,
                package_path.display()
            ),
        ));
    }

    let bytes = std::fs::read(package_path).map_err(|error| {
        AppError::new(
            "PLUGIN_PACKAGE_READ_FAILED",
            format!("failed to read plugin package: {error}"),
        )
    })?;
    if bytes.len() as u64 > limits.max_package_bytes {
        return Err(AppError::new(
            "PLUGIN_PACKAGE_TOO_LARGE",
            format!(
                "plugin package exceeds {} bytes: {}",
                limits.max_package_bytes,
                package_path.display()
            ),
        ));
    }

    if staging_dir.exists() {
        std::fs::remove_dir_all(staging_dir).map_err(|error| {
            AppError::new(
                "PLUGIN_PACKAGE_STAGING_FAILED",
                format!(
                    "failed to clean staging dir {}: {error}",
                    staging_dir.display()
                ),
            )
        })?;
    }
    std::fs::create_dir_all(staging_dir).map_err(|error| {
        AppError::new(
            "PLUGIN_PACKAGE_STAGING_FAILED",
            format!(
                "failed to create staging dir {}: {error}",
                staging_dir.display()
            ),
        )
    })?;

    let checksum = format!("sha256:{:x}", Sha256::digest(&bytes));
    match extract_zip_bytes(bytes, staging_dir, &limits, checksum, false) {
        Ok(extracted) => Ok(extracted),
        Err(error) => {
            let _ = std::fs::remove_dir_all(staging_dir);
            Err(error)
        }
    }
}
```

The new inspection path must still reject invalid archives, unsafe paths, missing `plugin.json`, invalid JSON, and manifest shapes that cannot deserialize. It must only skip the final host compatibility/runtime/permission validation so `PluginInstallPreview.compatibility` and runtime blockers can explain those failures.

- [ ] **Step 3: Add package inspection helpers**

Add these helpers near `install_plugin_from_local_package_with_policy`:

```rust
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

fn compare_version_direction(from: &str, to: &str) -> String {
    fn parse(version: &str) -> Option<(u64, u64, u64)> {
        let core = version.split_once('-').map_or(version, |(core, _)| core);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor, patch))
    }

    match (parse(from), parse(to)) {
        (Some(left), Some(right)) if right > left => "upgrade".to_string(),
        (Some(left), Some(right)) if right < left => "downgrade".to_string(),
        (Some(_), Some(_)) => "same".to_string(),
        _ => "unknown".to_string(),
    }
}
```

- [ ] **Step 4: Add summary builders**

Add these helpers in `src-tauri/src/app/plugin_service.rs`:

```rust
fn runtime_lifecycle_summary(manifest: &PluginManifest) -> PluginRuntimeLifecycleSummary {
    match &manifest.runtime {
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
    PluginTrustSummary {
        checksum: extracted.checksum.clone(),
        expected_checksum: policy.expected_checksum.clone(),
        checksum_verified: policy.expected_checksum.is_some(),
        signature_verified: trust.signature_verified,
        unsigned: !trust.signature_verified,
        developer_mode: policy.developer_mode,
    }
}
```

- [ ] **Step 5: Implement install preview**

Add this exported service function near `install_plugin_from_local_package_with_policy`:

```rust
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
    let staging_dir =
        staging_root.join(format!("preview-{}", crate::shared::time::now_unix_seconds()));
    let extracted = package::extract_plugin_package_for_inspection(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    )?;
    let result = build_install_preview(db, &extracted, host_version, PluginInstallSource::Local, &policy);
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
    let mut blocking_reasons = Vec::new();
    let mut warnings = Vec::new();
    let compatibility = compatibility_summary(manifest, host_version);
    blocking_reasons.extend(compatibility.blocking_reasons.clone());
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
                &app_error_code(&error),
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
            &app_error_code(&error),
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
        permissions: permission_lifecycle_summaries(&manifest.permissions, &[], &manifest.permissions),
        compatibility,
        trust: trust_summary(extracted, policy, trust),
        existing_status,
        existing_version,
        blocking_reasons,
        warnings,
    })
}
```

`AppError` exposes `code()` but not a public message accessor, so add these local helpers and use them for lifecycle notices:

```rust
fn app_error_code(error: &AppError) -> String {
    error.to_string().split_once(':').map_or_else(
        || "PLUGIN_LIFECYCLE_ERROR".to_string(),
        |(code, _)| code.to_string(),
    )
}

fn app_error_message(error: &AppError) -> String {
    error.to_string().split_once(':').map_or_else(
        || error.to_string(),
        |(_, message)| message.trim().to_string(),
    )
}
```

- [ ] **Step 6: Implement update diff**

Add these helpers below the install preview helpers:

```rust
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
    let staging_dir =
        staging_root.join(format!("update-preview-{}", crate::shared::time::now_unix_seconds()));
    let extracted = package::extract_plugin_package_for_inspection(
        package_path,
        &staging_dir,
        package::PluginPackageLimits::default(),
    )?;
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
                &app_error_code(&error),
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
            &app_error_code(&error),
            app_error_message(&error),
        ));
    }

    let current_runtime = runtime_lifecycle_summary(&current.manifest);
    let next_runtime = runtime_lifecycle_summary(manifest);
    let runtime_change = (current_runtime.kind != next_runtime.kind
        || current_runtime.label != next_runtime.label)
        .then(|| PluginLifecycleChange {
            name: "runtime".to_string(),
            change: "changed".to_string(),
            before: Some(current_runtime.label),
            after: Some(next_runtime.label),
        });

    let hook_changes = diff_hooks(&current.manifest, manifest);
    let permission_changes = diff_permissions(&current, manifest);
    let config_version_change = config_version_change(&current.manifest, manifest);
    let rollback_available = current
        .summary
        .current_version
        .as_deref()
        .is_some_and(|version| repository::get_plugin_version(db, &manifest.id, version).is_ok());

    if compare_version_direction(
        current.summary.current_version.as_deref().unwrap_or(&current.manifest.version),
        &manifest.version,
    ) == "downgrade"
    {
        warnings.push(lifecycle_notice(
            "warn",
            "PLUGIN_UPDATE_DOWNGRADE",
            "selected package version is lower than the installed version",
        ));
    }

    Ok(PluginUpdateDiff {
        plugin_id: manifest.id.clone(),
        from_version: current
            .summary
            .current_version
            .clone()
            .unwrap_or_else(|| current.manifest.version.clone()),
        to_version: manifest.version.clone(),
        version_direction: compare_version_direction(
            current.summary.current_version.as_deref().unwrap_or(&current.manifest.version),
            &manifest.version,
        ),
        runtime_change,
        hook_changes,
        permission_changes,
        config_version_change,
        compatibility,
        trust: trust_summary(extracted, policy, trust),
        rollback_available,
        blocking_reasons,
        warnings,
    })
}
```

Add these diff helpers:

```rust
fn diff_hooks(before: &PluginManifest, after: &PluginManifest) -> Vec<PluginLifecycleChange> {
    let mut changes = Vec::new();
    for hook in &before.hooks {
        match after.hooks.iter().find(|next| next.name == hook.name) {
            Some(next)
                if next.priority != hook.priority
                    || next.failure_policy != hook.failure_policy =>
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
            PluginPermissionLifecycleChange {
                risk: permission_risk(&permission).unwrap_or(PluginPermissionRisk::Low),
                permission,
                change: change.to_string(),
            }
        })
        .filter(|change| change.change != "not_requested")
        .collect()
}

fn config_version_change(before: &PluginManifest, after: &PluginManifest) -> Option<String> {
    let before_version = before.config_version.unwrap_or(1);
    let after_version = after.config_version.unwrap_or(1);
    (before_version != after_version).then(|| format!("{before_version} -> {after_version}"))
}
```

- [ ] **Step 7: Run focused Rust tests**

Run:

```bash
cd src-tauri && cargo test plugin_local_install_preview_reports_identity_risk_and_trust_without_db_mutation --lib
cd src-tauri && cargo test plugin_local_install_preview_reports_incompatible_manifest_without_installing --lib
cd src-tauri && cargo test plugin_local_update_preview_reports_permission_runtime_hook_and_config_changes --lib
```

Expected: all three pass.

- [ ] **Step 8: Run existing lifecycle regression tests**

Run:

```bash
cd src-tauri && cargo test plugin_update_rollback --lib
cd src-tauri && cargo test plugin_market_revoked_quarantines_installed_plugin --lib
cd src-tauri && cargo test plugin_remote_install --lib
```

Expected: all pass.

- [ ] **Step 9: Commit lifecycle DTOs, tests, and service implementation**

Run:

```bash
git add src-tauri/src/domain/plugins.rs src-tauri/src/infra/plugins/package.rs src-tauri/src/app/plugin_service.rs
git commit -m "feat(plugins): add lifecycle preview and update diff"
```

Expected: commit succeeds.

## Task 3: Expose Preview/Diff Through Tauri Commands And Bindings

**Files:**

- Modify: `src-tauri/src/commands/plugins.rs`
- Modify: `src-tauri/src/commands/registry.rs`
- Modify: `src/generated/bindings.ts`
- Modify: `src/services/plugins.ts`

- [ ] **Step 1: Add failing registry test entries**

In `src-tauri/src/commands/registry.rs`, update `includes_plugin_commands_in_generated_command_registry` to include:

```rust
"plugin_preview_from_file",
"plugin_preview_update_from_file",
```

Run:

```bash
cd src-tauri && cargo test includes_plugin_commands_in_generated_command_registry --lib
```

Expected: FAIL because the commands are not registered.

- [ ] **Step 2: Add command input types**

In `src-tauri/src/commands/plugins.rs`, add after `PluginInstallFromFileInput`:

```rust
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPreviewFromFileInput {
    pub file_path: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPreviewUpdateFromFileInput {
    pub file_path: String,
}
```

Update the imports at the top of the file:

```rust
use crate::domain::plugins::{
    PluginAuditLog, PluginDetail, PluginInstallPreview, PluginInstallSource, PluginUpdateDiff,
};
```

- [ ] **Step 3: Add preview commands**

In `src-tauri/src/commands/plugins.rs`, add these commands before `plugin_install_from_file`:

```rust
#[tauri::command]
#[specta::specta]
pub(crate) async fn plugin_preview_from_file(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: PluginPreviewFromFileInput,
) -> Result<PluginInstallPreview, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("plugin_preview_from_file", move || {
        let path = PathBuf::from(&input.file_path);
        let cache_dir = crate::app_paths::plugins_cache_dir(&app)?;
        plugin_service::preview_plugin_from_local_package_with_policy(
            &db,
            &path,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            plugin_service::LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..plugin_service::LocalPackageInstallPolicy::default()
            },
        )
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn plugin_preview_update_from_file(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: PluginPreviewUpdateFromFileInput,
) -> Result<PluginUpdateDiff, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("plugin_preview_update_from_file", move || {
        let path = PathBuf::from(&input.file_path);
        let cache_dir = crate::app_paths::plugins_cache_dir(&app)?;
        plugin_service::preview_plugin_update_from_local_package(
            &db,
            &path,
            &cache_dir,
            env!("CARGO_PKG_VERSION"),
            plugin_service::LocalPackageInstallPolicy {
                allow_unsigned: true,
                developer_mode: true,
                ..plugin_service::LocalPackageInstallPolicy::default()
            },
        )
    })
    .await
    .map_err(Into::into)
}
```

- [ ] **Step 4: Register commands**

In `src-tauri/src/commands/registry.rs`, add these entries in the plugin command block before install/update:

```rust
plugin_preview_from_file => crate::commands::plugins::plugin_preview_from_file,
plugin_preview_update_from_file => crate::commands::plugins::plugin_preview_update_from_file,
```

- [ ] **Step 5: Run command registry test**

Run:

```bash
cd src-tauri && cargo test includes_plugin_commands_in_generated_command_registry --lib
```

Expected: PASS.

- [ ] **Step 6: Regenerate TypeScript bindings**

Run:

```bash
pnpm tauri:gen-types
pnpm check:generated-bindings
```

Expected: both pass and `src/generated/bindings.ts` contains `pluginPreviewFromFile`, `pluginPreviewUpdateFromFile`, `PluginInstallPreview`, and `PluginUpdateDiff`.

- [ ] **Step 7: Add frontend IPC wrappers**

In `src/services/plugins.ts`, update the generated type import/export list to include:

```ts
type PluginInstallPreview,
type PluginUpdateDiff,
```

Add these exported functions after `pluginGet`:

```ts
export async function pluginPreviewFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginInstallPreview>({
    title: "预检插件失败",
    cmd: "plugin_preview_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginPreviewFromFile({ filePath: normalizedFilePath }),
  });
}

export async function pluginPreviewUpdateFromFile(filePath: string) {
  const normalizedFilePath = normalizePluginFilePath(filePath);

  return invokeGeneratedIpc<PluginUpdateDiff>({
    title: "预检插件更新失败",
    cmd: "plugin_preview_update_from_file",
    args: { filePath: normalizedFilePath },
    invoke: async () => commands.pluginPreviewUpdateFromFile({ filePath: normalizedFilePath }),
  });
}
```

- [ ] **Step 8: Run focused typecheck**

Run:

```bash
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 9: Commit command and bindings work**

Run:

```bash
git add src-tauri/src/commands/plugins.rs src-tauri/src/commands/registry.rs src/generated/bindings.ts src/services/plugins.ts
git commit -m "feat(plugins): expose lifecycle preview commands"
```

Expected: commit succeeds.

## Task 4: Add Frontend Query Hooks And Preview Dialog Components

**Files:**

- Modify: `src/query/keys.ts`
- Modify: `src/query/plugins.ts`
- Create: `src/pages/plugins/PluginInstallPreviewDialog.tsx`
- Create: `src/pages/plugins/PluginUpdatePreviewDialog.tsx`
- Create: `src/pages/plugins/PluginLifecyclePanel.tsx`
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`

- [ ] **Step 1: Add failing frontend tests for preview and diff UI**

In `src/pages/__tests__/PluginsPage.test.tsx`, extend the query mock import to include:

```ts
usePluginPreviewFromFileMutation,
usePluginPreviewUpdateFromFileMutation,
```

Add these to the `vi.mock("../query/plugins", ...)` return object:

```ts
usePluginPreviewFromFileMutation: vi.fn(),
usePluginPreviewUpdateFromFileMutation: vi.fn(),
```

Add default mocks in the test setup:

```ts
vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(mutation() as any);
vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(mutation() as any);
```

Add this test:

```ts
it("previews a local plugin package before installing it", async () => {
  const previewMutation = mutation({
    mutateAsync: vi.fn().mockResolvedValue({
      pluginId: "local.preview-safe",
      name: "Local Package",
      version: "1.0.0",
      source: "local",
      description: "Preview package",
      author: null,
      homepage: null,
      repository: null,
      license: "MIT",
      category: "gateway",
      runtime: {
        kind: "declarativeRules",
        label: "Declarative Rules",
        supported: true,
        blockingReasons: [],
      },
      hooks: [{ name: "gateway.request.afterBodyRead", priority: 10, failurePolicy: "fail-open" }],
      permissions: [{ permission: "request.meta.read", risk: "low", granted: false, pending: true }],
      compatibility: {
        compatible: true,
        hostVersion: "0.62.2",
        appRange: ">=0.56.0 <1.0.0",
        pluginApiRange: "^1.0.0",
        platforms: ["macos", "windows", "linux"],
        blockingReasons: [],
      },
      trust: {
        checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        expectedChecksum: null,
        checksumVerified: false,
        signatureVerified: false,
        unsigned: true,
        developerMode: true,
      },
      existingStatus: null,
      existingVersion: null,
      blockingReasons: [],
      warnings: [],
    }),
  });
  const installMutation = mutation();
  vi.mocked(usePluginPreviewFromFileMutation).mockReturnValue(previewMutation as any);
  vi.mocked(usePluginInstallFromFileMutation).mockReturnValue(installMutation as any);
  vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/local-preview.aio-plugin");

  renderWithProviders(<PluginsPage />);
  fireEvent.click(screen.getByRole("button", { name: "导入 .aio-plugin" }));

  expect(await screen.findByText("安装前预检")).toBeInTheDocument();
  expect(screen.getByText("local.preview-safe")).toBeInTheDocument();
  expect(screen.getByText("Declarative Rules")).toBeInTheDocument();
  expect(screen.getByText("未签名")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "确认安装" }));

  await waitFor(() => {
    expect(previewMutation.mutateAsync).toHaveBeenCalledWith("/tmp/local-preview.aio-plugin");
    expect(installMutation.mutateAsync).toHaveBeenCalledWith("/tmp/local-preview.aio-plugin");
  });
});
```

Add this test:

```ts
it("shows update diff before applying a local plugin update", async () => {
  const updatePreviewMutation = mutation({
    mutateAsync: vi.fn().mockResolvedValue({
      pluginId: "community.redactor",
      fromVersion: "1.0.0",
      toVersion: "1.1.0",
      versionDirection: "upgrade",
      runtimeChange: null,
      hookChanges: [
        {
          name: "gateway.request.beforeSend",
          change: "added",
          before: null,
          after: "priority=20, failurePolicy=fail-open",
        },
      ],
      permissionChanges: [
        { permission: "request.body.read", risk: "high", change: "unchanged_granted" },
        { permission: "request.header.read", risk: "medium", change: "added_pending" },
      ],
      configVersionChange: "1 -> 2",
      compatibility: {
        compatible: true,
        hostVersion: "0.62.2",
        appRange: ">=0.56.0 <1.0.0",
        pluginApiRange: "^1.0.0",
        platforms: ["macos", "windows", "linux"],
        blockingReasons: [],
      },
      trust: {
        checksum: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        expectedChecksum: null,
        checksumVerified: false,
        signatureVerified: false,
        unsigned: true,
        developerMode: true,
      },
      rollbackAvailable: true,
      blockingReasons: [],
      warnings: [],
    }),
  });
  const updateMutation = mutation();
  vi.mocked(usePluginPreviewUpdateFromFileMutation).mockReturnValue(updatePreviewMutation as any);
  vi.mocked(usePluginUpdateFromFileMutation).mockReturnValue(updateMutation as any);
  vi.mocked(openDesktopSinglePath).mockResolvedValue("/tmp/community-redactor-1.1.0.aio-plugin");

  renderWithProviders(<PluginsPage />);
  fireEvent.click(screen.getByRole("button", { name: "更新" }));

  expect(await screen.findByText("更新影响预览")).toBeInTheDocument();
  expect(screen.getByText("1.0.0 -> 1.1.0")).toBeInTheDocument();
  expect(screen.getByText("request.header.read")).toBeInTheDocument();
  expect(screen.getByText("新增待授权")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "确认更新" }));

  await waitFor(() => {
    expect(updatePreviewMutation.mutateAsync).toHaveBeenCalledWith(
      "/tmp/community-redactor-1.1.0.aio-plugin"
    );
    expect(updateMutation.mutateAsync).toHaveBeenCalledWith(
      "/tmp/community-redactor-1.1.0.aio-plugin"
    );
  });
});
```

- [ ] **Step 2: Run frontend tests to verify failure**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
```

Expected: FAIL because preview hooks and dialogs do not exist.

- [ ] **Step 3: Add query keys**

In `src/query/keys.ts`, update `pluginKeys`:

```ts
const pluginsAllKey = ["plugins"] as const;
export const pluginKeys = {
  all: pluginsAllKey,
  list: () => [...pluginsAllKey, "list"] as const,
  detail: (pluginId: string | null) => [...pluginsAllKey, "detail", pluginId] as const,
  auditLogs: (pluginId: string | null, limit: number | null) =>
    [...pluginsAllKey, "auditLogs", pluginId, limit] as const,
  installPreview: (filePath: string | null) =>
    [...pluginsAllKey, "installPreview", filePath] as const,
  updatePreview: (filePath: string | null) =>
    [...pluginsAllKey, "updatePreview", filePath] as const,
};
```

- [ ] **Step 4: Add query mutations**

In `src/query/plugins.ts`, import the new service functions:

```ts
pluginPreviewFromFile,
pluginPreviewUpdateFromFile,
type PluginInstallPreview,
type PluginUpdateDiff,
```

Add these hooks after `usePluginQuery`:

```ts
export function usePluginPreviewFromFileMutation() {
  return useMutation<PluginInstallPreview, Error, string>({
    mutationFn: (filePath) => pluginPreviewFromFile(filePath),
  });
}

export function usePluginPreviewUpdateFromFileMutation() {
  return useMutation<PluginUpdateDiff, Error, string>({
    mutationFn: (filePath) => pluginPreviewUpdateFromFile(filePath),
  });
}
```

- [ ] **Step 5: Create install preview dialog**

Create `src/pages/plugins/PluginInstallPreviewDialog.tsx`:

```tsx
import { AlertTriangle, CheckCircle2, ShieldAlert } from "lucide-react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import type { PluginInstallPreview } from "../../services/plugins";
import { pluginRiskLabel } from "./pluginProductCopy";

type Props = {
  open: boolean;
  preview: PluginInstallPreview | null;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => void;
};

function noticeTone(severity: string) {
  return severity === "error" ? "text-destructive" : "text-warning";
}

export function PluginInstallPreviewDialog({ open, preview, busy, onClose, onConfirm }: Props) {
  const blocked = (preview?.blockingReasons.length ?? 0) > 0;

  return (
    <Dialog open={open} title="安装前预检" description="确认插件来源、权限、兼容性和运行时支持" onOpenChange={(next) => !next && onClose()}>
      {preview ? (
        <div className="space-y-4 text-sm">
          <div className="grid gap-2 rounded-md border border-border p-3">
            <div className="font-semibold text-foreground">{preview.name}</div>
            <div className="font-mono text-xs text-muted-foreground">{preview.pluginId}</div>
            <div className="text-xs text-muted-foreground">版本 {preview.version}</div>
          </div>

          <div className="grid gap-2 sm:grid-cols-2">
            <div className="rounded-md border border-border p-3">
              <div className="text-xs text-muted-foreground">Runtime</div>
              <div className="mt-1 flex items-center gap-2 font-medium text-foreground">
                {preview.runtime.supported ? <CheckCircle2 className="h-4 w-4 text-success" /> : <ShieldAlert className="h-4 w-4 text-destructive" />}
                {preview.runtime.label}
              </div>
            </div>
            <div className="rounded-md border border-border p-3">
              <div className="text-xs text-muted-foreground">Trust</div>
              <div className="mt-1 font-medium text-foreground">
                {preview.trust.signatureVerified ? "签名已验证" : "未签名"}
              </div>
            </div>
          </div>

          <div className="rounded-md border border-border p-3">
            <div className="mb-2 text-xs text-muted-foreground">Permissions</div>
            <div className="grid gap-1">
              {preview.permissions.map((permission) => (
                <div key={permission.permission} className="flex items-center justify-between gap-3">
                  <span className="font-mono text-xs text-foreground">{permission.permission}</span>
                  <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                    {pluginRiskLabel(permission.risk)}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {[...preview.blockingReasons, ...preview.warnings].map((notice) => (
            <div key={`${notice.code}-${notice.message}`} className={`flex gap-2 rounded-md border border-border p-3 ${noticeTone(notice.severity)}`}>
              <AlertTriangle className="mt-0.5 h-4 w-4" />
              <div>
                <div className="font-medium">{notice.code}</div>
                <div className="text-xs">{notice.message}</div>
              </div>
            </div>
          ))}

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onClose} disabled={busy}>取消</Button>
            <Button variant="primary" onClick={onConfirm} disabled={busy || blocked}>
              {busy ? "安装中..." : "确认安装"}
            </Button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}
```

- [ ] **Step 6: Create update preview dialog**

Create `src/pages/plugins/PluginUpdatePreviewDialog.tsx`:

```tsx
import { AlertTriangle } from "lucide-react";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import type { PluginUpdateDiff } from "../../services/plugins";
import { pluginRiskLabel } from "./pluginProductCopy";

type Props = {
  open: boolean;
  diff: PluginUpdateDiff | null;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => void;
};

function permissionChangeLabel(change: string) {
  switch (change) {
    case "added_pending":
      return "新增待授权";
    case "unchanged_granted":
      return "已授权保留";
    case "unchanged_pending":
      return "仍待授权";
    case "removed":
      return "已移除";
    default:
      return "请求不变";
  }
}

export function PluginUpdatePreviewDialog({ open, diff, busy, onClose, onConfirm }: Props) {
  const blocked = (diff?.blockingReasons.length ?? 0) > 0;

  return (
    <Dialog open={open} title="更新影响预览" description="确认版本、权限、Hook 和配置版本变化" onOpenChange={(next) => !next && onClose()}>
      {diff ? (
        <div className="space-y-4 text-sm">
          <div className="rounded-md border border-border p-3">
            <div className="font-mono text-xs text-muted-foreground">{diff.pluginId}</div>
            <div className="mt-1 font-semibold text-foreground">
              {diff.fromVersion} -&gt; {diff.toVersion}
            </div>
            {diff.configVersionChange ? (
              <div className="mt-1 text-xs text-muted-foreground">
                Config {diff.configVersionChange}
              </div>
            ) : null}
          </div>

          <div className="rounded-md border border-border p-3">
            <div className="mb-2 text-xs text-muted-foreground">Permission delta</div>
            <div className="grid gap-1">
              {diff.permissionChanges.map((permission) => (
                <div key={permission.permission} className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-mono text-xs text-foreground">{permission.permission}</span>
                  <span className="text-xs text-muted-foreground">{pluginRiskLabel(permission.risk)}</span>
                  <span className="rounded-md border border-border px-2 py-0.5 text-xs">
                    {permissionChangeLabel(permission.change)}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {diff.hookChanges.length > 0 ? (
            <div className="rounded-md border border-border p-3">
              <div className="mb-2 text-xs text-muted-foreground">Hook changes</div>
              {diff.hookChanges.map((hook) => (
                <div key={`${hook.name}-${hook.change}`} className="font-mono text-xs text-foreground">
                  {hook.name}: {hook.change}
                </div>
              ))}
            </div>
          ) : null}

          {[...diff.blockingReasons, ...diff.warnings].map((notice) => (
            <div key={`${notice.code}-${notice.message}`} className="flex gap-2 rounded-md border border-border p-3 text-warning">
              <AlertTriangle className="mt-0.5 h-4 w-4" />
              <div>
                <div className="font-medium">{notice.code}</div>
                <div className="text-xs">{notice.message}</div>
              </div>
            </div>
          ))}

          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={onClose} disabled={busy}>取消</Button>
            <Button variant="primary" onClick={onConfirm} disabled={busy || blocked}>
              {busy ? "更新中..." : "确认更新"}
            </Button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}
```

- [ ] **Step 7: Create lifecycle panel**

Create `src/pages/plugins/PluginLifecyclePanel.tsx`:

```tsx
import { RotateCcw, ShieldAlert } from "lucide-react";
import type { PluginDetail } from "../../services/plugins";
import { Button } from "../../ui/Button";
import { pluginStatusLabel } from "./pluginProductCopy";

type Props = {
  detail: PluginDetail;
  rollbackVersion: string | null;
  busy: boolean;
  onRollback: (version: string) => void;
};

function auditDetailString(detail: PluginDetail, key: string) {
  for (const log of detail.audit_logs) {
    const details = log.details;
    if (details && typeof details === "object" && !Array.isArray(details)) {
      const value = (details as Record<string, unknown>)[key];
      if (typeof value === "string" && value.trim()) return value;
    }
  }
  return null;
}

export function PluginLifecyclePanel({ detail, rollbackVersion, busy, onRollback }: Props) {
  const unsigned = detail.audit_logs.some((log) => {
    const details = log.details;
    return Boolean(details && typeof details === "object" && !Array.isArray(details) && (details as Record<string, unknown>).unsigned === true);
  });
  const quarantineReason = detail.summary.status === "quarantined" ? detail.summary.last_error : null;
  const checksum = auditDetailString(detail, "packageChecksum");

  return (
    <section className="space-y-2">
      <h2 className="text-sm font-semibold text-foreground">生命周期</h2>
      <div className="grid gap-2 rounded-md border border-border p-3 text-sm">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-muted-foreground">状态</span>
          <span className="font-medium text-foreground">{pluginStatusLabel(detail.summary.status)}</span>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-muted-foreground">来源</span>
          <span className="font-medium text-foreground">{detail.install_source}</span>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-muted-foreground">信任</span>
          <span className="font-medium text-foreground">{unsigned ? "未签名" : "签名或官方来源"}</span>
        </div>
        {checksum ? (
          <div className="break-all font-mono text-xs text-muted-foreground">{checksum}</div>
        ) : null}
        {quarantineReason ? (
          <div className="flex gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-2 text-destructive">
            <ShieldAlert className="mt-0.5 h-4 w-4" />
            <span>{quarantineReason}</span>
          </div>
        ) : null}
        {rollbackVersion ? (
          <Button size="sm" variant="secondary" onClick={() => onRollback(rollbackVersion)} disabled={busy}>
            <RotateCcw className="h-4 w-4" />
            回滚 {rollbackVersion}
          </Button>
        ) : null}
      </div>
    </section>
  );
}
```

- [ ] **Step 8: Run component tests**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
```

Expected: still fails because `PluginsPage.tsx` is not wired to these components yet.

- [ ] **Step 9: Keep frontend red tests uncommitted until page integration**

Do not commit after this step. The tests intentionally fail until Task 5 wires `PluginsPage.tsx` to the new preview hooks and dialogs. Keep these working tree changes in place:

```bash
src/query/keys.ts
src/query/plugins.ts
src/pages/plugins/PluginInstallPreviewDialog.tsx
src/pages/plugins/PluginUpdatePreviewDialog.tsx
src/pages/plugins/PluginLifecyclePanel.tsx
src/pages/__tests__/PluginsPage.test.tsx
```

Expected: no commit yet.

## Task 5: Wire PluginsPage Lifecycle Flow

**Files:**

- Modify: `src/pages/PluginsPage.tsx`
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`

- [ ] **Step 1: Import new hooks and components**

In `src/pages/PluginsPage.tsx`, extend query imports:

```ts
usePluginPreviewFromFileMutation,
usePluginPreviewUpdateFromFileMutation,
```

Add component imports:

```ts
import { PluginInstallPreviewDialog } from "./plugins/PluginInstallPreviewDialog";
import { PluginLifecyclePanel } from "./plugins/PluginLifecyclePanel";
import { PluginUpdatePreviewDialog } from "./plugins/PluginUpdatePreviewDialog";
```

Extend type imports:

```ts
PluginInstallPreview,
PluginUpdateDiff,
```

- [ ] **Step 2: Add preview state**

Inside `PluginsPage`, add state near existing mutation setup:

```ts
const previewMutation = usePluginPreviewFromFileMutation();
const updatePreviewMutation = usePluginPreviewUpdateFromFileMutation();
const [pendingInstallPath, setPendingInstallPath] = useState<string | null>(null);
const [installPreview, setInstallPreview] = useState<PluginInstallPreview | null>(null);
const [pendingUpdatePath, setPendingUpdatePath] = useState<string | null>(null);
const [updatePreview, setUpdatePreview] = useState<PluginUpdateDiff | null>(null);
```

Include preview mutations in `busy`:

```ts
previewMutation.isPending ||
updatePreviewMutation.isPending ||
```

- [ ] **Step 3: Change import action to preview first**

Replace the current direct install behavior in `handleImportPlugin`:

```ts
async function handleImportPlugin() {
  const filePath = await openDesktopSinglePath({
    title: "选择 .aio-plugin",
    filters: [{ name: "AIO Plugin", extensions: ["aio-plugin"] }],
  });
  if (!filePath) return;
  await runAction("预检插件", async () => {
    const preview = await previewMutation.mutateAsync(filePath);
    setPendingInstallPath(filePath);
    setInstallPreview(preview);
  });
}
```

Add confirmation handler:

```ts
async function confirmInstallPreview() {
  if (!pendingInstallPath) return;
  await runAction("导入插件", async () => {
    await installMutation.mutateAsync(pendingInstallPath);
    setPendingInstallPath(null);
    setInstallPreview(null);
  });
}
```

- [ ] **Step 4: Change update action to preview first**

Replace direct update behavior:

```ts
async function handleUpdatePlugin() {
  const filePath = await openDesktopSinglePath({
    title: "选择更新包",
    filters: [{ name: "AIO Plugin", extensions: ["aio-plugin"] }],
  });
  if (!filePath) return;
  await runAction("预检插件更新", async () => {
    const diff = await updatePreviewMutation.mutateAsync(filePath);
    setPendingUpdatePath(filePath);
    setUpdatePreview(diff);
  });
}
```

Add confirmation handler:

```ts
async function confirmUpdatePreview() {
  if (!pendingUpdatePath) return;
  await runAction("更新插件", async () => {
    await updateMutation.mutateAsync(pendingUpdatePath);
    setPendingUpdatePath(null);
    setUpdatePreview(null);
  });
}
```

- [ ] **Step 5: Render lifecycle panel**

In the plugin detail area, replace duplicated lifecycle-ish rows and inline rollback block with:

```tsx
<PluginLifecyclePanel
  detail={detail}
  rollbackVersion={rollbackVersion}
  busy={busy}
  onRollback={(version) =>
    runAction("回滚插件", () =>
      rollbackMutation.mutateAsync({ pluginId: selectedPluginId, version })
    )
  }
/>
```

Keep the existing detail rows for identity/manifest facts. The lifecycle panel owns status/source/trust/quarantine/rollback explanation.

- [ ] **Step 6: Render preview dialogs**

Near the bottom of `PluginsPage` JSX, render:

```tsx
<PluginInstallPreviewDialog
  open={installPreview != null}
  preview={installPreview}
  busy={busy}
  onClose={() => {
    setInstallPreview(null);
    setPendingInstallPath(null);
  }}
  onConfirm={() => void confirmInstallPreview()}
/>

<PluginUpdatePreviewDialog
  open={updatePreview != null}
  diff={updatePreview}
  busy={busy}
  onClose={() => {
    setUpdatePreview(null);
    setPendingUpdatePath(null);
  }}
  onConfirm={() => void confirmUpdatePreview()}
/>
```

- [ ] **Step 7: Run frontend tests**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Run frontend typecheck**

Run:

```bash
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 9: Commit frontend preview flow**

Run:

```bash
git add src/query/keys.ts src/query/plugins.ts src/pages/plugins/PluginInstallPreviewDialog.tsx src/pages/plugins/PluginUpdatePreviewDialog.tsx src/pages/plugins/PluginLifecyclePanel.tsx src/pages/PluginsPage.tsx src/pages/__tests__/PluginsPage.test.tsx
git commit -m "feat(plugins): wire lifecycle preview flow"
```

Expected: commit succeeds.

## Task 6: Docs, Regression Gates, And Final Verification

**Files:**

- Modify: `docs/plugins/developer-guide.md`
- Modify: `docs/plugins/reference/publishing.md`
- Modify: `docs/plugins/reference/compatibility.md`

- [ ] **Step 1: Update developer guide install workflow**

In `docs/plugins/developer-guide.md`, update the local install paragraph under "10 分钟快速开始" to state:

```markdown
在 Plugins 页面选择本地包 `acme.redactor.aio-plugin` 后，0.62.2 会先展示安装前预检：插件 id、版本、runtime、hooks、permissions、兼容性、checksum 和签名状态。确认后才会写入插件库。更新插件时，页面会先展示版本变化、Hook 变化、配置版本变化和权限 delta；新增权限会进入待授权列表，不会静默继承授权。
```

- [ ] **Step 2: Update publishing reference**

In `docs/plugins/reference/publishing.md`, add a section after the current checklist:

```markdown
## 0.62.2 生命周期预检

0.62.2 在安装或更新 `.aio-plugin` 前会生成 host-side preview。Preview 只用于解释风险和变化，不是安全边界；真正安装或更新时宿主仍会重新执行解压、manifest 校验、兼容性校验、checksum、签名和 runtime policy 检查。

更新插件时，宿主会比较当前版本和待安装版本：

- 新增 permissions 进入 pending，不会自动授权。
- 已授权且仍被请求的 permissions 保持 granted。
- 已不再请求的 permissions 从新 manifest 中移除。
- runtime、hooks、configVersion 和 compatibility 变化会展示在更新预览中。
- rollback 只允许回到数据库中已经记录的历史版本。
```

- [ ] **Step 3: Update compatibility reference**

In `docs/plugins/reference/compatibility.md`, add:

```markdown
## 0.62.2 Lifecycle Boundary

0.62.2 不改变 Plugin API v1。新增的是宿主侧 lifecycle preview 和 update diff：用户可以在安装或更新前看到兼容性、runtime support、permissions、trust summary 和 blocking reasons。

Quarantined 或 incompatible 插件不能启用。WASM 仍然 policy-gated。Provider Plugin API 仍然不开放。
```

- [ ] **Step 4: Run docs check**

Run:

```bash
pnpm check:plugin-system-docs
```

Expected: PASS.

- [ ] **Step 5: Run Rust plugin tests**

Run:

```bash
cd src-tauri && cargo test plugin --lib
```

Expected: PASS.

- [ ] **Step 6: Run frontend plugin tests**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Run generated binding and type checks**

Run:

```bash
pnpm check:generated-bindings
pnpm typecheck
```

Expected: PASS.

- [ ] **Step 8: Run release gates**

Run:

```bash
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
pnpm check:prepush
```

Expected: PASS.

- [ ] **Step 9: Commit docs and final verification state**

Run:

```bash
git add docs/plugins/developer-guide.md docs/plugins/reference/publishing.md docs/plugins/reference/compatibility.md
git commit -m "docs(plugins): document lifecycle preview flow"
```

Expected: commit succeeds.

## Final Review Checklist

Before marking 0.62.2 implementation complete, verify:

- [ ] Plugin API v1 manifest shape is unchanged.
- [ ] No Provider Plugin API is exposed.
- [ ] Preview and update diff commands do not mutate the database.
- [ ] Install/update commands still re-run validation and trust policy.
- [ ] New permissions after update are pending, not granted.
- [ ] Quarantined plugins cannot be enabled.
- [ ] GUI import/update flows require preview confirmation before mutation.
- [ ] Generated bindings are in sync.
- [ ] Docs say 0.62.2 is lifecycle stabilization, not a full marketplace.
- [ ] `pnpm check:prepush` passes.
