//! AIO gateway publication over the native worker's single file/CAS boundary.
use crate::app_state::{ensure_db_ready, DbInitState};
use crate::domain::native_channels::{
    self as channels, ChannelBindingSummary, ChannelLifecycleInput,
};
use crate::domain::native_cli::NativeTarget;
use crate::domain::native_gateway::*;
use crate::infra::native_cli::{self, node_digest, NativeNodePatch, NativeTargetSession};
use crate::shared::error::{AppError, AppResult};
use crate::{blocking, db, settings};
use rusqlite::{Connection, TransactionBehavior};
use std::collections::BTreeMap;

fn state_error(_: impl std::fmt::Display) -> AppError {
    AppError::new(
        "NATIVE_GATEWAY_DB_ERROR",
        "Cannot persist generated-entry state",
    )
}

pub(super) fn context(app: &tauri::AppHandle) -> AppResult<(Option<String>, String)> {
    let status = super::gateway_runtime_access::app_gateway_status(app);
    let origin = if status.running {
        status.base_url
    } else {
        None
    };
    let policy =
        serde_json::to_string(&settings::read(app)?.model_routing_policy).map_err(state_error)?;
    Ok((origin, policy))
}

#[cfg(test)]
pub(crate) fn preview_for_target(
    db: &db::Db,
    target: &NativeTarget,
    origin: Option<&str>,
    policy: &str,
) -> AppResult<GatewayCatalogPreview> {
    preview_with_guard(db, target, origin, policy, || Ok(()))
}

fn preview_with_guard(
    db: &db::Db,
    target: &NativeTarget,
    origin: Option<&str>,
    policy: &str,
    check_target: impl FnOnce() -> AppResult<()>,
) -> AppResult<GatewayCatalogPreview> {
    native_cli::with_target_lock(target, |session| {
        check_target()?;
        let snapshot = session.snapshot()?;
        let conn = db.open_connection()?;
        let (catalog_revision, groups) = catalog(&conn, target.client.as_str(), origin, policy)?;
        let entries = origin
            .map(|url| generate_entries(target.client.as_str(), url, &groups))
            .transpose()?
            .unwrap_or_default();
        let manifests = manifest_summaries(
            &list_manifests(&conn, &target.target_id)?,
            snapshot.providers(),
            &catalog_revision,
        );
        Ok(GatewayCatalogPreview {
            target_id: target.target_id.clone(),
            revision: snapshot.revision,
            catalog_revision,
            listener_ready: origin.is_some(),
            groups,
            entries,
            manifests,
        })
    })
}

/// Reconcile an interrupted durable intent using exact node digests, never filenames
/// or prefixes. A foreign edit remains a conflict and is never replaced.
fn reconcile(
    conn: &Connection,
    session: &NativeTargetSession<'_>,
    target: &NativeTarget,
    channel_scope: bool,
) -> AppResult<bool> {
    let snapshot = session.snapshot()?;
    let mut changed = false;
    for manifest in owned_manifests(conn, &target.target_id, channel_scope)? {
        if manifest.cli_key != target.client.as_str() {
            return Err(state_error("target client mismatch"));
        }
        if manifest.state == "applied" {
            continue;
        }
        let actual = snapshot
            .providers()
            .get(&manifest.native_key)
            .map(node_digest);
        let desired = manifest
            .payload
            .desired
            .as_ref()
            .map(|e| node_digest(&e.node));
        let previous = manifest
            .payload
            .previous
            .as_ref()
            .map(|v| node_digest(&v.entry.node));
        if actual == desired {
            if let Some(entry) = manifest.payload.desired {
                put_manifest(
                    conn,
                    &Manifest::applied_owned(
                        &target.target_id,
                        target.client.as_str(),
                        ManifestVersion {
                            entry,
                            catalog_revision: manifest.catalog_revision,
                            generation: manifest.generation,
                            policy_hash: manifest.payload.policy_hash,
                        },
                        manifest.channel.clone(),
                    ),
                )?;
            } else {
                delete_owned_manifest(conn, &manifest)?;
            }
        } else if actual == previous {
            if let Some(version) = manifest.payload.previous {
                put_manifest(
                    conn,
                    &Manifest::applied_owned(
                        &target.target_id,
                        target.client.as_str(),
                        version,
                        manifest.channel.clone(),
                    ),
                )?;
            } else {
                delete_owned_manifest(conn, &manifest)?;
            }
        } else {
            return Err(AppError::new("NATIVE_GATEWAY_RECOVERY_CONFLICT","Pending generated node was externally changed; restore its recorded node before retrying"));
        }
        changed = true;
    }
    Ok(changed)
}

