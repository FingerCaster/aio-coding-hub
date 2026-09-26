use super::{db_error, error, ChannelBindingSummary, ChannelIdentity, SourceChannel};
use crate::domain::native_gateway::{hash, Manifest};
use crate::infra::native_cli::node_digest;
use crate::shared::error::AppResult;
use crate::shared::gateway_protocol::GatewayProtocol;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

pub(crate) fn binding_identity(
    target: &str,
    source: SourceChannel,
    protocol: GatewayProtocol,
) -> ChannelIdentity {
    ChannelIdentity {
        binding_id: hash(format!(
            "{target}:{}:{}",
            source.as_str(),
            protocol.as_str()
        )),
        source_channel: source,
    }
}

pub(crate) fn list_bindings(conn: &Connection, target: &str) -> AppResult<Vec<Manifest>> {
    let mut stmt = conn.prepare("SELECT binding_id,source_channel,protocol,native_key,manifest_json FROM native_channel_bindings WHERE target_id=?1 ORDER BY source_channel,protocol").map_err(db_error)?;
    let rows = stmt
        .query_map([target], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (id, source, protocol, key, raw) = row.map_err(db_error)?;
        let m: Manifest = serde_json::from_str(&raw).map_err(db_error)?;
        let owner = m
            .channel
            .as_ref()
            .ok_or_else(|| db_error("missing channel identity"))?;
        if m.target_id != target
            || owner.binding_id != id
            || owner.source_channel.as_str() != source
            || m.protocol.as_str() != protocol
            || m.native_key != key
            || !owner.source_channel.protocols().contains(&m.protocol)
            || binding_identity(target, owner.source_channel, m.protocol).binding_id != id
            || !matches!(
                m.state.as_str(),
                "applied" | "intent_apply" | "intent_remove"
            )
            || m.payload
                .desired
                .iter()
                .chain(m.payload.previous.iter().map(|v| &v.entry))
                .any(|e| e.native_key != key || e.protocol != m.protocol)
        {
            return Err(db_error("channel binding identity mismatch"));
        }
        Ok(m)
    })
    .collect()
}

pub(crate) fn put_binding(conn: &Connection, manifest: &Manifest) -> AppResult<()> {
    let owner = manifest
        .channel
        .as_ref()
        .ok_or_else(|| db_error("missing channel identity"))?;
    let raw = serde_json::to_string(manifest).map_err(db_error)?;
    conn.execute("INSERT INTO native_channel_bindings(binding_id,target_id,cli_key,source_channel,protocol,native_key,manifest_json,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(binding_id) DO UPDATE SET manifest_json=excluded.manifest_json,updated_at=excluded.updated_at",params![owner.binding_id,manifest.target_id,manifest.cli_key,owner.source_channel.as_str(),manifest.protocol.as_str(),manifest.native_key,raw,crate::shared::time::now_unix_seconds()]).map_err(db_error)?;
    Ok(())
}

pub(crate) fn load_binding(conn: &Connection, id: &str, consumer: &str) -> AppResult<Manifest> {
    let target: Option<String> = conn
        .query_row(
            "SELECT target_id FROM native_channel_bindings WHERE binding_id=?1 AND cli_key=?2",
            params![id, consumer],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let binding = target
        .map(|target| list_bindings(conn, &target))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|m| {
            m.channel
                .as_ref()
                .is_some_and(|owner| owner.binding_id == id)
        });
    binding
        .filter(|m| m.state == "applied" && m.cli_key == consumer)
        .ok_or_else(|| {
            error(
                "NATIVE_CHANNEL_BINDING_UNAVAILABLE",
                "Channel binding is absent, pending, or withdrawn",
            )
        })
}

pub(crate) fn binding_summaries(
    manifests: &[Manifest],
    nodes: &Map<String, Value>,
    revision: &str,
) -> Vec<ChannelBindingSummary> {
    manifests
        .iter()
        .filter_map(|m| {
            let owner = m.channel.as_ref()?;
            let actual = nodes.get(&m.native_key).map(node_digest);
            let modified = if m.state == "applied" {
                actual.as_deref() != Some(m.node_digest.as_str())
            } else {
                actual != m.payload.desired.as_ref().map(|e| node_digest(&e.node))
                    && actual
                        != m.payload
                            .previous
                            .as_ref()
                            .map(|v| node_digest(&v.entry.node))
            };
            Some(ChannelBindingSummary {
                binding_id: owner.binding_id.clone(),
                source_channel: owner.source_channel,
                protocol: m.protocol,
                native_key: m.native_key.clone(),
                models: m
                    .payload
                    .desired
                    .as_ref()
                    .or_else(|| m.payload.previous.as_ref().map(|v| &v.entry))
                    .map(|e| e.models.clone())
                    .unwrap_or_default(),
                state: m.state.clone(),
                modified,
                stale: m.catalog_revision != revision,
            })
        })
        .collect()
}
