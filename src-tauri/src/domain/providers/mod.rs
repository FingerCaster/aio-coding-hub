//! Usage: Provider configuration persistence and gateway selection helpers.

mod queries;
mod share;
mod types;
mod validation;

pub use types::{
    ClaudeModels, DailyResetMode, ProviderAuthMode, ProviderBaseUrlMode, ProviderExtensionValues,
    ProviderExtensionValuesInput, ProviderSummary, ProviderUpsertParams,
};

#[allow(unused_imports)]
pub(crate) use types::{
    has_bridged_input_semantics, is_supported_bridge_type, ClaudeTerminalLaunchContext,
    GatewayProvidersSelection, ProviderAccountUsageCredentialContext,
    ProviderAccountUsageFetchContext, ProviderForGateway, ProviderOAuthDetails, ProviderRouteRow,
    ProviderTransportContext, CX2CC_BRIDGE_TYPE,
};

pub use queries::{
    default_route_list, default_route_set_order, delete, get_api_key_plaintext, list_by_cli,
    names_by_id, reorder, upsert,
};

pub(crate) use queries::{
    claude_terminal_launch_context, clear_oauth, cli_key_by_id,
    first_disabled_provider_for_gateway, get_account_usage_credential_context,
    get_account_usage_fetch_context, get_by_id, get_enabled_direct_codex_for_gateway_by_identity,
    get_oauth_details, get_source_provider_for_gateway, list_account_usage_gateway_target_contexts,
    list_enabled_for_gateway_using_active_mode, list_oauth_providers_needing_refresh,
    model_routing_policy_override_from_json, model_routing_policy_override_to_json,
    replace_extension_values, resolve_effective_credential, resolve_effective_transport_credential,
    set_enabled, set_oauth_last_error, update_oauth_tokens,
    update_oauth_tokens_if_last_refreshed_matches, upsert_with_provider_uuid,
};

#[cfg(test)]
pub(crate) use queries::list_enabled_for_gateway_in_mode;

pub(crate) use share::{
    export_provider_share_v4, import_provider_share, parse_provider_share, preview_provider_share,
    provider_share_default_filename, serialize_provider_share_v4, ProviderShareCredentialStatus,
    ProviderShareEnvelopeV4, ProviderShareExtensionPreview, ProviderSharePreviewDraft,
    PROVIDER_SHARE_MAX_BYTES,
};

#[cfg(test)]
use types::{claude_models_from_json, normalize_model_slot, MAX_MODEL_NAME_LEN};
#[cfg(test)]
use validation::{
    base_urls_from_row, normalize_base_urls, normalize_reset_time_hms_lossy,
    normalize_reset_time_hms_strict, parse_reset_time_hms, validate_limit_usd, MAX_LIMIT_USD,
    MAX_PROVIDER_BASE_URLS, MAX_PROVIDER_BASE_URL_CHARS, MAX_PROVIDER_NOTE_CHARS,
    MAX_PROVIDER_ORDER_IDS,
};

#[cfg(test)]
mod tests;
