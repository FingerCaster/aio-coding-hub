//! Explicit, client-owned capabilities. No model-name inference or second model mapper.
use crate::shared::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_MODELS: usize = 512;
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum ModelInput {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "client", rename_all = "lowercase", deny_unknown_fields)]
pub enum ThinkingSpec {
    Pi {
        #[serde(rename = "levelMap")]
        level_map: BTreeMap<String, Option<String>>,
    },
    Omp {
        mode: String,
        efforts: Vec<String>,
        #[serde(rename = "defaultLevel")]
        default_level: Option<String>,
        #[serde(rename = "effortMap")]
        effort_map: BTreeMap<String, String>,
        #[serde(rename = "supportsDisplay")]
        supports_display: Option<bool>,
        #[serde(rename = "requiresEffort")]
        requires_effort: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeModelSpec {
    pub request_model_id: String,
    pub display_name: String,
    pub input: Vec<ModelInput>,
    pub context_window: u32,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub thinking: Option<ThinkingSpec>,
    pub supports_tools: Option<bool>,
}

pub(super) fn invalid(message: &str) -> AppError {
    AppError::new("NATIVE_GATEWAY_INVALID_METADATA", message)
}

pub(crate) fn validate_client(client: &str) -> AppResult<()> {
    if !matches!(client, "pi" | "omp") {
        return Err(invalid("only Pi and OMP are supported"));
    }
    Ok(())
}

pub(super) fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

impl NativeModelSpec {
    pub(crate) fn validate(&self, client: &str) -> AppResult<()> {
        validate_client(client)?;
        if !bounded_text(&self.request_model_id, 256)
            || self.request_model_id.trim() != self.request_model_id
            || !bounded_text(&self.display_name, 256)
        {
            return Err(invalid("model ID/name is invalid"));
        }
        if !(1..=10_000_000).contains(&self.context_window)
            || self.max_tokens == 0
            || self.max_tokens > self.context_window
        {
            return Err(invalid("explicit token capacities are required"));
        }
        if self.input.is_empty()
            || self.input.len() > 2
            || self.input.iter().collect::<BTreeSet<_>>().len() != self.input.len()
            || !self.input.contains(&ModelInput::Text)
        {
            return Err(invalid(
                "explicit text/image input capabilities are required",
            ));
        }
        if client == "pi" && self.supports_tools == Some(false) {
            return Err(invalid(
                "Pi cannot faithfully publish a tools-disabled model",
            ));
        }
        if self.reasoning != self.thinking.is_some() {
            return Err(invalid(
                "reasoning requires an explicit client thinking declaration",
            ));
        }
        match &self.thinking {
            None => {}
            Some(ThinkingSpec::Pi { level_map }) if client == "pi" => {
                // Pi otherwise falls back to inferred/default mappings. Every level must be explicit.
                if level_map.len() != LEVELS.len()
                    || LEVELS.iter().any(|key| !level_map.contains_key(*key))
                    || level_map
                        .values()
                        .flatten()
                        .any(|value| !bounded_text(value, 64))
                    || !level_map
                        .iter()
                        .any(|(key, value)| key != "off" && value.is_some())
                {
                    return Err(invalid(
                        "Pi thinking map must explicitly declare every level",
                    ));
                }
            }
            Some(ThinkingSpec::Omp {
                mode,
                efforts,
                default_level,
                effort_map,
                ..
            }) if client == "omp" => {
                if !matches!(
                    mode.as_str(),
                    "effort"
                        | "budget"
                        | "google-level"
                        | "anthropic-adaptive"
                        | "anthropic-budget-effort"
                ) || efforts.is_empty()
                    || efforts.len() > 6
                    || efforts.iter().any(|s| !LEVELS[1..].contains(&s.as_str()))
                    || efforts.iter().collect::<BTreeSet<_>>().len() != efforts.len()
                    || default_level.as_ref().is_some_and(|s| !efforts.contains(s))
                    || effort_map
                        .iter()
                        .any(|(key, value)| !efforts.contains(key) || !bounded_text(value, 64))
                {
                    return Err(invalid("OMP thinking declaration is invalid"));
                }
            }
            _ => return Err(invalid("thinking declaration belongs to another client")),
        }
        Ok(())
    }

    /// Covers a published snapshot without relying on route order or model-name guesses.
    pub(crate) fn covers(&self, published: &Self) -> bool {
        self.request_model_id == published.request_model_id
            && self.context_window >= published.context_window
            && self.max_tokens >= published.max_tokens
            && published.input.iter().all(|kind| self.input.contains(kind))
            && (published.supports_tools != Some(true) || self.supports_tools == Some(true))
            && (published.reasoning || self.supports_non_reasoning())
            && (!published.reasoning
                || (self.reasoning
                    && thinking_covers(self.thinking.as_ref(), published.thinking.as_ref())))
    }

    fn supports_non_reasoning(&self) -> bool {
        match &self.thinking {
            Some(ThinkingSpec::Pi { level_map }) => {
                level_map.get("off").is_some_and(Option::is_some)
            }
            Some(ThinkingSpec::Omp {
                requires_effort, ..
            }) => *requires_effort != Some(true),
            None => true,
        }
    }
}

fn thinking_covers(candidate: Option<&ThinkingSpec>, published: Option<&ThinkingSpec>) -> bool {
    match (candidate, published) {
        (_, None) => true,
        (Some(ThinkingSpec::Pi { level_map: a }), Some(ThinkingSpec::Pi { level_map: b })) => b
            .iter()
            .all(|(key, value)| value.is_none() || a.get(key) == Some(value)),
        (
            Some(ThinkingSpec::Omp {
                mode: am,
                efforts: ae,
                effort_map: av,
                requires_effort: ar,
                supports_display: ad,
                ..
            }),
            Some(ThinkingSpec::Omp {
                mode: bm,
                efforts: be,
                effort_map: bv,
                requires_effort: br,
                supports_display: bd,
                ..
            }),
        ) => {
            am == bm
                && ar == br
                && (*bd != Some(true) || *ad == Some(true))
                && be.iter().all(|e| ae.contains(e) && av.get(e) == bv.get(e))
        }
        _ => false,
    }
}

pub(crate) fn intersect(client: &str, models: &[NativeModelSpec]) -> AppResult<NativeModelSpec> {
    let mut result = models
        .first()
        .cloned()
        .ok_or_else(|| invalid("no compatible model candidates"))?;
    result.validate(client)?;
    for next in &models[1..] {
        next.validate(client)?;
        if result.request_model_id != next.request_model_id {
            return Err(invalid("cannot intersect different model identities"));
        }
        result.context_window = result.context_window.min(next.context_window);
        result.max_tokens = result.max_tokens.min(next.max_tokens);
        result.input.retain(|kind| next.input.contains(kind));
        result.supports_tools = match (result.supports_tools, next.supports_tools) {
            (Some(a), Some(b)) => Some(a && b),
            _ => None,
        };
        if !next.reasoning {
            result.reasoning = false;
            result.thinking = None;
        }
        if let Some(current) = &mut result.thinking {
            match (current, next.thinking.as_ref()) {
                (ThinkingSpec::Pi { level_map: a }, Some(ThinkingSpec::Pi { level_map: b })) => {
                    for (key, value) in a.iter_mut() {
                        if b.get(key) != Some(value) {
                            *value = None;
                        }
                    }
                    if !a.iter().any(|(key, value)| key != "off" && value.is_some()) {
                        result.reasoning = false;
                        result.thinking = None;
                    }
                }
                (
                    ThinkingSpec::Omp {
                        mode: am,
                        efforts: ae,
                        default_level: ad,
                        effort_map: av,
                        supports_display: ads,
                        requires_effort: ar,
                    },
                    Some(ThinkingSpec::Omp {
                        mode: bm,
                        efforts: be,
                        effort_map: bv,
                        supports_display: bds,
                        requires_effort: br,
                        ..
                    }),
                ) if am == bm && ar == br => {
                    ae.retain(|e| be.contains(e) && av.get(e) == bv.get(e));
                    av.retain(|e, _| ae.contains(e));
                    if ad.as_ref().is_some_and(|e| !ae.contains(e)) {
                        *ad = None;
                    }
                    if ads != bds {
                        *ads = Some(false);
                    }
                    if ae.is_empty() {
                        return Err(invalid("no common thinking effort"));
                    }
                }
                _ => return Err(invalid("thinking modes are not comparable")),
            }
        }
    }
    result.validate(client)?;
    if models.iter().any(|candidate| !candidate.covers(&result)) {
        return Err(invalid(
            "no common explicit capabilities cover the publication",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn model() -> NativeModelSpec {
        NativeModelSpec {
            request_model_id: "explicit-model".into(),
            display_name: "Explicit".into(),
            input: vec![ModelInput::Text, ModelInput::Image],
            context_window: 128000,
            max_tokens: 16000,
            reasoning: false,
            thinking: None,
            supports_tools: Some(true),
        }
    }
    #[test]
    fn thinking_display_and_required_effort_cannot_be_silently_weakened() {
        let plain = model();
        let mut candidate = plain.clone();
        candidate.reasoning = true;
        candidate.thinking = Some(ThinkingSpec::Omp {
            mode: "effort".into(),
            efforts: vec!["low".into()],
            default_level: Some("low".into()),
            effort_map: BTreeMap::new(),
            supports_display: Some(false),
            requires_effort: Some(true),
        });
        assert!(!candidate.covers(&plain));
        assert!(intersect("omp", &[candidate.clone(), plain]).is_err());
        let mut displayed = candidate.clone();
        if let Some(ThinkingSpec::Omp {
            supports_display, ..
        }) = &mut displayed.thinking
        {
            *supports_display = Some(true);
        }
        assert!(!candidate.covers(&displayed));
        assert!(displayed.covers(&candidate));
    }
    #[test]
    fn capacities_and_input_intersect_and_stale_candidate_is_rejected() {
        let a = model();
        let mut b = a.clone();
        b.context_window = 64000;
        b.max_tokens = 8000;
        b.input = vec![ModelInput::Text];
        let out = intersect("pi", &[a.clone(), b.clone()]).unwrap();
        assert_eq!(out.context_window, 64000);
        assert_eq!(out.max_tokens, 8000);
        assert_eq!(out.input, vec![ModelInput::Text]);
        assert!(a.covers(&out));
        assert!(b.covers(&out));
        assert!(!b.covers(&a));
    }
    #[test]
    fn missing_capability_and_cross_client_thinking_fail_closed() {
        let mut m = model();
        m.context_window = 0;
        assert!(m.validate("pi").is_err());
        m.context_window = 100000;
        m.reasoning = true;
        assert!(m.validate("pi").is_err());
        m.thinking = Some(ThinkingSpec::Omp {
            mode: "effort".into(),
            efforts: vec!["low".into()],
            default_level: None,
            effort_map: BTreeMap::new(),
            supports_display: None,
            requires_effort: None,
        });
        assert!(m.validate("pi").is_err());
        assert!(m.validate("omp").is_ok());
    }
}
