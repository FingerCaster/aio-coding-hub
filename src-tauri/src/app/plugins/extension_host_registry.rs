//! Usage: Managed Extension Host process instance reuse and disposal.

use super::extension_host::ExtensionHostInstance;
use crate::app::app_state::{ensure_db_ready, DbInitState};
use crate::db;
use crate::domain::plugins::{PluginDetail, PluginManifest, PluginRuntime};
use crate::shared::error::{AppError, AppResult};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const DEFAULT_MAX_WARM_INSTANCES: usize = 8;
const DEFAULT_IDLE_RECYCLE: Duration = Duration::from_secs(120);

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ExtensionHostInstanceKey {
    pub(crate) plugin_id: String,
    pub(crate) version: String,
    pub(crate) installed_dir: String,
    pub(crate) main: String,
    pub(crate) runtime_kind: String,
    pub(crate) runtime_language: String,
    pub(crate) contribution_hash: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExtensionHostRegistryLimits {
    pub(crate) max_warm_instances: usize,
    pub(crate) idle_recycle: Duration,
}

impl Default for ExtensionHostRegistryLimits {
    fn default() -> Self {
        Self {
            max_warm_instances: DEFAULT_MAX_WARM_INSTANCES,
            idle_recycle: DEFAULT_IDLE_RECYCLE,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ExtensionHostCommandOutput {
    pub(crate) value: Value,
    pub(crate) cold_start: bool,
}

trait ExtensionHostProcess: Send {
    fn execute_command<'a>(
        &'a mut self,
        command: &'a str,
        args: Value,
    ) -> BoxFuture<'a, AppResult<Value>>;
    fn is_running(&mut self) -> bool;
    fn dispose<'a>(&'a mut self) -> BoxFuture<'a, ()>;
}

trait ExtensionHostFactory: Send + Sync {
    fn start<'a>(
        &'a self,
        detail: PluginDetail,
        db: db::Db,
    ) -> BoxFuture<'a, AppResult<Box<dyn ExtensionHostProcess>>>;
}

#[allow(dead_code)]
struct RealExtensionHostFactory;

impl ExtensionHostFactory for RealExtensionHostFactory {
    fn start<'a>(
        &'a self,
        detail: PluginDetail,
        db: db::Db,
    ) -> BoxFuture<'a, AppResult<Box<dyn ExtensionHostProcess>>> {
        Box::pin(async move {
            let plugin_root = plugin_root(&detail)?;
            let host = ExtensionHostInstance::start_with_host_api(
                detail.manifest.clone(),
                plugin_root,
                db,
            )
            .await?;
            Ok(Box::new(RealExtensionHostProcess { host }) as Box<dyn ExtensionHostProcess>)
        })
    }
}

#[allow(dead_code)]
struct RealExtensionHostProcess {
    host: ExtensionHostInstance,
}

impl ExtensionHostProcess for RealExtensionHostProcess {
    fn execute_command<'a>(
        &'a mut self,
        command: &'a str,
        args: Value,
    ) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move { self.host.execute_command(command, args).await })
    }

    fn is_running(&mut self) -> bool {
        self.host.is_running()
    }

    fn dispose<'a>(&'a mut self) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.host.dispose().await;
        })
    }
}

struct ManagedExtensionHostInstance {
    process: Box<dyn ExtensionHostProcess>,
    last_used: Instant,
}

impl ManagedExtensionHostInstance {
    async fn dispose(mut self) {
        self.process.dispose().await;
    }
}

pub(crate) struct ExtensionHostInstanceRegistry {
    db: db::Db,
    instances: Mutex<BTreeMap<ExtensionHostInstanceKey, ManagedExtensionHostInstance>>,
    limits: ExtensionHostRegistryLimits,
    factory: Arc<dyn ExtensionHostFactory>,
}

impl ExtensionHostInstanceRegistry {
    #[allow(dead_code)]
    pub(crate) fn new(db: db::Db) -> Self {
        Self::with_factory(
            db,
            Arc::new(RealExtensionHostFactory),
            ExtensionHostRegistryLimits::default(),
        )
    }

