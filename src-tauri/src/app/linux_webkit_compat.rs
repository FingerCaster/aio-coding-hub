//! Linux-only: WebKitGTK compatibility shims for Wayland / EGL environments.
//!
//! ## Problem
//! When running as an AppImage, WebKitGTK bundles its own EGL/Mesa libraries.
//! On systems with a newer Mesa stack (e.g., Arch Linux + Niri / Wayland),
//! the bundled EGL libraries conflict with the system stack, causing the
//! process to abort with:
//!
//!   "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
//!
//! ## Fix
//! Setting `WEBKIT_DISABLE_COMPOSITING_MODE=1` before WebKitGTK initialises
//! disables GPU layer compositing and avoids the EGL display-creation path
//! entirely, allowing the renderer to fall back to software compositing.
//!
//! This function is a no-op on non-Linux platforms and on X11-only sessions.
//! Users can opt out by setting `WEBKIT_DISABLE_COMPOSITING_MODE` to any value
//! before launching the app.
//!
//! Ref: <https://github.com/FingerCaster/aio-coding-hub/issues/93>

/// Apply WebKitGTK compatibility patches for Linux/Wayland.
///
/// **Must be called before `tauri::Builder::default()`** – WebKitGTK reads
/// these env vars during display initialisation, which happens early in
/// Tauri's setup phase.
#[cfg(target_os = "linux")]
pub fn apply() {
    // Honour any value the user has already set; never override an explicit choice.
    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_some() {
        return;
    }

    if !is_wayland_from_env(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    ) {
        return;
    }

    // std::env::set_var is safe in Rust edition 2021; this is also called
    // before Tauri spawns worker threads so no concurrent access can occur.
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

    // Log to stderr because the structured logger has not been initialised yet.
    // This message helps users (and developers) diagnose display-related crashes.
    eprintln!(
        "[aio] Wayland session detected: set WEBKIT_DISABLE_COMPOSITING_MODE=1 \
         to avoid EGL display initialisation failure (issue #93). \
         To override, set the variable to any value before launching the app."
    );
}

/// No-op on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
#[inline]
pub fn apply() {}

/// Returns `true` when the given env var values indicate a Wayland session.
///
/// Heuristic: a Wayland compositor sets `WAYLAND_DISPLAY`; login managers
/// additionally set `XDG_SESSION_TYPE=wayland`. Either signal is sufficient.
///
/// Accepts the env values as parameters so callers (and tests) can supply
/// arbitrary values without mutating the process environment.
#[cfg(target_os = "linux")]
fn is_wayland_from_env(
    wayland_display: Option<&std::ffi::OsStr>,
    xdg_session_type: Option<&str>,
) -> bool {
    if wayland_display.is_some() {
        return true;
    }
    xdg_session_type
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::is_wayland_from_env;
    use std::ffi::OsStr;

    #[test]
    fn detects_wayland_display_var() {
        assert!(is_wayland_from_env(Some(OsStr::new(":0")), None));
    }

    #[test]
    fn detects_xdg_session_type_wayland() {
        assert!(is_wayland_from_env(None, Some("wayland")));
    }

    #[test]
    fn xdg_session_type_case_insensitive() {
        assert!(is_wayland_from_env(None, Some("Wayland")));
        assert!(is_wayland_from_env(None, Some("WAYLAND")));
    }

    #[test]
    fn not_wayland_on_x11() {
        assert!(!is_wayland_from_env(None, Some("x11")));
    }

    #[test]
    fn not_wayland_when_no_env_vars() {
        assert!(!is_wayland_from_env(None, None));
    }

    #[test]
    fn wayland_display_takes_precedence_over_xdg() {
        // WAYLAND_DISPLAY present → Wayland, regardless of XDG_SESSION_TYPE
        assert!(is_wayland_from_env(Some(OsStr::new(":0")), Some("x11")));
    }
}
