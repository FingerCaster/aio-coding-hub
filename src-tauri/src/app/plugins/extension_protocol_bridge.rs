use serde_json::Value;

use crate::shared::error::{AppError, AppResult};

#[derive(Debug, Default, Clone)]
pub(crate) struct ExtensionProtocolBridgeRegistry;

impl ExtensionProtocolBridgeRegistry {
    pub(crate) fn contribution_id(plugin_id: &str, bridge_type: &str) -> String {
        if is_namespaced_by_plugin(plugin_id, bridge_type) {
            bridge_type.to_string()
        } else {
            format!("{plugin_id}:{bridge_type}")
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn dispatch(
        &self,
        plugin_id: &str,
        bridge_type: &str,
        payload: Value,
    ) -> AppResult<Value> {
        self.execute(plugin_id, bridge_type, payload).await
    }

    pub(crate) async fn execute(
        &self,
        plugin_id: &str,
        bridge_type: &str,
        _payload: Value,
    ) -> AppResult<Value> {
        Err(AppError::new(
            "PLUGIN_EXTENSION_PROTOCOL_BRIDGE_NOT_IMPLEMENTED",
            format!(
                "extension protocol bridge {} is not implemented",
                Self::contribution_id(plugin_id, bridge_type)
            ),
        ))
    }
}

fn is_namespaced_by_plugin(plugin_id: &str, value: &str) -> bool {
    if value == plugin_id {
        return true;
    }
    value
        .strip_prefix(plugin_id)
        .is_some_and(|suffix| matches!(suffix.as_bytes().first(), Some(b'.' | b'/' | b':')))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ExtensionProtocolBridgeRegistry;

    #[test]
    fn contribution_id_namespaces_bridge_type() {
        assert_eq!(
            ExtensionProtocolBridgeRegistry::contribution_id("acme.bridge", "openai-gemini"),
            "acme.bridge:openai-gemini"
        );
    }

    #[tokio::test]
    async fn dispatch_returns_not_implemented() {
        let registry = ExtensionProtocolBridgeRegistry;

        let err = registry
            .dispatch(
                "acme.bridge",
                "acme.bridge.openai-gemini",
                json!({ "body": "hello" }),
            )
            .await
            .unwrap_err();

        assert_eq!(
            err.code(),
            "PLUGIN_EXTENSION_PROTOCOL_BRIDGE_NOT_IMPLEMENTED"
        );
    }
}
