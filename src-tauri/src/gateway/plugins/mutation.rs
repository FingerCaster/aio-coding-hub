//! Usage: Descriptor-driven gateway plugin mutation enforcement.

use super::context::{GatewayHookResult, GatewayPluginHookName};
use super::permissions::GatewayPluginError;
use super::registry::HookDescriptor;

pub(crate) const DEFAULT_PLUGIN_MUTATION_STREAM_BYTES: usize = 64 * 1024;
pub(crate) const DEFAULT_PLUGIN_MUTATION_LOG_BYTES: usize = 64 * 1024;
pub(crate) const DEFAULT_PLUGIN_MUTATION_HEADER_COUNT: usize = 64;
pub(crate) const DEFAULT_PLUGIN_MUTATION_HEADER_VALUE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GatewayPluginMutationBudget {
    pub(crate) body_bytes: usize,
    pub(crate) stream_bytes: usize,
    pub(crate) log_bytes: usize,
    pub(crate) header_count: usize,
    pub(crate) header_value_bytes: usize,
}

impl Default for GatewayPluginMutationBudget {
    fn default() -> Self {
        Self {
            body_bytes: crate::gateway::util::max_request_body_bytes(),
            stream_bytes: DEFAULT_PLUGIN_MUTATION_STREAM_BYTES,
            log_bytes: DEFAULT_PLUGIN_MUTATION_LOG_BYTES,
            header_count: DEFAULT_PLUGIN_MUTATION_HEADER_COUNT,
            header_value_bytes: DEFAULT_PLUGIN_MUTATION_HEADER_VALUE_BYTES,
        }
    }
}

pub(crate) fn enforce_descriptor_permissions(
    descriptor: HookDescriptor,
    permissions: &[String],
    result: &GatewayHookResult,
) -> Result<(), GatewayPluginError> {
    enforce_descriptor_permissions_with_budget(
        descriptor,
        permissions,
        result,
        GatewayPluginMutationBudget::default(),
    )
}

pub(crate) fn enforce_descriptor_permissions_with_budget(
    descriptor: HookDescriptor,
    permissions: &[String],
    result: &GatewayHookResult,
    budget: GatewayPluginMutationBudget,
) -> Result<(), GatewayPluginError> {
    if result.request_body.is_some() {
        require_mutation_field(descriptor, "requestBody", "request body mutation")?;
        require_permission(permissions, "request.body.write")?;
    }
    if result.response_body.is_some() {
        require_mutation_field(descriptor, "responseBody", "response body mutation")?;
        require_permission(permissions, "response.body.write")?;
    }
    if result.stream_chunk.is_some() {
        require_mutation_field(descriptor, "streamChunk", "stream chunk mutation")?;
        require_permission(permissions, "stream.modify")?;
    }
    if result.log_message.is_some() {
        require_mutation_field(descriptor, "logMessage", "log mutation")?;
        require_permission(permissions, "log.redact")?;
    }
    if !result.headers.is_empty() {
        require_header_mutation(descriptor, permissions)?;
    }
    enforce_output_budget(result, budget)?;
    Ok(())
}

fn enforce_output_budget(
    result: &GatewayHookResult,
    budget: GatewayPluginMutationBudget,
) -> Result<(), GatewayPluginError> {
    if result
        .request_body
        .as_ref()
        .is_some_and(|body| body.len() > budget.body_bytes)
        || result
            .response_body
            .as_ref()
            .is_some_and(|body| body.len() > budget.body_bytes)
    {
        return Err(output_too_large(
            "body mutation exceeds plugin output budget",
        ));
    }
    if result
        .stream_chunk
        .as_ref()
        .is_some_and(|chunk| chunk.len() > budget.stream_bytes)
    {
        return Err(output_too_large(
            "stream mutation exceeds plugin output budget",
        ));
    }
    if result
        .log_message
        .as_ref()
        .is_some_and(|message| message.len() > budget.log_bytes)
    {
        return Err(output_too_large(
            "log mutation exceeds plugin output budget",
        ));
    }
    if result.headers.len() > budget.header_count
        || result
            .headers
            .values()
            .any(|value| value.len() > budget.header_value_bytes)
    {
        return Err(output_too_large(
            "header mutation exceeds plugin output budget",
        ));
    }
    Ok(())
}

fn output_too_large(message: &'static str) -> GatewayPluginError {
    GatewayPluginError::new("PLUGIN_OUTPUT_TOO_LARGE", message)
}

