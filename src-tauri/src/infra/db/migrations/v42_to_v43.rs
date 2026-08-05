//! Usage: SQLite migration v42->v43 - Add Codex stream-error retry defaults to provider overrides.

use rusqlite::{params, types::Value, Connection};
use std::collections::HashSet;

fn default_stream_internal_error_policy() -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "retry_keywords": [crate::infra::settings::DEFAULT_CAPACITY_RETRY_KEYWORD],
        "non_retry_keywords": [
            "invalid_request",
            "content_policy",
            "policy",
            "safety",
            "high-risk cyber",
            "not allowed",
            "violat"
        ]
    })
}

fn capacity_http_rule() -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "status_code": 400,
        "body_contains": [crate::infra::settings::DEFAULT_CAPACITY_RETRY_KEYWORD],
        "description": "Codex model capacity"
    })
}

fn is_capacity_retry_intent(rule: &serde_json::Value) -> bool {
    let Some(rule) = rule.as_object() else {
        return false;
    };
    if rule.get("status_code").and_then(serde_json::Value::as_u64) != Some(400) {
        return false;
    }
    match rule.get("body_contains") {
        None => true,
        Some(serde_json::Value::Array(body_contains)) => {
            body_contains.is_empty()
                || body_contains.iter().any(|value| {
                    value.as_str().is_some_and(|value| {
                        value
                            .to_lowercase()
                            .contains(crate::infra::settings::DEFAULT_CAPACITY_RETRY_KEYWORD)
                    })
                })
        }
        Some(_) => false,
    }
}

fn status_only_rule(status_code: u64) -> serde_json::Value {
    serde_json::json!({
        "enabled": true,
        "status_code": status_code,
        "body_contains": [],
        "description": ""
    })
}

fn http_rules_from_wire_default(
    object: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<serde_json::Value>, String> {
    if let Some(http_rules) = object.get("http_rules") {
        return http_rules
            .as_array()
            .cloned()
            .ok_or_else(|| "http_rules must be an array".to_string());
    }

    if let Some(status_codes) = object.remove("status_codes") {
        let statuses = status_codes
            .as_array()
            .ok_or_else(|| "status_codes must be an array".to_string())?;
        let mut seen = HashSet::new();
        let mut rules = Vec::with_capacity(statuses.len());
        for status in statuses {
            let status = status
                .as_u64()
                .filter(|status| (400..=599).contains(status))
                .ok_or_else(|| "status_codes contains an invalid status".to_string())?;
            if seen.insert(status) {
                rules.push(status_only_rule(status));
            }
        }
        return Ok(rules);
    }

    Ok([502, 503, 504].into_iter().map(status_only_rule).collect())
}

fn add_stream_error_defaults(raw: &str) -> Result<Option<String>, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "retry policy must be an object".to_string())?;

    let mut changed = false;
    if !object.contains_key("stream_internal_errors") {
        object.insert(
            "stream_internal_errors".to_string(),
            default_stream_internal_error_policy(),
        );
        changed = true;
    }

    let mut http_rules = http_rules_from_wire_default(object)?;
    if !http_rules.iter().any(is_capacity_retry_intent) && http_rules.len() < 17 {
        http_rules.push(capacity_http_rule());
        changed = true;
    }
    if object
        .get("http_rules")
        .and_then(serde_json::Value::as_array)
        != Some(&http_rules)
    {
        object.insert(
            "http_rules".to_string(),
            serde_json::Value::Array(http_rules),
        );
        changed = true;
    }

    if !changed {
        return Ok(None);
    }
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|error| format!("failed to serialize migrated retry policy: {error}"))
}

