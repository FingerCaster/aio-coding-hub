use serde_json::json;

use crate::infra::codex_retry_gateway::util::now_rfc3339;
use crate::infra::codex_retry_gateway::{AioGatewayOrigin, CODEX_RETRY_GATEWAY_DEFAULT_PORT};

pub(crate) const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";
pub(crate) const DEFAULT_HEALTH_PATH: &str = "/__codex_retry_gateway/health";
pub(crate) const DEFAULT_REQUEST_BODY_LIMIT_BYTES: u64 = 100 * 1024 * 1024;
pub(crate) const MANAGED_PROVIDER_AIO: &str = "aio";
pub(crate) const MANAGED_PROVIDER_OPENAI: &str = "OpenAI";

pub(crate) fn normalize_preferred_port(port: u16, fallback: u16) -> u16 {
    if port >= 1024 {
        port
    } else {
        fallback
    }
}

pub(crate) fn validate_managed_provider_name(
    provider_name: &str,
) -> crate::shared::error::AppResult<()> {
    match provider_name {
        MANAGED_PROVIDER_AIO | MANAGED_PROVIDER_OPENAI => Ok(()),
        _ => Err(format!(
            "CODEX_RETRY_GATEWAY_PROVIDER_INVALID: unsupported managed provider {provider_name}"
        )
        .into()),
    }
}

pub(crate) fn managed_gateway_config(
    listen_port: u16,
    aio_origin: &AioGatewayOrigin,
) -> serde_json::Value {
    json!({
        "listen_host": DEFAULT_LISTEN_HOST,
        "listen_port": normalize_preferred_port(
            listen_port,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT,
        ),
        "upstream_base_url": aio_origin.url,
        "request_body_limit_bytes": DEFAULT_REQUEST_BODY_LIMIT_BYTES,
        "health_path": DEFAULT_HEALTH_PATH
    })
}

pub(crate) struct ManagedGatewayStateInput<'a> {
    pub(crate) gateway_base_url: &'a str,
    pub(crate) state_root: &'a str,
    pub(crate) config_path: &'a str,
    pub(crate) log_path: &'a str,
    pub(crate) pid_path: &'a str,
    pub(crate) upstream_base_url: &'a str,
    pub(crate) provider_name: &'a str,
    pub(crate) instance_nonce: &'a str,
    pub(crate) process_id: Option<u32>,
    pub(crate) process_start_identity: Option<u64>,
}

pub(crate) fn managed_gateway_state(input: ManagedGatewayStateInput<'_>) -> serde_json::Value {
    let now = now_rfc3339();
    json!({
        "installed_at": now,
        "last_started_at": now_rfc3339(),
        "codex_config_path": "",
        "provider_name": input.provider_name,
        "original_base_url": input.upstream_base_url,
        "gateway_base_url": input.gateway_base_url,
        "gateway_config_path": input.config_path,
        "gateway_log_path": input.log_path,
        "gateway_pid_path": input.pid_path,
        "latest_backup_path": serde_json::Value::Null,
        "state_root": input.state_root,
        "aio_instance_nonce": input.instance_nonce,
        "process_id": input.process_id,
        "aio_process_start_identity": input.process_start_identity
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
    fn normalize_preferred_port_preserves_every_non_privileged_port() {
        assert_eq!(normalize_preferred_port(0, 4610), 4610);
        assert_eq!(normalize_preferred_port(1023, 4610), 4610);
        assert_eq!(normalize_preferred_port(1024, 4610), 1024);
        assert_eq!(normalize_preferred_port(4609, 4610), 4609);
        assert_eq!(normalize_preferred_port(4610, 4610), 4610);
        assert_eq!(normalize_preferred_port(u16::MAX, 4610), u16::MAX);
    }

    #[test]
    fn managed_gateway_state_omits_restorable_backup() {
        let state = managed_gateway_state(ManagedGatewayStateInput {
            gateway_base_url: "http://127.0.0.1:4610",
            state_root: "D:/gateway/runtime",
            config_path: "D:/gateway/runtime/config/config.json",
            log_path: "D:/gateway/runtime/logs/gateway.log",
            pid_path: "D:/gateway/runtime/gateway.pid",
            upstream_base_url: "http://127.0.0.1:37123/v1",
            provider_name: "aio",
            instance_nonce: "deadbeef",
            process_id: Some(7),
            process_start_identity: Some(99),
        });
        assert!(state["latest_backup_path"].is_null());
        assert_eq!(state["aio_instance_nonce"], "deadbeef");
        assert_eq!(state["process_id"], 7);
        assert_eq!(state["aio_process_start_identity"], 99);
    }
}
