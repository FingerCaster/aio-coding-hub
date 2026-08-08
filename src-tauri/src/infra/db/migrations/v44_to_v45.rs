//! SQLite migration v44->v45: provider-scoped model routing policy.

use rusqlite::Connection;

pub(super) fn migrate_v44_to_v45(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v44->v45: {error}"))?;
    let providers_table_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='providers')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect providers table: {error}"))?;
    if !providers_table_exists {
        return Err("v44->v45 requires the providers table".to_string());
    }

    let column_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('providers') WHERE name = 'model_routing_policy_json')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| {
            format!("failed to inspect providers.model_routing_policy_json: {error}")
        })?;
    if !column_exists {
        tx.execute(
            "ALTER TABLE providers ADD COLUMN model_routing_policy_json TEXT DEFAULT NULL",
            [],
        )
        .map_err(|error| format!("failed to add providers.model_routing_policy_json: {error}"))?;
    }

    super::set_user_version(&tx, 45)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v44->v45: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_nullable_policy_column_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("open migration database");
        conn.execute_batch(
            r#"
CREATE TABLE providers(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
INSERT INTO providers(id, name) VALUES (1, 'existing');
PRAGMA user_version = 44;
"#,
        )
        .expect("create v44 fixture");

        migrate_v44_to_v45(&mut conn).expect("migrate v44->v45");
        migrate_v44_to_v45(&mut conn).expect("repeat v44->v45");

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read version");
        assert_eq!(version, 45);
        let policy: Option<String> = conn
            .query_row(
                "SELECT model_routing_policy_json FROM providers WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("read migrated provider");
        assert_eq!(policy, None);
    }
}
