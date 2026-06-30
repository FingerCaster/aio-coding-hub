//! Usage: Rust metadata for the Plugin API v1 gateway contract.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookKind {
    Request,
    Response,
    Stream,
    Log,
}

impl HookKind {
    #[cfg(test)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Stream => "stream",
            Self::Log => "log",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookStatus {
    Active,
    Reserved,
}

impl HookStatus {
    #[cfg(test)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HookContract {
    pub(crate) id: &'static str,
    pub(crate) phase: &'static str,
    pub(crate) kind: HookKind,
    pub(crate) status: HookStatus,
    pub(crate) default_failure_policy: &'static str,
    pub(crate) timeout_ms: u64,
    pub(crate) reserved_header_policy: &'static str,
    pub(crate) read_permissions: &'static [&'static str],
    pub(crate) write_permissions: &'static [&'static str],
    pub(crate) permission_dependencies: &'static [PermissionDependency],
    pub(crate) mutation_fields: &'static [&'static str],
    pub(crate) context_fields: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionDependency {
    pub(crate) permission: &'static str,
    pub(crate) requires: &'static [&'static str],
}

pub(crate) const DEFAULT_HOOK_TIMEOUT_MS: u64 = 150;
pub(crate) const DEFAULT_FAILURE_POLICY: &str = "fail-open";
const RESERVED_HEADER_POLICY: &str = "block-gateway-owned";

pub(crate) const ACTIVE_HOOKS: &[HookContract] = &[
    HookContract {
        id: "gateway.request.afterBodyRead",
        phase: "after request body read and before upstream provider send",
        kind: HookKind::Request,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &[
            "request.meta.read",
            "request.header.read",
            "request.header.readSensitive",
            "request.body.read",
        ],
        write_permissions: &["request.header.write", "request.body.write"],
        permission_dependencies: &[PermissionDependency {
            permission: "request.body.write",
            requires: &["request.body.read"],
        }],
        mutation_fields: &["headers", "requestBody"],
        context_fields: &[
            "traceId",
            "request.cliKey",
            "request.method",
            "request.path",
            "request.query",
            "request.headers",
            "request.body",
            "request.requestedModel",
            "request.normalizedMessages",
        ],
    },
    HookContract {
        id: "gateway.request.beforeSend",
        phase: "after provider resolution and before upstream provider send",
        kind: HookKind::Request,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &[
            "request.meta.read",
            "request.header.read",
            "request.header.readSensitive",
            "request.body.read",
        ],
        write_permissions: &["request.header.write", "request.body.write"],
        permission_dependencies: &[],
        mutation_fields: &["headers", "requestBody"],
        context_fields: &[
            "traceId",
            "request.cliKey",
            "request.method",
            "request.path",
            "request.query",
            "request.headers",
            "request.body",
            "request.requestedModel",
            "request.normalizedMessages",
        ],
    },
    HookContract {
        id: "gateway.response.chunk",
        phase: "for each bounded streaming response chunk",
        kind: HookKind::Stream,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &["stream.inspect"],
        write_permissions: &["stream.modify"],
        permission_dependencies: &[PermissionDependency {
            permission: "stream.modify",
            requires: &["stream.inspect"],
        }],
        mutation_fields: &["streamChunk"],
        context_fields: &["traceId", "stream.sequence", "stream.chunk"],
    },
    HookContract {
        id: "gateway.response.after",
        phase: "after a complete non-streaming upstream response body is available",
        kind: HookKind::Response,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &["response.header.read", "response.body.read"],
        write_permissions: &["response.header.write", "response.body.write"],
        permission_dependencies: &[PermissionDependency {
            permission: "response.body.write",
            requires: &["response.body.read"],
        }],
        mutation_fields: &["headers", "responseBody"],
        context_fields: &[
            "traceId",
            "response.status",
            "response.headers",
            "response.body",
        ],
    },
    HookContract {
        id: "gateway.error",
        phase: "after gateway error response materialization and before it is sent",
        kind: HookKind::Response,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &["response.header.read", "response.body.read"],
        write_permissions: &["response.header.write", "response.body.write"],
        permission_dependencies: &[],
        mutation_fields: &["headers", "responseBody"],
        context_fields: &[
            "traceId",
            "response.status",
            "response.headers",
            "response.body",
        ],
    },
    HookContract {
        id: "log.beforePersist",
        phase: "before gateway request log persistence",
        kind: HookKind::Log,
        status: HookStatus::Active,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &["log.redact"],
        write_permissions: &["log.redact"],
        permission_dependencies: &[],
        mutation_fields: &["logMessage"],
        context_fields: &["traceId", "log.message"],
    },
];

