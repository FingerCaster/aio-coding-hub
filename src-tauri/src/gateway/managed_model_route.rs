//! Server-owned routing context for Codex managed model aliases.

use serde_json::json;
use std::sync::{Arc, Mutex};

use crate::shared::mutex_ext::MutexExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway) struct ManagedModelRoute {
    pub(in crate::gateway) canonical_model: String,
    pub(in crate::gateway) model_uuid: String,
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_uuid: String,
    pub(in crate::gateway) remote_model_id: String,
}

impl ManagedModelRoute {
    pub(in crate::gateway) fn audit_requested_model(
        route: Option<&Self>,
        requested_model: Option<&str>,
        active_requested_model: Option<&str>,
    ) -> Option<String> {
        if route.is_some() {
            return requested_model.map(str::to_string);
        }
        active_requested_model
            .map(str::to_string)
            .or_else(|| requested_model.map(str::to_string))
    }

    pub(in crate::gateway) fn initial_special_setting(&self) -> serde_json::Value {
        json!({
            "type": "aio_managed_model_route",
            "scope": "request",
            "canonicalModel": self.canonical_model,
            "modelUuid": self.model_uuid,
            "providerId": self.provider_id,
            "providerUuid": self.provider_uuid,
            "remoteModelId": self.remote_model_id,
            "requestedUpstreamModel": null,
            "pricedModel": null,
            "applied": false,
            "observation": "unobserved",
        })
    }

    fn applied_special_setting(&self, wire_model: &str) -> serde_json::Value {
        json!({
            "type": "aio_managed_model_route",
            "scope": "request",
            "canonicalModel": self.canonical_model,
            "modelUuid": self.model_uuid,
            "providerId": self.provider_id,
            "providerUuid": self.provider_uuid,
            "remoteModelId": self.remote_model_id,
            "requestedUpstreamModel": wire_model,
            "pricedModel": wire_model,
            "applied": true,
            "observation": "unobserved",
        })
    }
}

pub(in crate::gateway) fn push_initial_special_setting(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    route: &ManagedModelRoute,
) {
    crate::gateway::response_fixer::upsert_aio_managed_model_route(
        special_settings,
        route.initial_special_setting(),
    );
}

pub(in crate::gateway) fn mark_applied(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    route: &ManagedModelRoute,
    wire_model: &str,
) {
    crate::gateway::response_fixer::upsert_aio_managed_model_route(
        special_settings,
        route.applied_special_setting(wire_model),
    );
}

pub(in crate::gateway) fn update_observation(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    observation: &'static str,
) {
    let mut settings = special_settings.lock_or_recover();
    let Some(route) = settings.iter_mut().rev().find(|setting| {
        setting.get("type").and_then(serde_json::Value::as_str) == Some("aio_managed_model_route")
            && setting
                .get("providerId")
                .and_then(serde_json::Value::as_i64)
                == Some(provider_id)
            && setting.get("applied").and_then(serde_json::Value::as_bool) == Some(true)
    }) else {
        return;
    };

    if let Some(object) = route.as_object_mut() {
        object.insert("observation".to_string(), json!(observation));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> ManagedModelRoute {
        ManagedModelRoute {
            canonical_model: "aio/11111111-1111-4111-8111-111111111111".to_string(),
            model_uuid: "11111111-1111-4111-8111-111111111111".to_string(),
            provider_id: 17,
            provider_uuid: "22222222-2222-4222-8222-222222222222".to_string(),
            remote_model_id: "grok-4.5".to_string(),
        }
    }

    #[test]
    fn managed_route_keeps_canonical_model_for_request_audit() {
        let route = route();
        assert_eq!(
            ManagedModelRoute::audit_requested_model(
                Some(&route),
                Some(&route.canonical_model),
                Some("grok-4.5"),
            )
            .as_deref(),
            Some("aio/11111111-1111-4111-8111-111111111111")
        );
    }

    #[test]
    fn ordinary_route_keeps_existing_active_model_precedence() {
        let requested = "gpt-requested".to_string();
        assert_eq!(
            ManagedModelRoute::audit_requested_model(None, Some(&requested), Some("gpt-active"))
                .as_deref(),
            Some("gpt-active")
        );
    }

    #[test]
    fn initial_setting_is_provider_scoped_unobserved_and_not_applied() {
        let setting = route().initial_special_setting();
        assert_eq!(
            setting.get("type").and_then(serde_json::Value::as_str),
            Some("aio_managed_model_route")
        );
        assert_eq!(
            setting
                .get("providerId")
                .and_then(serde_json::Value::as_i64),
            Some(17)
        );
        assert_eq!(
            setting
                .get("providerUuid")
                .and_then(serde_json::Value::as_str),
            Some("22222222-2222-4222-8222-222222222222")
        );
        assert_eq!(
            setting
                .get("observation")
                .and_then(serde_json::Value::as_str),
            Some("unobserved")
        );
        assert_eq!(
            setting.get("applied").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert!(setting
            .get("requestedUpstreamModel")
            .is_some_and(serde_json::Value::is_null));
    }

    #[test]
    fn applied_setting_can_be_updated_with_observation() {
        let route = route();
        let settings = Arc::new(Mutex::new(Vec::new()));
        push_initial_special_setting(&settings, &route);
        mark_applied(&settings, &route, "grok-4.5");
        update_observation(&settings, route.provider_id, "matched");

        let settings = settings.lock().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(
            settings[0]
                .get("requestedUpstreamModel")
                .and_then(serde_json::Value::as_str),
            Some("grok-4.5")
        );
        assert_eq!(
            settings[0]
                .get("observation")
                .and_then(serde_json::Value::as_str),
            Some("matched")
        );
    }
}
