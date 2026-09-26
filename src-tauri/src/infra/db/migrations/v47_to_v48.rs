//! Consumer-owned channel bindings keep source providers and credentials in place.
use rusqlite::Connection;

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(r#"
CREATE TABLE IF NOT EXISTS native_channel_bindings (
  binding_id TEXT PRIMARY KEY NOT NULL,
  target_id TEXT NOT NULL,
  cli_key TEXT NOT NULL CHECK(cli_key IN ('pi','omp')),
  source_channel TEXT NOT NULL CHECK(source_channel IN ('claude','codex','grok','gemini')),
  protocol TEXT NOT NULL CHECK(protocol IN ('anthropic-messages','openai-completions','openai-responses','google-generative-ai')),
  native_key TEXT NOT NULL,
  manifest_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(target_id, source_channel, protocol),
  UNIQUE(target_id, native_key)
);
CREATE INDEX IF NOT EXISTS idx_native_channel_bindings_target ON native_channel_bindings(target_id);
CREATE TABLE IF NOT EXISTS native_channel_model_specs (
  provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  consumer_cli TEXT NOT NULL CHECK(consumer_cli IN ('pi','omp')),
  protocol TEXT NOT NULL CHECK(protocol IN ('anthropic-messages','openai-completions','openai-responses','google-generative-ai')),
  request_model_id TEXT NOT NULL,
  metadata_json TEXT NOT NULL,
  PRIMARY KEY(provider_id, consumer_cli, protocol, request_model_id)
);
"#).map_err(|error| format!("failed to create native channel binding schema: {error}"))
}

pub(super) fn migrate_v47_to_v48(conn: &mut Connection) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to start v47->v48: {error}"))?;
    ensure_schema(&tx)?;
    super::set_user_version(&tx, 48)?;
    tx.commit()
        .map_err(|error| format!("failed to commit v47->v48: {error}"))
}
