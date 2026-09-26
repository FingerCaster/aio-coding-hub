use super::{NativeClient, NativeFieldPatch};
use crate::shared::error::{AppError, AppResult};
use serde_json::Value;

fn invalid(message: &str) -> AppError {
    AppError::new("NATIVE_INVALID_NODE", message)
}

pub(crate) fn validate_native_key(key: &str) -> AppResult<()> {
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
        return Err(invalid(
            "Provider key must be nonempty, bounded, and contain no control characters",
        ));
    }
    Ok(())
}

fn optional_string(node: &Value, key: &str) -> AppResult<()> {
    if let Some(value) = node.get(key) {
        if value.as_str().is_none_or(|s| s.is_empty()) {
            return Err(invalid("A known text field must be a nonempty string"));
        }
    }
    Ok(())
}

fn object_strings(node: &Value, key: &str) -> AppResult<()> {
    if let Some(value) = node.get(key) {
        let values = value
            .as_object()
            .ok_or_else(|| invalid("Headers must be a string map"))?;
        if values.values().any(|v| !v.is_string()) {
            return Err(invalid("Headers must be a string map"));
        }
    }
    Ok(())
}

fn optional_bool(node: &Value, key: &str) -> AppResult<()> {
    if node.get(key).is_some_and(|value| !value.is_boolean()) {
        return Err(invalid("A known flag must be boolean"));
    }
    Ok(())
}

fn validate_cost(cost: &Value, partial: bool, pi: bool) -> AppResult<()> {
    if !cost.is_object() {
        return Err(invalid("Model cost must be an object"));
    }
    for key in ["input", "output", "cacheRead", "cacheWrite"] {
        match cost.get(key) {
            Some(value) if value.is_number() => {}
            None if partial => {}
            _ => return Err(invalid("Model cost rates must be numbers")),
        }
    }
    if pi {
        if let Some(tiers) = cost.get("tiers") {
            let tiers = tiers
                .as_array()
                .ok_or_else(|| invalid("Cost tiers must be an array"))?;
            for tier in tiers {
                validate_cost(tier, false, false)?;
                if !tier.get("inputTokensAbove").is_some_and(Value::is_number) {
                    return Err(invalid("Cost tier threshold must be a number"));
                }
            }
        }
    }
    Ok(())
}

fn validate_thinking(client: NativeClient, model: &Value) -> AppResult<()> {
    const EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
    if client == NativeClient::Pi {
        if let Some(map) = model.get("thinkingLevelMap") {
            if !map.is_object() {
                return Err(invalid("Pi thinkingLevelMap must be an object"));
            }
            for key in EFFORTS.into_iter().chain(["off"]) {
                if map.get(key).is_some_and(|v| !v.is_string() && !v.is_null()) {
                    return Err(invalid("Pi thinking map values must be strings or null"));
                }
            }
        }
        return Ok(());
    }
    let Some(thinking) = model.get("thinking") else {
        return Ok(());
    };
    if !thinking.is_object()
        || !matches!(
            thinking.get("mode").and_then(Value::as_str),
            Some(
                "effort"
                    | "budget"
                    | "google-level"
                    | "anthropic-adaptive"
                    | "anthropic-budget-effort"
            )
        )
    {
        return Err(invalid("Invalid OMP thinking control mode"));
    }
    let valid_effort = |v: &Value| v.as_str().is_some_and(|s| EFFORTS.contains(&s));
    for key in ["defaultLevel", "minLevel", "maxLevel"] {
        if thinking.get(key).is_some_and(|v| !valid_effort(v)) {
            return Err(invalid("Invalid OMP thinking effort"));
        }
    }
    for key in ["efforts", "levels"] {
        if let Some(levels) = thinking.get(key) {
            if !levels
                .as_array()
                .is_some_and(|a| a.iter().all(valid_effort))
            {
                return Err(invalid(
                    "OMP thinking efforts must be an array of native levels",
                ));
            }
        }
    }
    if thinking.get("efforts").is_none()
        && thinking.get("levels").is_none()
        && !(thinking.get("minLevel").is_some() && thinking.get("maxLevel").is_some())
    {
        return Err(invalid(
            "OMP thinking requires efforts or a legacy level range",
        ));
    }
    for key in ["supportsDisplay", "requiresEffort"] {
        optional_bool(thinking, key)?;
    }
    if let Some(map) = thinking.get("effortMap") {
        if !map.is_object()
            || EFFORTS
                .iter()
                .any(|key| map.get(*key).is_some_and(|v| !v.is_string()))
        {
            return Err(invalid("OMP effortMap must map native levels to strings"));
        }
    }
    Ok(())
}

