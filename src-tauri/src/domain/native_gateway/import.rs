//! Credential-blind snapshot import. Native expressions are data, never evaluated.
use super::metadata::{
    bounded_text, invalid, validate_client, ModelInput, NativeModelSpec, ThinkingSpec, MAX_MODELS,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::gateway_protocol::GatewayProtocol;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayImportIssue {
    pub model_id: Option<String>,
    pub code: String,
    pub blocking: bool,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayImportGroup {
    pub group_id: String,
    pub protocol: GatewayProtocol,
    pub base_url: String,
    pub models: Vec<NativeModelSpec>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayImportPreview {
    pub target_id: String,
    pub native_key: String,
    pub revision: String,
    pub groups: Vec<GatewayImportGroup>,
    pub issues: Vec<GatewayImportIssue>,
    pub can_import: bool,
}
// Explicit user credentials never derive Debug or Serialize.
#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayImportCredential {
    pub group_id: String,
    pub api_key: String,
}
#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayImportConfirmInput {
    pub target_id: String,
    pub native_key: String,
    pub expected_revision: String,
    pub credentials: Vec<GatewayImportCredential>,
}

pub(crate) fn validate_explicit_api_key(key: &str) -> AppResult<()> {
    let key = key.trim();
    if key.is_empty()
        || key.len() > 8192
        || key.chars().any(char::is_control)
        || key.starts_with('!')
        || key.contains('$')
        || key.contains('`')
        || key.contains('%')
    {
        return Err(AppError::new(
            "NATIVE_GATEWAY_API_KEY_REQUIRED",
            "enter a literal API key in AIO; native expressions are not evaluated",
        ));
    }
    Ok(())
}

pub(crate) fn static_url(value: &str) -> AppResult<String> {
    if value.contains('$') || value.contains('`') || value.starts_with('!') || value.contains('%') {
        return Err(invalid("dynamic transport is unsupported"));
    }
    let url =
        reqwest::Url::parse(value).map_err(|_| invalid("explicit HTTP base URL is required"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "base URL contains unsupported credentials or transport options",
        ));
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn nonempty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Object(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        _ => true,
    }
}

pub(crate) fn model_from_native(client: &str, value: &Value) -> AppResult<NativeModelSpec> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("model must be an object"))?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("explicit model ID is required"))?;
    let input: Vec<ModelInput> = serde_json::from_value(
        obj.get("input")
            .cloned()
            .ok_or_else(|| invalid("explicit input capabilities are required"))?,
    )
    .map_err(|_| invalid("invalid input capabilities"))?;
    let number = |field: &str| {
        obj.get(field)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("explicit integer model capacities are required"))
    };
    let reasoning = obj
        .get("reasoning")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("explicit reasoning capability is required"))?;
    let thinking = if !reasoning {
        None
    } else if client == "pi" {
        Some(ThinkingSpec::Pi {
            level_map: serde_json::from_value(
                obj.get("thinkingLevelMap")
                    .cloned()
                    .ok_or_else(|| invalid("complete Pi thinking map is required"))?,
            )
            .map_err(|_| invalid("invalid Pi thinking map"))?,
        })
    } else {
        let mut thinking = obj
            .get("thinking")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| invalid("explicit OMP thinking declaration is required"))?;
        thinking.insert("client".into(), Value::String("omp".into()));
        thinking
            .entry("effortMap")
            .or_insert_with(|| Value::Object(Map::new()));
        Some(
            serde_json::from_value(Value::Object(thinking))
                .map_err(|_| invalid("unsupported OMP thinking declaration"))?,
        )
    };
    let out = NativeModelSpec {
        request_model_id: id.into(),
        display_name: obj.get("name").and_then(Value::as_str).unwrap_or(id).into(),
        input,
        context_window: number("contextWindow")?,
        max_tokens: number("maxTokens")?,
        reasoning,
        thinking,
        supports_tools: obj.get("supportsTools").and_then(Value::as_bool),
    };
    out.validate(client)?;
    Ok(out)
}

