use super::*;
use crate::domain::native_gateway::{self, hash, GatewayModelsSnapshot, Manifest, NativeModelSpec};
use crate::providers;
use crate::shared::error::AppResult;
use crate::shared::gateway_protocol::GatewayProtocol;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSpec {
    binding: String,
    model: NativeModelSpec,
}
struct SourceContext {
    source: SourceChannel,
    uuid: String,
    name: String,
    auth_mode: String,
    binding: String,
    blocked: Option<String>,
}

fn source_context(
    conn: &Connection,
    id: i64,
    protocol: GatewayProtocol,
) -> AppResult<SourceContext> {
    // Read only presence, never return/hash a credential in catalog data.
    let row=conn.query_row("SELECT cli_key,provider_uuid,name,auth_mode,oauth_provider_type,CASE WHEN auth_mode='oauth' THEN length(trim(COALESCE(oauth_access_token,'')))>0 ELSE length(trim(api_key_plaintext))>0 END,base_urls_json,source_provider_id,bridge_type,model_routing_policy_json,claude_models_json,model_mapping_json FROM providers WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,bool>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<i64>>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,Option<String>>(9)?,r.get::<_,String>(10)?,r.get::<_,String>(11)?))).optional().map_err(db_error)?.ok_or_else(||error("DB_NOT_FOUND","Source provider no longer exists"))?;
    let source = SourceChannel::ALL
        .into_iter()
        .find(|s| s.as_str() == row.0)
        .ok_or_else(|| {
            error(
                "NATIVE_CHANNEL_SOURCE_UNSUPPORTED",
                "Only Claude, Codex, Grok and Gemini source channels are allowed",
            )
        })?;
    if !source.protocols().contains(&protocol) {
        return Err(error(
            "NATIVE_CHANNEL_PROTOCOL_MISMATCH",
            "Protocol does not belong to this source channel",
        ));
    }
    let urls: Vec<String> = serde_json::from_str(&row.6).map_err(db_error)?;
    let blocked = if row.7.is_some() || row.8.is_some() {
        Some("暂不支持桥接或引用其他供应商的上游".into())
    } else if !row.5 {
        Some("来源供应商尚未配置有效凭证".into())
    } else if row.3 == "oauth" && source == SourceChannel::Gemini {
        // Current AIO stores no authoritative Code Assist license evidence. Do
        // not infer a tier from email/token/project, or launch legacy onboarding.
        Some("需核实 Gemini 企业许可及适配；旧个人免费/Pro/Ultra 通路已于 2026-06-18 迁移至 Antigravity".into())
    } else if row.3 == "oauth" {
        crate::gateway::oauth::registry::resolve_oauth_adapter(
            source.as_str(),
            id,
            row.4.as_deref(),
        )
        .err()
        .map(|_| "来源 OAuth 适配与渠道不匹配".into())
    } else if row.3 != "api_key" {
        Some("来源认证方式尚未支持".into())
    } else if urls.is_empty()
        || urls
            .iter()
            .any(|u| native_gateway::validate_static_base_url(u).is_err())
    {
        Some("来源地址缺失或不适用于直接 API 请求".into())
    } else {
        None
    };
    let binding = hash(
        serde_json::to_vec(&(
            &row.0, &row.1, protocol, &row.3, &row.4, &row.6, &row.7, &row.8, &row.9, &row.10,
            &row.11,
        ))
        .map_err(db_error)?,
    );
    Ok(SourceContext {
        source,
        uuid: row.1,
        name: row.2,
        auth_mode: row.3,
        binding,
        blocked,
    })
}

fn stored_models(
    conn: &Connection,
    id: i64,
    consumer: &str,
    protocol: GatewayProtocol,
) -> AppResult<Vec<String>> {
    let mut stmt=conn.prepare("SELECT metadata_json FROM native_channel_model_specs WHERE provider_id=?1 AND consumer_cli=?2 AND protocol=?3 ORDER BY request_model_id").map_err(db_error)?;
    let rows = stmt
        .query_map(params![id, consumer, protocol.as_str()], |r| {
            r.get::<_, String>(0)
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}
fn current_models(
    conn: &Connection,
    id: i64,
    consumer: &str,
    protocol: GatewayProtocol,
    context: &SourceContext,
) -> AppResult<Vec<NativeModelSpec>> {
    let mut models = Vec::new();
    for raw in stored_models(conn, id, consumer, protocol)? {
        let stored: StoredSpec = serde_json::from_str(&raw).map_err(db_error)?;
        if stored.binding == context.binding {
            stored.model.validate(consumer)?;
            models.push(stored.model);
        }
    }
    Ok(models)
}
pub(crate) fn verify_channel_discovery_source(
    conn: &Connection,
    id: i64,
    uuid: &str,
    protocol: GatewayProtocol,
) -> AppResult<()> {
    let context = source_context(conn, id, protocol)?;
    if context.uuid != uuid {
        return Err(error(
            "NATIVE_GATEWAY_PROVIDER_CONFLICT",
            "Source provider identity changed",
        ));
    }
    if let Some(reason) = context.blocked {
        return Err(error("NATIVE_CHANNEL_SOURCE_BLOCKED", &reason));
    }
    Ok(())
}

pub(crate) fn channel_models_get(
    conn: &Connection,
    id: i64,
    uuid: &str,
    consumer: &str,
    protocol: GatewayProtocol,
) -> AppResult<GatewayModelsSnapshot> {
    native_gateway::validate_native_client(consumer)?;
    let context = source_context(conn, id, protocol)?;
    if context.uuid != uuid {
        return Err(error(
            "NATIVE_GATEWAY_PROVIDER_CONFLICT",
            "Source provider identity changed",
        ));
    }
    let raw = stored_models(conn, id, consumer, protocol)?;
    let revision = hash(
        serde_json::to_vec(&(&context.uuid, &context.binding, consumer, protocol, &raw))
            .map_err(db_error)?,
    );
    let mut models = Vec::new();
    let mut stale = false;
    for value in raw {
        let stored: StoredSpec = serde_json::from_str(&value).map_err(db_error)?;
        stored.model.validate(consumer)?;
        stale |= stored.binding != context.binding;
        models.push(stored.model);
    }
    Ok(GatewayModelsSnapshot {
        provider_id: id,
        provider_uuid: uuid.into(),
        revision,
        models,
        stale,
    })
}
pub(crate) fn channel_models_set(
    conn: &Connection,
    id: i64,
    uuid: &str,
    consumer: &str,
    protocol: GatewayProtocol,
    expected: &str,
    models: &[NativeModelSpec],
) -> AppResult<GatewayModelsSnapshot> {
    if channel_models_get(conn, id, uuid, consumer, protocol)?.revision != expected {
        return Err(error(
            "NATIVE_GATEWAY_MODELS_CONFLICT",
            "Source or capabilities changed; reload before saving",
        ));
    }
    let context = source_context(conn, id, protocol)?;
    if let Some(reason) = context.blocked {
        return Err(error("NATIVE_CHANNEL_SOURCE_BLOCKED", &reason));
    }
    if models.len() > 512 {
        return Err(error("NATIVE_CHANNEL_INVALID_SELECTION", "Too many models"));
    }
    let mut seen = BTreeSet::new();
    for model in models {
        model.validate(consumer)?;
        if !seen.insert(&model.request_model_id) || model.supports_tools.is_none() {
            return Err(error(
                "NATIVE_CHANNEL_INVALID_SELECTION",
                "Models must be unique and tool capability explicit",
            ));
        }
    }
    conn.execute("DELETE FROM native_channel_model_specs WHERE provider_id=?1 AND consumer_cli=?2 AND protocol=?3",params![id,consumer,protocol.as_str()]).map_err(db_error)?;
    for model in models {
        let raw = serde_json::to_string(&StoredSpec {
            binding: context.binding.clone(),
            model: model.clone(),
        })
        .map_err(db_error)?;
        conn.execute("INSERT INTO native_channel_model_specs(provider_id,consumer_cli,protocol,request_model_id,metadata_json) VALUES (?1,?2,?3,?4,?5)",params![id,consumer,protocol.as_str(),model.request_model_id,raw]).map_err(db_error)?;
    }
    channel_models_get(conn, id, uuid, consumer, protocol)
}

pub(crate) fn channel_catalog(
    conn: &Connection,
    consumer: &str,
    origin: Option<&str>,
    policy: &str,
) -> AppResult<(String, Vec<ChannelSource>)> {
    native_gateway::validate_native_client(consumer)?;
    let mut sources = Vec::new();
    let mut revisions = Vec::new();
    for source in SourceChannel::ALL {
        let selection =
            providers::list_enabled_for_gateway_using_connection(conn, source.as_str())?;
        for &protocol in source.protocols() {
            let mut providers = Vec::new();
            let mut candidates: BTreeMap<String, Vec<NativeModelSpec>> = BTreeMap::new();
            for provider in &selection.providers {
                let context = source_context(conn, provider.id, protocol)?;
                let models = current_models(conn, provider.id, consumer, protocol, &context)?;
                revisions.push(serde_json::json!([
                    source,
                    protocol,
                    selection.sort_mode_id,
                    provider.id,
                    context.uuid,
                    context.binding,
                    context.blocked,
                    models
                ]));
                let mut ids: Vec<String> =
                    models.iter().map(|m| m.request_model_id.clone()).collect();
                let mut stmt=conn.prepare("SELECT remote_model_id FROM provider_models WHERE provider_id=?1 ORDER BY remote_model_id").map_err(db_error)?;
                for id in stmt
                    .query_map([provider.id], |r| r.get::<_, String>(0))
                    .map_err(db_error)?
                {
                    let id = id.map_err(db_error)?;
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                if context.blocked.is_none() {
                    for model in models {
                        candidates
                            .entry(model.request_model_id.clone())
                            .or_default()
                            .push(model);
                    }
                }
                providers.push(ChannelProvider {
                    provider_id: provider.id,
                    provider_uuid: context.uuid,
                    name: context.name,
                    auth_mode: context.auth_mode,
                    blocked_reason: context.blocked,
                    model_ids: ids,
                });
            }
            let mut models = Vec::new();
            for group in candidates.into_values() {
                if let Ok(model) = native_gateway::intersect_models(consumer, &group) {
                    models.push(model);
                }
            }
            let blocked_reason = if providers.is_empty() {
                Some("来源渠道当前工作模式下没有启用的上游".into())
            } else if providers.iter().all(|p| p.blocked_reason.is_some()) {
                Some("当前上游认证或适配不可用，请查看供应商原因".into())
            } else if models.is_empty() {
                Some("请先为来源上游补齐显式模型能力".into())
            } else {
                None
            };
            sources.push(ChannelSource {
                source_channel: source,
                protocol,
                providers,
                models,
                blocked_reason,
            });
        }
    }
    let revision =
        hash(serde_json::to_vec(&(consumer, origin, policy, revisions)).map_err(db_error)?);
    Ok((revision, sources))
}

pub(crate) struct ChannelPlan {
    pub revision: String,
    pub desired: Vec<(
        ChannelIdentity,
        crate::domain::native_gateway::GeneratedEntry,
    )>,
    pub previous: Vec<Manifest>,
}
pub(crate) fn channel_plan(
    conn: &Connection,
    consumer: &str,
    input: &ChannelLifecycleInput,
    origin: Option<&str>,
    policy: &str,
) -> AppResult<ChannelPlan> {
    if input.selections.len() + input.remove_binding_ids.len() > 10
        || input.selections.is_empty() && input.remove_binding_ids.is_empty()
    {
        return Err(error(
            "NATIVE_CHANNEL_INVALID_SELECTION",
            "Select channels or owned bindings to withdraw",
        ));
    }
    let previous = list_bindings(conn, &input.target_id)?;
    let (revision, sources) = if input.selections.is_empty() {
        (input.catalog_revision.clone(), Vec::new())
    } else {
        channel_catalog(conn, consumer, origin, policy)?
    };
    if revision != input.catalog_revision {
        return Err(error(
            "NATIVE_GATEWAY_CATALOG_CONFLICT",
            "Source channels changed; preview again",
        ));
    }
    let mut keys = BTreeSet::new();
    let mut desired = Vec::new();
    for selected in &input.selections {
        if !keys.insert((selected.source_channel, selected.protocol)) {
            return Err(error(
                "NATIVE_CHANNEL_INVALID_SELECTION",
                "Duplicate channel selection",
            ));
        }
        let source = sources
            .iter()
            .find(|s| {
                s.source_channel == selected.source_channel && s.protocol == selected.protocol
            })
            .ok_or_else(|| {
                error(
                    "NATIVE_CHANNEL_SOURCE_UNSUPPORTED",
                    "Invalid source protocol",
                )
            })?;
        let group = selected_group(source, &selected.model_ids)?;
        let origin = origin.ok_or_else(|| {
            error(
                "NATIVE_GATEWAY_NOT_READY",
                "Start AIO listener before publishing",
            )
        })?;
        let identity =
            binding_identity(&input.target_id, selected.source_channel, selected.protocol);
        let mut entry = native_gateway::generate_entries(consumer, origin, &[group])?.remove(0);
        entry.native_key = format!(
            "aio-channel-{}-{}",
            selected.source_channel.as_str(),
            selected.protocol.as_str()
        );
        entry.base_url = entry.base_url.replace(
            &format!("/_protocol/{}", selected.protocol.as_str()),
            &format!("/_aio/channel/{}", identity.binding_id),
        );
        entry.node["baseUrl"] = serde_json::json!(entry.base_url);
        entry.node["name"] =
            serde_json::json!(format!("AIO · {}", selected.source_channel.label()));
        desired.push((identity, entry));
    }
    let mut removals = BTreeSet::new();
    for id in &input.remove_binding_ids {
        let identity = SourceChannel::ALL
            .into_iter()
            .flat_map(|source| {
                source
                    .protocols()
                    .iter()
                    .map(move |&protocol| (source, protocol))
            })
            .find(|&(source, protocol)| {
                binding_identity(&input.target_id, source, protocol).binding_id == *id
            })
            .ok_or_else(|| {
                error(
                    "NATIVE_CHANNEL_BINDING_UNAVAILABLE",
                    "Only this target's owned bindings can be withdrawn",
                )
            })?;
        if !removals.insert(id) || keys.contains(&identity) {
            return Err(error(
                "NATIVE_CHANNEL_INVALID_SELECTION",
                "Conflicting or duplicate binding actions",
            ));
        }
    }
    let previous = previous
        .into_iter()
        .filter(|m| {
            m.channel.as_ref().is_some_and(|o| {
                keys.contains(&(o.source_channel, m.protocol)) || removals.contains(&o.binding_id)
            })
        })
        .collect();
    Ok(ChannelPlan {
        revision,
        desired,
        previous,
    })
}

pub(crate) fn channel_candidate_eligible(
    conn: &Connection,
    id: &str,
    consumer: &str,
    provider_id: i64,
    model: &str,
    policy: &crate::settings::ModelRoutingPolicy,
) -> AppResult<bool> {
    let Ok(manifest) = load_binding(conn, id, consumer) else {
        return Ok(false);
    };
    let owner = manifest.channel.as_ref().expect("validated identity");
    let Some(published) = manifest
        .payload
        .desired
        .as_ref()
        .and_then(|e| e.models.iter().find(|m| m.request_model_id == model))
    else {
        return Ok(false);
    };
    if manifest.payload.policy_hash != hash(serde_json::to_vec(policy).map_err(db_error)?) {
        return Ok(false);
    }
    let context = match source_context(conn, provider_id, manifest.protocol) {
        Ok(context) => context,
        Err(error)
            if matches!(
                error.code(),
                "DB_NOT_FOUND"
                    | "NATIVE_CHANNEL_SOURCE_UNSUPPORTED"
                    | "NATIVE_CHANNEL_PROTOCOL_MISMATCH"
            ) =>
        {
            return Ok(false)
        }
        Err(error) => return Err(error),
    };
    if context.source != owner.source_channel || context.blocked.is_some() {
        return Ok(false);
    }
    let selection =
        providers::list_enabled_for_gateway_using_connection(conn, owner.source_channel.as_str())?;
    if !selection.providers.iter().any(|p| p.id == provider_id) {
        return Ok(false);
    }
    Ok(
        current_models(conn, provider_id, consumer, manifest.protocol, &context)?
            .iter()
            .any(|candidate| candidate.request_model_id == model && candidate.covers(published)),
    )
}