fn validate_model(client: NativeClient, model: &Value, is_override: bool) -> AppResult<()> {
    if !model.is_object() {
        return Err(invalid("Model declarations must be objects"));
    }
    if !is_override
        && model
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|s| s.is_empty())
    {
        return Err(invalid("Model id is required"));
    }
    optional_string(model, "name")?;
    if !is_override {
        for key in ["id", "api", "baseUrl"] {
            optional_string(model, key)?;
        }
    }
    for key in ["contextWindow", "maxTokens"] {
        if let Some(value) = model.get(key) {
            if !value.as_f64().is_some_and(|v| {
                v.is_finite() && (client == NativeClient::Pi || is_override || v > 0.0)
            }) {
                return Err(invalid("Model capacity is invalid"));
            }
        }
    }
    if model.get("reasoning").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("Model reasoning must be boolean"));
    }
    if let Some(input) = model.get("input") {
        let input = input
            .as_array()
            .ok_or_else(|| invalid("Model input must be an array"))?;
        if input
            .iter()
            .any(|v| !matches!(v.as_str(), Some("text" | "image")))
        {
            return Err(invalid("Unsupported model input type"));
        }
    }
    for key in ["compat"] {
        if model.get(key).is_some_and(|v| !v.is_object()) {
            return Err(invalid("Known model map field must be an object"));
        }
    }
    validate_thinking(client, model)?;
    if let Some(cost) = model.get("cost") {
        validate_cost(cost, is_override, client == NativeClient::Pi)?;
    }
    if client == NativeClient::Pi {
        if model.get("samplingParams").is_some_and(|v| !v.is_object()) {
            return Err(invalid("Pi samplingParams must be an object"));
        }
    } else {
        for key in ["supportsTools", "omitMaxOutputTokens", "preferWebsockets"] {
            optional_bool(model, key)?;
        }
        for key in ["contextPromotionTarget", "compactionModel"] {
            optional_string(model, key)?;
        }
        if let Some(value) = model.get("maxContextWindow") {
            let max = value
                .as_f64()
                .ok_or_else(|| invalid("Invalid maximum context window"))?;
            if max <= 0.0
                || max.fract() != 0.0
                || max > 9_007_199_254_740_991.0
                || model
                    .get("contextWindow")
                    .and_then(Value::as_f64)
                    .is_some_and(|v| max < v)
            {
                return Err(invalid("Maximum context window must be a positive safe integer no smaller than contextWindow"));
            }
        }
    }
    object_strings(model, "headers")
}

/// Validate native shape without restricting APIs to the gateway allowlist or
/// synthesizing capabilities from names. Unknown fields are deliberately kept.
pub(crate) fn validate_provider(client: NativeClient, node: &Value) -> AppResult<()> {
    validate_node_budget(node)?;
    if !node.is_object() {
        return Err(invalid("Provider must be an object"));
    }
    for key in ["api", "apiKey", "baseUrl"] {
        optional_string(node, key)?;
    }
    object_strings(node, "headers")?;
    if node.get("compat").is_some_and(|v| !v.is_object()) {
        return Err(invalid("Provider compat must be an object"));
    }
    if client == NativeClient::Pi {
        optional_string(node, "name")?;
        if node
            .get("oauth")
            .is_some_and(|v| v.as_str() != Some("radius"))
        {
            return Err(invalid("Unsupported Pi OAuth provider declaration"));
        }
    }
    if node.get("authHeader").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("authHeader must be boolean"));
    }
    let mut model_count = 0;
    if let Some(models) = node.get("models") {
        let models = models
            .as_array()
            .ok_or_else(|| invalid("Models must be an array"))?;
        if models.len() > 4096 {
            return Err(invalid("Too many model declarations"));
        }
        model_count = models.len();
        for model in models {
            validate_model(client, model, false)?;
            if client == NativeClient::Omp
                && node.get("api").is_none()
                && model.get("api").is_none()
            {
                return Err(invalid("OMP custom models require provider or model api"));
            }
        }
    }
    if let Some(overrides) = node.get("modelOverrides") {
        let overrides = overrides
            .as_object()
            .ok_or_else(|| invalid("Model overrides must be a map"))?;
        for value in overrides.values() {
            validate_model(client, value, true)?;
        }
    }
    if client == NativeClient::Omp {
        optional_bool(node, "disableStrictTools")?;
        object_strings(node, "requestMetadata")?;
        if let Some(discovery) = node.get("discovery") {
            if !discovery.is_object()
                || !matches!(
                    discovery.get("type").and_then(Value::as_str),
                    Some(
                        "ollama"
                            | "llama.cpp"
                            | "lm-studio"
                            | "openai-models-list"
                            | "proxy"
                            | "litellm"
                            | "apple-foundation-models"
                    )
                )
            {
                return Err(invalid("Invalid OMP discovery type"));
            }
            if node.get("api").is_none() && discovery["type"] != "proxy" {
                return Err(invalid("OMP discovery requires a provider API"));
            }
            optional_bool(discovery, "injectV1")?;
            if discovery.get("injectV1").is_some() && discovery["type"] != "openai-models-list" {
                return Err(invalid(
                    "injectV1 only applies to openai-models-list discovery",
                ));
            }
            if discovery
                .get("timeoutMs")
                .is_some_and(|v| !v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0))
            {
                return Err(invalid(
                    "Discovery timeout must be a positive finite number",
                ));
            }
        }
        if let Some(auth) = node.get("auth") {
            if !matches!(auth.as_str(), Some("apiKey" | "none" | "oauth")) {
                return Err(invalid("Invalid OMP authentication mode"));
            }
        }
        if model_count > 0 {
            if node.get("baseUrl").is_none() {
                return Err(invalid("OMP custom models require baseUrl"));
            }
            if node.get("apiKey").is_none()
                && !matches!(
                    node.get("auth").and_then(Value::as_str),
                    Some("none" | "oauth")
                )
            {
                return Err(invalid(
                    "OMP custom models require apiKey or explicit none/oauth authentication",
                ));
            }
        } else if ![
            "baseUrl",
            "headers",
            "apiKey",
            "compat",
            "guardrailIdentifier",
            "requestMetadata",
            "remoteCompaction",
            "discovery",
        ]
        .iter()
        .any(|key| node.get(key).is_some())
            && node.get("disableStrictTools").and_then(Value::as_bool) != Some(true)
            && node
                .get("modelOverrides")
                .and_then(Value::as_object)
                .is_none_or(|m| m.is_empty())
            && node.get("auth").and_then(Value::as_str) != Some("none")
        {
            return Err(invalid(
                "OMP provider requires an explicit native override or models",
            ));
        }
    }
    Ok(())
}

