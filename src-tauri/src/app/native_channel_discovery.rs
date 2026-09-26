//! Read-only native model suggestions. No credential or catalog writes.
use super::provider_model_discovery::{
    self, ModelCapabilitySuggestion, ProviderModelDiscoveryInput, ProviderModelDiscoveryResult,
};
use crate::app_state::{ensure_db_ready, DbInitState};
use crate::domain::native_channels::{channel_models_get, verify_channel_discovery_source};
use crate::domain::native_gateway::hash;
use crate::shared::error::{AppError, AppResult};
use crate::shared::gateway_protocol::GatewayProtocol;
use crate::{blocking, providers};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChannelModelDiscovery {
    pub target_id: String,
    pub provider_id: i64,
    pub provider_uuid: String,
    pub protocol: GatewayProtocol,
    pub revision: String,
    pub models: Vec<ModelCapabilitySuggestion>,
    pub discovery: ProviderModelDiscoveryResult,
}

struct DiscoverySnapshot {
    revision: String,
    fingerprint: String,
    input: ProviderModelDiscoveryInput,
    configured: Vec<ModelCapabilitySuggestion>,
    routing: Option<crate::settings::ModelRoutingPolicy>,
}

fn configured_capabilities(
    conn: &Connection,
    provider_id: i64,
) -> AppResult<Vec<ModelCapabilitySuggestion>> {
    let mut statement = conn.prepare("SELECT remote_model_id, supported_reasoning_efforts_json, default_reasoning_effort, context_window FROM provider_models m WHERE provider_id=?1 AND capabilities_configured=1 AND stale=0 AND (source='manual' OR EXISTS(SELECT 1 FROM provider_model_catalogs c WHERE c.provider_id=m.provider_id AND c.stale=0)) ORDER BY remote_model_id").map_err(|e| crate::shared::error::db_err!("read model capabilities: {e}"))?;
    let rows = statement
        .query_map([provider_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })
        .map_err(|e| crate::shared::error::db_err!("read model capabilities: {e}"))?;
    let mut configured = rows
        .map(|row| {
            let (id, efforts, default, context) =
                row.map_err(|e| crate::shared::error::db_err!("read model capabilities: {e}"))?;
            let capabilities = crate::domain::provider_models::decode_stored_capabilities(
                true,
                &efforts,
                default.as_deref(),
                context,
            )?;
            let efforts: Vec<String> = capabilities
                .supported_reasoning_efforts
                .iter()
                .map(|v| v.as_str().to_owned())
                .collect();
            // Configured capacities do not assert tool or vision support.
            Ok(ModelCapabilitySuggestion {
                model_id: id,
                context_window: capabilities.context_window.and_then(|v| v.try_into().ok()),
                reasoning: (!efforts.is_empty()).then(|| {
                    efforts
                        .iter()
                        .any(|v| !matches!(v.as_str(), "none" | "off"))
                }),
                reasoning_efforts: (!efforts.is_empty()).then_some(efforts),
                default_reasoning_effort: capabilities
                    .default_reasoning_effort
                    .map(|v| v.as_str().to_owned()),
                sources: vec!["configured".into()],
                ..Default::default()
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    // Retain non-stale saved candidate IDs even if the upstream has no /models API.
    let mut candidates = conn.prepare("SELECT remote_model_id FROM provider_models WHERE provider_id=?1 AND stale=0 ORDER BY remote_model_id").map_err(|e| crate::shared::error::db_err!("read candidate IDs: {e}"))?;
    for id in candidates
        .query_map([provider_id], |row| row.get::<_, String>(0))
        .map_err(|e| crate::shared::error::db_err!("read candidate IDs: {e}"))?
    {
        let id = id.map_err(|e| crate::shared::error::db_err!("read candidate ID: {e}"))?;
        if !configured.iter().any(|m| m.model_id == id) {
            configured.push(ModelCapabilitySuggestion {
                model_id: id,
                sources: vec!["configured_candidate".into()],
                ..Default::default()
            });
        }
    }
    Ok(configured)
}

fn read_snapshot(
    conn: &Connection,
    id: i64,
    uuid: &str,
    consumer: &str,
    protocol: GatewayProtocol,
) -> AppResult<DiscoverySnapshot> {
    verify_channel_discovery_source(conn, id, uuid, protocol)?;
    let models = channel_models_get(conn, id, uuid, consumer, protocol)?;
    let provider = providers::get_by_id(conn, id)?;
    // URL and API key must come from one SQLite read snapshot. Never re-load
    // a rotated key after selecting the previous URL. OAuth uses fixed adapter URLs.
    let (key, oauth, account, mapping): (String, Option<String>, Option<String>, String) = conn.query_row("SELECT api_key_plaintext, oauth_access_token, oauth_id_token, model_mapping_json FROM providers WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(|e| crate::shared::error::db_err!("read discovery connection: {e}"))?;
    let configured = configured_capabilities(conn, id)?;
    let fingerprint = hash(
        serde_json::to_vec(&(
            &models.revision,
            &configured,
            &key,
            &oauth,
            &account,
            provider.base_url_mode.as_str(),
            &mapping,
        ))
        .map_err(|_| {
            AppError::new(
                "NATIVE_CHANNEL_INVALID_SELECTION",
                "Cannot identify discovery snapshot",
            )
        })?,
    );
    Ok(DiscoverySnapshot {
        revision: models.revision,
        fingerprint,
        input: ProviderModelDiscoveryInput {
            provider_id: Some(id),
            cli_key: provider.cli_key,
            auth_mode: if provider.auth_mode == "oauth" {
                providers::ProviderAuthMode::Oauth
            } else {
                providers::ProviderAuthMode::ApiKey
            },
            base_urls: provider.base_urls,
            base_url_mode: provider.base_url_mode,
            api_key: (provider.auth_mode == "api_key").then_some(key),
            source_provider_id: provider.source_provider_id,
            bridge_type: provider.bridge_type,
        },
        configured,
        routing: provider.model_routing_policy_override,
    })
}

fn merge_suggestions(
    discovery: &ProviderModelDiscoveryResult,
    configured: Vec<ModelCapabilitySuggestion>,
) -> Vec<ModelCapabilitySuggestion> {
    let mut result: BTreeMap<String, ModelCapabilitySuggestion> = BTreeMap::new();
    if let ProviderModelDiscoveryResult::Ready {
        models, details, ..
    } = discovery
    {
        for id in models {
            result.insert(
                id.clone(),
                ModelCapabilitySuggestion {
                    model_id: id.clone(),
                    sources: vec!["upstream".into()],
                    ..Default::default()
                },
            );
        }
        for model in details {
            if result.contains_key(&model.model_id) {
                result.insert(model.model_id.clone(), model.clone());
            }
        }
    }
    for saved in configured {
        let item =
            result
                .entry(saved.model_id.clone())
                .or_insert_with(|| ModelCapabilitySuggestion {
                    model_id: saved.model_id.clone(),
                    ..Default::default()
                });
        // The explicit provider configuration takes precedence over network metadata.
        if saved.context_window.is_some() {
            item.context_window = saved.context_window;
        }
        if saved.reasoning_efforts.is_some() {
            item.reasoning = saved.reasoning;
            item.reasoning_efforts = saved.reasoning_efforts;
            item.default_reasoning_effort = saved.default_reasoning_effort;
        }
        item.sources.extend(saved.sources);
        if item
            .context_window
            .zip(item.max_tokens)
            .is_some_and(|(context, max)| max > context)
        {
            item.max_tokens = None;
        }
    }
    result.into_values().collect()
}

fn guard_routed_suggestions(
    models: &mut [ModelCapabilitySuggestion],
    global: &crate::settings::ModelRoutingPolicy,
    provider: Option<&crate::settings::ModelRoutingPolicy>,
) {
    for model in models {
        if crate::gateway::discovery_requires_model_confirmation(&model.model_id, global, provider)
        {
            *model = ModelCapabilitySuggestion {
                model_id: model.model_id.clone(),
                sources: vec!["routing_confirmation".into()],
                ..Default::default()
            };
        }
    }
}

pub(crate) async fn native_channel_models_discover(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
) -> Result<ChannelModelDiscovery, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let (target, snapshot, global_routing) = blocking::run("native_channel_discovery_snapshot", {
        let app = app.clone();
        let db = db.clone();
        let id = target_id.clone();
        let uuid = provider_uuid.clone();
        move || {
            let target = super::native_cli_service::resolve_target(&app, &id)?;
            let snapshot = crate::infra::native_cli::with_target_lock(&target, |_| {
                super::native_cli_service::verify_selected_target(&app, &target)?;
                let conn = db.open_connection()?;
                let tx = conn
                    .unchecked_transaction()
                    .map_err(|e| crate::shared::error::db_err!("discovery snapshot: {e}"))?;
                read_snapshot(&tx, provider_id, &uuid, target.client.as_str(), protocol)
            })?;
            let global = crate::settings::read(&app)?.model_routing_policy;
            Ok::<_, AppError>((target, snapshot, global))
        }
    })
    .await
    .map_err(String::from)?;
    // No target lock or database transaction is held across a network request.
    let consumer = target.client.as_str().to_owned();
    let source = snapshot.input.cli_key.clone();
    let oauth = snapshot.input.auth_mode == providers::ProviderAuthMode::Oauth;
    let discovery = provider_model_discovery::provider_models_discover(
        app.clone(),
        db_state.inner(),
        snapshot.input,
    )
    .await?;
    let expected = snapshot.fingerprint;
    blocking::run("native_channel_discovery_verify", {
        let uuid = provider_uuid.clone();
        let global_routing = global_routing.clone();
        move || {
            crate::infra::native_cli::with_target_lock(&target, |_| {
                super::native_cli_service::verify_selected_target(&app, &target)?;
                let conn = db.open_connection()?;
                let tx = conn
                    .unchecked_transaction()
                    .map_err(|e| crate::shared::error::db_err!("discovery snapshot: {e}"))?;
                if read_snapshot(&tx, provider_id, &uuid, target.client.as_str(), protocol)?
                    .fingerprint
                    != expected
                    || crate::settings::read(&app)?.model_routing_policy != global_routing
                {
                    return Err(AppError::new(
                        "NATIVE_GATEWAY_MODELS_CONFLICT",
                        "Source changed during discovery; refresh suggestions",
                    ));
                }
                Ok(())
            })
        }
    })
    .await
    .map_err(String::from)?;
    let mut models = merge_suggestions(&discovery, snapshot.configured);
    super::native_model_defaults::fill_catalog_defaults(
        &mut models,
        &consumer,
        &source,
        oauth,
        protocol,
    );
    guard_routed_suggestions(&mut models, &global_routing, snapshot.routing.as_ref());
    Ok(ChannelModelDiscovery {
        target_id,
        provider_id,
        provider_uuid,
        protocol,
        revision: snapshot.revision,
        models,
        discovery,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_rewrites_do_not_reuse_another_models_capabilities() {
        let global: crate::settings::ModelRoutingPolicy = serde_json::from_value(serde_json::json!({"enabled":true,"rules":[{"source_model":"alias-*","target_model":"upstream-*"},{"source_model":"fixed","reasoning_effort":"high"},{"source_model":"same","target_model":"same"}]})).unwrap();
        let model = |id: &str| ModelCapabilitySuggestion {
            model_id: id.into(),
            context_window: Some(64000),
            reasoning: Some(true),
            ..Default::default()
        };
        let mut models = vec![
            model("alias-a"),
            model("fixed"),
            model("same"),
            model("untouched"),
        ];
        guard_routed_suggestions(&mut models, &global, None);
        assert_eq!(models[0].context_window, None);
        assert_eq!(models[1].reasoning, None);
        assert_eq!(models[2].context_window, Some(64000));
        assert_eq!(models[3].context_window, Some(64000));
        let mut models = vec![model("alias-a")];
        guard_routed_suggestions(
            &mut models,
            &global,
            Some(&crate::settings::ModelRoutingPolicy::default()),
        );
        assert_eq!(models[0].context_window, Some(64000));
    }
    fn total_changes(conn: &Connection) -> u64 {
        conn.query_row("SELECT total_changes()", [], |row| row.get(0))
            .unwrap()
    }
    fn fixture() -> (tempfile::TempDir, crate::db::Db, i64) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::init_for_tests(&dir.path().join("aio.db")).unwrap();
        let conn = db.open_connection().unwrap();
        conn.execute("INSERT INTO providers(provider_uuid,cli_key,name,base_url,base_urls_json,api_key_plaintext,enabled,auth_mode,created_at,updated_at) VALUES ('a7ce9abf-15fd-4137-9fdd-668eddf598cf','codex','Source','https://example.test','[\"https://example.test\"]','test-secret',1,'api_key',1,1)", []).unwrap();
        let id = conn.last_insert_rowid();
        (dir, db, id)
    }
    #[test]
    fn snapshot_is_read_only_and_binds_url_credential_identity_and_revision() {
        let (_dir, db, id) = fixture();
        let conn = db.open_connection().unwrap();
        let changes = total_changes(&conn);
        let initial = read_snapshot(
            &conn,
            id,
            "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
            "pi",
            GatewayProtocol::OpenaiResponses,
        )
        .unwrap();
        assert_eq!(total_changes(&conn), changes);
        assert_eq!(initial.input.api_key.as_deref(), Some("test-secret"));
        assert_eq!(initial.input.base_urls, ["https://example.test"]);
        assert!(!initial.revision.contains("test-secret"));
        assert!(!initial.fingerprint.contains("test-secret"));
        assert!(read_snapshot(
            &conn,
            id,
            "replaced-uuid",
            "pi",
            GatewayProtocol::OpenaiResponses
        )
        .is_err());
        assert!(read_snapshot(
            &conn,
            id,
            "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
            "pi",
            GatewayProtocol::AnthropicMessages
        )
        .is_err());
        conn.execute(
            "UPDATE providers SET api_key_plaintext='rotated' WHERE id=?1",
            [id],
        )
        .unwrap();
        assert_ne!(
            read_snapshot(
                &conn,
                id,
                "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
                "pi",
                GatewayProtocol::OpenaiResponses
            )
            .unwrap()
            .fingerprint,
            initial.fingerprint
        );
    }
    #[test]
    fn configured_suggestions_exclude_stale_rows_and_preserve_unknowns() {
        let (_dir, db, id) = fixture();
        let conn = db.open_connection().unwrap();
        for (uuid, name, stale) in [
            ("13d6ec02-44f0-443d-b7fa-808fe50b9271", "configured", 0),
            ("389eddfb-0aa4-4c2e-b2f2-f0a093ec5154", "stale", 1),
        ] {
            conn.execute("INSERT INTO provider_models(model_uuid,provider_id,remote_model_id,source,stale,created_at,updated_at,capabilities_configured,supported_reasoning_efforts_json,default_reasoning_effort,context_window) VALUES (?1,?2,?3,'manual',?4,1,1,1,'[\"low\",\"high\"]','high',64000)",rusqlite::params![uuid,id,name,stale]).unwrap();
        }
        let changes = total_changes(&conn);
        let snapshot = read_snapshot(
            &conn,
            id,
            "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
            "omp",
            GatewayProtocol::OpenaiResponses,
        )
        .unwrap();
        assert_eq!(total_changes(&conn), changes);
        assert_eq!(snapshot.configured.len(), 1);
        let model = &snapshot.configured[0];
        assert_eq!(model.model_id, "configured");
        assert_eq!(model.context_window, Some(64000));
        assert_eq!(model.default_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(model.supports_tools, None);
        assert_eq!(model.max_tokens, None);
        let fingerprint = snapshot.fingerprint;
        conn.execute(
            "UPDATE provider_models SET context_window=128000 WHERE model_uuid='13d6ec02-44f0-443d-b7fa-808fe50b9271'",
            [],
        )
        .unwrap();
        assert_ne!(
            read_snapshot(
                &conn,
                id,
                "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
                "omp",
                GatewayProtocol::OpenaiResponses
            )
            .unwrap()
            .fingerprint,
            fingerprint
        );
    }
    #[test]
    fn discovery_cannot_bypass_gemini_oauth_eligibility() {
        let (_dir, db, id) = fixture();
        let conn = db.open_connection().unwrap();
        conn.execute("UPDATE providers SET cli_key='gemini',auth_mode='oauth',oauth_access_token='fixture-token' WHERE id=?1",[id]).unwrap();
        let error = read_snapshot(
            &conn,
            id,
            "a7ce9abf-15fd-4137-9fdd-668eddf598cf",
            "omp",
            GatewayProtocol::GoogleGenerativeAi,
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "NATIVE_CHANNEL_SOURCE_BLOCKED");
    }
    #[test]
    fn configured_values_win_but_do_not_invent_missing_fields() {
        let remote = ModelCapabilitySuggestion {
            model_id: "a".into(),
            context_window: Some(2000),
            max_tokens: Some(1500),
            supports_tools: Some(true),
            ..Default::default()
        };
        let discovery = ProviderModelDiscoveryResult::Ready {
            models: vec!["a".into()],
            details: vec![remote],
            origin: "https://example.test".into(),
            base_url_index: None,
        };
        let configured = ModelCapabilitySuggestion {
            model_id: "a".into(),
            context_window: Some(1024),
            ..Default::default()
        };
        let merged = merge_suggestions(&discovery, vec![configured]);
        assert_eq!(merged[0].context_window, Some(1024));
        assert_eq!(merged[0].max_tokens, None);
        assert_eq!(merged[0].reasoning, None);
        assert_eq!(merged[0].supports_tools, Some(true));
    }
}