    fn with_factory(
        db: db::Db,
        factory: Arc<dyn ExtensionHostFactory>,
        limits: ExtensionHostRegistryLimits,
    ) -> Self {
        Self {
            db,
            instances: Mutex::new(BTreeMap::new()),
            limits,
            factory,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn execute_command(
        &self,
        detail: PluginDetail,
        command: &str,
        args: Value,
    ) -> AppResult<ExtensionHostCommandOutput> {
        self.execute_command_with_now(detail, command, args, Instant::now())
            .await
    }

    async fn execute_command_with_now(
        &self,
        detail: PluginDetail,
        command: &str,
        args: Value,
        now: Instant,
    ) -> AppResult<ExtensionHostCommandOutput> {
        let key = ExtensionHostInstanceKey::from_plugin_detail(&detail)?;
        let mut instances = self.instances.lock().await;

        if let Some(instance) = instances.get_mut(&key) {
            if instance.process.is_running() {
                let value = instance.process.execute_command(command, args).await?;
                instance.last_used = now;
                return Ok(ExtensionHostCommandOutput {
                    value,
                    cold_start: false,
                });
            }
        }

        if let Some(instance) = instances.remove(&key) {
            instance.dispose().await;
        }

        dispose_same_plugin_with_different_key(&mut instances, &key).await;
        dispose_idle_locked(&mut instances, self.limits.idle_recycle, now).await;

        let mut process = self.factory.start(detail, self.db.clone()).await?;
        let value = match process.execute_command(command, args).await {
            Ok(value) => value,
            Err(error) => {
                process.dispose().await;
                return Err(error);
            }
        };
        instances.insert(
            key,
            ManagedExtensionHostInstance {
                process,
                last_used: now,
            },
        );
        enforce_warm_limit_locked(&mut instances, self.limits.max_warm_instances).await;

        Ok(ExtensionHostCommandOutput {
            value,
            cold_start: true,
        })
    }

    #[allow(dead_code)]
    pub(crate) async fn dispose_plugin(&self, plugin_id: &str) {
        let mut instances = self.instances.lock().await;
        let keys = instances
            .keys()
            .filter(|key| key.plugin_id == plugin_id)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(instance) = instances.remove(&key) {
                instance.dispose().await;
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn dispose_idle(&self, now: Instant) {
        let mut instances = self.instances.lock().await;
        dispose_idle_locked(&mut instances, self.limits.idle_recycle, now).await;
    }

    pub(crate) async fn dispose_all(&self) {
        let mut instances = self.instances.lock().await;
        let instances = std::mem::take(&mut *instances);
        for (_, instance) in instances {
            instance.dispose().await;
        }
    }

    #[cfg(test)]
    fn new_for_tests(
        factory: Arc<dyn ExtensionHostFactory>,
        limits: ExtensionHostRegistryLimits,
    ) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("registry.db")).expect("init db");
        Self::with_factory(db, factory, limits)
    }

    #[cfg(test)]
    async fn instance_count(&self) -> usize {
        self.instances.lock().await.len()
    }
}

impl ExtensionHostInstanceKey {
    pub(crate) fn from_plugin_detail(detail: &PluginDetail) -> AppResult<Self> {
        let (runtime_kind, runtime_language) = match &detail.manifest.runtime {
            PluginRuntime::ExtensionHost { language } => {
                ("extensionHost".to_string(), language.clone())
            }
            PluginRuntime::Native { .. } => {
                return Err(AppError::new(
                    "PLUGIN_COMMAND_RUNTIME_UNSUPPORTED",
                    format!(
                        "plugin {} is not backed by an extension host runtime",
                        detail.summary.plugin_id
                    ),
                ));
            }
        };
        let main = detail
            .manifest
            .main
            .as_ref()
            .filter(|main| !main.trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                AppError::new("PLUGIN_MISSING_MAIN", "extensionHost runtime requires main")
            })?;
        Ok(Self {
            plugin_id: detail.manifest.id.clone(),
            version: detail.manifest.version.clone(),
            installed_dir: plugin_root(detail)?.display().to_string(),
            main,
            runtime_kind,
            runtime_language,
            contribution_hash: contribution_hash(&detail.manifest),
        })
    }
}

#[derive(Default)]
pub(crate) struct ExtensionHostRuntimeState {
    registry: Mutex<Option<Arc<ExtensionHostInstanceRegistry>>>,
}

impl ExtensionHostRuntimeState {
    #[allow(dead_code)]
    pub(crate) async fn registry<R: tauri::Runtime>(
        &self,
        app: tauri::AppHandle<R>,
        db_state: &DbInitState,
    ) -> AppResult<Arc<ExtensionHostInstanceRegistry>> {
        let mut guard = self.registry.lock().await;
        if let Some(registry) = guard.as_ref() {
            return Ok(registry.clone());
        }
        let db = ensure_db_ready(app, db_state).await?;
        let registry = Arc::new(ExtensionHostInstanceRegistry::new(db));
        *guard = Some(registry.clone());
        Ok(registry)
    }