pub(super) fn migrate_v42_to_v43(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v42->v43: {error}"))?;

    let has_column: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM pragma_table_info('providers') WHERE name = 'upstream_retry_policy_json'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect provider retry policy column: {error}"))?;

    if has_column != 0 {
        let rows = {
            let mut statement = tx
                .prepare(
                    "SELECT id, upstream_retry_policy_json FROM providers WHERE upstream_retry_policy_json IS NOT NULL AND trim(upstream_retry_policy_json) <> ''",
                )
                .map_err(|error| format!("failed to prepare v42->v43 provider migration: {error}"))?;
            let mapped = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Value>(1)?))
                })
                .map_err(|error| format!("failed to query provider retry policies: {error}"))?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(
                    row.map_err(|error| format!("failed to read provider retry policy: {error}"))?,
                );
            }
            rows
        };

        for (provider_id, raw) in rows {
            let Value::Text(raw) = raw else {
                tracing::warn!(
                    provider_id,
                    "skipping non-text provider retry policy during v42->v43 migration"
                );
                continue;
            };
            let migrated = match add_stream_error_defaults(&raw) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(
                        provider_id,
                        "skipping malformed provider retry policy during v42->v43 migration: {error}"
                    );
                    None
                }
            };
            if let Some(migrated) = migrated {
                tx.execute(
                    "UPDATE providers SET upstream_retry_policy_json = ?1 WHERE id = ?2",
                    params![migrated, provider_id],
                )
                .map_err(|error| format!("failed to migrate provider retry policy: {error}"))?;
            }
        }
    }

    super::set_user_version(&tx, 43)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v42->v43: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::add_stream_error_defaults;

    #[test]
    fn adds_stream_defaults_and_capacity_rule_once() {
        let raw = r#"{"enabled":false,"http_rules":[{"enabled":false,"status_code":503,"body_contains":[],"description":""}]}"#;
        let migrated = add_stream_error_defaults(raw)
            .expect("migration succeeds")
            .expect("migration changes legacy policy");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("valid JSON");
        assert_eq!(value["stream_internal_errors"]["enabled"], true);
        assert_eq!(value["http_rules"].as_array().expect("rules").len(), 2);
        assert!(add_stream_error_defaults(&migrated)
            .expect("idempotent migration")
            .is_none());
    }

    #[test]
    fn disabled_capacity_and_status_only_rules_count_as_user_intent() {
        for rule in [
            serde_json::json!({
                "enabled": false,
                "status_code": 400,
                "body_contains": ["SELECTED MODEL IS AT CAPACITY"],
                "description": ""
            }),
            serde_json::json!({
                "enabled": false,
                "status_code": 400,
                "body_contains": [],
                "description": ""
            }),
            serde_json::json!({
                "enabled": false,
                "status_code": 400,
                "description": ""
            }),
        ] {
            let raw = serde_json::json!({
                "http_rules": [rule],
                "stream_internal_errors": {"enabled": false, "retry_keywords": [], "non_retry_keywords": []}
            })
            .to_string();
            assert!(add_stream_error_defaults(&raw)
                .expect("migration succeeds")
                .is_none());
        }
    }

    #[test]
    fn preserves_legacy_status_codes_when_adding_capacity_rule() {
        let migrated = add_stream_error_defaults(
            r#"{"enabled":true,"status_codes":[429,502,429],"transport_errors":["timeout"]}"#,
        )
        .expect("migration succeeds")
        .expect("migration changes policy");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("valid JSON");
        let statuses: Vec<u64> = value["http_rules"]
            .as_array()
            .expect("rules")
            .iter()
            .map(|rule| rule["status_code"].as_u64().expect("status"))
            .collect();
        assert_eq!(statuses, vec![429, 502, 400]);
        assert!(value.get("status_codes").is_none());
    }

    #[test]
    fn materializes_full_wire_defaults_when_http_rules_are_missing() {
        let migrated = add_stream_error_defaults(
            r#"{"enabled":true,"transport_errors":["timeout"],"max_retries":1}"#,
        )
        .expect("migration succeeds")
        .expect("migration changes policy");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("valid JSON");
        let statuses: Vec<u64> = value["http_rules"]
            .as_array()
            .expect("rules")
            .iter()
            .map(|rule| rule["status_code"].as_u64().expect("status"))
            .collect();
        assert_eq!(statuses, vec![502, 503, 504, 400]);
    }

    #[test]
    fn uses_the_new_seventeenth_slot_for_legacy_sixteen_rule_overrides() {
        let rules = (0..16)
            .map(|index| {
                serde_json::json!({
                    "enabled": index % 2 == 0,
                    "status_code": 401,
                    "body_contains": [format!("legacy-{index}")],
                    "description": "legacy"
                })
            })
            .collect::<Vec<_>>();
        let raw = serde_json::json!({
            "enabled": true,
            "http_rules": rules,
            "stream_internal_errors": {
                "enabled": true,
                "retry_keywords": [],
                "non_retry_keywords": []
            }
        })
        .to_string();

        let migrated = add_stream_error_defaults(&raw)
            .expect("migration succeeds")
            .expect("capacity rule is appended");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("valid JSON");
        let rules = value["http_rules"].as_array().expect("rules");
        assert_eq!(rules.len(), 17);
        assert!(super::is_capacity_retry_intent(
            rules.last().expect("capacity rule")
        ));
    }

    #[test]
    fn full_rule_list_keeps_user_rules_and_still_adds_stream_defaults() {
        let rules = (0..17)
            .map(|index| {
                serde_json::json!({
                    "enabled": true,
                    "status_code": 401,
                    "body_contains": [format!("legacy-{index}")],
                    "description": "legacy"
                })
            })
            .collect::<Vec<_>>();
        let raw = serde_json::json!({
            "enabled": true,
            "http_rules": rules
        })
        .to_string();

        let migrated = add_stream_error_defaults(&raw)
            .expect("migration succeeds")
            .expect("stream defaults are added");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("valid JSON");
        let rules = value["http_rules"].as_array().expect("rules");
        assert_eq!(rules.len(), 17);
        assert!(!rules.iter().any(super::is_capacity_retry_intent));
        assert_eq!(value["stream_internal_errors"]["enabled"], true);
        assert!(add_stream_error_defaults(&migrated)
            .expect("idempotent migration")
            .is_none());
    }
}