pub(crate) const RESERVED_HOOKS: &[HookContract] = &[
    HookContract {
        id: "gateway.request.received",
        phase: "reserved for a future host integration",
        kind: HookKind::Request,
        status: HookStatus::Reserved,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &[],
        write_permissions: &[],
        permission_dependencies: &[],
        mutation_fields: &[],
        context_fields: &[],
    },
    HookContract {
        id: "gateway.request.beforeProviderResolution",
        phase: "reserved for a future host integration",
        kind: HookKind::Request,
        status: HookStatus::Reserved,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &[],
        write_permissions: &[],
        permission_dependencies: &[],
        mutation_fields: &[],
        context_fields: &[],
    },
    HookContract {
        id: "gateway.response.headers",
        phase: "reserved for a future host integration",
        kind: HookKind::Response,
        status: HookStatus::Reserved,
        default_failure_policy: DEFAULT_FAILURE_POLICY,
        timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
        reserved_header_policy: RESERVED_HEADER_POLICY,
        read_permissions: &[],
        write_permissions: &[],
        permission_dependencies: &[],
        mutation_fields: &[],
        context_fields: &[],
    },
];

#[cfg(test)]
pub(crate) const RESERVED_PERMISSIONS: &[&str] = &[
    "plugin.storage",
    "network.fetch",
    "file.read",
    "file.write",
    "secret.read",
];

pub(crate) fn hook_contract(hook: &str) -> Option<&'static HookContract> {
    ACTIVE_HOOKS
        .iter()
        .chain(RESERVED_HOOKS.iter())
        .find(|contract| contract.id == hook)
}

pub(crate) fn is_active_hook(hook: &str) -> bool {
    ACTIVE_HOOKS.iter().any(|contract| contract.id == hook)
}

pub(crate) fn is_reserved_hook(hook: &str) -> bool {
    RESERVED_HOOKS.iter().any(|contract| contract.id == hook)
}

#[allow(dead_code)]
pub(crate) fn is_known_hook(hook: &str) -> bool {
    hook_contract(hook).is_some()
}

