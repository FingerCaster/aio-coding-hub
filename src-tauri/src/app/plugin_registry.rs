//! Usage: Shared Tauri builder setup (managed state + plugin wiring).

use super::{
    app_state::DbInitState, gateway_state::GatewayState, maintenance::MaintenanceState, resident,
    startup_state::StartupState,
};
use tauri::plugin::Plugin;
use tauri::Manager;

struct MaintenanceGatedPlugin<P> {
    inner: P,
    initialized: bool,
}

impl<P> MaintenanceGatedPlugin<P> {
    fn new(inner: P) -> Self {
        Self {
            inner,
            initialized: false,
        }
    }
}

impl<P> Plugin<tauri::Wry> for MaintenanceGatedPlugin<P>
where
    P: Plugin<tauri::Wry>,
{
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn initialize(
        &mut self,
        app: &tauri::AppHandle<tauri::Wry>,
        config: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if crate::app::maintenance::ensure_normal_operation(app).is_err() {
            return Ok(());
        }
        self.inner.initialize(app, config)?;
        self.initialized = true;
        Ok(())
    }

    fn initialization_script(&self) -> Option<String> {
        if self.initialized {
            self.inner.initialization_script()
        } else {
            None
        }
    }

    fn window_created(&mut self, window: tauri::Window<tauri::Wry>) {
        if self.initialized {
            self.inner.window_created(window);
        }
    }

    fn webview_created(&mut self, webview: tauri::Webview<tauri::Wry>) {
        if self.initialized {
            self.inner.webview_created(webview);
        }
    }

    fn on_navigation(&mut self, webview: &tauri::Webview<tauri::Wry>, url: &tauri::Url) -> bool {
        !self.initialized || self.inner.on_navigation(webview, url)
    }

    fn on_page_load(
        &mut self,
        webview: &tauri::Webview<tauri::Wry>,
        payload: &tauri::webview::PageLoadPayload<'_>,
    ) {
        if self.initialized {
            self.inner.on_page_load(webview, payload);
        }
    }

    fn on_event(&mut self, app: &tauri::AppHandle<tauri::Wry>, event: &tauri::RunEvent) {
        if self.initialized {
            self.inner.on_event(app, event);
        }
    }

    fn extend_api(&mut self, invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
        if self.initialized
            && crate::app::maintenance::invoke_allowed_during_maintenance(
                invoke.message.webview_ref().app_handle(),
                invoke.message.command(),
            )
        {
            return self.inner.extend_api(invoke);
        }

        invoke
            .resolver
            .reject("APP_MAINTENANCE_REQUIRED: 应用正在维护中，插件命令不可用");
        true
    }
}

fn maintenance_gate_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("maintenance-gate")
        .setup(|app, _api| {
            let _ = crate::app::maintenance::run_before_startup(app);
            Ok(())
        })
        .build()
}

pub(crate) fn create_builder() -> tauri::Builder<tauri::Wry> {
    let builder = tauri::Builder::default()
        .manage(DbInitState::default())
        .manage(MaintenanceState::default())
        .manage(GatewayState::default())
        .manage(resident::ResidentState::default())
        .manage(StartupState::default())
        .manage(crate::app::provider_share_service::ProviderShareService::default())
        .manage(
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState::default(),
        )
        .manage(crate::app::heartbeat_watchdog::HeartbeatWatchdogState::default())
        .manage(crate::app::plugins::extension_host_registry::ExtensionHostRuntimeState::default())
        // Plugin initialization runs before Tauri setup. Keep the maintenance
        // preflight first, then suppress every normal application plugin while
        // a corrupt or incomplete reset marker blocks startup.
        .plugin(maintenance_gate_plugin())
        .plugin(MaintenanceGatedPlugin::new(tauri_plugin_opener::init()))
        .plugin(MaintenanceGatedPlugin::new(tauri_plugin_dialog::init()))
        .plugin(MaintenanceGatedPlugin::new(
            tauri_plugin_clipboard_manager::init(),
        ))
        .plugin(MaintenanceGatedPlugin::new(tauri_plugin_fs::init()));

    #[cfg(desktop)]
    let builder = builder
        .plugin(MaintenanceGatedPlugin::new(
            tauri_plugin_autostart::Builder::new().build(),
        ))
        .plugin(MaintenanceGatedPlugin::new(
            tauri_plugin_notification::init(),
        ))
        // Keep process ownership in maintenance mode so a second launch
        // cannot race the first process while it retries the same marker.
        // This plugin has no renderer IPC surface or persistent app-data work.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            resident::show_main_window(app);
        }))
        .plugin(MaintenanceGatedPlugin::new(
            tauri_plugin_window_state::Builder::default().build(),
        ));

    builder
}

#[cfg(test)]
mod tests {
    use super::MaintenanceGatedPlugin;

    struct ScriptPlugin;

    impl tauri::plugin::Plugin<tauri::Wry> for ScriptPlugin {
        fn name(&self) -> &'static str {
            "script-test"
        }

        fn initialization_script(&self) -> Option<String> {
            Some("window.__SCRIPT_TEST__ = true;".to_string())
        }
    }

    #[test]
    fn desktop_builder_keeps_single_instance_registration() {
        let source = std::fs::read_to_string(file!()).expect("read plugin registry source");
        let needle = ["tauri_plugin_", "single_", "instance::", "init"].concat();

        assert!(
            source.contains("#[cfg(desktop)]") && source.contains(&needle),
            "startup request-log reconciliation relies on desktop single-instance ownership"
        );
    }

    #[test]
    fn maintenance_preflight_precedes_normal_plugin_registration() {
        let source = std::fs::read_to_string(file!()).expect("read plugin registry source");
        let gate = source
            .find(".plugin(maintenance_gate_plugin())")
            .expect("maintenance gate registration");
        let opener = source
            .find("tauri_plugin_opener::init()")
            .expect("normal plugin registration");

        assert!(gate < opener, "maintenance gate must initialize first");
        assert!(
            source.contains("MaintenanceGatedPlugin::new"),
            "normal plugin IPC must stay behind the maintenance gate"
        );
        let single_instance = source
            .find(".plugin(tauri_plugin_single_instance::init")
            .expect("single-instance ownership registration");
        assert!(
            gate < single_instance,
            "maintenance preflight must run before process ownership setup"
        );
    }

    #[test]
    fn skipped_plugins_do_not_inject_renderer_initialization_scripts() {
        use tauri::plugin::Plugin as _;

        let mut plugin = MaintenanceGatedPlugin::new(ScriptPlugin);
        assert_eq!(plugin.initialization_script(), None);

        plugin.initialized = true;
        assert_eq!(
            plugin.initialization_script().as_deref(),
            Some("window.__SCRIPT_TEST__ = true;")
        );
    }
}