pub(crate) fn apply_field_patches(node: &Value, patches: &[NativeFieldPatch]) -> AppResult<Value> {
    if patches.len() > 1024 {
        return Err(invalid("Too many field edits"));
    }
    let mut next = node.clone();
    'patches: for patch in patches {
        if patch.path.is_empty()
            || patch.path.len() > 32
            || patch.path.iter().any(|p| p.is_empty() || p.len() > 256)
        {
            return Err(invalid("Invalid field edit path"));
        }
        let mut cursor = &mut next;
        for key in &patch.path[..patch.path.len() - 1] {
            if cursor.is_array() {
                let index = key
                    .parse::<usize>()
                    .map_err(|_| invalid("Invalid array field index"))?;
                cursor = cursor
                    .get_mut(index)
                    .ok_or_else(|| invalid("Array field index is missing"))?;
            } else {
                let map = cursor
                    .as_object_mut()
                    .ok_or_else(|| invalid("Field edit parent must be a map"))?;
                if patch.value.is_none() && !map.contains_key(key) {
                    continue 'patches;
                }
                cursor = map
                    .entry(key.clone())
                    .or_insert_with(|| serde_json::json!({}));
            }
        }
        let key = patch.path.last().expect("validated nonempty path");
        if let Some(array) = cursor.as_array_mut() {
            let index = key
                .parse::<usize>()
                .map_err(|_| invalid("Invalid array field index"))?;
            if index >= array.len() {
                return Err(invalid("Array field index is missing"));
            }
            match &patch.value {
                Some(value) => array[index] = value.clone(),
                None => {
                    array.remove(index);
                }
            }
        } else {
            let map = cursor
                .as_object_mut()
                .ok_or_else(|| invalid("Field edit parent must be a map"))?;
            match &patch.value {
                Some(value) => {
                    map.insert(key.clone(), value.clone());
                }
                None => {
                    map.remove(key);
                }
            }
        }
    }
    Ok(next)
}

