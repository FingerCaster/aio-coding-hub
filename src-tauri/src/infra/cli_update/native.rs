//! Read-only native CLI update previews and explicit, single-use installation plans.
use super::{fetch_latest_version, CliUpdateResult};
use crate::domain::native_cli::NativeClient;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

mod install;
#[cfg(test)]
mod tests;

const PI_PACKAGE: &str = "@earendil-works/pi-coding-agent";
const PI_LEGACY_PACKAGE: &str = "@mariozechner/pi-coding-agent";
const OMP_PACKAGE: &str = "@oh-my-pi/pi-coding-agent";
const PLAN_LIFETIME: Duration = Duration::from_secs(30 * 60);
static PLANS: OnceLock<Mutex<HashMap<String, Plan>>> = OnceLock::new();
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeCliVersionCheck {
    pub client: NativeClient,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub latest_version: String,
    pub update_available: bool,
    pub install_method: String,
    pub install_directory: Option<String>,
    pub executable_path: Option<String>,
    pub plan_id: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallMethod {
    Npm {
        node: PathBuf,
        script: PathBuf,
        prefix: PathBuf,
        package: String,
    },
    Bun {
        executable: PathBuf,
        root: PathBuf,
        package: String,
    },
    Standalone {
        target: PathBuf,
    },
}

impl InstallMethod {
    fn label(&self) -> &'static str {
        match self {
            Self::Npm { .. } => "npm",
            Self::Bun { .. } => "Bun",
            Self::Standalone { .. } => "官方独立程序",
        }
    }
    fn directory(&self) -> &Path {
        match self {
            Self::Npm { prefix, .. } => prefix,
            Self::Bun { root, .. } => root,
            Self::Standalone { target } => target.parent().expect("absolute installation path"),
        }
    }
    fn package(&self) -> &str {
        match self {
            Self::Npm { package, .. } | Self::Bun { package, .. } => package,
            Self::Standalone { .. } => OMP_PACKAGE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Installation {
    executable: Option<PathBuf>,
    version: Option<String>,
    method: Option<InstallMethod>,
    blocked_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct Plan {
    client: NativeClient,
    installation: Installation,
    version: String,
    checksum: Option<String>,
    created: Instant,
}

fn stable_version(value: &str) -> Result<semver::Version, String> {
    let version = semver::Version::parse(value).map_err(|_| "官方版本号无效".to_string())?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("仅支持官方稳定版本".into());
    }
    Ok(version)
}

fn newer_version(installed: Option<&str>, latest: &str) -> bool {
    let Some(installed) = installed.and_then(|v| semver::Version::parse(v).ok()) else {
        return false;
    };
    semver::Version::parse(latest).is_ok_and(|version| version > installed)
}

fn block_legacy_migration(installation: &mut Installation) -> bool {
    if installation
        .method
        .as_ref()
        .is_some_and(|method| method.package() == PI_LEGACY_PACKAGE)
    {
        installation.blocked_reason = Some("检测到已停用的旧 Pi 包 @mariozechner/pi-coding-agent；请先手动迁移到 @earendil-works/pi-coding-agent，再使用一键升级".into());
        true
    } else {
        false
    }
}

fn package_version(path: &Path, expected: &str) -> Option<String> {
    let bytes = crate::shared::fs::read_optional_file_with_max_len(path, 128 * 1024).ok()??;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("name")?.as_str()? != expected {
        return None;
    }
    let version = value.get("version")?.as_str()?;
    semver::Version::parse(version).ok()?;
    Some(version.to_owned())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn npm_runtime(npm: Option<&Path>, node: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
    let npm = npm?;
    let parent = npm.parent()?;
    let sibling_node = parent.join(if cfg!(windows) { "node.exe" } else { "node" });
    let node = if sibling_node.is_file() {
        sibling_node
    } else {
        node?.to_owned()
    };
    let sibling_script = parent.join("node_modules/npm/bin/npm-cli.js");
    let script = if sibling_script.is_file() {
        sibling_script
    } else {
        let canonical = std::fs::canonicalize(npm).ok()?;
        if canonical.file_name()?.to_str()? != "npm-cli.js" {
            return None;
        }
        canonical
    };
    Some((node, script))
}

fn default_prefix(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("npm")
    }
    #[cfg(not(windows))]
    {
        home.join(".local")
    }
}

fn default_omp_target(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("omp/omp.exe")
    }
    #[cfg(not(windows))]
    {
        home.join(".local/bin/omp")
    }
}

fn identify_installation(
    client: NativeClient,
    executable: Option<PathBuf>,
    version: Option<String>,
    home: &Path,
    npm: Option<&Path>,
    node: Option<&Path>,
    bun: Option<&Path>,
) -> Installation {
    let packages: &[&str] = match client {
        NativeClient::Pi => &[PI_PACKAGE, PI_LEGACY_PACKAGE],
        NativeClient::Omp => &[OMP_PACKAGE],
    };
    let mut installation = Installation {
        executable: executable.clone(),
        version,
        method: None,
        blocked_reason: None,
    };
    if let Some(exe) = &executable {
        // Bun's global packages are separate from npm prefixes (including Windows PE shims).
        if let Some(bun) = bun {
            if let Some(root) = bun.parent().and_then(Path::parent) {
                let bin = root.join("bin");
                if exe.parent().is_some_and(|parent| same_path(parent, &bin)) {
                    for package in packages {
                        if let Some(version) = package_version(
                            &root
                                .join("install/global/node_modules")
                                .join(package)
                                .join("package.json"),
                            package,
                        ) {
                            installation.version = Some(version);
                            installation.method = Some(InstallMethod::Bun {
                                executable: bun.to_owned(),
                                root: root.to_owned(),
                                package: (*package).into(),
                            });
                            return installation;
                        }
                    }
                }
            }
        }
        if client == NativeClient::Pi {
            if let Some(parent) = exe.parent() {
                let prefix = if cfg!(windows) {
                    parent
                } else {
                    parent.parent().unwrap_or(parent)
                };
                let modules = if cfg!(windows) {
                    prefix.join("node_modules")
                } else {
                    prefix.join("lib/node_modules")
                };
                for package in packages {
                    if let Some(version) =
                        package_version(&modules.join(package).join("package.json"), package)
                    {
                        installation.version = Some(version);
                        if let Some((node, script)) = npm_runtime(npm, node) {
                            installation.method = Some(InstallMethod::Npm {
                                node,
                                script,
                                prefix: prefix.into(),
                                package: (*package).into(),
                            });
                        } else {
                            installation.blocked_reason =
                                Some("未找到可用的 Node.js / npm，请安装后重新检查".into());
                        }
                        return installation;
                    }
                }
            }
        }
        if client == NativeClient::Omp && installation.version.is_some() {
            // Requiring the isolated binary probe prevents overwriting source/Bun script shims.
            if crate::cli_manager::native_version::omp_standalone_version(exe).ok()
                == installation.version
            {
                if let Ok(target) = std::fs::canonicalize(exe) {
                    installation.method = Some(InstallMethod::Standalone { target });
                    return installation;
                }
            }
        }
        installation.blocked_reason =
            Some("无法确认当前安装方式，请使用下方官方命令手动更新后重新检查".into());
    } else if client == NativeClient::Omp {
        installation.method = Some(InstallMethod::Standalone {
            target: default_omp_target(home),
        });
    } else if let Some((node, script)) = npm_runtime(npm, node) {
        installation.method = Some(InstallMethod::Npm {
            node,
            script,
            prefix: default_prefix(home),
            package: PI_PACKAGE.into(),
        });
    } else {
        installation.blocked_reason =
            Some("安装 Pi 需要 Node.js 和 npm，请先安装官方要求的 Node.js 版本".into());
    }
    installation
}

fn resolve_installation(
    app: &tauri::AppHandle,
    client: NativeClient,
) -> Result<Installation, String> {
    let info =
        crate::cli_manager::native_cli_info_get(app, client.as_str()).map_err(|e| e.to_string())?;
    if let Some(error) = info.error {
        return Err(error);
    }
    let home = crate::app_paths::home_dir(app).map_err(|e| e.to_string())?;
    let scan = |key| crate::cli_manager::scan_executable(app, key).map_err(|e| e.to_string());
    let npm = scan("npm")?;
    let node = scan("node")?;
    let bun = scan("bun")?;
    Ok(identify_installation(
        client,
        info.executable_path.map(PathBuf::from),
        info.version,
        &home,
        npm.as_deref(),
        node.as_deref(),
        bun.as_deref(),
    ))
}

async fn probe(app: &tauri::AppHandle, client: NativeClient) -> Result<Installation, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || resolve_installation(&app, client))
        .await
        .map_err(|e| e.to_string())?
}

