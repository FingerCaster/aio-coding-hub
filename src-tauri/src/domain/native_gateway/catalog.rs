//! Persist explicit capabilities against the provider transport/mapping revision.
use super::import::{validate_explicit_api_key, GatewayImportConfirmInput, GatewayImportPreview};
use super::manifests::ManifestPayload;
use super::{db_error, hash, metadata, GatewayCatalogGroup, NativeModelSpec};
use crate::shared::error::{AppError, AppResult};
use crate::shared::gateway_protocol::GatewayProtocol;
use crate::{db, providers};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSpec {
    binding: String,
    model: NativeModelSpec,
}

fn provider_context(
    conn: &Connection,
    provider_id: i64,
) -> AppResult<(String, GatewayProtocol, String)> {
    let row = conn.query_row(
        "SELECT cli_key,gateway_protocol,auth_mode,api_key_plaintext,source_provider_id,bridge_type,base_urls_json,model_routing_policy_json,claude_models_json,model_mapping_json FROM providers WHERE id=?1",
        [provider_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?)))
        .optional().map_err(db_error)?.ok_or_else(||AppError::new("DB_NOT_FOUND","Provider not found"))?;
    metadata::validate_client(&row.0)?;
    let protocol = row
        .1
        .as_deref()
        .and_then(|p| p.parse().ok())
        .ok_or_else(|| metadata::invalid("Explicit gateway protocol required"))?;
    if row.2 != "api_key" || row.4.is_some() || row.5.is_some() {
        return Err(metadata::invalid(
            "Only direct API-key providers are supported",
        ));
    }
    validate_explicit_api_key(&row.3)?;
    let urls: Vec<String> = serde_json::from_str(&row.6).map_err(db_error)?;
    if urls.is_empty() {
        return Err(metadata::invalid("Explicit base URL required"));
    }
    for url in &urls {
        super::validate_static_base_url(url)?;
    }
    // No credential is serialized, even into a hash input persisted for the UI.
    let binding = hash(
        serde_json::to_vec(&(&row.0, &row.1, &row.6, &row.7, &row.8, &row.9)).map_err(db_error)?,
    );
    Ok((row.0, protocol, binding))
}