pub(crate) fn preview_import(
    client: &str,
    target_id: &str,
    native_key: &str,
    revision: &str,
    node: &Value,
) -> AppResult<GatewayImportPreview> {
    validate_client(client)?;
    let provider = node
        .as_object()
        .ok_or_else(|| invalid("native provider must be an object"))?;
    let mut out = GatewayImportPreview {
        target_id: target_id.into(),
        native_key: native_key.into(),
        revision: revision.into(),
        groups: vec![],
        issues: vec![],
        can_import: false,
    };
    for (key, value) in provider {
        if key == "auth" && client == "omp" && value.as_str() == Some("apiKey") {
            continue;
        }
        if !matches!(
            key.as_str(),
            "name" | "api" | "baseUrl" | "apiKey" | "models" | "modelOverrides"
        ) && nonempty(value)
        {
            out.issues.push(GatewayImportIssue {
                model_id: None,
                code: "UNSUPPORTED_PROVIDER_FIELD".into(),
                blocking: true,
            });
        }
    }
    // Even a literal source key is never copied. AIO credentials are supplied only at confirm.
    out.issues.push(GatewayImportIssue {
        model_id: None,
        code: "AIO_API_KEY_REQUIRED".into(),
        blocking: false,
    });
    let Some(models) = provider
        .get("models")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= MAX_MODELS)
    else {
        out.issues.push(GatewayImportIssue {
            model_id: None,
            code: "EXPLICIT_MODELS_REQUIRED".into(),
            blocking: true,
        });
        return Ok(out);
    };
    let mut groups: BTreeMap<(GatewayProtocol, String), Vec<NativeModelSpec>> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for raw in models {
        let mut model = raw.clone();
        let raw_id = model
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let safe_id = bounded_text(&raw_id, 256).then(|| raw_id.clone());
        if let Some(patch) = provider.get("modelOverrides").and_then(|v| v.get(&raw_id)) {
            if let (Some(base), Some(patch)) = (model.as_object_mut(), patch.as_object()) {
                base.extend(patch.clone());
            } else {
                out.issues.push(GatewayImportIssue {
                    model_id: safe_id.clone(),
                    code: "UNSUPPORTED_MODEL_OVERRIDE".into(),
                    blocking: true,
                });
                continue;
            }
        }
        let parsed = (|| -> AppResult<(GatewayProtocol, String, NativeModelSpec)> {
            let obj = model
                .as_object()
                .ok_or_else(|| invalid("model must be an object"))?;
            for (key, value) in obj {
                if !matches!(
                    key.as_str(),
                    "id" | "name"
                        | "api"
                        | "baseUrl"
                        | "input"
                        | "contextWindow"
                        | "maxTokens"
                        | "reasoning"
                        | "thinkingLevelMap"
                        | "thinking"
                        | "supportsTools"
                        | "cost"
                ) && nonempty(value)
                {
                    return Err(invalid("unsupported native model field"));
                }
            }
            let effective = |key| {
                obj.get(key)
                    .or_else(|| provider.get(key))
                    .and_then(Value::as_str)
            };
            let protocol: GatewayProtocol = effective("api")
                .ok_or_else(|| invalid("explicit protocol is required"))?
                .parse()
                .map_err(|_| invalid("unsupported protocol"))?;
            let url = static_url(
                effective("baseUrl").ok_or_else(|| invalid("explicit base URL is required"))?,
            )?;
            let spec = model_from_native(client, &model)?;
            if !ids.insert((protocol, spec.request_model_id.clone())) {
                return Err(invalid("duplicate model"));
            }
            Ok((protocol, url, spec))
        })();
        match parsed {
            Ok((protocol, url, spec)) => {
                if model.get("cost").is_some() {
                    out.issues.push(GatewayImportIssue {
                        model_id: safe_id,
                        code: "NATIVE_PRICING_NOT_IMPORTED".into(),
                        blocking: false,
                    });
                }
                groups.entry((protocol, url)).or_default().push(spec);
            }
            Err(_) => out.issues.push(GatewayImportIssue {
                model_id: safe_id,
                code: "UNSUPPORTED_OR_INCOMPLETE_MODEL".into(),
                blocking: true,
            }),
        }
    }
    if let Some(overrides) = provider.get("modelOverrides").and_then(Value::as_object) {
        if overrides.keys().any(|key| {
            !models
                .iter()
                .any(|m| m.get("id").and_then(Value::as_str) == Some(key))
        }) {
            out.issues.push(GatewayImportIssue {
                model_id: None,
                code: "BUILTIN_MODEL_OVERRIDE_REQUIRES_EXPLICIT_MODEL".into(),
                blocking: true,
            });
        }
    }
    out.groups = groups
        .into_iter()
        .map(|((protocol, base_url), models)| {
            let hash = Sha256::digest(format!("{}\n{}", protocol.as_str(), base_url));
            GatewayImportGroup {
                group_id: format!("{hash:x}"),
                protocol,
                base_url,
                models,
            }
        })
        .collect();
    out.can_import = !out.groups.is_empty() && !out.issues.iter().any(|issue| issue.blocking);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!({"api":"openai-responses","baseUrl":"https://upstream.example/v1","apiKey":"!echo NEVER_EXECUTE","models":[{"id":"explicit","input":["text"],"contextWindow":32000,"maxTokens":4000,"reasoning":false}]})
    }
    #[test]
    fn preview_is_credential_blind_and_groups_effective_model_overrides() {
        let mut node = fixture();
        let mut second = node["models"][0].clone();
        second["id"] = json!("second");
        second["baseUrl"] = json!("https://other.example/v1");
        node["models"].as_array_mut().unwrap().push(second);
        let out = preview_import("pi", "target", "native", "rev", &node).unwrap();
        assert!(out.can_import);
        assert_eq!(out.groups.len(), 2);
        assert!(!serde_json::to_string(&out)
            .unwrap()
            .contains("NEVER_EXECUTE"));
    }
    #[test]
    fn dynamic_transports_and_unsupported_native_semantics_are_rejected() {
        for field in [
            "headers",
            "transport",
            "discovery",
            "oauth",
            "remoteCompaction",
        ] {
            let mut node = fixture();
            node[field] = json!("sensitive");
            let out = preview_import("omp", "t", "n", "r", &node).unwrap();
            assert!(!out.can_import);
            assert!(!serde_json::to_string(&out).unwrap().contains("sensitive"));
        }
        let mut node = fixture();
        node["baseUrl"] = json!("${UPSTREAM}");
        assert!(
            !preview_import("pi", "t", "n", "r", &node)
                .unwrap()
                .can_import
        );
        for key in ["!command", "${ENV}", "%ENV%", "`command`", ""] {
            assert!(validate_explicit_api_key(key).is_err());
        }
    }
}
