//! Pinned official package installs and verified standalone OMP replacement.
use super::{InstallMethod, Plan};
use crate::cli_update::{prepend_command_path, run_update_command};
use crate::shared::http_body::read_text_with_limit;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const RELEASE_BASE: &str = "https://github.com/can1357/oh-my-pi/releases/download";
const MAX_BINARY_BYTES: u64 = 512 * 1024 * 1024;

fn asset_name() -> Result<String, String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err("当前 CPU 架构不支持 OMP 官方独立程序".into()),
    };
    let platform = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" if cfg!(target_env = "musl") => "linux-musl",
        "linux" => "linux",
        _ => return Err("当前系统不支持 OMP 官方独立程序".into()),
    };
    Ok(format!(
        "omp-{platform}-{arch}{}",
        if cfg!(windows) { ".exe" } else { "" }
    ))
}

fn release_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(timeout)
        .user_agent("AIO-Coding-Hub-native-cli-updater")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let allowed = matches!(
                attempt.url().host_str(),
                Some(
                    "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            );
            if allowed && attempt.previous().len() < 5 && attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.error("unexpected official release redirect")
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

fn checksum_from_manifest(manifest: &str, asset: &str) -> Result<String, String> {
    let mut matches = manifest.lines().filter_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset && parts.next().is_none()).then_some(digest)
    });
    let digest = matches.next().ok_or("官方校验清单不含当前平台程序")?;
    if matches.next().is_some()
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("官方 SHA256 校验清单无效".into());
    }
    Ok(digest.to_ascii_lowercase())
}