fn require_mutation_field(
    descriptor: HookDescriptor,
    field: &'static str,
    operation: &'static str,
) -> Result<(), GatewayPluginError> {
    if descriptor.allows_mutation_field(field) {
        Ok(())
    } else {
        Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "{operation} is not allowed in {}",
                descriptor.hook_name.as_str()
            ),
        ))
    }
}

fn require_header_mutation(
    descriptor: HookDescriptor,
    permissions: &[String],
) -> Result<(), GatewayPluginError> {
    if !descriptor.allows_mutation_field("headers") {
        if descriptor.hook_name == GatewayPluginHookName::ResponseChunk {
            // Pre-descriptor enforcement accepted response header writes on stream chunks
            // even though stream outputs do not apply header patches.
            return require_permission(permissions, "response.header.write");
        }
        return Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "headers cannot be mutated in {}",
                descriptor.hook_name.as_str()
            ),
        ));
    }
    debug_assert!(
        descriptor.hook_name.is_request_hook() || descriptor.hook_name.is_response_hook()
    );

    let Some(permission) = header_write_permission(descriptor) else {
        return Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "headers cannot be mutated in {}",
                descriptor.hook_name.as_str()
            ),
        ));
    };

    require_permission(permissions, permission)
}

fn header_write_permission(descriptor: HookDescriptor) -> Option<&'static str> {
    if descriptor.allows_write_permission("request.header.write") {
        Some("request.header.write")
    } else if descriptor.allows_write_permission("response.header.write") {
        Some("response.header.write")
    } else {
        None
    }
}

fn require_permission(
    permissions: &[String],
    permission: &'static str,
) -> Result<(), GatewayPluginError> {
    if permissions.iter().any(|item| item == permission) {
        Ok(())
    } else {
        Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!("missing plugin permission: {permission}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::plugins::context::GatewayPluginHookName;
    use crate::gateway::plugins::registry::HookRegistry;

    #[test]
    fn request_body_mutation_requires_descriptor_permission() {
        let descriptor = HookRegistry::new()
            .descriptor(GatewayPluginHookName::RequestBeforeSend)
            .expect("request before send descriptor should resolve");
        let result = GatewayHookResult {
            request_body: Some("changed".to_string()),
            ..GatewayHookResult::continue_unchanged()
        };

        let err = enforce_descriptor_permissions(descriptor, &[], &result)
            .expect_err("request body write should require permission");
        assert_eq!(err.code_for_logging(), "PLUGIN_PERMISSION_DENIED");

        enforce_descriptor_permissions(descriptor, &["request.body.write".to_string()], &result)
            .expect("request body write permission should allow mutation");
    }

    #[test]
    fn stream_chunk_header_mutation_preserves_legacy_permission_behavior() {
        let descriptor = HookRegistry::new()
            .descriptor(GatewayPluginHookName::ResponseChunk)
            .expect("response chunk descriptor should resolve");
        let mut result = GatewayHookResult::continue_unchanged();
        result
            .headers
            .insert("x-plugin".to_string(), "1".to_string());

        let err = enforce_descriptor_permissions(descriptor, &[], &result)
            .expect_err("legacy response header write should still require permission");
        assert_eq!(err.code_for_logging(), "PLUGIN_PERMISSION_DENIED");
        assert!(err.to_string().contains("response.header.write"));

        enforce_descriptor_permissions(descriptor, &["response.header.write".to_string()], &result)
            .expect("legacy response header write permission should allow stream hook result");
    }

    #[test]
    fn mutation_budget_rejects_oversized_hook_outputs() {
        let descriptor = HookRegistry::new()
            .descriptor(GatewayPluginHookName::RequestBeforeSend)
            .expect("request before send descriptor should resolve");
        let result = GatewayHookResult {
            request_body: Some("x".repeat(128)),
            ..GatewayHookResult::continue_unchanged()
        };
        let budget = GatewayPluginMutationBudget {
            body_bytes: 16,
            stream_bytes: 16,
            log_bytes: 16,
            header_count: 8,
            header_value_bytes: 64,
        };

        let err = enforce_descriptor_permissions_with_budget(
            descriptor,
            &["request.body.write".to_string()],
            &result,
            budget,
        )
        .expect_err("oversized body mutation should be rejected");

        assert_eq!(err.code_for_logging(), "PLUGIN_OUTPUT_TOO_LARGE");
    }
}
