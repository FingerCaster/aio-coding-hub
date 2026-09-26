//! Usage: Wire protocol identity independent of the client that originated a request.

use crate::shared::error::{AppError, AppResult};
use std::{fmt, str::FromStr};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayProtocol {
    AnthropicMessages,
    OpenaiCompletions,
    OpenaiResponses,
    GoogleGenerativeAi,
}

impl GatewayProtocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenaiCompletions => "openai-completions",
            Self::OpenaiResponses => "openai-responses",
            Self::GoogleGenerativeAi => "google-generative-ai",
        }
    }

    pub fn parse(value: &str) -> AppResult<Self> {
        value.parse()
    }
}

impl FromStr for GatewayProtocol {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "anthropic-messages" => Ok(Self::AnthropicMessages),
            "openai-completions" => Ok(Self::OpenaiCompletions),
            "openai-responses" => Ok(Self::OpenaiResponses),
            "google-generative-ai" => Ok(Self::GoogleGenerativeAi),
            _ => Err(AppError::new(
                "SEC_INVALID_INPUT",
                "unsupported gateway protocol",
            )),
        }
    }
}

impl fmt::Display for GatewayProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_protocol_api_values_round_trip_exactly() {
        for (protocol, api) in [
            (GatewayProtocol::AnthropicMessages, "anthropic-messages"),
            (GatewayProtocol::OpenaiCompletions, "openai-completions"),
            (GatewayProtocol::OpenaiResponses, "openai-responses"),
            (GatewayProtocol::GoogleGenerativeAi, "google-generative-ai"),
        ] {
            assert_eq!(protocol.as_str(), api);
            assert_eq!(protocol.to_string(), api);
            assert_eq!(GatewayProtocol::parse(api).unwrap(), protocol);
            let value = serde_json::to_value(protocol).unwrap();
            assert_eq!(value, api);
            assert_eq!(
                serde_json::from_value::<GatewayProtocol>(value).unwrap(),
                protocol
            );
        }
    }

    #[test]
    fn gateway_protocol_rejects_client_names_and_unsupported_native_apis() {
        for invalid in [
            "",
            "pi",
            "omp",
            "codex",
            "claude",
            "grok",
            "gemini",
            "openai-codex-responses",
            "pi-messages",
            "pi-native",
            "OpenaiResponses",
            "openai_responses",
            "openai-responses ",
        ] {
            assert!(GatewayProtocol::parse(invalid).is_err(), "{invalid}");
            assert!(serde_json::from_value::<GatewayProtocol>(serde_json::json!(invalid)).is_err());
        }
    }
}