fn validate_node_budget(node: &Value) -> AppResult<()> {
    fn walk(value: &Value, depth: usize, count: &mut usize) -> AppResult<()> {
        *count += 1;
        if depth > 64 || *count > 100_000 {
            return Err(invalid("Native node exceeds the complexity limit"));
        }
        match value {
            Value::Object(map) => {
                for child in map.values() {
                    walk(child, depth + 1, count)?;
                }
            }
            Value::Array(array) => {
                for child in array {
                    walk(child, depth + 1, count)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(node, 0, &mut 0)?;
    if serde_json::to_vec(node)
        .map_err(|_| invalid("Cannot encode native node"))?
        .len()
        > 4 * 1024 * 1024
    {
        return Err(invalid("Native node exceeds 4 MiB"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_apis_are_not_limited_to_gateway_protocols() {
        assert!(validate_provider(
            NativeClient::Pi,
            &json!({"api":"extension-api","vendorFuture":{"keep":true}})
        )
        .is_ok());
        assert!(validate_provider(NativeClient::Omp, &json!({"api":"bedrock-converse-stream","auth":"none","baseUrl":"https://example.invalid","models":[{"id":"native"}]})).is_ok());
    }
    #[test]
    fn nested_patch_preserves_unknown_fields() {
        let original = json!({"models":[{"id":"a","future":42}],"headers":{"keep":"literal"}});
        let result = apply_field_patches(
            &original,
            &[NativeFieldPatch {
                path: vec!["models".into(), "0".into(), "name".into()],
                value: Some(json!("edited")),
            }],
        )
        .unwrap();
        assert_eq!(result["models"][0]["future"], 42);
        assert_eq!(result["headers"], original["headers"]);
    }
    #[test]
    fn native_same_id_models_are_not_silently_deduplicated() {
        let node = json!({"models":[{"id":"shared","api":"openai-responses"},{"id":"shared","api":"anthropic-messages"}]});
        assert!(validate_provider(NativeClient::Pi, &node).is_ok());
    }
    #[test]
    fn removing_missing_nested_field_is_a_noop() {
        let node = json!({"future":true});
        assert_eq!(
            apply_field_patches(
                &node,
                &[NativeFieldPatch {
                    path: vec!["missing".into(), "child".into()],
                    value: None
                }]
            )
            .unwrap(),
            node
        );
    }
    #[test]
    fn archive_only_nodes_are_also_bounded() {
        let node = json!({"future":"x".repeat(4*1024*1024)});
        assert!(validate_provider(NativeClient::Pi, &node).is_err());
    }
    #[test]
    fn thinking_vocabularies_are_validated_without_cross_client_rewriting() {
        let pi = json!({"models":[{"id":"custom","thinkingLevelMap":{"off":null,"high":"vendor-high"},"thinking":"unknown-native-extension"}]});
        assert!(validate_provider(NativeClient::Pi, &pi).is_ok());
        let mut invalid_pi = pi.clone();
        invalid_pi["models"][0]["thinkingLevelMap"]["high"] = json!(true);
        assert!(validate_provider(NativeClient::Pi, &invalid_pi).is_err());
        let omp = json!({"api":"google-vertex","auth":"none","baseUrl":"https://example.invalid","models":[{"id":"custom","thinking":{"mode":"budget","minLevel":"low","maxLevel":"high","effortMap":{"high":"vendor-high"}},"thinkingLevelMap":"future-extension"}]});
        assert!(validate_provider(NativeClient::Omp, &omp).is_ok());
        let mut invalid_omp = omp.clone();
        invalid_omp["models"][0]["thinking"] = json!({"mode":"budget"});
        assert!(validate_provider(NativeClient::Omp, &invalid_omp).is_err());
    }
    #[test]
    fn cost_and_capacity_follow_native_definition_and_override_rules() {
        assert!(validate_provider(
            NativeClient::Pi,
            &json!({"models":[{"id":"p","cost":{"input":1}}]})
        )
        .is_err());
        assert!(validate_provider(
            NativeClient::Pi,
            &json!({"modelOverrides":{"p":{"cost":{"input":1}}}})
        )
        .is_ok());
        let base = json!({"api":"extension-api","auth":"none","baseUrl":"https://example.invalid","models":[{"id":"p","contextWindow":200,"maxContextWindow":100}]});
        assert!(validate_provider(NativeClient::Omp, &base).is_err());
        assert!(validate_provider(
            NativeClient::Omp,
            &json!({"modelOverrides":{"p":{"contextWindow":0}}})
        )
        .is_ok());
    }
    #[test]
    fn omp_discovery_and_empty_overrides_follow_runtime_validation() {
        for node in [
            json!({"modelOverrides":{}}),
            json!({"disableStrictTools":false}),
            json!({"discovery":{"type":"ollama"}}),
            json!({"api":"openai-completions","discovery":{"type":"ollama","injectV1":false}}),
        ] {
            assert!(validate_provider(NativeClient::Omp, &node).is_err());
        }
        for node in [
            json!({"headers":{}}),
            json!({"disableStrictTools":true}),
            json!({"discovery":{"type":"proxy"}}),
        ] {
            assert!(validate_provider(NativeClient::Omp, &node).is_ok());
        }
    }
}
