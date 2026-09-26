//! Standalone OMP version probing without loading the user's agent environment.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use crate::shared::error::AppResult;

fn is_native_binary(executable: &Path) -> bool {
    let mut magic = [0_u8; 4];
    if std::fs::File::open(executable)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_err()
    {
        return false;
    }
    matches!(
        magic,
        [b'M', b'Z', _, _]
            | [0x7f, b'E', b'L', b'F']
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
    )
}

fn parse_omp_version(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?.trim();
    let version = text.strip_prefix("omp/")?;
    if version.len() > 64
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    {
        return None;
    }
    let core = version.split(['-', '+']).next()?;
    let components: Vec<_> = core.split('.').collect();
    if components.len() != 3
        || components.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.parse::<u32>().is_err()
        })
    {
        return None;
    }
    Some(version.to_owned())
}

fn isolated_version_command(executable: &Path, root: &Path) -> AppResult<Command> {
    let mut command = Command::new(executable);
    command.env_clear().current_dir(root).arg("--version");
    // Preserve only OS loader variables, never profile, auth, proxy, PATH or preloads.
    for name in ["SystemRoot", "WINDIR", "SystemDrive"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, directory) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("APPDATA", "appdata"),
        ("LOCALAPPDATA", "localappdata"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
        ("PI_CODING_AGENT_DIR", "agent"),
        ("BUN_INSTALL_CACHE_DIR", "bun-cache"),
        ("BUN_RUNTIME_TRANSPILER_CACHE_PATH", "bun-transpiler"),
    ] {
        let directory = root.join(directory);
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to isolate OMP version probe: {error}"))?;
        command.env(name, directory);
    }
    command.env("NO_COLOR", "1").env("TERM", "dumb");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    Ok(command)
}

pub(crate) fn omp_standalone_version(executable: &Path) -> AppResult<String> {
    // Do not execute text-based npm/Bun shims or shell wrappers as a fallback.
    if !is_native_binary(executable) {
        return Err("OMP_VERSION_UNAVAILABLE: not a standalone binary".into());
    }
    let executable = std::fs::canonicalize(executable)
        .map_err(|error| format!("failed to resolve OMP executable: {error}"))?;
    let directory = tempfile::Builder::new()
        .prefix("aio-omp-version-")
        .tempdir()
        .map_err(|error| format!("failed to create OMP version probe directory: {error}"))?;
    let command = isolated_version_command(&executable, directory.path())?;
    let output = super::command_output_with_timeout_limit(
        command,
        super::VERSION_TIMEOUT,
        "isolated OMP --version".into(),
        4096,
    )?;
    if !output.status.success() || output.stdout.truncated || output.stderr.truncated {
        return Err("OMP_VERSION_UNAVAILABLE: version probe did not complete cleanly".into());
    }
    parse_omp_version(&output.stdout.bytes)
        .ok_or_else(|| "OMP_VERSION_UNAVAILABLE: unexpected version output".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omp_version_accepts_only_its_own_single_line_output() {
        assert_eq!(
            parse_omp_version(b"omp/18.3.2\r\n").as_deref(),
            Some("18.3.2")
        );
        assert_eq!(
            parse_omp_version(b"omp/18.4.0-beta.1+build.2").as_deref(),
            Some("18.4.0-beta.1+build.2")
        );
        for value in [
            "1.4.2",
            "bun/1.4.2",
            "omp/",
            "omp/18.3",
            "omp/18.x.2",
            "omp/18.3.2\nextra",
        ] {
            assert!(parse_omp_version(value.as_bytes()).is_none(), "{value}");
        }
        assert!(parse_omp_version(&[0xff]).is_none());
    }

    #[test]
    fn omp_version_refuses_to_execute_script_shims() {
        let directory = tempfile::tempdir().unwrap();
        let shim = directory.path().join("omp.cmd");
        std::fs::write(&shim, "@echo off\necho omp/99.0.0").unwrap();
        assert!(omp_standalone_version(&shim).is_err());
        assert!(omp_standalone_version(&directory.path().join("missing.exe")).is_err());
    }

    #[test]
    fn omp_version_isolates_config_and_runtime_directories() {
        let directory = tempfile::tempdir().unwrap();
        let command = isolated_version_command(Path::new("omp.exe"), directory.path()).unwrap();
        assert_eq!(command.get_current_dir(), Some(directory.path()));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--version"]);
        let environment: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        for name in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "PI_CODING_AGENT_DIR",
            "TEMP",
        ] {
            let value = environment[std::ffi::OsStr::new(name)].unwrap();
            assert!(Path::new(value).starts_with(directory.path()));
        }
        for name in [
            "NODE_OPTIONS",
            "BUN_OPTIONS",
            "OMP_PROFILE",
            "PI_PROFILE",
            "OPENAI_API_KEY",
            "PATH",
        ] {
            assert!(!environment.contains_key(std::ffi::OsStr::new(name)));
        }
    }

    #[test]
    #[ignore = "requires an explicit local standalone OMP executable"]
    fn omp_version_reads_installed_standalone_binary() {
        let path =
            std::env::var_os("AIO_TEST_OMP_EXECUTABLE").expect("set AIO_TEST_OMP_EXECUTABLE");
        let version = omp_standalone_version(Path::new(&path)).expect("standalone OMP version");
        println!("isolated standalone OMP version: {version}");
    }
}
