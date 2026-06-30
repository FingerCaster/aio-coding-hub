//! Usage: Internal descriptors for gateway plugin hook metadata.

use super::context::GatewayPluginHookName;
use super::contract::{hook_contract, HookKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HookDescriptor {
    pub(crate) hook_name: GatewayPluginHookName,
    pub(crate) id: &'static str,
    pub(crate) kind: HookKind,
    pub(crate) read_permissions: &'static [&'static str],
    pub(crate) write_permissions: &'static [&'static str],
    pub(crate) mutation_fields: &'static [&'static str],
    pub(crate) timeout_ms: u64,
    pub(crate) default_failure_policy: &'static str,
}

impl HookDescriptor {
    pub(crate) fn allows_read_permission(self, permission: &str) -> bool {
        self.read_permissions.contains(&permission)
    }

    pub(crate) fn allows_write_permission(self, permission: &str) -> bool {
        self.write_permissions.contains(&permission)
    }

    pub(crate) fn allows_mutation_field(self, field: &str) -> bool {
        self.mutation_fields.contains(&field)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HookRegistry;

impl HookRegistry {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn descriptor(self, hook_name: GatewayPluginHookName) -> Option<HookDescriptor> {
        let contract = hook_contract(hook_name.as_str())?;
        Some(HookDescriptor {
            hook_name,
            id: contract.id,
            kind: contract.kind,
            read_permissions: contract.read_permissions,
            write_permissions: contract.write_permissions,
            mutation_fields: contract.mutation_fields,
            timeout_ms: contract.timeout_ms,
            default_failure_policy: contract.default_failure_policy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::plugins::contract::{
        HookContract, HookKind, ACTIVE_HOOKS, DEFAULT_FAILURE_POLICY, DEFAULT_HOOK_TIMEOUT_MS,
        RESERVED_HOOKS,
    };

    fn assert_contracts_have_registry_descriptors(contracts: &[HookContract]) {
        let registry = HookRegistry::new();

        for contract in contracts {
            let hook_name = GatewayPluginHookName::from_str(contract.id).unwrap_or_else(|| {
                panic!("hook contract {} should parse as hook name", contract.id)
            });
            let descriptor = registry
                .descriptor(hook_name)
                .unwrap_or_else(|| panic!("descriptor missing for hook {}", contract.id));

            assert_eq!(descriptor.hook_name, hook_name);
            assert_eq!(descriptor.id, contract.id);
            assert_eq!(descriptor.kind, contract.kind);
            assert_eq!(descriptor.read_permissions, contract.read_permissions);
            assert_eq!(descriptor.write_permissions, contract.write_permissions);
            assert_eq!(descriptor.mutation_fields, contract.mutation_fields);
            assert_eq!(descriptor.timeout_ms, contract.timeout_ms);
            assert_eq!(
                descriptor.default_failure_policy,
                contract.default_failure_policy
            );
        }
    }

    #[test]
    fn registry_descriptors_mirror_every_hook_contract() {
        assert_contracts_have_registry_descriptors(ACTIVE_HOOKS);
        assert_contracts_have_registry_descriptors(RESERVED_HOOKS);
    }

    #[test]
    fn registry_resolves_active_request_hook() {
        let registry = HookRegistry::new();

        let descriptor = registry
            .descriptor(GatewayPluginHookName::RequestAfterBodyRead)
            .expect("active request hook should resolve");

        assert_eq!(
            descriptor.hook_name,
            GatewayPluginHookName::RequestAfterBodyRead
        );
        assert_eq!(descriptor.id, "gateway.request.afterBodyRead");
        assert_eq!(descriptor.kind, HookKind::Request);
        assert_eq!(descriptor.timeout_ms, DEFAULT_HOOK_TIMEOUT_MS);
        assert_eq!(descriptor.default_failure_policy, DEFAULT_FAILURE_POLICY);
        assert!(descriptor.allows_read_permission("request.body.read"));
        assert!(descriptor.allows_write_permission("request.body.write"));
        assert!(descriptor.allows_mutation_field("requestBody"));
        assert!(!descriptor.allows_read_permission("stream.inspect"));
    }

    #[test]
    fn registry_marks_stream_chunk_as_stream_kind() {
        let registry = HookRegistry::new();

        let descriptor = registry
            .descriptor(GatewayPluginHookName::ResponseChunk)
            .expect("stream chunk hook should resolve");

        assert_eq!(descriptor.hook_name, GatewayPluginHookName::ResponseChunk);
        assert_eq!(descriptor.id, "gateway.response.chunk");
        assert_eq!(descriptor.kind, HookKind::Stream);
        assert!(descriptor.allows_read_permission("stream.inspect"));
        assert!(descriptor.allows_write_permission("stream.modify"));
        assert!(descriptor.allows_mutation_field("streamChunk"));
        assert!(!descriptor.allows_mutation_field("responseBody"));
    }
}
