// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn check_webview2_registry(hkey: windows_sys::Win32::System::Registry::HKEY, subkey: &str) -> bool {
    use windows_sys::Win32::System::Registry::*;

    let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_name: Vec<u16> = "pv".encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey_result: HKEY = std::ptr::null_mut();

    let status =
        unsafe { RegOpenKeyExW(hkey, subkey_wide.as_ptr(), 0, KEY_READ, &mut hkey_result) };

    if status != 0 {
        return false;
    }

    let mut data_type: u32 = 0;
    let mut data_size: u32 = 0;

    let status = unsafe {
        RegQueryValueExW(
            hkey_result,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut data_type,
            std::ptr::null_mut(),
            &mut data_size,
        )
    };

    unsafe { RegCloseKey(hkey_result) };

    // If the "pv" value exists and is not empty, WebView2 is installed.
    // > 2 bytes means more than just a UTF-16 null terminator.
    status == 0 && data_size > 2
}

#[cfg(windows)]
fn ensure_webview2_or_exit() {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WV2_CLIENT_KEY: &str =
        "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    const WV2_WOW64_KEY: &str =
        "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

    let found = check_webview2_registry(HKEY_LOCAL_MACHINE, WV2_WOW64_KEY)
        || check_webview2_registry(HKEY_LOCAL_MACHINE, WV2_CLIENT_KEY)
        || check_webview2_registry(HKEY_CURRENT_USER, WV2_CLIENT_KEY);

    if found {
        return;
    }

    // Show a native Win32 MessageBox (does not require WebView2).
    let title: Vec<u16> = "AIO Coding Hub\0".encode_utf16().collect();
    let message: Vec<u16> =
        "This application requires Microsoft WebView2 Runtime.\n\nClick OK to open the download page, or Cancel to exit.\0"
            .encode_utf16()
            .collect();

    let result = unsafe {
        MessageBoxW(
            0 as HWND,
            message.as_ptr(),
            title.as_ptr(),
            MB_OKCANCEL | MB_ICONWARNING,
        )
    };

    if result == IDOK {
        let _ = std::process::Command::new("cmd")
            .args([
                "/c",
                "start",
                "https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section",
            ])
            .spawn();
    }

    std::process::exit(1);
}

fn main() {
    if std::env::args().any(|arg| arg == "--extension-host-worker") {
        aio_coding_hub_lib::run_extension_host_worker();
        return;
    }

    #[cfg(windows)]
    ensure_webview2_or_exit();

    aio_coding_hub_lib::run()
}
