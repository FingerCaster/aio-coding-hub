//! Usage: Shared plugin runtime cache key helpers.

pub(crate) struct RuntimeCacheKeyInput<'a> {
    pub(crate) plugin_id: &'a str,
    pub(crate) version: &'a str,
    pub(crate) installed_dir: &'a str,
    pub(crate) updated_at: i64,
    pub(crate) runtime_key: &'a str,
}

pub(crate) fn runtime_cache_key(input: RuntimeCacheKeyInput<'_>) -> String {
    format!(
        "{}\u{1e}{}\u{1e}{}\u{1e}{}\u{1e}{}",
        input.plugin_id, input.version, input.installed_dir, input.updated_at, input.runtime_key
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_changes_with_updated_at() {
        let first = runtime_cache_key(RuntimeCacheKeyInput {
            plugin_id: "plugin.alpha",
            version: "1.0.0",
            installed_dir: "/plugins/alpha",
            updated_at: 10,
            runtime_key: "rules/a.json",
        });
        let second = runtime_cache_key(RuntimeCacheKeyInput {
            plugin_id: "plugin.alpha",
            version: "1.0.0",
            installed_dir: "/plugins/alpha",
            updated_at: 11,
            runtime_key: "rules/a.json",
        });

        assert_ne!(first, second);
        assert_eq!(
            first,
            "plugin.alpha\u{1e}1.0.0\u{1e}/plugins/alpha\u{1e}10\u{1e}rules/a.json"
        );
    }

    #[test]
    fn cache_key_includes_runtime_key() {
        let rule_key = runtime_cache_key(RuntimeCacheKeyInput {
            plugin_id: "plugin.alpha",
            version: "1.0.0",
            installed_dir: "/plugins/alpha",
            updated_at: 10,
            runtime_key: "rules/a.json\u{1f}rules/b.json",
        });
        let native_key = runtime_cache_key(RuntimeCacheKeyInput {
            plugin_id: "plugin.alpha",
            version: "1.0.0",
            installed_dir: "/plugins/alpha",
            updated_at: 10,
            runtime_key: "native:hostPrivateRedactor",
        });

        assert_ne!(rule_key, native_key);
        assert!(rule_key.ends_with("\u{1e}rules/a.json\u{1f}rules/b.json"));
        assert!(native_key.ends_with("\u{1e}native:hostPrivateRedactor"));
    }
}
