use crate::{AutoLaunch, Result};
use std::io::{self, ErrorKind};
use winreg::enums::RegType::REG_BINARY;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::{RegKey, RegValue};

static AL_REGKEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
static TASK_MANAGER_OVERRIDE_REGKEY: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
static TASK_MANAGER_OVERRIDE_ENABLED_VALUE: [u8; 12] = [
    0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Windows implement
impl AutoLaunch {
    /// Create a new AutoLaunch instance
    /// - `app_name`: application name
    /// - `app_path`: application path
    /// - `args`: startup args passed to the binary
    ///
    /// ## Notes
    ///
    /// The parameters of `AutoLaunch::new` are different on each platform.
    pub fn new(app_name: &str, app_path: &str, args: &[impl AsRef<str>]) -> AutoLaunch {
        AutoLaunch {
            app_name: app_name.into(),
            app_path: app_path.into(),
            args: args.iter().map(|s| s.as_ref().to_string()).collect(),
        }
    }

    /// Enable the AutoLaunch setting
    ///
    /// ## Errors
    ///
    /// - failed to open the registry key
    /// - failed to set value
    pub fn enable(&self) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let command = format_run_command(&self.app_path, &self.args);
        hkcu.open_subkey_with_flags(AL_REGKEY, KEY_SET_VALUE)?
            .set_value::<_, _>(&self.app_name, &command)?;

        // this key maybe not found
        if let Ok(reg) = hkcu.open_subkey_with_flags(TASK_MANAGER_OVERRIDE_REGKEY, KEY_SET_VALUE) {
            reg.set_raw_value(
                &self.app_name,
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: TASK_MANAGER_OVERRIDE_ENABLED_VALUE.to_vec(),
                },
            )?;
        }

        Ok(())
    }

    /// Disable the AutoLaunch setting
    ///
    /// ## Errors
    ///
    /// - failed to open the registry key
    /// - failed to delete value
    ///
    /// A missing key or value already represents the disabled state and succeeds.
    pub fn disable(&self) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        delete_run_entry_with(
            || hkcu.open_subkey_with_flags(AL_REGKEY, KEY_SET_VALUE),
            |reg| reg.delete_value(&self.app_name),
        )?;
        Ok(())
    }

    /// Check whether the AutoLaunch setting is enabled
    pub fn is_enabled(&self) -> Result<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        let al_enabled = hkcu
            .open_subkey_with_flags(AL_REGKEY, KEY_READ)?
            .get_value::<String, _>(&self.app_name)
            .is_ok();
        let task_manager_enabled = self.task_manager_enabled(hkcu);

        Ok(al_enabled && task_manager_enabled.unwrap_or(true))
    }

    fn task_manager_enabled(&self, hkcu: RegKey) -> Option<bool> {
        let task_manager_override_raw_value = hkcu
            .open_subkey_with_flags(TASK_MANAGER_OVERRIDE_REGKEY, KEY_READ)
            .ok()?
            .get_raw_value(&self.app_name)
            .ok()?;
        Some(last_eight_bytes_all_zeros(
            &task_manager_override_raw_value.bytes,
        )?)
    }
}

fn delete_run_entry_with<T>(
    open_key: impl FnOnce() -> io::Result<T>,
    delete_value: impl FnOnce(T) -> io::Result<()>,
) -> io::Result<()> {
    match open_key().and_then(delete_value) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn last_eight_bytes_all_zeros(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 8 {
        return None;
    }
    Some(bytes.iter().rev().take(8).all(|v| *v == 0u8))
}

fn format_run_command(app_path: &str, args: &[String]) -> String {
    let app_path = if app_path.chars().any(|c| c.is_whitespace())
        && !(app_path.starts_with('\"') && app_path.ends_with('\"'))
    {
        format!("\"{app_path}\"")
    } else {
        app_path.to_string()
    };

    if args.is_empty() {
        app_path
    } else {
        format!("{app_path} {}", args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_run_entry_preserves_success() {
        delete_run_entry_with(|| Ok(()), |()| Ok(())).expect("successful deletion");
    }

    #[test]
    fn delete_run_entry_ignores_missing_key_and_value() {
        delete_run_entry_with(
            || Err::<(), _>(io::Error::from(ErrorKind::NotFound)),
            |()| panic!("delete must not run when the key is missing"),
        )
        .expect("missing key is already disabled");

        delete_run_entry_with(|| Ok(()), |()| Err(io::Error::from(ErrorKind::NotFound)))
            .expect("missing value is already disabled");
    }

    #[test]
    fn delete_run_entry_propagates_other_errors() {
        let open_error = delete_run_entry_with(
            || Err::<(), _>(io::Error::from_raw_os_error(5)),
            |()| panic!("delete must not run when opening the key fails"),
        )
        .expect_err("access-denied open must fail");
        assert_eq!(open_error.raw_os_error(), Some(5));

        let delete_error = delete_run_entry_with(
            || Ok(()),
            |()| Err(io::Error::new(ErrorKind::PermissionDenied, "delete denied")),
        )
        .expect_err("access-denied delete must fail");
        assert_eq!(delete_error.kind(), ErrorKind::PermissionDenied);
        assert_eq!(delete_error.to_string(), "delete denied");
    }
}
