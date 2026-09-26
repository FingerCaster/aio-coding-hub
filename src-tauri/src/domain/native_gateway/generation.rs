//! Generated entries are a white-list projection, never a copied native/upstream node.
use super::metadata::{invalid, validate_client, NativeModelSpec, ThinkingSpec};
use crate::shared::error::AppResult;
use crate::shared::gateway_protocol::GatewayProtocol;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayCatalogGroup {
    pub protocol: GatewayProtocol,
    pub models: Vec<NativeModelSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedEntry {
    pub protocol: GatewayProtocol,
    pub native_key: String,
    pub base_url: String,
    pub models: Vec<NativeModelSpec>,
    pub node: Value,
}

pub(crate) fn generate_entries(
    client: &str,
    origin: &str,
    groups: &[GatewayCatalogGroup],
) -> AppResult<Vec<GeneratedEntry>> {
    validate_client(client)?;
    let parsed = reqwest::Url::parse(origin).map_err(|_| invalid("invalid gateway origin"))?;
    let local = parsed.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !local
        || parsed.scheme() != "http"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || groups.len() > 4
    {
        return Err(invalid("gateway origin must be a local HTTP listener"));
    }
    let mut seen = std::collections::BTreeSet::new();
    groups.iter().map(|group| {
        if !seen.insert(group.protocol) || group.models.is_empty() || group.models.len() > super::metadata::MAX_MODELS {
            return Err(invalid("each nonempty protocol group must be unique"));
        }
        let suffix = match group.protocol {
            GatewayProtocol::AnthropicMessages => "",
            GatewayProtocol::OpenaiCompletions | GatewayProtocol::OpenaiResponses => "/v1",
            GatewayProtocol::GoogleGenerativeAi => "/v1beta",
        };
        let base_url = format!("{}/{client}/_protocol/{}{suffix}", origin.trim_end_matches('/'), group.protocol.as_str());
        let mut ids = std::collections::BTreeSet::new();
            let models: Vec<Value> = group.models.iter().map(|model| {
            model.validate(client)?;
            if model.supports_tools.is_none() {return Err(invalid("confirm tool capability explicitly before publishing this model"));}
            if !ids.insert(&model.request_model_id) { return Err(invalid("duplicate published model")); }
            let mut out = json!({"id": model.request_model_id, "name": model.display_name,
                "input": model.input, "contextWindow": model.context_window, "maxTokens": model.max_tokens,
                "reasoning": model.reasoning});
            if client == "omp" {
                if let Some(tools) = model.supports_tools { out["supportsTools"] = json!(tools); }
                out["preferWebsockets"] = json!(false);
            }
            match &model.thinking {
                Some(ThinkingSpec::Pi { level_map }) => out["thinkingLevelMap"] = json!(level_map),
                Some(ThinkingSpec::Omp { mode, efforts, default_level, effort_map, supports_display, requires_effort }) => {
                    let mut thinking = json!({"mode": mode, "efforts": efforts});
                    if let Some(level) = default_level { thinking["defaultLevel"] = json!(level); }
                    if !effort_map.is_empty() { thinking["effortMap"] = json!(effort_map); }
                    if let Some(value) = supports_display { thinking["supportsDisplay"] = json!(value); }
                    if let Some(value) = requires_effort { thinking["requiresEffort"] = json!(value); }
                    out["thinking"] = thinking;
                },
                None => {},
            }
            Ok(out)
        }).collect::<AppResult<_>>()?;
        // This deliberately invalid local credential is stripped at the wire boundary.
        let key = crate::cli_proxy::PLACEHOLDER_KEY;
        let mut node = json!({"name": "AIO Coding Hub", "api": group.protocol.as_str(), "baseUrl": base_url, "apiKey": key, "models": models});
        // OMP defaults can choose OAuth tool-name/system transforms. Publication
        // explicitly uses the API-key branch verified by the real CLI checkpoint.
        if client == "omp" { node["auth"] = json!("apiKey"); }
        Ok(GeneratedEntry { protocol: group.protocol, native_key: format!("aio-coding-hub-{}",group.protocol.as_str()), base_url, models: group.models.clone(), node })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_entries_contain_only_local_transport_and_declared_capabilities() {
        let model = NativeModelSpec {
            request_model_id: "remote-model".into(),
            display_name: "Model".into(),
            input: vec![super::super::metadata::ModelInput::Text],
            context_window: 32000,
            max_tokens: 4000,
            reasoning: false,
            thinking: None,
            supports_tools: Some(true),
        };
        for client in ["pi", "omp"] {
            let groups = vec![GatewayCatalogGroup {
                protocol: GatewayProtocol::AnthropicMessages,
                models: vec![model.clone()],
            }];
            let entries = generate_entries(client, "http://127.0.0.1:3711", &groups).unwrap();
            let node = &entries[0].node;
            assert!(node["baseUrl"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:3711/"));
            for key in [
                "headers",
                "transport",
                "discovery",
                "oauth",
                "remoteCompaction",
            ] {
                assert!(node.get(key).is_none());
                assert!(node["models"][0].get(key).is_none());
            }
            assert!(generate_entries(client, "https://upstream.example", &groups).is_err());
            assert!(generate_entries(client, "http://secret@localhost:3711", &groups).is_err());
            assert_eq!(
                generate_entries(client, "http://127.0.0.1:3711", &groups).unwrap(),
                entries
            );
        }
    }
}
