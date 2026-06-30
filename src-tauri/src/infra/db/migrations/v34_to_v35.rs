//! Usage: SQLite migration v34->v35 - Add plugin-owned provider extension values.

use rusqlite::Connection;

pub(super) fn migrate_v34_to_v35(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start v34->v35: {e}"))?;

    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS provider_extension_values (
  provider_id INTEGER NOT NULL,
  plugin_id TEXT NOT NULL,
  namespace TEXT NOT NULL,
  values_json TEXT NOT NULL DEFAULT '{}',
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(provider_id, plugin_id, namespace),
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE,
  FOREIGN KEY(plugin_id) REFERENCES plugins(plugin_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_provider_extension_values_plugin_namespace
  ON provider_extension_values(plugin_id, namespace);
"#,
    )
    .map_err(|e| format!("failed to migrate v34->v35: {e}"))?;

    super::set_user_version(&tx, 35)?;

    tx.commit()
        .map_err(|e| format!("failed to commit v34->v35: {e}"))?;

    Ok(())
}