#[cfg(test)]
pub(crate) fn is_reserved_permission(permission: &str) -> bool {
    RESERVED_PERMISSIONS.contains(&permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_api_v1_contract() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../../docs/plugins/plugin-api-v1-contract.json"
        ))
        .expect("plugin API v1 contract JSON should parse")
    }

    fn contract_string_array(contract: &serde_json::Value, key: &str) -> Vec<String> {
        contract[key]
            .as_array()
            .unwrap_or_else(|| panic!("{key} should be an array"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("{key} entries should be strings"))
                    .to_string()
            })
            .collect()
    }

    fn hook_entry_string_array(entry: &serde_json::Value, key: &str) -> Vec<String> {
        entry[key]
            .as_array()
            .unwrap_or_else(|| panic!("hookMatrix.{key} should be an array"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("hookMatrix.{key} entries should be strings"))
                    .to_string()
            })
            .collect()
    }

    fn hook_names(contracts: &[HookContract]) -> Vec<String> {
        contracts
            .iter()
            .map(|contract| contract.id.to_string())
            .collect()
    }

    fn string_slice(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn dependency_pairs(dependencies: &[PermissionDependency]) -> Vec<(String, Vec<String>)> {
        dependencies
            .iter()
            .map(|dependency| {
                (
                    dependency.permission.to_string(),
                    string_slice(dependency.requires),
                )
            })
            .collect()
    }

    fn assert_no_duplicate_values(hook_id: &str, field_name: &str, values: &[&str]) {
        let mut seen = std::collections::HashSet::new();
        for value in values {
            assert!(
                seen.insert(*value),
                "{hook_id} {field_name} contains duplicate value {value}"
            );
        }
    }

    fn contract_dependency_pairs(entry: &serde_json::Value) -> Vec<(String, Vec<String>)> {
        let dependencies = entry["permissionDependencies"]
            .as_object()
            .expect("hookMatrix permissionDependencies should be an object");
        dependencies
            .iter()
            .map(|(permission, requires)| {
                (
                    permission.clone(),
                    requires
                        .as_array()
                        .expect("permission dependency entries should be arrays")
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .expect("permission dependency entries should be strings")
                                .to_string()
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }

    #[test]
    fn hook_contract_arrays_do_not_contain_duplicates() {
        for hook in ACTIVE_HOOKS.iter().chain(RESERVED_HOOKS.iter()) {
            assert_no_duplicate_values(hook.id, "read_permissions", hook.read_permissions);
            assert_no_duplicate_values(hook.id, "write_permissions", hook.write_permissions);
            assert_no_duplicate_values(hook.id, "mutation_fields", hook.mutation_fields);
            assert_no_duplicate_values(hook.id, "context_fields", hook.context_fields);
        }
    }

    #[test]
    fn active_hook_metadata_matches_plugin_api_v1() {
        let contract = plugin_api_v1_contract();
        assert_eq!(
            DEFAULT_HOOK_TIMEOUT_MS,
            contract["defaultHookTimeoutMs"]
                .as_u64()
                .expect("defaultHookTimeoutMs should be a number")
        );
        assert_eq!(
            DEFAULT_FAILURE_POLICY,
            contract["defaultFailurePolicy"]
                .as_str()
                .expect("defaultFailurePolicy should be a string")
        );
        assert_eq!(
            hook_names(ACTIVE_HOOKS),
            contract_string_array(&contract, "activeHooks")
        );

        let matrix = contract["hookMatrix"]
            .as_object()
            .expect("hookMatrix should be an object");
        for hook in ACTIVE_HOOKS {
            let entry = matrix
                .get(hook.id)
                .unwrap_or_else(|| panic!("hookMatrix entry missing for {}", hook.id));
            assert_eq!(hook_contract(hook.id), Some(hook));
            assert!(is_active_hook(hook.id));
            assert!(is_known_hook(hook.id));
            assert!(!is_reserved_hook(hook.id));
            assert_eq!(
                hook.phase,
                entry["phase"]
                    .as_str()
                    .expect("hookMatrix phase should be a string")
            );
            assert_eq!(
                hook.kind.as_str(),
                entry["kind"]
                    .as_str()
                    .expect("hookMatrix kind should be a string")
            );
            assert_eq!(
                hook.status.as_str(),
                entry["status"]
                    .as_str()
                    .expect("hookMatrix status should be a string")
            );
            assert_eq!(
                hook.default_failure_policy,
                entry["defaultFailurePolicy"]
                    .as_str()
                    .expect("hookMatrix defaultFailurePolicy should be a string")
            );
            assert_eq!(
                hook.timeout_ms,
                entry["timeoutMs"]
                    .as_u64()
                    .expect("hookMatrix timeoutMs should be a number")
            );
            assert_eq!(
                hook.reserved_header_policy,
                entry["reservedHeaderPolicy"]
                    .as_str()
                    .expect("hookMatrix reservedHeaderPolicy should be a string")
            );
            assert_eq!(
                string_slice(hook.read_permissions),
                hook_entry_string_array(entry, "readPermissions")
            );
            assert_eq!(
                string_slice(hook.write_permissions),
                hook_entry_string_array(entry, "writePermissions")
            );
            assert_eq!(
                string_slice(hook.mutation_fields),
                hook_entry_string_array(entry, "mutationFields")
            );
            assert_eq!(
                string_slice(hook.context_fields),
                hook_entry_string_array(entry, "contextFields")
            );
        }
    }

    #[test]
    fn reserved_hook_metadata_matches_plugin_api_v1() {
        let contract = plugin_api_v1_contract();
        assert_eq!(
            hook_names(RESERVED_HOOKS),
            contract_string_array(&contract, "reservedHooks")
        );

        for hook in RESERVED_HOOKS {
            assert_eq!(hook_contract(hook.id), Some(hook));
            assert_eq!(hook.status, HookStatus::Reserved);
            assert!(!is_active_hook(hook.id));
            assert!(is_reserved_hook(hook.id));
            assert!(is_known_hook(hook.id));
        }
        assert!(!is_known_hook("gateway.request.missing"));
    }

    #[test]
    fn permission_metadata_marks_reserved_permissions() {
        let contract = plugin_api_v1_contract();
        assert_eq!(
            string_slice(RESERVED_PERMISSIONS),
            contract_string_array(&contract, "reservedPermissions")
        );

        for permission in RESERVED_PERMISSIONS {
            assert!(is_reserved_permission(permission));
        }
        for permission in contract_string_array(&contract, "activePermissions") {
            assert!(!is_reserved_permission(&permission));
        }
        assert!(!is_reserved_permission("unknown.permission"));
    }

    #[test]
    fn permission_dependency_metadata_matches_plugin_api_v1() {
        let contract = plugin_api_v1_contract();
        let matrix = contract["hookMatrix"]
            .as_object()
            .expect("hookMatrix should be an object");

        for hook in ACTIVE_HOOKS {
            let entry = matrix
                .get(hook.id)
                .unwrap_or_else(|| panic!("hookMatrix entry missing for {}", hook.id));
            assert_eq!(
                dependency_pairs(hook.permission_dependencies),
                contract_dependency_pairs(entry)
            );
        }
    }
}
