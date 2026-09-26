//! Durable, secret-free ownership and crash-recovery records for generated nodes.
use super::{db_error, GatewayManifestSummary, GeneratedEntry};
use crate::infra::native_cli::node_digest;
use crate::shared::error::{AppError, AppResult};
use crate::shared::gateway_protocol::GatewayProtocol;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestVersion {
    pub entry: GeneratedEntry,
    pub catalog_revision: String,
    pub generation: i64,
    pub policy_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestPayload {
    pub desired: Option<GeneratedEntry>,
    pub previous: Option<ManifestVersion>,
    pub policy_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Manifest {
    #[serde(default)]
    pub channel: Option<crate::domain::native_channels::ChannelIdentity>,
    pub target_id: String,
    pub cli_key: String,
    pub protocol: GatewayProtocol,
    pub native_key: String,
    pub node_digest: String,
    pub catalog_revision: String,
    pub generation: i64,
    pub state: String,
    pub payload: ManifestPayload,
}

impl Manifest {
    #[cfg(test)]
    pub(crate) fn applied(target_id: &str, client: &str, version: ManifestVersion) -> Self {
        Self::applied_owned(target_id, client, version, None)
    }
    pub(crate) fn applied_owned(
        target_id: &str,
        client: &str,
        version: ManifestVersion,
        channel: Option<crate::domain::native_channels::ChannelIdentity>,
    ) -> Self {
        Self {
            channel,
            target_id: target_id.into(),
            cli_key: client.into(),
            protocol: version.entry.protocol,
            native_key: version.entry.native_key.clone(),
            node_digest: node_digest(&version.entry.node),
            catalog_revision: version.catalog_revision,
            generation: version.generation,
            state: "applied".into(),
            payload: ManifestPayload {
                desired: Some(version.entry),
                previous: None,
                policy_hash: version.policy_hash,
            },
        }
    }
    pub(crate) fn version(&self) -> AppResult<ManifestVersion> {
        if self.state != "applied" {
            return Err(AppError::new(
                "NATIVE_GATEWAY_RECOVERY_REQUIRED",
                "Resolve the pending generated-entry operation",
            ));
        }
        Ok(ManifestVersion {
            entry: self
                .payload
                .desired
                .clone()
                .ok_or_else(|| db_error("missing applied entry"))?,
            catalog_revision: self.catalog_revision.clone(),
            generation: self.generation,
            policy_hash: self.payload.policy_hash.clone(),
        })
    }
}

pub(crate) fn managed_native_keys(db: &crate::db::Db, target_id: &str) -> AppResult<Vec<String>> {
    let conn = db.open_connection()?;
    let mut stmt=conn.prepare("SELECT native_key FROM native_gateway_manifests WHERE target_id=?1 UNION SELECT native_key FROM native_channel_bindings WHERE target_id=?1 ORDER BY native_key").map_err(db_error)?;
    let rows = stmt
        .query_map([target_id], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

pub(crate) fn list_manifests(conn: &Connection, target_id: &str) -> AppResult<Vec<Manifest>> {
    let mut stmt=conn.prepare("SELECT cli_key,protocol,native_key,node_digest,node_json,catalog_revision,generation,state FROM native_gateway_manifests WHERE target_id=?1 ORDER BY protocol").map_err(db_error)?;
    let rows = stmt
        .query_map([target_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (cli_key, protocol, native_key, node_digest, raw, catalog_revision, generation, state) =
            row.map_err(db_error)?;
        let payload: ManifestPayload = serde_json::from_str(&raw).map_err(db_error)?;
        let protocol: GatewayProtocol = protocol.parse().map_err(db_error)?;
        if !matches!(state.as_str(), "applied" | "intent_apply" | "intent_remove")
            || payload
                .desired
                .iter()
                .chain(payload.previous.iter().map(|v| &v.entry))
                .any(|e| e.protocol != protocol || e.native_key != native_key)
        {
            return Err(db_error("invalid manifest identity"));
        }
        Ok(Manifest {
            channel: None,
            target_id: target_id.into(),
            cli_key,
            protocol,
            native_key,
            node_digest,
            catalog_revision,
            generation,
            state,
            payload,
        })
    })
    .collect()
}

pub(crate) fn put_manifest(conn: &Connection, manifest: &Manifest) -> AppResult<()> {
    if manifest.channel.is_some() {
        return crate::domain::native_channels::put_binding(conn, manifest);
    }
    let raw = serde_json::to_string(&manifest.payload).map_err(db_error)?;
    conn.execute("INSERT INTO native_gateway_manifests(target_id,cli_key,protocol,native_key,node_digest,node_json,catalog_revision,generation,state,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(target_id,protocol) DO UPDATE SET cli_key=excluded.cli_key,native_key=excluded.native_key,node_digest=excluded.node_digest,node_json=excluded.node_json,catalog_revision=excluded.catalog_revision,generation=excluded.generation,state=excluded.state,updated_at=excluded.updated_at",params![manifest.target_id,manifest.cli_key,manifest.protocol.as_str(),manifest.native_key,manifest.node_digest,raw,manifest.catalog_revision,manifest.generation,manifest.state,crate::shared::time::now_unix_seconds()]).map_err(db_error)?;
    Ok(())
}

pub(super) fn delete_manifest(
    conn: &Connection,
    target_id: &str,
    protocol: GatewayProtocol,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM native_gateway_manifests WHERE target_id=?1 AND protocol=?2",
        params![target_id, protocol.as_str()],
    )
    .map_err(db_error)?;
    Ok(())
}

pub(crate) fn delete_owned_manifest(conn: &Connection, manifest: &Manifest) -> AppResult<()> {
    if let Some(owner) = &manifest.channel {
        conn.execute(
            "DELETE FROM native_channel_bindings WHERE binding_id=?1 AND target_id=?2",
            params![owner.binding_id, manifest.target_id],
        )
        .map_err(db_error)?;
        Ok(())
    } else {
        delete_manifest(conn, &manifest.target_id, manifest.protocol)
    }
}

pub(crate) fn manifest_summaries(
    manifests: &[Manifest],
    nodes: &Map<String, Value>,
    catalog_revision: &str,
) -> Vec<GatewayManifestSummary> {
    manifests
        .iter()
        .map(|m| {
            let digest = nodes.get(&m.native_key).map(node_digest);
            GatewayManifestSummary {
                protocol: m.protocol,
                native_key: m.native_key.clone(),
                generation: m.generation,
                state: m.state.clone(),
                stale: m.catalog_revision != catalog_revision,
                modified: digest.as_deref() != Some(&m.node_digest),
            }
        })
        .collect()
}
