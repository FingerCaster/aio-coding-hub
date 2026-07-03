//! Bridge type registry.
//!
//! Maps `bridge_type` strings (e.g. `"cx2cc"`) to factory functions that
//! produce fully assembled [`Bridge`] instances.

use super::bridge::Bridge;
use crate::domain::providers::{
    CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE, CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
    CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE, CX2CC_BRIDGE_TYPE,
};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub(crate) type BridgeFactory = fn() -> Bridge;

fn registry() -> &'static RwLock<HashMap<&'static str, BridgeFactory>> {
    static REGISTRY: OnceLock<RwLock<HashMap<&'static str, BridgeFactory>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert(CX2CC_BRIDGE_TYPE, cx2cc_factory as BridgeFactory);
        m.insert(
            CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
            codex_to_openai_chat_factory as BridgeFactory,
        );
        m.insert(
            CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE,
            codex_to_anthropic_messages_factory as BridgeFactory,
        );
        m.insert(
            CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE,
            codex_to_openai_responses_factory as BridgeFactory,
        );
        RwLock::new(m)
    })
}

/// Look up a bridge by type identifier and construct it.
pub(crate) fn get_bridge(bridge_type: &str) -> Option<Bridge> {
    registry().read().ok()?.get(bridge_type).map(|f| f())
}

/// Return the list of all registered bridge type identifiers.
#[allow(dead_code)]
pub(crate) fn available_bridge_types() -> Vec<&'static str> {
    registry()
        .read()
        .ok()
        .map(|r| r.keys().copied().collect())
        .unwrap_or_default()
}

/// Register a new bridge factory at runtime.
/// Returns `true` if inserted, `false` if `bridge_type` was already registered.
#[allow(dead_code)]
pub(crate) fn register_bridge(bridge_type: &'static str, factory: BridgeFactory) -> bool {
    if let Ok(mut map) = registry().write() {
        use std::collections::hash_map::Entry;
        match map.entry(bridge_type) {
            Entry::Vacant(e) => {
                e.insert(factory);
                true
            }
            Entry::Occupied(_) => false,
        }
    } else {
        false
    }
}

// ─── Factory functions ──────────────────────────────────────────────────────

fn cx2cc_factory() -> Bridge {
    Bridge {
        bridge_type: CX2CC_BRIDGE_TYPE,
        inbound: Box::new(super::inbound::anthropic::AnthropicMessagesInbound),
        outbound: Box::new(super::outbound::openai_responses::OpenAIResponsesOutbound),
        model_mapper: Box::new(super::cx2cc::CX2CCModelMapper),
    }
}

fn codex_to_openai_chat_factory() -> Bridge {
    Bridge {
        bridge_type: CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
        inbound: Box::new(super::inbound::openai_responses::OpenAIResponsesInbound),
        outbound: Box::new(super::outbound::openai_chat::OpenAIChatCompletionsOutbound),
        model_mapper: Box::new(ProviderModelMapper),
    }
}

fn codex_to_anthropic_messages_factory() -> Bridge {
    Bridge {
        bridge_type: CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE,
        inbound: Box::new(super::inbound::openai_responses::OpenAIResponsesInbound),
        outbound: Box::new(super::outbound::anthropic_messages::AnthropicMessagesOutbound),
        model_mapper: Box::new(ProviderModelMapper),
    }
}

fn codex_to_openai_responses_factory() -> Bridge {
    Bridge {
        bridge_type: CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE,
        inbound: Box::new(super::inbound::openai_responses::OpenAIResponsesInbound),
        outbound: Box::new(super::outbound::openai_responses::OpenAIResponsesOutbound),
        model_mapper: Box::new(ProviderModelMapper),
    }
}

struct ProviderModelMapper;

impl super::traits::ModelMapper for ProviderModelMapper {
    fn map(&self, source_model: &str, ctx: &super::traits::BridgeContext) -> String {
        ctx.model_mapping.map_model(source_model)
    }
}
