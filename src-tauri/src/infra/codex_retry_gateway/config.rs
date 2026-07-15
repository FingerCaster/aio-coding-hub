use serde_json::json;

use crate::infra::codex_retry_gateway::util::now_rfc3339;
use crate::infra::codex_retry_gateway::{AioGatewayOrigin, CODEX_RETRY_GATEWAY_DEFAULT_PORT};

pub(crate) const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";
pub(crate) const DEFAULT_HEALTH_PATH: &str = "/__codex_retry_gateway/health";
pub(crate) const DEFAULT_REQUEST_BODY_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

pub(crate) fn managed_gateway_config(
    listen_port: u16,
    aio_origin: &AioGatewayOrigin,
) -> serde_json::Value {
    json!({
        "listen_host": DEFAULT_LISTEN_HOST,
        "listen_port": if listen_port == 0 {
            CODEX_RETRY_GATEWAY_DEFAULT_PORT
        } else {
            listen_port
        },
        "upstream_base_url": aio_origin.url,
        "request_body_limit_bytes": DEFAULT_REQUEST_BODY_LIMIT_BYTES,
        "health_path": DEFAULT_HEALTH_PATH
    })
}

pub(crate) fn managed_gateway_state(
    gateway_base_url: &str,
    state_root: &str,
    config_path: &str,
    log_path: &str,
    pid_path: &str,
    upstream_base_url: &str,
    provider_name: &str,
    instance_nonce: &str,
) -> serde_json::Value {
    let now = now_rfc3339();
    json!({
        "installed_at": now,
        "last_started_at": now_rfc3339(),
        "codex_config_path": "",
        "provider_name": provider_name,
        "original_base_url": upstream_base_url,
        "gateway_base_url": gateway_base_url,
        "gateway_config_path": config_path,
        "gateway_log_path": log_path,
        "gateway_pid_path": pid_path,
        "latest_backup_path": serde_json::Value::Null,
        "state_root": state_root,
        "aio_instance_nonce": instance_nonce
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_gateway_config_pins_loopback_and_health_path() {
        let origin = AioGatewayOrigin {
            url: "http://127.0.0.1:37123/v1".to_string(),
        };
        let config = managed_gateway_config(4620, &origin);
        assert_eq!(config["listen_host"], DEFAULT_LISTEN_HOST);
        assert_eq!(config["listen_port"], 4620);
        assert_eq!(config["upstream_base_url"], origin.url);
        assert_eq!(config["health_path"], DEFAULT_HEALTH_PATH);
    }

    #[test]
    fn managed_gateway_state_omits_restorable_backup() {
        let state = managed_gateway_state(
            "http://127.0.0.1:4610",
            "D:/gateway",
            "D:/gateway/runtime/config/config.json",
            "D:/gateway/runtime/logs/gateway.log",
            "D:/gateway/runtime/gateway.pid",
            "http://127.0.0.1:37123/v1",
            "aio",
            "deadbeef",
        );
        assert!(state["latest_backup_path"].is_null());
        assert_eq!(state["aio_instance_nonce"], "deadbeef");
    }
}
