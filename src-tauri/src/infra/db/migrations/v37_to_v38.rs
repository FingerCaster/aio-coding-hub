//! Usage: SQLite migration v37->v38 - Move NewAPI account credentials out of extension JSON.

use rusqlite::{params, Connection};

pub(super) fn migrate_v37_to_v38(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v37->v38: {error}"))?;

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS provider_account_usage_credentials (
  provider_id INTEGER PRIMARY KEY,
  newapi_user_id TEXT,
  newapi_access_token_plaintext TEXT,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
"#,
    )
    .map_err(|error| format!("failed to create provider account credentials table: {error}"))?;

    let rows = {
        let mut statement = tx
            .prepare(
                r#"
SELECT provider_id, values_json
FROM provider_extension_values
WHERE plugin_id = ?1 AND namespace = ?2
"#,
            )
            .map_err(|error| format!("failed to prepare account extension migration: {error}"))?;
        let mapped = statement
            .query_map(
                params![
                    crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID,
                    crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE,
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|error| format!("failed to query account extension migration: {error}"))?;
        let mut rows = Vec::new();
        for row in mapped {
            rows.push(
                row.map_err(|error| format!("failed to read account extension row: {error}"))?,
            );
        }
        rows
    };

    let now = crate::shared::time::now_unix_seconds();
    for (provider_id, values_json) in rows {
        let parsed = serde_json::from_str::<serde_json::Value>(&values_json)
            .unwrap_or_else(|_| serde_json::json!({}));
        let migrated_user_id = parsed
            .get("newApiUserId")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| {
                crate::domain::provider_account_usage::normalize_newapi_user_id(value).ok()
            });
        if let Some(user_id) = migrated_user_id {
            tx.execute(
                r#"
INSERT INTO provider_account_usage_credentials(
  provider_id,
  newapi_user_id,
  newapi_access_token_plaintext,
  updated_at
) VALUES (?1, ?2, NULL, ?3)
ON CONFLICT(provider_id) DO UPDATE SET
  newapi_user_id = excluded.newapi_user_id,
  updated_at = excluded.updated_at
"#,
                params![provider_id, user_id, now],
            )
            .map_err(|error| format!("failed to migrate provider account User ID: {error}"))?;
        }

        let sanitized =
            crate::domain::provider_account_usage::sanitize_account_usage_extension_value(&parsed);
        let sanitized_json = serde_json::to_string(&sanitized)
            .map_err(|error| format!("failed to serialize account extension migration: {error}"))?;
        tx.execute(
            r#"
UPDATE provider_extension_values
SET values_json = ?1, updated_at = ?2
WHERE provider_id = ?3 AND plugin_id = ?4 AND namespace = ?5
"#,
            params![
                sanitized_json,
                now,
                provider_id,
                crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID,
                crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE,
            ],
        )
        .map_err(|error| format!("failed to sanitize account extension migration: {error}"))?;
    }

    super::set_user_version(&tx, 38)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v37->v38: {error}"))?;
    Ok(())
}
