//! Usage: Add Pi/OMP native archives and explicit gateway protocol/model ownership.

use rusqlite::Connection;

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), String> {
    let has_protocol = {
        let mut statement = conn
            .prepare("PRAGMA table_info(providers)")
            .map_err(|error| format!("failed to inspect provider protocol schema: {error}"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| format!("failed to read provider columns: {error}"))?;
        let columns = columns
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode provider columns: {error}"))?;
        columns.iter().any(|name| name == "gateway_protocol")
    };
    if !has_protocol {
        conn.execute_batch("ALTER TABLE providers ADD COLUMN gateway_protocol TEXT CHECK (gateway_protocol IS NULL OR gateway_protocol IN ('anthropic-messages','openai-completions','openai-responses','google-generative-ai'));")
            .map_err(|error| format!("failed to add provider protocol: {error}"))?;
    }
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS native_cli_provider_profiles (
  profile_uuid TEXT PRIMARY KEY NOT NULL,
  client TEXT NOT NULL CHECK (client IN ('pi','omp')),
  target_id TEXT NOT NULL,
  native_key TEXT NOT NULL,
  display_name TEXT NOT NULL,
  node_json TEXT NOT NULL,
  node_digest TEXT NOT NULL,
  revision TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(client, target_id, native_key)
);
CREATE INDEX IF NOT EXISTS idx_native_cli_profiles_target
  ON native_cli_provider_profiles(client, target_id);
CREATE TABLE IF NOT EXISTS native_gateway_model_specs (
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  request_model_id TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(provider_id, request_model_id)
);
CREATE TABLE IF NOT EXISTS native_gateway_manifests (
  target_id TEXT NOT NULL,
  cli_key TEXT NOT NULL CHECK (cli_key IN ('pi','omp')),
  protocol TEXT NOT NULL CHECK (protocol IN ('anthropic-messages','openai-completions','openai-responses','google-generative-ai')),
  native_key TEXT NOT NULL,
  node_digest TEXT NOT NULL,
  node_json TEXT NOT NULL,
  catalog_revision TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  state TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(target_id, protocol),
  UNIQUE(target_id, native_key)
);
CREATE INDEX IF NOT EXISTS idx_native_gateway_manifests_cli_protocol
  ON native_gateway_manifests(cli_key, protocol);
"#,
    ).map_err(|error| format!("failed to create native CLI gateway schema: {error}"))?;
    Ok(())
}

pub(super) fn migrate_v46_to_v47(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v46->v47: {error}"))?;
    ensure_schema(&tx)?;
    super::set_user_version(&tx, 47)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v46->v47: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_providers_and_cascades_only_model_declarations() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON; CREATE TABLE providers (id INTEGER PRIMARY KEY, cli_key TEXT NOT NULL); INSERT INTO providers VALUES (1,'codex'); PRAGMA user_version=46;").unwrap();
        migrate_v46_to_v47(&mut conn).unwrap();
        migrate_v46_to_v47(&mut conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 47);
        let protocol: Option<String> = conn
            .query_row(
                "SELECT gateway_protocol FROM providers WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(protocol, None);
        assert!(conn
            .execute(
                "UPDATE providers SET gateway_protocol='unsupported' WHERE id=1",
                []
            )
            .is_err());
        conn.execute(
            "INSERT INTO native_gateway_model_specs VALUES (1,'test','{}',1)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO native_cli_provider_profiles VALUES ('archive','pi','target','native','Native','{}','digest','rev',1,1)", []).unwrap();
        conn.execute("DELETE FROM providers WHERE id=1", [])
            .unwrap();
        let model_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM native_gateway_model_specs",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let archive_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM native_cli_provider_profiles",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model_count, 0);
        assert_eq!(archive_count, 1);
    }

    #[test]
    fn manifest_target_owns_an_exact_key_and_valid_protocol() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE providers (id INTEGER PRIMARY KEY);")
            .unwrap();
        ensure_schema(&conn).unwrap();
        conn.execute("INSERT INTO native_gateway_manifests VALUES ('target','pi','openai-responses','aio-responses','digest','{}','revision',1,'applied',1)", []).unwrap();
        assert!(conn.execute("INSERT INTO native_gateway_manifests VALUES ('target','pi','anthropic-messages','aio-responses','digest','{}','revision',2,'applied',1)", []).is_err());
        assert!(conn.execute("INSERT INTO native_gateway_manifests VALUES ('different','codex','openai-responses','aio','digest','{}','revision',1,'applied',1)", []).is_err());
    }
}