fn save_plan(plan: Plan) -> Result<String, String> {
    let mut plans = PLANS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    plans.retain(|_, plan| plan.created.elapsed() < PLAN_LIFETIME);
    if plans.len() >= 64 {
        if let Some(oldest) = plans
            .iter()
            .min_by_key(|(_, plan)| plan.created)
            .map(|(id, _)| id.clone())
        {
            plans.remove(&oldest);
        }
    }
    let id = format!("{:032x}", rand::random::<u128>());
    plans.insert(id.clone(), plan);
    Ok(id)
}

fn take_plan(id: &str, client: NativeClient) -> Result<Plan, String> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("安装计划无效，请重新检查版本".into());
    }
    let mut plans = PLANS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    let plan = plans
        .get(id)
        .filter(|plan| plan.client == client && plan.created.elapsed() < PLAN_LIFETIME)
        .ok_or("安装计划已失效，请重新检查版本并确认")?
        .clone();
    plans.remove(id);
    Ok(plan)
}

/// Only metadata is fetched here. No package manager, download or installer is started.
pub async fn check(
    app: &tauri::AppHandle,
    client: NativeClient,
) -> Result<NativeCliVersionCheck, String> {
    let mut installation = probe(app, client).await?;
    let legacy = block_legacy_migration(&mut installation);
    // The retired Pi package has a frozen dist-tag; compare against its maintained successor.
    let package = match client {
        NativeClient::Pi => PI_PACKAGE,
        NativeClient::Omp => OMP_PACKAGE,
    };
    let latest = fetch_latest_version(package).await?;
    stable_version(&latest)?;
    let update_available = newer_version(installation.version.as_deref(), &latest);
    let actionable = !legacy && (installation.executable.is_none() || update_available);
    let checksum =
        if actionable && matches!(installation.method, Some(InstallMethod::Standalone { .. })) {
            Some(install::fetch_checksum(&latest).await?)
        } else {
            None
        };
    let plan_id = if actionable && installation.method.is_some() {
        Some(save_plan(Plan {
            client,
            installation: installation.clone(),
            version: latest.clone(),
            checksum,
            created: Instant::now(),
        })?)
    } else {
        None
    };
    Ok(NativeCliVersionCheck {
        client,
        installed: installation.executable.is_some(),
        installed_version: installation.version.clone(),
        latest_version: latest,
        update_available,
        install_method: installation
            .method
            .as_ref()
            .map(InstallMethod::label)
            .unwrap_or("手动管理")
            .into(),
        install_directory: installation
            .method
            .as_ref()
            .map(|m| m.directory().to_string_lossy().into_owned()),
        executable_path: installation
            .executable
            .map(|p| p.to_string_lossy().into_owned()),
        plan_id,
        blocked_reason: installation.blocked_reason,
    })
}

pub async fn update(
    app: &tauri::AppHandle,
    client: NativeClient,
    plan_id: String,
) -> Result<CliUpdateResult, String> {
    let _guard = INSTALL_LOCK
        .try_lock()
        .map_err(|_| "另一个 CLI 安装或升级正在进行，请等待完成")?;
    let plan = take_plan(&plan_id, client)?;
    if probe(app, client).await? != plan.installation {
        return Err("安装状态已变化，请重新检查版本并确认".into());
    }
    let output = install::execute(&plan).await?;
    // Probe again, including PATH precedence: never claim a different installation was updated.
    let actual = probe(app, client).await?;
    let success = actual.version.as_deref() == Some(plan.version.as_str());
    Ok(CliUpdateResult {
        cli_key: client.as_str().into(),
        success,
        output,
        error: (!success)
            .then(|| "安装已执行，但当前命令未解析到目标版本。请检查 PATH 优先级并刷新".into()),
    })
}
