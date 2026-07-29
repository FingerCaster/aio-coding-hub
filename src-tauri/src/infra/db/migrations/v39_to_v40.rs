//! Usage: SQLite migration v39->v40 - Stable provider/model identities and managed profiles.

use rusqlite::{params, Connection};
use std::collections::HashSet;

pub(super) fn migrate_v39_to_v40(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v39->v40: {error}"))?;

    let has_providers_table: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect providers table: {error}"))?;
    if has_providers_table == 0 {
        return Err("v39->v40 requires the providers table".to_string());
    }

    let has_provider_uuid: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM pragma_table_info('providers') WHERE name = 'provider_uuid'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect provider_uuid column: {error}"))?;
    let added_provider_uuid = has_provider_uuid == 0;
    if added_provider_uuid {
        tx.execute_batch("ALTER TABLE providers ADD COLUMN provider_uuid TEXT;")
            .map_err(|error| format!("failed to add provider_uuid column: {error}"))?;
    }

    if added_provider_uuid {
        let provider_ids = {
            let mut statement = tx
                .prepare("SELECT id FROM providers ORDER BY id ASC")
                .map_err(|error| format!("failed to prepare provider UUID backfill: {error}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|error| format!("failed to query provider UUID backfill: {error}"))?;
            let mut provider_ids = Vec::new();
            for row in rows {
                provider_ids.push(row.map_err(|error| {
                    format!("failed to read provider UUID backfill row: {error}")
                })?);
            }
            provider_ids
        };
        for provider_id in provider_ids {
            tx.execute(
                "UPDATE providers SET provider_uuid = ?1 WHERE id = ?2",
                params![crate::shared::uuid::new_uuid_v4(), provider_id],
            )
            .map_err(|error| format!("failed to backfill provider UUID: {error}"))?;
        }
    } else {
        let mut statement = tx
            .prepare("SELECT provider_uuid FROM providers ORDER BY id ASC")
            .map_err(|error| format!("failed to prepare provider UUID validation: {error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .map_err(|error| format!("failed to query provider UUID validation: {error}"))?;
        let mut seen = HashSet::new();
        for row in rows {
            let provider_uuid = row
                .map_err(|_| "failed to validate existing provider UUIDs".to_string())?
                .ok_or_else(|| "existing provider UUID is invalid".to_string())?;
            if !crate::shared::uuid::is_canonical_uuid_v4(&provider_uuid) {
                return Err("existing provider UUID is invalid".to_string());
            }
            if !seen.insert(provider_uuid) {
                return Err("existing provider UUIDs are not unique".to_string());
            }
        }
    }

    tx.execute_batch(
        r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_provider_uuid
  ON providers(provider_uuid);

CREATE TRIGGER IF NOT EXISTS providers_provider_uuid_insert_guard
BEFORE INSERT ON providers
WHEN NEW.provider_uuid IS NULL
  OR length(NEW.provider_uuid) <> 36
  OR lower(NEW.provider_uuid) <> NEW.provider_uuid
  OR substr(NEW.provider_uuid, 9, 1) <> '-'
  OR substr(NEW.provider_uuid, 14, 1) <> '-'
  OR substr(NEW.provider_uuid, 19, 1) <> '-'
  OR substr(NEW.provider_uuid, 24, 1) <> '-'
  OR substr(NEW.provider_uuid, 15, 1) <> '4'
  OR substr(NEW.provider_uuid, 20, 1) NOT IN ('8', '9', 'a', 'b')
  OR length(replace(NEW.provider_uuid, '-', '')) <> 32
  OR replace(NEW.provider_uuid, '-', '') GLOB '*[^0-9a-f]*'
BEGIN
  SELECT RAISE(ABORT, 'provider_uuid must be a canonical UUID');
END;

CREATE TRIGGER IF NOT EXISTS providers_provider_uuid_update_guard
BEFORE UPDATE OF provider_uuid ON providers
WHEN NEW.provider_uuid IS NULL OR NEW.provider_uuid <> OLD.provider_uuid
BEGIN
  SELECT RAISE(ABORT, 'provider_uuid is immutable');
END;

CREATE TABLE IF NOT EXISTS provider_model_catalogs (
  provider_id INTEGER PRIMARY KEY,
  protocol TEXT NOT NULL DEFAULT 'openai_compatible'
    CHECK(protocol = 'openai_compatible'),
  stale INTEGER NOT NULL DEFAULT 1 CHECK(stale IN (0, 1)),
  last_attempt_at INTEGER,
  last_success_at INTEGER,
  last_error_code TEXT,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS provider_models (
  model_uuid TEXT PRIMARY KEY,
  provider_id INTEGER NOT NULL,
  remote_model_id TEXT NOT NULL,
  source TEXT NOT NULL CHECK(source IN ('discovered', 'manual')),
  stale INTEGER NOT NULL DEFAULT 0 CHECK(stale IN (0, 1)),
  last_seen_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(provider_id, remote_model_id),
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_provider_models_provider_id
  ON provider_models(provider_id);

CREATE TABLE IF NOT EXISTS codex_managed_profiles (
  profile_uuid TEXT PRIMARY KEY,
  profile_name TEXT NOT NULL,
  profile_name_key TEXT NOT NULL UNIQUE,
  model_uuid TEXT NOT NULL,
  codex_home_path TEXT NOT NULL,
  content_sha256 TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(model_uuid) REFERENCES provider_models(model_uuid) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_codex_managed_profiles_model_uuid
  ON codex_managed_profiles(model_uuid);
"#,
    )
    .map_err(|error| format!("failed to create provider model schema: {error}"))?;

    super::set_user_version(&tx, 40)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v39->v40: {error}"))?;
    Ok(())
}