/// Publication is prepare-intent -> node-CAS -> finalize-manifest. If persistence
/// fails, compensate only owned nodes; an interrupted intent remains recoverable.
#[cfg(test)]
pub(crate) fn mutate_for_target(
    db: &db::Db,
    target: &NativeTarget,
    input: &GatewayLifecycleInput,
    origin: Option<&str>,
    policy: &str,
    remove: bool,
) -> AppResult<GatewayMutationResult> {
    mutate_with_guard(db, target, input, origin, policy, remove, || Ok(()))
}

fn mutate_with_guard(
    db: &db::Db,
    target: &NativeTarget,
    input: &GatewayLifecycleInput,
    origin: Option<&str>,
    policy: &str,
    remove: bool,
    check_runtime: impl Fn() -> AppResult<()>,
) -> AppResult<GatewayMutationResult> {
    mutate_owned_with_guard(
        db,
        target,
        input,
        origin,
        policy,
        remove,
        None,
        check_runtime,
    )
    .map(|r| r.gateway)
}

pub(super) struct OwnedMutationResult {
    pub gateway: GatewayMutationResult,
    pub bindings: Vec<ChannelBindingSummary>,
}
fn owned_manifests(
    conn: &Connection,
    target_id: &str,
    channel_scope: bool,
) -> AppResult<Vec<Manifest>> {
    if channel_scope {
        channels::list_bindings(conn, target_id)
    } else {
        list_manifests(conn, target_id)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn mutate_owned_with_guard(
    db: &db::Db,
    target: &NativeTarget,
    input: &GatewayLifecycleInput,
    origin: Option<&str>,
    policy: &str,
    remove: bool,
    channel_input: Option<&ChannelLifecycleInput>,
    check_runtime: impl Fn() -> AppResult<()>,
) -> AppResult<OwnedMutationResult> {
    let channel_scope = channel_input.is_some();
    if input.target_id != target.target_id {
        return Err(AppError::new(
            "NATIVE_TARGET_NOT_FOUND",
            "Target identity changed",
        ));
    }
    if !remove && origin.is_none() {
        return Err(AppError::new(
            "NATIVE_GATEWAY_NOT_READY",
            "Start the AIO listener before applying generated entries",
        ));
    }
    native_cli::with_target_lock(target, |session| {
        check_runtime()?;
        let snapshot = session.snapshot()?;
        if snapshot.revision != input.expected_revision {
            return Err(AppError::new(
                "NATIVE_REVISION_CONFLICT",
                "Reload the native configuration before applying",
            ));
        }
        if !target.writable {
            return Err(AppError::new(
                "NATIVE_TARGET_READ_ONLY",
                "This native target is read-only",
            ));
        }
        let mut conn = db.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(state_error)?;
        let recovered = reconcile(&tx, session, target, channel_scope)?;
        let (revision, previous, desired) = if let Some(channel_input) = channel_input {
            let plan =
                channels::channel_plan(&tx, target.client.as_str(), channel_input, origin, policy)?;
            (
                plan.revision,
                plan.previous,
                plan.desired
                    .into_iter()
                    .map(|(owner, entry)| (Some(owner), entry))
                    .collect::<Vec<_>>(),
            )
        } else {
            // Withdrawal remains available when the provider catalog is invalid.
            let (revision, groups) = if remove {
                (input.catalog_revision.clone(), Vec::new())
            } else {
                catalog(&tx, target.client.as_str(), origin, policy)?
            };
            if !remove && revision != input.catalog_revision {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_CATALOG_CONFLICT",
                    "Provider routing or capabilities changed; preview again",
                ));
            }
            let previous = list_manifests(&tx, &target.target_id)?;
            let desired = if remove {
                Vec::new()
            } else {
                validate_publication(
                    &tx,
                    &target.target_id,
                    target.client.as_str(),
                    &groups,
                    policy,
                )?;
                generate_entries(
                    target.client.as_str(),
                    origin.expect("checked listener"),
                    &groups,
                )?
                .into_iter()
                .map(|entry| (None, entry))
                .collect()
            };
            (revision, previous, desired)
        };
        let mut targets: BTreeMap<_, _> = previous
            .iter()
            .map(|m| (m.native_key.clone(), (m.channel.clone(), None)))
            .collect();
        for (owner, entry) in desired {
            targets.insert(entry.native_key.clone(), (owner, Some(entry)));
        }
        let mut intents = Vec::new();
        let mut patches = Vec::new();
        for (key, (channel, entry)) in targets {
            let old = previous.iter().find(|m| m.native_key == key);
            let protocol = entry
                .as_ref()
                .map(|e| e.protocol)
                .or_else(|| old.map(|m| m.protocol))
                .expect("publication protocol");
            let expected = old.map(|m| m.node_digest.clone());
            let actual = snapshot.providers().get(&key).map(node_digest);
            if actual != expected {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_NODE_CONFLICT",
                    "Generated entry was modified, removed, or its exact name is already in use",
                ));
            }
            let next_digest = entry.as_ref().map(|e| node_digest(&e.node));
            if actual == next_digest && old.is_some_and(|m| m.catalog_revision == revision) {
                continue;
            }
            let generation = old.map_or(1, |m| m.generation.saturating_add(1));
            let intent = Manifest {
                channel,
                target_id: target.target_id.clone(),
                cli_key: target.client.as_str().into(),
                protocol,
                native_key: key.clone(),
                node_digest: next_digest
                    .clone()
                    .or_else(|| expected.clone())
                    .unwrap_or_default(),
                catalog_revision: revision.clone(),
                generation,
                state: if entry.is_some() {
                    "intent_apply"
                } else {
                    "intent_remove"
                }
                .into(),
                payload: ManifestPayload {
                    desired: entry.clone(),
                    previous: old.map(Manifest::version).transpose()?,
                    policy_hash: hash(policy),
                },
            };
            put_manifest(&tx, &intent)?;
            patches.push(NativeNodePatch {
                native_key: key,
                expected_digest: expected,
                replacement: entry.map(|e| e.node),
            });
            intents.push(intent);
        }
        tx.commit().map_err(state_error)?;
        let receipt =
            match check_runtime().and_then(|_| session.patch(&snapshot.revision, &patches)) {
                Ok(receipt) => receipt,
                Err(error) => {
                    // If nothing changed, this rolls back intents; if a process/file
                    // race changed a node, retain the intent as explicit recovery state.
                    if let Ok(tx) = conn.transaction_with_behavior(TransactionBehavior::Immediate) {
                        if reconcile(&tx, session, target, channel_scope).is_ok() {
                            let _ = tx.commit();
                        }
                    }
                    return Err(error);
                }
            };
        let finish = (|| {
            check_runtime()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(state_error)?;
            let fresh_revision = if remove {
                revision.clone()
            } else if channel_scope {
                channels::channel_catalog(&tx, target.client.as_str(), origin, policy)?.0
            } else {
                catalog(&tx, target.client.as_str(), origin, policy)?.0
            };
            if fresh_revision != revision {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_CATALOG_CONFLICT",
                    "Catalog changed during publication; generated nodes were rolled back",
                ));
            }
            reconcile(&tx, session, target, channel_scope)?;
            tx.commit().map_err(state_error)
        })();
        if let Err(error) = finish {
            if session.compensate(&receipt).is_err() {
                return Err(AppError::new("NATIVE_GATEWAY_RECOVERY_REQUIRED","Persistence failed and a later native edit prevents compensation; pending manifest was retained"));
            }
            if let Ok(tx) = conn.transaction_with_behavior(TransactionBehavior::Immediate) {
                if reconcile(&tx, session, target, channel_scope).is_ok() {
                    let _ = tx.commit();
                }
            }
            return Err(error);
        }
        let final_snapshot = session.snapshot()?;
        let manifests = manifest_summaries(
            &list_manifests(&conn, &target.target_id)?,
            final_snapshot.providers(),
            &revision,
        );
        let bindings = if channel_scope {
            channels::binding_summaries(
                &channels::list_bindings(&conn, &target.target_id)?,
                final_snapshot.providers(),
                &revision,
            )
        } else {
            Vec::new()
        };
        Ok(OwnedMutationResult {
            bindings,
            gateway: GatewayMutationResult {
                target_id: target.target_id.clone(),
                revision: final_snapshot.revision,
                changed: receipt.changed || recovered || !intents.is_empty(),
                backup_path: receipt.backup_path,
                manifests,
            },
        })
    })
}