    pub(crate) async fn dispose_all(&self) {
        let registry = { self.registry.lock().await.clone() };
        if let Some(registry) = registry {
            registry.dispose_all().await;
        }
    }

    pub(crate) async fn dispose_plugin_if_initialized(&self, plugin_id: &str) {
        let registry = { self.registry.lock().await.clone() };
        if let Some(registry) = registry {
            registry.dispose_plugin(plugin_id).await;
        }
    }

    #[cfg(test)]
    async fn set_registry_for_tests(&self, registry: Arc<ExtensionHostInstanceRegistry>) {
        *self.registry.lock().await = Some(registry);
    }
}

async fn dispose_same_plugin_with_different_key(
    instances: &mut BTreeMap<ExtensionHostInstanceKey, ManagedExtensionHostInstance>,
    key: &ExtensionHostInstanceKey,
) {
    let keys = instances
        .keys()
        .filter(|existing| existing.plugin_id == key.plugin_id && *existing != key)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(instance) = instances.remove(&key) {
            instance.dispose().await;
        }
    }
}

async fn dispose_idle_locked(
    instances: &mut BTreeMap<ExtensionHostInstanceKey, ManagedExtensionHostInstance>,
    idle_recycle: Duration,
    now: Instant,
) {
    let idle_keys = instances
        .iter()
        .filter(|(_, instance)| {
            now.checked_duration_since(instance.last_used)
                .is_some_and(|elapsed| elapsed >= idle_recycle)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in idle_keys {
        if let Some(instance) = instances.remove(&key) {
            instance.dispose().await;
        }
    }
}

async fn enforce_warm_limit_locked(
    instances: &mut BTreeMap<ExtensionHostInstanceKey, ManagedExtensionHostInstance>,
    max_warm_instances: usize,
) {
    while instances.len() > max_warm_instances {
        let Some(key) = instances
            .iter()
            .min_by_key(|(_, instance)| instance.last_used)
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        if let Some(instance) = instances.remove(&key) {
            instance.dispose().await;
        }
    }
}

fn plugin_root(detail: &PluginDetail) -> AppResult<PathBuf> {
    detail
        .installed_dir
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| {
            AppError::new(
                "PLUGIN_EXTENSION_HOST_ROOT_UNAVAILABLE",
                format!(
                    "plugin {} does not have an installed extension host directory",
                    detail.summary.plugin_id
                ),
            )
        })
}

fn contribution_hash(manifest: &PluginManifest) -> String {
    let bytes = serde_json::to_vec(&json!({
        "runtime": manifest.runtime,
        "main": manifest.main,
        "activationEvents": manifest.activation_events,
        "contributes": manifest.contributes,
        "capabilities": manifest.capabilities,
        "permissions": manifest.permissions,
    }))
    .unwrap_or_default();
    format!("{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugins::{
        PluginDetail, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::{Duration, Instant};

    struct FakeExtensionHostFactory {
        state: Arc<StdMutex<FakeFactoryState>>,
    }

    #[derive(Default)]
    struct FakeFactoryState {
        next_id: u64,
        starts: Vec<u64>,
        executions: Vec<u64>,
        disposals: Vec<u64>,
    }

    struct FakeExtensionHostProcess {
        id: u64,
        state: Arc<StdMutex<FakeFactoryState>>,
        running: bool,
    }

    impl Default for FakeExtensionHostFactory {
        fn default() -> Self {
            Self {
                state: Arc::new(StdMutex::new(FakeFactoryState::default())),
            }
        }
    }

    impl FakeExtensionHostFactory {
        fn start_count(&self) -> usize {
            self.state.lock().unwrap().starts.len()
        }

        fn dispose_count(&self) -> usize {
            self.state.lock().unwrap().disposals.len()
        }

        fn executed_instance_ids(&self) -> Vec<u64> {
            self.state.lock().unwrap().executions.clone()
        }

        fn disposed_instance_ids(&self) -> Vec<u64> {
            self.state.lock().unwrap().disposals.clone()
        }
    }

    impl ExtensionHostFactory for FakeExtensionHostFactory {
        fn start<'a>(
            &'a self,
            _detail: PluginDetail,
            _db: db::Db,
        ) -> BoxFuture<'a, AppResult<Box<dyn ExtensionHostProcess>>> {
            Box::pin(async move {
                let mut state = self.state.lock().unwrap();
                state.next_id += 1;
                let id = state.next_id;
                state.starts.push(id);
                Ok(Box::new(FakeExtensionHostProcess {
                    id,
                    state: self.state.clone(),
                    running: true,
                }) as Box<dyn ExtensionHostProcess>)
            })
        }
    }

    impl ExtensionHostProcess for FakeExtensionHostProcess {
        fn execute_command<'a>(
            &'a mut self,
            command: &'a str,
            args: Value,
        ) -> BoxFuture<'a, AppResult<Value>> {
            Box::pin(async move {
                self.state.lock().unwrap().executions.push(self.id);
                Ok(json!({
                    "instanceId": self.id,
                    "command": command,
                    "args": args,
                }))
            })
        }

        fn is_running(&mut self) -> bool {
            self.running
        }

        fn dispose<'a>(&'a mut self) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                self.running = false;
                self.state.lock().unwrap().disposals.push(self.id);
            })
        }
    }

    fn plugin_detail(plugin_id: &str, contribution_hash_seed: &str) -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: plugin_id.to_string(),
                name: "Acme Echo".to_string(),
                current_version: Some("1.0.0".to_string()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::Low,
                update_available: false,
                last_error: None,
                created_at: 0,
                updated_at: 0,
            },
            manifest: PluginManifest {
                id: plugin_id.to_string(),
                name: "Acme Echo".to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime: PluginRuntime::ExtensionHost {
                    language: "typescript".to_string(),
                },
                hooks: Vec::new(),
                permissions: Vec::new(),
                main: Some("dist/extension.js".to_string()),
                activation_events: Vec::new(),
                contributes: None,
                capabilities: vec![
                    "commands.execute".to_string(),
                    contribution_hash_seed.to_string(),
                ],
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.60.0".to_string(),
                    plugin_api: "^1.0.0".to_string(),
                    platforms: Vec::new(),
                },
                entry: None,
                config_schema: None,
                config_version: None,
                description: None,
                author: None,
                homepage: None,
                repository: None,
                license: None,
                checksum: None,
                signature: None,
                category: None,
            },
            install_source: PluginInstallSource::Local,
            installed_dir: Some(format!("/tmp/{plugin_id}")),
            config: json!({}),
            granted_permissions: Vec::new(),
            pending_permissions: Vec::new(),
            audit_logs: Vec::new(),
            runtime_failures: Vec::new(),
            rollback_versions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn registry_reuses_warm_instance_for_same_key() {
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 8,
                idle_recycle: Duration::from_secs(120),
            },
        );
        let detail = plugin_detail("acme.echo", "same");

        let first = registry
            .execute_command_with_now(
                detail.clone(),
                "acme.echo",
                json!({ "n": 1 }),
                Instant::now(),
            )
            .await
            .expect("first command");
        let second = registry
            .execute_command_with_now(detail, "acme.echo", json!({ "n": 2 }), Instant::now())
            .await
            .expect("second command");

        assert!(first.cold_start);
        assert!(!second.cold_start);
        assert_eq!(factory.start_count(), 1);
        assert_eq!(factory.executed_instance_ids(), vec![1, 1]);
    }

    #[tokio::test]
    async fn registry_replaces_instance_when_contribution_hash_changes() {
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 8,
                idle_recycle: Duration::from_secs(120),
            },
        );

        registry
            .execute_command_with_now(
                plugin_detail("acme.echo", "before"),
                "acme.echo",
                json!({}),
                Instant::now(),
            )
            .await
            .expect("first command");
        let changed = registry
            .execute_command_with_now(
                plugin_detail("acme.echo", "after"),
                "acme.echo",
                json!({}),
                Instant::now(),
            )
            .await
            .expect("changed command");

        assert!(changed.cold_start);
        assert_eq!(factory.start_count(), 2);
        assert_eq!(factory.dispose_count(), 1);
        assert_eq!(factory.disposed_instance_ids(), vec![1]);
    }

    #[tokio::test]
    async fn registry_disposes_plugin_instances() {
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 8,
                idle_recycle: Duration::from_secs(120),
            },
        );

        registry
            .execute_command_with_now(
                plugin_detail("acme.echo", "one"),
                "acme.echo",
                json!({}),
                Instant::now(),
            )
            .await
            .expect("first command");
        registry
            .execute_command_with_now(
                plugin_detail("acme.other", "two"),
                "acme.other",
                json!({}),
                Instant::now(),
            )
            .await
            .expect("second command");

        registry.dispose_plugin("acme.echo").await;

        assert_eq!(factory.disposed_instance_ids(), vec![1]);
        assert_eq!(registry.instance_count().await, 1);
    }

    #[tokio::test]
    async fn registry_disposes_idle_instances() {
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 8,
                idle_recycle: Duration::from_secs(10),
            },
        );
        let now = Instant::now();

        registry
            .execute_command_with_now(
                plugin_detail("acme.echo", "idle"),
                "acme.echo",
                json!({}),
                now,
            )
            .await
            .expect("first command");
        registry.dispose_idle(now + Duration::from_secs(11)).await;

        assert_eq!(factory.disposed_instance_ids(), vec![1]);
        assert_eq!(registry.instance_count().await, 0);
    }

    #[tokio::test]
    async fn registry_evicts_least_recently_used_idle_instance() {
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 2,
                idle_recycle: Duration::from_secs(120),
            },
        );
        let now = Instant::now();

        registry
            .execute_command_with_now(plugin_detail("acme.one", "one"), "acme.one", json!({}), now)
            .await
            .expect("first command");
        registry
            .execute_command_with_now(
                plugin_detail("acme.two", "two"),
                "acme.two",
                json!({}),
                now + Duration::from_secs(1),
            )
            .await
            .expect("second command");
        registry
            .execute_command_with_now(
                plugin_detail("acme.one", "one"),
                "acme.one",
                json!({}),
                now + Duration::from_secs(2),
            )
            .await
            .expect("touch first command");
        registry
            .execute_command_with_now(
                plugin_detail("acme.three", "three"),
                "acme.three",
                json!({}),
                now + Duration::from_secs(3),
            )
            .await
            .expect("third command");

        assert_eq!(factory.disposed_instance_ids(), vec![2]);
        assert_eq!(registry.instance_count().await, 2);
    }

    #[tokio::test]
    async fn runtime_state_dispose_plugin_if_initialized_is_noop_before_registry_init() {
        let state = ExtensionHostRuntimeState::default();

        state.dispose_plugin_if_initialized("acme.echo").await;
    }

    #[tokio::test]
    async fn runtime_state_dispose_plugin_if_initialized_disposes_existing_registry_instance() {
        let state = ExtensionHostRuntimeState::default();
        let factory = Arc::new(FakeExtensionHostFactory::default());
        let registry = Arc::new(ExtensionHostInstanceRegistry::new_for_tests(
            factory.clone(),
            ExtensionHostRegistryLimits {
                max_warm_instances: 8,
                idle_recycle: Duration::from_secs(120),
            },
        ));
        state.set_registry_for_tests(registry.clone()).await;

        registry
            .execute_command_with_now(
                plugin_detail("acme.echo", "dispose"),
                "acme.echo",
                json!({}),
                Instant::now(),
            )
            .await
            .expect("execute command");

        state.dispose_plugin_if_initialized("acme.echo").await;

        assert_eq!(factory.disposed_instance_ids(), vec![1]);
        assert_eq!(registry.instance_count().await, 0);
    }
}