pub(super) async fn fetch_checksum(version: &str) -> Result<String, String> {
    super::stable_version(version)?;
    let response = release_client(Duration::from_secs(20))?
        .get(format!("{RELEASE_BASE}/v{version}/SHA256SUMS.txt"))
        .send()
        .await
        .map_err(|e| format!("读取 OMP 发布校验信息失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("OMP 对应官方发布尚不可用：{e}"))?;
    let manifest = read_text_with_limit(response, 64 * 1024, "OMP checksums")
        .await
        .map_err(|e| e.to_string())?;
    checksum_from_manifest(&manifest, &asset_name()?)
}

fn package_command(
    method: &InstallMethod,
    version: &str,
    working_dir: &Path,
) -> Result<Command, String> {
    super::stable_version(version)?;
    let mut command = match method {
        InstallMethod::Npm {
            node,
            script,
            prefix,
            package,
        } => {
            let mut command = Command::new(node);
            command
                .arg(script)
                .args([
                    "install",
                    "--global",
                    "--engine-strict",
                    "--no-audit",
                    "--no-fund",
                    "--registry=https://registry.npmjs.org",
                    "--prefix",
                ])
                .arg(prefix)
                .arg(format!("{package}@{version}"));
            if let Some(dir) = node.parent() {
                prepend_command_path(&mut command, dir);
            }
            command
        }
        InstallMethod::Bun {
            executable,
            root,
            package,
        } => {
            let config_path = working_dir.join("bunfig.toml");
            let config = toml::to_string(&serde_json::json!({ "install": {
                "globalDir": root.join("install/global").to_string_lossy(),
                "globalBinDir": root.join("bin").to_string_lossy()
            }}))
            .map_err(|e| e.to_string())?;
            std::fs::write(&config_path, config).map_err(|e| e.to_string())?;
            let mut command = Command::new(executable);
            command
                .args([
                    "install",
                    "--global",
                    "--registry=https://registry.npmjs.org",
                    "--config",
                ])
                .arg(config_path)
                .arg(format!("{package}@{version}"));
            command.env("BUN_INSTALL", root);
            if let Some(dir) = executable.parent() {
                prepend_command_path(&mut command, dir);
            }
            command
        }
        _ => return Err("不是包管理器安装计划".into()),
    };
    command.current_dir(working_dir);
    // Do not load project config or injected runtime code while running an installer.
    for key in [
        "NODE_OPTIONS",
        "BUN_OPTIONS",
        "NPM_CONFIG_NODE_OPTIONS",
        "npm_config_node_options",
    ] {
        command.env_remove(key);
    }
    Ok(command)
}

async fn standalone_install(plan: &Plan, target: &Path) -> Result<String, String> {
    let checksum = plan.checksum.as_deref().ok_or("安装计划缺少 SHA256")?;
    let asset = asset_name()?;
    let parent = target.parent().ok_or("安装目录无效")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("无法创建安装目录：{e}"))?;
    let temporary = tempfile::Builder::new()
        .prefix(".aio-omp-download-")
        .suffix(if cfg!(windows) { ".exe" } else { "" })
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?
        .into_temp_path();
    let mut response = release_client(Duration::from_secs(600))?
        .get(format!("{RELEASE_BASE}/v{}/{asset}", plan.version))
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载失败：{e}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BINARY_BYTES)
    {
        return Err("程序下载超过大小限制".into());
    }
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("下载中断：{e}"))?
    {
        count += chunk.len() as u64;
        if count > MAX_BINARY_BYTES {
            return Err("程序下载超过大小限制".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    if format!("{:x}", hash.finalize()) != checksum {
        return Err("SHA256 校验失败，保留原安装，未替换程序".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    let staged = temporary.to_path_buf();
    let expected = plan.version.clone();
    tokio::task::spawn_blocking(move || {
        let actual = crate::cli_manager::native_version::omp_standalone_version(&staged)
            .map_err(|e| e.to_string())?;
        if actual != expected {
            return Err("下载程序版本与已确认版本不一致".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    // Revalidate after the potentially long download; do not overwrite a later external install.
    verify_target_unchanged(target, plan.installation.version.as_deref())?;
    activate_binary(&temporary, target)?;
    Ok(format!(
        "已校验 SHA256 并安装 OMP {} 到 {}",
        plan.version,
        target.display()
    ))
}

fn verify_target_unchanged(target: &Path, expected: Option<&str>) -> Result<(), String> {
    match expected {
        Some(version)
            if crate::cli_manager::native_version::omp_standalone_version(target)
                .ok()
                .as_deref()
                == Some(version) =>
        {
            Ok(())
        }
        None if !target.try_exists().map_err(|e| e.to_string())? => Ok(()),
        _ => Err("安装目录中的程序已变化，请重新检查版本".into()),
    }
}

fn activate_binary(staged: &Path, target: &Path) -> Result<(), String> {
    crate::shared::fs::write_file_atomic_with(target, |output| {
        let mut input = std::fs::File::open(staged).map_err(|e| e.to_string())?;
        std::io::copy(&mut input, output).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .map_err(|e| format!("无法替换程序，请关闭正在运行的 OMP 后重试；原安装保持不变：{e}"))
}

async fn ensure_path(method: &InstallMethod) -> String {
    let bin = match method {
        InstallMethod::Npm { prefix, .. } => {
            if cfg!(windows) {
                prefix.clone()
            } else {
                prefix.join("bin")
            }
        }
        InstallMethod::Bun { root, .. } => root.join("bin"),
        InstallMethod::Standalone { target } => {
            target.parent().expect("installation parent").into()
        }
    };
    #[cfg(windows)]
    {
        // Fixed code, with the backend-derived path passed as data (never interpolated).
        let shell = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(shell);
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; $p=[string][Environment]::GetEnvironmentVariable('Path','User'); $d=$env:AIO_NATIVE_INSTALL_BIN; if (($p -split ';') -notcontains $d) { [Environment]::SetEnvironmentVariable('Path', (($p.TrimEnd(';') + ';' + $d).TrimStart(';')), 'User') }"])
            .env("AIO_NATIVE_INSTALL_BIN", &bin);
        let result = run_update_command(command, "path".into(), Duration::from_secs(15)).await;
        if !result.success {
            return format!(
                "程序已安装；请手动将 {} 加入用户 PATH，并重新打开终端",
                bin.display()
            );
        }
        "程序已安装；请重新打开终端以使用最新 PATH".into()
    }
    #[cfg(not(windows))]
    {
        format!(
            "请确认 {} 在 PATH 中；重新打开 CLI 会话使用新版本",
            bin.display()
        )
    }
}

pub(super) async fn execute(plan: &Plan) -> Result<String, String> {
    let method = plan
        .installation
        .method
        .as_ref()
        .ok_or("无法自动处理此安装方式")?;
    let output = match method {
        InstallMethod::Standalone { target } => standalone_install(plan, target).await?,
        _ => {
            let working_dir = tempfile::Builder::new()
                .prefix("aio-cli-install-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let command = package_command(method, &plan.version, working_dir.path())?;
            let result = run_update_command(
                command,
                plan.client.as_str().into(),
                Duration::from_secs(600),
            )
            .await;
            if !result.success {
                return Err(format!(
                    "{}\n{}",
                    result.error.unwrap_or_else(|| "安装失败".into()),
                    result.output
                ));
            }
            result.output
        }
    };
    let path_note = ensure_path(method).await;
    Ok(format!("{output}\n{path_note}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "downloads official OMP into a temporary directory; explicit network test only"]
    async fn live_omp_install_in_isolated_directory() {
        assert_eq!(std::env::var("AIO_TEST_NATIVE_INSTALL").as_deref(), Ok("1"));
        let directory = tempfile::tempdir().unwrap();
        let target = directory
            .path()
            .join(if cfg!(windows) { "omp.exe" } else { "omp" });
        let version = crate::cli_update::fetch_latest_version(super::super::OMP_PACKAGE)
            .await
            .unwrap();
        let checksum = fetch_checksum(&version).await.unwrap();
        let plan = Plan {
            client: crate::domain::native_cli::NativeClient::Omp,
            installation: super::super::Installation {
                executable: None,
                version: None,
                method: Some(InstallMethod::Standalone {
                    target: target.clone(),
                }),
                blocked_reason: None,
            },
            version: version.clone(),
            checksum: Some(checksum),
            created: std::time::Instant::now(),
        };
        std::fs::write(directory.path().join("settings.json"), b"preserved").unwrap();
        standalone_install(&plan, &target).await.unwrap();
        assert_eq!(
            crate::cli_manager::native_version::omp_standalone_version(&target).unwrap(),
            version
        );
        assert_eq!(
            std::fs::read(directory.path().join("settings.json")).unwrap(),
            b"preserved"
        );
        println!(
            "OMP isolated download, SHA256, activation and version verification passed: {version}"
        );
    }

    #[tokio::test]
    #[ignore = "installs official Pi into temporary npm prefix/cache; explicit network test only"]
    async fn live_pi_install_in_isolated_directory() {
        assert_eq!(std::env::var("AIO_TEST_NATIVE_INSTALL").as_deref(), Ok("1"));
        let directory = tempfile::tempdir().unwrap();
        let node =
            std::path::PathBuf::from(std::env::var_os("AIO_TEST_NODE").expect("AIO_TEST_NODE"));
        let npm = std::path::PathBuf::from(std::env::var_os("AIO_TEST_NPM").expect("AIO_TEST_NPM"));
        let (node, script) =
            super::super::npm_runtime(Some(&npm), Some(&node)).expect("npm runtime");
        let version = crate::cli_update::fetch_latest_version(super::super::PI_PACKAGE)
            .await
            .unwrap();
        let prefix = directory.path().join("prefix");
        let method = InstallMethod::Npm {
            node,
            script,
            prefix: prefix.clone(),
            package: super::super::PI_PACKAGE.into(),
        };
        let mut command = package_command(&method, &version, directory.path()).unwrap();
        command.env("NPM_CONFIG_CACHE", directory.path().join("cache"));
        command.env(
            "NPM_CONFIG_USERCONFIG",
            directory.path().join("empty-npmrc"),
        );
        let result = run_update_command(command, "pi".into(), Duration::from_secs(600)).await;
        assert!(result.success, "{:?}: {}", result.error, result.output);
        let modules = if cfg!(windows) {
            prefix.join("node_modules")
        } else {
            prefix.join("lib/node_modules")
        };
        let installed = super::super::package_version(
            &modules.join(super::super::PI_PACKAGE).join("package.json"),
            super::super::PI_PACKAGE,
        );
        assert_eq!(installed.as_deref(), Some(version.as_str()));
        println!("Pi isolated npm install and version verification passed: {version}");
    }
    #[test]
    fn checksum_requires_exact_unique_asset_and_valid_digest() {
        let digest = "a".repeat(64);
        assert_eq!(
            checksum_from_manifest(&format!("{digest}  omp.exe\n"), "omp.exe").unwrap(),
            digest
        );
        for bad in [
            "bad  omp.exe".to_string(),
            format!("{digest}  other.exe"),
            format!("{digest}  omp.exe\n{digest}  omp.exe"),
        ] {
            assert!(checksum_from_manifest(&bad, "omp.exe").is_err());
        }
    }
    #[test]
    fn package_commands_pin_version_and_directory_without_shell() {
        let method = InstallMethod::Npm {
            node: "C:/node/node.exe".into(),
            script: "C:/node/npm-cli.js".into(),
            prefix: "C:/user & data/npm".into(),
            package: super::super::PI_PACKAGE.into(),
        };
        let command = package_command(&method, "1.2.3", Path::new(".")).unwrap();
        let std = command.as_std();
        assert_eq!(std.get_program(), "C:/node/node.exe");
        let args: Vec<_> = std
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"C:/user & data/npm".into()));
        assert!(args.contains(&"@earendil-works/pi-coding-agent@1.2.3".into()));
        assert!(package_command(&method, "1.2.3 & echo bad", Path::new(".")).is_err());
    }
    #[test]
    fn bun_uses_explicit_config_for_the_existing_global_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root with spaces");
        let method = InstallMethod::Bun {
            executable: root.join("bin/bun.exe"),
            root: root.clone(),
            package: super::super::OMP_PACKAGE.into(),
        };
        let command = package_command(&method, "18.3.2", directory.path()).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--config".to_string()));
        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(directory.path().join("bunfig.toml")).unwrap())
                .unwrap();
        assert_eq!(
            config["install"]["globalDir"].as_str(),
            Some(root.join("install/global").to_string_lossy().as_ref())
        );
        assert_eq!(
            config["install"]["globalBinDir"].as_str(),
            Some(root.join("bin").to_string_lossy().as_ref())
        );
    }
    #[test]
    fn activation_failure_preserves_previous_binary() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("omp");
        std::fs::write(&target, b"old").unwrap();
        assert!(activate_binary(&dir.path().join("missing"), &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(verify_target_unchanged(&target, None).is_err());
    }
    #[test]
    fn activation_replaces_binary_and_keeps_adjacent_config() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("new");
        let target = dir.path().join("omp");
        std::fs::write(&staged, b"new binary").unwrap();
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(dir.path().join("settings.json"), b"preserved").unwrap();
        activate_binary(&staged, &target).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"new binary");
        assert_eq!(
            std::fs::read(dir.path().join("settings.json")).unwrap(),
            b"preserved"
        );
    }
}
