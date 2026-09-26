//! Native archives live in AIO's existing database, never in CLI auth storage.
use super::node_digest;
use crate::domain::native_cli::NativeTarget;
use crate::shared::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub(crate) struct Profile {
    pub profile_uuid: String,
    pub native_key: String,
    pub display_name: String,
    pub node: Value,
    pub node_digest: String,
    pub revision: String,
}

fn db_error() -> AppError {
    AppError::new("NATIVE_ARCHIVE_FAILED", "Native archive transaction failed")
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let text: String = row.get(3)?;
    let node = serde_json::from_str(&text).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(Profile {
        profile_uuid: row.get(0)?,
        native_key: row.get(1)?,
        display_name: row.get(2)?,
        node,
        node_digest: row.get(4)?,
        revision: row.get(5)?,
    })
}

pub(crate) fn list(conn: &Connection, target: &NativeTarget) -> AppResult<Vec<Profile>> {
    let mut statement = conn.prepare("SELECT profile_uuid,native_key,display_name,node_json,node_digest,revision FROM native_cli_provider_profiles WHERE client=?1 AND target_id=?2 ORDER BY native_key").map_err(|_|db_error())?;
    let rows = statement
        .query_map(params![target.client.as_str(), target.target_id], decode)
        .map_err(|_| db_error())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| db_error())
}

pub(crate) fn get(
    conn: &Connection,
    target: &NativeTarget,
    key: &str,
) -> AppResult<Option<Profile>> {
    conn.query_row("SELECT profile_uuid,native_key,display_name,node_json,node_digest,revision FROM native_cli_provider_profiles WHERE client=?1 AND target_id=?2 AND native_key=?3",params![target.client.as_str(),target.target_id,key],decode).optional().map_err(|_|db_error())
}

pub(crate) fn check_revision(current: Option<&Profile>, expected: Option<&str>) -> AppResult<()> {
    if current.map(|p| p.revision.as_str()) != expected {
        return Err(AppError::new(
            "NATIVE_ARCHIVE_CONFLICT",
            "Saved native profile changed; reload the editor",
        ));
    }
    Ok(())
}

pub(crate) fn put(
    conn: &Connection,
    target: &NativeTarget,
    key: &str,
    display_name: &str,
    node: &Value,
    previous: Option<&Profile>,
) -> AppResult<Profile> {
    let digest = node_digest(node);
    let unchanged =
        previous.is_some_and(|p| p.node_digest == digest && p.display_name == display_name);
    let profile_uuid = previous
        .map(|p| p.profile_uuid.clone())
        .unwrap_or_else(crate::shared::uuid::new_uuid_v4);
    let revision = if unchanged {
        previous.expect("unchanged profile").revision.clone()
    } else {
        crate::shared::uuid::new_uuid_v4()
    };
    if !unchanged {
        let node_json = serde_json::to_string(node).map_err(|_| db_error())?;
        let now = crate::shared::time::now_unix_seconds();
        conn.execute("INSERT INTO native_cli_provider_profiles(profile_uuid,client,target_id,native_key,display_name,node_json,node_digest,revision,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9) ON CONFLICT(client,target_id,native_key) DO UPDATE SET display_name=excluded.display_name,node_json=excluded.node_json,node_digest=excluded.node_digest,revision=excluded.revision,updated_at=excluded.updated_at",params![profile_uuid,target.client.as_str(),target.target_id,key,display_name,node_json,digest,revision,now]).map_err(|_|db_error())?;
    }
    Ok(Profile {
        profile_uuid,
        native_key: key.into(),
        display_name: display_name.into(),
        node: node.clone(),
        node_digest: digest,
        revision,
    })
}

pub(crate) fn sync(
    conn: &Connection,
    target: &NativeTarget,
    providers: &serde_json::Map<String, Value>,
    managed: &[String],
) -> AppResult<()> {
    for (key, node) in providers {
        if managed.contains(key) {
            continue;
        }
        let previous = get(conn, target, key)?;
        let name = previous
            .as_ref()
            .map(|p| p.display_name.as_str())
            .unwrap_or(key);
        put(conn, target, key, name, node, previous.as_ref())?;
    }
    Ok(())
}

pub(crate) fn delete(conn: &Connection, target: &NativeTarget, key: &str) -> AppResult<bool> {
    conn.execute("DELETE FROM native_cli_provider_profiles WHERE client=?1 AND target_id=?2 AND native_key=?3",params![target.client.as_str(),target.target_id,key]).map(|n|n != 0).map_err(|_|db_error())
}