fn read_models(
    conn: &Connection,
    provider_id: i64,
    binding: &str,
    client: &str,
) -> AppResult<Vec<NativeModelSpec>> {
    let mut stmt = conn.prepare("SELECT metadata_json FROM native_gateway_model_specs WHERE provider_id=?1 ORDER BY request_model_id").map_err(db_error)?;
    let rows = stmt
        .query_map([provider_id], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    let mut models = Vec::new();
    for row in rows {
        let stored: StoredSpec = serde_json::from_str(&row.map_err(db_error)?).map_err(db_error)?;
        if stored.binding == binding {
            stored.model.validate(client)?;
            models.push(stored.model);
        }
    }
    Ok(models)
}

fn models_snapshot(
    conn: &Connection,
    provider_id: i64,
    provider_uuid: &str,
) -> AppResult<super::GatewayModelsSnapshot> {
    let uuid: String = conn
        .query_row(
            "SELECT provider_uuid FROM providers WHERE id=?1",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if uuid != provider_uuid {
        return Err(AppError::new(
            "NATIVE_GATEWAY_PROVIDER_CONFLICT",
            "Provider identity changed; reload providers",
        ));
    }
    let (client, _, binding) = provider_context(conn, provider_id)?;
    let mut stmt=conn.prepare("SELECT metadata_json FROM native_gateway_model_specs WHERE provider_id=?1 ORDER BY request_model_id").map_err(db_error)?;
    let rows = stmt
        .query_map([provider_id], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    let raw = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
    let revision = hash(serde_json::to_vec(&(&uuid, &binding, &raw)).map_err(db_error)?);
    let mut models = Vec::new();
    let mut stale = false;
    for value in raw {
        let stored: StoredSpec = serde_json::from_str(&value).map_err(db_error)?;
        stored.model.validate(&client)?;
        stale |= stored.binding != binding;
        models.push(stored.model);
    }
    Ok(super::GatewayModelsSnapshot {
        provider_id,
        provider_uuid: uuid,
        revision,
        models,
        stale,
    })
}

pub(crate) fn models_get(
    db: &db::Db,
    provider_id: i64,
    provider_uuid: &str,
) -> AppResult<super::GatewayModelsSnapshot> {
    let mut conn = db.open_connection()?;
    let tx = conn.transaction().map_err(db_error)?;
    let result = models_snapshot(&tx, provider_id, provider_uuid)?;
    tx.commit().map_err(db_error)?;
    Ok(result)
}

fn set_models_tx(conn: &Connection, provider_id: i64, models: &[NativeModelSpec]) -> AppResult<()> {
    let (client, _, binding) = provider_context(conn, provider_id)?;
    if models.len() > metadata::MAX_MODELS {
        return Err(metadata::invalid("Too many model declarations"));
    }
    let mut seen = BTreeSet::new();
    for model in models {
        model.validate(&client)?;
        if !seen.insert(&model.request_model_id) {
            return Err(metadata::invalid("Duplicate model declaration"));
        }
    }
    conn.execute(
        "DELETE FROM native_gateway_model_specs WHERE provider_id=?1",
        [provider_id],
    )
    .map_err(db_error)?;
    for model in models {
        let raw = serde_json::to_string(&StoredSpec {
            binding: binding.clone(),
            model: model.clone(),
        })
        .map_err(db_error)?;
        conn.execute("INSERT INTO native_gateway_model_specs(provider_id,request_model_id,metadata_json,updated_at) VALUES (?1,?2,?3,?4)",params![provider_id,model.request_model_id,raw,crate::shared::time::now_unix_seconds()]).map_err(db_error)?;
    }
    Ok(())
}

pub(crate) fn models_set(
    db: &db::Db,
    provider_id: i64,
    provider_uuid: &str,
    expected_revision: &str,
    models: Vec<NativeModelSpec>,
) -> AppResult<super::GatewayModelsSnapshot> {
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if models_snapshot(&tx, provider_id, provider_uuid)?.revision != expected_revision {
        return Err(AppError::new(
            "NATIVE_GATEWAY_MODELS_CONFLICT",
            "Model declarations or provider configuration changed; reload before saving",
        ));
    }
    set_models_tx(&tx, provider_id, &models)?;
    let result = models_snapshot(&tx, provider_id, provider_uuid)?;
    tx.commit().map_err(db_error)?;
    Ok(result)
}

/// Use the same active-mode provider selection as the request path. Routing remains
/// wholly owned by ModelRoutingPolicy; declarations are keyed by its public input ID.
pub(crate) fn catalog(
    conn: &Connection,
    client: &str,
    origin: Option<&str>,
    global_policy: &str,
) -> AppResult<(String, Vec<GatewayCatalogGroup>)> {
    metadata::validate_client(client)?;
    let selection = providers::list_enabled_for_gateway_using_connection(conn, client)?;
    let mut grouped: BTreeMap<GatewayProtocol, BTreeMap<String, Vec<NativeModelSpec>>> =
        BTreeMap::new();
    let mut inputs = Vec::new();
    for provider in selection.providers {
        let Ok((_, protocol, binding)) = provider_context(conn, provider.id) else {
            continue;
        };
        let models = read_models(conn, provider.id, &binding, client)?;
        inputs.push(serde_json::json!([
            provider.id,
            provider.provider_uuid,
            protocol,
            binding,
            models
        ]));
        for model in models {
            grouped
                .entry(protocol)
                .or_default()
                .entry(model.request_model_id.clone())
                .or_default()
                .push(model);
        }
    }
    let mut groups = Vec::new();
    for (protocol, by_model) in grouped {
        let models = by_model
            .into_values()
            .map(|models| metadata::intersect(client, &models))
            .collect::<AppResult<Vec<_>>>()?;
        if !models.is_empty() {
            groups.push(GatewayCatalogGroup { protocol, models });
        }
    }
    let revision = hash(
        serde_json::to_vec(&(
            client,
            selection.sort_mode_id,
            origin,
            global_policy,
            inputs,
            &groups,
        ))
        .map_err(db_error)?,
    );
    Ok((revision, groups))
}

/// A candidate must cover EVERY still-published snapshot, including interrupted
/// intents. Missing declarations or publication cannot admit cached native sessions.
pub(crate) fn candidate_eligible(
    conn: &Connection,
    provider_id: i64,
    source_cli: &str,
    protocol: GatewayProtocol,
    request_model_id: &str,
    global_policy: &crate::settings::ModelRoutingPolicy,
) -> AppResult<bool> {
    if !matches!(source_cli, "pi" | "omp") {
        return Ok(false);
    }
    let Ok((client, stored_protocol, binding)) = provider_context(conn, provider_id) else {
        return Ok(false);
    };
    if client != source_cli || stored_protocol != protocol {
        return Ok(false);
    }
    let enabled: bool = conn
        .query_row(
            "SELECT enabled<>0 FROM providers WHERE id=?1",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !enabled {
        return Ok(false);
    }
    let Some(candidate) = read_models(conn, provider_id, &binding, &client)?
        .into_iter()
        .find(|m| m.request_model_id == request_model_id)
    else {
        return Ok(false);
    };
    let policy_hash = hash(serde_json::to_vec(global_policy).map_err(db_error)?);
    let (compatible, published) =
        publication_coverage(conn, &candidate, &client, protocol, &policy_hash, None)?;
    Ok(compatible && published)
}

fn publication_coverage(
    conn: &Connection,
    candidate: &NativeModelSpec,
    client: &str,
    protocol: GatewayProtocol,
    current_policy_hash: &str,
    skip_target: Option<&str>,
) -> AppResult<(bool, bool)> {
    let mut stmt = conn
        .prepare("SELECT target_id,node_json FROM native_gateway_manifests WHERE cli_key=?1 AND protocol=?2")
        .map_err(db_error)?;
    let rows = stmt
        .query_map(params![client, protocol.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut published = false;
    for row in rows {
        let (target, raw) = row.map_err(db_error)?;
        if skip_target == Some(target.as_str()) {
            continue;
        }
        let payload: ManifestPayload = serde_json::from_str(&raw).map_err(db_error)?;
        for (entry, policy_hash) in payload
            .desired
            .iter()
            .map(|e| (e, &payload.policy_hash))
            .chain(payload.previous.iter().map(|p| (&p.entry, &p.policy_hash)))
        {
            for snapshot in entry
                .models
                .iter()
                .filter(|m| m.request_model_id == candidate.request_model_id)
            {
                if policy_hash != current_policy_hash {
                    return Ok((false, published));
                }
                snapshot.validate(client)?;
                published = true;
                if !candidate.covers(snapshot) {
                    return Ok((false, published));
                }
            }
        }
    }
    Ok((true, published))
}

/// Replacing this target may intentionally lower its old claims; other still-joined
/// targets must remain serviceable. Never report a publication usable with zero routes.
pub(crate) fn validate_publication(
    conn: &Connection,
    target_id: &str,
    client: &str,
    groups: &[GatewayCatalogGroup],
    global_policy: &str,
) -> AppResult<()> {
    let selection = providers::list_enabled_for_gateway_using_connection(conn, client)?;
    let mut candidates: BTreeMap<(GatewayProtocol, String), Vec<NativeModelSpec>> = BTreeMap::new();
    for provider in selection.providers {
        let Ok((_, protocol, binding)) = provider_context(conn, provider.id) else {
            continue;
        };
        for model in read_models(conn, provider.id, &binding, client)? {
            candidates
                .entry((protocol, model.request_model_id.clone()))
                .or_default()
                .push(model);
        }
    }
    let policy_hash = hash(global_policy);
    for group in groups {
        for published in &group.models {
            let mut available = false;
            if let Some(options) =
                candidates.get(&(group.protocol, published.request_model_id.clone()))
            {
                for candidate in options {
                    if candidate.covers(published)
                        && publication_coverage(
                            conn,
                            candidate,
                            client,
                            group.protocol,
                            &policy_hash,
                            Some(target_id),
                        )?
                        .0
                    {
                        available = true;
                        break;
                    }
                }
            }
            if !available {
                return Err(AppError::new("NATIVE_GATEWAY_NO_COMPATIBLE_CANDIDATE","Other published targets still require incompatible capabilities or model policy; withdraw or update those entries before applying"));
            }
        }
    }
    Ok(())
}

/// Snapshot import creates only disabled direct providers, atomically with all model
/// declarations. Native credentials/expressions never enter this function.
pub(crate) fn import_confirm(
    db: &db::Db,
    client: &str,
    preview: &GatewayImportPreview,
    input: &GatewayImportConfirmInput,
) -> AppResult<Vec<providers::ProviderSummary>> {
    metadata::validate_client(client)?;
    if !preview.can_import
        || input.expected_revision != preview.revision
        || input.target_id != preview.target_id
        || input.native_key != preview.native_key
    {
        return Err(AppError::new(
            "NATIVE_IMPORT_CONFLICT",
            "Reload the supported native snapshot before confirming",
        ));
    }
    let mut keys = BTreeMap::new();
    for credential in &input.credentials {
        validate_explicit_api_key(&credential.api_key)?;
        if keys
            .insert(&credential.group_id, credential.api_key.trim())
            .is_some()
        {
            return Err(metadata::invalid("Duplicate import group credential"));
        }
    }
    if keys.len() != preview.groups.len()
        || preview
            .groups
            .iter()
            .any(|g| !keys.contains_key(&g.group_id))
    {
        return Err(metadata::invalid(
            "Supply one explicit AIO key for every import group",
        ));
    }
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let now = crate::shared::time::now_unix_seconds();
    let mut ids = Vec::new();
    for group in &preview.groups {
        super::validate_static_base_url(&group.base_url)?;
        let uuid = crate::shared::uuid::new_uuid_v4();
        // Deterministic bounded display prefix; a UUID suffix avoids name collisions.
        let label: String = preview.native_key.chars().take(32).collect();
        let name = format!("{label} · {} · {}", group.protocol.as_str(), &uuid[..8]);
        let urls = serde_json::to_string(&[&group.base_url]).map_err(db_error)?;
        tx.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,gateway_protocol,sort_order,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,0,'api_key',?7,(SELECT COALESCE(MAX(sort_order),-1)+1 FROM providers WHERE cli_key=?2),?8,?8)",params![uuid,client,name,group.base_url,urls,keys[&group.group_id],group.protocol.as_str(),now]).map_err(db_error)?;
        let id = tx.last_insert_rowid();
        set_models_tx(&tx, id, &group.models)?;
        ids.push(id);
    }
    tx.commit().map_err(db_error)?;
    ids.into_iter()
        .map(|id| providers::get_by_id(&conn, id))
        .collect()
}

#[cfg(test)]
pub(super) fn test_set_models(
    conn: &Connection,
    id: i64,
    models: &[NativeModelSpec],
) -> AppResult<()> {
    set_models_tx(conn, id, models)
}

#[cfg(test)]
pub(crate) fn seed_native_gateway_for_test(
    db: &db::Db,
    provider_id: i64,
    client: &str,
    protocol: GatewayProtocol,
    request_model_id: &str,
    policy: &crate::settings::ModelRoutingPolicy,
) -> AppResult<()> {
    let mut conn = db.open_connection()?;
    let tx = conn.transaction().map_err(db_error)?;
    let model = NativeModelSpec {
        request_model_id: request_model_id.into(),
        display_name: request_model_id.into(),
        input: vec![metadata::ModelInput::Text, metadata::ModelInput::Image],
        context_window: 200000,
        max_tokens: 32000,
        reasoning: false,
        thinking: None,
        supports_tools: Some(true),
    };
    let (_, _, binding) = provider_context(&tx, provider_id)?;
    let mut models = read_models(&tx, provider_id, &binding, client)?;
    models.retain(|m| m.request_model_id != request_model_id);
    models.push(model.clone());
    set_models_tx(&tx, provider_id, &models)?;
    let entry = super::generate_entries(
        client,
        "http://127.0.0.1:1234",
        &[GatewayCatalogGroup { protocol, models }],
    )?
    .remove(0);
    let manifest = super::Manifest::applied(
        &format!("test-{client}-{provider_id}"),
        client,
        super::ManifestVersion {
            entry,
            catalog_revision: "test".into(),
            generation: 1,
            policy_hash: hash(serde_json::to_vec(policy).map_err(db_error)?),
        },
    );
    super::put_manifest(&tx, &manifest)?;
    tx.commit().map_err(db_error)
}
