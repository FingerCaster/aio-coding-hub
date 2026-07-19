//! Usage: SQLite migration v38->v39 - Convert provider retry status codes to HTTP rules.

use rusqlite::{params, types::Value, Connection};
use std::collections::HashSet;

fn convert_retry_policy_json(raw: &str) -> Result<Option<String>, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "retry policy must be an object".to_string())?;

    if object.contains_key("http_rules") {
        return Ok(None);
    }
    let Some(status_codes) = object.get("status_codes") else {
        return Ok(None);
    };
    let status_codes = status_codes
        .as_array()
        .ok_or_else(|| "status_codes must be an array".to_string())?;

    let mut seen = HashSet::new();
    let mut rules = Vec::with_capacity(status_codes.len());
    for status in status_codes {
        let status = status
            .as_u64()
            .filter(|status| (400..=599).contains(status))
            .ok_or_else(|| "status_codes contains an invalid status".to_string())?;
        let status = status as u16;
        if seen.insert(status) {
            rules.push(serde_json::json!({
                "enabled": true,
                "status_code": status,
                "body_contains": [],
                "description": ""
            }));
        }
    }

    object.remove("status_codes");
    object.insert("http_rules".to_string(), serde_json::Value::Array(rules));
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|error| format!("failed to serialize migrated retry policy: {error}"))
}

pub(super) fn migrate_v38_to_v39(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v38->v39: {error}"))?;

    let has_providers_table: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect providers table: {error}"))?;
    if has_providers_table == 0 {
        super::set_user_version(&tx, 39)?;
        tx.commit()
            .map_err(|error| format!("failed to commit v38->v39: {error}"))?;
        return Ok(());
    }

    let has_retry_policy_column: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM pragma_table_info('providers') WHERE name = 'upstream_retry_policy_json'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect providers retry policy column: {error}"))?;
    if has_retry_policy_column == 0 {
        tx.execute_batch(
            "ALTER TABLE providers ADD COLUMN upstream_retry_policy_json TEXT DEFAULT NULL;",
        )
        .map_err(|error| format!("failed to add providers retry policy column: {error}"))?;
    }

    let rows = {
        let mut statement = tx
            .prepare(
                "SELECT id, upstream_retry_policy_json FROM providers WHERE upstream_retry_policy_json IS NOT NULL AND trim(upstream_retry_policy_json) <> ''",
            )
            .map_err(|error| format!("failed to prepare provider retry policy migration: {error}"))?;
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
                "skipping non-text provider retry policy during v38->v39 migration"
            );
            continue;
        };
        let migrated = match convert_retry_policy_json(&raw) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    provider_id,
                    "skipping malformed provider retry policy during v38->v39 migration: {error}"
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

    super::set_user_version(&tx, 39)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v38->v39: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::convert_retry_policy_json;

    #[test]
    fn converts_legacy_status_codes_without_touching_other_fields() {
        let migrated = convert_retry_policy_json(
            r#"{"enabled":false,"status_codes":[429,502,429],"transport_errors":["timeout"],"max_retries":2,"backoff_ms":321,"counts_toward_circuit_breaker":true}"#,
        )
        .expect("convert")
        .expect("changed");
        let value: serde_json::Value = serde_json::from_str(&migrated).expect("parse migrated");
        assert!(value.get("status_codes").is_none());
        assert_eq!(value["http_rules"].as_array().expect("rules").len(), 2);
        assert_eq!(value["http_rules"][0]["status_code"], 429);
        assert_eq!(value["transport_errors"], serde_json::json!(["timeout"]));
        assert_eq!(value["max_retries"], 2);
    }

    #[test]
    fn leaves_new_format_stable_and_rejects_malformed_legacy_statuses() {
        assert!(convert_retry_policy_json(r#"{"http_rules":[]}"#)
            .expect("new format")
            .is_none());
        assert!(convert_retry_policy_json(r#"{"status_codes":["503"]}"#).is_err());
        assert!(convert_retry_policy_json("not-json").is_err());
    }
}
