//! Usage: Host-owned plugin runtime lifecycle and cache retention boundary.

use crate::plugins::PluginDetail;
use std::sync::{Arc, RwLock};

pub(crate) trait PluginRuntimeCache: Send + Sync {
    fn retain_for_plugins(&self, plugins: &[PluginDetail]);
    #[allow(dead_code)]
    fn clear_all(&self);
}

pub(crate) trait PluginRuntimeInstanceRegistry: Send + Sync {
    fn retain_for_plugins(&self, plugins: &[PluginDetail]);
    fn dispose_plugin(&self, plugin_id: &str);
    fn dispose_all(&self);
}

#[derive(Default)]
pub(crate) struct RuntimeLifecycleRegistry {
    caches: RwLock<Vec<Arc<dyn PluginRuntimeCache>>>,
    instances: RwLock<Vec<Arc<dyn PluginRuntimeInstanceRegistry>>>,
}

impl RuntimeLifecycleRegistry {
    pub(crate) fn register_cache(&self, cache: Arc<dyn PluginRuntimeCache>) {
        self.caches
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(cache);
    }

    #[allow(dead_code)]
    pub(crate) fn register_instance_registry(
        &self,
        registry: Arc<dyn PluginRuntimeInstanceRegistry>,
    ) {
        self.instances
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(registry);
    }

    pub(crate) fn retain_for_plugins(&self, plugins: &[PluginDetail]) {
        for cache in self.caches_snapshot() {
            cache.retain_for_plugins(plugins);
        }
        for registry in self.instances_snapshot() {
            registry.retain_for_plugins(plugins);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn dispose_plugin(&self, plugin_id: &str) {
        for registry in self.instances_snapshot() {
            registry.dispose_plugin(plugin_id);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn dispose_all(&self) {
        for cache in self.caches_snapshot() {
            cache.clear_all();
        }
        for registry in self.instances_snapshot() {
            registry.dispose_all();
        }
    }

    fn caches_snapshot(&self) -> Vec<Arc<dyn PluginRuntimeCache>> {
        self.caches
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn instances_snapshot(&self) -> Vec<Arc<dyn PluginRuntimeInstanceRegistry>> {
        self.instances
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingCache {
        retain_calls: Mutex<Vec<Vec<String>>>,
        clear_calls: Mutex<u32>,
    }

    #[derive(Default)]
    struct RecordingInstanceRegistry {
        retain_calls: Mutex<Vec<Vec<String>>>,
        dispose_plugin_calls: Mutex<Vec<String>>,
        dispose_all_calls: Mutex<u32>,
    }

    impl RecordingCache {
        fn retain_calls(&self) -> Vec<Vec<String>> {
            self.retain_calls.lock().unwrap().clone()
        }

        fn clear_calls(&self) -> u32 {
            *self.clear_calls.lock().unwrap()
        }
    }

    impl PluginRuntimeCache for RecordingCache {
        fn retain_for_plugins(&self, plugins: &[PluginDetail]) {
            self.retain_calls.lock().unwrap().push(
                plugins
                    .iter()
                    .map(|plugin| plugin.summary.plugin_id.clone())
                    .collect(),
            );
        }

        fn clear_all(&self) {
            *self.clear_calls.lock().unwrap() += 1;
        }
    }

    impl RecordingInstanceRegistry {
        fn retain_calls(&self) -> Vec<Vec<String>> {
            self.retain_calls.lock().unwrap().clone()
        }

        fn dispose_plugin_calls(&self) -> Vec<String> {
            self.dispose_plugin_calls.lock().unwrap().clone()
        }

        fn dispose_all_calls(&self) -> u32 {
            *self.dispose_all_calls.lock().unwrap()
        }
    }

    impl PluginRuntimeInstanceRegistry for RecordingInstanceRegistry {
        fn retain_for_plugins(&self, plugins: &[PluginDetail]) {
            self.retain_calls.lock().unwrap().push(
                plugins
                    .iter()
                    .map(|plugin| plugin.summary.plugin_id.clone())
                    .collect(),
            );
        }

        fn dispose_plugin(&self, plugin_id: &str) {
            self.dispose_plugin_calls
                .lock()
                .unwrap()
                .push(plugin_id.to_string());
        }

        fn dispose_all(&self) {
            *self.dispose_all_calls.lock().unwrap() += 1;
        }
    }

    #[test]
    fn lifecycle_registry_retains_and_disposes_all_registered_runtime_caches() {
        let registry = RuntimeLifecycleRegistry::default();
        let first = std::sync::Arc::new(RecordingCache::default());
        let second = std::sync::Arc::new(RecordingCache::default());

        registry.register_cache(first.clone());
        registry.register_cache(second.clone());
        registry.retain_for_plugins(&[]);
        registry.dispose_all();

        assert_eq!(first.retain_calls(), vec![Vec::<String>::new()]);
        assert_eq!(second.retain_calls(), vec![Vec::<String>::new()]);
        assert_eq!(first.clear_calls(), 1);
        assert_eq!(second.clear_calls(), 1);
    }

    #[test]
    fn lifecycle_registry_retains_and_disposes_registered_runtime_instances() {
        let registry = RuntimeLifecycleRegistry::default();
        let instances = std::sync::Arc::new(RecordingInstanceRegistry::default());

        registry.register_instance_registry(instances.clone());
        registry.retain_for_plugins(&[]);
        registry.dispose_plugin("acme.echo");
        registry.dispose_all();

        assert_eq!(instances.retain_calls(), vec![Vec::<String>::new()]);
        assert_eq!(
            instances.dispose_plugin_calls(),
            vec!["acme.echo".to_string()]
        );
        assert_eq!(instances.dispose_all_calls(), 1);
    }
}