pub(crate) async fn native_gateway_models_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
) -> Result<GatewayModelsSnapshot, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("native_gateway_models_get", move || {
        models_get(&db, provider_id, &provider_uuid)
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_gateway_models_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
    expected_revision: String,
    models: Vec<NativeModelSpec>,
) -> Result<GatewayModelsSnapshot, String> {
    let db = ensure_db_ready(app, db_state.inner()).await?;
    blocking::run("native_gateway_models_set", move || {
        models_set(&db, provider_id, &provider_uuid, &expected_revision, models)
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_gateway_catalog_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<GatewayCatalogPreview, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_gateway_catalog_preview", move || {
        let (origin, policy) = context(&app)?;
        let target = super::native_cli_service::resolve_target(&app, &target_id)?;
        preview_with_guard(&db, &target, origin.as_deref(), &policy, || {
            super::native_cli_service::verify_selected_target(&app, &target)
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_gateway_mutate(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: GatewayLifecycleInput,
    remove: bool,
) -> Result<GatewayMutationResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_gateway_mutate", move || {
        let (origin, policy) = context(&app)?;
        let target = super::native_cli_service::resolve_target(&app, &input.target_id)?;
        mutate_with_guard(
            &db,
            &target,
            &input,
            origin.as_deref(),
            &policy,
            remove,
            || {
                super::native_cli_service::verify_selected_target(&app, &target)?;
                if remove {
                    return Ok(());
                }
                let current = context(&app)?;
                if current.0 != origin || current.1 != policy {
                    return Err(AppError::new(
                        "NATIVE_GATEWAY_CATALOG_CONFLICT",
                        "Listener or global model policy changed during publication; preview again",
                    ));
                }
                Ok(())
            },
        )
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_gateway_import_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    native_key: String,
) -> Result<GatewayImportPreview, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_gateway_import_preview", move || {
        let target = super::native_cli_service::resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |session| {
            super::native_cli_service::verify_selected_target(&app, &target)?;
            if managed_native_keys(&db, &target_id)?.contains(&native_key) {
                return Err(AppError::new(
                    "NATIVE_MANAGED_NODE",
                    "Generated AIO entries cannot become upstreams",
                ));
            }
            let snapshot = session.snapshot()?;
            let node = snapshot.providers().get(&native_key).ok_or_else(|| {
                AppError::new(
                    "NATIVE_PROVIDER_NOT_FOUND",
                    "Native provider is not currently present",
                )
            })?;
            preview_import(
                target.client.as_str(),
                &target_id,
                &native_key,
                &snapshot.revision,
                node,
            )
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_gateway_import_confirm(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: GatewayImportConfirmInput,
) -> Result<Vec<crate::providers::ProviderSummary>, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_gateway_import_confirm", move || {
        let target = super::native_cli_service::resolve_target(&app, &input.target_id)?;
        native_cli::with_target_lock(&target, |session| {
            super::native_cli_service::verify_selected_target(&app, &target)?;
            if managed_native_keys(&db, &target.target_id)?.contains(&input.native_key) {
                return Err(AppError::new(
                    "NATIVE_MANAGED_NODE",
                    "Generated AIO entries cannot become upstreams",
                ));
            }
            let snapshot = session.snapshot()?;
            if snapshot.revision != input.expected_revision {
                return Err(AppError::new(
                    "NATIVE_REVISION_CONFLICT",
                    "Native source changed; preview import again",
                ));
            }
            let node = snapshot.providers().get(&input.native_key).ok_or_else(|| {
                AppError::new(
                    "NATIVE_PROVIDER_NOT_FOUND",
                    "Native provider is not currently present",
                )
            })?;
            let preview = preview_import(
                target.client.as_str(),
                &target.target_id,
                &input.native_key,
                &snapshot.revision,
                node,
            )?;
            import_confirm(&db, target.client.as_str(), &preview, &input)
        })
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
#[path = "native_gateway_service_tests.rs"]
mod tests;
